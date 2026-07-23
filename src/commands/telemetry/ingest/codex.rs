use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{ImportedInvocation, ImportedRecord, ParseOutcome};

mod tool_read;

#[derive(Debug, Default)]
pub(super) struct Context {
    session_id: Option<String>,
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
            context.session_workspace = payload
                .get("cwd")
                .and_then(Value::as_str)
                .map(PathBuf::from);
            context.workspace = context.session_workspace.clone();
            ParseOutcome::Ignored
        }
        Some("turn_context") => {
            context.workspace = context.session_workspace.clone();
            context.turn_id = None;
            context.mentioned_skills.clear();
            context.read_skills.clear();
            context.next_skill_ordinal = 0;
            if let Some(payload) = value.get("payload") {
                update_workspace(context, payload);
                context.turn_id = payload
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
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
    let names = tool_read::skill_entrypoint_names(payload)
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
mod tests {
    use std::path::PathBuf;

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
    fn current_exec_command_skill_read_is_parsed_once_per_turn() {
        let mut context = Context::default();
        for value in [
            json!({"type":"session_meta","payload":{"id":"session","cwd":"/workspace"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
        ] {
            assert!(matches!(
                parse_record(&value, &mut context),
                ParseOutcome::Ignored
            ));
        }
        let read = json!({
            "timestamp":"2026-07-23T00:00:00Z",
            "type":"response_item",
            "payload":{
                "type":"function_call",
                "name":"exec_command",
                "call_id":"call-1",
                "arguments":serde_json::to_string(&json!({
                    "cmd":"sed -n '1,240p' /home/user/.loom-registry/skills/demo/SKILL.md"
                })).unwrap()
            }
        });
        let ParseOutcome::Record(record) = parse_record(&read, &mut context) else {
            panic!("current Codex skill entrypoint read must parse");
        };
        assert_eq!(record.stable_record_key, "call-1");
        assert_eq!(record.invocations[0].name, "demo");
        assert!(matches!(
            parse_record(&read, &mut context),
            ParseOutcome::Ignored
        ));

        assert!(matches!(
            parse_record(
                &json!({"type":"turn_context","payload":{"turn_id":"turn-2"}}),
                &mut context,
            ),
            ParseOutcome::Ignored
        ));
        assert!(matches!(
            parse_record(&read, &mut context),
            ParseOutcome::Record(_)
        ));
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
    fn repeated_mentions_are_consumed_once_per_injection() {
        let mut context = Context::default();
        for value in [
            json!({"type":"session_meta","payload":{"id":"session"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
            json!({
                "type":"response_item",
                "payload":{"type":"message","role":"user","content":[{
                    "type":"input_text","text":"use $demo and then $demo"
                }]}
            }),
        ] {
            assert!(matches!(
                parse_record(&value, &mut context),
                ParseOutcome::Ignored
            ));
        }
        let injection = json!({
            "timestamp":"2026-07-01T00:00:00Z",
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[{
                "type":"input_text","text":"<skill><name>demo</name>body</skill>"
            }]}
        });
        for expected_ordinal in [0, 1] {
            let ParseOutcome::Record(record) = parse_record(&injection, &mut context) else {
                panic!("each mention must admit one structured injection");
            };
            assert_eq!(record.invocations[0].ordinal, expected_ordinal);
        }
        assert!(matches!(
            parse_record(&injection, &mut context),
            ParseOutcome::Ignored
        ));
    }

    #[test]
    fn same_item_mentions_only_match_later_injections() {
        let mut context = Context::default();
        for value in [
            json!({"type":"session_meta","payload":{"id":"session"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
        ] {
            assert!(matches!(
                parse_record(&value, &mut context),
                ParseOutcome::Ignored
            ));
        }
        let marker_before = json!({
            "timestamp":"2026-07-01T00:00:00Z",
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[
                {"type":"input_text","text":"please use $demo"},
                {"type":"input_text","text":"<skill><name>demo</name>body</skill>"}
            ]}
        });
        assert!(matches!(
            parse_record(&marker_before, &mut context),
            ParseOutcome::Record(_)
        ));

        let marker_after = json!({
            "timestamp":"2026-07-01T00:00:01Z",
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[
                {"type":"input_text","text":"<skill><name>other</name>body</skill>"},
                {"type":"input_text","text":"please use $other"}
            ]}
        });
        assert!(matches!(
            parse_record(&marker_after, &mut context),
            ParseOutcome::Ignored
        ));
    }

    #[test]
    fn same_item_collects_all_matched_injections_in_order() {
        let mut context = Context::default();
        for value in [
            json!({"type":"session_meta","payload":{"id":"session"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
        ] {
            assert!(matches!(
                parse_record(&value, &mut context),
                ParseOutcome::Ignored
            ));
        }
        let value = json!({
            "timestamp":"2026-07-01T00:00:00Z",
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[
                {"type":"input_text","text":"please use $foo and $bar"},
                {"type":"input_text","text":"<skill><name>unmatched</name>body</skill>"},
                {"type":"input_text","text":"<skill><name>foo</name>body</skill>"},
                {"type":"input_text","text":"<skill><name>bar</name>body</skill>"}
            ]}
        });
        let ParseOutcome::Record(record) = parse_record(&value, &mut context) else {
            panic!("all matched injections in one item must parse");
        };
        assert_eq!(record.invocations.len(), 2);
        assert_eq!(record.invocations[0].name, "foo");
        assert_eq!(record.invocations[0].ordinal, 0);
        assert_eq!(record.invocations[1].name, "bar");
        assert_eq!(record.invocations[1].ordinal, 1);
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

    #[test]
    fn malformed_turn_context_clears_the_previous_turn() {
        let mut context = Context::default();
        for value in [
            json!({"type":"session_meta","payload":{"id":"session","cwd":"/session"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn-1","cwd":"/turn"}}),
            json!({
                "type":"response_item",
                "payload":{"type":"message","role":"user","content":[{
                    "type":"input_text","text":"please use $demo"
                }]}
            }),
        ] {
            assert!(matches!(
                parse_record(&value, &mut context),
                ParseOutcome::Ignored
            ));
        }
        let stale_injection = json!({
            "timestamp":"2026-07-01T00:00:00Z",
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[{
                "type":"input_text","text":"<skill><name>demo</name>body</skill>"
            }]}
        });
        let ParseOutcome::Record(overridden) = parse_record(&stale_injection, &mut context) else {
            panic!("turn workspace must override the session fallback");
        };
        assert_eq!(overridden.workspace, Some(PathBuf::from("/turn")));
        assert!(matches!(
            parse_record(
                &json!({
                    "type":"response_item",
                    "payload":{"type":"message","role":"user","content":[{
                        "type":"input_text","text":"please use $demo"
                    }]}
                }),
                &mut context,
            ),
            ParseOutcome::Ignored
        ));
        assert!(matches!(
            parse_record(&json!({"type":"turn_context"}), &mut context),
            ParseOutcome::Ignored
        ));
        assert!(matches!(
            parse_record(&stale_injection, &mut context),
            ParseOutcome::Ignored
        ));

        assert!(matches!(
            parse_record(
                &json!({"type":"turn_context","payload":{"turn_id":"turn-2"}}),
                &mut context,
            ),
            ParseOutcome::Ignored
        ));
        assert!(matches!(
            parse_record(
                &json!({
                    "type":"response_item",
                    "payload":{"type":"message","role":"user","content":[{
                        "type":"input_text","text":"please use $demo"
                    }]}
                }),
                &mut context,
            ),
            ParseOutcome::Ignored
        ));
        let ParseOutcome::Record(record) = parse_record(&stale_injection, &mut context) else {
            panic!("new turn must accept a fresh mention");
        };
        assert_eq!(record.stable_record_key, "turn-2");
        assert_eq!(record.workspace, Some(PathBuf::from("/session")));
        assert_eq!(record.invocations[0].ordinal, 0);
    }

    #[test]
    fn rejected_injection_preserves_mention_for_a_valid_retry() {
        let mut context = Context::default();
        for value in [
            json!({"type":"session_meta","payload":{"id":"session"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
            json!({
                "type":"response_item",
                "payload":{"type":"message","role":"user","content":[{
                    "type":"input_text","text":"please use $demo"
                }]}
            }),
        ] {
            assert!(matches!(
                parse_record(&value, &mut context),
                ParseOutcome::Ignored
            ));
        }
        let missing_timestamp = json!({
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[{
                "type":"input_text","text":"<skill><name>demo</name>body</skill>"
            }]}
        });
        assert!(matches!(
            parse_record(&missing_timestamp, &mut context),
            ParseOutcome::Rejected("invalid_timestamp")
        ));
        let mut valid = missing_timestamp;
        valid["timestamp"] = json!("2026-07-01T00:00:00Z");
        assert!(matches!(
            parse_record(&valid, &mut context),
            ParseOutcome::Record(_)
        ));
    }

    #[test]
    fn empty_session_identity_is_rejected() {
        let mut context = Context::default();
        assert!(matches!(
            parse_record(
                &json!({"type":"session_meta","payload":{"id":""}}),
                &mut context,
            ),
            ParseOutcome::Rejected("missing_session_identity")
        ));
    }

    #[test]
    fn malformed_session_boundary_clears_previous_turn_state() {
        let mut context = Context::default();
        for value in [
            json!({"type":"session_meta","payload":{"id":"session"}}),
            json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
            json!({
                "type":"response_item",
                "payload":{"type":"message","role":"user","content":[{
                    "type":"input_text","text":"please use $demo"
                }]}
            }),
        ] {
            assert!(matches!(
                parse_record(&value, &mut context),
                ParseOutcome::Ignored
            ));
        }
        assert!(matches!(
            parse_record(&json!({"type":"session_meta"}), &mut context),
            ParseOutcome::Rejected("missing_session_metadata")
        ));
        let stale_injection = json!({
            "timestamp":"2026-07-01T00:00:00Z",
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[{
                "type":"input_text","text":"<skill><name>demo</name>body</skill>"
            }]}
        });
        assert!(matches!(
            parse_record(&stale_injection, &mut context),
            ParseOutcome::Ignored
        ));
    }
}
