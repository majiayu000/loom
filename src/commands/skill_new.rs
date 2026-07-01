use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{AgentKind, SkillNewArgs, SkillNewTemplate};
use crate::envelope::Meta;
use crate::fs_util::remove_path_if_exists;
use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::file_ops::rollback_added_skill;
use super::helpers::{agent_kind_as_str, map_arg, map_git, map_io, map_lock, validate_skill_name};
use super::projections::maybe_autosync_or_queue;
use super::{App, CommandFailure, SkillLintMode, lint_skill_source};

const NEW_SKILL_FILES: [&str; 7] = [
    "SKILL.md",
    "references/README.md",
    "scripts/README.md",
    "assets/README.md",
    "evals/triggers.jsonl",
    "evals/tasks.jsonl",
    "loom.skill.toml",
];

#[derive(Clone)]
struct SkillNewPlan {
    skill: String,
    path: PathBuf,
    template: SkillNewTemplate,
    description: String,
    agent: Option<AgentKind>,
    files: Vec<GeneratedFile>,
}

#[derive(Clone)]
struct GeneratedFile {
    rel: &'static str,
    body: String,
}

#[derive(Debug, Serialize)]
struct LoomSkillManifest {
    schema: String,
    name: String,
    trust: String,
    owners: Vec<String>,
    requires_tools: Vec<String>,
    requires_mcp: Vec<String>,
    risk: String,
    default_activation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
}

impl App {
    pub fn cmd_skill_new(
        &self,
        args: &SkillNewArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_portable_skill_name(&args.name).map_err(map_arg)?;
        let plan = build_skill_new_plan(&self.ctx, args);

        if args.dry_run {
            return Ok((render_skill_new_plan(&plan), Meta::default()));
        }

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;

        if plan.path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("skill '{}' already exists", plan.skill),
            ));
        }

        let staging_root = self
            .ctx
            .state_dir
            .join(format!("tmp-skill-new-{}", Uuid::new_v4()));
        let staging_skill = staging_root.join(&plan.skill);
        let cleanup_staging = || {
            let _ = remove_path_if_exists(&staging_root);
        };

        remove_path_if_exists(&staging_root).map_err(map_io)?;
        if let Err(err) = write_generated_files(&staging_skill, &plan.files) {
            cleanup_staging();
            return Err(err);
        }

        let lint = lint_skill_source(&staging_skill, &plan.skill, SkillLintMode::Strict);
        if lint.summary.error_count > 0 {
            cleanup_staging();
            let mut failure = CommandFailure::new(
                ErrorCode::SchemaMismatch,
                format!("generated skill '{}' failed strict lint", plan.skill),
            );
            failure.details = json!({ "report": lint });
            return Err(failure);
        }

        let manifest = match parse_manifest_file(&staging_skill.join("loom.skill.toml")) {
            Ok(manifest) => manifest,
            Err(err) => {
                cleanup_staging();
                return Err(err);
            }
        };
        if let Err(err) = fs::create_dir_all(&self.ctx.skills_dir) {
            cleanup_staging();
            return Err(map_io(err));
        }
        if let Err(err) = fs::rename(&staging_skill, &plan.path) {
            cleanup_staging();
            return Err(map_io(err));
        }
        cleanup_staging();

        let skill_rel = format!("skills/{}", plan.skill);
        if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
            rollback_added_skill(&self.ctx, &skill_rel, &plan.path);
            return Err(map_git(err));
        }

        let commit = match gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel)) {
            Ok(false) => None,
            Ok(true) => {
                let message = format!("skill({}): create skeleton", plan.skill);
                match gitops::commit(&self.ctx, &message) {
                    Ok(commit) => Some(commit),
                    Err(err) => {
                        rollback_added_skill(&self.ctx, &skill_rel, &plan.path);
                        return Err(map_git(err));
                    }
                }
            }
            Err(err) => {
                rollback_added_skill(&self.ctx, &skill_rel, &plan.path);
                return Err(map_git(err));
            }
        };

        let mut meta = Meta::default();
        if let Some(commit) = commit.as_deref()
            && let Err(err) = maybe_autosync_or_queue(
                &self.ctx,
                "skill.new",
                request_id,
                json!({"skill": plan.skill, "commit": commit}),
                &mut meta,
            )
        {
            rollback_added_skill(&self.ctx, &skill_rel, &plan.path);
            return Err(err);
        }

        Ok((
            json!({
                "skill": plan.skill,
                "path": plan.path,
                "template": template_id(plan.template),
                "description": plan.description,
                "agent": plan.agent.map(agent_kind_as_str),
                "created": true,
                "dry_run": false,
                "files": NEW_SKILL_FILES,
                "manifest": manifest,
                "lint": {
                    "valid": lint.valid,
                    "error_count": lint.summary.error_count,
                    "warning_count": lint.summary.warning_count
                },
                "commit": commit,
                "next_actions": skill_new_next_actions(&plan.skill)
            }),
            meta,
        ))
    }
}

fn build_skill_new_plan(ctx: &AppContext, args: &SkillNewArgs) -> SkillNewPlan {
    let description = args.description.clone().unwrap_or_else(|| {
        format!(
            "Use when an AI agent needs the {} skill workflow for a focused task.",
            args.name
        )
    });
    let path = ctx.skill_path(&args.name);
    let files = generated_files(&args.name, args.template, &description, args.agent);
    SkillNewPlan {
        skill: args.name.clone(),
        path,
        template: args.template,
        description,
        agent: args.agent,
        files,
    }
}

fn generated_files(
    skill: &str,
    template: SkillNewTemplate,
    description: &str,
    agent: Option<AgentKind>,
) -> Vec<GeneratedFile> {
    vec![
        GeneratedFile {
            rel: "SKILL.md",
            body: skill_markdown(skill, template, description, agent),
        },
        GeneratedFile {
            rel: "references/README.md",
            body: references_readme(template),
        },
        GeneratedFile {
            rel: "scripts/README.md",
            body: scripts_readme(template),
        },
        GeneratedFile {
            rel: "assets/README.md",
            body: "# Assets\n\nPlace optional images, fixtures, or static resources here.\n"
                .to_string(),
        },
        GeneratedFile {
            rel: "evals/triggers.jsonl",
            body: format!(
                "{{\"prompt\":\"Use {skill} for a representative task\",\"should_trigger\":true}}\n"
            ),
        },
        GeneratedFile {
            rel: "evals/tasks.jsonl",
            body: format!(
                "{{\"id\":\"{skill}-smoke\",\"prompt\":\"Run the {skill} workflow on a small example\",\"expected\":\"document the result\"}}\n"
            ),
        },
        GeneratedFile {
            rel: "loom.skill.toml",
            body: manifest_toml(skill, template, agent),
        },
    ]
}

fn skill_markdown(
    skill: &str,
    template: SkillNewTemplate,
    description: &str,
    agent: Option<AgentKind>,
) -> String {
    let title = title_from_skill(skill);
    let description_scalar = frontmatter_string(description);
    let agent_line = agent
        .map(|agent| {
            format!(
                "compatibility: {}\n",
                frontmatter_string(&format!(
                    "Designed for {} agent skill directories.",
                    agent_kind_as_str(agent)
                ))
            )
        })
        .unwrap_or_default();
    let workflow = match template {
        SkillNewTemplate::Basic => {
            "1. Confirm the task fits this skill.\n2. Read relevant references.\n3. Execute the workflow and report results.\n"
        }
        SkillNewTemplate::CodingWorkflow => {
            "1. Reproduce the relevant code or test behavior.\n2. Make the smallest scoped code change.\n3. Run focused verification before reporting results.\n"
        }
        SkillNewTemplate::Scripted => {
            "1. Inspect `scripts/README.md` before running helpers.\n2. Run scripts with explicit arguments.\n3. Capture exit codes and important output.\n"
        }
        SkillNewTemplate::ReferenceHeavy => {
            "1. Load only the reference files needed for the task.\n2. Apply the relevant procedure.\n3. Cite the reference path used in the result.\n"
        }
    };
    format!(
        "---\nname: {skill}\ndescription: {description_scalar}\nlicense: Proprietary\n{agent_line}---\n\n# {title}\n\n## When To Use\n\n{description}\n\n## Workflow\n\n{workflow}\n## References\n\nSee `references/README.md` for optional background.\n\n## Scripts\n\nSee `scripts/README.md` before adding executable helpers.\n"
    )
}

fn frontmatter_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn references_readme(template: SkillNewTemplate) -> String {
    let body = match template {
        SkillNewTemplate::ReferenceHeavy => {
            "Move background material here so `SKILL.md` stays short. Keep references task-focused and link them from the main workflow."
        }
        _ => {
            "Add optional background material here when the main skill file would become too long."
        }
    };
    format!("# References\n\n{body}\n")
}

fn scripts_readme(template: SkillNewTemplate) -> String {
    let body = match template {
        SkillNewTemplate::Scripted => {
            "Document each helper script, required arguments, dependencies, and failure modes before adding executable files."
        }
        _ => "Document helper scripts here before adding executable files.",
    };
    format!("# Scripts\n\n{body}\n")
}

fn manifest_toml(skill: &str, template: SkillNewTemplate, agent: Option<AgentKind>) -> String {
    let agent_line = agent
        .map(|agent| format!("agent = \"{}\"\n", agent_kind_as_str(agent)))
        .unwrap_or_default();
    format!(
        "schema = \"loom.skill.v1\"\nname = \"{skill}\"\ntemplate = \"{}\"\ntrust = \"local-draft\"\nowners = []\nrequires_tools = []\nrequires_mcp = []\nrisk = \"unknown\"\ndefault_activation = \"manual-or-model\"\n{agent_line}",
        template_id(template)
    )
}

fn write_generated_files(
    staging_skill: &Path,
    files: &[GeneratedFile],
) -> std::result::Result<(), CommandFailure> {
    for file in files {
        let path = staging_skill.join(file.rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(map_io)?;
        }
        fs::write(path, &file.body).map_err(map_io)?;
    }
    Ok(())
}

fn render_skill_new_plan(plan: &SkillNewPlan) -> Value {
    json!({
        "skill": plan.skill,
        "path": plan.path,
        "template": template_id(plan.template),
        "description": plan.description,
        "agent": plan.agent.map(agent_kind_as_str),
        "created": false,
        "dry_run": true,
        "files": NEW_SKILL_FILES,
        "previews": plan.files.iter().map(|file| {
            json!({
                "path": file.rel,
                "content": file.body
            })
        }).collect::<Vec<_>>(),
        "next_actions": skill_new_next_actions(&plan.skill)
    })
}

fn parse_manifest_file(path: &Path) -> std::result::Result<LoomSkillManifest, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    parse_manifest(&raw).map_err(|message| {
        CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!("generated loom.skill.toml is invalid: {message}"),
        )
    })
}

fn parse_manifest(raw: &str) -> Result<LoomSkillManifest, String> {
    let mut schema = None;
    let mut name = None;
    let mut trust = None;
    let mut owners = None;
    let mut requires_tools = None;
    let mut requires_mcp = None;
    let mut risk = None;
    let mut default_activation = None;
    let mut agent = None;

    for (index, line) in raw.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (key, value) = trimmed
            .split_once('=')
            .ok_or_else(|| format!("line {line_no} is not a key/value assignment"))?;
        let key = key.trim();
        let value = value.trim();
        match key {
            "schema" => schema = Some(parse_toml_string(value, key, line_no)?),
            "name" => name = Some(parse_toml_string(value, key, line_no)?),
            "trust" => trust = Some(parse_toml_string(value, key, line_no)?),
            "owners" => owners = Some(parse_toml_string_array(value, key, line_no)?),
            "requires_tools" => {
                requires_tools = Some(parse_toml_string_array(value, key, line_no)?)
            }
            "requires_mcp" => requires_mcp = Some(parse_toml_string_array(value, key, line_no)?),
            "risk" => risk = Some(parse_toml_string(value, key, line_no)?),
            "default_activation" => {
                default_activation = Some(parse_toml_string(value, key, line_no)?)
            }
            "template" => {
                let _ = parse_toml_string(value, key, line_no)?;
            }
            "agent" => agent = Some(parse_toml_string(value, key, line_no)?),
            other => return Err(format!("line {line_no} has unsupported key '{other}'")),
        }
    }

    Ok(LoomSkillManifest {
        schema: required_manifest_field(schema, "schema")?,
        name: required_manifest_field(name, "name")?,
        trust: required_manifest_field(trust, "trust")?,
        owners: owners.unwrap_or_default(),
        requires_tools: requires_tools.unwrap_or_default(),
        requires_mcp: requires_mcp.unwrap_or_default(),
        risk: required_manifest_field(risk, "risk")?,
        default_activation: required_manifest_field(default_activation, "default_activation")?,
        agent,
    })
}

fn parse_toml_string(value: &str, key: &str, line_no: usize) -> Result<String, String> {
    if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
        return Err(format!("field '{key}' on line {line_no} must be a string"));
    }
    Ok(value[1..value.len() - 1].to_string())
}

fn parse_toml_string_array(value: &str, key: &str, line_no: usize) -> Result<Vec<String>, String> {
    if value == "[]" {
        return Ok(Vec::new());
    }
    if !value.starts_with('[') || !value.ends_with(']') {
        return Err(format!(
            "field '{key}' on line {line_no} must be a string array"
        ));
    }
    let inner = value[1..value.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    inner
        .split(',')
        .map(|item| parse_toml_string(item.trim(), key, line_no))
        .collect()
}

fn required_manifest_field(value: Option<String>, key: &str) -> Result<String, String> {
    value.ok_or_else(|| format!("missing required field '{key}'"))
}

fn validate_portable_skill_name(skill: &str) -> anyhow::Result<()> {
    validate_skill_name(skill)?;
    if !(1..=64).contains(&skill.len()) {
        anyhow::bail!("skill name must be 1-64 characters");
    }
    if skill.starts_with('-') || skill.ends_with('-') {
        anyhow::bail!("skill name must not start or end with '-'");
    }
    if skill.contains("--") {
        anyhow::bail!("skill name must not contain consecutive '-'");
    }
    if !skill
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        anyhow::bail!("skill name must use lowercase letters, digits, and hyphens only");
    }
    Ok(())
}

fn template_id(template: SkillNewTemplate) -> &'static str {
    match template {
        SkillNewTemplate::Basic => "basic",
        SkillNewTemplate::CodingWorkflow => "coding-workflow",
        SkillNewTemplate::Scripted => "scripted",
        SkillNewTemplate::ReferenceHeavy => "reference-heavy",
    }
}

fn title_from_skill(skill: &str) -> String {
    skill
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_ascii_uppercase().to_string();
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn skill_new_next_actions(skill: &str) -> Vec<String> {
    vec![
        format!("edit skills/{skill}/SKILL.md workflow details"),
        format!("run loom skill lint {skill} --strict"),
        format!("run loom skill eval {skill}"),
    ]
}
