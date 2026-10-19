mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{TestDir, run_loom, write_file};
use serde_json::{Value, json};

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgSign=false"])
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn skill_add_resolves_refs_when_source_head_is_another_branch() {
    let source = TestDir::new("skill-ref-source");
    git(source.path(), &["init"]);
    git(source.path(), &["config", "user.name", "Skill Source"]);
    git(source.path(), &["config", "user.email", "source@example.com"]);
    git(source.path(), &["checkout", "-b", "main"]);
    write_file(&source.path().join("skill/SKILL.md"), "# Main\n");
    git(source.path(), &["add", "."]);
    git(source.path(), &["commit", "-m", "main skill"]);
    let main = git(source.path(), &["rev-parse", "HEAD"]);
    git(source.path(), &["tag", "-a", "v1", "-m", "main release"]);
    git(source.path(), &["tag", "lightweight"]);
    git(source.path(), &["checkout", "-b", "feature/demo"]);
    write_file(&source.path().join("skill/SKILL.md"), "# Feature\n");
    git(source.path(), &["add", "."]);
    git(source.path(), &["commit", "-m", "feature skill"]);
    let feature = git(source.path(), &["rev-parse", "HEAD"]);
    git(source.path(), &["checkout", "-b", "other"]);
    write_file(&source.path().join("skill/SKILL.md"), "# Other\n");
    git(source.path(), &["add", "."]);
    git(source.path(), &["commit", "-m", "other skill"]);

    let other = git(source.path(), &["rev-parse", "HEAD"]);

    for (reference, expected_commit, expected_content) in [
        ("other", other.as_str(), "# Other\n"),
        ("main", main.as_str(), "# Main\n"),
        ("feature/demo", feature.as_str(), "# Feature\n"),
        ("origin/feature/demo", feature.as_str(), "# Feature\n"),
        ("v1", main.as_str(), "# Main\n"),
        ("lightweight", main.as_str(), "# Main\n"),
        (main.as_str(), main.as_str(), "# Main\n"),
    ] {
        let root = TestDir::new("skill-ref-import");
        let (output, envelope) = run_loom(
            root.path(),
            &[
                "skill",
                "add",
                source.path().to_str().expect("source path"),
                "--name",
                "demo",
                "--subdir",
                "skill",
                "--ref",
                reference,
            ],
        );
        assert!(output.status.success(), "ref {reference}: {envelope}");
        assert_eq!(
            fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("imported skill"),
            expected_content
        );
        let lock: Value = serde_json::from_str(
            &fs::read_to_string(root.path().join("loom.lock")).expect("lock"),
        )
        .expect("parse lock");
        assert_eq!(lock["skills"]["demo"]["ref"], json!(reference));
        assert_eq!(lock["skills"]["demo"]["commit"], json!(expected_commit));
        let tree = git(
            source.path(),
            &["rev-parse", &format!("{expected_commit}:skill")],
        );
        assert_eq!(lock["skills"]["demo"]["tree_sha"], json!(tree));
    }

    for reference in ["missing-branch", "HEAD^{tree}"] {
        let root = TestDir::new("skill-ref-invalid");
        let (output, envelope) = run_loom(
            root.path(),
            &[
                "skill",
                "add",
                source.path().to_str().expect("source path"),
                "--name",
                "demo",
                "--subdir",
                "skill",
                "--ref",
                reference,
            ],
        );
        assert!(!output.status.success(), "ref {reference}: {envelope}");
        assert_eq!(envelope["error"]["code"], json!("ARG_INVALID"));
        assert!(
            envelope["error"]["message"]
                .as_str()
                .expect("error message")
                .contains("failed to resolve ref")
        );
        assert!(!root.path().join("skills/demo").exists());
    }
}
