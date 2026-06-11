use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::cli::Cli;
use crate::envelope::Envelope;
use crate::fs_util::{append_jsonl_raw, ensure_append_log, maybe_fault_inject_any};
use crate::state::AppContext;

const COMMAND_EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
struct CommandEvent {
    schema_version: u32,
    event_id: String,
    request_id: String,
    cmd: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    side_effects: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

pub(crate) fn command_event_input(cli: &Cli, request_id: &str) -> serde_json::Value {
    let mut audit_cli = cli.clone();
    audit_cli.request_id = Some(request_id.to_string());
    let mut input = serde_json::to_value(audit_cli).unwrap_or_else(|err| {
        json!({
            "serialization_error": err.to_string(),
            "request_id": request_id,
            "command": format!("{:?}", cli.command),
            "json": cli.json,
            "root": cli.root.as_ref().map(|root| root.display().to_string()),
        })
    });
    redact_sensitive_strings(&mut input);
    input
}

pub(crate) fn append_command_started(
    ctx: &AppContext,
    cmd: &str,
    input: serde_json::Value,
    request_id: &str,
) -> Result<String> {
    let event_id = format!("evt_{}", Uuid::new_v4().simple());
    let event = CommandEvent {
        schema_version: COMMAND_EVENT_SCHEMA_VERSION,
        event_id: event_id.clone(),
        request_id: request_id.to_string(),
        cmd: cmd.to_string(),
        status: "started".to_string(),
        exit_code: None,
        input: Some(input),
        output: None,
        error: None,
        side_effects: None,
        created_at: Utc::now(),
    };
    append_command_event(ctx, &event, &["command_event_append_started"])?;
    Ok(event_id)
}

pub(crate) fn append_command_finished(
    ctx: &AppContext,
    cmd: &str,
    envelope: &Envelope,
    exit_code: i32,
) -> Result<()> {
    append_command_finished_with_fault_tags(
        ctx,
        cmd,
        envelope,
        exit_code,
        &["command_event_append_finished", "command_event_append"],
    )
}

pub(crate) fn append_command_audit_failure(
    ctx: &AppContext,
    cmd: &str,
    envelope: &Envelope,
    exit_code: i32,
) -> Result<()> {
    append_command_finished_with_fault_tags(ctx, cmd, envelope, exit_code, &[])
}

fn append_command_finished_with_fault_tags(
    ctx: &AppContext,
    cmd: &str,
    envelope: &Envelope,
    exit_code: i32,
    fault_tags: &[&str],
) -> Result<()> {
    let event = CommandEvent {
        schema_version: COMMAND_EVENT_SCHEMA_VERSION,
        event_id: format!("evt_{}", Uuid::new_v4().simple()),
        request_id: envelope.request_id.clone(),
        cmd: cmd.to_string(),
        status: if envelope.ok {
            "succeeded".to_string()
        } else {
            "failed".to_string()
        },
        exit_code: Some(exit_code),
        input: None,
        output: Some(redacted_value(envelope.data.clone())),
        error: envelope
            .error
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?
            .map(redacted_value),
        side_effects: Some(redacted_value(serde_json::to_value(&envelope.meta)?)),
        created_at: Utc::now(),
    };
    append_command_event(ctx, &event, fault_tags)
}

fn append_command_event(ctx: &AppContext, event: &CommandEvent, fault_tags: &[&str]) -> Result<()> {
    maybe_fault_inject_any(fault_tags)?;
    let raw = serde_json::to_string(event).context("failed to encode command event")?;
    append_jsonl_raw(&ctx.command_events_file, &raw).with_context(|| {
        format!(
            "failed to append command event {}",
            ctx.command_events_file.display()
        )
    })
}

pub(crate) fn prepare_command_event_store(ctx: &AppContext) -> Result<()> {
    maybe_fault_inject_any(&["command_event_prepare"])?;
    ensure_append_log(&ctx.command_events_file).with_context(|| {
        format!(
            "failed to prepare command event log {}",
            ctx.command_events_file.display()
        )
    })
}

fn redact_sensitive_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(raw) => {
            *raw = redact_sensitive_string(raw);
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_sensitive_strings(item);
            }
        }
        serde_json::Value::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                if key_is_sensitive(key) {
                    *value = serde_json::Value::String("<redacted>".to_string());
                } else {
                    redact_sensitive_strings(value);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn redact_sensitive_string(raw: &str) -> String {
    if looks_like_secret(raw) {
        return "<redacted>".to_string();
    }
    let redacted = redact_url_sensitive_parts(&redact_url_userinfo(raw));
    redact_embedded_secrets(&redacted)
}

fn redacted_value(mut value: serde_json::Value) -> serde_json::Value {
    redact_sensitive_strings(&mut value);
    value
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

fn redact_url_sensitive_parts(raw: &str) -> String {
    if raw.find("://").is_none() {
        return raw.to_string();
    }

    let (without_fragment, fragment) = match raw.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (raw, None),
    };
    let (base, query) = match without_fragment.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (without_fragment, None),
    };

    let mut redacted = base.to_string();
    if let Some(query) = query {
        redacted.push('?');
        redacted.push_str(&redact_query(query));
    }
    if let Some(fragment) = fragment {
        redacted.push('#');
        if fragment.is_empty() {
            redacted.push_str(fragment);
        } else {
            redacted.push_str("<redacted>");
        }
    }
    redacted
}

fn redact_query(query: &str) -> String {
    query
        .split('&')
        .map(|part| {
            let Some((key, value)) = part.split_once('=') else {
                return if looks_like_secret(part) {
                    "<redacted>".to_string()
                } else {
                    part.to_string()
                };
            };
            if key_is_sensitive(key) || looks_like_secret(value) {
                format!("{key}=<redacted>")
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn redact_embedded_secrets(raw: &str) -> String {
    let mut redacted = String::with_capacity(raw.len());
    let mut cursor = 0;

    while let Some((start, end)) = find_secret_span(raw, cursor) {
        redacted.push_str(&raw[cursor..start]);
        redacted.push_str("<redacted>");
        cursor = end;
    }

    redacted.push_str(&raw[cursor..]);
    redacted
}

fn find_secret_span(raw: &str, from: usize) -> Option<(usize, usize)> {
    for (offset, _) in raw[from..].char_indices() {
        let start = from + offset;
        if let Some(end) = secret_span_at(raw, start) {
            return Some((start, end));
        }
    }
    None
}

fn secret_span_at(raw: &str, start: usize) -> Option<usize> {
    if !is_secret_boundary_before(raw, start) {
        return None;
    }

    if raw[start..].starts_with("Bearer ") {
        let token_start = start + "Bearer ".len();
        let token_end = secret_token_end(raw, token_start);
        return (token_end > token_start).then_some(token_end);
    }

    for prefix in [
        "github_pat_",
        "ghp_",
        "glpat-",
        "sk-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "ya29.",
    ] {
        if raw[start..].starts_with(prefix) {
            let token_end = secret_token_end(raw, start + prefix.len());
            return (token_end > start + prefix.len()).then_some(token_end);
        }
    }

    if raw[start..].starts_with("AKIA") {
        let token_end = secret_token_end(raw, start);
        if token_end - start >= 20 {
            return Some(token_end);
        }
    }

    None
}

fn secret_token_end(raw: &str, token_start: usize) -> usize {
    let mut end = token_start;
    for (offset, ch) in raw[token_start..].char_indices() {
        if !is_secret_token_char(ch) {
            break;
        }
        end = token_start + offset + ch.len_utf8();
    }
    end
}

fn is_secret_boundary_before(raw: &str, start: usize) -> bool {
    raw[..start]
        .chars()
        .next_back()
        .is_none_or(|ch| !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '-' | '.'))
}

fn is_secret_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '+' | '=')
}

fn key_is_sensitive(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "authorization",
        "apikey",
        "accesskey",
        "privatekey",
        "signature",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn looks_like_secret(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.starts_with("Bearer ")
        || trimmed.starts_with("ghp_")
        || trimmed.starts_with("github_pat_")
        || trimmed.starts_with("glpat-")
        || trimmed.starts_with("sk-")
        || trimmed.starts_with("xoxb-")
        || trimmed.starts_with("xoxp-")
        || trimmed.starts_with("xoxa-")
        || trimmed.starts_with("ya29.")
        || (trimmed.starts_with("AKIA") && trimmed.len() >= 20)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::Arc;

    use chrono::Utc;
    use serde_json::json;

    use super::{
        COMMAND_EVENT_SCHEMA_VERSION, CommandEvent, append_command_event, redact_sensitive_string,
        redact_sensitive_strings, redact_url_userinfo,
    };
    use crate::state::AppContext;

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

    #[test]
    fn redacts_url_query_fragments_and_token_like_values() {
        assert_eq!(
            redact_sensitive_string(
                "https://user:pass@example.com/org/repo.git?access_token=ghp_secret&ref=main#ghp_fragment"
            ),
            "https://<redacted>@example.com/org/repo.git?access_token=<redacted>&ref=main#<redacted>"
        );
        assert_eq!(
            redact_sensitive_string("github_pat_abcdefghijklmnopqrstuvwxyz1234567890"),
            "<redacted>"
        );
    }

    #[test]
    fn redacts_embedded_token_like_values() {
        assert_eq!(
            redact_sensitive_string("prefix sk-reviewtoken and ghp_reviewtoken suffix"),
            "prefix <redacted> and <redacted> suffix"
        );
        assert_eq!(
            redact_sensitive_string("Authorization: Bearer reviewtoken"),
            "Authorization: <redacted>"
        );
        assert_eq!(
            redact_sensitive_string("mask-sk-not-a-token"),
            "mask-sk-not-a-token"
        );
    }

    #[test]
    fn redacts_sensitive_object_fields() {
        let mut value = json!({
            "source": "https://example.com/repo.git?token=secret&ref=main",
            "api_key": "sk-secret",
            "nested": {
                "password": "p@ssw0rd",
                "plain": "visible"
            }
        });

        redact_sensitive_strings(&mut value);

        assert_eq!(
            value["source"],
            json!("https://example.com/repo.git?token=<redacted>&ref=main")
        );
        assert_eq!(value["api_key"], json!("<redacted>"));
        assert_eq!(value["nested"]["password"], json!("<redacted>"));
        assert_eq!(value["nested"]["plain"], json!("visible"));
    }

    #[test]
    fn concurrent_command_event_appends_preserve_jsonl_rows() {
        let dir = std::env::temp_dir().join(format!(
            "loom-command-events-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).expect("create temp root");
        let ctx = Arc::new(AppContext::new(Some(dir.clone())).expect("create context"));
        let workers = 8usize;
        let per_worker = 8usize;
        let payload = "x".repeat(16 * 1024);

        let handles = (0..workers)
            .map(|worker| {
                let ctx = Arc::clone(&ctx);
                let payload = payload.clone();
                std::thread::spawn(move || {
                    for index in 0..per_worker {
                        let event = CommandEvent {
                            schema_version: COMMAND_EVENT_SCHEMA_VERSION,
                            event_id: format!("evt_{worker}_{index}"),
                            request_id: format!("req_{worker}_{index}"),
                            cmd: "test.concurrent".to_string(),
                            status: "started".to_string(),
                            exit_code: None,
                            input: Some(json!({
                                "worker": worker,
                                "index": index,
                                "payload": payload,
                            })),
                            output: None,
                            error: None,
                            side_effects: None,
                            created_at: Utc::now(),
                        };
                        append_command_event(&ctx, &event, &[]).expect("append command event");
                    }
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().expect("worker must not panic");
        }

        let raw = fs::read_to_string(dir.join("state/events/commands.jsonl"))
            .expect("read command events");
        let lines = raw.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), workers * per_worker);

        let mut ids = BTreeSet::new();
        for line in lines {
            let event: serde_json::Value =
                serde_json::from_str(line).expect("each line must be a JSON event");
            let event_id = event["event_id"].as_str().expect("event_id must be string");
            assert!(ids.insert(event_id.to_string()), "duplicate {event_id}");
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
