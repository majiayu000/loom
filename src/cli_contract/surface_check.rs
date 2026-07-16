use std::{collections::BTreeMap, fs, path::Path};

use walkdir::WalkDir;

use super::{
    ExampleClassification, InventoryError, SurfaceExample, check_panel_mutations,
    load_surface_inventory, validate_public_argv,
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
    validate_public_surface_coverage(repo_root, &inventory.surfaces)?;
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
        for (offset, line) in lines.iter().enumerate() {
            let line_number = offset + 1;
            let logical_line = join_continuation_lines(&lines, offset, line);
            let commands = extract_loom_commands(&logical_line);
            if commands.is_empty() {
                continue;
            }
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
            if example.classification != ExampleClassification::Executable {
                continue;
            }
            for command in commands {
                let argv = normalize_command(&command);
                if let Err(error) = validate_public_argv(&argv) {
                    validation_errors.push(format!(
                        "{}:{line_number}: example '{}': {}: {:?}",
                        surface.path, example.id, error.message, argv
                    ));
                }
                command_count += 1;
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
    Ok(SurfaceCheckReport {
        surface_count: inventory.surfaces.len(),
        example_count: inventory.examples.len(),
        command_count,
        next_action_emitter_count: inventory.next_action_emitters.len(),
        panel_mutation_count,
    })
}

fn join_continuation_lines(lines: &[&str], offset: usize, first: &str) -> String {
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
        (repo_root.join("docs"), 1),
        (repo_root.join("skills"), 2),
        (repo_root.join(".github/workflows"), 1),
    ];
    for (root, max_depth) in roots {
        for entry in WalkDir::new(root)
            .max_depth(max_depth)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() || !is_public_surface_candidate(entry.path()) {
                continue;
            }
            let source = fs::read_to_string(entry.path()).map_err(|error| {
                InventoryError::new(format!("{}: {error}", entry.path().display()))
            })?;
            if !source
                .lines()
                .any(|line| !extract_loom_commands(line).is_empty())
            {
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

fn extract_loom_commands(line: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let trimmed = line.trim_start();
    if trimmed.starts_with("loom ") {
        push_commands(trimmed, &mut commands);
    }
    for (index, inline) in line.split('`').enumerate() {
        if index % 2 == 1 && inline.contains("loom ") {
            push_commands(inline, &mut commands);
        }
    }
    for marker in ["$(loom ", "\"loom "] {
        if let Some(index) = line.find(marker) {
            push_commands(&line[index + marker.len() - 5..], &mut commands);
        }
    }
    commands.sort();
    commands.dedup();
    commands
}

fn push_commands(value: &str, commands: &mut Vec<String>) {
    let mut remaining = value;
    while let Some(index) = remaining.find("loom ") {
        let candidate = &remaining[index..];
        let end = ['`', ';', '#']
            .into_iter()
            .filter_map(|delimiter| candidate.find(delimiter))
            .chain(candidate.find(" | "))
            .min()
            .unwrap_or(candidate.len());
        let command = candidate[..end]
            .trim_end_matches(|character: char| {
                character.is_whitespace() || matches!(character, ')' | ',' | '.')
            })
            .to_string();
        if !command.is_empty() {
            commands.push(command);
        }
        remaining = &candidate[end.min(candidate.len())..];
        if end == candidate.len() {
            break;
        }
        remaining = &remaining[1..];
    }
}

fn normalize_command(command: &str) -> Vec<String> {
    let mut argv = Vec::new();
    let mut optional_group = false;
    for raw in shell_tokens(command) {
        if raw.starts_with('[') {
            optional_group = true;
        }
        if !optional_group {
            let token = raw
                .trim_matches(|character| matches!(character, '"' | '\'' | '(' | ')' | ',' | '\\'));
            if !token.is_empty() {
                argv.push(normalize_token(token));
            }
        }
        if raw.ends_with(']') {
            optional_group = false;
        }
    }
    argv
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
    } else if lower == "to" {
        "skill".to_string()
    } else if lower.contains("command") {
        "skill".to_string()
    } else {
        "fixture".to_string()
    }
}
