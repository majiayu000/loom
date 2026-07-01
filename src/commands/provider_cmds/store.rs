use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

use crate::cli::{ProviderAddArgs, ProviderCommand, ProviderRemoveArgs};
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::helpers::{commit_registry_state, map_io, map_lock, map_registry_state};
use super::super::projections::{maybe_autosync_or_queue, record_registry_operation};
use super::super::{App, CommandFailure};
use super::{ProviderKind, ProviderListItem, ProviderRecord, ProvidersFile, ResolvedProvider};

pub(super) const PROVIDERS_REL: &str = "state/registry/providers.json";

pub(super) fn providers_path(ctx: &AppContext) -> PathBuf {
    ctx.root.join(PROVIDERS_REL)
}

pub(super) fn load_providers(
    ctx: &AppContext,
) -> std::result::Result<ProvidersFile, CommandFailure> {
    let path = providers_path(ctx);
    if !path.exists() {
        return Ok(empty_providers_file());
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    let providers: ProvidersFile = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to parse {}: {}", path.display(), err),
        )
    })?;
    if providers.schema_version != 1 {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "provider schema version mismatch: expected 1, got {}",
                providers.schema_version
            ),
        ));
    }
    let mut seen = BTreeSet::new();
    for provider in &providers.providers {
        if !seen.insert(provider.id.clone()) {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!(
                    "duplicate provider id '{}' in {}",
                    provider.id, PROVIDERS_REL
                ),
            ));
        }
    }
    Ok(providers)
}

pub(super) fn save_providers(
    ctx: &AppContext,
    providers: &ProvidersFile,
) -> std::result::Result<(), CommandFailure> {
    let mut providers = providers.clone();
    providers
        .providers
        .sort_by(|left, right| left.id.cmp(&right.id));
    let path = providers_path(ctx);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    let raw = serde_json::to_string_pretty(&providers).map_err(map_io)? + "\n";
    fs::write(path, raw).map_err(map_io)
}

pub(super) fn resolve_provider(
    ctx: &AppContext,
    id: &str,
) -> std::result::Result<ResolvedProvider, CommandFailure> {
    let providers = load_providers(ctx)?;
    if let Some(record) = providers
        .providers
        .into_iter()
        .find(|provider| provider.id == id)
    {
        return Ok(ResolvedProvider {
            record,
            persisted: true,
        });
    }
    if let Some(record) = default_provider(id) {
        return Ok(ResolvedProvider {
            record,
            persisted: false,
        });
    }
    Err(provider_not_found(id))
}

pub(super) fn list_providers(
    ctx: &AppContext,
) -> std::result::Result<Vec<ProviderListItem>, CommandFailure> {
    let persisted = load_providers(ctx)?;
    let mut by_id = BTreeMap::new();
    for id in ["github", "local"] {
        let provider = default_provider(id).expect("built-in provider");
        by_id.insert(
            id.to_string(),
            ProviderListItem::from_resolved(ResolvedProvider {
                record: provider,
                persisted: false,
            }),
        );
    }
    for provider in persisted.providers {
        by_id.insert(
            provider.id.clone(),
            ProviderListItem::from_resolved(ResolvedProvider {
                record: provider,
                persisted: true,
            }),
        );
    }
    Ok(by_id.into_values().collect())
}

pub(super) fn provider_record(id: &str, kind: ProviderKind, url: &str) -> ProviderRecord {
    let now = Utc::now();
    ProviderRecord {
        id: id.to_string(),
        kind,
        url: url.to_string(),
        capabilities: kind.capabilities(),
        trust_default: "third-party-unreviewed".to_string(),
        requires_network: kind.requires_network(),
        created_at: now,
        updated_at: now,
    }
}

pub(super) fn validate_provider_id(id: &str) -> std::result::Result<(), CommandFailure> {
    if id.is_empty() || id == "." || id == ".." {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provider id cannot be empty, '.' or '..'",
        ));
    }
    if id == "team" {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provider id 'team' is reserved for a future policy-backed provider",
        ));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("provider id '{}' contains unsupported characters", id),
        ));
    }
    Ok(())
}

pub(super) fn validate_provider_url(
    kind: ProviderKind,
    url: &str,
) -> std::result::Result<(), CommandFailure> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed != url {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provider url must be non-empty and must not contain surrounding whitespace",
        ));
    }
    if trimmed.contains('\0') {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provider url must not contain NUL bytes",
        ));
    }
    if kind == ProviderKind::Github
        && !(trimmed.starts_with("https://") || trimmed.starts_with("http://"))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "github provider url must start with https:// or http://",
        ));
    }
    reject_url_credentials(trimmed)
}

pub(super) fn provider_not_found(id: &str) -> CommandFailure {
    CommandFailure::new(
        ErrorCode::ProviderNotFound,
        format!("provider '{}' not found", id),
    )
}

fn empty_providers_file() -> ProvidersFile {
    ProvidersFile {
        schema_version: 1,
        providers: Vec::new(),
    }
}

fn default_provider(id: &str) -> Option<ProviderRecord> {
    let epoch = Utc.timestamp_opt(0, 0).single().expect("valid epoch");
    let (kind, url) = match id {
        "github" => (ProviderKind::Github, "https://github.com"),
        "local" => (ProviderKind::Local, "local"),
        _ => return None,
    };
    Some(ProviderRecord {
        id: id.to_string(),
        kind,
        url: url.to_string(),
        capabilities: kind.capabilities(),
        trust_default: "third-party-unreviewed".to_string(),
        requires_network: kind.requires_network(),
        created_at: epoch,
        updated_at: epoch,
    })
}

fn reject_url_credentials(url: &str) -> std::result::Result<(), CommandFailure> {
    if let Some(authority) = http_authority(url)
        && authority.contains('@')
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provider url must not contain embedded userinfo credentials",
        ));
    }
    if let Some(query) = url
        .split_once('?')
        .map(|(_, query)| query.split('#').next().unwrap_or(""))
    {
        for pair in query.split('&') {
            let key = pair.split_once('=').map(|(key, _)| key).unwrap_or(pair);
            let key = key.to_ascii_lowercase();
            if ["token", "key", "secret", "password", "credential", "auth"]
                .iter()
                .any(|needle| key.contains(needle))
            {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "provider url must not contain token-like query parameters",
                ));
            }
        }
    }
    let lower = url.to_ascii_lowercase();
    if ["ghp_", "github_pat_", "xoxb-", "sk-"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        let mut failure = CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provider url must not contain credential-like token values",
        );
        failure.details = json!({"validation_code": "PROVIDER_URL_SECRET"});
        return Err(failure);
    }
    Ok(())
}

fn http_authority(url: &str) -> Option<&str> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    Some(
        after_scheme
            .split(['/', '?', '#'])
            .next()
            .unwrap_or(after_scheme),
    )
}

impl App {
    pub fn cmd_provider(
        &self,
        command: &ProviderCommand,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            ProviderCommand::Add(args) => self.cmd_provider_add(args, request_id),
            ProviderCommand::List => {
                let providers = list_providers(&self.ctx)?;
                Ok((
                    json!({"count": providers.len(), "providers": providers}),
                    Meta::default(),
                ))
            }
            ProviderCommand::Remove(args) => self.cmd_provider_remove(args, request_id),
        }
    }

    fn cmd_provider_add(
        &self,
        args: &ProviderAddArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_provider_id(&args.id)?;
        let kind = args.kind.into();
        validate_provider_url(kind, &args.url)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let mut providers = load_providers(&self.ctx)?;
        let original = providers.clone();
        if providers
            .providers
            .iter()
            .any(|provider| provider.id == args.id)
        {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("provider '{}' already exists", args.id),
            ));
        }
        let record = provider_record(&args.id, kind, &args.url);
        providers.providers.push(record.clone());
        save_providers(&self.ctx, &providers)?;
        let op_id = match record_registry_operation(
            &paths,
            "provider.add",
            json!({
                "provider_id": record.id,
                "kind": record.kind,
                "url": record.url,
                "request_id": request_id
            }),
            json!({"provider_id": record.id}),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                save_providers(&self.ctx, &original)?;
                return Err(map_registry_state(err));
            }
        };
        let commit = commit_registry_state(&self.ctx, &format!("provider({}): add", args.id))?;
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "provider.add",
                request_id,
                json!({"provider_id": args.id, "commit": commit}),
                &mut meta,
            )?;
        }
        Ok((
            json!({"provider": record, "path": PROVIDERS_REL, "commit": commit}),
            meta,
        ))
    }

    fn cmd_provider_remove(
        &self,
        args: &ProviderRemoveArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_provider_id(&args.id)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let mut providers = load_providers(&self.ctx)?;
        let original = providers.clone();
        let before = providers.providers.len();
        providers
            .providers
            .retain(|provider| provider.id != args.id);
        if providers.providers.len() == before {
            let resolved = resolve_provider(&self.ctx, &args.id)?;
            if !resolved.persisted {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "provider '{}' is a built-in default and has no persisted record to remove",
                        args.id
                    ),
                ));
            }
        }
        save_providers(&self.ctx, &providers)?;
        let op_id = match record_registry_operation(
            &paths,
            "provider.remove",
            json!({"provider_id": args.id, "request_id": request_id}),
            json!({"provider_id": args.id}),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                save_providers(&self.ctx, &original)?;
                return Err(map_registry_state(err));
            }
        };
        let commit = commit_registry_state(&self.ctx, &format!("provider({}): remove", args.id))?;
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "provider.remove",
                request_id,
                json!({"provider_id": args.id, "commit": commit}),
                &mut meta,
            )?;
        }
        Ok((
            json!({
                "provider_id": args.id,
                "removed": true,
                "path": PROVIDERS_REL,
                "commit": commit,
            }),
            meta,
        ))
    }
}

pub(super) fn find_license(path: &Path) -> std::result::Result<Option<String>, CommandFailure> {
    for candidate in ["LICENSE", "LICENSE.md", "COPYING"] {
        if path.join(candidate).exists() {
            return Ok(Some(candidate.to_string()));
        }
    }
    Ok(None)
}

pub(super) fn is_script_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| {
            matches!(
                ext,
                "sh" | "bash" | "zsh" | "py" | "js" | "ts" | "rb" | "pl" | "ps1"
            )
        })
}

#[cfg(unix)]
pub(super) fn is_executable(path: &Path) -> std::result::Result<bool, CommandFailure> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::metadata(path).map_err(map_io)?.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub(super) fn is_executable(_path: &Path) -> std::result::Result<bool, CommandFailure> {
    Ok(false)
}
