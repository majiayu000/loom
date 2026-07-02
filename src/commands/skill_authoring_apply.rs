use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{
    EvalBaselineArg, EvalRunnerArg, SkillApplyPatchArgs, SkillEvalRunArgs, SkillEvalTriggerArgs,
};
use crate::envelope::Meta;
use crate::fs_util::{remove_path_if_exists, write_atomic};
use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::file_ops::copy_dir_recursive_without_symlinks;
use super::helpers::{map_arg, map_git, map_io, validate_non_empty, validate_skill_name};
use super::projections::maybe_autosync_or_queue;
use super::skill_authoring::{sha256_digest, skill_source_digest, validate_patch_id};
use super::skill_authoring_patch::{ParsedPatchChange, ReviewedPatchFile, parse_patch_changes};
use super::skill_safety::{SafetyFinding, SkillSafetyReport, evaluate_skill_safety};
use super::{App, CommandFailure, SkillLintMode, lint_skill_source};

const APPLY_RECORD_SCHEMA: &str = "skill-authoring-apply-record-v1";
const PATCH_APPLY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
struct SkillPatchArtifact {
    schema_version: u32,
    patch_id: String,
    skill: String,
    source_ref: String,
    source_digest: String,
    files: Vec<SkillPatchFile>,
}

#[derive(Debug, Deserialize)]
struct SkillPatchFile {
    path: String,
    #[serde(default)]
    change: String,
}

#[derive(Debug)]
struct Preimage {
    rel: PathBuf,
    bytes: Option<Vec<u8>>,
}

#[derive(Debug, Serialize)]
struct ApplyValidationReport {
    lint: Value,
    safety: Value,
    eval: Value,
}

impl App {
    pub fn cmd_skill_apply_patch(
        &self,
        args: &SkillApplyPatchArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_patch_id(&args.patch_id)?;
        let Some(idempotency_key) = args.idempotency_key.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--idempotency-key is required for skill apply-patch",
            ));
        };
        validate_non_empty("idempotency-key", idempotency_key)?;

        let key_digest = sha256_digest(idempotency_key.as_bytes());
        let artifact_path = self
            .ctx
            .state_dir
            .join("patches")
            .join(format!("{}.json", args.patch_id));
        let patch_path = self
            .ctx
            .state_dir
            .join("patches")
            .join(format!("{}.patch", args.patch_id));
        let record_path = skill_apply_record_path(&self.ctx, &key_digest);
        if record_path.exists() {
            let patch_digest = optional_patch_digest(&patch_path)?;
            return replay_apply_record(
                &record_path,
                &args.patch_id,
                patch_digest.as_deref(),
                &key_digest,
            );
        }
        if !artifact_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("patch artifact '{}' not found", args.patch_id),
            ));
        }
        if !patch_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("patch file '{}' not found", patch_path.display()),
            ));
        }

        let artifact = load_patch_artifact(&artifact_path)?;
        validate_artifact(&artifact, &args.patch_id)?;
        let patch_body = fs::read_to_string(&patch_path).map_err(map_io)?;
        let patch_digest = sha256_digest(patch_body.as_bytes());

        let _workspace = self
            .ctx
            .lock_workspace()
            .map_err(super::helpers::map_lock)?;
        self.ensure_write_repo_ready()?;
        if record_path.exists() {
            return replay_apply_record(
                &record_path,
                &artifact.patch_id,
                Some(&patch_digest),
                &key_digest,
            );
        }

        revalidate_source(&self.ctx, &artifact)?;
        let baseline_safety = baseline_safety(&self.ctx, &artifact.skill)?;
        let reviewed_files = reviewed_patch_files(&artifact);
        let changes =
            parse_patch_changes(&self.ctx, &artifact.skill, &patch_body, &reviewed_files)?;
        for change in &changes {
            ensure_safe_registry_path(&self.ctx, &change.rel)?;
        }

        let staging_root = stage_patch_for_validation(&self.ctx, &artifact.skill, &changes)?;
        let staging_app = staging_app(&staging_root)?;
        if let Err(err) =
            run_apply_validations(&staging_app, &artifact.skill, baseline_safety.as_ref())
        {
            let _ = remove_path_if_exists(&staging_root);
            return Err(err);
        }
        let _ = remove_path_if_exists(&staging_root);

        revalidate_source(&self.ctx, &artifact)?;
        let preimages = capture_preimages(&self.ctx, &changes)?;
        if let Err(err) = materialize_changes(&self.ctx, &changes) {
            restore_preimages(&self.ctx, &preimages);
            return Err(err);
        }

        let validation =
            match run_apply_validations(self, &artifact.skill, baseline_safety.as_ref()) {
                Ok(report) => report,
                Err(err) => {
                    restore_preimages(&self.ctx, &preimages);
                    return Err(err);
                }
            };

        let commit_paths = reviewed_commit_paths(&changes);
        let commit_path_refs = commit_paths.iter().map(String::as_str).collect::<Vec<_>>();
        let commit = match gitops::commit_paths_if_changed(
            &self.ctx,
            &commit_path_refs,
            &format!("skill({}): apply authoring patch", artifact.skill),
        ) {
            Ok(Some(commit)) => commit,
            Ok(None) => gitops::head(&self.ctx).map_err(map_git)?,
            Err(err) => {
                let mut failure = map_git(err);
                restore_preimages(&self.ctx, &preimages);
                if let Err(reset_err) = reset_staged_paths(&self.ctx, &commit_paths) {
                    failure.details = json!({
                        "staged_reset_error": reset_err.to_string(),
                    });
                }
                return Err(failure);
            }
        };

        let applied_source_digest = skill_source_digest(&self.ctx, &artifact.skill)?;
        let response = skill_apply_response(
            &artifact,
            &key_digest,
            &patch_digest,
            &commit,
            &applied_source_digest,
            false,
            &validation,
        );
        write_apply_record(&record_path, &response)?;

        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "skill.apply_patch",
            request_id,
            json!({"skill": artifact.skill, "patch_id": artifact.patch_id, "commit": commit}),
            &mut meta,
        )?;

        Ok((response, meta))
    }
}

fn load_patch_artifact(path: &Path) -> std::result::Result<SkillPatchArtifact, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!("invalid skill patch artifact '{}': {err}", path.display()),
        )
    })
}

fn optional_patch_digest(path: &Path) -> std::result::Result<Option<String>, CommandFailure> {
    if !path.exists() {
        return Ok(None);
    }
    let body = fs::read_to_string(path).map_err(map_io)?;
    Ok(Some(sha256_digest(body.as_bytes())))
}

fn validate_artifact(
    artifact: &SkillPatchArtifact,
    patch_id: &str,
) -> std::result::Result<(), CommandFailure> {
    if artifact.schema_version != PATCH_APPLY_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "unsupported patch artifact schema_version {}",
                artifact.schema_version
            ),
        ));
    }
    if artifact.patch_id != patch_id {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "patch artifact id does not match requested patch id",
        ));
    }
    validate_skill_name(&artifact.skill).map_err(map_arg)?;
    if artifact.files.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "patch artifact must list at least one changed file",
        ));
    }
    Ok(())
}

fn revalidate_source(
    ctx: &AppContext,
    artifact: &SkillPatchArtifact,
) -> std::result::Result<(), CommandFailure> {
    let current_digest = skill_source_digest(ctx, &artifact.skill)?;
    if current_digest != artifact.source_digest {
        let mut failure = CommandFailure::new(
            ErrorCode::CaptureConflict,
            "skill source digest changed since patch artifact was generated",
        );
        failure.details = json!({
            "skill": artifact.skill,
            "expected_source_digest": artifact.source_digest,
            "current_source_digest": current_digest,
        });
        return Err(failure);
    }
    if artifact.source_ref != "working-tree" {
        let current_ref = gitops::resolve_ref(ctx, "HEAD").map_err(map_git)?;
        if current_ref != artifact.source_ref {
            let mut failure = CommandFailure::new(
                ErrorCode::CaptureConflict,
                "registry HEAD changed since patch artifact was generated",
            );
            failure.details = json!({
                "skill": artifact.skill,
                "expected_source_ref": artifact.source_ref,
                "current_source_ref": current_ref,
            });
            return Err(failure);
        }
    }
    Ok(())
}

fn reviewed_patch_files(artifact: &SkillPatchArtifact) -> Vec<ReviewedPatchFile> {
    artifact
        .files
        .iter()
        .map(|file| ReviewedPatchFile {
            path: file.path.clone(),
            change: file.change.clone(),
        })
        .collect()
}

fn baseline_safety(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<Option<SkillSafetyReport>, CommandFailure> {
    if !ctx.skill_path(skill).is_dir() {
        return Ok(None);
    }
    evaluate_skill_safety(ctx, skill, "install", true).map(Some)
}

fn reviewed_commit_paths(changes: &[ParsedPatchChange]) -> Vec<String> {
    changes
        .iter()
        .map(|change| change.rel.to_string_lossy().to_string())
        .collect()
}

fn ensure_safe_registry_path(
    ctx: &AppContext,
    rel: &Path,
) -> std::result::Result<(), CommandFailure> {
    let mut current = ctx.root.clone();
    for component in rel.components() {
        let Component::Normal(part) = component else {
            return Err(CommandFailure::new(
                ErrorCode::PolicyBlocked,
                "patch path contains unsafe path components",
            ));
        };
        current.push(part);
        if let Ok(meta) = fs::symlink_metadata(&current) {
            if meta.file_type().is_symlink() {
                return Err(CommandFailure::new(
                    ErrorCode::PolicyBlocked,
                    format!("patch target '{}' crosses a symlink", current.display()),
                ));
            }
            if current == ctx.root.join(rel) && meta.is_dir() {
                return Err(CommandFailure::new(
                    ErrorCode::PolicyBlocked,
                    format!("patch target '{}' is a directory", current.display()),
                ));
            }
        }
    }
    Ok(())
}

fn stage_patch_for_validation(
    ctx: &AppContext,
    skill: &str,
    changes: &[ParsedPatchChange],
) -> std::result::Result<PathBuf, CommandFailure> {
    let staging_root = ctx
        .state_dir
        .join(format!("tmp-skill-apply-{}", Uuid::new_v4()));
    remove_path_if_exists(&staging_root).map_err(map_io)?;
    let staging_skill = staging_root.join("skills").join(skill);
    let real_skill = ctx.skill_path(skill);
    if real_skill.exists() {
        copy_dir_recursive_without_symlinks(&real_skill, &staging_skill).map_err(map_io)?;
    } else {
        fs::create_dir_all(&staging_skill).map_err(map_io)?;
    }
    for change in changes {
        write_change(&staging_root, change)?;
    }
    Ok(staging_root)
}

fn staging_app(staging_root: &Path) -> std::result::Result<App, CommandFailure> {
    let ctx = AppContext::new(Some(staging_root.to_path_buf())).map_err(|err| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("failed to create staging context: {err}"),
        )
    })?;
    Ok(App { ctx })
}

fn run_apply_validations(
    app: &App,
    skill: &str,
    baseline_safety: Option<&SkillSafetyReport>,
) -> std::result::Result<ApplyValidationReport, CommandFailure> {
    let skill_path = app.ctx.skill_path(skill);
    let lint = lint_skill_source(&skill_path, skill, SkillLintMode::Strict);
    if lint.summary.error_count > 0 {
        let mut failure = CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!("applied patch for skill '{skill}' failed strict lint"),
        );
        failure.details = json!({"lint": lint});
        return Err(failure);
    }

    let safety = evaluate_skill_safety(&app.ctx, skill, "install", true)?;
    let new_findings = baseline_safety
        .map(|baseline| new_blocking_safety_findings(baseline, &safety))
        .unwrap_or_default();
    let trust_blocked = safety.trust.quarantined
        || matches!(safety.trust.trust.as_str(), "blocked" | "quarantined");
    let safety_blocked = if baseline_safety.is_some() {
        trust_blocked || !new_findings.is_empty()
    } else {
        !safety.activation_allowed
    };
    if safety_blocked {
        let mut failure = CommandFailure::new(
            ErrorCode::PolicyBlocked,
            format!("applied patch for skill '{skill}' failed safety scan"),
        );
        failure.details = json!({"safety": safety, "new_findings": new_findings});
        return Err(failure);
    }

    let eval = run_eval_gates(app, skill)?;
    Ok(ApplyValidationReport {
        lint: json!({
            "status": "passed",
            "error_count": lint.summary.error_count,
            "warning_count": lint.summary.warning_count
        }),
        safety: json!({
            "status": "passed",
            "decision": safety.decision,
            "summary": safety.summary,
            "new_blocking_findings": new_findings.len()
        }),
        eval,
    })
}

fn new_blocking_safety_findings(
    baseline: &SkillSafetyReport,
    current: &SkillSafetyReport,
) -> Vec<SafetyFinding> {
    let baseline_keys = baseline
        .findings
        .iter()
        .map(safety_finding_key)
        .collect::<BTreeSet<_>>();
    current
        .findings
        .iter()
        .filter(|finding| matches!(finding.severity.as_str(), "critical" | "high"))
        .filter(|finding| !baseline_keys.contains(&safety_finding_key(finding)))
        .cloned()
        .collect()
}

fn safety_finding_key(finding: &SafetyFinding) -> (String, String, Option<String>, String, String) {
    (
        finding.id.clone(),
        finding.severity.clone(),
        finding.path.clone(),
        finding.message.clone(),
        finding.suggested_action.clone(),
    )
}

fn run_eval_gates(app: &App, skill: &str) -> std::result::Result<Value, CommandFailure> {
    let skill_path = app.ctx.skill_path(skill);
    let triggers_path = skill_path.join("evals/triggers.jsonl");
    let tasks_path = skill_path.join("evals/tasks.jsonl");
    let mut gates = Vec::new();
    if triggers_path.is_file() {
        let output = app
            .ctx
            .state_dir
            .join(format!("tmp-eval-trigger-{}.json", Uuid::new_v4()));
        let result = app.cmd_skill_eval_trigger(&SkillEvalTriggerArgs {
            skill: skill.to_string(),
            agent: "codex".to_string(),
            cases: None,
            runs: 1,
            runner: EvalRunnerArg::Mock,
            output: Some(output.clone()),
        });
        remove_path_if_exists(&output).map_err(map_io)?;
        let (report, _) = result?;
        gates.push(json!({
            "gate": "trigger",
            "status": "passed",
            "summary": report["summary"].clone()
        }));
    }
    if tasks_path.is_file() {
        let output = app
            .ctx
            .state_dir
            .join(format!("tmp-eval-run-{}.json", Uuid::new_v4()));
        let result = app.cmd_skill_eval_run(&SkillEvalRunArgs {
            skill: skill.to_string(),
            agent: "codex".to_string(),
            baseline: EvalBaselineArg::NoSkill,
            workspace: None,
            cases: None,
            runs: 1,
            runner: EvalRunnerArg::Mock,
            dry_run: false,
            output: Some(output.clone()),
        });
        remove_path_if_exists(&output).map_err(map_io)?;
        let (report, _) = result?;
        gates.push(json!({
            "gate": "run",
            "status": "passed",
            "summary": report["summary"].clone()
        }));
    }
    if gates.is_empty() {
        gates.push(json!({
            "gate": "eval",
            "status": "skipped",
            "reason": "no eval fixtures present"
        }));
    }
    Ok(json!({
        "status": "passed",
        "gates": gates
    }))
}

fn capture_preimages(
    ctx: &AppContext,
    changes: &[ParsedPatchChange],
) -> std::result::Result<Vec<Preimage>, CommandFailure> {
    let mut preimages = Vec::new();
    for change in changes {
        let path = ctx.root.join(&change.rel);
        let bytes = if path.exists() {
            Some(fs::read(&path).map_err(map_io)?)
        } else {
            None
        };
        preimages.push(Preimage {
            rel: change.rel.clone(),
            bytes,
        });
    }
    Ok(preimages)
}

fn materialize_changes(
    ctx: &AppContext,
    changes: &[ParsedPatchChange],
) -> std::result::Result<(), CommandFailure> {
    for change in changes {
        ensure_safe_registry_path(ctx, &change.rel)?;
        write_change(&ctx.root, change)?;
    }
    Ok(())
}

fn write_change(
    root: &Path,
    change: &ParsedPatchChange,
) -> std::result::Result<(), CommandFailure> {
    let path = root.join(&change.rel);
    write_atomic(&path, &change.body).map_err(map_io)
}

fn restore_preimages(ctx: &AppContext, preimages: &[Preimage]) {
    for preimage in preimages {
        let path = ctx.root.join(&preimage.rel);
        match &preimage.bytes {
            Some(bytes) => {
                if let Ok(raw) = String::from_utf8(bytes.clone()) {
                    let _ = write_atomic(&path, &raw);
                } else if let Some(parent) = path.parent()
                    && fs::create_dir_all(parent).is_ok()
                {
                    let _ = fs::write(&path, bytes);
                }
            }
            None => {
                let _ = remove_path_if_exists(&path);
                cleanup_empty_parents(ctx, &preimage.rel);
            }
        }
    }
}

fn cleanup_empty_parents(ctx: &AppContext, rel: &Path) {
    let stop = ctx.root.join("skills");
    let mut current = ctx.root.join(rel).parent().map(Path::to_path_buf);
    while let Some(path) = current {
        if path == stop || path == ctx.root {
            break;
        }
        if fs::remove_dir(&path).is_err() {
            break;
        }
        current = path.parent().map(Path::to_path_buf);
    }
}

fn reset_staged_paths(ctx: &AppContext, paths: &[String]) -> anyhow::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["reset", "HEAD", "--"];
    args.extend(paths.iter().map(String::as_str));
    gitops::run_git(ctx, &args).map(|_| ())
}

fn skill_apply_record_path(ctx: &AppContext, key_digest: &str) -> PathBuf {
    let suffix = key_digest.strip_prefix("sha256:").unwrap_or(key_digest);
    ctx.state_dir
        .join("patches/apply-records")
        .join(format!("{suffix}.json"))
}

fn replay_apply_record(
    path: &Path,
    patch_id: &str,
    patch_digest: Option<&str>,
    key_digest: &str,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    let record: Value = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("invalid skill apply record '{}': {err}", path.display()),
        )
    })?;
    let digest_mismatch =
        patch_digest.is_some_and(|digest| record["patch_digest"] != json!(digest));
    if record["schema"] != json!(APPLY_RECORD_SCHEMA)
        || record["patch_id"] != json!(patch_id)
        || digest_mismatch
    {
        let mut failure = CommandFailure::new(
            ErrorCode::ReplayConflict,
            "idempotency key was already used for a different skill patch apply",
        );
        failure.details = json!({
            "idempotency_key_digest": key_digest,
            "record": record,
        });
        return Err(failure);
    }
    let mut response = record["response"].clone();
    response["replayed"] = json!(true);
    Ok((response, Meta::default()))
}

fn write_apply_record(path: &Path, response: &Value) -> std::result::Result<(), CommandFailure> {
    let raw = serde_json::to_string_pretty(&json!({
        "schema": APPLY_RECORD_SCHEMA,
        "patch_id": response["patch_id"],
        "patch_digest": response["patch_digest"],
        "commit": response["commit"],
        "response": response,
        "created_at": Utc::now().to_rfc3339(),
    }))
    .map_err(|err| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("failed to serialize skill apply record: {err}"),
        )
    })?;
    write_atomic(path, &(raw + "\n")).map_err(map_io)
}

fn skill_apply_response(
    artifact: &SkillPatchArtifact,
    key_digest: &str,
    patch_digest: &str,
    commit: &str,
    applied_source_digest: &str,
    replayed: bool,
    validation: &ApplyValidationReport,
) -> Value {
    json!({
        "schema": APPLY_RECORD_SCHEMA,
        "patch_id": artifact.patch_id,
        "skill": artifact.skill,
        "applied": true,
        "replayed": replayed,
        "commit": commit,
        "idempotency_key_digest": key_digest,
        "patch_digest": patch_digest,
        "source_digest": {
            "before": artifact.source_digest,
            "after": applied_source_digest
        },
        "source_ref": artifact.source_ref,
        "files": artifact.files.iter().map(|file| {
            json!({"path": file.path, "change": file.change})
        }).collect::<Vec<_>>(),
        "validation": validation,
        "recovery": {
            "recorded": true,
            "artifact": format!("state/patches/{}.json", artifact.patch_id),
            "replay": "rerun with the same idempotency key to retrieve this result"
        }
    })
}
