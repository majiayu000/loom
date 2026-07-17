use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::next_action_trace::observe_next_actions;

use crate::commands::CommandFailure;
use crate::commands::helpers::map_io;
use crate::commands::mcp::utils::digest_str;
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::{McpPlan, policy_blocked, read_config_snapshot};

const APPLY_RECORD_SCHEMA: &str = "mcp-apply-record-v1";

pub(super) struct ApplyRecordLock {
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ApplyLockMetadata {
    pid: u32,
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

pub(super) fn replay_existing_apply(
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

pub(super) fn load_apply_record(path: &Path) -> std::result::Result<Value, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    serde_json::from_str(&raw)
        .map_err(|err| CommandFailure::new(ErrorCode::ArgInvalid, err.to_string()))
}

pub(super) fn claim_apply_lock(
    lock_path: &Path,
    busy_message: &str,
    busy_details: Value,
) -> std::result::Result<ApplyRecordLock, CommandFailure> {
    fs::create_dir_all(lock_path.parent().unwrap_or(Path::new("."))).map_err(map_io)?;
    for _ in 0..2 {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
        {
            Ok(mut file) => {
                let metadata = json!({
                    "pid": std::process::id(),
                    "created_at_unix": SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or_default(),
                });
                writeln!(file, "{metadata}").map_err(map_io)?;
                file.sync_all().map_err(map_io)?;
                return Ok(ApplyRecordLock {
                    path: lock_path.to_path_buf(),
                });
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                if reap_stale_apply_lock(lock_path)? {
                    continue;
                }
                return Err(policy_blocked(busy_message, busy_details));
            }
            Err(err) => return Err(map_io(err)),
        }
    }
    Err(policy_blocked(busy_message, busy_details))
}

fn reap_stale_apply_lock(lock_path: &Path) -> std::result::Result<bool, CommandFailure> {
    let raw = match fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(true),
        Err(err) => return Err(map_io(err)),
    };
    let Ok(metadata) = serde_json::from_str::<ApplyLockMetadata>(&raw) else {
        return Ok(false);
    };
    if process_alive(metadata.pid) == Some(false) {
        match fs::remove_file(lock_path) {
            Ok(()) => return Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(true),
            Err(err) => return Err(map_io(err)),
        }
    }
    Ok(false)
}

#[cfg(unix)]
fn process_alive(pid: u32) -> Option<bool> {
    if pid == 0 {
        return Some(false);
    }
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        Some(true)
    } else {
        std::io::Error::last_os_error()
            .raw_os_error()
            .map(|code| code != libc::ESRCH)
    }
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> Option<bool> {
    None
}

pub(super) fn config_lock_path(ctx: &AppContext, config_path: &Path) -> PathBuf {
    let digest = digest_str(&config_path.display().to_string())
        .strip_prefix("sha256:")
        .unwrap_or_default()
        .replace(['/', '\\', ':'], "_");
    ctx.state_dir
        .join("mcp")
        .join("locks")
        .join(format!("config_{digest}.lock"))
}

pub(super) fn write_apply_record(
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

pub(super) fn apply_record_path(ctx: &AppContext, key_digest: &str) -> PathBuf {
    let suffix = key_digest
        .strip_prefix("sha256:")
        .unwrap_or(key_digest)
        .replace(['/', '\\', ':'], "_");
    ctx.state_dir
        .join("mcp")
        .join("applies")
        .join(format!("{suffix}.json"))
}

pub(super) fn apply_response(
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
            "next_actions": observe_next_actions(
                "mcp.apply.response",
                if target_writes_performed {
                    vec!["restart codex or start a new Codex session".to_string()]
                } else {
                    Vec::new()
                },
            ),
            "secret_values_written": false,
        }),
        Meta::default(),
    )
}
