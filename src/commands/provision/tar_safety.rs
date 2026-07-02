use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::commands::CommandFailure;
use crate::commands::helpers::{map_arg, map_git, map_io, validate_skill_name};
use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::model::ProvisionPlan;

pub(super) fn active_skill_ids(plan: &ProvisionPlan) -> Result<BTreeSet<String>, CommandFailure> {
    let mut skills = BTreeSet::new();
    for view in &plan.active_views {
        for skill in &view.skills {
            validate_skill_name(skill).map_err(map_arg)?;
            skills.insert(skill.clone());
        }
    }
    Ok(skills)
}

pub(super) fn ensure_reviewed_registry_source(
    ctx: &AppContext,
    plan: &ProvisionPlan,
    skills: &BTreeSet<String>,
) -> Result<(), CommandFailure> {
    let reviewed_head = required_guard_string(plan, "registry_head")?;
    let reachable = plan
        .guards
        .get("registry_head_reachable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !reachable || reviewed_head == "working-tree" {
        return Err(provision_policy_blocked(
            "provision tar export requires a reachable reviewed registry head",
            json!({
                "registry_head": reviewed_head,
                "registry_head_reachable": reachable,
            }),
        ));
    }

    let current_head = gitops::head(ctx).map_err(map_git)?;
    if current_head != reviewed_head {
        return Err(provision_policy_blocked(
            "provision plan registry head is stale; create a new provision plan",
            json!({
                "expected": reviewed_head,
                "actual": current_head,
            }),
        ));
    }

    for skill in skills {
        ensure_skill_source_clean(ctx, skill)?;
    }
    Ok(())
}

pub(super) fn reject_output_inside_skill_sources(
    ctx: &AppContext,
    output: &Path,
    skills: &BTreeSet<String>,
) -> Result<(), CommandFailure> {
    let output_parent = output.parent().unwrap_or_else(|| Path::new("."));
    let output_parent = normalize_provision_output_parent(output_parent)?;
    let registry_state = ctx
        .root
        .join("state")
        .canonicalize()
        .unwrap_or(ctx.root.join("state"));
    if output_parent.starts_with(&registry_state) {
        return Err(provision_policy_blocked(
            "provision tar output path must not be inside private registry state",
            json!({"output": output.display().to_string()}),
        ));
    }

    for skill in skills {
        let skill_root = ctx.skill_path(skill);
        let skill_root = canonicalize_existing_output_prefix(&skill_root)?;
        if output_parent.starts_with(&skill_root) {
            return Err(provision_policy_blocked(
                "provision tar output path must not be inside packaged skill source",
                json!({"output": output.display().to_string(), "skill": skill}),
            ));
        }
    }
    Ok(())
}

pub(super) fn reject_forbidden_source_content(
    ctx: &AppContext,
    archive_path: &str,
    bytes: &[u8],
) -> Result<(), CommandFailure> {
    let lower_path = archive_path.to_ascii_lowercase();
    if lower_path.contains("state/registry")
        || lower_path.contains("/.git/")
        || lower_path.ends_with(".env")
        || lower_path.contains("credentials")
        || lower_path.contains("settings.local")
    {
        return Err(provision_policy_blocked(
            "provision tar source includes private registry state or user-specific config",
            json!({"path": archive_path}),
        ));
    }

    let text = String::from_utf8_lossy(bytes);
    let lower = text.to_ascii_lowercase();
    let root = ctx.root.display().to_string();
    if (!root.is_empty() && text.contains(&root))
        || lower.contains("/users/")
        || lower.contains("/home/")
        || lower.contains("c:\\")
    {
        return Err(provision_policy_blocked(
            "provision tar source includes a local absolute path",
            json!({"path": archive_path}),
        ));
    }

    for needle in [
        "token=",
        "password=",
        "secret=",
        "api_key=",
        "begin private key",
    ] {
        if lower.contains(needle) {
            return Err(provision_policy_blocked(
                "provision tar source includes secret-looking material",
                json!({"path": archive_path, "pattern": needle}),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn reject_source_hardlink(path: &Path) -> Result<(), CommandFailure> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(path).map_err(map_io)?;
    if metadata.nlink() > 1 {
        return Err(provision_policy_blocked(
            "provision tar source contains a hardlink; hardlink exports are not supported",
            json!({"path": path.display().to_string()}),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn reject_source_hardlink(_path: &Path) -> Result<(), CommandFailure> {
    Ok(())
}

fn ensure_skill_source_clean(ctx: &AppContext, skill: &str) -> Result<(), CommandFailure> {
    let path = format!("skills/{skill}");
    let status = gitops::run_git(
        ctx,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            &path,
        ],
    )
    .map_err(map_git)?;
    if !status.trim().is_empty() {
        return Err(provision_policy_blocked(
            "provision tar export requires clean reviewed skill sources",
            json!({
                "skill": skill,
                "status": status.lines().next().unwrap_or_default(),
            }),
        ));
    }
    Ok(())
}

fn required_guard_string(plan: &ProvisionPlan, key: &str) -> Result<String, CommandFailure> {
    plan.guards
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("provision plan is missing {key} guard"),
            )
        })
}

fn normalize_provision_output_parent(path: &Path) -> Result<PathBuf, CommandFailure> {
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) && !path.is_absolute()
    {
        return Err(provision_policy_blocked(
            "provision tar output path must not contain parent directory components",
            json!({"output_parent": path.display().to_string()}),
        ));
    }
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(map_io)?.join(path)
    };
    canonicalize_existing_output_prefix(&candidate)
}

fn canonicalize_existing_output_prefix(path: &Path) -> Result<PathBuf, CommandFailure> {
    if path.exists() {
        return path.canonicalize().map_err(map_io);
    }
    let mut suffix = PathBuf::new();
    let mut cursor = path;
    loop {
        if cursor.exists() {
            let mut normalized = cursor.canonicalize().map_err(map_io)?;
            if !suffix.as_os_str().is_empty() {
                normalized.push(suffix);
            }
            return Ok(normalized);
        }
        let Some(name) = cursor.file_name() else {
            return Ok(path.to_path_buf());
        };
        suffix = Path::new(name).join(suffix);
        let Some(parent) = cursor.parent() else {
            return Ok(path.to_path_buf());
        };
        cursor = parent;
    }
}

fn provision_policy_blocked(message: &str, details: Value) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    failure.details = details;
    failure
}
