use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use walkdir::WalkDir;

use super::agent_capabilities::public_agent_capabilities;
use super::{
    ExampleClassification, InventoryError, SurfaceExample, check_next_action_emitters,
    check_panel_mutations, load_surface_inventory, public_command_schema_capabilities,
    public_command_tree_capabilities, validate_public_argv,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceCheckReport {
    pub surface_count: usize,
    pub example_count: usize,
    pub command_count: usize,
    pub next_action_emitter_count: usize,
    pub panel_mutation_count: usize,
}

pub fn check_surface_inventory(repo_root: &Path) -> Result<SurfaceCheckReport, InventoryError> {
    let inventory = load_surface_inventory(repo_root)?;
    if inventory.agent_capabilities.is_empty() {
        return Err(InventoryError::new(
            "agent capability inventory must not be empty",
        ));
    }
    if repo_root.join("src/envelope.rs").is_file() {
        let expected = public_agent_capabilities()?;
        let declared = inventory
            .agent_capabilities
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if declared != expected {
            let missing = expected.difference(&declared).cloned().collect::<Vec<_>>();
            let stale = declared.difference(&expected).cloned().collect::<Vec<_>>();
            return Err(InventoryError::new(format!(
                "agent capability snapshot differs from the serialized envelope and code-owned semantics; missing={missing:?}; stale={stale:?}"
            )));
        }
    }
    validate_public_surface_coverage(repo_root, &inventory.surfaces)?;
    let next_action_emitter_count =
        check_next_action_emitters(repo_root, &inventory.next_action_emitters)?;
    let panel_mutation_count = check_panel_mutations(repo_root, &inventory.panel_mutations)?;
    let examples_by_surface = inventory.examples.iter().fold(
        BTreeMap::<&str, Vec<&SurfaceExample>>::new(),
        |mut grouped, example| {
            grouped
                .entry(example.surface.as_str())
                .or_default()
                .push(example);
            grouped
        },
    );
    let mut command_count = 0;
    let mut command_capabilities = BTreeSet::new();
    let mut validation_errors = Vec::new();
    for surface in &inventory.surfaces {
        let path = repo_root.join(&surface.path);
        let source = fs::read_to_string(&path)
            .map_err(|error| InventoryError::new(format!("{}: {error}", path.display())))?;
        let lines = source.lines().collect::<Vec<_>>();
        let examples = examples_by_surface
            .get(surface.id.as_str())
            .ok_or_else(|| {
                InventoryError::new(format!(
                    "{}: surface '{}' has no per-example inventory",
                    path.display(),
                    surface.id
                ))
            })?;
        validate_ranges(&surface.path, lines.len(), examples)?;
        for (line_number, commands) in extract_surface_commands(&lines) {
            let covering = examples
                .iter()
                .filter(|example| (example.start_line..=example.end_line).contains(&line_number))
                .copied()
                .collect::<Vec<_>>();
            if covering.len() != 1 {
                return Err(InventoryError::new(format!(
                    "{}:{line_number}: expected exactly one inventory example, found {}",
                    surface.path,
                    covering.len()
                )));
            }
            let example = covering[0];
            if example.classification == ExampleClassification::NonCommand {
                return Err(InventoryError::new(format!(
                    "{}:{line_number}: non_command example '{}' contains a loom command",
                    surface.path, example.id
                )));
            }
            if !matches!(
                example.classification,
                ExampleClassification::Executable | ExampleClassification::OutputExample
            ) {
                continue;
            }
            for command in commands {
                for argv in command_variants(&command, example.classification) {
                    match validate_public_argv(&argv) {
                        Ok(parsed) => {
                            match public_command_schema_capabilities(&parsed.command_path) {
                                Ok(capabilities) => command_capabilities.extend(capabilities),
                                Err(error) => validation_errors.push(format!(
                                    "{}:{line_number}: example '{}': {}: {:?}",
                                    surface.path, example.id, error.message, argv
                                )),
                            }
                        }
                        Err(error) => validation_errors.push(format!(
                            "{}:{line_number}: example '{}': {}: {:?}",
                            surface.path, example.id, error.message, argv
                        )),
                    }
                    command_count += 1;
                }
            }
        }
    }
    if !validation_errors.is_empty() {
        return Err(InventoryError::new(format!(
            "public command validation failed:\n{}",
            validation_errors.join("\n")
        )));
    }
    if command_count == 0 {
        return Err(InventoryError::new(
            "surface inventory produced no executable commands",
        ));
    }
    command_capabilities.extend(
        public_command_tree_capabilities().map_err(|error| InventoryError::new(error.message))?,
    );
    let declared = inventory
        .command_capabilities
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if declared != command_capabilities {
        let missing = command_capabilities
            .difference(&declared)
            .cloned()
            .collect::<Vec<_>>();
        let stale = declared
            .difference(&command_capabilities)
            .cloned()
            .collect::<Vec<_>>();
        return Err(InventoryError::new(format!(
            "command capability snapshot differs from the public Clap schema; missing={missing:?}; stale={stale:?}"
        )));
    }
    Ok(SurfaceCheckReport {
        surface_count: inventory.surfaces.len(),
        example_count: inventory.examples.len(),
        command_count,
        next_action_emitter_count,
        panel_mutation_count,
    })
}

pub(super) fn join_continuation_lines(lines: &[&str], offset: usize, first: &str) -> String {
    if !first.trim_end().ends_with('\\') {
        return first.to_string();
    }
    let mut joined = first.trim_end_matches([' ', '\\']).to_string();
    for continuation in &lines[offset + 1..] {
        joined.push(' ');
        joined.push_str(continuation.trim().trim_end_matches('\\'));
        if !continuation.trim_end().ends_with('\\') {
            break;
        }
    }
    joined
}

fn validate_public_surface_coverage(
    repo_root: &Path,
    surfaces: &[super::SurfaceSpec],
) -> Result<(), InventoryError> {
    let registered = surfaces
        .iter()
        .map(|surface| surface.path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let roots = [
        (repo_root.join("README.md"), 0),
        (repo_root.join("docs"), usize::MAX),
        (repo_root.join("skills"), usize::MAX),
        (repo_root.join(".github/workflows"), usize::MAX),
    ];
    for (root, max_depth) in roots {
        for entry in WalkDir::new(root).max_depth(max_depth) {
            let entry = entry.map_err(|error| InventoryError::new(error.to_string()))?;
            if !entry.file_type().is_file() || !is_public_surface_candidate(entry.path()) {
                continue;
            }
            let source = fs::read_to_string(entry.path()).map_err(|error| {
                InventoryError::new(format!("{}: {error}", entry.path().display()))
            })?;
            let lines = source.lines().collect::<Vec<_>>();
            if extract_surface_commands(&lines).is_empty() {
                continue;
            }
            let relative = entry.path().strip_prefix(repo_root).map_err(|error| {
                InventoryError::new(format!("{}: {error}", entry.path().display()))
            })?;
            let relative = relative.to_string_lossy().replace('\\', "/");
            if !registered.contains(relative.as_str()) {
                return Err(InventoryError::new(format!(
                    "{relative}: public command surface is absent from the review-owned inventory"
                )));
            }
        }
    }
    Ok(())
}

fn is_public_surface_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("md" | "yml" | "yaml")
    )
}

fn validate_ranges(
    path: &str,
    line_count: usize,
    examples: &[&SurfaceExample],
) -> Result<(), InventoryError> {
    for example in examples {
        if line_count > 1 && example.start_line == 1 && example.end_line == line_count {
            return Err(InventoryError::new(format!(
                "{path}: example '{}' uses prohibited whole-file classification",
                example.id
            )));
        }
        if example.end_line > line_count {
            return Err(InventoryError::new(format!(
                "{path}: example '{}' ends at line {}, but file has {line_count} lines",
                example.id, example.end_line
            )));
        }
    }
    for (index, left) in examples.iter().enumerate() {
        for right in &examples[index + 1..] {
            if left.start_line <= right.end_line && right.start_line <= left.end_line {
                return Err(InventoryError::new(format!(
                    "{path}: examples '{}' and '{}' overlap",
                    left.id, right.id
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn extract_loom_commands(line: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let trimmed = line.trim_start();
    let shell_line = trimmed.strip_prefix("$ ").unwrap_or(trimmed);
    if shell_line.starts_with("loom ") {
        push_commands(shell_line, &mut commands);
    }
    for (index, inline) in line.split('`').enumerate() {
        if index % 2 == 1 && inline.contains("loom ") {
            push_commands(inline, &mut commands);
        }
    }
    for marker in ["$(loom ", "\"loom ", "run: loom "] {
        if let Some(index) = line.find(marker) {
            push_commands(&line[index + marker.len() - 5..], &mut commands);
        }
    }
    commands.sort();
    commands.dedup();
    commands
}

fn extract_surface_commands(lines: &[&str]) -> Vec<(usize, Vec<String>)> {
    let mut commands = Vec::new();
    let mut fence: Option<(char, bool)> = None;
    for (offset, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some(marker) = fence_marker(trimmed) {
            match fence {
                Some((active, _)) if active == marker => fence = None,
                None => fence = Some((marker, is_shell_fence(trimmed, marker))),
                _ => {}
            }
            continue;
        }
        let logical_line = join_continuation_lines(lines, offset, line);
        let extracted = if fence.is_some_and(|(_, is_shell)| is_shell) {
            let mut extracted = Vec::new();
            push_commands(&logical_line, &mut extracted);
            extracted.sort();
            extracted.dedup();
            extracted
        } else {
            extract_loom_commands(&logical_line)
        };
        if !extracted.is_empty() {
            commands.push((offset + 1, extracted));
        }
    }
    commands
}

fn fence_marker(line: &str) -> Option<char> {
    if line.starts_with("```") {
        Some('`')
    } else if line.starts_with("~~~") {
        Some('~')
    } else {
        None
    }
}

fn is_shell_fence(line: &str, marker: char) -> bool {
    let language = line
        .trim_start_matches(marker)
        .trim()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        language.as_str(),
        "" | "sh" | "bash" | "shell" | "zsh" | "console"
    )
}

fn push_commands(value: &str, commands: &mut Vec<String>) {
    let mut remaining = value;
    while let Some(index) = find_command_start(remaining) {
        let candidate = &remaining[index..];
        let end = ['`', ';', '#']
            .into_iter()
            .filter_map(|delimiter| candidate.find(delimiter))
            .chain(
                [" | ", " && ", " || "]
                    .into_iter()
                    .filter_map(|delimiter| candidate.find(delimiter)),
            )
            .min()
            .unwrap_or(candidate.len());
        let command = candidate[..end]
            .trim_end_matches(|character: char| {
                character.is_whitespace() || matches!(character, ')' | ',' | '.')
            })
            .to_string();
        if command != "loom" {
            commands.push(command);
        }
        remaining = &candidate[end.min(candidate.len())..];
        if end == candidate.len() {
            break;
        }
        remaining = &remaining[1..];
    }
}

fn find_command_start(value: &str) -> Option<usize> {
    value.match_indices("loom ").find_map(|(index, _)| {
        let preceding = value[..index].chars().next_back();
        let embedded = preceding.is_some_and(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '/' | '.')
        });
        (!embedded).then_some(index)
    })
}

pub(super) fn normalize_command(command: &str) -> Vec<String> {
    normalize_command_variants(command)
        .into_iter()
        .next()
        .unwrap_or_default()
}

pub(super) fn command_variants(
    command: &str,
    classification: ExampleClassification,
) -> Vec<Vec<String>> {
    let mut variants = normalize_command_variants(command);
    if classification != ExampleClassification::OutputExample {
        return variants;
    }
    if variants[0].get(1).is_none_or(String::is_empty) {
        return Vec::new();
    }
    for index in 0..variants[0].len() {
        if !is_command_family_token(&variants[0][index]) {
            continue;
        }
        let choices = variants[0][index]
            .split('/')
            .map(str::to_string)
            .collect::<Vec<_>>();
        variants = variants
            .into_iter()
            .flat_map(|variant| {
                choices.iter().map(move |choice| {
                    let mut expanded = variant.clone();
                    expanded[index] = choice.clone();
                    expanded
                })
            })
            .collect();
    }
    for variant in &mut variants {
        if !variant.iter().any(|token| token == "--help") {
            variant.push("--help".to_string());
        }
    }
    variants
}

fn normalize_command_variants(command: &str) -> Vec<Vec<String>> {
    let mut variants = vec![Vec::new()];
    let mut optional = vec![Vec::new()];
    let mut in_optional = false;
    for raw in shell_tokens(command) {
        let starts_optional = raw.starts_with('[');
        let ends_optional = raw.ends_with(']');
        if starts_optional {
            in_optional = true;
        }
        if in_optional && raw == "|" {
            optional.push(Vec::new());
            continue;
        }
        let token = raw.trim_matches(|character| {
            matches!(character, '"' | '\'' | '(' | ')' | ',' | '\\' | '[' | ']')
        });
        if !token.is_empty() {
            let token = normalize_token(token);
            if token.is_empty() {
                continue;
            } else if in_optional {
                match optional.last_mut() {
                    Some(tokens) => tokens.push(token),
                    None => optional.push(vec![token]),
                }
            } else {
                for variant in &mut variants {
                    variant.push(token.clone());
                }
            }
        }
        if ends_optional && in_optional {
            let included = variants
                .iter()
                .flat_map(|variant| {
                    optional
                        .iter()
                        .filter(|tokens| !tokens.is_empty())
                        .map(move |tokens| {
                            let mut included = variant.clone();
                            included.extend(tokens.iter().cloned());
                            included
                        })
                })
                .collect::<Vec<_>>();
            variants.extend(included);
            optional = vec![Vec::new()];
            in_optional = false;
        }
    }
    if in_optional && optional.iter().any(|tokens| !tokens.is_empty()) {
        let included = variants
            .iter()
            .flat_map(|variant| {
                optional
                    .iter()
                    .filter(|tokens| !tokens.is_empty())
                    .map(move |tokens| {
                        let mut included = variant.clone();
                        included.extend(tokens.iter().cloned());
                        included
                    })
            })
            .collect::<Vec<_>>();
        variants.extend(included);
    }
    variants.sort();
    variants.dedup();
    variants
}

fn is_command_family_token(token: &str) -> bool {
    token.contains('/')
        && token.split('/').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

fn shell_tokens(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in command.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                current.push(character);
            }
            continue;
        }
        if character.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn normalize_token(token: &str) -> String {
    if token.starts_with('$') || token.starts_with('<') {
        return placeholder_value(token);
    }
    let token = token
        .trim_end_matches(['.', ':'])
        .trim_end_matches(']')
        .to_string();
    if token.contains('|') {
        return token.split('|').next().unwrap_or_default().to_string();
    }
    token
}

fn placeholder_value(token: &str) -> String {
    let content = token
        .trim_matches(|character| matches!(character, '<' | '>' | '[' | ']' | '$' | '{' | '}'));
    let first_choice = content.split('|').next().unwrap_or(content);
    let lower = first_choice.to_ascii_lowercase();
    if content.contains('|') {
        first_choice.to_string()
    } else if lower.contains("matcher-kind") {
        "path-prefix".to_string()
    } else if lower.contains("profile") {
        "default".to_string()
    } else if lower.contains("path")
        || lower.contains("root")
        || lower.contains("workspace")
        || lower.contains("file")
        || lower.contains("artifact")
    {
        "/tmp/loom-contract-fixture".to_string()
    } else if lower.contains("agent") {
        "codex".to_string()
    } else if lower.contains("version") || lower.contains("ref") {
        "v1.0.0".to_string()
    } else if lower.contains("port") {
        "43117".to_string()
    } else if lower.contains("scope") {
        "user".to_string()
    } else if lower.contains("method") {
        "symlink".to_string()
    } else if lower.contains("ownership") {
        "managed".to_string()
    } else if lower.contains("strategy") {
        "local".to_string()
    } else if lower == "n"
        || lower == "ms"
        || lower.contains("count")
        || lower.contains("seconds")
        || lower.ends_with("-ms")
    {
        "1".to_string()
    } else if lower.contains("template") {
        "basic".to_string()
    } else if lower.contains("trust") || lower.contains("level") {
        "reviewed".to_string()
    } else if lower.contains("format") {
        "jsonl".to_string()
    } else if lower == "to" || lower.contains("command") {
        "skill".to_string()
    } else {
        "fixture".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{ExampleClassification, command_variants, extract_surface_commands};

    #[test]
    fn optional_groups_keep_included_flag_variants() {
        let variants = command_variants(
            "loom skill watch demo [--max-cycles 1]",
            ExampleClassification::Executable,
        );
        assert!(variants.contains(&vec![
            "loom".to_string(),
            "skill".to_string(),
            "watch".to_string(),
            "demo".to_string(),
            "--max-cycles".to_string(),
            "1".to_string(),
        ]));
    }

    #[test]
    fn shell_fences_scan_commands_after_assignments_and_env() {
        let lines = [
            "```bash",
            "LOOM_ROOT=/tmp/demo loom skill save demo",
            "env FOO=1 loom workspace status",
            "```",
        ];
        let commands = extract_surface_commands(&lines);
        assert_eq!(commands[0].1, ["loom skill save demo"]);
        assert_eq!(commands[1].1, ["loom workspace status"]);
    }
}
