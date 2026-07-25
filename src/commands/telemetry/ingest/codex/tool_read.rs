use std::env;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

const READ_COMMANDS: &[&str] = &[
    "cat",
    "sed",
    "head",
    "tail",
    "less",
    "more",
    "bat",
    "get-content",
];

pub(super) fn skill_entrypoint_names(payload: &Value) -> Result<Vec<String>, &'static str> {
    let Some(tool_name) = payload.get("name").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let commands = match tool_name {
        "exec_command" => vec![exec_command_input(payload)?],
        "exec" => exec_inputs(payload)?,
        _ => return Ok(Vec::new()),
    };
    let segments = commands
        .iter()
        .flat_map(|command| shell_segments(command))
        .filter(|segment| {
            segment.first().is_some_and(|executable| {
                READ_COMMANDS
                    .iter()
                    .any(|candidate| executable.eq_ignore_ascii_case(candidate))
            })
        })
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return Ok(Vec::new());
    }
    let home = actual_home().ok_or("missing_home_directory")?;
    let mut names = Vec::new();
    for segment in segments {
        for operand in &segment[1..] {
            if let Some(name) = trusted_skill_name(operand, &home)
                && !names.iter().any(|existing| existing == &name)
            {
                names.push(name);
            }
        }
    }
    Ok(names)
}

fn exec_command_input(payload: &Value) -> Result<String, &'static str> {
    let arguments = payload
        .get("arguments")
        .ok_or("missing_exec_command_arguments")?
        .as_str()
        .ok_or("invalid_exec_command_arguments")?;
    let arguments: Value =
        serde_json::from_str(arguments).map_err(|_| "malformed_exec_command_arguments")?;
    let object = arguments
        .as_object()
        .ok_or("invalid_exec_command_arguments")?;
    object
        .get("cmd")
        .ok_or("missing_exec_command_cmd")?
        .as_str()
        .map(str::to_string)
        .ok_or("invalid_exec_command_cmd")
}

fn exec_inputs(payload: &Value) -> Result<Vec<String>, &'static str> {
    let source = payload
        .get("arguments")
        .ok_or("missing_exec_arguments")?
        .as_str()
        .ok_or("invalid_exec_arguments")?;
    extract_nested_exec_commands(source)
}

fn extract_nested_exec_commands(source: &str) -> Result<Vec<String>, &'static str> {
    const CALL: &str = "tools.exec_command";
    let bytes = source.as_bytes();
    let mut index = 0usize;
    let mut commands = Vec::new();
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = skip_js_line_comment(bytes, index + 2);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index =
                    skip_js_block_comment(bytes, index + 2).ok_or("malformed_exec_arguments")?;
            }
            b'\'' | b'"' => {
                index = skip_js_string(bytes, index).ok_or("malformed_exec_arguments")?;
            }
            b'`' => return Err("malformed_exec_arguments"),
            _ if bytes.get(index..index + CALL.len()) == Some(CALL.as_bytes())
                && token_start_boundary(bytes, index)
                && token_end_boundary(bytes, index + CALL.len()) =>
            {
                index += CALL.len();
                index = skip_ascii_whitespace(bytes, index);
                if bytes.get(index) != Some(&b'(') {
                    return Err("malformed_nested_exec_command");
                }
                index = skip_ascii_whitespace(bytes, index + 1);
                if bytes.get(index) != Some(&b'{') {
                    return Err("invalid_nested_exec_command_arguments");
                }
                let object_end =
                    matching_js_brace(bytes, index).ok_or("malformed_nested_exec_command")?;
                let object = &source[index + 1..object_end];
                commands.push(extract_cmd_property(object)?);
                index = skip_ascii_whitespace(bytes, object_end + 1);
                if bytes.get(index) != Some(&b')') {
                    return Err("malformed_nested_exec_command");
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    Ok(commands)
}

fn extract_cmd_property(object: &str) -> Result<String, &'static str> {
    let bytes = object.as_bytes();
    let mut index = 0usize;
    let mut depth = 0usize;
    let mut command = None;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = skip_js_line_comment(bytes, index + 2);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_js_block_comment(bytes, index + 2)
                    .ok_or("malformed_nested_exec_command")?;
            }
            b'.' if depth == 0 && bytes.get(index..index + 3) == Some(b"...") => {
                return Err("invalid_nested_exec_command_arguments");
            }
            b'\'' | b'"' => {
                let (value, end) =
                    parse_js_string(bytes, index).ok_or("malformed_nested_exec_command")?;
                let after = skip_ascii_whitespace(bytes, end);
                if depth == 0 && value == "cmd" && bytes.get(after) == Some(&b':') {
                    if command.is_some() {
                        return Err("invalid_nested_exec_command_arguments");
                    }
                    let (value, end) = parse_cmd_value(bytes, after + 1)?;
                    command = Some(value);
                    index = end;
                    continue;
                }
                index = end;
            }
            b'[' if depth == 0 => {
                return Err("invalid_nested_exec_command_arguments");
            }
            b'{' | b'[' | b'(' => {
                depth = depth
                    .checked_add(1)
                    .ok_or("malformed_nested_exec_command")?;
                index += 1;
            }
            b'}' | b']' | b')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or("malformed_nested_exec_command")?;
                index += 1;
            }
            byte if depth == 0 && is_js_identifier_start(byte) => {
                let start = index;
                index += 1;
                while index < bytes.len() && is_js_identifier_continue(bytes[index]) {
                    index += 1;
                }
                let after = skip_ascii_whitespace(bytes, index);
                if &object[start..index] == "cmd" && bytes.get(after) == Some(&b':') {
                    if command.is_some() {
                        return Err("invalid_nested_exec_command_arguments");
                    }
                    let (value, end) = parse_cmd_value(bytes, after + 1)?;
                    command = Some(value);
                    index = end;
                }
            }
            _ => index += 1,
        }
    }
    command.ok_or("missing_nested_exec_command_cmd")
}

fn parse_cmd_value(bytes: &[u8], index: usize) -> Result<(String, usize), &'static str> {
    let index = skip_ascii_whitespace(bytes, index);
    if !matches!(bytes.get(index), Some(b'\'') | Some(b'"')) {
        return Err("invalid_nested_exec_command_cmd");
    }
    let (value, end) = parse_js_string(bytes, index).ok_or("malformed_nested_exec_command")?;
    let after = skip_ascii_whitespace(bytes, end);
    if after < bytes.len() && bytes[after] != b',' {
        return Err("invalid_nested_exec_command_cmd");
    }
    Ok((value, after))
}

fn parse_js_string(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    let quote = *bytes.get(start)?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let mut value = String::new();
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            byte if byte == quote => return Some((value, index + 1)),
            b'\\' => {
                index += 1;
                let escaped = *bytes.get(index)?;
                value.push(match escaped {
                    b'\\' => '\\',
                    b'\'' => '\'',
                    b'"' => '"',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    _ => return None,
                });
                index += 1;
            }
            byte if byte.is_ascii() => {
                value.push(char::from(byte));
                index += 1;
            }
            _ => return None,
        }
    }
    None
}

fn skip_js_string(bytes: &[u8], start: usize) -> Option<usize> {
    parse_js_string(bytes, start).map(|(_, end)| end)
}

fn matching_js_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = skip_js_line_comment(bytes, index + 2);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_js_block_comment(bytes, index + 2)?;
            }
            b'\'' | b'"' => index = skip_js_string(bytes, index)?,
            b'`' => return None,
            b'{' => {
                depth = depth.checked_add(1)?;
                index += 1;
            }
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

fn skip_js_line_comment(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(|byte| *byte != b'\n') {
        index += 1;
    }
    index
}

fn skip_js_block_comment(bytes: &[u8], mut index: usize) -> Option<usize> {
    while index + 1 < bytes.len() {
        if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            return Some(index + 2);
        }
        index += 1;
    }
    None
}

fn token_start_boundary(bytes: &[u8], index: usize) -> bool {
    index == 0 || !is_js_identifier_continue(bytes[index - 1])
}

fn token_end_boundary(bytes: &[u8], index: usize) -> bool {
    index == bytes.len() || !is_js_identifier_continue(bytes[index])
}

fn is_js_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$')
}

fn is_js_identifier_continue(byte: u8) -> bool {
    is_js_identifier_start(byte) || byte.is_ascii_digit() || byte == b'.'
}

fn skip_ascii_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

fn shell_segments(command: &str) -> Vec<Vec<String>> {
    let mut segments = Vec::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut chars = command.chars().peekable();
    let mut quote = None;
    let mut invalid_segment = false;
    while let Some(ch) = chars.next() {
        match quote {
            Some('\'') => {
                if ch == '\'' {
                    quote = None;
                } else {
                    word.push(ch);
                }
            }
            Some('"') => match ch {
                '"' => quote = None,
                '\\' => match chars.next() {
                    Some(escaped) => word.push(escaped),
                    None => invalid_segment = true,
                },
                '`' => invalid_segment = true,
                '$' if chars.peek() == Some(&'(') => invalid_segment = true,
                _ => word.push(ch),
            },
            _ => match ch {
                '\'' | '"' => quote = Some(ch),
                '\\' => match chars.next() {
                    Some(escaped) => word.push(escaped),
                    None => invalid_segment = true,
                },
                '`' => invalid_segment = true,
                '$' if chars.peek() == Some(&'(') => invalid_segment = true,
                '<' | '>' => invalid_segment = true,
                '#' if word.is_empty() => {
                    for comment in chars.by_ref() {
                        if comment == '\n' {
                            break;
                        }
                    }
                    finish_segment(&mut segments, &mut words, &mut word, &mut invalid_segment);
                }
                ';' | '\n' | '|' | '&' => {
                    finish_segment(&mut segments, &mut words, &mut word, &mut invalid_segment);
                    if matches!(chars.peek(), Some(next) if *next == ch && matches!(ch, '|' | '&'))
                    {
                        chars.next();
                    }
                }
                whitespace if whitespace.is_whitespace() => finish_word(&mut words, &mut word),
                _ => word.push(ch),
            },
        }
    }
    if quote.is_some() {
        invalid_segment = true;
    }
    finish_segment(&mut segments, &mut words, &mut word, &mut invalid_segment);
    segments
}

fn finish_word(words: &mut Vec<String>, word: &mut String) {
    if !word.is_empty() {
        words.push(std::mem::take(word));
    }
}

fn finish_segment(
    segments: &mut Vec<Vec<String>>,
    words: &mut Vec<String>,
    word: &mut String,
    invalid: &mut bool,
) {
    finish_word(words, word);
    if !*invalid && !words.is_empty() {
        segments.push(std::mem::take(words));
    } else {
        words.clear();
    }
    *invalid = false;
}

fn actual_home() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| env::var_os("USERPROFILE").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .and_then(|path| normalize_path(&path))
}

fn trusted_skill_name(raw: &str, home: &Path) -> Option<String> {
    let expanded = expand_home(raw, home)?;
    let normalized = normalize_path(&expanded)?;
    let ordinary_roots = [
        home.join(".codex/skills"),
        home.join(".agents/skills"),
        home.join(".claude/skills"),
        home.join(".loom-registry/skills"),
        home.join(".vibeguard/installed/skills"),
    ];
    for root in ordinary_roots {
        if let Ok(relative) = normalized.strip_prefix(root) {
            return skill_name_from_relative(relative);
        }
    }

    let plugin_root = home.join(".codex/plugins/cache");
    let relative = normalized.strip_prefix(plugin_root).ok()?;
    let components = normal_components(relative)?;
    let skills = components
        .iter()
        .enumerate()
        .rev()
        .find(|(index, component)| **component == "skills" && *index > 0)?;
    let suffix = &components[skills.0 + 1..];
    if suffix.len() != 2 || suffix[1] != "SKILL.md" {
        return None;
    }
    valid_skill_path_component(suffix[0]).then(|| suffix[0].to_string())
}

fn expand_home(raw: &str, home: &Path) -> Option<PathBuf> {
    let suffix = raw
        .strip_prefix("~/")
        .or_else(|| raw.strip_prefix("$HOME/"))
        .or_else(|| raw.strip_prefix("${HOME}/"));
    if let Some(suffix) = suffix {
        return Some(home.join(suffix));
    }
    let path = PathBuf::from(raw);
    path.is_absolute().then_some(path)
}

fn normalize_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => return None,
            Component::Normal(value) => normalized.push(value),
        }
    }
    Some(normalized)
}

fn skill_name_from_relative(relative: &Path) -> Option<String> {
    let components = normal_components(relative)?;
    if components.len() < 2 || components.last()? != &"SKILL.md" {
        return None;
    }
    let name = components[components.len() - 2];
    valid_skill_path_component(name).then(|| name.to_string())
}

fn normal_components(path: &Path) -> Option<Vec<&str>> {
    path.components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect()
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
    use std::env;
    use std::path::PathBuf;

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
            assert!(skill_entrypoint_names(&value).unwrap().is_empty());
        }
    }

    #[test]
    fn returns_distinct_skill_names_in_path_order() {
        let home = env::var("HOME").expect("test requires HOME");
        let value = payload(
            "exec_command",
            &format!(
                "cat {home}/.codex/skills/first/SKILL.md \
                 {home}/.agents/skills/second/SKILL.md \
                 {home}/.codex/skills/first/SKILL.md"
            ),
        );
        assert_eq!(skill_entrypoint_names(&value).unwrap(), ["first", "second"]);
    }

    #[test]
    fn read_paths_are_bound_to_their_command_segment() {
        let cases = [
            (
                "echo 'cat $HOME/.codex/skills/free-text/SKILL.md'; \
                 rm $HOME/.agents/skills/deleted/SKILL.md",
                Vec::<String>::new(),
            ),
            (
                "cat \"$HOME/.codex/skills/read-one/SKILL.md\" && \
                 rm \"$HOME/.agents/skills/deleted/SKILL.md\"",
                vec!["read-one".to_string()],
            ),
            (
                "printf ignored | cat '$HOME/.claude/skills/piped/SKILL.md'",
                vec!["piped".to_string()],
            ),
            (
                "echo sed -n 1p $HOME/.codex/skills/free-text/SKILL.md",
                Vec::<String>::new(),
            ),
        ];
        for (command, expected) in cases {
            assert_eq!(
                skill_entrypoint_names(&payload("exec_command", command)).unwrap(),
                expected,
                "{command}"
            );
        }
    }

    #[test]
    fn exec_and_exec_command_extract_supported_reads() {
        let exec_command = payload(
            "exec_command",
            "sed -n '1,80p' \"$HOME/.loom-registry/skills/direct/SKILL.md\"",
        );
        assert_eq!(skill_entrypoint_names(&exec_command).unwrap(), ["direct"]);

        let exec = json!({
            "type":"function_call",
            "name":"exec",
            "call_id":"call-1",
            "arguments":"const r = await tools.exec_command({cmd:\"cat '$HOME/.vibeguard/installed/skills/nested/SKILL.md'\"});text(r.output);"
        });
        assert_eq!(skill_entrypoint_names(&exec).unwrap(), ["nested"]);

        let text_only = json!({
            "type":"function_call",
            "name":"exec",
            "call_id":"call-2",
            "arguments":"text(\"tools.exec_command({cmd: cat $HOME/.codex/skills/fake/SKILL.md})\")"
        });
        assert!(skill_entrypoint_names(&text_only).unwrap().is_empty());
    }

    #[test]
    fn exec_rejects_dynamic_or_ambiguous_cmd_values() {
        for (arguments, reason) in [
            (
                "await tools.exec_command({cmd:\"cat $HOME/.codex/skills/a/SKILL.md\" + suffix})",
                "invalid_nested_exec_command_cmd",
            ),
            (
                "await tools.exec_command({cmd:\"cat $HOME/.codex/skills/a/SKILL.md\",cmd:\"cat $HOME/.codex/skills/b/SKILL.md\"})",
                "invalid_nested_exec_command_arguments",
            ),
            (
                "await tools.exec_command({cmd:\"cat $HOME/.codex/skills/a/SKILL.md\",...override})",
                "invalid_nested_exec_command_arguments",
            ),
        ] {
            let value = json!({
                "type":"function_call",
                "name":"exec",
                "call_id":"call-dynamic",
                "arguments":arguments
            });
            assert_eq!(skill_entrypoint_names(&value), Err(reason));
        }

        let comments_only = json!({
            "type":"function_call",
            "name":"exec",
            "call_id":"call-comment",
            "arguments":"// tools.exec_command({cmd:\"cat $HOME/.codex/skills/fake/SKILL.md\"})\ntext(\"done\");"
        });
        assert!(skill_entrypoint_names(&comments_only).unwrap().is_empty());
    }

    #[test]
    fn trusted_paths_are_component_aware() {
        let home = PathBuf::from(env::var_os("HOME").expect("test requires HOME"));
        let home = home.to_string_lossy();
        let plugin =
            format!("{home}/.codex/plugins/cache/vendor/plugin/1.0/skills/plugin-skill/SKILL.md");
        assert_eq!(
            skill_entrypoint_names(&payload("exec_command", &format!("cat '{plugin}'"))).unwrap(),
            ["plugin-skill"]
        );

        for command in [
            format!("cat '{home}/.codex-evil/.codex/skills/prefix/SKILL.md'"),
            format!("cat '{home}/project/.codex/skills/nested/SKILL.md'"),
            format!("cat '{home}/.codex/skills/../escape/SKILL.md'"),
            format!("cat '{home}/.codex/plugins/cache/skills/no-plugin/SKILL.md'"),
            "cat /tmp/.codex/skills/outside/SKILL.md".to_string(),
        ] {
            assert!(
                skill_entrypoint_names(&payload("exec_command", &command))
                    .unwrap()
                    .is_empty(),
                "{command}"
            );
        }
    }
}
