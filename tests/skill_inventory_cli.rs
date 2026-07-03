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

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "test-writer", "--brief"]);
    assert!(
        output.status.success(),
        "skill inspect --brief should pass: {env}"
    );
    assert_eq!(env["cmd"], json!("skill.inspect"));
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

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "recommend",
            "model onboarding flow",
            "--agent",
            "claude",
            "--binding",
            "bind_claude_project_a",
            "--policy-profile",
            "safe-capture",
            "--workspace",
            "/tmp/project-a/src",
        ],
    );
    assert!(
        output.status.success(),
        "skill recommend should pass: {env}"
    );
    assert_eq!(env["cmd"], json!("skill.recommend"));
    assert!(
        env["data"].get("selected").is_none(),
        "recommend must not emit resolve-only selected fields: {env}"
    );
    assert_eq!(
        env["data"]["policy_context"]["binding_id"],
        json!("bind_claude_project_a")
    );
    assert_eq!(
        env["data"]["policy_context"]["policy_profile"],
        json!("safe-capture")
    );
    assert_eq!(
        env["data"]["recommendations"]["filters"]["binding"],
        json!("bind_claude_project_a")
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "recommend",
            "model onboarding",
            "--binding",
            "missing",
        ],
    );
    assert!(
        !output.status.success(),
        "unknown binding should fail closed: {env}"
    );
    assert_eq!(env["error"]["code"], json!("BINDING_NOT_FOUND"));
}

#[test]
fn index_build_and_status_are_local_and_deterministic() {
    let root = TestDir::new("skill-index-build");
    write_demo_skills(root.path());

    let (output, env) = run_loom(root.path(), &["index", "build", "--no-embeddings"]);
    assert!(output.status.success(), "index build should pass: {env}");
    assert_eq!(env["cmd"], json!("index.build"));
    assert_eq!(env["data"]["network_required"], json!(false));
    assert_eq!(env["data"]["counts"]["skills"], json!(2));
    assert!(
        root.path()
            .join("state/index/skills.lexical.json")
            .is_file()
    );
    assert!(
        root.path()
            .join("state/index/skills.capabilities.json")
            .is_file()
    );
    assert!(root.path().join("state/index/workspaces.json").is_file());

    let first = fs::read_to_string(root.path().join("state/index/skills.lexical.json"))
        .expect("read first lexical index");
    let (output, env) = run_loom(root.path(), &["index", "build", "--no-embeddings"]);
    assert!(
        output.status.success(),
        "second index build should pass: {env}"
    );
    let second = fs::read_to_string(root.path().join("state/index/skills.lexical.json"))
        .expect("read second lexical index");
    assert_eq!(first, second, "lexical-only index should be deterministic");

    let (output, env) = run_loom(root.path(), &["index", "status"]);
    assert!(output.status.success(), "index status should pass: {env}");
    assert_eq!(env["cmd"], json!("index.status"));
    assert_eq!(env["data"]["ready"], json!(true));
    assert_eq!(env["data"]["files"]["lexical"]["records"], json!(2));
}

#[test]
fn skill_recommend_and_resolve_semantic_fall_back_to_lexical() {
    let root = TestDir::new("skill-recommend-semantic");
    write_demo_skills(root.path());

    let (output, env) = run_loom(
        root.path(),
        &["skill", "recommend", "review pull request", "--semantic"],
    );
    assert!(
        output.status.success(),
        "skill recommend should pass: {env}"
    );
    assert_eq!(env["cmd"], json!("skill.recommend"));
    assert!(
        env["data"].get("selected").is_none(),
        "recommend must not emit resolve-only selected fields: {env}"
    );
    assert_eq!(env["data"]["mode"], json!("semantic-disabled"));
    assert!(
        env["meta"]["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|value| value.contains("semantic provider not configured"))),
        "search explain should warn about semantic fallback: {env}"
    );
    assert_eq!(
        env["data"]["recommendations"]["results"][0]["kind"],
        json!("skill")
    );
    assert_eq!(
        env["data"]["recommendations"]["results"][0]["id"],
        json!("review-helper")
    );
    assert_eq!(
        env["data"]["recommendations"]["results"][0]["warnings"][0],
        json!("no trust metadata recorded")
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "resolve", "review pull request", "--semantic"],
    );
    assert!(output.status.success(), "skill resolve should pass: {env}");
    assert_eq!(env["cmd"], json!("skill.resolve"));
    assert_eq!(env["data"]["strategy"]["mode"], json!("semantic-disabled"));
    assert!(
        env["meta"]["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|value| value.contains("semantic provider not configured"))),
        "task search should warn about semantic fallback: {env}"
    );
}

#[test]
fn skill_recommend_filters_quarantined_activation_candidates() {
    let root = TestDir::new("skill-recommend-quarantine");
    write_skill(
        root.path(),
        "risky-review",
        "---\nname: risky-review\ndescription: Use when reviewing risky pull requests.\n---\n# Risky\n",
    );
    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[{"skill_id":"risky-review","trust":"local-draft","quarantined":true,"reason":"blocked","updated_at":"2026-06-30T00:00:00Z","updated_by":"test"}]}
"#,
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "search", "risky review", "--explain"],
    );
    assert!(
        output.status.success(),
        "skill search --explain should pass: {env}"
    );
    assert_eq!(env["data"]["recommendations"]["count"], json!(0));
    assert!(
        env["data"]["recommendations"]["results"]
            .as_array()
            .expect("results")
            .iter()
            .all(|result| result["id"] != json!("risky-review")),
        "quarantined skill must not be recommended: {env}"
    );
}

#[test]
fn skill_recommend_includes_read_only_skillset_candidates() {
    let root = TestDir::new("skill-recommend-skillset");
    write_demo_skills(root.path());

    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "create",
            "coding-flow",
            "--description",
            "Coding flow for focused tests and review",
        ],
    );
    assert!(
        output.status.success(),
        "skillset create should pass: {env}"
    );
    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "add",
            "coding-flow",
            "test-writer",
            "--role",
            "testing",
        ],
    );
    assert!(output.status.success(), "skillset add should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "search",
            "focused tests",
            "--explain",
            "--agent",
            "claude",
        ],
    );
    assert!(
        output.status.success(),
        "skill search --explain should pass: {env}"
    );
    let results = env["data"]["recommendations"]["results"]
        .as_array()
        .expect("results");
    let skillset = results
        .iter()
        .find(|result| result["kind"] == json!("skillset") && result["id"] == json!("coding-flow"))
        .unwrap_or_else(|| panic!("missing skillset recommendation: {env}"));
    assert_eq!(skillset["recommended_action"], json!("activate"));
    assert!(
        skillset["suggested_commands"]
            .as_array()
            .expect("commands")
            .iter()
            .all(|command| !command
                .as_str()
                .unwrap_or_default()
                .contains("skill activate coding-flow")),
        "skillset must not emit invalid direct skill activation command: {skillset}"
    );
}

#[test]
fn skill_recommend_degrades_unsafe_skillset_to_inspection() {
    let root = TestDir::new("skill-recommend-unsafe-skillset");
    write_demo_skills(root.path());
    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[{"skill_id":"review-helper","trust":"local-draft","quarantined":true,"reason":"blocked","updated_at":"2026-06-30T00:00:00Z","updated_by":"test"}]}
"#,
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "create",
            "review-pack",
            "--description",
            "Review pull request workflow",
        ],
    );
    assert!(
        output.status.success(),
        "skillset create should pass: {env}"
    );
    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "add",
            "review-pack",
            "review-helper",
            "--role",
            "review",
        ],
    );
    assert!(output.status.success(), "skillset add should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "search",
            "review pull request",
            "--explain",
            "--agent",
            "claude",
        ],
    );
    assert!(
        output.status.success(),
        "skill search --explain should pass: {env}"
    );
    let skillset = env["data"]["recommendations"]["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|result| result["kind"] == json!("skillset") && result["id"] == json!("review-pack"))
        .unwrap_or_else(|| panic!("missing unsafe skillset recommendation: {env}"));
    assert_eq!(skillset["recommended_action"], json!("inspect"));
    assert!(
        skillset["risks"]
            .as_array()
            .expect("risks")
            .iter()
            .any(|risk| risk.as_str().is_some_and(
                |value| value.contains("review-helper") && value.contains("quarantined")
            )),
        "unsafe member should be listed as a risk: {skillset}"
    );
    assert_eq!(
        skillset["suggested_commands"],
        json!(["loom --json skillset show review-pack"])
    );
}

#[test]
fn active_recommend_returns_dry_run_plan_without_mutation() {
    let root = TestDir::new("active-recommend-readonly");
    write_demo_skills(root.path());
    let registry_before = tree_snapshot(&root.path().join("state/registry"));
    let skills_before = tree_snapshot(&root.path().join("skills"));

    let (output, env) = run_loom(
        root.path(),
        &[
            "active",
            "recommend",
            "review pull request",
            "--agent",
            "claude",
        ],
    );
    assert!(
        output.status.success(),
        "active recommend should pass: {env}"
    );
    assert_eq!(env["cmd"], json!("active.recommend"));
    assert_eq!(env["data"]["dry_run"], json!(true));
    assert_eq!(env["data"]["plan"]["remove"], json!([]));
    assert!(
        env["data"]["plan"]["add"]
            .as_array()
            .expect("add plan")
            .iter()
            .any(|item| item["skill"] == json!("review-helper")),
        "active recommend should plan matching skill activation: {env}"
    );
    assert_eq!(
        tree_snapshot(&root.path().join("state/registry")),
        registry_before
    );
    assert_eq!(tree_snapshot(&root.path().join("skills")), skills_before);
}

#[test]
fn active_recommend_blocks_unsafe_explicit_desired_skill() {
    let root = TestDir::new("active-recommend-unsafe-desired");
    write_demo_skills(root.path());
    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[{"skill_id":"review-helper","trust":"local-draft","quarantined":true,"reason":"blocked","updated_at":"2026-06-30T00:00:00Z","updated_by":"test"}]}
"#,
    );
    let registry_before = tree_snapshot(&root.path().join("state/registry"));
    let skills_before = tree_snapshot(&root.path().join("skills"));

    let (output, env) = run_loom(
        root.path(),
        &[
            "active",
            "recommend",
            "review pull request",
            "--agent",
            "claude",
            "--desired-skill",
            "review-helper",
        ],
    );
    assert!(
        output.status.success(),
        "active recommend should pass: {env}"
    );
    assert_eq!(env["data"]["plan"]["add"], json!([]));
    assert_eq!(env["data"]["policy"]["allowed"], json!(false));
    assert!(
        env["data"]["risks"]
            .as_array()
            .expect("risks")
            .iter()
            .any(|risk| risk.as_str().is_some_and(
                |value| value.contains("review-helper") && value.contains("quarantined")
            )),
        "unsafe desired skill should be listed as a risk: {env}"
    );
    assert_eq!(
        tree_snapshot(&root.path().join("state/registry")),
        registry_before
    );
    assert_eq!(tree_snapshot(&root.path().join("skills")), skills_before);
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
    let git_index = root.path().join(".git/index");

    for args in [
        vec!["skill", "list"],
        vec!["skill", "inspect", "review-helper", "--brief"],
        vec!["skill", "search", "review", "--status", "present"],
        vec!["skill", "search", "review pull requests", "--for-task"],
        vec!["skill", "recommend", "review pull requests"],
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
