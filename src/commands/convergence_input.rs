use std::fs;
use std::path::{Path, PathBuf};

use tar::Archive;
use uuid::Uuid;

use crate::core::convergence::{ProjectionInputEvidence, ProjectionInputState};
use crate::core::vocab::ProjectionMethod;
use crate::gitops;
use crate::state::AppContext;
use crate::state_model::RegistryProjectionInstance;

use super::CommandFailure;
use super::codex_visibility::projection_path_is_safe_symlink;
use super::helpers::{map_git, map_io};
use super::provenance::{materialized_tree_digest, skill_tree_digest};

pub(crate) fn source_dirty_paths(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<Vec<String>, CommandFailure> {
    let prefix = format!("skills/{skill}");
    let mut paths = Vec::new();
    if git_head_exists(ctx)? {
        collect_git_paths(
            ctx,
            &["diff", "--name-only", "HEAD", "--", &prefix],
            &mut paths,
        )?;
        collect_git_paths(
            ctx,
            &["diff", "--name-only", "--cached", "--", &prefix],
            &mut paths,
        )?;
    }
    collect_git_paths(
        ctx,
        &["ls-files", "--others", "--exclude-standard", "--", &prefix],
        &mut paths,
    )?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub(crate) fn projection_input_evidence(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
) -> std::result::Result<ProjectionInputEvidence, CommandFailure> {
    let live_path = PathBuf::from(&projection.materialized_path);
    let mut evidence = ProjectionInputEvidence {
        instance_id: projection.instance_id.clone(),
        method: projection.method.as_str().to_string(),
        materialized_path: projection.materialized_path.clone(),
        baseline_revision: Some(projection.last_applied_rev.clone()),
        baseline_tree_digest: None,
        live_tree_digest: None,
        state: ProjectionInputState::Untracked,
        issue: None,
    };

    let metadata = match fs::symlink_metadata(&live_path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            evidence.state = ProjectionInputState::Missing;
            evidence.issue = Some("materialized_path_missing".to_string());
            return Ok(evidence);
        }
        Err(_) => {
            evidence.state = ProjectionInputState::Unreadable;
            evidence.issue = Some("materialized_path_unreadable".to_string());
            return Ok(evidence);
        }
    };

    if projection.method == ProjectionMethod::Symlink {
        evidence.state = if metadata.file_type().is_symlink()
            && projection_path_is_safe_symlink(&live_path, &ctx.skill_path(&projection.skill_id))
        {
            ProjectionInputState::SourceLinked
        } else {
            evidence.issue = Some("symlink_target_mismatch".to_string());
            ProjectionInputState::MetadataMismatch
        };
        return Ok(evidence);
    }
    if metadata.file_type().is_symlink() {
        evidence.state = ProjectionInputState::MetadataMismatch;
        evidence.issue = Some("copy_projection_is_symlink".to_string());
        return Ok(evidence);
    }
    if !metadata.is_dir() {
        evidence.state = ProjectionInputState::NotDirectory;
        evidence.issue = Some("materialized_path_not_directory".to_string());
        return Ok(evidence);
    }

    evidence.live_tree_digest = match digest_for_method(&live_path, projection.method) {
        Ok(digest) => Some(digest),
        Err(_) => {
            evidence.state = ProjectionInputState::Unreadable;
            evidence.issue = Some("materialized_tree_unreadable".to_string());
            return Ok(evidence);
        }
    };
    evidence.baseline_tree_digest = match baseline_tree_digest(ctx, projection, projection.method)?
    {
        Ok(digest) => Some(digest),
        Err(issue) => {
            evidence.state = ProjectionInputState::BaselineUnavailable;
            evidence.issue = Some(issue);
            return Ok(evidence);
        }
    };
    evidence.state = if evidence.live_tree_digest == evidence.baseline_tree_digest {
        ProjectionInputState::Clean
    } else {
        ProjectionInputState::Dirty
    };
    Ok(evidence)
}

fn digest_for_method(path: &Path, method: ProjectionMethod) -> anyhow::Result<String> {
    match method {
        ProjectionMethod::Materialize => materialized_tree_digest(path),
        ProjectionMethod::Copy | ProjectionMethod::Symlink => skill_tree_digest(path),
    }
}

fn baseline_tree_digest(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
    method: ProjectionMethod,
) -> std::result::Result<Result<String, String>, CommandFailure> {
    let temp_root = std::env::temp_dir().join(format!(
        "loom-convergence-baseline-{}",
        Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&temp_root).map_err(map_io)?;
    let result = materialize_skill_at_ref(
        ctx,
        &projection.skill_id,
        &projection.last_applied_rev,
        &temp_root,
    )
    .and_then(|()| {
        digest_for_method(&temp_root.join("skills").join(&projection.skill_id), method)
            .map_err(|_| "baseline_tree_unreadable".to_string())
    });
    fs::remove_dir_all(&temp_root).map_err(map_io)?;
    Ok(result)
}

fn materialize_skill_at_ref(
    ctx: &AppContext,
    skill: &str,
    reference: &str,
    root: &Path,
) -> Result<(), String> {
    let skill_rel = format!("skills/{skill}");
    let output = gitops::run_git_allow_failure(
        ctx,
        &["archive", "--format=tar", reference, "--", &skill_rel],
    )
    .map_err(|_| "baseline_git_archive_failed".to_string())?;
    if !output.status.success() {
        return Err("baseline_revision_unavailable".to_string());
    }
    Archive::new(&output.stdout[..])
        .unpack(root)
        .map_err(|_| "baseline_archive_unreadable".to_string())
}

fn collect_git_paths(
    ctx: &AppContext,
    args: &[&str],
    paths: &mut Vec<String>,
) -> std::result::Result<(), CommandFailure> {
    let stdout = gitops::run_git(ctx, args).map_err(map_git)?;
    paths.extend(
        stdout
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string),
    );
    Ok(())
}

fn git_head_exists(ctx: &AppContext) -> std::result::Result<bool, CommandFailure> {
    let output =
        gitops::run_git_allow_failure(ctx, &["rev-parse", "--verify", "HEAD"]).map_err(map_git)?;
    Ok(output.status.success())
}
