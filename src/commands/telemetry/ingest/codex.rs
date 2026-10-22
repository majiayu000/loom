use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::super::model::SessionOrigin;
use super::{ImportedInvocation, ImportedRecord, ParseOutcome, origin};

mod tool_read;

#[derive(Debug, Default)]
pub(super) struct Context {
    session_id: Option<String>,
    origin: Option<SessionOrigin>,
    session_workspace: Option<PathBuf>,
    workspace: Option<PathBuf>,
    turn_id: Option<String>,
    mentioned_skills: BTreeMap<String, usize>,
    read_skills: BTreeSet<String>,
    next_skill_ordinal: usize,
}

pub(super) fn parse_record(value: &Value, context: &mut Context) -> ParseOutcome {
    match value.get("type").and_then(Value::as_str) {
        Some("session_meta") => {
            context.session_id = None;
            context.origin = None;
            context.session_workspace = None;
            context.workspace = None;
            context.turn_id = None;
            context.mentioned_skills.clear();
            context.read_skills.clear();
            context.next_skill_ordinal = 0;
            let Some(payload) = value.get("payload") else {
                return ParseOutcome::Rejected("missing_session_metadata");
            };
            let Some(session_id) = payload
                .get("session_id")
                .or_else(|| payload.get("id"))
                .and_then(Value::as_str)
                .filter(|session_id| !session_id.is_empty())
            else {
                return ParseOutcome::Rejected("missing_session_identity");
            };
            context.session_id = Some(session_id.to_string());
            context.origin = payload
                .get("originator")
                .and_then(Value::as_str)
                .and_then(origin::codex_originator);
            context.session_workspace = payload
                .get("cwd")
                .and_then(Value::as_str)
                .map(PathBuf::from);
            context.workspace = context.session_workspace.clone();
            ParseOutcome::Ignored
        }
        Some("turn_context") => {
            context.workspace = context.session_workspace.clone();
            let payload = value.get("payload");
            let incoming_turn_id = payload
                .and_then(|payload| payload.get("turn_id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let same_stable_turn = incoming_turn_id
                .as_deref()
                .is_some_and(|turn_id| context.turn_id.as_deref() == Some(turn_id));
            if !same_stable_turn {
                context.turn_id = incoming_turn_id;
                context.mentioned_skills.clear();
                context.read_skills.clear();
                context.next_skill_ordinal = 0;
            }
            if let Some(payload) = payload {
                update_workspace(context, payload);
            }
            ParseOutcome::Ignored
        }
        Some("response_item") => parse_response_item(value, context),
        _ => ParseOutcome::Ignored,
    }
}

fn parse_response_item(value: &Value, context: &mut Context) -> ParseOutcome {
    let Some(payload) = value.get("payload") else {
        return ParseOutcome::Ignored;
    };
    match payload.get("type").and_then(Value::as_str) {
        Some("message") if payload.get("role").and_then(Value::as_str) == Some("user") => {
            parse_skill_injection(value, payload, context)
        }
        Some("function_call") => parse_skill_entrypoint_read(value, payload, context),
        _ => ParseOutcome::Ignored,
    }
}

fn parse_skill_injection(value: &Value, payload: &Value, context: &mut Context) -> ParseOutcome {
    let texts = payload
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .filter_map(|item| {
                    (item.get("type").and_then(Value::as_str) == Some("input_text"))
                        .then(|| item.get("text").and_then(Value::as_str))
                        .flatten()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut mentioned_skills = context.mentioned_skills.clone();
    let mut names = Vec::new();
    for text in texts {
        if let Some(structured_name) = structured_skill_name(text) {
            if take_mentioned_skill(&mut mentioned_skills, structured_name) {
                names.push(structured_name);
            }
            continue;
        }
        collect_skill_mentions(text, &mut mentioned_skills);
    }
    if names.is_empty() {
        context.mentioned_skills = mentioned_skills;
        return ParseOutcome::Ignored;
    }
    let Some(stable_record_key) = context.turn_id.as_deref() else {
        return ParseOutcome::Rejected("missing_stable_record_key");
    };
    let Some(session_id) = context.session_id.as_deref() else {
        return ParseOutcome::Rejected("missing_session_identity");
    };
    let Some(timestamp) = parse_timestamp(value) else {
        return ParseOutcome::Rejected("invalid_timestamp");
    };
    let first_ordinal = context.next_skill_ordinal;
    let Some(next_skill_ordinal) = context.next_skill_ordinal.checked_add(names.len()) else {
        return ParseOutcome::Rejected("invocation_ordinal_overflow");
    };
    context.mentioned_skills = mentioned_skills;
    context.next_skill_ordinal = next_skill_ordinal;
    ParseOutcome::Record(ImportedRecord {
        stable_record_key: stable_record_key.to_string(),
        session_id: session_id.to_string(),
        workspace: context.workspace.clone(),
        timestamp,
        turn_id: context.turn_id.clone(),
        origin: context.origin,
        rejected_reasons: Vec::new(),
        invocations: names
            .into_iter()
            .enumerate()
            .map(|(offset, name)| {
                let ordinal = first_ordinal + offset;
                ImportedInvocation {
                    name: name.to_string(),
                    identity: format!("skill-injection-{ordinal}"),
                    ordinal,
                }
            })
            .collect(),
    })
}

fn parse_skill_entrypoint_read(
    value: &Value,
    payload: &Value,
    context: &mut Context,
) -> ParseOutcome {
    let names = match tool_read::skill_entrypoint_names(payload) {
        Ok(names) => names,
        Err(reason) => return ParseOutcome::Rejected(reason),
    }
    .into_iter()
    .filter(|name| !context.read_skills.contains(name))
    .collect::<Vec<_>>();
    if names.is_empty() {
        return ParseOutcome::Ignored;
    }
    let Some(stable_record_key) = payload
        .get("call_id")
        .or_else(|| payload.get("id"))
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
    else {
        return ParseOutcome::Rejected("missing_stable_record_key");
    };
    let Some(session_id) = context.session_id.as_deref() else {
        return ParseOutcome::Rejected("missing_session_identity");
    };
    let Some(timestamp) = parse_timestamp(value) else {
        return ParseOutcome::Rejected("invalid_timestamp");
    };
    let first_ordinal = context.next_skill_ordinal;
    let Some(next_skill_ordinal) = context.next_skill_ordinal.checked_add(names.len()) else {
        return ParseOutcome::Rejected("invocation_ordinal_overflow");
    };
    context.read_skills.extend(names.iter().cloned());
    context.next_skill_ordinal = next_skill_ordinal;
    ParseOutcome::Record(ImportedRecord {
        stable_record_key: stable_record_key.to_string(),
        session_id: session_id.to_string(),
        workspace: context.workspace.clone(),
        timestamp,
        turn_id: context.turn_id.clone(),
        origin: context.origin,
        rejected_reasons: Vec::new(),
        invocations: names
            .into_iter()
            .enumerate()
            .map(|(offset, name)| {
                let ordinal = first_ordinal + offset;
                ImportedInvocation {
                    name,
                    identity: format!("skill-entrypoint-read-{ordinal}"),
                    ordinal,
                }
            })
            .collect(),
    })
}

fn take_mentioned_skill(names: &mut BTreeMap<String, usize>, name: &str) -> bool {
    take_exact_mention(names, name) || take_exact_mention(names, &format!("{name}."))
}

fn take_exact_mention(names: &mut BTreeMap<String, usize>, name: &str) -> bool {
    let remove = match names.get_mut(name) {
        Some(count) => {
            *count -= 1;
            *count == 0
        }
        None => return false,
    };
    if remove {
        names.remove(name);
    }
    true
}

fn update_workspace(context: &mut Context, payload: &Value) {
    if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
        context.workspace = Some(PathBuf::from(cwd));
    }
}

fn structured_skill_name(text: &str) -> Option<&str> {
    let text = text.trim();
    let body = text.strip_prefix("<skill>")?.strip_suffix("</skill>")?;
    let tail = body.get(body.find("<name>")? + "<name>".len()..)?;
    let name = tail.get(..tail.find("</name>")?)?;
    (!name.is_empty() && !name.contains('<')).then_some(name)
}

fn collect_skill_mentions(text: &str, names: &mut BTreeMap<String, usize>) {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'$' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'-' | b'_' | b'.'))
        {
            end += 1;
        }
        if end > start
            && let Some(name) = text.get(start..end)
        {
            let count = names.entry(name.to_string()).or_default();
            *count = count.saturating_add(1);
        }
        index = end.max(start);
    }
}

fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests;
