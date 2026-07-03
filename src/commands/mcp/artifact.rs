use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use walkdir::WalkDir;

use crate::commands::CommandFailure;
use crate::commands::helpers::map_io;
use crate::fs_util::write_atomic;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::McpPlan;

pub(super) const MCP_PLAN_SCHEMA: &str = "mcp-plan-v1";

pub(super) fn write_durable_mcp_plan(
    ctx: &AppContext,
    plan: &McpPlan,
) -> std::result::Result<PathBuf, CommandFailure> {
    let path = durable_plan_path(ctx, &plan.plan_id)?;
    let mut raw = serde_json::to_string_pretty(plan).map_err(map_io)?;
    raw.push('\n');
    write_atomic(&path, &raw).map_err(map_io)?;
    Ok(path)
}

pub(super) fn load_reviewed_mcp_plan(
    ctx: &AppContext,
    plan: &str,
) -> std::result::Result<McpPlan, CommandFailure> {
    let path = Path::new(plan);
    if path.is_file() {
        return load_plan_file(path);
    }

    let durable_path = durable_plan_path(ctx, plan)?;
    if !durable_path.is_file() {
        return Err(reviewed_plan_not_found(plan));
    }
    let loaded = load_plan_file(&durable_path)?;
    if loaded.plan_id != plan {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!(
                "durable MCP plan '{}' contains mismatched plan_id '{}'",
                durable_path.display(),
                loaded.plan_id
            ),
        ));
    }
    Ok(loaded)
}

pub(super) fn durable_plan_path(
    ctx: &AppContext,
    plan_id: &str,
) -> std::result::Result<PathBuf, CommandFailure> {
    validate_plan_id(plan_id)?;
    Ok(ctx
        .state_dir
        .join("mcp")
        .join("plans")
        .join(format!("{plan_id}.json")))
}

pub(super) fn validate_plan_id(plan_id: &str) -> std::result::Result<(), CommandFailure> {
    if plan_id.is_empty()
        || plan_id.len() > 128
        || !plan_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "MCP plan id must match [A-Za-z0-9_-]{1,128}",
        ));
    }
    Ok(())
}

pub(super) fn skill_source_digest(path: &Path) -> std::result::Result<String, CommandFailure> {
    if !path.is_dir() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill source '{}' not found", path.display()),
        ));
    }

    let mut entries = Vec::new();
    for entry in WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(map_io)?;
        if entry.path() == path || entry.file_type().is_dir() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(path)
            .map_err(map_io)?
            .to_string_lossy()
            .replace('\\', "/");
        entries.push((
            rel,
            entry.path().to_path_buf(),
            entry.file_type().is_symlink(),
        ));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    hasher.update(b"loom.mcp.skill-source.v1\n");
    for (rel, full, is_symlink) in entries {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        if is_symlink {
            hasher.update(b"symlink\0");
            hasher.update(
                fs::read_link(&full)
                    .map_err(map_io)?
                    .to_string_lossy()
                    .as_bytes(),
            );
        } else {
            hasher.update(b"file\0");
            hasher.update(&fs::read(&full).map_err(map_io)?);
        }
        hasher.update(b"\0");
    }
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

fn load_plan_file(path: &Path) -> std::result::Result<McpPlan, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| CommandFailure::new(ErrorCode::ArgInvalid, err.to_string()))?;
    let plan_value = if value.get("schema_version").and_then(Value::as_str) == Some(MCP_PLAN_SCHEMA)
    {
        value
    } else {
        value
            .get("data")
            .filter(|data| {
                data.get("schema_version").and_then(Value::as_str) == Some(MCP_PLAN_SCHEMA)
            })
            .cloned()
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::SchemaMismatch,
                    format!("unsupported MCP plan artifact '{}'", path.display()),
                )
            })?
    };
    let plan: McpPlan = serde_json::from_value(plan_value)
        .map_err(|err| CommandFailure::new(ErrorCode::ArgInvalid, err.to_string()))?;
    if plan.schema_version != MCP_PLAN_SCHEMA {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "unsupported MCP plan schema_version {}",
                plan.schema_version
            ),
        ));
    }
    Ok(plan)
}

fn reviewed_plan_not_found(plan: &str) -> CommandFailure {
    let mut failure = CommandFailure::new(
        ErrorCode::PolicyBlocked,
        "MCP reviewed plan was not found; pass a plan artifact path or durable plan id produced by mcp plan",
    );
    failure.details = json!({
        "plan": plan,
        "target_writes_performed": false,
    });
    failure
}
