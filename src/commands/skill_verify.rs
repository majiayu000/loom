use std::fs;

use anyhow::Result;
use serde_json::json;

use crate::cli::SkillOnlyArgs;
use crate::envelope::Meta;
use crate::gitops::{run_git, run_git_allow_failure};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_git, validate_skill_name};
use super::{App, CommandFailure};

const REGISTRY_OPERATIONS_LOG: &str = "state/registry/ops/operations.jsonl";

impl App {
    pub fn cmd_verify(
        &self,
        args: &SkillOnlyArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let skill_rel = format!("skills/{}", args.skill);
        let skill_path = self.ctx.root.join(&skill_rel);
        if !skill_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        let head_tree_oid = head_tree_oid_for_path(&self.ctx, &skill_rel).map_err(map_git)?;
        let last_source_commit = last_saved_commit_for_skill(&self.ctx, &args.skill)
            .and_then(|commit| {
                if commit.is_some() {
                    Ok(commit)
                } else {
                    last_commit_for_path(&self.ctx, &skill_rel)
                }
            })
            .map_err(map_git)?;
        let drifted_paths =
            drifted_paths_under(&self.ctx, &skill_rel, last_source_commit.as_deref())
                .map_err(map_git)?;
        let matches = drifted_paths.is_empty() && head_tree_oid.is_some();

        Ok((
            json!({
                "skill": args.skill,
                "matches": matches,
                "head_tree_oid": head_tree_oid,
                "last_source_commit": last_source_commit,
                "drifted_paths": drifted_paths,
            }),
            Meta::default(),
        ))
    }
}

fn last_saved_commit_for_skill(ctx: &AppContext, skill: &str) -> Result<Option<String>> {
    let Some(op_id) = last_save_op_id_for_skill(ctx, skill)? else {
        return Ok(None);
    };
    commit_that_introduced_registry_op(ctx, &op_id)
}

fn last_save_op_id_for_skill(ctx: &AppContext, skill: &str) -> Result<Option<String>> {
    let path = ctx.root.join(REGISTRY_OPERATIONS_LOG);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    last_save_op_id_in_log(&raw, skill)
}

fn last_save_op_id_in_log(raw: &str, skill: &str) -> Result<Option<String>> {
    let mut last_op_id = None;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(trimmed)?;
        if value.get("intent").and_then(|value| value.as_str()) == Some("skill.save")
            && value
                .get("payload")
                .and_then(|payload| payload.get("skill"))
                .and_then(|value| value.as_str())
                == Some(skill)
            && let Some(op_id) = value.get("op_id").and_then(|value| value.as_str())
        {
            last_op_id = Some(op_id.to_string());
        }
    }
    Ok(last_op_id)
}

fn commit_that_introduced_registry_op(ctx: &AppContext, op_id: &str) -> Result<Option<String>> {
    let stdout = run_git(ctx, &["log", "--format=%H", "--", REGISTRY_OPERATIONS_LOG])?;
    let mut introducing_commit = None;
    for commit in stdout
        .lines()
        .map(str::trim)
        .filter(|commit| !commit.is_empty())
    {
        if operation_log_at_commit_contains(ctx, commit, op_id)? {
            introducing_commit = Some(commit.to_string());
        } else if introducing_commit.is_some() {
            break;
        }
    }
    Ok(introducing_commit)
}

fn operation_log_at_commit_contains(ctx: &AppContext, commit: &str, op_id: &str) -> Result<bool> {
    let spec = format!("{commit}:{REGISTRY_OPERATIONS_LOG}");
    let output = run_git_allow_failure(ctx, &["show", &spec])?;
    if !output.status.success() {
        return Ok(false);
    }
    operation_log_contains_op_id(&String::from_utf8_lossy(&output.stdout), op_id)
}

fn operation_log_contains_op_id(raw: &str, op_id: &str) -> Result<bool> {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(trimmed)?;
        if value.get("op_id").and_then(|value| value.as_str()) == Some(op_id) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn head_tree_oid_for_path(ctx: &AppContext, path: &str) -> Result<Option<String>> {
    let output = run_git_allow_failure(ctx, &["rev-parse", &format!("HEAD:{path}")])?;
    if output.status.success() {
        let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if oid.is_empty() {
            Ok(None)
        } else {
            Ok(Some(oid))
        }
    } else {
        Ok(None)
    }
}

fn last_commit_for_path(ctx: &AppContext, path: &str) -> Result<Option<String>> {
    let stdout = run_git(ctx, &["log", "-1", "--format=%H", "--", path])?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn drifted_paths_under(
    ctx: &AppContext,
    prefix: &str,
    base_commit: Option<&str>,
) -> Result<Vec<String>> {
    // run_git trims the entire stdout, which mangles porcelain's fixed-offset
    // status prefix. Use --name-only outputs that contain only paths, so the
    // result is stable under the trim.
    let mut paths: Vec<String> = Vec::new();
    let tracked = run_git(
        ctx,
        &[
            "diff",
            "--name-only",
            base_commit.unwrap_or("HEAD"),
            "--",
            prefix,
        ],
    )?;
    for line in tracked.lines() {
        let p = line.trim();
        if !p.is_empty() && !paths.iter().any(|x| x == p) {
            paths.push(p.to_string());
        }
    }
    let staged = run_git(ctx, &["diff", "--name-only", "--cached", "--", prefix])?;
    for line in staged.lines() {
        let p = line.trim();
        if !p.is_empty() && !paths.iter().any(|x| x == p) {
            paths.push(p.to_string());
        }
    }
    let untracked = run_git(
        ctx,
        &["ls-files", "--others", "--exclude-standard", "--", prefix],
    )?;
    for line in untracked.lines() {
        let p = line.trim();
        if !p.is_empty() && !paths.iter().any(|x| x == p) {
            paths.push(p.to_string());
        }
    }
    paths.sort();
    Ok(paths)
}
