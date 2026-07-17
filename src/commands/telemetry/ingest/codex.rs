use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{ImportedInvocation, ImportedRecord, ParseOutcome};

#[derive(Debug, Default)]
pub(super) struct Context {
    session_id: Option<String>,
    workspace: Option<PathBuf>,
    turn_id: Option<String>,
    mentioned_skills: BTreeSet<String>,
    next_skill_ordinal: usize,
}

pub(super) fn parse_record(value: &Value, context: &mut Context) -> ParseOutcome {
    match value.get("type").and_then(Value::as_str) {
        Some("session_meta") => {
            let Some(payload) = value.get("payload") else {
                return ParseOutcome::Rejected("missing_session_metadata");
            };
            let Some(session_id) = payload
                .get("session_id")
                .or_else(|| payload.get("id"))
                .and_then(Value::as_str)
            else {
                return ParseOutcome::Rejected("missing_session_identity");
            };
            context.session_id = Some(session_id.to_string());
            update_workspace(context, payload);
            ParseOutcome::Ignored
        }
        Some("turn_context") => {
            if let Some(payload) = value.get("payload") {
                update_workspace(context, payload);
                context.turn_id = payload
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                context.mentioned_skills.clear();
                context.next_skill_ordinal = 0;
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
    if payload.get("type").and_then(Value::as_str) != Some("message")
        || payload.get("role").and_then(Value::as_str) != Some("user")
    {
        return ParseOutcome::Ignored;
    }
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
    let Some(name) = texts.iter().find_map(|text| structured_skill_name(text)) else {
        for text in texts {
            collect_skill_mentions(text, &mut context.mentioned_skills);
        }
        return ParseOutcome::Ignored;
    };
    if !take_mentioned_skill(&mut context.mentioned_skills, name) {
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
    let ordinal = context.next_skill_ordinal;
    let Some(next_skill_ordinal) = context.next_skill_ordinal.checked_add(1) else {
        return ParseOutcome::Rejected("invocation_ordinal_overflow");
    };
    context.next_skill_ordinal = next_skill_ordinal;
    ParseOutcome::Record(ImportedRecord {
        stable_record_key: stable_record_key.to_string(),
        session_id: session_id.to_string(),
        workspace: context.workspace.clone(),
        timestamp,
        invocations: vec![ImportedInvocation {
            name: name.to_string(),
            identity: format!("skill-injection-{ordinal}"),
            ordinal,
        }],
    })
}

fn take_mentioned_skill(names: &mut BTreeSet<String>, name: &str) -> bool {
    names.remove(name) || names.remove(&format!("{name}."))
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

fn collect_skill_mentions(text: &str, names: &mut BTreeSet<String>) {
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
            names.insert(name.to_string());
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
mod tests {
    use serde_json::json;

    use super::{Context, ParseOutcome, parse_record};

    #[test]
    fn rollout_context_and_structured_skill_are_parsed() {
        let mut context = Context::default();
        assert!(matches!(
            parse_record(
                &json!({"type":"session_meta","payload":{"id":"session","cwd":"/workspace"}}),
                &mut context,
            ),
            ParseOutcome::Ignored
        ));
        assert!(matches!(
            parse_record(
                &json!({"type":"turn_context","payload":{"turn_id":"turn-1","cwd":"/workspace"}}),
                &mut context,
            ),
            ParseOutcome::Ignored
        ));
        let mention = json!({
            "timestamp": "2026-07-01T00:00:00Z",
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type":"input_text","text":"please use $demo."}]
            }
        });
        assert!(matches!(
            parse_record(&mention, &mut context),
            ParseOutcome::Ignored
        ));
        let value = json!({
            "timestamp": "2026-07-01T00:00:00Z",
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type":"input_text","text":"<skill>\n<name>demo</name>\nbody\n</skill>"}]
            }
        });
        let ParseOutcome::Record(record) = parse_record(&value, &mut context) else {
            panic!("structured rollout skill must parse");
        };
        assert_eq!(record.session_id, "session");
        assert_eq!(record.invocations[0].name, "demo");
    }

    #[test]
    fn dotted_skill_mentions_match_structured_injections() {
        let mut context = Context::default();
        for value in [
            json!({"type":"session_meta","payload":{"id":"session"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
            json!({
                "timestamp":"2026-07-01T00:00:00Z",
                "type":"response_item",
                "payload":{"type":"message","role":"user","content":[{
                    "type":"input_text","text":"please use $team.skill."
                }]}
            }),
        ] {
            assert!(matches!(
                parse_record(&value, &mut context),
                ParseOutcome::Ignored
            ));
        }
        let injection = json!({
            "timestamp":"2026-07-01T00:00:01Z",
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[{
                "type":"input_text","text":"<skill><name>team.skill</name>body</skill>"
            }]}
        });
        let ParseOutcome::Record(record) = parse_record(&injection, &mut context) else {
            panic!("dotted skill injection must parse");
        };
        assert_eq!(record.invocations[0].name, "team.skill");
    }

    #[test]
    fn free_text_is_not_an_invocation() {
        let mut context = Context::default();
        let value = json!({
            "timestamp": "2026-07-01T00:00:00Z",
            "type": "response_item",
            "payload": {
                "type": "message",
                "id": "msg-1",
                "role": "user",
                "content": [{"type":"input_text","text":"please use $demo"}]
            }
        });
        assert!(matches!(
            parse_record(&value, &mut context),
            ParseOutcome::Ignored
        ));
    }

    #[test]
    fn isolated_skill_xml_is_not_an_invocation() {
        let mut context = Context::default();
        let _ = parse_record(
            &json!({"type":"session_meta","payload":{"id":"session"}}),
            &mut context,
        );
        let _ = parse_record(
            &json!({"type":"turn_context","payload":{"turn_id":"turn-1","cwd":"/workspace"}}),
            &mut context,
        );
        let value = json!({
            "timestamp": "2026-07-01T00:00:00Z",
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type":"input_text","text":"<skill>\n<name>demo</name>\nbody\n</skill>"}]
            }
        });
        assert!(matches!(
            parse_record(&value, &mut context),
            ParseOutcome::Ignored
        ));
    }
}
