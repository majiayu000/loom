use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::commands::CommandFailure;
use crate::state::{AppContext, home_dir, resolve_agent_skill_dirs};
use crate::state_model::RegistryProjectionTarget;
use crate::types::ErrorCode;

pub(crate) const ADAPTER_API_VERSION: &str = "1";
pub(crate) const SOURCE_BUILT_IN: &str = "built-in";
pub(crate) const SOURCE_EXTERNAL: &str = "external";
pub(crate) const SOURCE_UNKNOWN: &str = "unknown";

#[derive(Debug, Clone)]
pub(crate) struct AgentAdapter {
    pub id: String,
    pub source: String,
    pub supported_scopes: Vec<String>,
    pub projection_methods: Vec<String>,
    pub skill_entrypoint: String,
    pub capabilities: AdapterCapabilities,
    pub default_skill_dirs: Vec<PathBuf>,
    pub config_path: Option<PathBuf>,
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
pub(crate) struct AgentAdapterRegistry {
    adapters: Vec<AgentAdapter>,
    by_id: BTreeMap<String, AgentAdapter>,
    config_locations: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct ExternalAdapterRecord {
    adapter_api: String,
    id: String,
    supported_scopes: Vec<String>,
    projection_methods: Vec<String>,
    skill_entrypoint: String,
    capabilities: AdapterCapabilities,
    #[serde(default)]
    default_skill_dirs: Vec<String>,
    #[serde(default)]
    health_checks: Vec<String>,
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

    pub(crate) fn config_locations(&self) -> &[Value] {
        &self.config_locations
    }

    pub(crate) fn adapters_json(&self) -> Vec<Value> {
        self.adapters
            .iter()
            .map(|adapter| {
                json!({
                    "adapter_api": ADAPTER_API_VERSION,
                    "id": adapter.id,
                    "source": adapter.source,
                    "supported_scopes": adapter.supported_scopes,
                    "projection_methods": adapter.projection_methods,
                    "skill_entrypoint": adapter.skill_entrypoint,
                    "capabilities": {
                        "automatic_discovery": adapter.capabilities.automatic_discovery,
                        "explicit_invocation": adapter.capabilities.explicit_invocation,
                        "reload_required": adapter.capabilities.reload_required,
                    },
                    "default_skill_dirs": adapter.default_skill_dirs.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
                    "config_path": adapter.config_path.as_ref().map(|path| path.display().to_string()),
                })
            })
            .collect()
    }
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
        .map(|id| AgentAdapter {
            id: id.to_string(),
            source: SOURCE_BUILT_IN.to_string(),
            supported_scopes: vec!["user".to_string(), "project".to_string()],
            projection_methods: vec![
                "symlink".to_string(),
                "copy".to_string(),
                "materialize".to_string(),
            ],
            skill_entrypoint: "SKILL.md".to_string(),
            capabilities: AdapterCapabilities {
                automatic_discovery: true,
                explicit_invocation: true,
                reload_required: false,
            },
            default_skill_dirs: dirs_by_agent.get(id).cloned().unwrap_or_default(),
            config_path: None,
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
    let record = serde_json::from_str::<ExternalAdapterRecord>(&raw).map_err(|err| {
        adapter_failure(
            format!("adapter config '{}' is invalid JSON: {err}", path.display()),
            "ADAPTER_JSON_INVALID",
            Some(path),
        )
    })?;
    validate_external_record(&record, path)?;
    let default_skill_dirs = record
        .default_skill_dirs
        .iter()
        .map(|raw| expand_adapter_path(raw, home, path))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(AgentAdapter {
        id: record.id,
        source: SOURCE_EXTERNAL.to_string(),
        supported_scopes: record.supported_scopes,
        projection_methods: record.projection_methods,
        skill_entrypoint: record.skill_entrypoint,
        capabilities: record.capabilities,
        default_skill_dirs,
        config_path: Some(path.to_path_buf()),
    })
}

fn validate_external_record(
    record: &ExternalAdapterRecord,
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    if record.adapter_api != ADAPTER_API_VERSION {
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
    for check in &record.health_checks {
        if check.trim().is_empty() {
            return Err(adapter_failure(
                format!("adapter '{}' has an empty health check", record.id),
                "ADAPTER_HEALTH_CHECK_INVALID",
                Some(path),
            ));
        }
    }
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
    let allowed_set = allowed.iter().copied().collect::<BTreeSet<_>>();
    for value in values {
        if !allowed_set.contains(value.as_str()) {
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
