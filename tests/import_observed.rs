mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use common::actions::target_add;
use common::{TestDir, run_loom, write_file};

#[cfg(unix)]
fn symlink_dir(src: &Path, dst: &Path) {
    std::os::unix::fs::symlink(src, dst).expect("create symlink dir");
}

fn git_path_exists(root: &Path, path: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("cat-file")
        .arg("-e")
        .arg(format!("HEAD:{path}"))
        .status()
        .expect("run git cat-file")
        .success()
}

fn imported_skill_names(env: &Value) -> Vec<String> {
    let mut names = env["data"]["imported"]
        .as_array()
        .expect("imported array")
        .iter()
        .map(|item| item["skill"].as_str().expect("skill name").to_string())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn skipped_reasons(env: &Value) -> Vec<String> {
    let mut reasons = env["data"]["skipped"]
        .as_array()
        .expect("skipped array")
        .iter()
        .filter_map(|item| item["reason"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    reasons.sort();
    reasons
}

#[test]
fn skill_import_observed_imports_real_skill_dirs_and_commits_them() {
    let root = TestDir::new("import-observed");
    let observed = root.path().join("observed-skills");
    let managed = root.path().join("managed-skills");

    write_file(&observed.join("alpha/SKILL.md"), "# alpha\n");
    write_file(&observed.join("alpha/nested/config.txt"), "alpha config\n");
    write_file(&observed.join("beta/SKILL.md"), "# beta\n");
    write_file(&observed.join("not-a-skill/README.md"), "ignore me\n");
    write_file(&observed.join("plain-file.txt"), "ignore me\n");

    write_file(&managed.join("managed-only/SKILL.md"), "# managed only\n");

    let (observed_output, observed_env) = target_add(root.path(), "claude", &observed, "observed");
    assert!(
        observed_output.status.success(),
        "target add failed: {}",
        String::from_utf8_lossy(&observed_output.stderr)
    );
    let observed_target_id = observed_env["data"]["target"]["target_id"]
        .as_str()
        .expect("observed target id")
        .to_string();

    let (managed_output, _) = target_add(root.path(), "codex", &managed, "managed");
    assert!(managed_output.status.success(), "managed target add failed");

    let (import_output, import_env) = run_loom(root.path(), &["skill", "import-observed"]);
    assert!(
        import_output.status.success(),
        "import failed: stderr={} stdout={}",
        String::from_utf8_lossy(&import_output.stderr),
        String::from_utf8_lossy(&import_output.stdout)
    );

    assert_eq!(import_env["ok"], Value::Bool(true));
    assert_eq!(
        import_env["cmd"],
        Value::String("skill.import_observed".into())
    );
    assert_eq!(import_env["data"]["count"], Value::from(2));
    assert_eq!(imported_skill_names(&import_env), vec!["alpha", "beta"]);
    assert_eq!(
        import_env["meta"]["op_id"]
            .as_str()
            .map(|op_id| op_id.starts_with("op_")),
        Some(true)
    );

    assert_eq!(
        fs::read_to_string(root.path().join("skills/alpha/SKILL.md")).expect("read alpha"),
        "# alpha\n"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("skills/alpha/nested/config.txt"))
            .expect("read alpha nested"),
        "alpha config\n"
    );
    assert!(root.path().join("skills/beta/SKILL.md").is_file());
    assert!(!root.path().join("skills/not-a-skill").exists());
    assert!(!root.path().join("skills/managed-only").exists());

    assert!(git_path_exists(root.path(), "skills/alpha/SKILL.md"));
    assert!(git_path_exists(
        root.path(),
        "skills/alpha/nested/config.txt"
    ));
    assert!(git_path_exists(root.path(), "skills/beta/SKILL.md"));

    let (repeat_output, repeat_env) = run_loom(
        root.path(),
        &["skill", "import-observed", "--target", &observed_target_id],
    );
    assert!(
        repeat_output.status.success(),
        "repeat import failed: stderr={} stdout={}",
        String::from_utf8_lossy(&repeat_output.stderr),
        String::from_utf8_lossy(&repeat_output.stdout)
    );
    assert_eq!(repeat_env["data"]["count"], Value::from(0));
    assert_eq!(
        skipped_reasons(&repeat_env),
        vec!["already-exists", "already-exists"]
    );
    assert_eq!(repeat_env["data"]["noop"], Value::Bool(true));
}

#[test]
#[cfg(unix)]
fn skill_import_observed_materializes_top_level_symlinked_skill_dirs() {
    let root = TestDir::new("import-observed-symlink");
    let observed = root.path().join("observed-skills");
    let linked_source = root.path().join("linked-source");

    fs::create_dir_all(&observed).expect("create observed dir");
    write_file(&linked_source.join("skill.md"), "# linked\n");
    write_file(&linked_source.join("nested/config.txt"), "linked config\n");
    symlink_dir(&linked_source, &observed.join("linked-skill"));

    let (target_output, _) = target_add(root.path(), "claude", &observed, "observed");
    assert!(
        target_output.status.success(),
        "target add failed: {}",
        String::from_utf8_lossy(&target_output.stderr)
    );

    let (import_output, import_env) = run_loom(root.path(), &["skill", "import-observed"]);
    assert!(
        import_output.status.success(),
        "import failed: stderr={} stdout={}",
        String::from_utf8_lossy(&import_output.stderr),
        String::from_utf8_lossy(&import_output.stdout)
    );

    assert_eq!(import_env["data"]["count"], Value::from(1));
    assert_eq!(imported_skill_names(&import_env), vec!["linked-skill"]);
    assert_eq!(
        import_env["data"]["imported"][0]["source_kind"],
        Value::String("symlink".to_string())
    );
    assert_eq!(
        import_env["data"]["imported"][0]["resolved_source"],
        Value::String(
            fs::canonicalize(&linked_source)
                .expect("canonical linked source")
                .display()
                .to_string()
        )
    );

    let imported_path = root.path().join("skills/linked-skill");
    assert!(
        !fs::symlink_metadata(&imported_path)
            .expect("imported path metadata")
            .file_type()
            .is_symlink(),
        "registry skill should be materialized as a real directory"
    );
    assert_eq!(
        fs::read_to_string(imported_path.join("skill.md")).expect("read linked skill"),
        "# linked\n"
    );
    assert_eq!(
        fs::read_to_string(imported_path.join("nested/config.txt")).expect("read nested config"),
        "linked config\n"
    );
    assert!(git_path_exists(root.path(), "skills/linked-skill/skill.md"));
    assert!(git_path_exists(
        root.path(),
        "skills/linked-skill/nested/config.txt"
    ));
}

#[test]
fn skill_import_observed_rejects_non_observed_target_filter() {
    let root = TestDir::new("import-observed-managed-target");
    let managed = root.path().join("managed-skills");
    write_file(&managed.join("managed-only/SKILL.md"), "# managed only\n");

    let (target_output, target_env) = target_add(root.path(), "codex", &managed, "managed");
    assert!(target_output.status.success(), "managed target add failed");
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");

    let (output, env) = run_loom(
        root.path(),
        &["skill", "import-observed", "--target", target_id],
    );

    assert!(
        !output.status.success(),
        "managed target unexpectedly imported"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    assert!(!root.path().join("skills/managed-only").exists());
}
