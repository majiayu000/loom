use std::process::Stdio;

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
};
use serde_json::json;
use tokio::io::AsyncReadExt;

use super::auth::{v3_error, v3_ok};
use super::{DiffParams, PanelState};

pub(super) fn is_valid_git_rev(rev: &str) -> bool {
    let len = rev.len();
    (7..=40).contains(&len) && rev.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub(super) fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.'))
}

/// Returns the SHA of the second-newest commit that touched `skill_path`, if any.
pub(super) fn skill_parent_rev(root: &std::path::Path, skill_path: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("log")
        .arg("--format=%H")
        .arg("-n")
        .arg("2")
        .arg("--")
        .arg(skill_path)
        .output()
        .ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout);
        let mut lines = s.lines();
        lines.next()?; // newest commit (will be rev_b)
        lines.next().map(|s| s.to_string())
    } else {
        None
    }
}

fn skill_exists_in_rev(root: &std::path::Path, rev: &str, skill_path: &str) -> bool {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("ls-tree")
        .arg("--name-only")
        .arg(rev)
        .arg("--")
        .arg(skill_path)
        .output()
        .ok();
    matches!(out, Some(o) if o.status.success() && !o.stdout.is_empty())
}

fn resolve_rev(root: &std::path::Path, rev: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("rev-parse")
        .arg(rev)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// Extract the b-side (new) path from a `diff --git` header line.
///
/// Handles unquoted (`diff --git a/path b/path`) and git-quoted forms
/// (`diff --git "a/path" "b/path"`), decoding git octal escape sequences
/// (e.g. `\346\226\207` for UTF-8 bytes of non-ASCII filenames).
/// Returns the b-side so rename diffs report the new filename.
fn parse_diff_git_path(line: &str) -> Option<String> {
    let rest = line.strip_prefix("diff --git ")?;
    if rest.starts_with('"') {
        // Quoted form: skip the a-side quoted string, then decode the b-side.
        let bytes = rest.as_bytes();
        let mut i = 1; // skip opening quote of a-side
        while i < bytes.len() {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 1; // skip backslash
                if bytes[i].is_ascii_digit() {
                    i += 3; // skip 3-digit octal NNN
                } else {
                    i += 1; // skip single escape char (e.g. `"` or `\`)
                }
            } else if bytes[i] == b'"' {
                i += 1; // step past closing quote of a-side
                break;
            } else {
                i += 1;
            }
        }
        // After a-side, expect ` "b/..."`.
        let after_a = &rest[i..];
        if after_a.starts_with(" \"") {
            let b_bytes = after_a.as_bytes();
            let mut j = 2; // skip ` "`
            let mut decoded: Vec<u8> = Vec::new();
            while j < b_bytes.len() && b_bytes[j] != b'"' {
                if b_bytes[j] == b'\\' && j + 1 < b_bytes.len() {
                    j += 1;
                    if b_bytes[j].is_ascii_digit()
                        && j + 2 < b_bytes.len()
                        && b_bytes[j + 1].is_ascii_digit()
                        && b_bytes[j + 2].is_ascii_digit()
                    {
                        // Octal escape \NNN → single byte
                        let v = (b_bytes[j] - b'0') as u32 * 64
                            + (b_bytes[j + 1] - b'0') as u32 * 8
                            + (b_bytes[j + 2] - b'0') as u32;
                        decoded.push(v as u8);
                        j += 3;
                    } else {
                        decoded.push(match b_bytes[j] {
                            b'n' => b'\n',
                            b't' => b'\t',
                            b'r' => b'\r',
                            c => c,
                        });
                        j += 1;
                    }
                } else {
                    decoded.push(b_bytes[j]);
                    j += 1;
                }
            }
            let b_path = String::from_utf8_lossy(&decoded).into_owned();
            b_path.strip_prefix("b/").map(|s| s.to_string())
        } else if after_a.starts_with(" b/") {
            Some(after_a[3..].to_string())
        } else {
            None
        }
    } else {
        // Unquoted form: `a/path b/path` — take the b-side (after last ` b/`).
        rest.rfind(" b/").map(|i| rest[i + 3..].to_string())
    }
}

pub(super) fn parse_unified_diff(diff_text: &str) -> Vec<serde_json::Value> {
    const MAX_HUNK_LINES: usize = 500;

    let mut files: Vec<serde_json::Value> = Vec::new();
    let mut f_path = String::new();
    let mut f_added: usize = 0;
    let mut f_removed: usize = 0;
    let mut f_hunks: Vec<serde_json::Value> = Vec::new();
    let mut h_hdr = String::new();
    let mut h_lines: Vec<String> = Vec::new();
    let mut h_count: usize = 0;
    let mut in_file = false;

    for line in diff_text.lines() {
        if line.starts_with("diff --git ") {
            if !h_hdr.is_empty() {
                f_hunks.push(json!({
                    "header": std::mem::take(&mut h_hdr),
                    "lines": std::mem::take(&mut h_lines),
                }));
                h_count = 0;
            }
            if in_file {
                files.push(json!({
                    "path": std::mem::take(&mut f_path),
                    "added": f_added,
                    "removed": f_removed,
                    "hunks": std::mem::take(&mut f_hunks),
                }));
                f_added = 0;
                f_removed = 0;
            }
            f_path = parse_diff_git_path(line).unwrap_or_default();
            in_file = true;
        } else if line.starts_with("@@ ") {
            if !h_hdr.is_empty() {
                f_hunks.push(json!({
                    "header": std::mem::take(&mut h_hdr),
                    "lines": std::mem::take(&mut h_lines),
                }));
                // h_count is intentionally NOT reset here — cap is per file, not per hunk
            }
            h_hdr = line.to_string();
        } else if line.starts_with('+') && !line.starts_with("+++ ") {
            // Guard against matching the `+++ b/file` header — real headers always
            // have a space after `+++`; content lines starting with `++` must not
            // be silently dropped.
            f_added += 1;
            if h_count < MAX_HUNK_LINES {
                h_lines.push(line.to_string());
                h_count += 1;
            }
        } else if line.starts_with('-') && !line.starts_with("--- ") {
            f_removed += 1;
            if h_count < MAX_HUNK_LINES {
                h_lines.push(line.to_string());
                h_count += 1;
            }
        } else if !h_hdr.is_empty()
            && (line.starts_with(' ') || line.is_empty())
            && h_count < MAX_HUNK_LINES
        {
            h_lines.push(line.to_string());
            h_count += 1;
        }
    }

    if !h_hdr.is_empty() {
        f_hunks.push(json!({
            "header": h_hdr,
            "lines": h_lines,
        }));
    }
    if in_file {
        files.push(json!({
            "path": f_path,
            "added": f_added,
            "removed": f_removed,
            "hunks": f_hunks,
        }));
    }

    files
}

pub(super) async fn v3_skill_diff(
    AxumPath(skill_name): AxumPath<String>,
    Query(params): Query<DiffParams>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_skill_name(&skill_name) {
        return (
            StatusCode::BAD_REQUEST,
            v3_error(
                "GIT_DIFF_FAILED",
                "skill name must contain only [a-zA-Z0-9._-]".to_string(),
            ),
        );
    }

    if let Some(ref r) = params.rev_a
        && !is_valid_git_rev(r)
    {
        return (
            StatusCode::BAD_REQUEST,
            v3_error(
                "GIT_DIFF_FAILED",
                "rev_a must match [a-f0-9]{7,40}".to_string(),
            ),
        );
    }
    if let Some(ref r) = params.rev_b
        && !is_valid_git_rev(r)
    {
        return (
            StatusCode::BAD_REQUEST,
            v3_error(
                "GIT_DIFF_FAILED",
                "rev_b must match [a-f0-9]{7,40}".to_string(),
            ),
        );
    }

    let skill_path = format!("skills/{}/", skill_name);
    let rev_b = params.rev_b.unwrap_or_else(|| "HEAD".to_string());

    let rev_a = match params.rev_a {
        Some(r) => r,
        None => match skill_parent_rev(&state.ctx.root, &skill_path) {
            Some(sha) => sha,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    v3_error(
                        "GIT_DIFF_FAILED",
                        "fewer than 2 commits touch this skill; provide rev_a explicitly"
                            .to_string(),
                    ),
                );
            }
        },
    };
    let range = format!("{}..{}", rev_a, rev_b);

    if !skill_exists_in_rev(&state.ctx.root, &rev_b, &skill_path)
        && !skill_exists_in_rev(&state.ctx.root, &rev_a, &skill_path)
    {
        return (
            StatusCode::BAD_REQUEST,
            v3_error(
                "GIT_DIFF_FAILED",
                format!("skill '{skill_name}' not found in revision range"),
            ),
        );
    }

    const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024; // 4 MiB

    let mut child = match tokio::process::Command::new("git")
        .arg("-C")
        .arg(&state.ctx.root)
        .arg("diff")
        .arg("--no-ext-diff")
        .arg("--no-textconv")
        .arg("--unified=3")
        .arg(&range)
        .arg("--")
        .arg(&skill_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                v3_error("GIT_DIFF_FAILED", format!("git process error: {e}")),
            );
        }
    };

    // Drain stderr concurrently so git never blocks on a full pipe.
    let stderr_handle = child.stderr.take().map(|mut e| {
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = e.read_to_end(&mut buf).await;
            buf
        })
    });

    let mut stdout_buf = Vec::with_capacity(64 * 1024);
    if let Some(stdout) = child.stdout.take() {
        if let Err(e) = stdout
            .take(MAX_DIFF_BYTES as u64 + 1)
            .read_to_end(&mut stdout_buf)
            .await
        {
            let _ = child.kill().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                v3_error("GIT_DIFF_FAILED", format!("reading git output: {e}")),
            );
        }
    }

    if stdout_buf.len() > MAX_DIFF_BYTES {
        let _ = child.kill().await;
        return (
            StatusCode::BAD_REQUEST,
            v3_error(
                "GIT_DIFF_FAILED",
                format!("diff exceeds {MAX_DIFF_BYTES} bytes; narrow the revision range"),
            ),
        );
    }

    let stderr_bytes = match stderr_handle {
        Some(h) => h.await.unwrap_or_default(),
        None => Vec::new(),
    };
    let status = match child.wait().await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                v3_error("GIT_DIFF_FAILED", format!("waiting for git: {e}")),
            );
        }
    };

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        return (
            StatusCode::BAD_REQUEST,
            v3_error("GIT_DIFF_FAILED", stderr.trim().to_string()),
        );
    }

    let diff_text = String::from_utf8_lossy(&stdout_buf);
    let files = parse_unified_diff(&diff_text);

    let resolved_a = resolve_rev(&state.ctx.root, &rev_a).unwrap_or(rev_a);
    let resolved_b = resolve_rev(&state.ctx.root, &rev_b).unwrap_or(rev_b);

    (
        StatusCode::OK,
        v3_ok(json!({
            "skill": skill_name,
            "rev_a": resolved_a,
            "rev_b": resolved_b,
            "files": files,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::{is_valid_git_rev, parse_diff_git_path, parse_unified_diff, v3_skill_diff};
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
            dist_dir: root.join("panel/dist"),
            panel_origin: "http://127.0.0.1:43117".to_string(),
        }
    }

    #[test]
    fn is_valid_skill_name_accepts_dotted_names() {
        use super::is_valid_skill_name;
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
        assert!(!is_valid_git_rev("abc123")); // 6 chars — too short
        assert!(!is_valid_git_rev("abc123g")); // invalid char
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
        // A source file line starting with `++` appears as `+++...` in the diff.
        // The old `!starts_with("+++")` check would drop it; the fixed check
        // `!starts_with("+++ ")` (with trailing space) keeps it.
        let diff = "\
diff --git a/skills/foo/foo.md b/skills/foo/foo.md
index abc1234..def5678 100644
--- a/skills/foo/foo.md
+++ b/skills/foo/foo.md
@@ -1,2 +1,3 @@
 context
-old line
+++content that starts with ++
";
        let files = parse_unified_diff(diff);
        assert_eq!(files[0]["removed"], json!(1));
        assert_eq!(files[0]["added"], json!(1));
        let hunks = files[0]["hunks"].as_array().unwrap();
        let lines: Vec<&str> = hunks[0]["lines"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|l| l.as_str())
            .collect();
        assert!(
            lines.iter().any(|l| *l == "+++content that starts with ++"),
            "content line starting with ++ must not be dropped"
        );
    }

    #[test]
    fn parse_unified_diff_handles_quoted_path() {
        // Git quotes paths that contain spaces or non-ASCII chars.
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
        // Unquoted rename: a-side is old.md, b-side is new.md.
        assert_eq!(
            parse_diff_git_path("diff --git a/skills/foo/old.md b/skills/foo/new.md"),
            Some("skills/foo/new.md".to_string()),
        );
    }

    #[test]
    fn parse_diff_git_path_decodes_octal_in_quoted_path() {
        // \346\226\207 is the UTF-8 encoding of the kanji 文 (U+6587).
        let line = r#"diff --git "a/skills/foo/\346\226\207" "b/skills/foo/\346\226\207""#;
        assert_eq!(parse_diff_git_path(line), Some("skills/foo/文".to_string()),);
    }

    #[tokio::test]
    async fn v3_skill_diff_returns_error_for_nonexistent_skill() {
        let root = std::env::temp_dir().join(format!("loom-diff-nopath-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("skills/other")).unwrap();

        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .expect("git")
        };

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
        let (status, Json(payload)) = v3_skill_diff(
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
    async fn v3_skill_diff_rejects_malformed_rev_a() {
        let root = std::env::temp_dir().join(format!("loom-diff-bad-rev-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let state = make_state(&root);

        let (status, Json(payload)) = v3_skill_diff(
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
    async fn v3_skill_diff_returns_diff_for_two_commits() {
        let root = std::env::temp_dir().join(format!("loom-diff-integ-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("skills/foo")).unwrap();

        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .expect("git")
        };

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
        let (status, Json(payload)) = v3_skill_diff(
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
}
