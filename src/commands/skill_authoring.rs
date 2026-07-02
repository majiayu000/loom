use std::env;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::cli::{
    SkillAuthoringProviderArg, SkillDraftArgs, SkillExtractArgs, SkillGenerateEvalsArgs,
    SkillRewriteArgs, SkillTuneDescriptionArgs,
};
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::gitops;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{
    agent_kind_as_str, ensure_skill_exists, map_arg, map_io, validate_non_empty,
    validate_skill_name,
};
use super::{App, CommandFailure, redact_sensitive_string};

const PATCH_SCHEMA_VERSION: u32 = 1;
const MOCK_CREATED_AT: &str = "2026-07-01T00:00:00Z";
const MAX_PROMPT_SOURCE_BYTES: usize = 8192;

trait AuthoringProvider {
    fn generate_patch(
        &self,
        ctx: &AppContext,
        request: AuthoringRequest,
    ) -> std::result::Result<GeneratedPatch, CommandFailure>;
}

struct MockAuthoringProvider;

#[derive(Clone)]
struct AuthoringRequest {
    action: &'static str,
    skill: String,
    goal: String,
    prompt_sources: Vec<PromptSource>,
    patch_kind: PatchKind,
}

#[derive(Clone)]
enum PatchKind {
    Draft { agent: Option<String> },
    Extract,
    Rewrite { instruction: String },
    TuneDescription { description: String },
    GenerateEvals { task: String },
}

#[derive(Clone)]
struct PromptSource {
    kind: String,
    path: String,
    bytes: usize,
    truncated: bool,
    content_redacted: String,
}

struct GeneratedPatch {
    patch_body: String,
    files: Vec<Value>,
    validation_plan: Vec<Value>,
    risk_notes: Vec<String>,
}

impl App {
    pub fn cmd_skill_draft(
        &self,
        args: &SkillDraftArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.name).map_err(map_arg)?;
        validate_non_empty("from-session", &args.from_session)?;

        let source = prompt_source_from_path_or_id("session", &args.from_session)?;
        let request = AuthoringRequest {
            action: "draft",
            skill: args.name.clone(),
            goal: format!("draft a new skill from {}", source.path),
            prompt_sources: vec![source],
            patch_kind: PatchKind::Draft {
                agent: args.agent.map(agent_kind_as_str).map(ToString::to_string),
            },
        };
        run_authoring_command(&self.ctx, args.provider, args.dry_run, request)
    }

    pub fn cmd_skill_extract(
        &self,
        args: &SkillExtractArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let source = prompt_source_from_path("diff", &args.from_diff)?;
        let request = AuthoringRequest {
            action: "extract",
            skill: args.skill.clone(),
            goal: format!("extract reviewed diff context from {}", source.path),
            prompt_sources: vec![source],
            patch_kind: PatchKind::Extract,
        };
        run_authoring_command(&self.ctx, args.provider, args.dry_run, request)
    }

    pub fn cmd_skill_rewrite(
        &self,
        args: &SkillRewriteArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        ensure_skill_exists(&self.ctx, &args.skill)?;
        validate_non_empty("instruction", &args.instruction)?;

        let source = prompt_source_for_skill(&self.ctx, &args.skill)?;
        let instruction = redact_prompt_material(&args.instruction);
        let request = AuthoringRequest {
            action: "rewrite",
            skill: args.skill.clone(),
            goal: instruction.clone(),
            prompt_sources: vec![source],
            patch_kind: PatchKind::Rewrite { instruction },
        };
        run_authoring_command(&self.ctx, args.provider, args.dry_run, request)
    }

    pub fn cmd_skill_tune_description(
        &self,
        args: &SkillTuneDescriptionArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let description = args.description.clone().unwrap_or_else(|| {
            format!(
                "Use when an agent needs the {} workflow with precise trigger guidance.",
                args.skill
            )
        });
        validate_non_empty("description", &description)?;
        let description = redact_prompt_material(&description);

        let source = prompt_source_for_skill(&self.ctx, &args.skill)?;
        let request = AuthoringRequest {
            action: "tune-description",
            skill: args.skill.clone(),
            goal: "tune frontmatter description and trigger fixtures".to_string(),
            prompt_sources: vec![source],
            patch_kind: PatchKind::TuneDescription { description },
        };
        run_authoring_command(&self.ctx, args.provider, args.dry_run, request)
    }

    pub fn cmd_skill_generate_evals(
        &self,
        args: &SkillGenerateEvalsArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let task = args
            .task
            .clone()
            .unwrap_or_else(|| format!("Run the {} workflow on a representative task", args.skill));
        validate_non_empty("task", &task)?;
        let task = redact_prompt_material(&task);

        let source = prompt_source_for_skill(&self.ctx, &args.skill)?;
        let request = AuthoringRequest {
            action: "generate-evals",
            skill: args.skill.clone(),
            goal: "generate reviewable eval fixture diffs".to_string(),
            prompt_sources: vec![source],
            patch_kind: PatchKind::GenerateEvals { task },
        };
        run_authoring_command(&self.ctx, args.provider, args.dry_run, request)
    }
}

impl AuthoringProvider for MockAuthoringProvider {
    fn generate_patch(
        &self,
        ctx: &AppContext,
        request: AuthoringRequest,
    ) -> std::result::Result<GeneratedPatch, CommandFailure> {
        let skill_rel = format!("skills/{}/SKILL.md", request.skill);
        match request.patch_kind.clone() {
            PatchKind::Draft { agent } => {
                let body = draft_skill_body(&request.skill, agent.as_deref(), &request);
                Ok(GeneratedPatch {
                    patch_body: add_file_patch(&skill_rel, &body),
                    files: vec![json!({"path": skill_rel, "change": "add"})],
                    validation_plan: validation_plan(&request.skill),
                    risk_notes: base_risk_notes("draft"),
                })
            }
            PatchKind::Extract => {
                let path = format!("skills/{}/references/extracted-context.md", request.skill);
                let body = format!(
                    "# Extracted Context\n\nGoal: {}\n\nSource material was redacted and captured in the patch artifact for review.\n",
                    request.goal
                );
                Ok(GeneratedPatch {
                    patch_body: add_file_patch(&path, &body),
                    files: vec![json!({"path": path, "change": "add"})],
                    validation_plan: validation_plan(&request.skill),
                    risk_notes: base_risk_notes("extract"),
                })
            }
            PatchKind::Rewrite { instruction } => {
                let path = ctx.skill_path(&request.skill).join("SKILL.md");
                let old = fs::read_to_string(&path).map_err(map_io)?;
                let addition = format!(
                    "\n## Mock Rewrite Notes\n\n- Requested rewrite: {}\n",
                    instruction
                );
                let new = append_once(&old, &addition);
                Ok(GeneratedPatch {
                    patch_body: replace_file_patch(&skill_rel, &old, &new),
                    files: vec![json!({"path": skill_rel, "change": "modify"})],
                    validation_plan: validation_plan(&request.skill),
                    risk_notes: base_risk_notes("rewrite"),
                })
            }
            PatchKind::TuneDescription { description } => {
                let path = ctx.skill_path(&request.skill).join("SKILL.md");
                let old = fs::read_to_string(&path).map_err(map_io)?;
                let new = tune_frontmatter_description(&old, &description);
                let trigger_rel = "evals/triggers.jsonl";
                let trigger_body = tune_description_trigger_body(&request.skill, &description);
                let trigger = skill_file_patch(ctx, &request.skill, trigger_rel, &trigger_body)?;
                Ok(GeneratedPatch {
                    patch_body: format!(
                        "{}{}",
                        replace_file_patch(&skill_rel, &old, &new),
                        trigger.patch_body
                    ),
                    files: vec![json!({"path": skill_rel, "change": "modify"}), trigger.file],
                    validation_plan: validation_plan(&request.skill),
                    risk_notes: base_risk_notes("tune-description"),
                })
            }
            PatchKind::GenerateEvals { task } => {
                let trigger = skill_file_patch(
                    ctx,
                    &request.skill,
                    "evals/triggers.jsonl",
                    &generate_eval_trigger_body(&request.skill, &task),
                )?;
                let task = skill_file_patch(
                    ctx,
                    &request.skill,
                    "evals/tasks.jsonl",
                    &generate_eval_task_body(&request.skill, &task),
                )?;
                Ok(GeneratedPatch {
                    patch_body: format!("{}{}", trigger.patch_body, task.patch_body),
                    files: vec![trigger.file, task.file],
                    validation_plan: validation_plan(&request.skill),
                    risk_notes: base_risk_notes("generate-evals"),
                })
            }
        }
    }
}

fn run_authoring_command(
    ctx: &AppContext,
    provider: SkillAuthoringProviderArg,
    dry_run: bool,
    request: AuthoringRequest,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    if !dry_run {
        ctx.ensure_not_loom_tool_repo_root().map_err(map_arg)?;
        ctx.ensure_state_layout().map_err(map_io)?;
    }

    let provider_name = match provider {
        SkillAuthoringProviderArg::Mock => "mock",
    };
    let generated = MockAuthoringProvider.generate_patch(ctx, request.clone())?;
    let source_digest = skill_source_digest(ctx, &request.skill)?;
    let source_ref =
        gitops::resolve_ref(ctx, "HEAD").unwrap_or_else(|_| "working-tree".to_string());
    let patch_id = patch_id(
        &request,
        provider_name,
        &source_digest,
        &generated.patch_body,
    );
    let patch_dir = ctx.state_dir.join("patches");
    let artifact_path = patch_dir.join(format!("{patch_id}.json"));
    let patch_path = patch_dir.join(format!("{patch_id}.patch"));
    let prompt_material = prompt_material_json(&request.prompt_sources);
    let artifact = json!({
        "schema_version": PATCH_SCHEMA_VERSION,
        "patch_id": patch_id,
        "skill": request.skill,
        "action": request.action,
        "goal": request.goal,
        "source_ref": source_ref,
        "source_digest": source_digest,
        "provider": provider_name,
        "created_at": MOCK_CREATED_AT,
        "files": generated.files,
        "prompt_material": prompt_material,
        "validation_plan": generated.validation_plan,
        "risk_notes": generated.risk_notes,
        "patch_path": path_display_string(&patch_path),
        "artifact_path": path_display_string(&artifact_path),
        "deferred_apply": true
    });

    if !dry_run {
        let raw = serde_json::to_string_pretty(&artifact).map_err(|err| {
            CommandFailure::new(
                ErrorCode::InternalError,
                format!("failed to serialize patch artifact: {err}"),
            )
        })? + "\n";
        write_atomic(&artifact_path, &raw).map_err(map_io)?;
        write_atomic(&patch_path, &generated.patch_body).map_err(map_io)?;
    }

    Ok((
        json!({
            "patch_id": artifact["patch_id"],
            "skill": artifact["skill"],
            "action": artifact["action"],
            "provider": provider_name,
            "dry_run": dry_run,
            "artifact_written": !dry_run,
            "artifact_path": path_display_string(&artifact_path),
            "patch_path": path_display_string(&patch_path),
            "artifact": artifact,
            "patch": generated.patch_body,
            "next_actions": [
                format!("review {}", patch_path.display()),
                format!("run loom skill apply-patch {} --idempotency-key <key> after validation gates land", artifact["patch_id"].as_str().unwrap_or(""))
            ]
        }),
        Meta::default(),
    ))
}

fn prompt_source_from_path_or_id(
    kind: &str,
    raw: &str,
) -> std::result::Result<PromptSource, CommandFailure> {
    let path = Path::new(raw);
    if path.exists() {
        return prompt_source_from_path(kind, path);
    }
    let material = format!("{kind} id: {raw}");
    Ok(prompt_source(kind, raw, material.as_bytes()))
}

fn prompt_source_from_path(
    kind: &str,
    path: &Path,
) -> std::result::Result<PromptSource, CommandFailure> {
    let bytes = fs::read(path).map_err(map_io)?;
    Ok(prompt_source(kind, &path_display_string(path), &bytes))
}

fn prompt_source_for_skill(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<PromptSource, CommandFailure> {
    prompt_source_from_path("skill_source", &ctx.skill_path(skill).join("SKILL.md"))
}

fn prompt_source(kind: &str, path: &str, bytes: &[u8]) -> PromptSource {
    let truncated = bytes.len() > MAX_PROMPT_SOURCE_BYTES;
    let slice = if truncated {
        &bytes[..MAX_PROMPT_SOURCE_BYTES]
    } else {
        bytes
    };
    let raw = String::from_utf8_lossy(slice).to_string();
    PromptSource {
        kind: kind.to_string(),
        path: redact_prompt_material(path),
        bytes: bytes.len(),
        truncated,
        content_redacted: redact_prompt_material(&raw),
    }
}

fn prompt_material_json(sources: &[PromptSource]) -> Value {
    let total_bytes = sources.iter().map(|source| source.bytes).sum::<usize>();
    json!({
        "sources": sources.iter().map(|source| {
            json!({
                "kind": source.kind,
                "path": source.path,
                "bytes": source.bytes,
                "truncated": source.truncated,
                "content_redacted": source.content_redacted
            })
        }).collect::<Vec<_>>(),
        "total_bytes": total_bytes,
        "max_source_bytes": MAX_PROMPT_SOURCE_BYTES,
        "redacted": true
    })
}

fn redact_prompt_material(raw: &str) -> String {
    let mut redacted = redact_sensitive_string(raw);
    for (key, value) in env::vars() {
        if is_sensitive_env_key(&key) && value.len() >= 4 {
            redacted = redacted.replace(&value, "<redacted>");
        }
    }
    redacted
}

fn is_sensitive_env_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "API_KEY",
        "AUTH",
        "CREDENTIAL",
    ]
    .iter()
    .any(|needle| upper.contains(needle))
}

pub(super) fn skill_source_digest(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<String, CommandFailure> {
    let path = ctx.skill_path(skill);
    if !path.exists() {
        return Ok(sha256_digest(format!("missing:{skill}").as_bytes()));
    }
    let mut files = Vec::new();
    collect_digest_paths(&path, &path, &mut files)?;
    let mut hasher = Sha256::new();
    for (rel, bytes) in files {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(&bytes);
        hasher.update(b"\0");
    }
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

fn collect_digest_paths(
    root: &Path,
    path: &Path,
    out: &mut Vec<(String, Vec<u8>)>,
) -> std::result::Result<(), CommandFailure> {
    let mut entries = fs::read_dir(path)
        .map_err(map_io)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(map_io)?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let entry_path = entry.path();
        let meta = fs::symlink_metadata(&entry_path).map_err(map_io)?;
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&entry_path).map_err(map_io)?;
            out.push((
                relative_path(root, &entry_path),
                format!("symlink:{}", target.display()).into_bytes(),
            ));
        } else if meta.is_dir() {
            collect_digest_paths(root, &entry_path, out)?;
        } else if meta.is_file() {
            out.push((
                relative_path(root, &entry_path),
                fs::read(&entry_path).map_err(map_io)?,
            ));
        }
    }
    Ok(())
}

fn patch_id(
    request: &AuthoringRequest,
    provider: &str,
    source_digest: &str,
    patch_body: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(request.action.as_bytes());
    hasher.update(request.skill.as_bytes());
    hasher.update(request.goal.as_bytes());
    hasher.update(provider.as_bytes());
    hasher.update(source_digest.as_bytes());
    hasher.update(patch_body.as_bytes());
    for source in &request.prompt_sources {
        hasher.update(source.kind.as_bytes());
        hasher.update(source.path.as_bytes());
        hasher.update(source.content_redacted.as_bytes());
    }
    let hex = to_hex(&hasher.finalize());
    format!("skillpatch_{}", &hex[..24])
}

pub(super) fn sha256_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

pub(super) fn validate_patch_id(patch_id: &str) -> std::result::Result<(), CommandFailure> {
    if !patch_id.starts_with("skillpatch_") {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "patch id must start with skillpatch_",
        ));
    }
    if patch_id == "skillpatch_" || patch_id.len() > 80 {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "patch id length is invalid",
        ));
    }
    if !patch_id
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "patch id must contain only lowercase letters, digits, and underscore",
        ));
    }
    Ok(())
}

fn draft_skill_body(skill: &str, agent: Option<&str>, request: &AuthoringRequest) -> String {
    let title = skill
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| format!("{}{}", first.to_ascii_uppercase(), chars.as_str()))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let agent_line = agent
        .map(|agent| format!("compatibility: \"Designed for {agent} agent skill directories.\"\n"))
        .unwrap_or_default();
    format!(
        "---\nname: {skill}\ndescription: \"Use when agents need the {skill} workflow from reviewed prompt material.\"\nlicense: Proprietary\n{agent_line}---\n\n# {title}\n\n## When To Use\n\nUse when the reviewed prompt material matches this workflow.\n\n## Workflow\n\n1. Confirm the task matches the selected context.\n2. Read the redacted source notes in patch artifact `{}`.\n3. Execute the workflow and record validation evidence.\n",
        request.action
    )
}

fn tune_frontmatter_description(raw: &str, description: &str) -> String {
    let mut lines = raw.lines().map(ToString::to_string).collect::<Vec<_>>();
    if lines.first().is_some_and(|line| line == "---")
        && let Some(end) = lines.iter().skip(1).position(|line| *line == "---")
    {
        let frontmatter_end = end + 1;
        for line in lines.iter_mut().take(frontmatter_end).skip(1) {
            if line.starts_with("description:") {
                *line = format!("description: {}", quoted_yaml_scalar(description));
                return lines.join("\n") + "\n";
            }
        }
        lines.insert(
            frontmatter_end,
            format!("description: {}", quoted_yaml_scalar(description)),
        );
        return lines.join("\n") + "\n";
    }
    format!("description: {}\n{}", quoted_yaml_scalar(description), raw)
}

fn quoted_yaml_scalar(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

fn append_once(old: &str, addition: &str) -> String {
    if old.contains(addition.trim()) {
        return old.to_string();
    }
    let mut new = old.to_string();
    if !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(addition);
    new
}

struct FilePatch {
    patch_body: String,
    file: Value,
}

fn skill_file_patch(
    ctx: &AppContext,
    skill: &str,
    rel: &str,
    body: &str,
) -> std::result::Result<FilePatch, CommandFailure> {
    let path = format!("skills/{skill}/{rel}");
    let source_path = ctx.skill_path(skill).join(rel);
    if source_path.exists() {
        let old = fs::read_to_string(&source_path).map_err(map_io)?;
        let new = append_once(&old, body);
        Ok(FilePatch {
            patch_body: replace_file_patch(&path, &old, &new),
            file: json!({"path": path, "change": "modify"}),
        })
    } else {
        Ok(FilePatch {
            patch_body: add_file_patch(&path, body),
            file: json!({"path": path, "change": "add"}),
        })
    }
}

fn tune_description_trigger_body(skill: &str, description: &str) -> String {
    format!(
        "{}\n{}\n",
        json!({
            "id": format!("{skill}-tuned-positive"),
            "prompt": format!("Use {skill} when {description}"),
            "should_trigger": true
        }),
        json!({
            "id": format!("{skill}-tuned-negative"),
            "prompt": "Summarize a neutral planning note without selecting specialized workflows.",
            "should_trigger": false,
            "observed_trigger": false
        })
    )
}

fn generate_eval_trigger_body(skill: &str, task: &str) -> String {
    format!(
        "{}\n{}\n",
        json!({
            "id": format!("{skill}-generated-positive"),
            "prompt": task,
            "should_trigger": true
        }),
        json!({
            "id": format!("{skill}-generated-negative"),
            "prompt": "Summarize a neutral planning note without selecting specialized workflows.",
            "should_trigger": false,
            "observed_trigger": false
        })
    )
}

fn generate_eval_task_body(skill: &str, task: &str) -> String {
    format!(
        "{}\n",
        json!({
            "id": format!("{skill}-generated-smoke"),
            "prompt": task,
            "checks": {
                "outcome_contains": ["task complete"],
                "commands_contains": ["loom skill eval"],
                "process_contains": ["loom skill eval"],
                "exit_code": 0,
                "max_tokens": 200,
                "max_commands": 3
            }
        })
    )
}

fn add_file_patch(path: &str, body: &str) -> String {
    let lines = split_lines(body);
    let mut patch = format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in lines {
        patch.push('+');
        patch.push_str(&line);
        patch.push('\n');
    }
    patch
}

fn replace_file_patch(path: &str, old: &str, new: &str) -> String {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let mut patch = format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    );
    for line in old_lines {
        patch.push('-');
        patch.push_str(&line);
        patch.push('\n');
    }
    for line in new_lines {
        patch.push('+');
        patch.push_str(&line);
        patch.push('\n');
    }
    patch
}

pub(super) fn split_lines(raw: &str) -> Vec<String> {
    let without_trailing = raw.strip_suffix('\n').unwrap_or(raw);
    if without_trailing.is_empty() {
        return Vec::new();
    }
    without_trailing.lines().map(ToString::to_string).collect()
}

fn validation_plan(skill: &str) -> Vec<Value> {
    vec![
        json!({"command": format!("loom skill lint {skill} --strict"), "required": true}),
        json!({"command": format!("loom skill scan {skill} --mode install --strict"), "required": true}),
        json!({"command": format!("loom skill eval run {skill} --agent codex --baseline no-skill --runner mock --dry-run"), "required": true}),
    ]
}

fn base_risk_notes(action: &str) -> Vec<String> {
    vec![
        format!("{action} output is a reviewable artifact only"),
        "source files are not mutated by generation commands".to_string(),
        "network and hosted model providers are disabled in this slice".to_string(),
        "scripts or destructive behavior require later apply-patch safety gates".to_string(),
    ]
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(super) fn path_display_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
