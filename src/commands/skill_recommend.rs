use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::cli::{ActiveRecommendArgs, IndexArgs, SkillSearchArgs};
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::gitops;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::state_model::{REGISTRY_SCHEMA_VERSION, RegistryStatePaths};
use crate::types::ErrorCode;

use super::helpers::map_git;
use super::helpers::{map_io, map_registry_state, validate_non_empty, validate_skill_name};
use super::skill_inventory::{SkillDiscoveryFilters, score_and_filter_skills, tokenize};
use super::skill_recommend_active::{activation_plan_delta, active_view};
use super::skill_safety::evaluate_skill_safety_with_policy;
use super::{App, CommandFailure, build_skill_read_model};

const INDEX_DIR_REL: &str = "state/index";
const LEXICAL_FILE: &str = "skills.lexical.json";
const CAPABILITY_FILE: &str = "skills.capabilities.json";
const WORKSPACES_FILE: &str = "workspaces.json";

impl App {
    pub fn cmd_index_build(
        &self,
        args: &IndexArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        if args.provider != "none" && args.provider != "local" {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("bad index provider '{}'", args.provider),
            ));
        }
        let model = build_skill_read_model(&self.ctx).map_err(map_registry_state)?;
        let skillsets = load_skillsets_value(&self.ctx)?;
        let index_dir = self.ctx.root.join(INDEX_DIR_REL);
        fs::create_dir_all(&index_dir).map_err(map_io)?;
        ensure_index_git_exclude(&self.ctx)?;

        let lexical = lexical_index_payload(&model.skills);
        let capabilities = capability_index_payload(&model.skills, &skillsets);
        let workspaces = workspace_index_payload(&self.ctx)?;

        write_index_file(&index_dir.join(LEXICAL_FILE), &lexical)?;
        write_index_file(&index_dir.join(CAPABILITY_FILE), &capabilities)?;
        write_index_file(&index_dir.join(WORKSPACES_FILE), &workspaces)?;

        let mut warnings = model.warnings;
        if args.provider == "local" && !args.no_embeddings {
            warnings.push("no embeddings written".to_string());
        }

        Ok((
            json!({
                "index_dir": index_dir,
                "provider": args.provider,
                "embeddings": {
                    "enabled": false,
                    "reason": if args.no_embeddings { "disabled" } else { "no local provider" },
                },
                "files": {
                    "lexical": index_dir.join(LEXICAL_FILE),
                    "capabilities": index_dir.join(CAPABILITY_FILE),
                    "workspaces": index_dir.join(WORKSPACES_FILE),
                },
                "counts": {
                    "skills": lexical["records"].as_array().map_or(0, Vec::len),
                    "capabilities": capabilities["records"].as_array().map_or(0, Vec::len),
                    "workspaces": workspaces["records"].as_array().map_or(0, Vec::len),
                },
                "derived": true,
                "network_required": false,
            }),
            Meta {
                warnings,
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_index_status(&self) -> std::result::Result<(Value, Meta), CommandFailure> {
        let index_dir = self.ctx.root.join(INDEX_DIR_REL);
        let files = [
            ("lexical", LEXICAL_FILE),
            ("capabilities", CAPABILITY_FILE),
            ("workspaces", WORKSPACES_FILE),
        ];
        let mut status = BTreeMap::new();
        let mut ready = true;
        for (name, file) in files {
            let path = index_dir.join(file);
            let exists = path.is_file();
            ready &= exists;
            status.insert(
                name,
                json!({
                    "path": path,
                    "exists": exists,
                    "records": if exists { count_index_records(&path)? } else { 0 },
                }),
            );
        }
        Ok((
            json!({
                "index_dir": index_dir,
                "ready": ready,
                "derived": true,
                "files": status,
                "next_actions": if ready { Vec::<String>::new() } else { vec!["loom index build --no-embeddings".to_string()] },
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_skill_search(
        &self,
        args: &SkillSearchArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_non_empty("query", args.query.trim())?;
        let mut warnings = Vec::new();
        let mode = if args.semantic {
            warnings.push("semantic provider not configured".to_string());
            "semantic-disabled"
        } else {
            "lexical"
        };
        let model = build_skill_read_model(&self.ctx).map_err(map_registry_state)?;
        let mut results = score_and_filter_skills(
            &model.skills,
            &args.query,
            SkillDiscoveryFilters {
                agent: args.agent.as_deref(),
                profile: args.profile.as_deref(),
                status: args.status.as_deref(),
                trust: args.trust.as_deref(),
                workspace: args.workspace.as_deref(),
            },
            args.for_task || args.workspace.is_some(),
        );
        if args.active {
            results.retain(|result| {
                result["skill"]["projection_summary"]["count"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0
            });
        }
        warnings.extend(model.warnings);
        let selected = results.first().cloned();
        let candidates = results.clone();
        let mut payload = json!({
            "query": args.query,
            "mode": mode,
            "for_task": args.for_task,
            "filters": {
                "agent": args.agent,
                "profile": args.profile,
                "status": args.status,
                "trust": args.trust,
                "workspace": args.workspace.as_ref().map(|path| path.display().to_string()),
                "active": args.active,
            },
            "count": results.len(),
            "results": results,
        });
        if args.for_task {
            payload["task_description"] = json!(args.query);
            payload["strategy"] = json!({
                "type": if args.semantic {
                    "semantic_disabled_lexical"
                } else {
                    "deterministic_lexical"
                },
                "mode": mode,
                "llm_invoked": false,
                "tie_break": "score_desc_then_skill_id_asc",
            });
            payload["selected"] = selected.unwrap_or(Value::Null);
            payload["candidates"] = json!(candidates);
        }
        if args.explain {
            let skillsets = load_skillsets_value(&self.ctx)?;
            let recommendation_skill_results = score_and_filter_skills(
                &model.skills,
                &args.query,
                SkillDiscoveryFilters {
                    agent: None,
                    profile: None,
                    status: None,
                    trust: None,
                    workspace: args.workspace.as_deref(),
                },
                true,
            );
            let recommendations = recommendation_results(
                &self.ctx,
                &args.query,
                args.agent.as_deref(),
                args.workspace.as_deref(),
                mode,
                &recommendation_skill_results,
                &skillsets,
            )?;
            payload["recommendations"] = json!({
                "task_description": args.query,
                "mode": mode,
                "filters": {
                    "agent": args.agent,
                    "workspace": args.workspace.as_ref().map(|path| path.display().to_string()),
                },
                "count": recommendations.len(),
                "results": recommendations,
            });
            payload["explain"] = json!({
                "score_inputs": true,
                "skillsets": true,
                "safety_risks": true,
            });
        }
        Ok((
            payload,
            Meta {
                warnings,
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_skill_recommend(
        &self,
        args: &SkillSearchArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let mut args = args.clone();
        args.for_task = true;
        args.explain = true;
        self.cmd_skill_search(&args)
    }

    pub fn cmd_skill_resolve(
        &self,
        args: &SkillSearchArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let mut args = args.clone();
        args.for_task = true;
        self.cmd_skill_search(&args)
    }

    pub fn cmd_active_recommend(
        &self,
        args: &ActiveRecommendArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_non_empty("task_description", args.task_description.trim())?;
        validate_non_empty("agent", args.agent.trim())?;
        validate_active_agent(args.agent.trim())?;
        for skill in &args.desired_skills {
            validate_skill_name(skill).map_err(super::helpers::map_arg)?;
        }
        let model = build_skill_read_model(&self.ctx).map_err(map_registry_state)?;
        let skill_results = score_and_filter_skills(
            &model.skills,
            &args.task_description,
            SkillDiscoveryFilters {
                agent: None,
                profile: None,
                status: None,
                trust: None,
                workspace: args.workspace.as_deref(),
            },
            true,
        );
        let skillsets = load_skillsets_value(&self.ctx)?;
        let recommend = recommendation_results(
            &self.ctx,
            &args.task_description,
            None,
            args.workspace.as_deref(),
            "lexical",
            &skill_results,
            &skillsets,
        )?;
        let mut meta = Meta {
            warnings: model.warnings,
            ..Meta::default()
        };
        let mut desired = args.desired_skills.clone();
        if desired.is_empty() {
            desired.extend(
                recommend
                    .iter()
                    .filter(|result| result["kind"].as_str() == Some("skill"))
                    .filter(|result| result["risks"].as_array().is_none_or(Vec::is_empty))
                    .filter_map(|result| result["id"].as_str().map(str::to_string))
                    .take(3),
            );
        }
        let active_view = active_view(
            &self.ctx,
            &args.agent,
            args.workspace.as_deref(),
            args.binding.as_deref(),
        )?;
        let (add, keep, remove, risks) = activation_plan_delta(
            &self.ctx,
            &desired,
            &args.agent,
            args.workspace.as_deref(),
            &active_view,
        )?;
        if add.is_empty() && keep.is_empty() {
            meta.warnings.push("no activation candidates".to_string());
        }
        Ok((
            json!({
                "agent": args.agent,
                "workspace": args.workspace.as_ref().map(|path| path.display().to_string()),
                "task": args.task_description,
                "binding_id": args.binding,
                "dry_run": true,
                "plan": {
                    "add": add,
                    "keep": keep,
                    "remove": remove,
                },
                "risks": risks,
                "policy": {
                    "allowed": risks.is_empty(),
                    "mode": "dry-run-only",
                },
                "suggested_commands": add.iter().filter_map(|item| item["command"].as_str().map(str::to_string)).collect::<Vec<_>>(),
            }),
            meta,
        ))
    }
}

fn lexical_index_payload(skills: &[Value]) -> Value {
    let records = skills
        .iter()
        .filter_map(|skill| {
            let skill_id = skill["skill_id"].as_str()?;
            let mut fields = BTreeMap::new();
            fields.insert("name", tokenize(skill_id));
            fields.insert(
                "description",
                tokenize(skill["description"].as_str().unwrap_or_default()),
            );
            fields.insert("warnings", tokenized_array(&skill["warnings"]));
            let tokens = fields
                .values()
                .flatten()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            Some(json!({
                "schema_version": REGISTRY_SCHEMA_VERSION,
                "skill_id": skill_id,
                "source_digest": digest_json(skill),
                "tokens": tokens,
                "fields": fields,
                "source_timestamp": skill["latest_updated_at"].clone(),
            }))
        })
        .collect::<Vec<_>>();
    json!({ "schema_version": REGISTRY_SCHEMA_VERSION, "records": records })
}

fn capability_index_payload(skills: &[Value], skillsets: &Value) -> Value {
    let membership = skillset_membership(skillsets);
    let records = skills
        .iter()
        .filter_map(|skill| {
            let skill_id = skill["skill_id"].as_str()?;
            let description = skill["description"].as_str().unwrap_or_default();
            Some(json!({
                "schema_version": REGISTRY_SCHEMA_VERSION,
                "skill_id": skill_id,
                "source_digest": digest_json(skill),
                "capabilities": tokenize(description),
                "triggers": [],
                "domains": [],
                "tools": [],
                "risk": "unknown",
                "trust": skill["trust"].as_str().unwrap_or("unknown"),
                "dependency_status": if skill["source_status"].as_str() == Some("present") { "unknown" } else { "missing-source" },
                "eval": {
                    "trigger_precision": Value::Null,
                    "trigger_recall": Value::Null,
                    "baseline_delta": Value::Null,
                },
                "skillsets": membership.get(skill_id).cloned().unwrap_or_default(),
            }))
        })
        .collect::<Vec<_>>();
    json!({ "schema_version": REGISTRY_SCHEMA_VERSION, "records": records })
}

fn workspace_index_payload(ctx: &AppContext) -> std::result::Result<Value, CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
    let Some(snapshot) = snapshot else {
        return Ok(json!({ "schema_version": REGISTRY_SCHEMA_VERSION, "records": [] }));
    };
    let mut records = snapshot
        .bindings
        .bindings
        .iter()
        .map(|binding| {
            json!({
                "schema_version": REGISTRY_SCHEMA_VERSION,
                "workspace": binding.workspace_matcher.value,
                "agent": binding.agent,
                "binding_id": binding.binding_id,
                "policy_profile": binding.policy_profile,
                "active": binding.active,
                "source_digest": digest_json(&json!(binding)),
            })
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left["workspace"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["workspace"].as_str().unwrap_or_default())
            .then_with(|| {
                left["agent"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["agent"].as_str().unwrap_or_default())
            })
            .then_with(|| {
                left["binding_id"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["binding_id"].as_str().unwrap_or_default())
            })
    });
    Ok(json!({ "schema_version": REGISTRY_SCHEMA_VERSION, "records": records }))
}

fn recommendation_results(
    ctx: &AppContext,
    task: &str,
    agent: Option<&str>,
    workspace: Option<&Path>,
    mode: &str,
    skill_search_results: &[Value],
    skillsets: &Value,
) -> std::result::Result<Vec<Value>, CommandFailure> {
    let mut results = Vec::new();
    for result in skill_search_results {
        if let Some(recommendation) = skill_recommendation(ctx, result, agent, mode)? {
            results.push(recommendation);
        }
    }
    results.extend(skillset_recommendations(
        ctx,
        task,
        agent,
        workspace,
        mode,
        skill_search_results,
        skillsets,
    )?);
    results.sort_by(|left, right| {
        let l = left["score"].as_i64().unwrap_or_default();
        let r = right["score"].as_i64().unwrap_or_default();
        r.cmp(&l)
            .then_with(|| {
                left["kind"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["kind"].as_str().unwrap_or_default())
            })
            .then_with(|| {
                left["id"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["id"].as_str().unwrap_or_default())
            })
    });
    Ok(results)
}

fn skill_recommendation(
    ctx: &AppContext,
    result: &Value,
    agent: Option<&str>,
    mode: &str,
) -> std::result::Result<Option<Value>, CommandFailure> {
    let skill = &result["skill"];
    let Some(skill_id) = skill["skill_id"].as_str() else {
        return Ok(None);
    };
    if skill["quarantined"].as_bool() == Some(true) {
        return Ok(None);
    }
    let mut reasons = vec!["lexical match".to_string()];
    let mut risks = Vec::new();
    let mut warnings = Vec::new();
    if skill["trust"].as_str().unwrap_or("unknown") == "unknown" {
        warnings.push("no trust metadata recorded".to_string());
    }
    if skill["trust"].as_str() == Some("blocked") {
        risks.push("trust blocked".to_string());
    }
    warnings.push("no eval evidence".to_string());
    if skill["source_status"].as_str() != Some("present") {
        risks.push(format!(
            "source {}",
            skill["source_status"].as_str().unwrap_or("unknown")
        ));
    } else if let Some(risk) = activation_safety_risk(ctx, skill_id)? {
        risks.push(risk);
    }
    if !skill["warnings"].as_array().is_none_or(Vec::is_empty) {
        risks.push("inventory warnings".to_string());
    }
    if agent.is_some() {
        reasons.push("agent match".to_string());
    }
    let can_activate = risks.is_empty() && agent.is_some();
    Ok(Some(json!({
        "kind": "skill",
        "id": skill_id,
        "score": result["score"],
        "mode": mode,
        "score_inputs": result["score_inputs"],
        "reasons": reasons,
        "risks": risks,
        "warnings": warnings,
        "recommended_action": if can_activate { "activate" } else { "inspect" },
        "suggested_commands": if can_activate {
            vec![format!("loom --json skill activate {skill_id} --agent {} --dry-run", agent.unwrap())]
        } else {
            vec![format!("loom --json skill inspect {skill_id}")]
        },
    })))
}

fn skillset_recommendations(
    ctx: &AppContext,
    task: &str,
    agent: Option<&str>,
    _workspace: Option<&Path>,
    mode: &str,
    skill_results: &[Value],
    skillsets: &Value,
) -> std::result::Result<Vec<Value>, CommandFailure> {
    let inventory = build_skill_read_model(ctx).map_err(map_registry_state)?;
    let inventory = inventory
        .skills
        .into_iter()
        .filter_map(|skill| {
            skill["skill_id"]
                .as_str()
                .map(|id| (id.to_string(), skill.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let skill_scores = skill_results
        .iter()
        .filter_map(|result| {
            Some((
                result["skill"]["skill_id"].as_str()?.to_string(),
                result["score"].as_i64().unwrap_or_default(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let tokens = tokenize(task);
    let mut out = Vec::new();
    for skillset in skillsets["skillsets"].as_array().into_iter().flatten() {
        let Some(id) = skillset["id"].as_str() else {
            continue;
        };
        let mut score = lexical_score_text(id, &tokens)
            + lexical_score_text(
                skillset["description"].as_str().unwrap_or_default(),
                &tokens,
            );
        let mut risks = Vec::new();
        let mut warnings = Vec::new();
        let mut reasons = Vec::new();
        let mut required_safe = true;
        let mut member_commands = Vec::new();
        for member in skillset["members"].as_array().into_iter().flatten() {
            let Some(skill_id) = member["skill_id"].as_str() else {
                continue;
            };
            let required = member["required"].as_bool().unwrap_or(true);
            let member_score = *skill_scores.get(skill_id).unwrap_or(&0);
            score += member_score / 2;
            if member_score > 0 {
                reasons.push(format!("member '{skill_id}' matched"));
            }
            match inventory.get(skill_id) {
                Some(skill) => {
                    let member_kind = if required { "required" } else { "optional" };
                    if skill["quarantined"].as_bool() == Some(true) {
                        required_safe = false;
                        risks.push(format!("{member_kind} member '{skill_id}' quarantined"));
                    } else if skill["trust"].as_str() == Some("blocked") {
                        required_safe = false;
                        risks.push(format!("{member_kind} member '{skill_id}' trust blocked"));
                    } else if skill["source_status"].as_str() != Some("present") {
                        if required {
                            required_safe = false;
                            risks.push(format!("required member '{skill_id}' source missing"));
                        } else {
                            warnings.push(format!("optional member '{skill_id}' source missing"));
                        }
                    } else if !skill["warnings"].as_array().is_none_or(Vec::is_empty) {
                        required_safe = false;
                        risks.push(format!("{member_kind} member '{skill_id}' warnings"));
                    } else if let Some(risk) = activation_safety_risk(ctx, skill_id)? {
                        required_safe = false;
                        risks.push(format!("{member_kind} member '{skill_id}' {risk}"));
                    } else if let Some(agent) = agent {
                        member_commands.push(format!(
                            "loom --json skill activate {skill_id} --agent {agent} --dry-run"
                        ));
                    }
                }
                None if required => {
                    required_safe = false;
                    risks.push(format!("required member '{skill_id}' missing"));
                }
                None => warnings.push(format!("optional member '{skill_id}' missing")),
            }
        }
        if score == 0 {
            continue;
        }
        if reasons.is_empty() {
            reasons.push("skillset text matched".to_string());
        }
        warnings.push("skillset activation unavailable".to_string());
        let can_activate_members = required_safe && risks.is_empty() && agent.is_some();
        out.push(json!({
            "kind": "skillset",
            "id": id,
            "score": score,
            "mode": mode,
            "score_inputs": {
                "matched_fields": ["skillset", "members"],
            },
            "reasons": reasons,
            "risks": risks,
            "warnings": warnings,
            "recommended_action": if can_activate_members { "activate" } else { "inspect" },
            "suggested_commands": if can_activate_members && !member_commands.is_empty() {
                member_commands
            } else {
                vec![format!("loom --json skillset show {id}")]
            },
        }));
    }
    Ok(out)
}

fn activation_safety_risk(
    ctx: &AppContext,
    skill_id: &str,
) -> std::result::Result<Option<String>, CommandFailure> {
    let evaluation =
        evaluate_skill_safety_with_policy(ctx, skill_id, "activate", false, "safe-capture")?;
    if evaluation.report.activation_allowed {
        Ok(None)
    } else {
        Ok(Some(format!("safety {}", evaluation.report.decision)))
    }
}

fn validate_active_agent(agent: &str) -> std::result::Result<(), CommandFailure> {
    match agent {
        "claude" | "codex" | "cursor" | "windsurf" | "cline" | "copilot" | "aider" | "opencode"
        | "gemini-cli" | "goose" => Ok(()),
        _ => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("unsupported agent '{agent}'"),
        )),
    }
}

fn lexical_score_text(value: &str, tokens: &[String]) -> i64 {
    let value = value.to_ascii_lowercase();
    tokens
        .iter()
        .filter(|token| value.contains(token.as_str()))
        .count() as i64
        * 4
}

fn tokenized_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|item| tokenize(item.as_str().unwrap_or_default()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn skillset_membership(skillsets: &Value) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for skillset in skillsets["skillsets"].as_array().into_iter().flatten() {
        let Some(skillset_id) = skillset["id"].as_str() else {
            continue;
        };
        for member in skillset["members"].as_array().into_iter().flatten() {
            if let Some(skill_id) = member["skill_id"].as_str() {
                out.entry(skill_id.to_string())
                    .or_default()
                    .insert(skillset_id.to_string());
            }
        }
    }
    out.into_iter()
        .map(|(skill, sets)| (skill, sets.into_iter().collect()))
        .collect()
}

fn load_skillsets_value(ctx: &AppContext) -> std::result::Result<Value, CommandFailure> {
    let path = ctx.root.join("state/registry/skillsets.json");
    if !path.exists() {
        return Ok(json!({ "schema_version": REGISTRY_SCHEMA_VERSION, "skillsets": [] }));
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to parse {}: {}", path.display(), err),
        )
    })?;
    if parsed["schema_version"].as_u64() != Some(REGISTRY_SCHEMA_VERSION as u64) {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "{} schema {} unsupported",
                path.display(),
                parsed["schema_version"]
            ),
        ));
    }
    Ok(parsed)
}

fn write_index_file(path: &Path, payload: &Value) -> std::result::Result<(), CommandFailure> {
    let raw = serde_json::to_string_pretty(payload).map_err(map_io)? + "\n";
    write_atomic(path, &raw).map_err(map_io)
}

fn ensure_index_git_exclude(ctx: &AppContext) -> std::result::Result<(), CommandFailure> {
    if !gitops::repo_is_initialized(ctx).map_err(map_git)? {
        return Ok(());
    }
    let output = gitops::run_git_allow_failure(ctx, &["rev-parse", "--git-path", "info/exclude"])
        .map_err(map_git)?;
    if !output.status.success() {
        return Ok(());
    }
    let rel = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if rel.is_empty() {
        return Ok(());
    }
    let path = ctx.root.join(rel);
    let mut content = if path.exists() {
        fs::read_to_string(&path).map_err(map_io)?
    } else {
        String::new()
    };
    if !content.lines().any(|line| line.trim() == INDEX_DIR_REL) {
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str(INDEX_DIR_REL);
        content.push('\n');
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(map_io)?;
        }
        write_atomic(&path, &content).map_err(map_io)?;
    }
    Ok(())
}

fn count_index_records(path: &Path) -> std::result::Result<usize, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to parse {}: {}", path.display(), err),
        )
    })?;
    Ok(parsed["records"].as_array().map_or(0, Vec::len))
}

fn digest_json(value: &Value) -> String {
    let raw = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&raw);
    format!("sha256:{}", to_hex(&hasher.finalize()))
}
