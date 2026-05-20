use anyhow::Result;
use serde_json::json;

use crate::cli::SkillOnlyArgs;
use crate::envelope::Meta;
use crate::gitops::{run_git, run_git_allow_failure};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_git, validate_skill_name};
use super::{App, CommandFailure};

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
        let last_source_commit = last_commit_for_path(&self.ctx, &skill_rel).map_err(map_git)?;
        let drifted_paths = drifted_paths_under(&self.ctx, &skill_rel).map_err(map_git)?;
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

fn drifted_paths_under(ctx: &AppContext, prefix: &str) -> Result<Vec<String>> {
    // run_git trims the entire stdout, which mangles porcelain's fixed-offset
    // status prefix. Use --name-only outputs that contain only paths, so the
    // result is stable under the trim.
    let mut paths: Vec<String> = Vec::new();
    let tracked = run_git(ctx, &["diff", "--name-only", "HEAD", "--", prefix])?;
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
