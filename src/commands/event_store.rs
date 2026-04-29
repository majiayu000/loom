use std::fs::{self, OpenOptions};
use std::io::Write;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::cli::Cli;
use crate::envelope::Envelope;
use crate::state::AppContext;

#[derive(Debug, Serialize)]
struct CommandEvent {
    event_id: String,
    request_id: String,
    cmd: String,
    status: String,
    exit_code: i32,
    input: serde_json::Value,
    output: serde_json::Value,
    error: serde_json::Value,
    side_effects: serde_json::Value,
    created_at: DateTime<Utc>,
}

pub(crate) fn command_event_input(cli: &Cli) -> serde_json::Value {
    let mut input = serde_json::to_value(cli).unwrap_or_else(|err| {
        json!({
            "serialization_error": err.to_string(),
            "command": format!("{:?}", cli.command),
            "json": cli.json,
            "root": cli.root.as_ref().map(|root| root.display().to_string()),
        })
    });
    redact_sensitive_strings(&mut input);
    input
}

pub(crate) fn append_command_event(
    ctx: &AppContext,
    cmd: &str,
    input: serde_json::Value,
    envelope: &Envelope,
    exit_code: i32,
) -> Result<()> {
    let event = CommandEvent {
        event_id: format!("evt_{}", Uuid::new_v4().simple()),
        request_id: envelope.request_id.clone(),
        cmd: cmd.to_string(),
        status: if envelope.ok {
            "succeeded".to_string()
        } else {
            "failed".to_string()
        },
        exit_code,
        input,
        output: envelope.data.clone(),
        error: envelope
            .error
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?
            .unwrap_or(serde_json::Value::Null),
        side_effects: serde_json::to_value(&envelope.meta)?,
        created_at: Utc::now(),
    };

    let path = ctx.state_dir.join("events/commands.jsonl");
    let parent = path
        .parent()
        .context("command event path must have a parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create command event dir {}", parent.display()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open command event log {}", path.display()))?;
    let raw = serde_json::to_string(&event).context("failed to encode command event")?;
    writeln!(file, "{raw}")
        .with_context(|| format!("failed to append command event {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync command event {}", path.display()))?;
    Ok(())
}

fn redact_sensitive_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(raw) => {
            *raw = redact_url_userinfo(raw);
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_sensitive_strings(item);
            }
        }
        serde_json::Value::Object(fields) => {
            for value in fields.values_mut() {
                redact_sensitive_strings(value);
            }
        }
        _ => {}
    }
}

fn redact_url_userinfo(raw: &str) -> String {
    let Some(scheme_end) = raw.find("://") else {
        return raw.to_string();
    };
    let authority_start = scheme_end + 3;
    let rest = &raw[authority_start..];
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let Some(at) = authority.rfind('@') else {
        return raw.to_string();
    };

    format!(
        "{}://<redacted>@{}{}",
        &raw[..scheme_end],
        &authority[at + 1..],
        &rest[authority_end..]
    )
}

#[cfg(test)]
mod tests {
    use super::redact_url_userinfo;

    #[test]
    fn redacts_url_userinfo_without_changing_plain_urls() {
        assert_eq!(
            redact_url_userinfo("https://token@example.com/org/repo.git"),
            "https://<redacted>@example.com/org/repo.git"
        );
        assert_eq!(
            redact_url_userinfo("https://example.com/org/repo.git"),
            "https://example.com/org/repo.git"
        );
    }
}
