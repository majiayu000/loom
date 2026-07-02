use std::path::Path;
use std::process::Output;

use serde_json::Value;

use super::run_loom;

pub fn save_skill(root: &Path, skill: &str) -> (Output, Value) {
    run_loom(root, &["skill", "commit", skill, "--from-source"])
}

pub fn target_add(root: &Path, agent: &str, path: &Path, ownership: &str) -> (Output, Value) {
    let path = path.to_string_lossy().into_owned();
    run_loom(
        root,
        &[
            "target",
            "add",
            "--agent",
            agent,
            "--path",
            &path,
            "--ownership",
            ownership,
        ],
    )
}

pub fn target_add_with_default_ownership(root: &Path, agent: &str, path: &Path) -> (Output, Value) {
    let path = path.to_string_lossy().into_owned();
    run_loom(root, &["target", "add", "--agent", agent, "--path", &path])
}

pub fn binding_add(
    root: &Path,
    agent: &str,
    profile: &str,
    matcher_kind: &str,
    matcher_value: &str,
    target: &str,
) -> (Output, Value) {
    run_loom(
        root,
        &[
            "workspace",
            "binding",
            "add",
            "--agent",
            agent,
            "--profile",
            profile,
            "--matcher-kind",
            matcher_kind,
            "--matcher-value",
            matcher_value,
            "--target",
            target,
        ],
    )
}

pub fn skill_project(
    root: &Path,
    skill: &str,
    binding: &str,
    method: Option<&str>,
) -> (Output, Value) {
    match method {
        Some(method) => run_loom(
            root,
            &[
                "skill",
                "project",
                skill,
                "--binding",
                binding,
                "--method",
                method,
            ],
        ),
        None => run_loom(root, &["skill", "project", skill, "--binding", binding]),
    }
}
