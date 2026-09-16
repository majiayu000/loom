mod common;

use std::fs;

use serde_json::{Value, json};

use common::{TestDir, fake_codex_path, run_loom_with_env, write_file, write_skill};

const SCRIPT: &str = r#"#!/bin/sh
printf '%s\n' "$PWD" > "$LOOM_TEST_CALL"
case "$*" in *"--sandbox read-only"*) ;; *) exit 19 ;; esac
if [ -n "$LOOM_TEST_EXPECT_PROMPT" ]; then
  case "$*" in *"$LOOM_TEST_EXPECT_PROMPT"*) ;; *) exit 29 ;; esac
fi
if [ "$LOOM_TEST_EXIT" = 1 ]; then exit 1; fi
cat "$LOOM_TEST_ANSWER"
"#;

fn setup(answer: Value) -> (TestDir, String) {
    let root = TestDir::new("author-codex");
    write_skill(root.path(), "demo", "# Demo\n");
    write_file(
        &root.path().join("answer.jsonl"),
        &format!(
            "{}\n",
            json!({
                "type": "item.completed", "item": {"type": "agent_message", "text": answer.to_string()},
            })
        ),
    );
    let path = fake_codex_path(root.path(), SCRIPT);
    (root, path)
}

fn answer(path: &str) -> Value {
    json!({
        "patch": format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,1 +1,2 @@\n # Demo\n+Better guidance\n"),
        "files": [{"path": path, "change": "modify"}],
    })
}

fn run(root: &TestDir, path: &str, dry_run: bool, fail: bool) -> (std::process::Output, Value) {
    let input = root.path().join("answer.jsonl");
    let call = root.path().join("called");
    let mut args = vec![
        "skill",
        "author",
        "rewrite",
        "demo",
        "--instruction",
        "Improve the guide",
        "--provider",
        "codex-cli",
    ];
    if dry_run {
        args.push("--dry-run");
    }
    run_loom_with_env(
        root.path(),
        &[
            ("PATH", path),
            ("LOOM_TEST_ANSWER", input.to_str().unwrap()),
            ("LOOM_TEST_CALL", call.to_str().unwrap()),
            ("LOOM_TEST_EXIT", if fail { "1" } else { "0" }),
        ],
        &args,
    )
}

#[test]
fn real_provider_transport_creates_reviewable_patch_without_applying_it() {
    let (root, path) = setup(answer("skills/demo/SKILL.md"));
    let (output, value) = run(&root, &path, false, false);
    assert!(output.status.success(), "{value}");
    assert_eq!(value["data"]["provider"], "codex-cli");
    assert_eq!(value["data"]["artifact_written"], true);
    assert_eq!(
        fs::read_to_string(root.path().join("skills/demo/SKILL.md")).unwrap(),
        "# Demo\n"
    );
    let patch = value["data"]["patch_path"].as_str().unwrap();
    assert!(
        fs::read_to_string(patch)
            .unwrap()
            .contains("+Better guidance")
    );
    let scratch = fs::read_to_string(root.path().join("called")).unwrap();
    assert!(!std::path::Path::new(scratch.trim()).exists());
}

#[test]
fn provider_preview_never_calls_codex_or_writes_artifacts() {
    let (root, path) = setup(answer("skills/demo/SKILL.md"));
    let (output, value) = run(&root, &path, true, false);
    assert!(output.status.success(), "{value}");
    assert_eq!(value["data"]["artifact_written"], false);
    assert!(!root.path().join("called").exists());
    assert!(!root.path().join("state/patches").exists());
}

#[test]
fn malformed_or_out_of_scope_model_output_is_rejected() {
    for (answer, code) in [
        (json!({"text": "not a patch"}), "SCHEMA_MISMATCH"),
        (answer("skills/other/SKILL.md"), "POLICY_BLOCKED"),
    ] {
        let (root, path) = setup(answer);
        let (output, value) = run(&root, &path, false, false);
        assert!(!output.status.success(), "{value}");
        assert_eq!(value["error"]["code"], code);
        assert!(!root.path().join("state/patches").exists());
    }
}

#[test]
fn process_failure_does_not_create_a_patch() {
    let (root, path) = setup(answer("skills/demo/SKILL.md"));
    let (output, value) = run(&root, &path, false, true);
    assert!(!output.status.success(), "{value}");
    assert!(!root.path().join("state/patches").exists());
    let scratch = fs::read_to_string(root.path().join("called")).unwrap();
    assert!(!std::path::Path::new(scratch.trim()).exists());
}

#[cfg(unix)]
#[test]
fn model_patch_cannot_follow_a_symlink_to_another_source() {
    let (root, path) = setup(answer("skills/demo/references/other.md"));
    let outside = root.path().join("outside.md");
    write_file(&outside, "# Demo\n");
    fs::create_dir_all(root.path().join("skills/demo/references")).unwrap();
    std::os::unix::fs::symlink(
        &outside,
        root.path().join("skills/demo/references/other.md"),
    )
    .unwrap();
    let (output, value) = run(&root, &path, false, false);
    assert!(!output.status.success(), "{value}");
    assert_eq!(value["error"]["code"], "POLICY_BLOCKED");
    assert_eq!(fs::read_to_string(&outside).unwrap(), "# Demo\n");
    assert!(!root.path().join("state/patches").exists());
}

#[test]
fn provider_receives_explicit_description_and_eval_task() {
    for (action, flag, text) in [
        (
            "tune-description",
            "--description",
            "Use when reviewing an inventory",
        ),
        (
            "generate-evals",
            "--task",
            "Verify the custom inventory scenario",
        ),
    ] {
        let (root, path) = setup(answer("skills/demo/SKILL.md"));
        let input = root.path().join("answer.jsonl");
        let call = root.path().join("called");
        let (output, value) = run_loom_with_env(
            root.path(),
            &[
                ("PATH", &path),
                ("LOOM_TEST_ANSWER", input.to_str().unwrap()),
                ("LOOM_TEST_CALL", call.to_str().unwrap()),
                ("LOOM_TEST_EXPECT_PROMPT", text),
            ],
            &[
                "skill",
                "author",
                action,
                "demo",
                flag,
                text,
                "--provider",
                "codex-cli",
            ],
        );
        assert!(output.status.success(), "{value}");
    }
}
