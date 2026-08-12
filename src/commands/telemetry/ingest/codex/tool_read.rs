use serde_json::Value;

mod shell;

pub(super) fn skill_entrypoint_names(payload: &Value) -> Result<Vec<String>, &'static str> {
    let Some(tool_name) = payload.get("name").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let commands = match tool_name {
        "exec_command" => vec![exec_command_input(payload)?],
        "exec" => exec_inputs(payload)?,
        _ => return Ok(Vec::new()),
    };
    Ok(shell::skill_names(&commands))
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
    let mut statement_start = 0usize;
    let mut braces = 0usize;
    let mut brackets = 0usize;
    let mut parentheses = 0usize;
    let mut commands = Vec::new();
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                let comment = index;
                index = skip_js_line_comment(bytes, index + 2);
                if braces == 0
                    && brackets == 0
                    && parentheses == 0
                    && source[statement_start..comment].trim().is_empty()
                {
                    statement_start = index;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let comment = index;
                index =
                    skip_js_block_comment(bytes, index + 2).ok_or("malformed_exec_arguments")?;
                if braces == 0
                    && brackets == 0
                    && parentheses == 0
                    && source[statement_start..comment].trim().is_empty()
                {
                    statement_start = index;
                }
            }
            b'\'' | b'"' => {
                index = skip_js_string(bytes, index).ok_or("malformed_exec_arguments")?;
            }
            b'`' => return Err("malformed_exec_arguments"),
            b'{' => {
                braces = braces.checked_add(1).ok_or("malformed_exec_arguments")?;
                index += 1;
            }
            b'}' => {
                braces = braces.checked_sub(1).ok_or("malformed_exec_arguments")?;
                index += 1;
            }
            b'[' => {
                brackets = brackets.checked_add(1).ok_or("malformed_exec_arguments")?;
                index += 1;
            }
            b']' => {
                brackets = brackets.checked_sub(1).ok_or("malformed_exec_arguments")?;
                index += 1;
            }
            b'(' => {
                parentheses = parentheses
                    .checked_add(1)
                    .ok_or("malformed_exec_arguments")?;
                index += 1;
            }
            b')' => {
                parentheses = parentheses
                    .checked_sub(1)
                    .ok_or("malformed_exec_arguments")?;
                index += 1;
            }
            b';' if braces == 0 && brackets == 0 && parentheses == 0 => {
                index += 1;
                statement_start = index;
            }
            _ if bytes.get(index..index + CALL.len()) == Some(CALL.as_bytes())
                && token_start_boundary(bytes, index)
                && token_end_boundary(bytes, index + CALL.len()) =>
            {
                if braces != 0
                    || brackets != 0
                    || parentheses != 0
                    || !is_direct_exec_prefix(&source[statement_start..index])
                {
                    index += CALL.len();
                    continue;
                }
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
    (braces == 0 && brackets == 0 && parentheses == 0)
        .then_some(commands)
        .ok_or("malformed_exec_arguments")
}

fn is_direct_exec_prefix(prefix: &str) -> bool {
    let prefix = prefix.trim();
    if matches!(prefix, "" | "await") {
        return true;
    }
    ["const", "let", "var"].iter().any(|declaration| {
        let Some(rest) = prefix.strip_prefix(declaration) else {
            return false;
        };
        if !rest.starts_with(char::is_whitespace) {
            return false;
        }
        let Some((name, value)) = rest.trim().split_once('=') else {
            return false;
        };
        let name = name.trim();
        !name.is_empty()
            && name.bytes().all(is_js_identifier_continue)
            && is_js_identifier_start(name.as_bytes()[0])
            && matches!(value.trim(), "" | "await")
    })
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
            _ => {
                let character = std::str::from_utf8(&bytes[index..]).ok()?.chars().next()?;
                value.push(character);
                index += character.len_utf8();
            }
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

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;

    use serde_json::{Value, json};

    use super::{extract_nested_exec_commands, skill_entrypoint_names};

    fn test_home() -> PathBuf {
        env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .expect("test requires a home directory")
    }

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
        let home = test_home();
        let value = payload(
            "exec_command",
            &format!(
                "cat '{}/.codex/skills/first/SKILL.md' \
                 '{}/.agents/skills/second/SKILL.md' \
                 '{}/.codex/skills/first/SKILL.md'",
                home.display(),
                home.display(),
                home.display()
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
                "printf ignored | cat \"$HOME/.claude/skills/piped/SKILL.md\"",
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
            "arguments":"const r = await tools.exec_command({cmd:\"cat \\\"$HOME/.vibeguard/installed/skills/nested/SKILL.md\\\"\"});text(r.output);"
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
    fn exec_accepts_utf8_strings() {
        let home = if cfg!(windows) {
            PathBuf::from(r"C:\Users\测试")
        } else {
            PathBuf::from("/Users/测试")
        };
        let command = format!("cat \"{}/.codex/skills/demo/SKILL.md\"", home.display());
        let source = format!(
            "// @exec: {{\"yield_time_ms\":30000}}\nconst r = await tools.exec_command({{cmd:{}, note:\"中文\"}});",
            serde_json::to_string(&command).unwrap()
        );
        let commands = extract_nested_exec_commands(&source).unwrap();
        assert_eq!(commands, [command]);
        assert_eq!(
            super::shell::skill_names_with_home(&commands, Some(&home)),
            ["demo"]
        );
    }

    #[test]
    fn exec_ignores_calls_in_dormant_javascript() {
        for arguments in [
            "const unused = () => tools.exec_command({cmd:\"cat $HOME/.codex/skills/callback/SKILL.md\"});",
            "if (false) { tools.exec_command({cmd:\"cat $HOME/.codex/skills/branch/SKILL.md\"}); }",
            "false && tools.exec_command({cmd:\"cat $HOME/.codex/skills/short-circuit/SKILL.md\"});",
        ] {
            let value = json!({
                "type":"function_call",
                "name":"exec",
                "call_id":"call-dormant",
                "arguments":arguments
            });
            assert!(
                skill_entrypoint_names(&value).unwrap().is_empty(),
                "{arguments}"
            );
        }
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
        let home = test_home();
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
