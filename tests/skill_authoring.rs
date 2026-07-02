use std::fs;

use serde_json::json;

mod common;

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};

#[test]
fn draft_writes_redacted_patch_artifact_without_source_mutation() {
    let root = TestDir::new("authoring-draft");
    let session = root
        .path()
        .join("ghp_abcdefghijklmnopqrstuvwxyz1234567890-session.txt");
    write_file(
        &session,
        "Use ghp_abcdefghijklmnopqrstuvwxyz1234567890 and env-super-secret from https://user:pass@example.com/repo.git?token=abc\n",
    );

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("SECRET_TOKEN", "env-super-secret")],
        &[
            "skill",
            "draft",
            "draft-skill",
            "--from-session",
            session.to_str().expect("session path"),
        ],
    );

    assert!(output.status.success(), "draft should pass: {env}");
    assert_eq!(env["data"]["artifact_written"], json!(true));
    assert_eq!(env["data"]["provider"], json!("mock"));
    assert!(
        !env["data"]["artifact"]["prompt_material"]["sources"][0]["path"]
            .as_str()
            .expect("source path")
            .contains("ghp_abcdefghijklmnopqrstuvwxyz1234567890"),
        "source path should be redacted: {env}"
    );
    assert!(
        !root.path().join("skills/draft-skill").exists(),
        "draft must not create source files"
    );

    let patch_id = env["data"]["patch_id"].as_str().expect("patch id");
    let artifact_path = root
        .path()
        .join("state/patches")
        .join(format!("{patch_id}.json"));
    let patch_path = root
        .path()
        .join("state/patches")
        .join(format!("{patch_id}.patch"));
    assert!(artifact_path.exists(), "artifact json missing");
    assert!(patch_path.exists(), "artifact patch missing");

    let artifact = fs::read_to_string(&artifact_path).expect("read artifact");
    assert!(
        artifact.contains("<redacted>"),
        "artifact should redact secrets"
    );
    assert!(
        !artifact.contains("env-super-secret"),
        "env value leaked: {artifact}"
    );
    assert!(!artifact.contains("ghp_abcdefghijklmnopqrstuvwxyz1234567890"));
    assert!(!artifact.contains("user:pass@example.com"));

    let patch = fs::read_to_string(&patch_path).expect("read patch");
    assert!(patch.contains("new file mode 100644"));
    assert!(patch.contains("+++ b/skills/draft-skill/SKILL.md"));
    assert!(patch.contains(
        "description: \"Use when agents need the draft-skill workflow from reviewed prompt material.\""
    ));
}

#[test]
fn rewrite_writes_reviewable_patch_without_mutating_skill() {
    let root = TestDir::new("authoring-rewrite");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo skill.\n---\n# Demo\n",
    );
    let before =
        fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("read source before");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "rewrite",
            "demo",
            "--instruction",
            "tighten trigger precision",
        ],
    );

    assert!(output.status.success(), "rewrite should pass: {env}");
    assert_eq!(env["data"]["action"], json!("rewrite"));
    assert_eq!(
        fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("read source after"),
        before,
        "rewrite must not mutate source"
    );
    assert!(
        env["data"]["artifact"]["validation_plan"]
            .as_array()
            .is_some_and(|items| items.len() >= 3),
        "validation plan missing: {env}"
    );
    assert!(
        env["data"]["artifact"]["risk_notes"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "risk notes missing: {env}"
    );

    let patch = env["data"]["patch"].as_str().expect("patch");
    assert!(patch.contains("Requested rewrite: tighten trigger precision"));
}

#[test]
fn tune_description_and_generate_evals_emit_reviewable_diffs() {
    let root = TestDir::new("authoring-evals");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Old description.\n---\n# Demo\n",
    );
    let original_triggers =
        "{\"id\":\"existing-negative\",\"prompt\":\"read a memo\",\"should_trigger\":false}\n";
    write_file(
        &root.path().join("skills/demo/evals/triggers.jsonl"),
        original_triggers,
    );
    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        "{\"id\":\"existing-task\",\"prompt\":\"Run existing task\",\"checks\":{\"exit_code\":0}}\n",
    );

    let (output, tuned) = run_loom(
        root.path(),
        &[
            "skill",
            "tune-description",
            "demo",
            "--description",
            "Use when testing safer trigger routing.",
        ],
    );
    assert!(
        output.status.success(),
        "tune-description should pass: {tuned}"
    );
    let patch = tuned["data"]["patch"].as_str().expect("tune patch");
    assert!(patch.contains("+description: \"Use when testing safer trigger routing.\""));
    assert!(patch.contains("skills/demo/evals/triggers.jsonl"));
    assert!(patch.contains("demo-tuned-positive"));
    assert!(patch.contains("demo-tuned-negative"));

    let (output, evals) = run_loom(
        root.path(),
        &[
            "skill",
            "generate-evals",
            "demo",
            "--task",
            "Use demo for a focused review task",
        ],
    );
    assert!(
        output.status.success(),
        "generate-evals should pass: {evals}"
    );
    let patch = evals["data"]["patch"].as_str().expect("eval patch");
    assert!(patch.contains("skills/demo/evals/triggers.jsonl"));
    assert!(patch.contains("\"should_trigger\":false"));
    assert!(
        !patch.contains("without demo"),
        "negative trigger should not self-trigger: {patch}"
    );
    assert!(patch.contains("--- a/skills/demo/evals/triggers.jsonl"));
    assert!(patch.contains("--- a/skills/demo/evals/tasks.jsonl"));
    assert!(patch.contains("\"checks\""));
    assert!(patch.contains("\"commands_contains\""));
    assert!(!patch.contains("\"expected\":\"review the generated skill behavior\""));
    assert_eq!(
        fs::read_to_string(root.path().join("skills/demo/evals/triggers.jsonl"))
            .expect("read triggers"),
        original_triggers,
        "generate-evals must not mutate source eval files"
    );
}

#[test]
fn tune_description_inserts_missing_frontmatter_key() {
    let root = TestDir::new("authoring-description-insert");
    write_skill(root.path(), "demo", "---\nname: demo\n---\n# Demo\n");

    let (output, tuned) = run_loom(
        root.path(),
        &[
            "skill",
            "tune-description",
            "demo",
            "--description",
            "Use when testing safer trigger routing.",
        ],
    );

    assert!(
        output.status.success(),
        "tune-description should pass: {tuned}"
    );
    let patch = tuned["data"]["patch"].as_str().expect("tune patch");
    assert!(patch.contains("+description: \"Use when testing safer trigger routing.\""));
    assert!(
        !patch.starts_with("+description:"),
        "description must be inserted inside frontmatter: {patch}"
    );
}

#[test]
fn apply_patch_requires_key_and_returns_deferred_gate() {
    let root = TestDir::new("authoring-apply");
    write_file(
        &root.path().join("state/patches/skillpatch_test.json"),
        "{\"schema_version\":1,\"patch_id\":\"skillpatch_test\"}\n",
    );

    let (output, missing_key) = run_loom(root.path(), &["skill", "apply-patch", "skillpatch_test"]);
    assert!(
        !output.status.success(),
        "missing idempotency key should fail"
    );
    assert_eq!(missing_key["error"]["code"], json!("ARG_INVALID"));
    assert!(
        missing_key["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("--idempotency-key")),
        "parse error should name idempotency key: {missing_key}"
    );

    let (output, deferred) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            "skillpatch_test",
            "--idempotency-key",
            "req-apply-secret",
        ],
    );
    assert!(!output.status.success(), "apply is deferred in this slice");
    assert_eq!(deferred["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(deferred["error"]["details"]["deferred"], json!(true));
    assert!(
        deferred["error"]["details"]["idempotency_key_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:")),
        "idempotency key digest missing: {deferred}"
    );
    assert!(
        !deferred.to_string().contains("req-apply-secret"),
        "raw idempotency key leaked: {deferred}"
    );
    let audit = fs::read_to_string(root.path().join("state/events/commands.jsonl"))
        .expect("read command audit");
    assert!(
        !audit.contains("req-apply-secret"),
        "raw idempotency key leaked to audit: {audit}"
    );
    assert!(
        audit.contains("<redacted>"),
        "audit should redact idempotency key: {audit}"
    );
}
