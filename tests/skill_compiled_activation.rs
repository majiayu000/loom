use std::fs;
use std::path::Path;

use serde_json::{Value, json};

mod common;
use common::{TestDir, run_loom_with_env, write_file, write_skill};

fn write_compile_ready_skill(root: &Path, skill: &str) {
    write_skill(
        root,
        skill,
        &format!(
            "---\nname: {skill}\ndescription: Use when testing compiled activation projection.\n---\n# {skill}\n\nUse when testing compiled activation projection.\n\nDo not use for production claims.\n"
        ),
    );
}

fn run_with_home(root: &Path, home: &Path, args: &[&str]) -> (std::process::Output, Value) {
    let home = home.to_string_lossy().to_string();
    run_loom_with_env(root, &[("HOME", &home)], args)
}

#[test]
fn compiled_activation_materializes_valid_artifact_projection() {
    let root = TestDir::new("skill-compiled-activation");
    let home = TestDir::new("skill-compiled-activation-home");
    write_compile_ready_skill(root.path(), "demo");
    write_passing_eval(root.path(), "demo");

    let (_compile_output, compile_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "compile", "demo", "--agent", "codex"],
    );
    assert_eq!(compile_env["ok"], json!(true), "{compile_env}");
    assert_eq!(compile_env["data"]["manifest"]["status"], json!("valid"));
    assert_eq!(
        compile_env["data"]["manifest"]["eval_evidence"]["agent"],
        json!("codex")
    );
    let artifact_id = compile_env["data"]["artifact"]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_string();

    let (_verify_output, verify_env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "compile",
            "verify",
            "demo",
            "--artifact",
            &artifact_id,
        ],
    );
    assert_eq!(verify_env["ok"], json!(true), "{verify_env}");
    assert_eq!(verify_env["data"]["valid"], json!(true), "{verify_env}");

    let (dry_output, dry_env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--compiled",
            "--artifact",
            &artifact_id,
            "--dry-run",
        ],
    );
    assert!(
        dry_output.status.success(),
        "compiled dry-run should pass: {dry_env}"
    );
    assert_eq!(dry_env["data"]["dry_run"], json!(true));
    assert_eq!(dry_env["data"]["plan"]["method"], json!("materialize"));
    assert_eq!(
        dry_env["data"]["compiled"]["artifact_id"],
        json!(artifact_id)
    );
    assert!(
        !home.path().join(".agents/skills/demo").exists(),
        "compiled dry-run must not materialize the target"
    );

    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--compiled",
            "--artifact",
            &artifact_id,
        ],
    );
    assert!(
        activate_output.status.success(),
        "compiled activation should pass: {activate_env}"
    );
    assert_eq!(activate_env["data"]["noop"], json!(false));
    assert_eq!(activate_env["data"]["plan"]["method"], json!("materialize"));
    assert_eq!(
        activate_env["data"]["compiled"]["artifact_id"],
        json!(artifact_id)
    );

    let projected = home.path().join(".agents/skills/demo");
    assert!(
        projected.join("SKILL.md").is_file(),
        "compiled activation should materialize an agent SKILL.md"
    );
    assert!(
        !fs::symlink_metadata(&projected)
            .expect("projection metadata")
            .file_type()
            .is_symlink(),
        "compiled activation must materialize a directory, not a source symlink"
    );
    let skill_md = fs::read_to_string(projected.join("SKILL.md")).expect("projected skill md");
    assert!(skill_md.contains("# Compiled Activation: demo"));
    assert!(skill_md.contains(&format!("artifact_id: {artifact_id}")));
    let projection_manifest = projected.join(".loom/compiled/manifest.json");
    assert!(
        projection_manifest.is_file(),
        "compiled projection should preserve manifest metadata"
    );

    let (_inspect_output, inspect_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "inspect", "demo", "--agent", "codex"],
    );
    assert_eq!(inspect_env["ok"], json!(true), "{inspect_env}");
    assert_eq!(
        inspect_env["data"]["runtime"]["codex"]["projected_to_target"],
        json!(true)
    );
    assert_eq!(
        inspect_env["data"]["runtime"]["codex"]["materialized_path"],
        json!(projected.display().to_string())
    );
    assert_eq!(
        inspect_env["data"]["compiled"]["artifacts"][0]["artifact_id"],
        json!(artifact_id)
    );

    let (noop_output, noop_env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--compiled",
            "--artifact",
            &artifact_id,
        ],
    );
    assert!(
        noop_output.status.success(),
        "compiled activation should be idempotent: {noop_env}"
    );
    assert_eq!(noop_env["data"]["noop"], json!(true));
}

fn write_passing_eval(root: &Path, skill: &str) {
    write_file(
        &root.join("skills").join(skill).join("evals/tasks.jsonl"),
        r#"{"id":"happy-path","task":"Run the compiled activation eval","output":"Done with concise result","trace":["read SKILL.md","checked output"],"metrics":{"tokens":40,"commands":1},"checks":{"outcome_contains":["Done"],"process_contains":["SKILL.md"],"style_contains":["concise"],"max_tokens":100,"max_commands":3}}
"#,
    );
}
