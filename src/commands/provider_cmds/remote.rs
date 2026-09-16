use std::{fs, path::Path, process::Command};

use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    ParsedLocator, ResolvedProvider,
    install::github_install_source,
    locator::{local_preview, locator_name, parse_locator},
};
use crate::{
    commands::{
        CommandFailure,
        helpers::{map_io, shell_arg},
        redact_sensitive_string,
    },
    state::AppContext,
    types::ErrorCode,
};

#[derive(Deserialize)]
struct SearchResponse {
    total_count: u64,
    incomplete_results: bool,
    items: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct SearchItem {
    path: String,
    html_url: String,
    repository: Repository,
}

#[derive(Deserialize)]
struct Repository {
    full_name: String,
    description: Option<String>,
}

pub(super) fn search(
    ctx: &AppContext,
    provider: &ResolvedProvider,
    query: &str,
) -> Result<Value, CommandFailure> {
    let base = provider.record.url.trim_end_matches('/');
    let host = base
        .split_once("://")
        .map(|(_, host)| host)
        .filter(|host| !host.is_empty() && !host.contains(['/', '?', '#', '@']))
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                "GitHub search requires a provider URL containing only scheme and host",
            )
        })?;
    if query.trim().is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "catalog search query must not be empty",
        ));
    }
    let output = Command::new("gh")
        .args(["api", "--hostname", host, "--method", "GET", "search/code", "--raw-field", &format!("q={query} filename:SKILL.md"), "--field", "per_page=30"])
        .env("GH_PROMPT_DISABLED", "1")
        .output()
        .map_err(|err| CommandFailure::new(ErrorCode::IoError, format!("GitHub CLI search could not start; install gh and authenticate for this provider: {err}")))?;
    if !output.status.success() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "GitHub catalog search failed: {}",
                redact_sensitive_string(&String::from_utf8_lossy(&output.stderr))
            ),
        ));
    }
    let response: SearchResponse = serde_json::from_slice(&output.stdout).map_err(|err| {
        CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!("invalid GitHub search response: {err}"),
        )
    })?;
    let mut results = Vec::new();
    for item in response.items {
        let path = Path::new(&item.path);
        if path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
            return Err(CommandFailure::new(
                ErrorCode::SchemaMismatch,
                "GitHub search returned a non-skill entrypoint",
            ));
        }
        let subdir = path.parent().and_then(|p| p.to_str()).unwrap_or("");
        let raw = if subdir.is_empty() {
            format!("{}:{}", provider.record.id, item.repository.full_name)
        } else {
            format!(
                "{}:{}//{subdir}",
                provider.record.id, item.repository.full_name
            )
        };
        let locator = parse_locator(ctx, &raw, None)?;
        let name = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or(&item.repository.full_name);
        results.push(json!({"locator":raw,"name":name,"description":item.repository.description,"url":item.html_url,"source":locator.source_json(),"signals":{"verified":false},"warnings":["third-party-unreviewed","preview to resolve an immutable commit before install"]}));
    }
    Ok(
        json!({"query":query,"provider":provider.record.id,"results":results,"total_count":response.total_count,"incomplete_results":response.incomplete_results,"truncated":response.total_count > results.len() as u64,"limit":30}),
    )
}

pub(super) fn preview(ctx: &AppContext, locator: &ParsedLocator) -> Result<Value, CommandFailure> {
    let staging = std::env::temp_dir().join(format!("loom-catalog-preview-{}", Uuid::new_v4()));
    fs::create_dir(&staging).map_err(map_io)?;
    let result = (|| {
        let source = github_install_source(ctx, locator, &staging)?;
        // Repository metadata is not part of the skill and must not consume its inspection budget.
        fs::remove_dir_all(staging.join("clone/.git")).map_err(map_io)?;
        let mut preview = local_preview(&source.copy_source, Some(&locator_name(locator)))?;
        preview["provenance"]["source"] =
            serde_json::to_value(&source.descriptor).map_err(map_io)?;
        preview["provenance"]["pinned"] = json!(locator.pinned);
        let commit = source
            .descriptor
            .resolved_commit
            .as_deref()
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::StateCorrupt,
                    "preview did not resolve a source commit",
                )
            })?;
        let repository = source.descriptor.repository.as_deref().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "preview source repository is missing",
            )
        })?;
        let pinned = if locator.subdir.is_empty() {
            format!("{}:{repository}@{commit}", locator.provider_id())
        } else {
            format!(
                "{}:{repository}//{}@{commit}",
                locator.provider_id(),
                locator.subdir
            )
        };
        Ok(
            json!({"locator":locator.raw,"resolved_locator":pinned,"source":source.descriptor,"preview":preview,"warnings":["third-party-unreviewed","inspected fetched files without executing skill scripts"],"scripts_executed":false,"suggested_preview":format!("loom catalog preview {}", shell_arg(&pinned))}),
        )
    })();
    let cleanup = fs::remove_dir_all(&staging).map_err(map_io);
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), Ok(())) => Err(err),
        (Ok(_), Err(err)) => Err(err),
        (Err(err), Err(cleanup)) => {
            Err(err.with_rollback_errors(vec![json!({"cleanup_error":cleanup.message})]))
        }
    }
}
