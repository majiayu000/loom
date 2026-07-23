use std::{collections::BTreeSet, fs, path::Path};

use super::{InventoryError, PanelBinding, PanelMutation, validate_public_argv};

const ROUTER_PATH: &str = "src/panel/mod.rs";
const HANDLERS_ROOT: &str = "src/panel/handlers";
const FRONTEND_CLIENT_ROOT: &str = "panel/src/lib/api";

pub fn check_panel_mutations(
    repo_root: &Path,
    mutations: &[PanelMutation],
) -> Result<usize, InventoryError> {
    if mutations.is_empty() {
        return Err(InventoryError::new("panel mutation inventory is empty"));
    }
    let router_source = read(repo_root, ROUTER_PATH)?;
    let actual_routes = extract_post_routes(&router_source)?;
    let expected_routes = mutations
        .iter()
        .map(|mutation| {
            let route = mutation
                .backend_route
                .strip_prefix("POST ")
                .ok_or_else(|| {
                    InventoryError::new(format!(
                        "{}: backend_route must start with 'POST '",
                        mutation.id
                    ))
                })?;
            Ok((route.to_string(), mutation.handler.clone()))
        })
        .collect::<Result<BTreeSet<_>, InventoryError>>()?;
    if expected_routes != actual_routes {
        return Err(InventoryError::new(format!(
            "panel mutation route inventory drift: expected {expected_routes:?}, actual {actual_routes:?}"
        )));
    }

    let actual_frontend_routes = frontend_post_routes(repo_root)?;
    let expected_frontend_routes = mutations
        .iter()
        .map(|mutation| {
            normalize_route(
                mutation
                    .backend_route
                    .strip_prefix("POST ")
                    .unwrap_or(&mutation.backend_route),
            )
        })
        .collect::<BTreeSet<_>>();
    if expected_frontend_routes != actual_frontend_routes {
        return Err(InventoryError::new(format!(
            "panel frontend mutation route drift: expected {expected_frontend_routes:?}, actual {actual_frontend_routes:?}"
        )));
    }

    for mutation in mutations {
        check_handler_action(repo_root, mutation)?;
        check_action_label(repo_root, mutation)?;
        match mutation.binding {
            PanelBinding::CliEquivalent => {
                let argv = panel_fixture_argv(mutation);
                validate_public_argv(argv.iter().map(String::as_str)).map_err(|error| {
                    InventoryError::new(format!(
                        "{}: panel CLI equivalent {:?} is not public: {}",
                        mutation.id, mutation.cli_argv, error.message
                    ))
                })?;
            }
            PanelBinding::NoCliEquivalent => {}
        }
    }
    Ok(mutations.len())
}

pub(super) fn panel_fixture_argv(mutation: &PanelMutation) -> Vec<String> {
    mutation
        .cli_argv
        .iter()
        .map(|value| fixture_value(value))
        .collect()
}

fn read(repo_root: &Path, relative: &str) -> Result<String, InventoryError> {
    let path = repo_root.join(relative);
    fs::read_to_string(&path)
        .map_err(|error| InventoryError::new(format!("{}: {error}", path.display())))
}

fn extract_post_routes(source: &str) -> Result<BTreeSet<(String, String)>, InventoryError> {
    let mut routes = BTreeSet::new();
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find(".route(") {
        let start = cursor + relative + ".route".len();
        let end = balanced_end(source, start, '(', ')').ok_or_else(|| {
            InventoryError::new(format!("{ROUTER_PATH}: unterminated route registration"))
        })?;
        let call = &source[start + 1..end];
        let Some(route) = first_quoted(call) else {
            return Err(InventoryError::new(format!(
                "{ROUTER_PATH}: route registration has no literal path"
            )));
        };
        if let Some(handler) = post_handler(call) {
            routes.insert((route, handler));
        }
        cursor = end + 1;
    }
    if routes.is_empty() {
        return Err(InventoryError::new(format!(
            "{ROUTER_PATH}: no POST mutation routes found"
        )));
    }
    Ok(routes)
}

fn balanced_end(source: &str, open_index: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in source[open_index..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        if character == '"' {
            in_string = true;
        } else if character == open {
            depth += 1;
        } else if character == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(open_index + offset);
            }
        }
    }
    None
}

fn first_quoted(source: &str) -> Option<String> {
    let start = source.find('"')? + 1;
    let end = source[start..].find('"')? + start;
    Some(source[start..end].to_string())
}

fn post_handler(source: &str) -> Option<String> {
    let marker = "post(";
    let start = source.find(marker)? + marker.len();
    let identifier = source[start..]
        .chars()
        .skip_while(|character| character.is_whitespace())
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .collect::<String>();
    (!identifier.is_empty()).then_some(identifier)
}

fn extract_frontend_post_routes(source: &str) -> Result<BTreeSet<String>, InventoryError> {
    let api_source = source
        .split_once("export const ")
        .map(|(_, source)| source)
        .unwrap_or_default();
    let mut routes = BTreeSet::new();
    let mut cursor = 0;
    while let Some(relative) = api_source[cursor..].find("postJson(") {
        let start = cursor + relative + "postJson".len();
        let end = balanced_end(api_source, start, '(', ')').ok_or_else(|| {
            InventoryError::new("frontend API module: unterminated postJson call")
        })?;
        let arguments = &api_source[start + 1..end];
        let route = first_string_or_template(arguments).ok_or_else(|| {
            InventoryError::new(
                "frontend API module: postJson route must be a string or template literal",
            )
        })?;
        if route.starts_with("/api/") {
            routes.insert(normalize_route(&route));
        }
        cursor = end + 1;
    }
    Ok(routes)
}

fn frontend_post_routes(repo_root: &Path) -> Result<BTreeSet<String>, InventoryError> {
    let root = repo_root.join(FRONTEND_CLIENT_ROOT);
    let mut routes = BTreeSet::new();
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.map_err(|error| InventoryError::new(error.to_string()))?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("ts")
            || entry
                .path()
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with(".test.ts"))
        {
            continue;
        }
        let source = fs::read_to_string(entry.path())
            .map_err(|error| InventoryError::new(format!("{}: {error}", entry.path().display())))?;
        routes.extend(extract_frontend_post_routes(&source)?);
    }
    if routes.is_empty() {
        return Err(InventoryError::new(format!(
            "{FRONTEND_CLIENT_ROOT}: no frontend POST routes found"
        )));
    }
    Ok(routes)
}

fn first_string_or_template(source: &str) -> Option<String> {
    let trimmed = source.trim_start();
    let quote = trimmed.chars().next()?;
    if quote != '"' && quote != '`' {
        return None;
    }
    let end = trimmed[1..].find(quote)? + 1;
    Some(trimmed[1..end].to_string())
}

fn normalize_route(route: &str) -> String {
    let mut normalized = String::new();
    let mut cursor = 0;
    while cursor < route.len() {
        let remaining = &route[cursor..];
        let Some(relative) = remaining.find('{') else {
            normalized.push_str(remaining);
            break;
        };
        let start = cursor + relative;
        let literal_end = if start > cursor && route.as_bytes()[start - 1] == b'$' {
            start - 1
        } else {
            start
        };
        normalized.push_str(&route[cursor..literal_end]);
        let Some(end_relative) = route[start..].find('}') else {
            normalized.push_str(&route[start..]);
            break;
        };
        normalized.push_str("{}");
        cursor = start + end_relative + 1;
    }
    normalized
}

fn check_handler_action(repo_root: &Path, mutation: &PanelMutation) -> Result<(), InventoryError> {
    let handlers_root = repo_root.join(HANDLERS_ROOT);
    let declaration = format!("fn {}", mutation.handler);
    for entry in walkdir::WalkDir::new(&handlers_root) {
        let entry = entry.map_err(|error| InventoryError::new(error.to_string()))?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("rs")
        {
            continue;
        }
        let source = fs::read_to_string(entry.path())
            .map_err(|error| InventoryError::new(format!("{}: {error}", entry.path().display())))?;
        let Some(start) = source.find(&declaration) else {
            continue;
        };
        let brace = source[start..]
            .find('{')
            .map(|offset| start + offset)
            .ok_or_else(|| {
                InventoryError::new(format!("{}: missing function body", mutation.id))
            })?;
        let end = balanced_end(&source, brace, '{', '}').ok_or_else(|| {
            InventoryError::new(format!("{}: unterminated handler body", mutation.id))
        })?;
        let body = &source[brace..=end];
        let action_literal = format!("\"{}\"", mutation.action_id);
        if body.contains("ensure_mutation_authorized") && body.contains(&action_literal) {
            return Ok(());
        }
        return Err(InventoryError::new(format!(
            "{}: handler '{}' does not authorize action id '{}'",
            mutation.id, mutation.handler, mutation.action_id
        )));
    }
    Err(InventoryError::new(format!(
        "{}: handler '{}' was not found below {HANDLERS_ROOT}",
        mutation.id, mutation.handler
    )))
}

fn check_action_label(repo_root: &Path, mutation: &PanelMutation) -> Result<(), InventoryError> {
    let source = read(repo_root, &mutation.label_path)?;
    let marker = format!("\"{}\":", mutation.action_id);
    if !source.contains(&marker) {
        return Err(InventoryError::new(format!(
            "{}: action id '{}' has no stable label in {}",
            mutation.id, mutation.action_id, mutation.label_path
        )));
    }
    Ok(())
}

fn fixture_value(value: &str) -> String {
    if !value.starts_with('<') || !value.ends_with('>') {
        return value.to_string();
    }
    match value {
        "<url>" => "https://example.com/loom.git".to_string(),
        "<path>" | "<workspace>" | "<source>" => "/tmp/loom-contract".to_string(),
        "<version>" => "v1.0.0".to_string(),
        "<ref>" => "HEAD".to_string(),
        _ => "contract-fixture".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_frontend_post_routes, extract_post_routes};

    #[test]
    fn route_and_frontend_extractors_handle_multiline_calls() {
        let routes = extract_post_routes(
            r#"Router::new()
                .route("/read", get(read))
                .route(
                    "/write",
                    get(read).post(write),
                )"#,
        )
        .expect("routes");
        assert!(routes.contains(&("/write".to_string(), "write".to_string())));
        let routes = extract_frontend_post_routes(
            "export const api = {\n  skillCommit: (name: string) =>\n    postJson(`/api/v1/skills/${encodeURIComponent(name)}/commit`, {}),\n}",
        )
        .expect("frontend routes");
        assert!(routes.contains("/api/v1/skills/{}/commit"));
    }
}
