use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SkillYaml {
    Null,
    Bool(bool),
    Integer(i64),
    Real(String),
    String(String),
    Sequence(Vec<Self>),
    Mapping(BTreeMap<String, Self>),
}

pub(crate) fn parse_skill_yaml(raw: &str) -> Result<SkillYaml, String> {
    Parser::new(raw)?.parse()
}

struct SourceLine<'a> {
    number: usize,
    indent: usize,
    raw: &'a str,
}

struct Parser<'a> {
    lines: Vec<SourceLine<'a>>,
}

impl<'a> Parser<'a> {
    fn new(raw: &'a str) -> Result<Self, String> {
        let mut lines = Vec::new();
        for (index, raw_line) in raw.lines().enumerate() {
            let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let indent = raw_line.bytes().take_while(|byte| *byte == b' ').count();
            if raw_line.as_bytes().get(indent) == Some(&b'\t') {
                return Err(format!(
                    "line {}: tabs are not supported for YAML indentation",
                    index + 1
                ));
            }
            lines.push(SourceLine {
                number: index + 1,
                indent,
                raw: raw_line,
            });
        }
        Ok(Self { lines })
    }

    fn parse(&self) -> Result<SkillYaml, String> {
        let Some(start) = self.next_content(0) else {
            return Err("YAML document is empty".to_string());
        };
        if self.lines[start].indent != 0 {
            return Err(self.error(start, "root YAML mapping must not be indented"));
        }
        let (value, end) = self.parse_node(start, 0)?;
        if let Some(extra) = self.next_content(end) {
            return Err(self.error(extra, "unexpected content after root YAML value"));
        }
        if !matches!(value, SkillYaml::Mapping(_)) {
            return Err("YAML root must be a mapping".to_string());
        }
        Ok(value)
    }

    fn parse_node(&self, start: usize, indent: usize) -> Result<(SkillYaml, usize), String> {
        let content = self.content(start);
        if sequence_item(content).is_some() {
            self.parse_sequence(start, indent)
        } else {
            self.parse_mapping(start, indent)
        }
    }

    fn parse_mapping(&self, start: usize, indent: usize) -> Result<(SkillYaml, usize), String> {
        let mut mapping = BTreeMap::new();
        let mut cursor = start;
        loop {
            let Some(index) = self.next_content(cursor) else {
                cursor = self.lines.len();
                break;
            };
            let line = &self.lines[index];
            if line.indent < indent {
                cursor = index;
                break;
            }
            if line.indent > indent {
                return Err(self.error(index, "unexpected indentation in YAML mapping"));
            }
            let content = self.content(index);
            if sequence_item(content).is_some() {
                if index == start {
                    return Err(self.error(index, "expected a YAML mapping"));
                }
                cursor = index;
                break;
            }
            let (raw_key, raw_value) = split_mapping_entry(content)
                .ok_or_else(|| self.error(index, "expected `key: value` YAML mapping entry"))?;
            let key = parse_key(raw_key).map_err(|err| self.error(index, &err))?;
            if mapping.contains_key(&key) {
                return Err(self.error(index, &format!("duplicate YAML key `{key}`")));
            }

            let value_text = strip_comment(raw_value).trim();
            let (value, next) = if value_text.is_empty() {
                match self.next_content(index + 1) {
                    Some(child)
                        if self.lines[child].indent > indent
                            || (self.lines[child].indent == indent
                                && sequence_item(self.content(child)).is_some()) =>
                    {
                        let child_indent = self.lines[child].indent;
                        self.parse_node(child, child_indent)?
                    }
                    _ => (SkillYaml::Null, index + 1),
                }
            } else if block_header(value_text).is_some() {
                self.parse_block_scalar(index, indent, value_text)?
            } else {
                let (logical, next) = self.logical_value(index, raw_value)?;
                (
                    parse_inline(logical.trim()).map_err(|err| self.error(index, &err))?,
                    next,
                )
            };
            mapping.insert(key, value);
            cursor = next;
        }
        Ok((SkillYaml::Mapping(mapping), cursor))
    }

    fn parse_sequence(&self, start: usize, indent: usize) -> Result<(SkillYaml, usize), String> {
        let mut items = Vec::new();
        let mut cursor = start;
        loop {
            let Some(index) = self.next_content(cursor) else {
                cursor = self.lines.len();
                break;
            };
            let line = &self.lines[index];
            if line.indent < indent {
                cursor = index;
                break;
            }
            if line.indent > indent {
                return Err(self.error(index, "unexpected indentation in YAML sequence"));
            }
            let Some(raw_value) = sequence_item(self.content(index)) else {
                cursor = index;
                break;
            };
            let value_text = strip_comment(raw_value).trim();
            let (value, next) = if value_text.is_empty() {
                let child = self
                    .next_content(index + 1)
                    .ok_or_else(|| self.error(index, "YAML sequence item has no value"))?;
                if self.lines[child].indent <= indent {
                    return Err(self.error(index, "YAML sequence item has no nested value"));
                }
                self.parse_node(child, self.lines[child].indent)?
            } else if split_mapping_entry(value_text).is_some() {
                return Err(self.error(
                    index,
                    "block mappings inside sequences are unsupported; use an inline mapping",
                ));
            } else if block_header(value_text).is_some() {
                self.parse_block_scalar(index, indent, value_text)?
            } else {
                let (logical, next) = self.logical_value(index, raw_value)?;
                (
                    parse_inline(logical.trim()).map_err(|err| self.error(index, &err))?,
                    next,
                )
            };
            items.push(value);
            cursor = next;
        }
        Ok((SkillYaml::Sequence(items), cursor))
    }

    fn logical_value(&self, start: usize, initial: &str) -> Result<(String, usize), String> {
        let mut logical = initial.trim().to_string();
        let mut cursor = start + 1;
        loop {
            let state = scan_state(&logical)?;
            if state.complete() {
                return Ok((strip_comment(&logical).trim().to_string(), cursor));
            }
            let Some(next) = self.next_content(cursor) else {
                return Err(self.error(start, "unterminated quoted or flow YAML value"));
            };
            if self.lines[next].indent <= self.lines[start].indent {
                return Err(self.error(start, "unterminated quoted or flow YAML value"));
            }
            logical.push(' ');
            logical.push_str(self.content(next).trim());
            cursor = next + 1;
        }
    }

    fn parse_block_scalar(
        &self,
        header_index: usize,
        parent_indent: usize,
        header: &str,
    ) -> Result<(SkillYaml, usize), String> {
        let (folded, strip_final) = block_header(header)
            .ok_or_else(|| self.error(header_index, "unsupported block scalar header"))?;
        let mut cursor = header_index + 1;
        let mut content_indent = None;
        while cursor < self.lines.len() {
            let line = &self.lines[cursor];
            if !line.raw.trim().is_empty() {
                if line.indent <= parent_indent {
                    break;
                }
                content_indent.get_or_insert(line.indent);
                break;
            }
            cursor += 1;
        }
        let Some(content_indent) = content_indent else {
            return Ok((SkillYaml::String(String::new()), cursor));
        };

        let mut parts = Vec::new();
        cursor = header_index + 1;
        while cursor < self.lines.len() {
            let line = &self.lines[cursor];
            if !line.raw.trim().is_empty() && line.indent <= parent_indent {
                break;
            }
            if line.raw.trim().is_empty() {
                parts.push(String::new());
            } else {
                if line.indent < content_indent {
                    return Err(self.error(cursor, "inconsistent block scalar indentation"));
                }
                parts.push(line.raw[content_indent..].to_string());
            }
            cursor += 1;
        }
        let mut value = if folded {
            fold_block_lines(&parts)
        } else {
            parts.join("\n")
        };
        if !strip_final && !value.is_empty() {
            value.push('\n');
        }
        Ok((SkillYaml::String(value), cursor))
    }

    fn next_content(&self, mut index: usize) -> Option<usize> {
        while index < self.lines.len() {
            let content = self.content(index).trim();
            if !content.is_empty() && !content.starts_with('#') {
                return Some(index);
            }
            index += 1;
        }
        None
    }

    fn content(&self, index: usize) -> &'a str {
        &self.lines[index].raw[self.lines[index].indent..]
    }

    fn error(&self, index: usize, message: &str) -> String {
        format!("line {}: {message}", self.lines[index].number)
    }
}

fn sequence_item(content: &str) -> Option<&str> {
    let rest = content.strip_prefix('-')?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim_start())
}

fn split_mapping_entry(content: &str) -> Option<(&str, &str)> {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    let mut depth = 0_usize;
    for (index, ch) in content.char_indices() {
        if double && escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if double => escaped = true,
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '[' | '{' if !single && !double => depth += 1,
            ']' | '}' if !single && !double => depth = depth.saturating_sub(1),
            ':' if !single && !double && depth == 0 => {
                return Some((&content[..index], &content[index + 1..]));
            }
            _ => {}
        }
    }
    None
}

fn parse_key(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("YAML mapping key is empty".to_string());
    }
    let value = parse_inline(raw)?;
    match value {
        SkillYaml::String(key) if !key.is_empty() => Ok(key),
        SkillYaml::String(_) => Err("YAML mapping key is empty".to_string()),
        _ => Err("YAML mapping keys must be strings".to_string()),
    }
}

fn parse_inline(raw: &str) -> Result<SkillYaml, String> {
    if !matches!(raw.chars().next(), Some('[' | '{' | '\'' | '"')) {
        return parse_plain_scalar(raw);
    }
    let mut parser = FlowParser::new(raw);
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.peek().is_some() {
        return Err("unexpected trailing content in YAML value".to_string());
    }
    Ok(value)
}

struct FlowParser {
    chars: Vec<char>,
    cursor: usize,
}

impl FlowParser {
    fn new(raw: &str) -> Self {
        Self {
            chars: raw.chars().collect(),
            cursor: 0,
        }
    }

    fn parse_value(&mut self) -> Result<SkillYaml, String> {
        self.skip_ws();
        match self.peek() {
            Some('[') => self.parse_sequence(),
            Some('{') => self.parse_mapping(),
            Some('\'') => self.parse_single_quoted().map(SkillYaml::String),
            Some('"') => self.parse_double_quoted().map(SkillYaml::String),
            Some(_) => self.parse_plain(),
            None => Err("YAML value is empty".to_string()),
        }
    }

    fn parse_sequence(&mut self) -> Result<SkillYaml, String> {
        self.take('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.consume(']') {
            return Ok(SkillYaml::Sequence(items));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_ws();
            if self.consume(']') {
                break;
            }
            self.take(',')?;
            self.skip_ws();
            if self.peek() == Some(']') {
                return Err("trailing commas are unsupported in YAML sequences".to_string());
            }
        }
        Ok(SkillYaml::Sequence(items))
    }

    fn parse_mapping(&mut self) -> Result<SkillYaml, String> {
        self.take('{')?;
        let mut mapping = BTreeMap::new();
        self.skip_ws();
        if self.consume('}') {
            return Ok(SkillYaml::Mapping(mapping));
        }
        loop {
            let key = match self.peek() {
                Some('\'') => self.parse_single_quoted()?,
                Some('"') => self.parse_double_quoted()?,
                Some(_) => {
                    let start = self.cursor;
                    while !matches!(self.peek(), None | Some(':')) {
                        self.cursor += 1;
                    }
                    let raw: String = self.chars[start..self.cursor].iter().collect();
                    match parse_plain_scalar(raw.trim())? {
                        SkillYaml::String(key) => key,
                        _ => return Err("YAML mapping keys must be strings".to_string()),
                    }
                }
                None => return Err("unterminated YAML mapping".to_string()),
            };
            self.skip_ws();
            self.take(':')?;
            if mapping.contains_key(&key) {
                return Err(format!("duplicate YAML key `{key}`"));
            }
            mapping.insert(key, self.parse_value()?);
            self.skip_ws();
            if self.consume('}') {
                break;
            }
            self.take(',')?;
            self.skip_ws();
            if self.peek() == Some('}') {
                return Err("trailing commas are unsupported in YAML mappings".to_string());
            }
        }
        Ok(SkillYaml::Mapping(mapping))
    }

    fn parse_single_quoted(&mut self) -> Result<String, String> {
        self.take('\'')?;
        let mut out = String::new();
        loop {
            match self.next() {
                Some('\'') if self.peek() == Some('\'') => {
                    self.cursor += 1;
                    out.push('\'');
                }
                Some('\'') => return Ok(out),
                Some(ch) => out.push(ch),
                None => return Err("unterminated single-quoted YAML string".to_string()),
            }
        }
    }

    fn parse_double_quoted(&mut self) -> Result<String, String> {
        let start = self.cursor;
        self.take('"')?;
        let mut escaped = false;
        while let Some(ch) = self.next() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                let encoded: String = self.chars[start..self.cursor].iter().collect();
                return serde_json::from_str(&encoded)
                    .map_err(|err| format!("unsupported double-quoted YAML escape: {err}"));
            }
        }
        Err("unterminated double-quoted YAML string".to_string())
    }

    fn parse_plain(&mut self) -> Result<SkillYaml, String> {
        let start = self.cursor;
        while !matches!(self.peek(), None | Some(',') | Some(']') | Some('}')) {
            self.cursor += 1;
        }
        let raw: String = self.chars[start..self.cursor].iter().collect();
        parse_plain_scalar(raw.trim())
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.cursor += 1;
        }
    }

    fn take(&mut self, expected: char) -> Result<(), String> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(format!("expected `{expected}` in YAML value"))
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<char> {
        let value = self.peek()?;
        self.cursor += 1;
        Some(value)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.cursor).copied()
    }
}

fn parse_plain_scalar(raw: &str) -> Result<SkillYaml, String> {
    if raw.is_empty() {
        return Err("YAML value is empty".to_string());
    }
    if matches!(
        raw.as_bytes()[0],
        b'&' | b'*' | b'!' | b'?' | b'@' | b'`' | b'|' | b'>' | b'%'
    ) {
        return Err("unsupported YAML indicator or advanced construct".to_string());
    }
    match raw {
        "null" | "Null" | "NULL" | "~" => return Ok(SkillYaml::Null),
        "true" | "True" | "TRUE" => return Ok(SkillYaml::Bool(true)),
        "false" | "False" | "FALSE" => return Ok(SkillYaml::Bool(false)),
        _ => {}
    }
    if integer_literal(raw) {
        return raw
            .parse::<i64>()
            .map(SkillYaml::Integer)
            .map_err(|_| "YAML integer is outside the supported i64 range".to_string());
    }
    if real_literal(raw) {
        let number = raw
            .parse::<f64>()
            .map_err(|_| "invalid YAML real number".to_string())?;
        if !number.is_finite() {
            return Err("non-finite YAML numbers are unsupported".to_string());
        }
        return Ok(SkillYaml::Real(raw.to_string()));
    }
    if raw.contains(": ") || raw.ends_with(':') {
        return Err("ambiguous `:` in plain YAML scalar; quote the value".to_string());
    }
    Ok(SkillYaml::String(raw.to_string()))
}

fn integer_literal(raw: &str) -> bool {
    let digits = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn real_literal(raw: &str) -> bool {
    let value = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    let mut digits = 0_usize;
    let mut dots = 0_usize;
    let mut exponents = 0_usize;
    let mut after_exponent = false;
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'0'..=b'9' => {
                digits += 1;
                after_exponent = false;
            }
            b'.' if dots == 0 && exponents == 0 => dots += 1,
            b'e' | b'E' if exponents == 0 && digits > 0 => {
                exponents += 1;
                after_exponent = true;
            }
            b'+' | b'-' if index > 0 && after_exponent => after_exponent = false,
            _ => return false,
        }
    }
    digits > 0 && (dots == 1 || exponents == 1) && !after_exponent
}

fn strip_comment(raw: &str) -> &str {
    let mut single = false;
    let mut double = false;
    let mut escaped = false;
    let mut previous_whitespace = true;
    for (index, ch) in raw.char_indices() {
        if double && escaped {
            escaped = false;
            previous_whitespace = ch.is_whitespace();
            continue;
        }
        match ch {
            '\\' if double => escaped = true,
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '#' if !single && !double && previous_whitespace => return &raw[..index],
            _ => {}
        }
        previous_whitespace = ch.is_whitespace();
    }
    raw
}

struct ScanState {
    single: bool,
    double: bool,
    depth: usize,
}

impl ScanState {
    fn complete(&self) -> bool {
        !self.single && !self.double && self.depth == 0
    }
}

fn scan_state(raw: &str) -> Result<ScanState, String> {
    let mut state = ScanState {
        single: false,
        double: false,
        depth: 0,
    };
    let mut escaped = false;
    for ch in raw.chars() {
        if state.double && escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if state.double => escaped = true,
            '\'' if !state.double => state.single = !state.single,
            '"' if !state.single => state.double = !state.double,
            '[' | '{' if !state.single && !state.double => state.depth += 1,
            ']' | '}' if !state.single && !state.double => {
                state.depth = state
                    .depth
                    .checked_sub(1)
                    .ok_or_else(|| "unexpected closing delimiter in YAML value".to_string())?;
            }
            '#' if !state.single && !state.double => break,
            _ => {}
        }
    }
    Ok(state)
}

fn block_header(raw: &str) -> Option<(bool, bool)> {
    match raw {
        ">" | ">+" => Some((true, false)),
        ">-" => Some((true, true)),
        "|" | "|+" => Some((false, false)),
        "|-" => Some((false, true)),
        _ => None,
    }
}

fn fold_block_lines(lines: &[String]) -> String {
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            if line.is_empty() || lines[index - 1].is_empty() {
                out.push('\n');
            } else {
                out.push(' ');
            }
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
#[path = "skill_yaml_tests.rs"]
mod tests;
