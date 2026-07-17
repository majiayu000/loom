use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::{
    DependencyDeclarations, add_csv_value, dep_finding, set_network, yaml_dependency_values,
};

pub(super) fn read_agent_metadata(
    skill_path: &Path,
    agent: Option<&str>,
    declarations: &mut DependencyDeclarations,
) {
    let agents_dir = skill_path.join("agents");
    if !agents_dir.is_dir() {
        return;
    }
    let paths = match metadata_paths(&agents_dir, agent) {
        Ok(paths) => paths,
        Err(err) => {
            declarations.findings.push(dep_finding(
                "agent_metadata_directory_read_failed",
                "warning",
                "agent metadata directory could not be read",
                "fix agent metadata directory permissions",
                json!({ "path": agents_dir, "error": err.to_string() }),
            ));
            return;
        }
    };
    for path in paths.into_iter().filter(|path| path.is_file()) {
        read_agent_metadata_file(&path, declarations);
    }
}

fn metadata_paths(agents_dir: &Path, agent: Option<&str>) -> std::io::Result<Vec<PathBuf>> {
    if let Some(agent) = agent {
        return Ok(vec![
            agents_dir.join(format!("{agent}.yaml")),
            agents_dir.join(format!("{agent}.yml")),
        ]);
    }
    fs::read_dir(agents_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect()
}

fn read_agent_metadata_file(path: &Path, declarations: &mut DependencyDeclarations) {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            declarations.findings.push(dep_finding(
                "agent_metadata_read_failed",
                "warning",
                "agent dependency metadata could not be read",
                "fix agent metadata permissions or remove the unreadable file",
                json!({ "path": path, "error": err.to_string() }),
            ));
            return;
        }
    };
    declarations.sources.insert("agent metadata".to_string());
    let values = match yaml_dependency_values(&raw) {
        Ok(values) => values,
        Err(err) => {
            declarations.findings.push(dep_finding(
                "agent_metadata_yaml_invalid",
                "warning",
                "agent dependency metadata YAML did not parse",
                "fix agent metadata YAML before relying on dependency readiness",
                json!({ "path": path, "error": err }),
            ));
            return;
        }
    };
    for (key, value) in values {
        match key.as_str() {
            "requires_tools" => add_csv_value(&value, "agent metadata", &mut declarations.tools),
            "requires_mcp" => add_csv_value(&value, "agent metadata", &mut declarations.mcp),
            "requires_env" => add_csv_value(&value, "agent metadata", &mut declarations.env),
            "network" => set_network(value.trim(), "agent metadata", declarations),
            _ => {}
        }
    }
}
