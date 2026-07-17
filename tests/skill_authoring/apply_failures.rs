use super::*;

#[cfg(unix)]
#[test]
fn apply_patch_resets_index_when_commit_hook_rejects_commit() {
    let root = TestDir::new("authoring-apply-hook-reset");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n",
    );
    let (output, saved) = run_loom(root.path(), &["skill", "commit", "demo", "--from-source"]);
    assert!(output.status.success(), "seed save should pass: {saved}");

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "author",
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
            "author",
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

#[cfg(unix)]
#[test]
fn apply_patch_reports_preimage_restore_failures_with_path_details() {
    let root = TestDir::new("authoring-apply-restore-failure");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when agents need demo workflow checks for focused local tasks.\n---\n# Demo\n",
    );
    let (output, saved) = run_loom(root.path(), &["skill", "commit", "demo", "--from-source"]);
    assert!(output.status.success(), "seed save should pass: {saved}");

    let (output, generated) = run_loom(
        root.path(),
        &[
            "skill",
            "author",
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
    fs::write(
        &hook,
        "#!/bin/sh\nrm -f skills/demo/SKILL.md\nmkdir -p skills/demo/SKILL.md\nexit 1\n",
    )
    .expect("write pre-commit hook");
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
            "author",
            "apply-patch",
            patch_id,
            "--idempotency-key",
            "req-restore-failure",
        ],
    );

    assert!(!output.status.success(), "commit hook should fail");
    assert_eq!(blocked["error"]["code"], json!("GIT_ERROR"));
    let rollback_errors = blocked["error"]["details"]["rollback_errors"]
        .as_array()
        .expect("rollback_errors");
    assert!(
        rollback_errors.iter().any(|error| {
            error["path"] == json!("skills/demo/SKILL.md")
                && error["action"] == json!("restore_preimage")
                && error["operation"] == json!("write_atomic")
        }),
        "rollback errors should identify failed restore path: {blocked}"
    );
    assert_eq!(
        blocked["error"]["details"]["original_error"]["code"],
        json!("GIT_ERROR")
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
            "author",
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
            "author",
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
