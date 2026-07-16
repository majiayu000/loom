use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::commands::CommandFailure;
use crate::state::{AppContext, home_dir, resolve_agent_skill_dirs};
use crate::state_model::RegistryProjectionTarget;
use crate::types::ErrorCode;

mod metadata;

use metadata::{
    adapter_json_invalid, built_in_default_skill_dirs, built_in_discovery_roots, built_in_reload,
    built_in_visibility, capabilities_from_reload, default_scan_eligible, default_visibility,
    discovery_root_json, external_discovery_root, external_visibility, reload_from_capability,
    reload_json, resolve_root_template, role_rank, v1_discovery_roots, validate_discovery_root,
    validate_visibility, visibility_json,
};

pub(crate) const ADAPTER_API_V1: &str = "1";
pub(crate) const ADAPTER_API_V2: &str = "2";
pub(crate) const ADAPTER_API_VERSION: &str = ADAPTER_API_V2;
pub(crate) const SOURCE_BUILT_IN: &str = "built-in";
pub(crate) const SOURCE_EXTERNAL: &str = "external";
pub(crate) const SOURCE_UNKNOWN: &str = "unknown";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdapterFidelity {
    Verified,
    Generic,
}

impl AdapterFidelity {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Generic => "generic",
        }
    }

    pub(crate) fn is_verified(self) -> bool {
        self == Self::Verified
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentAdapter {
    pub adapter_api: String,
    pub id: String,
    pub source: String,
    pub fidelity: AdapterFidelity,
    pub supported_scopes: Vec<String>,
    pub projection_methods: Vec<String>,
    pub skill_entrypoint: String,
    pub capabilities: AdapterCapabilities,
    pub default_skill_dirs: Vec<PathBuf>,
    pub discovery_roots: Vec<AdapterDiscoveryRoot>,
    pub visibility: AdapterVisibility,
    pub reload: AdapterReload,
    pub config_path: Option<PathBuf>,
}

impl AgentAdapter {
    pub(crate) fn has_discovery_root_for_scope(&self, scope: &str) -> bool {
        self.discovery_roots.iter().any(|root| root.scope == scope)
    }

    pub(crate) fn has_verified_visibility_metadata(&self) -> bool {
        self.fidelity.is_verified() && !self.visibility.identity_by_projection_method.is_empty()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct AdapterCapabilities {
    #[serde(default)]
    pub automatic_discovery: bool,
    #[serde(default)]
    pub explicit_invocation: bool,
    #[serde(default)]
    pub reload_required: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AdapterDiscoveryRoot {
    pub scope: String,
    pub path_template: String,
    pub role: String,
    pub source_env_var: Option<String>,
    pub priority: Option<u32>,
    pub scan_eligible: bool,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AdapterVisibility {
    pub follows_symlink_dirs: bool,
    pub identity_by_projection_method: BTreeMap<String, String>,
    pub config_file: Option<String>,
    pub disable_rules: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AdapterReload {
    pub strategy: String,
    pub hot_reload: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedDiscoveryRoot {
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentAdapterRegistry {
    adapters: Vec<AgentAdapter>,
    by_id: BTreeMap<String, AgentAdapter>,
    config_locations: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct AdapterApiProbe {
    adapter_api: String,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalAdapterV1Record {
    adapter_api: String,
    id: String,
    supported_scopes: Vec<String>,
    projection_methods: Vec<String>,
    skill_entrypoint: String,
    capabilities: AdapterCapabilities,
    #[serde(default)]
    default_skill_dirs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalAdapterV2Record {
    adapter_api: String,
    id: String,
    supported_scopes: Vec<String>,
    projection_methods: Vec<String>,
    skill_entrypoint: String,
    capabilities: AdapterCapabilities,
    discovery_roots: Vec<ExternalDiscoveryRootRecord>,
    visibility: ExternalVisibilityRecord,
    reload: ExternalReloadRecord,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalDiscoveryRootRecord {
    scope: String,
    #[serde(alias = "path")]
    path_template: String,
    role: String,
    #[serde(default)]
    source_env_var: Option<String>,
    #[serde(default)]
    priority: Option<u32>,
    #[serde(default = "default_scan_eligible")]
    scan_eligible: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalVisibilityRecord {
    #[serde(default)]
    follows_symlink_dirs: bool,
    #[serde(default)]
    identity_by_projection_method: BTreeMap<String, String>,
    #[serde(default)]
    identity: Option<String>,
    #[serde(default)]
    config_file: Option<String>,
    #[serde(default)]
    disable_rules: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalReloadRecord {
    strategy: String,
    hot_reload: bool,
    #[serde(default)]
    notes: Option<String>,
}

pub(crate) fn load_agent_adapters(
    ctx: &AppContext,
) -> std::result::Result<AgentAdapterRegistry, CommandFailure> {
    let home = home_dir();
    let mut adapters = built_in_adapters(&ctx.root, home.as_deref());
    let mut config_locations = Vec::new();

    for location in external_adapter_locations(&ctx.root) {
        match location.kind {
            AdapterLocationKind::Directory => {
                config_locations.push(json!({
                    "kind": "directory",
                    "path": location.path.display().to_string(),
                    "present": location.path.is_dir(),
                }));
                if !location.path.exists() {
                    continue;
                }
                if !location.path.is_dir() {
                    return Err(adapter_failure(
                        format!(
                            "adapter location '{}' is not a directory",
                            location.path.display()
                        ),
                        "ADAPTER_LOCATION_NOT_DIRECTORY",
                        Some(&location.path),
                    ));
                }
                let mut entries = fs::read_dir(&location.path)
                    .map_err(|err| adapter_io_failure(&location.path, err))?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|err| adapter_io_failure(&location.path, err))?;
                entries.sort_by_key(|entry| entry.path());
                for entry in entries {
                    let path = entry.path();
                    if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                        adapters.push(load_external_adapter(&path, home.as_deref())?);
                    }
                }
            }
            AdapterLocationKind::File => {
                config_locations.push(json!({
                    "kind": "file",
                    "path": location.path.display().to_string(),
                    "present": location.path.is_file(),
                }));
                if !location.path.is_file() {
                    return Err(adapter_failure(
                        format!("adapter file '{}' does not exist", location.path.display()),
                        "ADAPTER_FILE_NOT_FOUND",
                        Some(&location.path),
                    ));
                }
                adapters.push(load_external_adapter(&location.path, home.as_deref())?);
            }
        }
    }

    build_registry(adapters, config_locations)
}

pub(crate) fn built_in_adapter_for_agent(ctx: &AppContext, agent: &str) -> Option<AgentAdapter> {
    let home = home_dir();
    built_in_adapters(&ctx.root, home.as_deref())
        .into_iter()
        .find(|adapter| adapter.id == agent)
}

impl AgentAdapterRegistry {
    pub(crate) fn adapters(&self) -> &[AgentAdapter] {
        &self.adapters
    }

    pub(crate) fn source_for_agent(&self, agent: &str) -> &str {
        self.by_id
            .get(agent)
            .map(|adapter| adapter.source.as_str())
            .unwrap_or(SOURCE_UNKNOWN)
    }

    pub(crate) fn adapter_for_agent(&self, agent: &str) -> Option<&AgentAdapter> {
        self.by_id.get(agent)
    }

    pub(crate) fn config_locations(&self) -> &[Value] {
        &self.config_locations
    }

    pub(crate) fn adapters_json(&self) -> Vec<Value> {
        self.adapters
            .iter()
            .map(|adapter| {
                json!({
                    "adapter_api": ADAPTER_API_VERSION,
                    "declared_adapter_api": adapter.adapter_api,
                    "id": adapter.id,
                    "source": adapter.source,
                    "fidelity": adapter.fidelity.as_str(),
                    "supported_scopes": adapter.supported_scopes,
                    "projection_methods": adapter.projection_methods,
                    "skill_entrypoint": adapter.skill_entrypoint,
                    "capabilities": {
                        "automatic_discovery": adapter.capabilities.automatic_discovery,
                        "explicit_invocation": adapter.capabilities.explicit_invocation,
                        "reload_required": adapter.capabilities.reload_required,
                    },
                    "default_skill_dirs": adapter.default_skill_dirs.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
                    "discovery_roots": adapter.discovery_roots.iter().map(discovery_root_json).collect::<Vec<_>>(),
                    "visibility": visibility_json(&adapter.visibility),
                    "reload": reload_json(&adapter.reload),
                    "config_path": adapter.config_path.as_ref().map(|path| path.display().to_string()),
                })
            })
            .collect()
    }
}

pub(crate) fn preferred_discovery_root(
    adapter: &AgentAdapter,
    scope: &str,
    workspace: &Path,
) -> std::result::Result<ResolvedDiscoveryRoot, CommandFailure> {
    let mut candidates = adapter
        .discovery_roots
        .iter()
        .filter(|root| root.scope == scope && root.available)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(adapter_failure(
            format!(
                "adapter '{}' does not declare an available discovery root for scope '{}'",
                adapter.id, scope
            ),
            "ADAPTER_DISCOVERY_ROOT_MISSING",
            adapter.config_path.as_deref(),
        ));
    }
    candidates.sort_by_key(|root| {
        (
            role_rank(&root.role, scope),
            root.priority.unwrap_or(u32::MAX),
            root.path_template.as_str(),
        )
    });
    let root = candidates[0];
    let path = resolve_root_template(&root.path_template, workspace)?;
    Ok(ResolvedDiscoveryRoot { path })
}

pub(crate) fn decorate_target_for_output(
    target: &RegistryProjectionTarget,
    registry: &AgentAdapterRegistry,
) -> Value {
    let mut value = serde_json::to_value(target).unwrap_or_else(|_| json!({}));
    value["agent_source"] = json!(registry.source_for_agent(&target.agent));
    value
}

fn build_registry(
    adapters: Vec<AgentAdapter>,
    config_locations: Vec<Value>,
) -> std::result::Result<AgentAdapterRegistry, CommandFailure> {
    let mut by_id = BTreeMap::new();
    for adapter in &adapters {
        if let Some(previous) = by_id.insert(adapter.id.clone(), adapter.clone()) {
            return Err(adapter_failure(
                format!(
                    "adapter id '{}' is defined by both '{}' and '{}'",
                    adapter.id, previous.source, adapter.source
                ),
                "ADAPTER_DUPLICATE_ID",
                adapter.config_path.as_deref(),
            ));
        }
    }
    Ok(AgentAdapterRegistry {
        adapters,
        by_id,
        config_locations,
    })
}

fn built_in_adapters(root: &Path, home: Option<&Path>) -> Vec<AgentAdapter> {
    let dirs_by_agent = if home.is_some() {
        resolve_agent_skill_dirs(root)
            .all
            .into_iter()
            .map(|dir| (dir.agent.to_string(), vec![dir.path]))
            .collect::<BTreeMap<_, _>>()
    } else {
        BTreeMap::new()
    };
    built_in_agent_specs()
        .into_iter()
        .map(|id| {
            let reload = built_in_reload(id);
            AgentAdapter {
                adapter_api: ADAPTER_API_V2.to_string(),
                id: id.to_string(),
                source: SOURCE_BUILT_IN.to_string(),
                fidelity: metadata::built_in_fidelity(id),
                supported_scopes: vec!["user".to_string(), "project".to_string()],
                projection_methods: vec![
                    "symlink".to_string(),
                    "copy".to_string(),
                    "materialize".to_string(),
                ],
                skill_entrypoint: "SKILL.md".to_string(),
                capabilities: capabilities_from_reload(&reload),
                default_skill_dirs: built_in_default_skill_dirs(id, dirs_by_agent.get(id), home),
                discovery_roots: built_in_discovery_roots(id, dirs_by_agent.get(id), home),
                visibility: built_in_visibility(id),
                reload,
                config_path: None,
            }
        })
        .collect()
}

fn built_in_agent_specs() -> [&'static str; 10] {
    [
        "claude",
        "codex",
        "cursor",
        "windsurf",
        "cline",
        "copilot",
        "aider",
        "opencode",
        "gemini-cli",
        "goose",
    ]
}

fn load_external_adapter(
    path: &Path,
    home: Option<&Path>,
) -> std::result::Result<AgentAdapter, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(|err| adapter_io_failure(path, err))?;
    let probe = serde_json::from_str::<AdapterApiProbe>(&raw).map_err(|err| {
        adapter_failure(
            format!("adapter config '{}' is invalid JSON: {err}", path.display()),
            "ADAPTER_JSON_INVALID",
            Some(path),
        )
    })?;
    match probe.adapter_api.as_str() {
        ADAPTER_API_V1 => load_external_adapter_v1(&raw, path, home),
        ADAPTER_API_V2 => load_external_adapter_v2(&raw, path, home),
        _ => Err(adapter_failure(
            format!(
                "adapter '{}' uses unsupported adapter_api '{}'",
                probe.id.as_deref().unwrap_or("<unknown>"),
                probe.adapter_api
            ),
            "ADAPTER_API_UNSUPPORTED",
            Some(path),
        )),
    }
}

fn load_external_adapter_v1(
    raw: &str,
    path: &Path,
    home: Option<&Path>,
) -> std::result::Result<AgentAdapter, CommandFailure> {
    let record = serde_json::from_str::<ExternalAdapterV1Record>(raw)
        .map_err(|err| adapter_json_invalid(path, err))?;
    validate_external_v1_record(&record, path)?;
    let default_skill_dirs = record
        .default_skill_dirs
        .iter()
        .map(|raw| expand_adapter_path(raw, home, path))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let discovery_roots = v1_discovery_roots(&record, &default_skill_dirs);
    let reload_required = record.capabilities.reload_required;
    Ok(AgentAdapter {
        adapter_api: ADAPTER_API_V1.to_string(),
        id: record.id,
        source: SOURCE_EXTERNAL.to_string(),
        fidelity: AdapterFidelity::Generic,
        supported_scopes: record.supported_scopes,
        projection_methods: record.projection_methods,
        skill_entrypoint: record.skill_entrypoint,
        capabilities: record.capabilities,
        default_skill_dirs,
        discovery_roots,
        visibility: default_visibility(),
        reload: reload_from_capability(reload_required),
        config_path: Some(path.to_path_buf()),
    })
}

fn load_external_adapter_v2(
    raw: &str,
    path: &Path,
    home: Option<&Path>,
) -> std::result::Result<AgentAdapter, CommandFailure> {
    let record = serde_json::from_str::<ExternalAdapterV2Record>(raw)
        .map_err(|err| adapter_json_invalid(path, err))?;
    validate_external_v2_record(&record, path)?;
    let discovery_roots = record
        .discovery_roots
        .iter()
        .map(|root| external_discovery_root(root, home, path))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let default_skill_dirs = discovery_roots
        .iter()
        .filter(|root| root.scan_eligible && root.available && root.scope == "user")
        .map(|root| resolve_root_template(&root.path_template, path))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(AgentAdapter {
        adapter_api: ADAPTER_API_V2.to_string(),
        id: record.id,
        source: SOURCE_EXTERNAL.to_string(),
        fidelity: AdapterFidelity::Generic,
        supported_scopes: record.supported_scopes,
        projection_methods: record.projection_methods,
        skill_entrypoint: record.skill_entrypoint,
        capabilities: record.capabilities,
        default_skill_dirs,
        discovery_roots,
        visibility: external_visibility(record.visibility),
        reload: AdapterReload {
            strategy: record.reload.strategy,
            hot_reload: record.reload.hot_reload,
            notes: record.reload.notes,
        },
        config_path: Some(path.to_path_buf()),
    })
}

fn validate_external_v1_record(
    record: &ExternalAdapterV1Record,
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    if record.adapter_api != ADAPTER_API_V1 {
        return Err(adapter_failure(
            format!(
                "adapter '{}' uses unsupported adapter_api '{}'",
                record.id, record.adapter_api
            ),
            "ADAPTER_API_UNSUPPORTED",
            Some(path),
        ));
    }
    validate_adapter_id(&record.id, path)?;
    validate_string_set(
        "supported_scopes",
        &record.supported_scopes,
        &["user", "project"],
        path,
    )?;
    validate_string_set(
        "projection_methods",
        &record.projection_methods,
        &["copy", "symlink", "materialize"],
        path,
    )?;
    if record.skill_entrypoint != "SKILL.md" {
        return Err(adapter_failure(
            format!(
                "adapter '{}' declares unsupported skill_entrypoint '{}'",
                record.id, record.skill_entrypoint
            ),
            "ADAPTER_ENTRYPOINT_UNSUPPORTED",
            Some(path),
        ));
    }
    if record.capabilities.automatic_discovery && record.default_skill_dirs.is_empty() {
        return Err(adapter_failure(
            format!(
                "adapter '{}' enables automatic_discovery but has no default_skill_dirs",
                record.id
            ),
            "ADAPTER_DISCOVERY_PATHS_MISSING",
            Some(path),
        ));
    }
    Ok(())
}

fn validate_external_v2_record(
    record: &ExternalAdapterV2Record,
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    if record.adapter_api != ADAPTER_API_V2 {
        return Err(adapter_failure(
            format!(
                "adapter '{}' uses unsupported adapter_api '{}'",
                record.id, record.adapter_api
            ),
            "ADAPTER_API_UNSUPPORTED",
            Some(path),
        ));
    }
    validate_adapter_id(&record.id, path)?;
    validate_string_set(
        "supported_scopes",
        &record.supported_scopes,
        &["user", "project"],
        path,
    )?;
    validate_string_set(
        "projection_methods",
        &record.projection_methods,
        &["copy", "symlink", "materialize"],
        path,
    )?;
    if record.skill_entrypoint != "SKILL.md" {
        return Err(adapter_failure(
            format!(
                "adapter '{}' declares unsupported skill_entrypoint '{}'",
                record.id, record.skill_entrypoint
            ),
            "ADAPTER_ENTRYPOINT_UNSUPPORTED",
            Some(path),
        ));
    }
    if record.capabilities.automatic_discovery && record.discovery_roots.is_empty() {
        return Err(adapter_failure(
            format!(
                "adapter '{}' enables automatic_discovery but has no discovery_roots",
                record.id
            ),
            "ADAPTER_DISCOVERY_PATHS_MISSING",
            Some(path),
        ));
    }
    for root in &record.discovery_roots {
        validate_discovery_root(root, &record.supported_scopes, path)?;
    }
    validate_visibility(&record.visibility, &record.projection_methods, path)?;
    validate_string_set(
        "reload.strategy",
        std::slice::from_ref(&record.reload.strategy),
        &[
            "no-reload-required",
            "new-session-recommended",
            "restart-required",
            "unknown",
        ],
        path,
    )?;
    Ok(())
}

fn validate_adapter_id(id: &str, path: &Path) -> std::result::Result<(), CommandFailure> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Err(adapter_failure(
            format!("adapter id '{id}' must match [a-z0-9_-]{{1,64}}"),
            "ADAPTER_ID_INVALID",
            Some(path),
        ));
    }
    Ok(())
}

fn validate_string_set(
    field: &str,
    values: &[String],
    allowed: &[&str],
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    if values.is_empty() {
        return Err(adapter_failure(
            format!("adapter field '{field}' must not be empty"),
            "ADAPTER_FIELD_EMPTY",
            Some(path),
        ));
    }
    for value in values {
        if !allowed.contains(&value.as_str()) {
            return Err(adapter_failure(
                format!("adapter field '{field}' contains unsupported value '{value}'"),
                "ADAPTER_FIELD_UNSUPPORTED",
                Some(path),
            ));
        }
    }
    Ok(())
}

fn expand_adapter_path(
    raw: &str,
    home: Option<&Path>,
    config_path: &Path,
) -> std::result::Result<PathBuf, CommandFailure> {
    if raw.trim().is_empty() {
        return Err(adapter_failure(
            "adapter default_skill_dirs must not contain empty paths",
            "ADAPTER_DISCOVERY_PATH_INVALID",
            Some(config_path),
        ));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.map(|home| home.join(rest)).ok_or_else(|| {
            adapter_failure(
                format!(
                    "adapter path '{}' requires HOME or USERPROFILE to be set",
                    raw
                ),
                "ADAPTER_HOME_UNAVAILABLE",
                Some(config_path),
            )
        });
    }
    Ok(PathBuf::from(raw))
}

enum AdapterLocationKind {
    Directory,
    File,
}

struct AdapterLocation {
    kind: AdapterLocationKind,
    path: PathBuf,
}

fn external_adapter_locations(root: &Path) -> Vec<AdapterLocation> {
    let mut locations = vec![AdapterLocation {
        kind: AdapterLocationKind::Directory,
        path: root.join("adapters"),
    }];
    if let Some(paths) = std::env::var_os("LOOM_ADAPTER_PATH") {
        for path in std::env::split_paths(&paths) {
            let kind = if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                AdapterLocationKind::File
            } else {
                AdapterLocationKind::Directory
            };
            locations.push(AdapterLocation { kind, path });
        }
    }
    locations
}

fn adapter_io_failure(path: &Path, err: std::io::Error) -> CommandFailure {
    adapter_failure(
        format!("failed to read adapter config '{}': {err}", path.display()),
        "ADAPTER_IO_ERROR",
        Some(path),
    )
}

fn adapter_failure(
    message: impl Into<String>,
    reason: &str,
    path: Option<&Path>,
) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::AdapterInvalid, message);
    failure.details = json!({
        "reason": reason,
        "path": path.map(|path| path.display().to_string()),
    });
    failure
}
