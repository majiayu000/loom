use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use serde_json::{Value, json};
use walkdir::WalkDir;

use crate::cli::{
    CatalogCommand, CatalogPreviewArgs, CatalogSearchArgs, CatalogShowArgs, SkillInstallArgs,
};
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::helpers::{map_arg, map_io, validate_policy_profile, validate_skill_name};
use super::super::provenance::{
    ArtifactDescriptor, SkillSourceRecord, SourceDescriptor, skill_tree_digest,
};
use super::super::{App, CommandFailure, SkillLintMode, lint_skill_source};
use super::store::{
    find_license, is_executable, is_script_extension, provider_not_found, resolve_provider,
    validate_provider_id,
};
use super::{LocatorSource, ParsedLocator, ProviderKind};

pub(super) fn parse_locator(
    ctx: &AppContext,
    raw: &str,
    ref_override: Option<&str>,
) -> std::result::Result<ParsedLocator, CommandFailure> {
    let (provider_id, body) = raw.split_once(':').ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::ArgInvalid,
            "locator must start with a provider prefix such as github: or local:",
        )
    })?;
    if provider_id == "team" {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "team: locators are reserved for a future policy-backed provider",
        ));
    }
    validate_provider_id(provider_id)?;
    let provider = resolve_provider(ctx, provider_id).map_err(|err| {
        if matches!(err.code, ErrorCode::ProviderNotFound) {
            provider_not_found(provider_id)
        } else {
            err
        }
    })?;
    let (body, locator_ref) = split_ref(body);
    let requested_ref = match (locator_ref, ref_override) {
        (Some(locator_ref), Some(override_ref)) if locator_ref != override_ref => {
            let mut failure = CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "locator ref '{}' conflicts with --ref '{}'",
                    locator_ref, override_ref
                ),
            );
            failure.details = json!({"validation_code": "LOCATOR_REF_CONFLICT"});
            return Err(failure);
        }
        (Some(locator_ref), _) => Some(locator_ref.to_string()),
        (None, Some(override_ref)) => Some(override_ref.to_string()),
        (None, None) => None,
    };

    let source = match provider.record.kind {
        ProviderKind::Github => parse_github_source(&provider.record.url, body)?,
        ProviderKind::Local => parse_local_source(body)?,
    };
    let subdir = match &source {
        LocatorSource::Github { .. } => source_subdir(body)?,
        LocatorSource::Local { .. } => source_subdir(body)?,
    };
    let pinned = match provider.record.kind {
        ProviderKind::Github => requested_ref.as_deref().is_some_and(is_commit_sha),
        ProviderKind::Local => requested_ref.as_deref().is_some_and(is_sha256_ref),
    };

    Ok(ParsedLocator {
        raw: raw.to_string(),
        provider,
        source,
        subdir,
        requested_ref,
        pinned,
    })
}

fn parse_github_source(
    provider_url: &str,
    body: &str,
) -> std::result::Result<LocatorSource, CommandFailure> {
    let (repo, _) = body.split_once("//").unwrap_or((body, ""));
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or("");
    let name = parts.next().unwrap_or("");
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "github locator must look like github:owner/repo//optional/subdir@ref",
        ));
    }
    validate_repo_segment(owner)?;
    validate_repo_segment(name)?;
    Ok(LocatorSource::Github {
        repository: format!("{owner}/{name}"),
        clone_url: format!("{}/{}.git", provider_url.trim_end_matches('/'), repo),
    })
}

fn parse_local_source(body: &str) -> std::result::Result<LocatorSource, CommandFailure> {
    let (base, _) = body.split_once("//").unwrap_or((body, ""));
    if base.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "local locator must include a base path",
        ));
    }
    Ok(LocatorSource::Local {
        base_path: PathBuf::from(base),
    })
}

fn source_subdir(body: &str) -> std::result::Result<String, CommandFailure> {
    let Some((_, subdir)) = body.split_once("//") else {
        return Ok(String::new());
    };
    normalize_relative_subdir(Path::new(subdir))
}

fn normalize_relative_subdir(path: &Path) -> std::result::Result<String, CommandFailure> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "locator subdir must be a relative path without '..'",
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

fn split_ref(body: &str) -> (&str, Option<&str>) {
    match body.rsplit_once('@') {
        Some((before, after)) if !before.is_empty() && !after.is_empty() => (before, Some(after)),
        _ => (body, None),
    }
}

fn validate_repo_segment(value: &str) -> std::result::Result<(), CommandFailure> {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        Ok(())
    } else {
        Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "github repository segments may only contain [A-Za-z0-9._-]",
        ))
    }
}

fn is_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_sha256_ref(value: &str) -> bool {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return false;
    };
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl App {
    pub fn cmd_catalog(
        &self,
        command: &CatalogCommand,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            CatalogCommand::Search(args) => self.cmd_catalog_search(args),
            CatalogCommand::Show(args) => self.cmd_catalog_show(args),
            CatalogCommand::Preview(args) => self.cmd_catalog_preview(args),
        }
    }

    pub fn cmd_skill_install(
        &self,
        args: &SkillInstallArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.name).map_err(map_arg)?;
        if let Some(profile) = &args.policy_profile {
            validate_policy_profile(profile)?;
        }
        let trust = args
            .trust
            .map(super::trust_arg_as_str)
            .unwrap_or("third-party-unreviewed");
        if trust == "reviewed" && args.review_evidence.as_deref().unwrap_or("").is_empty() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--trust reviewed requires --review-evidence",
            ));
        }
        if !args.dry_run {
            return Err(CommandFailure::new(
                ErrorCode::PolicyBlocked,
                "mutating provider install is deferred in this slice; rerun with --dry-run",
            ));
        }
        let locator = parse_locator(&self.ctx, &args.locator, args.source_ref.as_deref())?;
        if !locator.pinned {
            let mut failure = CommandFailure::new(
                ErrorCode::PolicyBlocked,
                "provider install requires an immutable pinned ref",
            );
            failure.details = json!({
                "pin_policy": pin_policy(&locator, args.policy_profile.as_deref()),
                "suggested_action": "use a commit SHA for GitHub or sha256:<digest> for local locators",
            });
            return Err(failure);
        }
        Ok((
            install_dry_run_plan(&locator, &args.name, trust, args.review_evidence.as_deref())?,
            Meta::default(),
        ))
    }

    fn cmd_catalog_search(
        &self,
        args: &CatalogSearchArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let Some(provider_id) = args.provider.as_deref() else {
            return Ok((
                json!({"query": args.query, "results": [], "warnings": ["catalog search requires an explicit --provider in this foundation slice"]}),
                Meta::default(),
            ));
        };
        let provider = resolve_provider(&self.ctx, provider_id)?;
        if provider.record.requires_network {
            if !args.allow_network {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "network-backed catalog search requires --allow-network",
                ));
            }
            if !provider.persisted {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "network-backed catalog search requires a persisted provider record",
                ));
            }
            return Ok((
                json!({"query": args.query, "provider": provider.record.id, "results": [], "warnings": ["network provider search is advisory and not implemented in this foundation slice"]}),
                Meta::default(),
            ));
        }
        if !provider.persisted {
            return Ok((
                json!({"query": args.query, "provider": provider.record.id, "results": [], "warnings": ["built-in local provider has no persisted catalog path"]}),
                Meta::default(),
            ));
        }
        Ok((
            json!({
                "query": args.query,
                "provider": provider.record.id,
                "agent": args.agent,
                "results": search_local_catalog(Path::new(&provider.record.url), &args.query)?,
                "warnings": ["catalog results are advisory until installed through loom skill install"],
            }),
            Meta::default(),
        ))
    }

    fn cmd_catalog_show(
        &self,
        args: &CatalogShowArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let locator = parse_locator(&self.ctx, &args.locator, None)?;
        Ok((
            json!({"result": catalog_result_for_locator(&locator)?, "locator": locator.source_json()}),
            Meta::default(),
        ))
    }

    fn cmd_catalog_preview(
        &self,
        args: &CatalogPreviewArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let locator = parse_locator(&self.ctx, &args.locator, args.source_ref.as_deref())?;
        Ok((preview_for_locator(&locator)?, Meta::default()))
    }
}

fn search_local_catalog(
    root: &Path,
    query: &str,
) -> std::result::Result<Vec<Value>, CommandFailure> {
    if !root.is_dir() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "local provider path '{}' is not a directory",
                root.display()
            ),
        ));
    }
    let needle = query.to_ascii_lowercase();
    let mut results = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .max_depth(4)
        .sort_by_file_name()
    {
        let entry = entry.map_err(map_io)?;
        if !entry.file_type().is_file() || entry.file_name().to_string_lossy() != "SKILL.md" {
            continue;
        }
        let skill_dir = entry.path().parent().unwrap_or(root);
        let preview = local_preview(skill_dir, None)?;
        let name = preview["metadata"]["name"].as_str().unwrap_or_else(|| {
            skill_dir
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("skill")
        });
        let description = preview["metadata"]["description"].as_str().unwrap_or("");
        if !name.to_ascii_lowercase().contains(&needle)
            && !description.to_ascii_lowercase().contains(&needle)
        {
            continue;
        }
        let rel = skill_dir
            .strip_prefix(root)
            .unwrap_or(skill_dir)
            .to_string_lossy()
            .to_string();
        results.push(json!({
            "locator": format!("local:{}//{}", root.display(), rel),
            "name": name,
            "description": description,
            "source": {"provider": "local", "path": root.display().to_string(), "subdir": rel, "ref": null},
            "signals": {"stars": null, "last_updated": null, "license": preview["metadata"]["license"], "verified": false},
            "warnings": ["third-party-unreviewed"],
        }));
    }
    Ok(results)
}

fn catalog_result_for_locator(
    locator: &ParsedLocator,
) -> std::result::Result<Value, CommandFailure> {
    let name = locator_name(locator);
    let mut description = String::new();
    let mut license = Value::Null;
    if let Some(path) = locator.source_path()
        && path.exists()
    {
        let preview = local_preview(&path, Some(&name))?;
        description = preview["metadata"]["description"]
            .as_str()
            .unwrap_or("")
            .to_string();
        license = preview["metadata"]["license"].clone();
    }
    Ok(json!({
        "locator": locator.raw,
        "name": name,
        "description": description,
        "source": locator.source_json(),
        "signals": {"stars": null, "last_updated": null, "license": license, "verified": false},
        "warnings": ["third-party-unreviewed"],
    }))
}

fn preview_for_locator(locator: &ParsedLocator) -> std::result::Result<Value, CommandFailure> {
    match locator.provider_kind() {
        ProviderKind::Local => {
            let path = locator.source_path().expect("local source path");
            Ok(json!({
                "locator": locator.raw,
                "source": locator.source_json(),
                "preview": local_preview(&path, Some(&locator_name(locator)))?,
                "suggested_install": format!("loom skill install '{}' --name {} --dry-run", locator.raw, locator_name(locator)),
                "warnings": ["preview inspected files without executing scripts"],
            }))
        }
        ProviderKind::Github => Ok(json!({
            "locator": locator.raw,
            "source": locator.source_json(),
            "preview": {
                "metadata": {"name": locator_name(locator), "description": null, "license": null},
                "file_tree": [],
                "scripts": [],
                "provenance": {"provider": locator.provider_id(), "pinned": locator.pinned, "requested_ref": locator.requested_ref},
                "lint": {"status": "not_run", "reason": "remote preview fetch is deferred"},
                "safety": {"status": "not_run", "reason": "remote preview fetch is deferred"},
            },
            "suggested_install": format!("loom skill install '{}' --name {} --dry-run", locator.raw, locator_name(locator)),
            "warnings": ["remote preview did not execute code; fetch-backed inspection is deferred in this slice"],
        })),
    }
}

fn local_preview(
    path: &Path,
    expected_name: Option<&str>,
) -> std::result::Result<Value, CommandFailure> {
    if !path.is_dir() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("skill source '{}' is not a directory", path.display()),
        ));
    }
    let name = expected_name.map(ToString::to_string).unwrap_or_else(|| {
        path.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("skill")
            .to_string()
    });
    let lint = lint_skill_source(path, &name, SkillLintMode::Compat);
    Ok(json!({
        "metadata": {
            "name": lint.frontmatter.name,
            "description": lint.frontmatter.description,
            "license": lint.frontmatter.license,
            "entrypoint": lint.entrypoint.path,
        },
        "file_tree": collect_file_tree(path, 200)?,
        "scripts": collect_scripts(path)?,
        "license": find_license(path)?,
        "provenance": {"digest": skill_tree_digest(path).map_err(map_io)?, "pinned": false},
        "lint": {"valid": lint.valid, "compatible": lint.compatible, "summary": lint.summary, "findings": lint.findings},
        "safety": scan_local_safety(path)?,
    }))
}

fn install_dry_run_plan(
    locator: &ParsedLocator,
    skill: &str,
    trust: &str,
    review_evidence: Option<&str>,
) -> std::result::Result<Value, CommandFailure> {
    let pin_policy = pin_policy(locator, None);
    let (source_digest, lint, safety, provenance, lock) = match locator.provider_kind() {
        ProviderKind::Local => install_local_plan(locator, skill)?,
        ProviderKind::Github => install_github_plan(locator, skill)?,
    };
    Ok(json!({
        "dry_run": true,
        "skill": skill,
        "resolved_locator": locator.source_json(),
        "pin_policy": pin_policy,
        "staging": {"mode": "isolated", "fetch_plan": fetch_plan(locator)},
        "lint": lint,
        "safety": safety,
        "would_write": {
            "skill_dir": format!("skills/{}", skill),
            "provenance_path": "state/registry/sources.json",
            "provenance_record": provenance,
            "lock_path": "loom.lock",
            "lock_record": lock,
            "trust_path": "state/registry/trust.json",
            "trust_record": {
                "skill_id": skill,
                "trust": trust,
                "quarantined": false,
                "review_evidence": review_evidence,
                "provider_id": locator.provider_id(),
                "source_digest": source_digest,
                "resolved_ref": locator.requested_ref,
            },
        },
        "next_actions": [
            format!("loom catalog preview '{}'", locator.raw),
            format!("loom skill scan {}", skill),
            format!("loom skill activate {} --dry-run", skill),
        ],
    }))
}

fn install_local_plan(
    locator: &ParsedLocator,
    skill: &str,
) -> std::result::Result<(Option<String>, Value, Value, Value, Value), CommandFailure> {
    let path = locator.source_path().expect("local source path");
    let preview = local_preview(&path, Some(skill))?;
    let digest = preview["provenance"]["digest"]
        .as_str()
        .ok_or_else(|| CommandFailure::new(ErrorCode::InternalError, "missing preview digest"))?
        .to_string();
    if locator.requested_ref.as_deref() != Some(digest.as_str()) {
        let mut failure = CommandFailure::new(
            ErrorCode::PolicyBlocked,
            "local provider digest pin does not match source content",
        );
        failure.details = json!({"requested_ref": locator.requested_ref, "actual_digest": digest});
        return Err(failure);
    }
    if preview["safety"]["summary"]["critical"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        let mut failure = CommandFailure::new(
            ErrorCode::PolicyBlocked,
            "critical safety findings block provider install",
        );
        failure.details = json!({"safety": preview["safety"]});
        return Err(failure);
    }
    let descriptor = local_source_descriptor(locator)?;
    let record = SkillSourceRecord {
        skill_id: skill.to_string(),
        source: descriptor,
        artifact: ArtifactDescriptor {
            digest: digest.clone(),
        },
        imported_at: chrono::Utc::now(),
        importer_version: format!("loom/{}", env!("CARGO_PKG_VERSION")),
    };
    let lock = json!({
        "source": locator.raw,
        "provider": locator.provider_id(),
        "ref": locator.requested_ref,
        "commit": null,
        "tree_sha": null,
        "digest": digest,
        "agents": [],
        "scope": "project",
    });
    Ok((
        Some(digest),
        preview["lint"].clone(),
        preview["safety"].clone(),
        json!(record),
        lock,
    ))
}

fn install_github_plan(
    locator: &ParsedLocator,
    skill: &str,
) -> std::result::Result<(Option<String>, Value, Value, Value, Value), CommandFailure> {
    let descriptor = github_source_descriptor(locator)?;
    let commit = descriptor.resolved_commit.clone();
    let provenance = json!({
        "skill_id": skill,
        "source": descriptor,
        "artifact": {"digest": null},
        "imported_at": chrono::Utc::now(),
        "importer_version": format!("loom/{}", env!("CARGO_PKG_VERSION")),
    });
    let lock = json!({
        "source": locator.raw,
        "provider": locator.provider_id(),
        "ref": locator.requested_ref,
        "commit": commit,
        "tree_sha": null,
        "digest": null,
        "agents": [],
        "scope": "project",
    });
    Ok((
        None,
        json!({"status": "not_run", "reason": "remote fetch is deferred"}),
        json!({"status": "not_run", "reason": "remote fetch is deferred", "summary": {"critical": 0, "high": 0, "medium": 0, "low": 0}}),
        provenance,
        lock,
    ))
}

fn local_source_descriptor(
    locator: &ParsedLocator,
) -> std::result::Result<SourceDescriptor, CommandFailure> {
    let LocatorSource::Local { base_path } = &locator.source else {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            "expected local locator",
        ));
    };
    let base = fs::canonicalize(base_path)
        .with_context(|| {
            format!(
                "failed to canonicalize provider path '{}'",
                base_path.display()
            )
        })
        .map_err(map_io)?;
    Ok(SourceDescriptor {
        provider: locator.provider_id().to_string(),
        locator: locator.raw.clone(),
        repository: None,
        path: Some(base.display().to_string()),
        subdir: locator.subdir.clone(),
        requested_ref: locator.requested_ref.clone(),
        resolved_commit: None,
        tree_sha: None,
    })
}

fn github_source_descriptor(
    locator: &ParsedLocator,
) -> std::result::Result<SourceDescriptor, CommandFailure> {
    let LocatorSource::Github { repository, .. } = &locator.source else {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            "expected github locator",
        ));
    };
    Ok(SourceDescriptor {
        provider: locator.provider_id().to_string(),
        locator: locator.raw.clone(),
        repository: Some(repository.clone()),
        path: None,
        subdir: locator.subdir.clone(),
        requested_ref: locator.requested_ref.clone(),
        resolved_commit: locator.requested_ref.clone(),
        tree_sha: None,
    })
}

fn pin_policy(locator: &ParsedLocator, policy_profile: Option<&str>) -> Value {
    json!({
        "policy_profile": policy_profile.unwrap_or("default-fail-closed"),
        "pinned": locator.pinned,
        "required": true,
        "provider_kind": locator.provider_kind().as_str(),
        "requested_ref": locator.requested_ref,
    })
}

fn fetch_plan(locator: &ParsedLocator) -> Value {
    match &locator.source {
        LocatorSource::Local { base_path } => json!({
            "provider": locator.provider_id(),
            "operation": "copy_without_symlinks",
            "path": base_path.display().to_string(),
            "subdir": locator.subdir,
        }),
        LocatorSource::Github {
            repository,
            clone_url,
        } => json!({
            "provider": locator.provider_id(),
            "operation": "git_clone_checkout",
            "repository": repository,
            "clone_url": clone_url,
            "ref": locator.requested_ref,
            "subdir": locator.subdir,
        }),
    }
}

fn locator_name(locator: &ParsedLocator) -> String {
    if !locator.subdir.is_empty() {
        return Path::new(&locator.subdir)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("skill")
            .to_string();
    }
    match &locator.source {
        LocatorSource::Github { repository, .. } => repository
            .split('/')
            .next_back()
            .unwrap_or("skill")
            .to_string(),
        LocatorSource::Local { base_path } => base_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("skill")
            .to_string(),
    }
}

fn collect_file_tree(path: &Path, limit: usize) -> std::result::Result<Vec<Value>, CommandFailure> {
    let mut entries = Vec::new();
    for entry in WalkDir::new(path)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .skip(1)
    {
        let entry = entry.map_err(map_io)?;
        if entries.len() >= limit {
            break;
        }
        let rel = entry
            .path()
            .strip_prefix(path)
            .map_err(map_io)?
            .to_string_lossy()
            .to_string();
        let file_type = if entry.file_type().is_dir() {
            "dir"
        } else if entry.file_type().is_symlink() {
            "symlink"
        } else {
            "file"
        };
        entries.push(json!({"path": rel, "type": file_type}));
    }
    Ok(entries)
}

fn collect_scripts(path: &Path) -> std::result::Result<Vec<Value>, CommandFailure> {
    let mut scripts = Vec::new();
    for entry in WalkDir::new(path).follow_links(false).sort_by_file_name() {
        let entry = entry.map_err(map_io)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(path)
            .map_err(map_io)?
            .to_string_lossy()
            .to_string();
        let executable = is_executable(entry.path())?;
        if rel.starts_with("scripts/") || is_script_extension(entry.path()) || executable {
            scripts.push(json!({"path": rel, "executable": executable}));
        }
    }
    Ok(scripts)
}

fn scan_local_safety(path: &Path) -> std::result::Result<Value, CommandFailure> {
    let mut findings = Vec::new();
    for entry in WalkDir::new(path).follow_links(false).sort_by_file_name() {
        let entry = entry.map_err(map_io)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(path)
            .map_err(map_io)?
            .to_string_lossy()
            .to_string();
        let raw = fs::read(entry.path()).map_err(map_io)?;
        if raw.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&raw).to_ascii_lowercase();
        if text.contains("rm -rf") || text.contains("read secrets") || text.contains("id_rsa") {
            findings.push(json!({
                "id": "provider_preview_critical_pattern",
                "severity": "critical",
                "path": rel,
                "message": "provider source contains critical safety pattern",
                "suggested_action": "remove destructive or secret-reading instructions before install",
            }));
        } else if text.contains("curl ") || text.contains("eval(") || text.contains("exec(") {
            findings.push(json!({
                "id": "provider_preview_high_pattern",
                "severity": "high",
                "path": rel,
                "message": "provider source contains high-risk script pattern",
                "suggested_action": "review network or dynamic execution before install",
            }));
        }
    }
    let critical = findings
        .iter()
        .filter(|finding| finding["severity"] == "critical")
        .count();
    let high = findings
        .iter()
        .filter(|finding| finding["severity"] == "high")
        .count();
    Ok(json!({
        "status": "completed",
        "summary": {"critical": critical, "high": high, "medium": 0, "low": 0},
        "findings": findings,
    }))
}
