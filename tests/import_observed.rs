mod common;

use std::fs;
use std::process::Command;

use serde_json::Value;

use common::actions::target_add;
use common::{TestDir, run_loom};

fn write_skill_dir(base: &std::path::Path, name: &str, body: &str) {
    let dir = base.join(name);
    fs::create_dir_all(&dir).expect("create skill dir");
    fs::write(dir.join("SKILL.md"), body).expect("write SKILL.md");
}

fn git_log_subjects(root: &std::path::Path) -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "--format=%s"])
        .output()
        .expect("git log");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect()
}

/// Scenario 1: two SKILL.md-bearing subdirs are imported; non-skill dirs ignored.
#[test]
fn import_observed_imports_skill_dirs() {
    let root = TestDir::new("import-obs-import");
    let target = TestDir::new("import-obs-import-target");

    write_skill_dir(target.path(), "skill-alpha", "# Alpha\n");
    write_skill_dir(target.path(), "skill-beta", "# Beta\n");
    // A directory without SKILL.md — must be ignored.
    fs::create_dir_all(target.path().join("not-a-skill")).unwrap();

    let (t_out, _) = target_add(root.path(), "claude", target.path(), "observed");
    assert!(
        t_out.status.success(),
        "target_add failed: {}",
        String::from_utf8_lossy(&t_out.stderr)
    );

    let (out, env) = run_loom(root.path(), &["skill", "import-observed"]);
    assert!(
        out.status.success(),
        "import-observed failed: stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));

    let imported = env["data"]["imported"].as_array().expect("imported array");
    assert_eq!(imported.len(), 2, "expected 2 imported skills");

    let skipped = env["data"]["skipped"].as_array().expect("skipped array");
    assert_eq!(skipped.len(), 0);

    assert!(
        root.path().join("skills/skill-alpha/SKILL.md").exists(),
        "skills/skill-alpha/SKILL.md missing"
    );
    assert!(
        root.path().join("skills/skill-beta/SKILL.md").exists(),
        "skills/skill-beta/SKILL.md missing"
    );
    assert!(
        !root.path().join("skills/not-a-skill").exists(),
        "non-skill dir must not be imported"
    );
}

/// Scenario 2: each imported skill produces a commit with the expected message.
#[test]
fn import_observed_commits_each_skill() {
    let root = TestDir::new("import-obs-commit");
    let target = TestDir::new("import-obs-commit-target");

    write_skill_dir(target.path(), "skill-alpha", "# Alpha\n");
    write_skill_dir(target.path(), "skill-beta", "# Beta\n");

    let (t_out, t_env) = target_add(root.path(), "claude", target.path(), "observed");
    assert!(t_out.status.success(), "target_add failed");
    let target_id = t_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target_id")
        .to_string();

    let (out, env) = run_loom(root.path(), &["skill", "import-observed"]);
    assert!(out.status.success(), "import-observed failed");

    // Collect reported commit SHAs.
    let imported = env["data"]["imported"].as_array().expect("imported array");
    let reported_shas: Vec<String> = imported
        .iter()
        .filter_map(|e| e["commit"].as_str().map(|s| s[..7].to_string()))
        .collect();
    assert_eq!(reported_shas.len(), 2, "expected 2 commit SHAs");

    // Git log must contain the two expected subjects.
    let subjects = git_log_subjects(root.path());
    let import_commits: Vec<_> = subjects
        .iter()
        .filter(|s| s.contains("import from observed target"))
        .collect();
    assert_eq!(import_commits.len(), 2, "expected 2 import commits in git log");

    for subject in &import_commits {
        assert!(
            subject.contains(&target_id),
            "commit message '{}' must contain target_id '{}'",
            subject,
            target_id
        );
    }
}

/// Scenario 3: running the command twice is idempotent — second run skips, adds no commits.
#[test]
fn import_observed_is_idempotent() {
    let root = TestDir::new("import-obs-idempotent");
    let target = TestDir::new("import-obs-idempotent-target");

    write_skill_dir(target.path(), "skill-alpha", "# Alpha\n");
    write_skill_dir(target.path(), "skill-beta", "# Beta\n");

    let (t_out, _) = target_add(root.path(), "claude", target.path(), "observed");
    assert!(t_out.status.success(), "target_add failed");

    // First run.
    let (out1, env1) = run_loom(root.path(), &["skill", "import-observed"]);
    assert!(out1.status.success(), "first import-observed failed");
    assert_eq!(
        env1["data"]["imported"].as_array().unwrap().len(),
        2,
        "first run must import 2 skills"
    );

    let commits_after_first = git_log_subjects(root.path()).len();

    // Second run.
    let (out2, env2) = run_loom(root.path(), &["skill", "import-observed"]);
    assert!(out2.status.success(), "second import-observed failed");

    let imported2 = env2["data"]["imported"].as_array().unwrap();
    assert_eq!(imported2.len(), 0, "second run must import nothing");

    let skipped2 = env2["data"]["skipped"].as_array().unwrap();
    assert_eq!(skipped2.len(), 2, "second run must skip 2 skills");

    let commits_after_second = git_log_subjects(root.path()).len();
    assert_eq!(
        commits_after_first, commits_after_second,
        "second run must not add new commits"
    );
}
