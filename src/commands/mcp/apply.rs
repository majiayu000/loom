use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value as TomlValue, value};

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
use super::utils::{digest_bytes, digest_json, digest_str};
use super::{McpPlan, McpRequirement, current_required_mcp_plan_inputs};

mod record;

use record::{
    apply_record_path, apply_response, claim_apply_lock, config_lock_path, load_apply_record,
    replay_existing_apply, write_apply_record,
};

struct ConfigSnapshot {
    raw: Option<String>,
    digest: Option<String>,
}

struct RenderedServer {
    command: String,
    args: Vec<String>,
    env_vars: Vec<String>,
}

pub(super) fn cmd_mcp_apply(
    ctx: &AppContext,
    args: &McpApplyArgs,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    validate_non_empty("idempotency-key", &args.idempotency_key)?;
    let plan = load_reviewed_mcp_plan(ctx, &args.plan)?;
    ensure_supported_plan(&plan)?;
    let config_path = planned_codex_config_path(&plan)?;
    let plan_digest = digest_json(&plan)?;
    let key_digest = digest_str(&args.idempotency_key);
    let record_path = apply_record_path(ctx, &key_digest);
    if record_path.is_file() {
        let record = load_apply_record(&record_path)?;
        return replay_existing_apply(&plan, &config_path, &plan_digest, &key_digest, &record);
    }

    let _config_lock = claim_apply_lock(
        &config_lock_path(ctx, &config_path),
        "MCP config apply is already in progress",
        json!({
            "plan_id": plan.plan_id,
            "config_path": config_path.display().to_string(),
            "target_writes_performed": false,
        }),
    )?;
    if record_path.is_file() {
        let record = load_apply_record(&record_path)?;
        return replay_existing_apply(&plan, &config_path, &plan_digest, &key_digest, &record);
    }

    validate_skill_source(ctx, &plan)?;
    validate_plan_matches_current_skill(ctx, &plan)?;
    validate_sources(&plan)?;

    let snapshot = read_config_snapshot(&config_path)?;
    let rendered = render_codex_config(snapshot.raw.as_deref(), &plan)?;
    let rendered_digest = digest_str(&rendered);
    if snapshot.digest.as_deref() == Some(rendered_digest.as_str()) {
        let _record_lock = claim_apply_lock(
            &record_path.with_extension("lock"),
            "MCP apply idempotency key is already in progress",
            json!({
                "idempotency_key_digest": key_digest,
                "target_writes_performed": false,
            }),
        )?;
        if record_path.is_file() {
            let record = load_apply_record(&record_path)?;
            return replay_existing_apply(&plan, &config_path, &plan_digest, &key_digest, &record);
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
        return Ok(apply_response(
            &plan,
            &config_path,
            &key_digest,
            true,
            false,
            &rendered_digest,
        ));
    }

    validate_approvals(&plan, &args.approvals)?;
    validate_env(&plan)?;
    validate_tools(&plan)?;
    validate_config_preimage(&plan, &snapshot)?;

    let _record_lock = claim_apply_lock(
        &record_path.with_extension("lock"),
        "MCP apply idempotency key is already in progress",
        json!({
            "idempotency_key_digest": key_digest,
            "target_writes_performed": false,
        }),
    )?;
    if record_path.is_file() {
        let record = load_apply_record(&record_path)?;
        return replay_existing_apply(&plan, &config_path, &plan_digest, &key_digest, &record);
    }
    let snapshot_after_lock = read_config_snapshot(&config_path)?;
    if snapshot_after_lock.digest.as_deref() == Some(rendered_digest.as_str()) {
        write_apply_record(
            ctx,
            &record_path,
            &plan,
            &plan_digest,
            &key_digest,
            &config_path,
            &rendered_digest,
        )?;
        return Ok(apply_response(
            &plan,
            &config_path,
            &key_digest,
            true,
            false,
            &rendered_digest,
        ));
    } else if snapshot_after_lock.digest != snapshot.digest {
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

fn validate_plan_matches_current_skill(
    ctx: &AppContext,
    plan: &McpPlan,
) -> std::result::Result<(), CommandFailure> {
    let (requirements, resolved_sources, _, _) =
        current_required_mcp_plan_inputs(ctx, &plan.skill, &plan.agent)?;
    if requirements != plan.requirements || resolved_sources != plan.resolved_sources {
        return Err(policy_blocked(
            "MCP plan reviewed sources no longer match the current skill requirements",
            json!({
                "plan_id": plan.plan_id,
                "skill": plan.skill,
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
    for source in plan
        .resolved_sources
        .iter()
        .filter(|source| source.kind == "local")
    {
        validate_local_source_digest(plan, source)?;
    }
    Ok(())
}

fn validate_local_source_digest(
    plan: &McpPlan,
    source: &McpResolvedSource,
) -> std::result::Result<(), CommandFailure> {
    let (path, digest) = local_source_path_and_digest(source)?;
    let expected = format!("sha256:{digest}");
    let bytes = fs::read(path).map_err(|err| {
        policy_blocked(
            "MCP local source could not be read during apply",
            json!({
                "plan_id": plan.plan_id,
                "server": source.server,
                "source": source.locator,
                "error": err.to_string(),
                "target_writes_performed": false,
            }),
        )
    })?;
    let actual = digest_bytes(&bytes);
    if actual != expected {
        return Err(policy_blocked(
            "MCP local source digest changed since the reviewed plan",
            json!({
                "plan_id": plan.plan_id,
                "server": source.server,
                "source": source.locator,
                "expected": expected,
                "actual": actual,
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
    for req in &plan.requirements {
        let source = sources.get(&req.server).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SchemaMismatch,
                format!("MCP plan is missing resolved source for '{}'", req.server),
            )
        })?;
        let rendered = rendered_server(plan, req, source)?;
        if server_matches_rendered(&*servers_table, &req.server, &rendered) {
            continue;
        }
        let mut table = servers_table
            .get(&req.server)
            .and_then(Item::as_table)
            .cloned()
            .unwrap_or_else(Table::new);
        apply_rendered_server(&mut table, &rendered);
        servers_table.insert(&req.server, Item::Table(table));
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

fn rendered_server(
    plan: &McpPlan,
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
            let mut args = vec!["-y".to_string(), format!("{package}@{version}")];
            if req.server == "filesystem" {
                let workspace = plan.workspace.as_deref().ok_or_else(|| {
                    policy_blocked(
                        "MCP filesystem apply requires a reviewed workspace scope",
                        json!({
                            "plan_id": plan.plan_id,
                            "server": req.server,
                            "target_writes_performed": false,
                        }),
                    )
                })?;
                args.push(workspace.to_string());
            }
            ("npx".to_string(), args)
        }
        "local" => {
            let (command, _) = local_source_path_and_digest(source)?;
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
        env_vars: req.auth_env.iter().cloned().collect(),
    })
}

fn local_source_path_and_digest(
    source: &McpResolvedSource,
) -> std::result::Result<(&str, &str), CommandFailure> {
    let Some((path, digest)) = source
        .locator
        .strip_prefix("local:")
        .and_then(|raw| raw.split_once("@sha256:"))
    else {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "MCP local source is missing a digest-pinned path",
        ));
    };
    if path.is_empty()
        || !Path::new(path).is_absolute()
        || digest.is_empty()
        || digest.starts_with("sha256:")
    {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "MCP local source is missing an absolute digest-pinned path",
        ));
    }
    Ok((path, digest))
}

fn server_matches_rendered(
    servers_table: &dyn TableLike,
    server: &str,
    rendered: &RenderedServer,
) -> bool {
    let Some(table) = servers_table.get(server).and_then(Item::as_table_like) else {
        return false;
    };
    table.get("command").and_then(Item::as_str) == Some(rendered.command.as_str())
        && args_match(table.get("args"), &rendered.args)
        && env_vars_match(table.get("env_vars"), &rendered.env_vars)
}

fn args_match(item: Option<&Item>, expected: &[String]) -> bool {
    let Some(item) = item else {
        return expected.is_empty();
    };
    let Some(args) = item.as_array() else {
        return false;
    };
    args.len() == expected.len()
        && args
            .iter()
            .zip(expected)
            .all(|(arg, expected)| arg.as_str() == Some(expected.as_str()))
}

fn env_vars_match(item: Option<&Item>, expected: &[String]) -> bool {
    let Some(item) = item else {
        return expected.is_empty();
    };
    let Some(env_vars) = item.as_array() else {
        return false;
    };
    if env_vars.len() != expected.len() {
        return false;
    }
    env_vars
        .iter()
        .zip(expected)
        .all(|(name, expected)| name.as_str() == Some(expected.as_str()))
}

fn apply_rendered_server(table: &mut Table, rendered: &RenderedServer) {
    table["command"] = value(rendered.command.clone());
    if !rendered.args.is_empty() {
        let mut args = Array::default();
        for arg in &rendered.args {
            args.push(arg.as_str());
        }
        table["args"] = Item::Value(TomlValue::Array(args));
    } else {
        table.remove("args");
    }
    if !rendered.env_vars.is_empty() {
        let mut env_vars = Array::default();
        for env_name in &rendered.env_vars {
            env_vars.push(env_name.as_str());
        }
        table["env_vars"] = Item::Value(TomlValue::Array(env_vars));
        remove_legacy_managed_env(table, &rendered.env_vars);
    } else {
        table.remove("env_vars");
    }
}

fn remove_legacy_managed_env(table: &mut Table, env_vars: &[String]) {
    let Some(env_table) = table.get_mut("env").and_then(Item::as_table_like_mut) else {
        return;
    };
    for env_name in env_vars {
        env_table.remove(env_name);
    }
    if env_table.is_empty() {
        table.remove("env");
    }
}

fn policy_blocked(message: &str, details: Value) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    failure.details = details;
    failure
}
