use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::cli::EvalRunnerArg;
use crate::fs_util::write_atomic;
use crate::gitops::{run_git, run_git_allow_failure};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::{map_arg, validate_skill_name};
use super::super::skill_eval::skill_eval_version;

pub(super) fn require_skill(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<PathBuf, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let skill_path = ctx.skill_path(skill);
    if !skill_path.is_dir() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }
    Ok(skill_path)
}

pub(super) fn resolve_cases_path(
    skill_path: &Path,
    explicit: Option<&Path>,
    default_rel: &str,
) -> PathBuf {
    match explicit {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => skill_path.join(path),
        None => skill_path.join(default_rel),
    }
}

pub(super) fn normalize_runs(runs: u32) -> std::result::Result<u32, CommandFailure> {
    if runs == 0 {
        Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--runs must be at least 1",
        ))
    } else {
        Ok(runs)
    }
}

pub(crate) fn persist_report(
    ctx: &AppContext,
    skill: &str,
    mode: &str,
    output_path: Option<&Path>,
    report: &mut Value,
) -> std::result::Result<PathBuf, CommandFailure> {
    let path = output_path
        .map(|path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                ctx.root.join(path)
            }
        })
        .unwrap_or_else(|| default_report_path(ctx, skill, mode));
    report["report_path"] = json!(path.display().to_string());
    let raw = serde_json::to_string_pretty(report).map_err(|err| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("failed to serialize eval report: {err}"),
        )
    })?;
    write_atomic(&path, &(raw + "\n"))
        .map_err(|err| io_failure("eval_report_write", &path, err))?;
    Ok(path)
}

pub(super) fn default_report_path(ctx: &AppContext, skill: &str, mode: &str) -> PathBuf {
    ctx.state_dir
        .join("registry/evals")
        .join(skill)
        .join(format!("{mode}-latest.json"))
}

pub(super) fn ensure_runner_available(
    runner: EvalRunnerArg,
) -> std::result::Result<(), CommandFailure> {
    match runner {
        EvalRunnerArg::Mock => Ok(()),
        EvalRunnerArg::CodexCli => {
            if !executable_in_path("codex") {
                return Err(eval_failed(
                    "codex-cli runner executable not found",
                    0,
                    "runner_executable_missing",
                    json!({"runner": "codex-cli"}),
                ));
            }
            if std::env::var("LOOM_EVAL_ALLOW_CODEX_CLI").ok().as_deref() != Some("1") {
                return Err(eval_failed(
                    "codex-cli runner requires LOOM_EVAL_ALLOW_CODEX_CLI=1",
                    0,
                    "runner_authorization_missing",
                    json!({"runner": "codex-cli"}),
                ));
            }
            Err(eval_failed(
                "codex-cli runner execution is not implemented in this safe harness slice",
                0,
                "runner_unsupported",
                json!({"runner": "codex-cli"}),
            ))
        }
    }
}

pub(super) fn resolve_compare_version(
    ctx: &AppContext,
    skill: &str,
    reference: &str,
) -> std::result::Result<Value, CommandFailure> {
    if reference == "working-tree" {
        let version = skill_eval_version(ctx, skill);
        return Ok(json!({
            "source_ref": reference,
            "head_tree_oid": version.head_tree_oid,
            "last_source_commit": version.last_source_commit,
        }));
    }
    let commit = run_git(
        ctx,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )
    .map_err(|err| {
        eval_failed(
            "compare ref could not be resolved",
            0,
            "compare_ref_invalid",
            json!({"ref": reference, "error": err.to_string()}),
        )
    })?;
    let skill_rel = format!("skills/{skill}");
    let tree =
        run_git(ctx, &["rev-parse", &format!("{reference}:{skill_rel}")]).map_err(|err| {
            eval_failed(
                "compare ref does not contain the skill source",
                0,
                "compare_skill_missing",
                json!({"ref": reference, "skill": skill, "error": err.to_string()}),
            )
        })?;
    let last = run_git_allow_failure(
        ctx,
        &["log", "-1", "--format=%H", reference, "--", &skill_rel],
    )
    .ok()
    .and_then(|output| {
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    })
    .filter(|value| !value.is_empty());
    Ok(json!({
        "source_ref": reference,
        "commit": commit,
        "head_tree_oid": tree,
        "last_source_commit": last,
    }))
}

pub(super) fn security_model() -> Value {
    json!({
        "eval_success_is_safety_guarantee": false,
        "note": "Eval success is quality evidence only. It does not prove the skill is safe."
    })
}

pub(super) fn runner_id(runner: EvalRunnerArg) -> &'static str {
    match runner {
        EvalRunnerArg::Mock => "mock",
        EvalRunnerArg::CodexCli => "codex-cli",
    }
}

pub(super) fn cleanup_to_value(cleanup: &super::runner::CleanupResult) -> Value {
    json!({
        "status": if cleanup.failed() { "failed" } else { "passed" },
        "message": cleanup.message,
    })
}

pub(super) fn eval_failed(
    message: &str,
    failed: u64,
    failure_kind: &str,
    report: Value,
) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::EvalFailed, message);
    failure.details = json!({
        "failed": failed,
        "failure_kind": failure_kind,
        "report": report,
    });
    failure
}

pub(super) fn harness_schema_failure(
    message: impl Into<String>,
    path: &Path,
    line: usize,
) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::SchemaMismatch, message);
    failure.details = json!({
        "path": path.display().to_string(),
        "line": line,
    });
    failure
}

pub(super) fn io_failure(tag: &str, path: &Path, err: std::io::Error) -> CommandFailure {
    let mut failure = CommandFailure::new(
        ErrorCode::IoError,
        format!("{} failed for '{}': {err}", tag, path.display()),
    );
    failure.details = json!({
        "path": path.display().to_string(),
        "failure_kind": tag,
    });
    failure
}

pub(super) fn harness_ratio(numerator: usize, denominator: usize) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

pub(super) fn option_delta(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    Some(left? - right?)
}

fn executable_in_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}
