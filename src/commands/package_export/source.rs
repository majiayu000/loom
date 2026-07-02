use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};
use walkdir::WalkDir;

use crate::state::AppContext;
use crate::types::ErrorCode;

use super::model::{CopyFile, PackageChecks, PackageFilePlan, PackageSource, PackageSourceMember};
use super::{CommandFailure, SkillLintMode, lint_skill_source};
use crate::commands::helpers::{map_arg, map_io, validate_skill_name};
use crate::commands::skill_safety::trust_metadata_for_skill;
use crate::commands::skillset_cmds::{SkillsetPackageSource, load_skillset_package_source};

pub(super) fn resolve_package_source(
    ctx: &AppContext,
    raw: &str,
) -> std::result::Result<PackageSource, CommandFailure> {
    let (kind, id) = if let Some(id) = raw.strip_prefix("skill:") {
        ("skill", id)
    } else if let Some(id) = raw.strip_prefix("skillset:") {
        ("skillset", id)
    } else {
        let skill_exists = ctx.skill_path(raw).is_dir();
        let skillset_exists = load_skillset_package_source(ctx, raw).is_ok();
        match (skill_exists, skillset_exists) {
            (true, true) => {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "source id is ambiguous; use skill:<id> or skillset:<id>",
                ));
            }
            (true, false) => ("skill", raw),
            (false, true) => ("skillset", raw),
            (false, false) => ("skill", raw),
        }
    };
    validate_skill_name(id).map_err(map_arg)?;
    let source = match kind {
        "skill" => {
            if !ctx.skill_path(id).is_dir() {
                return Err(CommandFailure::new(
                    ErrorCode::SkillNotFound,
                    format!("skill '{}' not found", id),
                ));
            }
            PackageSource {
                kind: "skill".to_string(),
                id: id.to_string(),
                description: None,
                members: vec![PackageSourceMember {
                    skill_id: id.to_string(),
                    role: None,
                    required: true,
                }],
            }
        }
        "skillset" => render_skillset_source(load_skillset_package_source(ctx, id)?)?,
        _ => unreachable!("validated package source kind"),
    };
    validate_source_metadata(ctx, &source)?;
    Ok(source)
}

fn render_skillset_source(
    source: SkillsetPackageSource,
) -> std::result::Result<PackageSource, CommandFailure> {
    if source.members.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("skillset '{}' has no members to package", source.id),
        ));
    }
    Ok(PackageSource {
        kind: "skillset".to_string(),
        id: source.id,
        description: source.description,
        members: source
            .members
            .into_iter()
            .map(|member| PackageSourceMember {
                skill_id: member.skill_id,
                role: member.role,
                required: member.required,
            })
            .collect(),
    })
}

pub(super) fn collect_source_files(
    ctx: &AppContext,
    source: &PackageSource,
) -> std::result::Result<Vec<CopyFile>, CommandFailure> {
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    for member in &source.members {
        if !seen.insert(member.skill_id.clone()) {
            continue;
        }
        validate_skill_name(&member.skill_id).map_err(map_arg)?;
        let skill_path = ctx.skill_path(&member.skill_id);
        if !skill_path.is_dir() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", member.skill_id),
            ));
        }
        collect_skill_files(ctx, &member.skill_id, &skill_path, &mut files)?;
    }
    files.sort_by(|left, right| left.archive_rel.cmp(&right.archive_rel));
    Ok(files)
}

fn collect_skill_files(
    ctx: &AppContext,
    skill: &str,
    skill_path: &Path,
    out: &mut Vec<CopyFile>,
) -> std::result::Result<(), CommandFailure> {
    for entry in WalkDir::new(skill_path)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
    {
        let entry = entry.map_err(map_io)?;
        let rel = entry.path().strip_prefix(skill_path).map_err(map_io)?;
        if rel.as_os_str().is_empty() || entry.file_type().is_dir() {
            continue;
        }
        validate_package_relative_path(rel)?;
        if entry.file_type().is_symlink() {
            return Err(package_policy_blocked(
                "package source contains a symlink; symlink exports are not supported",
                json!({"path": entry.path().display().to_string()}),
            ));
        }
        reject_hardlink(entry.path())?;
        let bytes = fs::read(entry.path()).map_err(map_io)?;
        reject_forbidden_content(ctx, &format!("skills/{skill}/{}", rel.display()), &bytes)?;
        out.push(CopyFile {
            archive_rel: format!(
                "skills/{skill}/{}",
                rel.to_string_lossy().replace('\\', "/")
            ),
            sha256: super::model::digest_bytes(&bytes),
            bytes,
        });
    }
    Ok(())
}

pub(super) fn package_checks(
    ctx: &AppContext,
    source: &PackageSource,
) -> std::result::Result<PackageChecks, CommandFailure> {
    for member in &source.members {
        let skill_path = ctx.skill_path(&member.skill_id);
        let lint = lint_skill_source(&skill_path, &member.skill_id, SkillLintMode::Strict);
        if !lint.valid {
            return Err(package_policy_blocked(
                "package source failed portable lint",
                json!({"skill": member.skill_id, "lint": lint}),
            ));
        }
        let trust = trust_metadata_for_skill(ctx, &member.skill_id)?;
        if trust.quarantined || trust.trust == "quarantined" || trust.trust == "blocked" {
            return Err(package_policy_blocked(
                "package source is blocked or quarantined",
                json!({"skill": member.skill_id, "trust": trust}),
            ));
        }
        if trust.trust == "third-party-unreviewed" {
            return Err(package_policy_blocked(
                "third-party-unreviewed package source requires an explicit draft/private policy",
                json!({"skill": member.skill_id, "trust": trust}),
            ));
        }
    }
    Ok(PackageChecks {
        portable_lint: "pass".to_string(),
        safety_scan: "not_run".to_string(),
        eval_gate: "not_required".to_string(),
        approval: "not_required".to_string(),
    })
}

pub(super) fn plan_files(files: &[CopyFile]) -> Vec<PackageFilePlan> {
    let mut planned = vec![
        generated_file("manifest.json"),
        generated_file("provenance.json"),
        generated_file("checksums.txt"),
    ];
    planned.extend(files.iter().map(|file| PackageFilePlan {
        path: file.archive_rel.clone(),
        kind: "copied".to_string(),
        size: file.bytes.len() as u64,
        sha256: file.sha256.clone(),
    }));
    planned
}

fn generated_file(path: &str) -> PackageFilePlan {
    PackageFilePlan {
        path: path.to_string(),
        kind: "generated".to_string(),
        size: 0,
        sha256: "pending".to_string(),
    }
}

pub(super) fn source_digest(source: &PackageSource, files: &[CopyFile]) -> String {
    let mut hasher = crate::sha256::Sha256::new();
    hash_source_metadata(&mut hasher, source);
    for file in files {
        hasher.update(file.archive_rel.as_bytes());
        hasher.update(&[0]);
        hasher.update(file.sha256.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", crate::sha256::to_hex(&hasher.finalize()))
}

pub(super) fn reject_output_inside_sources(
    ctx: &AppContext,
    output: &Path,
    source: &PackageSource,
) -> std::result::Result<(), CommandFailure> {
    let output_parent = output.parent().unwrap_or_else(|| Path::new("."));
    let output_parent = normalize_output_parent(output_parent)?;
    let registry_state = ctx
        .root
        .join("state")
        .canonicalize()
        .unwrap_or(ctx.root.join("state"));
    if output_parent.starts_with(&registry_state) {
        return Err(package_policy_blocked(
            "package output path must not be inside private registry state",
            json!({"output": output.display().to_string()}),
        ));
    }
    for member in &source.members {
        let skill_path = ctx.skill_path(&member.skill_id);
        let skill_path = skill_path.canonicalize().unwrap_or(skill_path);
        if output_parent.starts_with(skill_path) {
            return Err(package_policy_blocked(
                "package output path must not be inside packaged source",
                json!({"output": output.display().to_string(), "skill": member.skill_id}),
            ));
        }
    }
    Ok(())
}

fn validate_source_metadata(
    ctx: &AppContext,
    source: &PackageSource,
) -> std::result::Result<(), CommandFailure> {
    reject_forbidden_content(ctx, "package/source/id", source.id.as_bytes())?;
    if let Some(description) = &source.description {
        reject_forbidden_content(ctx, "package/source/description", description.as_bytes())?;
    }
    for member in &source.members {
        reject_forbidden_content(
            ctx,
            &format!("package/source/members/{}/skill_id", member.skill_id),
            member.skill_id.as_bytes(),
        )?;
        if let Some(role) = &member.role {
            reject_forbidden_content(
                ctx,
                &format!("package/source/members/{}/role", member.skill_id),
                role.as_bytes(),
            )?;
        }
    }
    Ok(())
}

fn hash_source_metadata(hasher: &mut crate::sha256::Sha256, source: &PackageSource) {
    hasher.update(source.kind.as_bytes());
    hasher.update(&[0]);
    hasher.update(source.id.as_bytes());
    hasher.update(&[0]);
    if let Some(description) = &source.description {
        hasher.update(description.as_bytes());
    }
    hasher.update(&[0]);
    for member in &source.members {
        hasher.update(member.skill_id.as_bytes());
        hasher.update(&[0]);
        if let Some(role) = &member.role {
            hasher.update(role.as_bytes());
        }
        hasher.update(&[0]);
        hasher.update(if member.required { b"1" } else { b"0" });
        hasher.update(b"\n");
    }
}

fn normalize_output_parent(path: &Path) -> std::result::Result<PathBuf, CommandFailure> {
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) && !path.is_absolute()
    {
        return Err(package_policy_blocked(
            "package output path must not contain parent directory components",
            json!({"output_parent": path.display().to_string()}),
        ));
    }
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(map_io)?.join(path)
    };
    canonicalize_existing_prefix(&candidate)
}

fn canonicalize_existing_prefix(path: &Path) -> std::result::Result<PathBuf, CommandFailure> {
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

pub(super) fn reject_forbidden_content(
    ctx: &AppContext,
    path: &str,
    bytes: &[u8],
) -> std::result::Result<(), CommandFailure> {
    let lower_path = path.to_ascii_lowercase();
    if lower_path.contains("state/registry")
        || lower_path.contains("/.git/")
        || lower_path.ends_with(".env")
        || lower_path.contains("settings.local")
    {
        return Err(package_policy_blocked(
            "package content includes private registry state or user-specific config",
            json!({"path": path}),
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
        return Err(package_policy_blocked(
            "package content includes a local absolute path",
            json!({"path": path}),
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
            return Err(package_policy_blocked(
                "package content includes secret-looking material",
                json!({"path": path, "pattern": needle}),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_package_relative_path(
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    if path.as_os_str().is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "package path must not be empty",
        ));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!("package path is not safely relative: {}", path.display()),
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn package_policy_blocked(message: &str, details: Value) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    failure.details = details;
    failure
}

#[cfg(unix)]
fn reject_hardlink(path: &Path) -> std::result::Result<(), CommandFailure> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).map_err(map_io)?;
    if metadata.nlink() > 1 {
        return Err(package_policy_blocked(
            "package source contains a hardlink; hardlink exports are not supported",
            json!({"path": path.display().to_string()}),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_hardlink(_path: &Path) -> std::result::Result<(), CommandFailure> {
    Ok(())
}
