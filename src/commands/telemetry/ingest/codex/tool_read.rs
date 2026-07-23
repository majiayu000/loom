use serde_json::Value;

pub(super) fn skill_entrypoint_names(payload: &Value) -> Vec<String> {
    let Some(tool_name) = payload.get("name").and_then(Value::as_str) else {
        return Vec::new();
    };
    if !matches!(tool_name, "exec_command" | "exec") {
        return Vec::new();
    }
    let Some(arguments) = payload.get("arguments").and_then(Value::as_str) else {
        return Vec::new();
    };
    if tool_name == "exec" && !arguments.contains("exec_command") {
        return Vec::new();
    }
    let input = if tool_name == "exec_command" {
        serde_json::from_str::<Value>(arguments)
            .ok()
            .and_then(|arguments| {
                arguments
                    .get("cmd")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
    } else {
        Some(arguments.to_string())
    };
    input
        .filter(|input| contains_read_command(input))
        .map_or_else(Vec::new, |input| names_from_paths(&input))
}

fn contains_read_command(input: &str) -> bool {
    const READ_COMMANDS: &[&str] = &["cat", "sed", "head", "tail", "less", "more", "bat"];
    let lower = input.to_ascii_lowercase();
    READ_COMMANDS
        .iter()
        .any(|command| contains_token(&lower, command))
        || contains_token(&lower, "get-content")
}

fn contains_token(input: &str, token: &str) -> bool {
    input.match_indices(token).any(|(start, _)| {
        let before = input[..start].chars().next_back();
        let after = input[start + token.len()..].chars().next();
        before.is_none_or(|ch| !is_token_char(ch)) && after.is_none_or(|ch| !is_token_char(ch))
    })
}

fn is_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn names_from_paths(input: &str) -> Vec<String> {
    const ENTRYPOINT: &str = "/SKILL.md";
    let normalized = input.replace('\\', "/");
    let mut names = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative) = normalized[search_start..].find(ENTRYPOINT) {
        let skill_end = search_start + relative;
        let Some(skill_start) = normalized[..skill_end].rfind('/').map(|index| index + 1) else {
            search_start = skill_end + ENTRYPOINT.len();
            continue;
        };
        let token_start = normalized[..skill_start]
            .rfind(is_path_boundary)
            .map_or(0, |index| index + 1);
        let token_end = skill_end + ENTRYPOINT.len();
        let path = &normalized[token_start..token_end];
        let name = &normalized[skill_start..skill_end];
        if trusted_skill_path(path)
            && valid_skill_path_component(name)
            && !names.iter().any(|existing| existing == name)
        {
            names.push(name.to_string());
        }
        search_start = token_end;
    }
    names
}

fn is_path_boundary(ch: char) -> bool {
    ch.is_ascii_whitespace()
        || matches!(
            ch,
            '"' | '\'' | '`' | '=' | '(' | ')' | '{' | '}' | '[' | ']' | ',' | ';' | '|' | '&'
        )
}

fn trusted_skill_path(path: &str) -> bool {
    const ROOTS: &[&str] = &[
        ".codex/skills/",
        ".agents/skills/",
        ".claude/skills/",
        ".loom-registry/skills/",
        ".vibeguard/installed/skills/",
    ];
    ROOTS.iter().any(|root| path.contains(root))
        || (path.contains(".codex/plugins/cache/") && path.contains("/skills/"))
}

fn valid_skill_path_component(name: &str) -> bool {
    !matches!(name, "" | "." | "..")
        && name.len() <= 128
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::skill_entrypoint_names;

    fn payload(name: &str, command: &str) -> Value {
        json!({
            "type":"function_call",
            "name":name,
            "call_id":"call-1",
            "arguments":serde_json::to_string(&json!({"cmd":command})).unwrap()
        })
    }

    #[test]
    fn requires_read_tool_and_trusted_root() {
        for value in [
            payload("apply_patch", "cat /home/user/.codex/skills/demo/SKILL.md"),
            payload("exec_command", "rm /home/user/.codex/skills/demo/SKILL.md"),
            payload("exec_command", "cat /tmp/fixtures/skills/demo/SKILL.md"),
        ] {
            assert!(skill_entrypoint_names(&value).is_empty());
        }
    }

    #[test]
    fn returns_distinct_skill_names_in_path_order() {
        let value = payload(
            "exec_command",
            "cat /home/user/.codex/skills/first/SKILL.md \
             /home/user/.agents/skills/second/SKILL.md \
             /home/user/.codex/skills/first/SKILL.md",
        );
        assert_eq!(skill_entrypoint_names(&value), ["first", "second"]);
    }
}
