use super::{
    is_valid_git_rev, is_valid_skill_name, parse_diff_git_path, parse_unified_diff,
    registry_skill_diff,
};
use crate::panel::PanelState;
use crate::state::AppContext;
use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
};
use serde_json::json;
use std::{fs, sync::Arc};
use uuid::Uuid;

fn make_state(root: &std::path::Path) -> PanelState {
    let ctx = AppContext::new(Some(root.to_path_buf())).expect("AppContext");
    PanelState {
        ctx: Arc::new(ctx),
        panel_origin: "http://127.0.0.1:43117".to_string(),
    }
}

fn git_ok(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn is_valid_skill_name_accepts_dotted_names() {
    assert!(
        is_valid_skill_name("foo.bar"),
        "dotted names must be accepted"
    );
    assert!(is_valid_skill_name("foo-bar_baz.v2"));
    assert!(!is_valid_skill_name("."), ". must be rejected");
    assert!(!is_valid_skill_name(".."), ".. must be rejected");
    assert!(!is_valid_skill_name("foo/bar"), "/ must be rejected");
    assert!(!is_valid_skill_name(""), "empty must be rejected");
}

#[test]
fn is_valid_git_rev_accepts_and_rejects() {
    assert!(is_valid_git_rev("abc1234"));
    assert!(is_valid_git_rev("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"));
    assert!(!is_valid_git_rev("abc123"));
    assert!(!is_valid_git_rev("abc123g"));
    assert!(!is_valid_git_rev("HEAD"));
    assert!(!is_valid_git_rev(""));
}

#[test]
fn parse_unified_diff_parses_simple_add() {
    let diff = "\
diff --git a/skills/foo/foo.md b/skills/foo/foo.md
index abc1234..def5678 100644
--- a/skills/foo/foo.md
+++ b/skills/foo/foo.md
@@ -1,1 +1,2 @@
 line one
+line two
";
    let files = parse_unified_diff(diff);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], json!("skills/foo/foo.md"));
    assert_eq!(files[0]["added"], json!(1));
    assert_eq!(files[0]["removed"], json!(0));
    let hunks = files[0]["hunks"].as_array().unwrap();
    assert_eq!(hunks.len(), 1);
    let lines = hunks[0]["lines"].as_array().unwrap();
    assert!(lines.iter().any(|l| l.as_str() == Some("+line two")));
}

#[test]
fn parse_unified_diff_preserves_double_plus_minus_content() {
    let diff = "\
diff --git a/skills/foo/foo.md b/skills/foo/foo.md
index abc1234..def5678 100644
--- a/skills/foo/foo.md
+++ b/skills/foo/foo.md
@@ -1,3 +1,3 @@
 context
--- old;
---deletion with no space
+++ i;
+++addition with no space
";
    let files = parse_unified_diff(diff);
    assert_eq!(files[0]["removed"], json!(2));
    assert_eq!(files[0]["added"], json!(2));
    let hunks = files[0]["hunks"].as_array().unwrap();
    let lines: Vec<&str> = hunks[0]["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l.as_str())
        .collect();
    assert!(
        lines.contains(&"+++ i;"),
        "addition line `++ i;` (emitted as `+++ i;`) must not be dropped"
    );
    assert!(
        lines.contains(&"+++addition with no space"),
        "addition line starting with ++ must not be dropped"
    );
    assert!(
        lines.contains(&"--- old;"),
        "deletion line `-- old;` (emitted as `--- old;`) must not be dropped"
    );
    assert!(
        lines.contains(&"---deletion with no space"),
        "deletion line starting with -- must not be dropped"
    );
}

#[test]
fn parse_unified_diff_handles_quoted_path() {
    let diff = "\
diff --git \"a/skills/foo/my file.md\" \"b/skills/foo/my file.md\"
index abc1234..def5678 100644
--- \"a/skills/foo/my file.md\"
+++ \"b/skills/foo/my file.md\"
@@ -1 +1,2 @@
 line one
+line two
";
    let files = parse_unified_diff(diff);
    assert_eq!(files.len(), 1, "quoted-path file must be parsed");
    assert_eq!(files[0]["path"], json!("skills/foo/my file.md"));
    assert_eq!(files[0]["added"], json!(1));
}

#[test]
fn parse_diff_git_path_returns_b_side_for_rename() {
    assert_eq!(
        parse_diff_git_path("diff --git a/skills/foo/old.md b/skills/foo/new.md"),
        Some("skills/foo/new.md".to_string()),
    );
}

#[test]
fn parse_diff_git_path_decodes_octal_in_quoted_path() {
    let line = r#"diff --git "a/skills/foo/\346\226\207" "b/skills/foo/\346\226\207""#;
    assert_eq!(parse_diff_git_path(line), Some("skills/foo/文".to_string()));
}

#[tokio::test]
async fn registry_skill_diff_returns_error_for_nonexistent_skill() {
    let root = std::env::temp_dir().join(format!("loom-diff-nopath-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("skills/other")).unwrap();

    let git = |args: &[&str]| git_ok(&root, args);

    git(&["init"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);

    fs::write(root.join("skills/other/other.md"), "v1\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-m", "initial"]);
    let rev_a = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();

    fs::write(root.join("skills/other/other.md"), "v2\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-m", "update"]);
    let rev_b = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();

    let state = make_state(&root);
    let (status, Json(payload)) = registry_skill_diff(
        AxumPath("nonexistent".to_string()),
        Query(super::super::DiffParams {
            rev_a: Some(rev_a),
            rev_b: Some(rev_b),
        }),
        State(state),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["error"]["code"], json!("GIT_DIFF_FAILED"));

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn registry_skill_diff_rejects_malformed_rev_a() {
    let root = std::env::temp_dir().join(format!("loom-diff-bad-rev-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let state = make_state(&root);

    let (status, Json(payload)) = registry_skill_diff(
        AxumPath("foo".to_string()),
        Query(super::super::DiffParams {
            rev_a: Some("invalid!rev".to_string()),
            rev_b: None,
        }),
        State(state),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["error"]["code"], json!("GIT_DIFF_FAILED"));

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn registry_skill_diff_returns_diff_for_two_commits() {
    let root = std::env::temp_dir().join(format!("loom-diff-integ-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("skills/foo")).unwrap();

    let git = |args: &[&str]| git_ok(&root, args);

    git(&["init"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);

    fs::write(root.join("skills/foo/foo.md"), "line one\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-m", "initial"]);

    let rev_a = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();

    fs::write(root.join("skills/foo/foo.md"), "line one\nline two\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-m", "add line two"]);

    let rev_b = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();

    let state = make_state(&root);
    let (status, Json(payload)) = registry_skill_diff(
        AxumPath("foo".to_string()),
        Query(super::super::DiffParams {
            rev_a: Some(rev_a),
            rev_b: Some(rev_b),
        }),
        State(state),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    let files = payload["data"]["files"].as_array().expect("files array");
    assert_eq!(files.len(), 1, "one file changed");
    assert_eq!(files[0]["added"], json!(1));
    let all_lines: Vec<&str> = files[0]["hunks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|h| h["lines"].as_array().unwrap())
        .filter_map(|l| l.as_str())
        .collect();
    assert!(
        all_lines.iter().any(|l| l.contains("line two")),
        "diff must contain the added line"
    );

    let _ = fs::remove_dir_all(&root);
}
