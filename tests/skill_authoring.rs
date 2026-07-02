use std::fs;
use std::process::Command;

use serde_json::{Value, json};

mod common;

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};

fn write_eval_fixtures(root: &std::path::Path, skill: &str) {
    write_file(
        &root.join("skills").join(skill).join("evals/triggers.jsonl"),
        &format!(
            "{{\"id\":\"positive\",\"prompt\":\"Use {skill} for a focused workflow\",\"should_trigger\":true}}\n{{\"id\":\"negative\",\"prompt\":\"Summarize a neutral planning note\",\"should_trigger\":false,\"observed_trigger\":false}}\n"
        ),
    );
    write_file(
        &root.join("skills").join(skill).join("evals/tasks.jsonl"),
        &format!(
            "{{\"id\":\"{skill}-smoke\",\"prompt\":\"Run the {skill} workflow\",\"checks\":{{\"outcome_contains\":[\"task complete\"],\"commands_contains\":[\"loom skill eval\"],\"exit_code\":0,\"max_tokens\":200,\"max_commands\":3}}}}\n"
        ),
    );
}

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
fn apply_patch_validates_commits_and_replays_by_idempotency_key() {
    let root = TestDir::new("authoring-apply");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n",
    );
    write_eval_fixtures(root.path(), "demo");

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "rewrite",
            "demo",
            "--instruction",
            "tighten trigger precision",
        ],
    );
    assert!(output.status.success(), "rewrite should pass: {generated}");
    let patch_id = generated["data"]["patch_id"].as_str().expect("patch id");

    let (output, missing_key) = run_loom(root.path(), &["skill", "apply-patch", patch_id]);
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

    let (output, applied) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-apply-secret",
        ],
    );
    assert!(output.status.success(), "apply should pass: {applied}");
    assert_eq!(applied["data"]["applied"], json!(true));
    assert_eq!(applied["data"]["replayed"], json!(false));
    assert!(
        applied["data"]["commit"]
            .as_str()
            .is_some_and(|commit| commit.len() == 40),
        "commit missing: {applied}"
    );
    assert_eq!(
        applied["data"]["validation"]["lint"]["status"],
        json!("passed")
    );
    assert_eq!(
        applied["data"]["validation"]["safety"]["status"],
        json!("passed")
    );
    assert_eq!(
        applied["data"]["validation"]["eval"]["status"],
        json!("passed")
    );
    assert!(
        fs::read_to_string(root.path().join("skills/demo/SKILL.md"))
            .expect("read skill")
            .contains("Requested rewrite: tighten trigger precision"),
        "apply should mutate source after gates pass"
    );

    let (output, replayed) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-apply-secret",
        ],
    );
    assert!(output.status.success(), "replay should pass: {replayed}");
    assert_eq!(replayed["data"]["replayed"], json!(true));
    assert_eq!(replayed["data"]["commit"], applied["data"]["commit"]);
    assert!(
        !applied.to_string().contains("req-apply-secret"),
        "raw idempotency key leaked: {applied}"
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

    fs::remove_file(
        root.path()
            .join("state/patches")
            .join(format!("{patch_id}.json")),
    )
    .expect("remove artifact json");
    fs::remove_file(
        root.path()
            .join("state/patches")
            .join(format!("{patch_id}.patch")),
    )
    .expect("remove artifact patch");
    let (output, replayed_without_artifacts) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-apply-secret",
        ],
    );
    assert!(
        output.status.success(),
        "replay after cleanup should pass: {replayed_without_artifacts}"
    );
    assert_eq!(replayed_without_artifacts["data"]["replayed"], json!(true));
}

#[test]
fn apply_patch_rejects_source_digest_drift_without_mutation() {
    let root = TestDir::new("authoring-apply-drift");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n",
    );
    write_eval_fixtures(root.path(), "demo");

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "rewrite",
            "demo",
            "--instruction",
            "tighten trigger precision",
        ],
    );
    assert!(output.status.success(), "rewrite should pass: {generated}");
    let patch_id = generated["data"]["patch_id"].as_str().expect("patch id");
    write_file(
        &root.path().join("skills/demo/notes.md"),
        "local unreviewed drift\n",
    );

    let (output, drift) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-drift",
        ],
    );
    assert!(!output.status.success(), "drift should fail: {drift}");
    assert_eq!(drift["error"]["code"], json!("CAPTURE_CONFLICT"));
    assert!(
        !fs::read_to_string(root.path().join("skills/demo/SKILL.md"))
            .expect("read skill")
            .contains("Requested rewrite"),
        "drift failure must not apply patch"
    );
}

#[test]
fn apply_patch_applies_contextual_hunks_without_truncating_source() {
    let root = TestDir::new("authoring-apply-context");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n\nAlpha\nBeta\nGamma\n",
    );

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "rewrite",
            "demo",
            "--instruction",
            "tighten trigger precision",
        ],
    );
    assert!(output.status.success(), "rewrite should pass: {generated}");
    let patch_id = generated["data"]["patch_id"].as_str().expect("patch id");
    write_file(
        &root
            .path()
            .join("state/patches")
            .join(format!("{patch_id}.patch")),
        "diff --git a/skills/demo/SKILL.md b/skills/demo/SKILL.md\n--- a/skills/demo/SKILL.md\n+++ b/skills/demo/SKILL.md\n@@ -7,3 +7,3 @@\n Alpha\n-Beta\n+Better beta\n Gamma\n",
    );

    let (output, applied) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-context",
        ],
    );
    assert!(
        output.status.success(),
        "context apply should pass: {applied}"
    );
    let source = fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("read skill");
    assert!(source.starts_with("---\nname: demo\n"));
    assert!(source.contains("Better beta\nGamma\n"));
    assert!(source.contains("# Demo\n\nAlpha\n"));
}

#[test]
fn apply_patch_preserves_change_semantics_and_commits_only_reviewed_files() {
    let root = TestDir::new("authoring-apply-reviewed-files");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n",
    );
    write_file(&root.path().join("skills/demo/notes.md"), "local note\n");

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "rewrite",
            "demo",
            "--instruction",
            "tighten trigger precision",
        ],
    );
    assert!(output.status.success(), "rewrite should pass: {generated}");
    let patch_id = generated["data"]["patch_id"].as_str().expect("patch id");

    let (output, applied) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-reviewed-files",
        ],
    );
    assert!(output.status.success(), "apply should pass: {applied}");
    let commit = applied["data"]["commit"].as_str().expect("commit");
    let show = Command::new("git")
        .current_dir(root.path())
        .args(["show", "--name-only", "--format=", commit])
        .output()
        .expect("git show");
    assert!(show.status.success(), "git show failed");
    let files = String::from_utf8_lossy(&show.stdout);
    assert!(
        files.contains("skills/demo/SKILL.md"),
        "commit files: {files}"
    );
    assert!(
        !files.contains("skills/demo/notes.md"),
        "commit files: {files}"
    );
}

#[test]
fn apply_patch_rejects_add_over_existing_file_without_mutation() {
    let root = TestDir::new("authoring-apply-add-existing");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n",
    );
    let before = fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("read before");

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "rewrite",
            "demo",
            "--instruction",
            "tighten trigger precision",
        ],
    );
    assert!(output.status.success(), "rewrite should pass: {generated}");
    let patch_id = generated["data"]["patch_id"].as_str().expect("patch id");
    let artifact_path = root
        .path()
        .join("state/patches")
        .join(format!("{patch_id}.json"));
    let mut artifact: Value =
        serde_json::from_str(&fs::read_to_string(&artifact_path).expect("read artifact"))
            .expect("parse artifact");
    artifact["files"][0]["change"] = json!("add");
    write_file(
        &artifact_path,
        &(serde_json::to_string_pretty(&artifact).expect("artifact json") + "\n"),
    );

    let (output, blocked) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-add-existing",
        ],
    );
    assert!(!output.status.success(), "add over existing should fail");
    assert_eq!(blocked["error"]["code"], json!("SCHEMA_MISMATCH"));
    assert_eq!(
        fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("read after"),
        before,
        "failed add patch must not mutate source"
    );
}

#[test]
fn apply_patch_allows_description_update_with_existing_script_finding() {
    let root = TestDir::new("authoring-apply-existing-script");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/scripts/helper.sh"),
        "#!/bin/sh\necho reviewed helper\n",
    );

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "tune-description",
            "demo",
            "--description",
            "Use when agents need demo workflow checks for focused local tasks with clearer routing.",
        ],
    );
    assert!(
        output.status.success(),
        "tune-description should pass: {generated}"
    );
    let patch_id = generated["data"]["patch_id"].as_str().expect("patch id");

    let (output, applied) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-existing-script",
        ],
    );
    assert!(
        output.status.success(),
        "existing script finding should not block unchanged description patch: {applied}"
    );
    assert_eq!(
        applied["data"]["validation"]["safety"]["new_blocking_findings"],
        json!(0)
    );
}

#[cfg(unix)]
#[test]
fn apply_patch_resets_index_when_commit_hook_rejects_commit() {
    let root = TestDir::new("authoring-apply-hook-reset");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n",
    );
    let (output, saved) = run_loom(root.path(), &["skill", "save", "demo"]);
    assert!(output.status.success(), "seed save should pass: {saved}");

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "rewrite",
            "demo",
            "--instruction",
            "tighten trigger precision",
        ],
    );
    assert!(output.status.success(), "rewrite should pass: {generated}");
    let patch_id = generated["data"]["patch_id"].as_str().expect("patch id");

    let hook = root.path().join(".git/hooks/pre-commit");
    fs::create_dir_all(hook.parent().expect("hook parent")).expect("create hooks dir");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").expect("write pre-commit hook");
    #[allow(clippy::permissions_set_readonly_false)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&hook).expect("hook metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook, perms).expect("set hook executable");
    }

    let (output, blocked) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-hook-reset",
        ],
    );
    assert!(!output.status.success(), "commit hook should fail");
    assert_eq!(blocked["error"]["code"], json!("GIT_ERROR"));
    let source = fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("read skill");
    assert!(
        !source.contains("Requested rewrite"),
        "commit failure must restore worktree"
    );
    let diff = Command::new("git")
        .current_dir(root.path())
        .args([
            "diff",
            "--cached",
            "--name-only",
            "--",
            "skills/demo/SKILL.md",
        ])
        .output()
        .expect("git diff cached");
    assert!(
        diff.status.success(),
        "git diff cached failed: {}",
        String::from_utf8_lossy(&diff.stderr)
    );
    assert!(
        String::from_utf8_lossy(&diff.stdout).trim().is_empty(),
        "commit failure must reset staged reviewed files"
    );
}

#[test]
fn apply_patch_blocks_high_risk_generated_scripts_without_mutation() {
    let root = TestDir::new("authoring-apply-risky");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n",
    );
    write_eval_fixtures(root.path(), "demo");

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "rewrite",
            "demo",
            "--instruction",
            "tighten trigger precision",
        ],
    );
    assert!(output.status.success(), "rewrite should pass: {generated}");
    let patch_id = generated["data"]["patch_id"].as_str().expect("patch id");
    let artifact_path = root
        .path()
        .join("state/patches")
        .join(format!("{patch_id}.json"));
    let patch_path = root
        .path()
        .join("state/patches")
        .join(format!("{patch_id}.patch"));
    let mut artifact: Value =
        serde_json::from_str(&fs::read_to_string(&artifact_path).expect("read artifact"))
            .expect("parse artifact");
    artifact["files"] = json!([{"path":"skills/demo/scripts/install.sh","change":"add"}]);
    write_file(
        &artifact_path,
        &(serde_json::to_string_pretty(&artifact).expect("artifact json") + "\n"),
    );
    write_file(
        &patch_path,
        "diff --git a/skills/demo/scripts/install.sh b/skills/demo/scripts/install.sh\nnew file mode 100644\n--- /dev/null\n+++ b/skills/demo/scripts/install.sh\n@@ -0,0 +1 @@\n+rm -rf /tmp/loom-risky\n",
    );

    let (output, blocked) = run_loom(
        root.path(),
        &[
            "skill",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-risky",
        ],
    );
    assert!(!output.status.success(), "risky patch should fail");
    assert_eq!(blocked["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(
        !root.path().join("skills/demo/scripts/install.sh").exists(),
        "blocked patch must not materialize risky script"
    );
}
