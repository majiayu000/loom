use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{ImportedInvocation, ImportedRecord, ParseOutcome};

pub(super) fn parse_record(value: &Value) -> ParseOutcome {
    let mut invocations = Vec::new();
    if let Some(content) = value.pointer("/message/content").and_then(Value::as_array) {
        for (ordinal, block) in content.iter().enumerate() {
            if block.get("type").and_then(Value::as_str) == Some("tool_use")
                && block.get("name").and_then(Value::as_str) == Some("Skill")
            {
                let Some(name) = block.pointer("/input/skill").and_then(Value::as_str) else {
                    return ParseOutcome::Rejected("missing_skill_name");
                };
                let Some(identity) = block.get("id").and_then(Value::as_str) else {
                    return ParseOutcome::Rejected("missing_invocation_identity");
                };
                invocations.push(ImportedInvocation {
                    name: name.to_string(),
                    identity: identity.to_string(),
                    ordinal,
                });
            }
        }
    }
    if let Some(content) = value.pointer("/message/content").and_then(Value::as_str)
        && let Some(name) = explicit_command_name(content)
    {
        invocations.push(ImportedInvocation {
            name: name.to_string(),
            identity: "command-name".to_string(),
            ordinal: 0,
        });
    }
    finish_record(value, invocations)
}

fn finish_record(value: &Value, invocations: Vec<ImportedInvocation>) -> ParseOutcome {
    if invocations.is_empty() {
        return ParseOutcome::Ignored;
    }
    let Some(stable_record_key) = value.get("uuid").and_then(Value::as_str) else {
        return ParseOutcome::Rejected("missing_stable_record_key");
    };
    let Some(session_id) = value.get("session_id").and_then(Value::as_str) else {
        return ParseOutcome::Rejected("missing_session_identity");
    };
    let Some(timestamp) = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .map(|value| value.with_timezone(&Utc))
    else {
        return ParseOutcome::Rejected("invalid_timestamp");
    };
    ParseOutcome::Record(ImportedRecord {
        stable_record_key: stable_record_key.to_string(),
        session_id: session_id.to_string(),
        timestamp,
        invocations,
    })
}

fn explicit_command_name(content: &str) -> Option<&str> {
    let value = content
        .strip_prefix("<command-name>/")?
        .strip_suffix("</command-name>")?;
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ParseOutcome, parse_record};

    #[test]
    fn free_text_is_not_an_invocation() {
        let value = json!({
            "uuid": "record",
            "session_id": "session",
            "timestamp": "2026-07-01T00:00:00Z",
            "message": {"content": "please mention /demo"}
        });
        assert!(matches!(parse_record(&value), ParseOutcome::Ignored));
    }
}
