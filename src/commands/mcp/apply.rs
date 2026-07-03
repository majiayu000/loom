use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use toml_edit::{Array, DocumentMut, Item, Table, Value as TomlValue, value};

use crate::cli::McpApplyArgs;
use crate::commands::CommandFailure;
use crate::commands::codex_config::codex_config_path;
use crate::commands::helpers::{map_io, validate_non_empty};
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::artifact::{load_reviewed_mcp_plan, skill_source_digest};
use super::source_policy::{McpResolvedSource, tool_availability};
use super::utils::{digest_json, digest_str};
use super::{McpPlan, McpRequirement};

const APPLY_RECORD_SCHEMA: &str = "mcp-apply-record-v1";

struct ApplyRecordLock {
    path: PathBuf,
}

struct ConfigSnapshot {
    raw: Option<String>,
    digest: Option<String>,
}

struct RenderedServer {
    command: String,
    args: Vec<String>,
    env_names: Vec<String>,
}

impl Drop for ApplyRecordLock {
    fn drop(&mut self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => eprintln!(
                "failed to remove MCP apply lock {}: {}",
                self.path.display(),
                err
            ),
        }
    }
}

pub(super) fn cmd_mcp_apply(
    ctx: &AppContext,
    args: &McpApplyArgs,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    validate_non_empty("idempotency-key", &args.idempotency_key)?;
    let plan = load_reviewed_mcp_plan(ctx, &args.plan)?;
    ensure_supported_plan(&plan)?;
    validate_skill_source(ctx, &plan)?;
    validate_sources(&plan)?;
    validate_approvals(&plan, &args.approvals)?;
    validate_env(&plan)?;
    validate_tools(&plan)?;

    let config_path = planned_codex_config_path(&plan)?;
    let plan_digest = digest_json(&plan)?;
    let key_digest = digest_str(&args.idempotency_key);
    let record_path = apply_record_path(ctx, &key_digest);
    if record_path.is_file() {
        let record = load_apply_record(&record_path)?;
        return replay_existing_apply(&plan, &config_path, &plan_digest, &key_digest, &record);
    }

    let snapshot = read_config_snapshot(&config_path)?;
    validate_config_preimage(&plan, &snapshot)?;
    let rendered = render_codex_config(snapshot.raw.as_deref(), &plan)?;
    let rendered_digest = digest_str(&rendered);

    let _record_lock = claim_apply_record_lock(&record_path, &key_digest)?;
    if record_path.is_file() {
        let record = load_apply_record(&record_path)?;
        return replay_existing_apply(&plan, &config_path, &plan_digest, &key_digest, &record);
    }
    let snapshot_after_lock = read_config_snapshot(&config_path)?;
    if snapshot_after_lock.digest != snapshot.digest {
        return Err(policy_blocked(
            "MCP config changed during apply; create a new MCP plan",
            json!({
                "plan_id": plan.plan_id,
                "config_path": config_path.display().to_string(),
                "target_writes_performed": false,
            }),
        ));
    }

    let target_writes_performed = snapshot.digest.as_deref() != Some(rendered_digest.as_str());
    if target_writes_performed {
        write_atomic(&config_path, &rendered).map_err(map_io)?;
    }
    write_apply_record(
        ctx,
        &record_path,
        &plan,
        &plan_digest,
        &key_digest,
        &config_path,
        &rendered_digest,
    )?;

    Ok(apply_response(
        &plan,
        &config_path,
        &key_digest,
        false,
        target_writes_performed,
        &rendered_digest,
    ))
}

fn ensure_supported_plan(plan: &McpPlan) -> std::result::Result<(), CommandFailure> {
    if plan.agent != "codex" {
        return Err(policy_blocked(
            "MCP apply supports only adapter-reviewed Codex config plans",
            json!({
                "agent": plan.agent,
                "target_writes_performed": false,
            }),
        ));
    }
    if plan.requirements.is_empty() {
        return Err(policy_blocked(
            "MCP apply requires at least one reviewed MCP server requirement",
            json!({
                "plan_id": plan.plan_id,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn validate_skill_source(
    ctx: &AppContext,
    plan: &McpPlan,
) -> std::result::Result<(), CommandFailure> {
    let current = skill_source_digest(&ctx.skill_path(&plan.skill))?;
    if current != plan.skill_source_digest {
        return Err(policy_blocked(
            "MCP plan skill source is stale; create a new MCP plan",
            json!({
                "plan_id": plan.plan_id,
                "skill": plan.skill,
                "expected": plan.skill_source_digest,
                "actual": current,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn validate_sources(plan: &McpPlan) -> std::result::Result<(), CommandFailure> {
    let blocked = plan
        .resolved_sources
        .iter()
        .filter(|source| !source.pinned || source.policy == "blocked_unpinned")
        .map(|source| {
            json!({
                "server": source.server,
                "source": source.locator,
                "policy": source.policy,
                "pinned": source.pinned,
            })
        })
        .collect::<Vec<_>>();
    if !blocked.is_empty() {
        return Err(policy_blocked(
            "MCP apply requires immutable pinned MCP server sources",
            json!({
                "plan_id": plan.plan_id,
                "blocked_sources": blocked,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn validate_approvals(
    plan: &McpPlan,
    raw_approvals: &[String],
) -> std::result::Result<(), CommandFailure> {
    let required = required_approvals(plan);
    let provided = split_approvals(raw_approvals);
    let missing = required
        .iter()
        .filter(|approval| !provided.contains(*approval))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(policy_blocked(
            "MCP apply requires approval token(s)",
            json!({
                "plan_id": plan.plan_id,
                "required_approvals": required,
                "missing_approvals": missing,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn required_approvals(plan: &McpPlan) -> Vec<String> {
    let mut approvals = BTreeSet::new();
    approvals.extend(
        plan.approvals_required
            .iter()
            .filter(|approval| !approval.is_empty())
            .cloned(),
    );
    for action in &plan.actions {
        if let Some(approval) = &action.approval_required
            && !approval.is_empty()
        {
            approvals.insert(approval.clone());
        }
    }
    approvals.into_iter().collect()
}

fn split_approvals(raw_approvals: &[String]) -> BTreeSet<String> {
    raw_approvals
        .iter()
        .flat_map(|raw| raw.split(','))
        .map(str::trim)
        .filter(|approval| !approval.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn validate_env(plan: &McpPlan) -> std::result::Result<(), CommandFailure> {
    let mut required = BTreeSet::new();
    for req in &plan.requirements {
        if let Some(name) = &req.auth_env {
            required.insert(name.clone());
        }
    }
    for env in &plan.env {
        required.insert(env.name.clone());
    }
    let missing = required
        .into_iter()
        .filter(|name| std::env::var_os(name).is_none())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(policy_blocked(
            "MCP apply requires environment variable(s) to be present",
            json!({
                "plan_id": plan.plan_id,
                "missing_env": missing,
                "redacted": true,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn validate_tools(plan: &McpPlan) -> std::result::Result<(), CommandFailure> {
    let missing = tool_availability(&plan.resolved_sources)
        .into_iter()
        .filter(|tool| tool["found"] == json!(false))
        .map(|tool| tool["tool"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(policy_blocked(
            "MCP apply requires MCP launcher tool(s) to be installed",
            json!({
                "plan_id": plan.plan_id,
                "missing_tools": missing,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn planned_codex_config_path(plan: &McpPlan) -> std::result::Result<PathBuf, CommandFailure> {
    let planned = plan
        .actions
        .iter()
        .find(|action| action.kind == "write_agent_config")
        .and_then(|action| action.path.as_deref())
        .ok_or_else(|| {
            policy_blocked(
                "MCP plan does not include an adapter-supported config write",
                json!({
                    "plan_id": plan.plan_id,
                    "target_writes_performed": false,
                }),
            )
        })?;
    let planned = PathBuf::from(planned);
    let current = codex_config_path()?;
    if current != planned {
        return Err(policy_blocked(
            "MCP plan Codex config path no longer matches adapter metadata",
            json!({
                "plan_id": plan.plan_id,
                "expected": planned.display().to_string(),
                "actual": current.display().to_string(),
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(planned)
}

fn read_config_snapshot(path: &Path) -> std::result::Result<ConfigSnapshot, CommandFailure> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let digest = digest_str(&raw);
            Ok(ConfigSnapshot {
                raw: Some(raw),
                digest: Some(digest),
            })
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(ConfigSnapshot {
            raw: None,
            digest: None,
        }),
        Err(err) => Err(map_io(err)),
    }
}

fn validate_config_preimage(
    plan: &McpPlan,
    snapshot: &ConfigSnapshot,
) -> std::result::Result<(), CommandFailure> {
    let expected = planned_preimage_digest(plan)?;
    if snapshot.digest != expected {
        return Err(policy_blocked(
            "MCP config changed since the reviewed plan; create a new MCP plan",
            json!({
                "plan_id": plan.plan_id,
                "expected": expected,
                "actual": snapshot.digest,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn planned_preimage_digest(plan: &McpPlan) -> std::result::Result<Option<String>, CommandFailure> {
    let mut preimages = BTreeSet::new();
    for action in plan
        .actions
        .iter()
        .filter(|action| action.kind == "write_agent_config")
    {
        match action.details.get("preimage_digest") {
            Some(Value::Null) | None => {
                preimages.insert(None);
            }
            Some(Value::String(value)) => {
                preimages.insert(Some(value.clone()));
            }
            Some(_) => {
                return Err(CommandFailure::new(
                    ErrorCode::SchemaMismatch,
                    "MCP plan config preimage digest is malformed",
                ));
            }
        }
    }
    if preimages.len() > 1 {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "MCP plan contains conflicting config preimage digests",
        ));
    }
    Ok(preimages.into_iter().next().flatten())
}

fn render_codex_config(
    raw: Option<&str>,
    plan: &McpPlan,
) -> std::result::Result<String, CommandFailure> {
    let mut doc = match raw {
        Some(raw) => raw.parse::<DocumentMut>().map_err(|err| {
            let mut failure = CommandFailure::new(
                ErrorCode::SchemaMismatch,
                "Codex config is malformed; MCP apply requires a parseable TOML config",
            );
            failure.details = json!({
                "plan_id": plan.plan_id,
                "error": err.to_string(),
                "target_writes_performed": false,
            });
            failure
        })?,
        None => DocumentMut::new(),
    };

    if !doc.as_table().contains_key("mcp_servers") {
        doc["mcp_servers"] = Item::Table(Table::new());
    }
    let Some(servers_table) = doc["mcp_servers"].as_table_like_mut() else {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "Codex config mcp_servers entry is not a table",
        ));
    };

    let sources = sources_by_server(plan);
    for req in plan
        .requirements
        .iter()
        .filter(|req| should_write_server(plan, &req.server))
    {
        let source = sources.get(&req.server).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SchemaMismatch,
                format!("MCP plan is missing resolved source for '{}'", req.server),
            )
        })?;
        let rendered = rendered_server(req, source)?;
        servers_table.insert(&req.server, Item::Table(server_table(&rendered)));
    }

    let rendered = doc.to_string();
    rendered.parse::<DocumentMut>().map_err(|err| {
        let mut failure = CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "rendered Codex MCP config failed TOML validation",
        );
        failure.details = json!({"error": err.to_string(), "target_writes_performed": false});
        failure
    })?;
    Ok(rendered)
}

fn sources_by_server(plan: &McpPlan) -> BTreeMap<String, McpResolvedSource> {
    plan.resolved_sources
        .iter()
        .map(|source| (source.server.clone(), source.clone()))
        .collect()
}

fn should_write_server(plan: &McpPlan, server: &str) -> bool {
    plan.actions
        .iter()
        .find(|action| action.kind == "install_server" && action.server.as_deref() == Some(server))
        .and_then(|action| action.details.get("configured"))
        .and_then(|configured| configured.get("status"))
        .and_then(Value::as_str)
        != Some("compatible")
}

fn rendered_server(
    req: &McpRequirement,
    source: &McpResolvedSource,
) -> std::result::Result<RenderedServer, CommandFailure> {
    if req.transport != "stdio" {
        return Err(policy_blocked(
            "MCP apply supports only stdio MCP server config in this slice",
            json!({
                "server": req.server,
                "transport": req.transport,
                "target_writes_performed": false,
            }),
        ));
    }
    let (command, args) = match source.kind.as_str() {
        "npm" => {
            let package = source.package.as_deref().ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::SchemaMismatch,
                    "MCP npm source is missing package",
                )
            })?;
            let version = source.version.as_deref().ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::SchemaMismatch,
                    "MCP npm source is missing version",
                )
            })?;
            (
                "npx".to_string(),
                vec!["-y".to_string(), format!("{package}@{version}")],
            )
        }
        "local" => {
            let command = source
                .locator
                .strip_prefix("local:")
                .and_then(|raw| raw.split_once("@sha256:").map(|(path, _)| path))
                .filter(|path| !path.is_empty())
                .ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::SchemaMismatch,
                        "MCP local source is missing a digest-pinned path",
                    )
                })?;
            (command.to_string(), Vec::new())
        }
        other => {
            return Err(policy_blocked(
                "MCP apply cannot render this MCP source kind yet",
                json!({
                    "server": req.server,
                    "source_kind": other,
                    "target_writes_performed": false,
                }),
            ));
        }
    };
    Ok(RenderedServer {
        command,
        args,
        env_names: req.auth_env.iter().cloned().collect(),
    })
}

fn server_table(rendered: &RenderedServer) -> Table {
    let mut table = Table::new();
    table["command"] = value(rendered.command.clone());
    if !rendered.args.is_empty() {
        let mut args = Array::default();
        for arg in &rendered.args {
            args.push(arg.as_str());
        }
        table["args"] = Item::Value(TomlValue::Array(args));
    }
    if !rendered.env_names.is_empty() {
        let mut env_table = Table::new();
        for env_name in &rendered.env_names {
            env_table[env_name] = value(format!("env:{env_name}"));
        }
        table["env"] = Item::Table(env_table);
    }
    table
}

fn replay_existing_apply(
    plan: &McpPlan,
    config_path: &Path,
    plan_digest: &str,
    key_digest: &str,
    record: &Value,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    if record.get("schema_version").and_then(Value::as_str) != Some(APPLY_RECORD_SCHEMA)
        || record.get("plan_digest").and_then(Value::as_str) != Some(plan_digest)
        || record.get("idempotency_key_digest").and_then(Value::as_str) != Some(key_digest)
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "MCP idempotency key was already used for a different plan",
        ));
    }
    let snapshot = read_config_snapshot(config_path)?;
    let expected = record
        .get("config_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SchemaMismatch,
                "MCP apply record is missing config digest",
            )
        })?;
    if snapshot.digest.as_deref() != Some(expected) {
        return Err(policy_blocked(
            "MCP apply replay config no longer matches the reviewed plan",
            json!({
                "plan_id": plan.plan_id,
                "config_path": config_path.display().to_string(),
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(apply_response(
        plan,
        config_path,
        key_digest,
        true,
        false,
        expected,
    ))
}

fn load_apply_record(path: &Path) -> std::result::Result<Value, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    serde_json::from_str(&raw)
        .map_err(|err| CommandFailure::new(ErrorCode::ArgInvalid, err.to_string()))
}

fn claim_apply_record_lock(
    record_path: &Path,
    key_digest: &str,
) -> std::result::Result<ApplyRecordLock, CommandFailure> {
    let lock_path = record_path.with_extension("lock");
    fs::create_dir_all(lock_path.parent().unwrap_or(Path::new("."))).map_err(map_io)?;
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(mut file) => {
            writeln!(file, "{key_digest}").map_err(map_io)?;
            file.sync_all().map_err(map_io)?;
            Ok(ApplyRecordLock { path: lock_path })
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Err(policy_blocked(
            "MCP apply idempotency key is already in progress",
            json!({
                "idempotency_key_digest": key_digest,
                "target_writes_performed": false,
            }),
        )),
        Err(err) => Err(map_io(err)),
    }
}

fn write_apply_record(
    ctx: &AppContext,
    path: &Path,
    plan: &McpPlan,
    plan_digest: &str,
    key_digest: &str,
    config_path: &Path,
    config_digest: &str,
) -> std::result::Result<(), CommandFailure> {
    let record = json!({
        "schema_version": APPLY_RECORD_SCHEMA,
        "plan_id": plan.plan_id,
        "plan_digest": plan_digest,
        "idempotency_key_digest": key_digest,
        "agent": plan.agent,
        "skill": plan.skill,
        "config_path": config_path.display().to_string(),
        "config_digest": config_digest,
    });
    fs::create_dir_all(path.parent().unwrap_or(&ctx.state_dir)).map_err(map_io)?;
    let mut raw = serde_json::to_string_pretty(&record).map_err(map_io)?;
    raw.push('\n');
    write_atomic(path, &raw).map_err(map_io)
}

fn apply_record_path(ctx: &AppContext, key_digest: &str) -> PathBuf {
    let suffix = key_digest
        .strip_prefix("sha256:")
        .unwrap_or(key_digest)
        .replace(['/', '\\', ':'], "_");
    ctx.state_dir
        .join("mcp")
        .join("applies")
        .join(format!("{suffix}.json"))
}

fn apply_response(
    plan: &McpPlan,
    config_path: &Path,
    key_digest: &str,
    idempotent_replay: bool,
    target_writes_performed: bool,
    config_digest: &str,
) -> (Value, Meta) {
    (
        json!({
            "plan_id": plan.plan_id,
            "agent": plan.agent,
            "skill": plan.skill,
            "config_path": config_path.display().to_string(),
            "config_digest": config_digest,
            "idempotency_key_digest": key_digest,
            "idempotent_replay": idempotent_replay,
            "target_writes_performed": target_writes_performed,
            "servers": plan.requirements.iter().map(|req| req.server.clone()).collect::<Vec<_>>(),
            "restart_required": target_writes_performed,
            "next_actions": if target_writes_performed {
                vec!["restart codex or start a new Codex session".to_string()]
            } else {
                Vec::new()
            },
            "secret_values_written": false,
        }),
        Meta::default(),
    )
}

fn policy_blocked(message: &str, details: Value) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    failure.details = details;
    failure
}
