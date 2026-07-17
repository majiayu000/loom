use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::PathBuf;

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
    let Some(session_id) = value.get("sessionId").and_then(Value::as_str) else {
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
        workspace: value.get("cwd").and_then(Value::as_str).map(PathBuf::from),
        timestamp,
        invocations,
    })
}

fn explicit_command_name(content: &str) -> Option<&str> {
    const OPEN: &str = "<command-name>/";
    const CLOSE: &str = "</command-name>";
    let tail = content.get(content.find(OPEN)? + OPEN.len()..)?;
    let value = tail.get(..tail.find(CLOSE)?)?;
    if value.is_empty() {
        return None;
    }
    match classify_command(value) {
        CommandClassification::NativeIgnore => None,
        CommandClassification::Skill(name) => Some(name),
    }
}

enum CommandClassification<'a> {
    NativeIgnore,
    Skill(&'a str),
}

fn classify_command(name: &str) -> CommandClassification<'_> {
    match name {
        "checkup" | "doctor" => return CommandClassification::Skill("doctor"),
        "proactive" | "loop" => return CommandClassification::Skill("loop"),
        "ultrareview" | "code-review" => {
            return CommandClassification::Skill("code-review");
        }
        "batch"
        | "claude-api"
        | "dataviz"
        | "debug"
        | "deep-research"
        | "design-sync"
        | "fewer-permission-prompts"
        | "run"
        | "run-skill-generator"
        | "simplify"
        | "verify" => return CommandClassification::Skill(name),
        _ => {}
    }
    const NATIVE_COMMANDS: &[&str] = &[
        "add-dir",
        "advisor",
        "agents",
        "allowed-tools",
        "android",
        "app",
        "autofix-pr",
        "background",
        "bashes",
        "bg",
        "branch",
        "btw",
        "bug",
        "cd",
        "checkpoint",
        "chrome",
        "clear",
        "color",
        "compact",
        "config",
        "context",
        "continue",
        "copy",
        "cost",
        "design-login",
        "desktop",
        "diff",
        "effort",
        "exit",
        "export",
        "extra-usage",
        "fast",
        "feedback",
        "focus",
        "fork",
        "goal",
        "heapdump",
        "help",
        "hooks",
        "ide",
        "init",
        "insights",
        "install-github-app",
        "install-slack-app",
        "ios",
        "keybindings",
        "login",
        "logout",
        "mcp",
        "memory",
        "mobile",
        "model",
        "new",
        "passes",
        "permissions",
        "plan",
        "plugin",
        "powerup",
        "pr-comments",
        "privacy-settings",
        "quit",
        "radio",
        "rc",
        "recap",
        "release-notes",
        "reload-plugins",
        "reload-skills",
        "remote-control",
        "remote-env",
        "rename",
        "reset",
        "resume",
        "review",
        "rewind",
        "routines",
        "sandbox",
        "schedule",
        "scroll-speed",
        "security-review",
        "settings",
        "setup-bedrock",
        "setup-vertex",
        "share",
        "skills",
        "stats",
        "status",
        "statusline",
        "stickers",
        "stop",
        "tasks",
        "team-onboarding",
        "teleport",
        "terminal-setup",
        "theme",
        "tp",
        "tui",
        "ultraplan",
        "undo",
        "upgrade",
        "usage",
        "usage-credits",
        "vim",
    ];
    if NATIVE_COMMANDS.contains(&name) {
        CommandClassification::NativeIgnore
    } else {
        CommandClassification::Skill(name)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ParseOutcome, parse_record};

    #[test]
    fn native_session_and_embedded_command_are_parsed() {
        let value = json!({
            "uuid": "record",
            "sessionId": "session",
            "cwd": "/workspace/demo",
            "timestamp": "2026-07-01T00:00:00Z",
            "message": {"content": concat!(
                "<command-message>demo args</command-message>",
                "<command-name>/demo</command-name>",
                "<command-args>args</command-args>"
            )}
        });
        let ParseOutcome::Record(record) = parse_record(&value) else {
            panic!("native Claude command must parse");
        };
        assert_eq!(record.session_id, "session");
        assert_eq!(record.invocations.len(), 1);
        assert_eq!(record.invocations[0].name, "demo");
    }

    #[test]
    fn free_text_is_not_an_invocation() {
        let value = json!({
            "uuid": "record",
            "sessionId": "session",
            "timestamp": "2026-07-01T00:00:00Z",
            "message": {"content": "please mention /demo"}
        });
        assert!(matches!(parse_record(&value), ParseOutcome::Ignored));
    }

    #[test]
    fn builtin_command_is_not_a_skill_invocation() {
        for name in [
            "add-dir", "agents", "clear", "context", "resume", "routines", "schedule", "tasks",
        ] {
            let value = json!({
                "uuid": "record",
                "sessionId": "session",
                "timestamp": "2026-07-01T00:00:00Z",
                "message": {"content": format!("<command-name>/{name}</command-name>")}
            });
            assert!(
                matches!(parse_record(&value), ParseOutcome::Ignored),
                "built-in /{name} must not be imported"
            );
        }
    }

    #[test]
    fn bundled_skill_aliases_are_canonicalized() {
        for (name, canonical) in [
            ("doctor", "doctor"),
            ("checkup", "doctor"),
            ("loop", "loop"),
            ("proactive", "loop"),
            ("code-review", "code-review"),
            ("ultrareview", "code-review"),
            ("demo", "demo"),
        ] {
            let value = json!({
                "uuid": "record",
                "sessionId": "session",
                "timestamp": "2026-07-01T00:00:00Z",
                "message": {"content": format!("<command-name>/{name}</command-name>")}
            });
            let ParseOutcome::Record(record) = parse_record(&value) else {
                panic!("skill /{name} must be imported");
            };
            assert_eq!(record.invocations[0].name, canonical);
        }
    }
}
