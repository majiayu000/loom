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
                "cmd":"sed -n '1,240p' $HOME/.loom-registry/skills/demo/SKILL.md"
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
            &json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
            &mut context,
        ),
        ParseOutcome::Ignored
    ));
    let mut repeated_read = read.clone();
    repeated_read["payload"]["call_id"] = json!("call-2");
    assert!(matches!(
        parse_record(&repeated_read, &mut context),
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
fn recognized_tool_schema_drift_is_rejected() {
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

    let cases = [
        (
            json!({"type":"response_item","payload":{
                "type":"function_call","name":"exec_command","arguments":"{"
            }}),
            "malformed_exec_command_arguments",
        ),
        (
            json!({"type":"response_item","payload":{
                "type":"function_call","name":"exec_command","arguments":"{}"
            }}),
            "missing_exec_command_cmd",
        ),
        (
            json!({"type":"response_item","payload":{
                "type":"function_call","name":"exec_command",
                "arguments":serde_json::to_string(&json!({"cmd":42})).unwrap()
            }}),
            "invalid_exec_command_cmd",
        ),
        (
            json!({"type":"response_item","payload":{
                "type":"function_call","name":"exec_command"
            }}),
            "missing_exec_command_arguments",
        ),
        (
            json!({"type":"response_item","payload":{
                "type":"function_call","name":"exec",
                "arguments":"await tools.exec_command({justification:\"missing cmd\"})"
            }}),
            "missing_nested_exec_command_cmd",
        ),
    ];
    for (value, expected) in cases {
        assert!(
            matches!(parse_record(&value, &mut context), ParseOutcome::Rejected(reason) if reason == expected),
            "expected stable rejection {expected}"
        );
    }

    assert!(matches!(
        parse_record(
            &json!({"type":"response_item","payload":{
                "type":"function_call","name":"unknown_tool","arguments":42
            }}),
            &mut context,
        ),
        ParseOutcome::Ignored
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

#[test]
fn originator_and_turn_are_carried_to_records() {
    use super::super::super::model::SessionOrigin;

    let mut context = Context::default();
    parse_record(
        &json!({"type":"session_meta","payload":{"id":"session","originator":"codex_exec"}}),
        &mut context,
    );
    parse_record(
        &json!({"type":"turn_context","payload":{"turn_id":"turn-1"}}),
        &mut context,
    );
    let outcome = parse_record(
        &json!({
            "type":"response_item",
            "timestamp":"2026-07-01T00:00:00Z",
            "payload":{
                "type":"function_call",
                "name":"exec_command",
                "call_id":"call-1",
                "arguments":"{\"cmd\":\"cat ~/.agents/skills/demo/SKILL.md\"}"
            }
        }),
        &mut context,
    );
    let ParseOutcome::Record(record) = outcome else {
        panic!("SKILL.md read must be imported, got {outcome:?}");
    };
    assert_eq!(record.origin, Some(SessionOrigin::Automated));
    assert_eq!(record.turn_id.as_deref(), Some("turn-1"));
}
