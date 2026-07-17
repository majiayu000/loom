use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

use super::{InventoryError, NextActionEmitter};

const SOURCE_ROOT: &str = "src";
const OBSERVER: &str = "observe_next_actions";

const REVIEWED_SINKS: &[(&str, &str)] = &[
    ("src/commands/mod.rs", "pub next_actions: Vec<NextAction>"),
    ("src/commands/mod.rs", "next_actions: Vec::new()"),
    (
        "src/commands/skillset_activation.rs",
        "\"next_actions\": err.next_actions.clone()",
    ),
    (
        "src/commands/provision/planner.rs",
        "next_actions: report.next_actions",
    ),
    (
        "src/commands/skillset_release.rs",
        "\"next_actions\": err.next_actions",
    ),
    (
        "src/commands/provision/model.rs",
        "pub next_actions: Vec<String>",
    ),
    (
        "src/commands/codex_visibility.rs",
        "pub(crate) next_actions: Vec<String>",
    ),
    (
        "src/commands/skill_deps.rs",
        "pub next_actions: Vec<String>",
    ),
    ("src/envelope.rs", "pub next_actions: Vec<NextAction>"),
    ("src/envelope.rs", "next_actions: Vec<NextAction>"),
    (
        "src/envelope.rs",
        "value[\"error\"][\"next_actions\"][0][\"cmd\"]",
    ),
    (
        "src/envelope.rs",
        "value[\"error\"][\"next_actions\"][0][\"reason\"]",
    ),
    ("src/envelope.rs", "value[\"error\"].get(\"next_actions\")"),
    (
        "src/panel/auth.rs",
        "error[\"next_actions\"] = json!(next_actions)",
    ),
    ("src/main_runtime.rs", "data[\"next_actions\"]"),
];

pub fn check_next_action_emitters(
    repo_root: &Path,
    emitters: &[NextActionEmitter],
) -> Result<usize, InventoryError> {
    if emitters.is_empty() {
        return Err(InventoryError::new(
            "next-action emitter inventory is empty",
        ));
    }
    let inventory_ids = emitters
        .iter()
        .map(|emitter| emitter.id.as_str())
        .collect::<BTreeSet<_>>();
    let observed = collect_observer_calls(repo_root)?;
    let observed_ids = observed.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if inventory_ids != observed_ids {
        return Err(InventoryError::new(format!(
            "next-action emitter inventory drift: expected {inventory_ids:?}, observed {observed_ids:?}"
        )));
    }
    for emitter in emitters {
        let (path, selector) = emitter.source.split_once('#').ok_or_else(|| {
            InventoryError::new(format!(
                "{}: source must use stable path#selector syntax",
                emitter.id
            ))
        })?;
        if selector.is_empty() {
            return Err(InventoryError::new(format!(
                "{}: source selector must not be empty",
                emitter.id
            )));
        }
        let source = read(repo_root, path)?;
        let marker = format!("\"{}\"", emitter.id);
        if !source.contains(&marker) {
            return Err(InventoryError::new(format!(
                "{}: source {} does not contain its observer marker",
                emitter.id, emitter.source
            )));
        }
        let actual_path = observed.get(&emitter.id).expect("sets already match");
        if actual_path != path {
            return Err(InventoryError::new(format!(
                "{}: inventory source path '{}' does not match observed path '{}'",
                emitter.id, path, actual_path
            )));
        }
    }
    validate_no_unwrapped_producers(repo_root)?;
    Ok(emitters.len())
}

fn collect_observer_calls(repo_root: &Path) -> Result<BTreeMap<String, String>, InventoryError> {
    let mut observed = BTreeMap::new();
    for path in rust_sources(repo_root)? {
        let relative = relative_path(repo_root, &path)?;
        if relative.starts_with("src/cli_contract/") || relative == "src/next_action_trace.rs" {
            continue;
        }
        let source = fs::read_to_string(&path)
            .map_err(|error| InventoryError::new(format!("{}: {error}", path.display())))?;
        let mut cursor = 0;
        while let Some(offset) = source[cursor..].find(OBSERVER) {
            let start = cursor + offset + OBSERVER.len();
            let remainder = &source[start..];
            let whitespace = remainder.len() - remainder.trim_start().len();
            let remainder = &remainder[whitespace..];
            if !remainder.starts_with('(') {
                cursor = start;
                continue;
            }
            let argument = remainder[1..].trim_start();
            let Some(argument) = argument.strip_prefix('"') else {
                return Err(InventoryError::new(format!(
                    "{relative}: next-action observer id must be a string literal"
                )));
            };
            let end = argument.find('"').ok_or_else(|| {
                InventoryError::new(format!("{relative}: unterminated next-action observer id"))
            })?;
            let id = argument[..end].to_string();
            if let Some(previous) = observed.insert(id.clone(), relative.clone()) {
                return Err(InventoryError::new(format!(
                    "duplicate next-action observer id '{id}' in {previous} and {relative}"
                )));
            }
            cursor = start + whitespace + 1 + end + 1;
        }
    }
    Ok(observed)
}

fn validate_no_unwrapped_producers(repo_root: &Path) -> Result<(), InventoryError> {
    for path in rust_sources(repo_root)? {
        let relative = relative_path(repo_root, &path)?;
        if relative.contains("/tests/")
            || relative.starts_with("src/cli_contract/")
            || relative == "src/next_action_trace.rs"
        {
            continue;
        }
        let source = fs::read_to_string(&path)
            .map_err(|error| InventoryError::new(format!("{}: {error}", path.display())))?;
        let lines = source.lines().collect::<Vec<_>>();
        for (offset, line) in lines.iter().enumerate() {
            if !is_candidate(line)
                || candidate_is_observed(&lines, offset)
                || reviewed_sink(&relative, line)
            {
                continue;
            }
            return Err(InventoryError::new(format!(
                "{relative}:{}: unwrapped next_actions producer or unreviewed sink: {}",
                offset + 1,
                line.trim()
            )));
        }
        if relative == "src/error_actions.rs" {
            for (offset, _) in source.match_indices("vec![NextAction::new(") {
                let prefix = &source[offset.saturating_sub(160)..offset];
                if !prefix.contains(OBSERVER) {
                    return Err(InventoryError::new(format!(
                        "{relative}: unwrapped default NextAction producer near byte {offset}"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn is_candidate(line: &str) -> bool {
    line.contains("\"next_actions\"")
        || line.contains(".next_actions =")
        || line.contains("next_actions:")
}

fn candidate_is_observed(lines: &[&str], offset: usize) -> bool {
    if lines[offset].contains(OBSERVER) {
        return true;
    }
    lines[offset + 1..]
        .iter()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.trim_start().starts_with(OBSERVER))
}

fn reviewed_sink(path: &str, line: &str) -> bool {
    if line.contains("next_actions: &mut Vec<String>") {
        return matches!(
            path,
            "src/commands/mcp.rs" | "src/commands/skill_deps/probes.rs"
        );
    }
    REVIEWED_SINKS
        .iter()
        .any(|(expected_path, snippet)| path == *expected_path && line.contains(snippet))
}

fn rust_sources(repo_root: &Path) -> Result<Vec<PathBuf>, InventoryError> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(repo_root.join(SOURCE_ROOT)) {
        let entry = entry.map_err(|error| InventoryError::new(error.to_string()))?;
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|value| value.to_str()) == Some("rs")
        {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn relative_path(repo_root: &Path, path: &Path) -> Result<String, InventoryError> {
    path.strip_prefix(repo_root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .map_err(|error| InventoryError::new(error.to_string()))
}

fn read(repo_root: &Path, relative: &str) -> Result<String, InventoryError> {
    let path = repo_root.join(relative);
    fs::read_to_string(&path)
        .map_err(|error| InventoryError::new(format!("{}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::candidate_is_observed;

    #[test]
    fn multiline_assignment_requires_observer_on_next_nonempty_line() {
        assert!(candidate_is_observed(
            &[
                "failure.next_actions =",
                "    observe_next_actions(\"id\", value);"
            ],
            0,
        ));
        assert!(!candidate_is_observed(
            &["failure.next_actions =", "    build_next_actions();"],
            0,
        ));
    }
}
