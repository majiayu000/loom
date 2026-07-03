use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::cli::IndexArgs;
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::gitops;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::state_model::{REGISTRY_SCHEMA_VERSION, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::helpers::{map_git, map_io, map_registry_state};
use super::super::skill_inventory::tokenize;
use super::super::{App, CommandFailure, build_skill_read_model};
use super::evidence::{
    dependency_report_for_skill, dependency_tools, latest_eval_summary, trigger_fixture_prompts,
};
use super::{load_skillsets_value, skillset_membership};

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
        let capabilities = capability_index_payload(&self.ctx, &model.skills, &skillsets)?;
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

fn capability_index_payload(
    ctx: &AppContext,
    skills: &[Value],
    skillsets: &Value,
) -> std::result::Result<Value, CommandFailure> {
    let membership = skillset_membership(skillsets);
    let mut records = Vec::new();
    for skill in skills {
        let Some(skill_id) = skill["skill_id"].as_str() else {
            continue;
        };
        let description = skill["description"].as_str().unwrap_or_default();
        let dependency = dependency_report_for_skill(ctx, skill_id, skill, None)?;
        let eval = latest_eval_summary(ctx, skill_id)?;
        let triggers = trigger_fixture_prompts(ctx, skill_id)?;
        records.push(json!({
            "schema_version": REGISTRY_SCHEMA_VERSION,
            "skill_id": skill_id,
            "source_digest": digest_json(skill),
            "capabilities": tokenize(description),
            "triggers": triggers,
            "domains": [],
            "tools": dependency_tools(dependency.as_ref()),
            "risk": "unknown",
            "trust": skill["trust"].as_str().unwrap_or("unknown"),
            "dependency_status": dependency
                .as_ref()
                .map(|report| report.status.as_str())
                .unwrap_or("missing-source"),
            "eval": {
                "trigger_precision": eval.trigger_precision,
                "trigger_recall": eval.trigger_recall,
                "baseline_delta": eval.baseline_delta,
            },
            "skillsets": membership.get(skill_id).cloned().unwrap_or_default(),
        }));
    }
    Ok(json!({ "schema_version": REGISTRY_SCHEMA_VERSION, "records": records }))
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
