use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

use super::{ImportedInvocation, ImportedRecord, ParseOutcome};

pub(super) fn parse_record(value: &Value) -> ParseOutcome {
    let invocation = if let Some(activation) = value.get("activation") {
        if activation.get("kind").and_then(Value::as_str) != Some("skill") {
            return ParseOutcome::Ignored;
        }
        let Some(name) = activation.get("name").and_then(Value::as_str) else {
            return ParseOutcome::Rejected("missing_skill_name");
        };
        let Some(identity) = activation.get("id").and_then(Value::as_str) else {
            return ParseOutcome::Rejected("missing_invocation_identity");
        };
        Some((name, identity))
    } else {
        match value.get("type").and_then(Value::as_str) {
            Some("skill_activation") => {
                let Some(name) = value.get("skill").and_then(Value::as_str) else {
                    return ParseOutcome::Rejected("missing_skill_name");
                };
                let Some(identity) = value.get("tool_call_id").and_then(Value::as_str) else {
                    return ParseOutcome::Rejected("missing_invocation_identity");
                };
                Some((name, identity))
            }
            Some("command_activation") => {
                let Some(name) = value.get("command").and_then(Value::as_str) else {
                    return ParseOutcome::Rejected("missing_skill_name");
                };
                let Some(identity) = value.get("tool_call_id").and_then(Value::as_str) else {
                    return ParseOutcome::Rejected("missing_invocation_identity");
                };
                Some((name, identity))
            }
            _ => None,
        }
    };
    let Some((name, identity)) = invocation else {
        return ParseOutcome::Ignored;
    };
    let Some(stable_record_key) = value.get("id").and_then(Value::as_str) else {
        return ParseOutcome::Rejected("missing_stable_record_key");
    };
    let Some(session_id) = value.get("session_id").and_then(Value::as_str) else {
        return ParseOutcome::Rejected("missing_session_identity");
    };
    let Some(timestamp) = parse_timestamp(value) else {
        return ParseOutcome::Rejected("invalid_timestamp");
    };
    ParseOutcome::Record(ImportedRecord {
        stable_record_key: stable_record_key.to_string(),
        session_id: session_id.to_string(),
        timestamp,
        invocations: vec![ImportedInvocation {
            name: name.to_string(),
            identity: identity.to_string(),
            ordinal: 0,
        }],
    })
}

fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(raw) = value.get("timestamp").and_then(Value::as_str) {
        return DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|value| value.with_timezone(&Utc));
    }
    value
        .get("ts")
        .and_then(Value::as_i64)
        .and_then(|seconds| Utc.timestamp_opt(seconds, 0).single())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ParseOutcome, parse_record};

    #[test]
    fn free_text_is_not_an_invocation() {
        let value = json!({
            "id": "record",
            "session_id": "session",
            "ts": 1,
            "text": "demo"
        });
        assert!(matches!(parse_record(&value), ParseOutcome::Ignored));
    }

    #[test]
    fn known_activation_missing_identity_is_rejected() {
        let value = json!({
            "id": "record",
            "session_id": "session",
            "timestamp": "2026-07-01T00:00:00Z",
            "type": "skill_activation",
            "skill": "demo"
        });
        assert!(matches!(
            parse_record(&value),
            ParseOutcome::Rejected("missing_invocation_identity")
        ));
    }
}
