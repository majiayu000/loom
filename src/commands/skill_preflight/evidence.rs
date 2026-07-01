use std::path::Path;

use serde_json::{Value, json};

use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::{map_arg, map_git, validate_skill_name};

pub(super) fn reject_release_candidate_baseline(
    ctx: &AppContext,
    baseline: &str,
) -> std::result::Result<(), CommandFailure> {
    let baseline_oid = gitops::resolve_ref(ctx, baseline).map_err(map_git)?;
    let head_oid = gitops::resolve_ref(ctx, "HEAD").map_err(map_git)?;
    if baseline_oid == head_oid {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "release preflight baseline must not resolve to the release candidate ref",
        ));
    }
    Ok(())
}

pub(super) fn ensure_skill_in_ref(
    ctx: &AppContext,
    skill: &str,
    reference: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let spec = format!("{reference}:skills/{skill}/SKILL.md");
    let output = gitops::run_git_allow_failure(ctx, &["cat-file", "-e", &spec]).map_err(map_git)?;
    if output.status.success() {
        return Ok(());
    }
    Err(CommandFailure::new(
        ErrorCode::SkillNotFound,
        format!("skill '{}' not found in ref '{}'", skill, reference),
    ))
}

pub(super) fn ensure_selected_skill_clean(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let rel = format!("skills/{skill}");
    if gitops::diff_has_changes_from_ref(ctx, "HEAD", Path::new(&rel)).map_err(map_git)? {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "skill release --preflight requires the selected skill to be clean",
        ));
    }
    let (_, untracked_count, _) = working_tree_untracked_paths(ctx, Path::new(&rel), 1)?;
    if untracked_count > 0 {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "skill release --preflight requires the selected skill to be clean",
        ));
    }
    Ok(())
}

pub(super) fn drift_report(
    ctx: &AppContext,
    baseline: &str,
    target: &str,
    pathspec: &Path,
) -> std::result::Result<Value, CommandFailure> {
    let mut changed = diff_has_changes(ctx, baseline, target, pathspec)?;
    let mut stat = if target == "working-tree" {
        gitops::diff_shortstat_from_ref(ctx, baseline, pathspec).map_err(map_git)?
    } else {
        diff_shortstat_between(ctx, baseline, target, pathspec)?
    };
    let (mut paths, mut truncated) = if target == "working-tree" {
        gitops::diff_changed_paths_from_ref(ctx, baseline, pathspec, 100).map_err(map_git)?
    } else {
        diff_changed_paths_between(ctx, baseline, target, pathspec, 100)?
    };
    let (untracked_paths, untracked_path_count, untracked_truncated) = if target == "working-tree" {
        working_tree_untracked_paths(ctx, pathspec, 100)?
    } else {
        (Vec::new(), 0, false)
    };

    changed = changed || untracked_path_count > 0;
    stat.files_changed = stat
        .files_changed
        .saturating_add(u32::try_from(untracked_path_count).unwrap_or(u32::MAX));
    for path in &untracked_paths {
        if paths.iter().any(|existing| existing == path) {
            continue;
        }
        if paths.len() < 100 {
            paths.push(path.clone());
        } else {
            truncated = true;
            break;
        }
    }
    truncated = truncated || untracked_truncated;

    Ok(json!({
        "changed": changed,
        "files_changed": stat.files_changed,
        "insertions": stat.insertions,
        "deletions": stat.deletions,
        "changed_paths": paths,
        "untracked_paths": untracked_paths,
        "untracked_path_count": untracked_path_count,
        "changed_paths_truncated": truncated
    }))
}

pub(super) fn baseline_eval_evidence_report(
    ctx: &AppContext,
    skill: &str,
    baseline: &str,
    target: &str,
) -> std::result::Result<Value, CommandFailure> {
    let eval_rel = format!("skills/{skill}/evals");
    let baseline_paths = eval_evidence_paths_in_ref(ctx, baseline, &eval_rel)?;
    if baseline_paths.is_empty() {
        return Ok(json!({
            "status": "skipped",
            "reason": "baseline has no eval evidence",
            "baseline_paths": baseline_paths,
        }));
    }

    let eval_pathspec = Path::new(&eval_rel);
    let mut changed = diff_has_changes(ctx, baseline, target, eval_pathspec)?;
    let mut untracked_paths = Vec::new();
    if target == "working-tree" {
        let (paths, count, _) = working_tree_untracked_paths(ctx, eval_pathspec, 20)?;
        changed = changed || count > 0;
        untracked_paths = paths;
    }
    let target_paths = if target == "working-tree" {
        eval_evidence_paths_in_working_tree(ctx, &eval_rel)?
    } else {
        eval_evidence_paths_in_ref(ctx, target, &eval_rel)?
    };
    let missing_paths = baseline_paths
        .iter()
        .filter(|path| !target_paths.iter().any(|candidate| candidate == *path))
        .cloned()
        .collect::<Vec<_>>();

    Ok(json!({
        "status": if changed { "fail" } else { "pass" },
        "reason": if changed {
            "baseline eval evidence changed; regression checks must not trust edited target fixtures"
        } else {
            "baseline eval evidence is preserved"
        },
        "baseline_paths": baseline_paths,
        "target_paths": target_paths,
        "missing_paths": missing_paths,
        "untracked_paths": untracked_paths,
    }))
}

pub(super) fn security_diff_status(report: &Value) -> &'static str {
    if report["status"].as_str() == Some("skipped") {
        return "skipped";
    }
    let summary = &report["summary"];
    let high = summary["critical"].as_u64().unwrap_or(0) + summary["high"].as_u64().unwrap_or(0);
    let review =
        high + summary["medium"].as_u64().unwrap_or(0) + summary["low"].as_u64().unwrap_or(0);
    if high > 0 {
        "fail"
    } else if review > 0 {
        "warning"
    } else {
        "pass"
    }
}

fn diff_has_changes(
    ctx: &AppContext,
    baseline: &str,
    target: &str,
    pathspec: &Path,
) -> std::result::Result<bool, CommandFailure> {
    if target == "working-tree" {
        return gitops::diff_has_changes_from_ref(ctx, baseline, pathspec).map_err(map_git);
    }
    let path = pathspec.to_string_lossy();
    let output =
        gitops::run_git_allow_failure(ctx, &["diff", "--quiet", baseline, target, "--", &path])
            .map_err(map_git)?;
    if output.status.success() {
        Ok(false)
    } else if output.status.code() == Some(1) {
        Ok(true)
    } else {
        Err(map_git(anyhow::anyhow!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn diff_shortstat_between(
    ctx: &AppContext,
    baseline: &str,
    target: &str,
    pathspec: &Path,
) -> std::result::Result<gitops::DiffShortStat, CommandFailure> {
    let path = pathspec.to_string_lossy();
    let output = gitops::run_git(ctx, &["diff", "--shortstat", baseline, target, "--", &path])
        .map_err(map_git)?;
    gitops::parse_diff_shortstat(&output).map_err(map_git)
}

fn diff_changed_paths_between(
    ctx: &AppContext,
    baseline: &str,
    target: &str,
    pathspec: &Path,
    limit: usize,
) -> std::result::Result<(Vec<String>, bool), CommandFailure> {
    let path = pathspec.to_string_lossy();
    let output = gitops::run_git(ctx, &["diff", "--name-only", baseline, target, "--", &path])
        .map_err(map_git)?;
    let paths = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let truncated = paths.len() > limit;
    Ok((paths.into_iter().take(limit).collect(), truncated))
}

fn working_tree_untracked_paths(
    ctx: &AppContext,
    pathspec: &Path,
    limit: usize,
) -> std::result::Result<(Vec<String>, usize, bool), CommandFailure> {
    let path = pathspec.to_string_lossy();
    let output = gitops::run_git(
        ctx,
        &["ls-files", "--others", "--exclude-standard", "--", &path],
    )
    .map_err(map_git)?;
    let paths = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let count = paths.len();
    let truncated = count > limit;
    Ok((paths.into_iter().take(limit).collect(), count, truncated))
}

fn eval_evidence_paths_in_ref(
    ctx: &AppContext,
    reference: &str,
    eval_rel: &str,
) -> std::result::Result<Vec<String>, CommandFailure> {
    let output = gitops::run_git(
        ctx,
        &["ls-tree", "-r", "--name-only", reference, "--", eval_rel],
    )
    .map_err(map_git)?;
    Ok(eval_evidence_paths(output.lines()))
}

fn eval_evidence_paths_in_working_tree(
    ctx: &AppContext,
    eval_rel: &str,
) -> std::result::Result<Vec<String>, CommandFailure> {
    let tracked = gitops::run_git(ctx, &["ls-files", "--", eval_rel]).map_err(map_git)?;
    let untracked = gitops::run_git(
        ctx,
        &["ls-files", "--others", "--exclude-standard", "--", eval_rel],
    )
    .map_err(map_git)?;
    Ok(eval_evidence_paths(
        tracked.lines().chain(untracked.lines()),
    ))
}

fn eval_evidence_paths<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut paths = lines
        .map(str::trim)
        .filter(|path| {
            path.ends_with("/evals/tasks.jsonl") || path.ends_with("/evals/triggers.jsonl")
        })
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}
