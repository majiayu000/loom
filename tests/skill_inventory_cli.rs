mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use common::{TestDir, run_loom, write_file, write_minimal_registry_state, write_skill};

fn skill<'a>(skills: &'a [Value], skill_id: &str) -> &'a Value {
    skills
        .iter()
        .find(|skill| skill["skill_id"].as_str() == Some(skill_id))
        .unwrap_or_else(|| panic!("missing skill {skill_id}: {skills:?}"))
}

fn tree_snapshot(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    if path.exists() {
        collect_files(path, path, &mut out);
    }
    out
}

fn collect_files(root: &Path, path: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    if path.is_file() {
        let rel = path
            .strip_prefix(root)
            .expect("path under root")
            .to_string_lossy()
            .to_string();
        out.insert(rel, fs::read(path).expect("read snapshot file"));
        return;
    }
    let mut entries = fs::read_dir(path)
        .expect("read snapshot dir")
        .collect::<Result<Vec<_>, _>>()
        .expect("read snapshot entries");
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        collect_files(root, &entry.path(), out);
    }
}

fn write_demo_skills(root: &Path) {
    write_skill(
        root,
        "review-helper",
        "---\nname: review-helper\ndescription: Use when reviewing pull requests and surfacing agent workflow risks.\n---\n# Review helper\n",
    );
    write_skill(
        root,
        "test-writer",
        "---\nname: test-writer\ndescription: Use when writing focused tests for new command behavior.\n---\n# Test writer\n",
    );
}

#[test]
fn skill_list_and_show_return_inventory_envelopes() {
    let root = TestDir::new("skill-inventory-list");
    write_demo_skills(root.path());

    let (output, env) = run_loom(root.path(), &["skill", "list"]);
    assert!(output.status.success(), "skill list should pass: {env}");
    assert_eq!(env["cmd"], json!("skill.list"));
    assert_eq!(env["data"]["state_model"], json!("union"));
    assert_eq!(env["data"]["registry_available"], json!(false));

    let skills = env["data"]["skills"].as_array().expect("skills array");
    let review = skill(skills, "review-helper");
    assert_eq!(review["source_status"], json!("present"));
    assert!(
        review["entrypoint"]
            .as_str()
            .is_some_and(|path| path.ends_with("review-helper/SKILL.md")),
        "entrypoint should point at SKILL.md: {review}"
    );
    assert!(
        review["description"]
            .as_str()
            .is_some_and(|description| description.contains("reviewing pull requests")),
        "description should come from frontmatter: {review}"
    );
    assert_eq!(review["projection_summary"]["count"], json!(0));
    assert!(
        review["next_actions"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );

    let (output, env) = run_loom(root.path(), &["skill", "show", "test-writer"]);
    assert!(output.status.success(), "skill show should pass: {env}");
    assert_eq!(env["cmd"], json!("skill.show"));
    assert_eq!(env["data"]["skill"]["skill_id"], json!("test-writer"));
    assert_eq!(env["data"]["skill"]["trust"], json!("unknown"));
}

#[test]
fn skill_search_and_resolve_are_deterministic_and_transparent() {
    let root = TestDir::new("skill-inventory-search");
    write_demo_skills(root.path());
    write_minimal_registry_state(root.path(), 1);

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "search",
            "model onboarding",
            "--agent",
            "claude",
            "--profile",
            "default",
            "--status",
            "missing",
            "--trust",
            "unknown",
        ],
    );
    assert!(output.status.success(), "skill search should pass: {env}");
    assert_eq!(env["cmd"], json!("skill.search"));
    assert_eq!(env["data"]["count"], json!(1));
    assert_eq!(
        env["data"]["results"][0]["skill"]["skill_id"],
        json!("model-onboarding")
    );
    assert!(
        env["data"]["results"][0]["score_inputs"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "search must expose scoring inputs: {env}"
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "resolve",
            "model onboarding flow",
            "--agent",
            "claude",
            "--workspace",
            "/tmp/project-a/src",
        ],
    );
    assert!(output.status.success(), "skill resolve should pass: {env}");
    assert_eq!(env["cmd"], json!("skill.resolve"));
    assert_eq!(env["data"]["strategy"]["llm_invoked"], json!(false));
    assert_eq!(
        env["data"]["selected"]["skill"]["skill_id"],
        json!("model-onboarding")
    );
    assert!(
        env["data"]["selected"]["score_inputs"]
            .as_array()
            .expect("score inputs")
            .iter()
            .any(|input| input["field"] == json!("workspace_matchers")),
        "resolve must explain workspace matcher boost: {env}"
    );
}

#[test]
fn inventory_read_commands_do_not_mutate_state_or_targets() {
    let root = TestDir::new("skill-inventory-readonly");
    let live = TestDir::new("skill-inventory-live-target");
    let live_path = live.path().join("skills");
    write_demo_skills(root.path());
    write_minimal_registry_state(root.path(), 1);
    write_file(
        &live_path.join("model-onboarding/SKILL.md"),
        "---\nname: model-onboarding\ndescription: Use when inspecting live target immutability.\n---\n# Live\n",
    );
    rewrite_registry_paths(root.path(), &live_path);

    let registry_before = tree_snapshot(&root.path().join("state/registry"));
    let skills_before = tree_snapshot(&root.path().join("skills"));
    let live_before = tree_snapshot(live.path());
    let pending_before = fs::read(root.path().join("state/pending_ops.jsonl")).ok();
    let git_index = root.path().join(".git/index");

    for args in [
        vec!["skill", "list"],
        vec!["skill", "show", "review-helper"],
        vec!["skill", "search", "review", "--status", "present"],
        vec!["skill", "resolve", "review pull requests"],
    ] {
        let (output, env) = run_loom(root.path(), &args);
        assert!(output.status.success(), "{args:?} should pass: {env}");
    }

    assert_eq!(
        tree_snapshot(&root.path().join("state/registry")),
        registry_before,
        "read commands must not mutate registry state"
    );
    assert_eq!(
        tree_snapshot(&root.path().join("skills")),
        skills_before,
        "read commands must not mutate canonical skills"
    );
    assert_eq!(
        tree_snapshot(live.path()),
        live_before,
        "read commands must not mutate live targets"
    );
    assert_eq!(
        fs::read(root.path().join("state/pending_ops.jsonl")).ok(),
        pending_before,
        "read commands must not mutate pending queue"
    );
    assert!(
        !git_index.exists(),
        "read commands must not initialize or mutate a Git index"
    );
}

fn rewrite_registry_paths(root: &Path, live_path: &Path) {
    let registry = root.join("state/registry");
    for rel in ["targets.json", "projections.json"] {
        let path = registry.join(rel);
        let raw = fs::read_to_string(&path).expect("read registry file");
        let rewritten = raw.replace("/tmp/claude-a/skills", &path_string(live_path));
        fs::write(path, rewritten).expect("rewrite registry path");
    }
}

fn path_string(path: &Path) -> String {
    PathBuf::from(path).to_string_lossy().to_string()
}
