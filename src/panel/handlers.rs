use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::cli::{
    CaptureArgs, Command, ProjectArgs, ProjectionMethod, SyncCommand, TargetCommand,
    TargetOwnership, WorkspaceBindingCommand, WorkspaceCommand,
};
use crate::commands::{collect_skill_inventory, remote_status_payload};
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::V3StatePaths;

use super::auth::{
    ensure_mutation_authorized, error_envelope, load_v3_snapshot, run_panel_command, v3_error,
    v3_ok,
};
use super::{
    BindingAddRequest, CaptureRequest, DiffParams, PanelState, ProjectRequest, TargetAddRequest,
};

/// Accept `[a-z0-9_-]{1,64}` for `policy_profile`. The backend does not
/// maintain a closed whitelist (users may extend profiles over time),
/// but the panel surface should refuse obviously malformed input so the
/// V3 bindings file stays auditable. CLI users may still submit other
/// formats directly via `loom workspace binding add`.
fn policy_profile_looks_sane(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub(super) async fn health() -> Json<serde_json::Value> {
    Json(json!({"ok": true, "service": "loom-panel"}))
}

pub(super) async fn info(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let target_dirs = resolve_agent_skill_dirs(&state.ctx.root);
    let remote_url = crate::gitops::remote_url(&state.ctx)
        .ok()
        .flatten()
        .unwrap_or_default();
    let v3_paths = V3StatePaths::from_app_context(&state.ctx);

    Json(json!({
        "root": state.ctx.root.display().to_string(),
        "state_dir": state.ctx.state_dir.display().to_string(),
        "v3_targets_file": v3_paths.targets_file.display().to_string(),
        "claude_dir": target_dirs.claude.display().to_string(),
        "codex_dir": target_dirs.codex.display().to_string(),
        "remote_url": remote_url,
    }))
}

pub(super) async fn skills(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let inventory = collect_skill_inventory(&state.ctx);
    Json(json!({
        "skills": inventory.source_skills,
        "backup_skills": inventory.backup_skills,
        "source_dirs": inventory
            .source_dirs
            .iter()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .collect::<Vec<_>>(),
        "warnings": inventory.warnings
    }))
}

pub(super) async fn v3_status(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => v3_ok(snapshot.status_view()),
        Err(err) => err,
    }
}

pub(super) async fn v3_bindings(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => v3_ok(json!({
            "state_model": "v3",
            "count": snapshot.bindings.bindings.len(),
            "bindings": snapshot.bindings.bindings
        })),
        Err(err) => err,
    }
}

pub(super) async fn v3_binding_show(
    AxumPath(binding_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    let snapshot = match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => snapshot,
        Err(err) => return err,
    };
    let binding = match snapshot.binding(&binding_id).cloned() {
        Some(binding) => binding,
        None => {
            return v3_error(
                "BINDING_NOT_FOUND",
                format!("binding '{}' not found", binding_id),
            );
        }
    };

    v3_ok(json!({
        "state_model": "v3",
        "binding": binding,
        "default_target": snapshot.binding_default_target(&binding),
        "rules": snapshot.binding_rules(&binding.binding_id),
        "projections": snapshot.binding_projections(&binding.binding_id)
    }))
}

pub(super) async fn v3_targets(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => v3_ok(json!({
            "state_model": "v3",
            "count": snapshot.targets.targets.len(),
            "targets": snapshot.targets.targets
        })),
        Err(err) => err,
    }
}

pub(super) async fn v3_target_show(
    AxumPath(target_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    let snapshot = match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => snapshot,
        Err(err) => return err,
    };
    let target = match snapshot.target(&target_id) {
        Some(target) => target,
        None => {
            return v3_error(
                "TARGET_NOT_FOUND",
                format!("target '{}' not found", target_id),
            );
        }
    };
    let relations = snapshot.target_relations(&target_id);

    v3_ok(json!({
        "state_model": "v3",
        "target": target,
        "bindings": relations.bindings,
        "rules": relations.rules,
        "projections": relations.projections
    }))
}

pub(super) async fn v3_target_add(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<TargetAddRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "target.add") {
        return response;
    }
    run_panel_command(
        &state,
        "target.add",
        StatusCode::CREATED,
        Command::Target {
            command: TargetCommand::Add(crate::cli::TargetAddArgs {
                agent: req.agent,
                path: req.path,
                ownership: req.ownership.unwrap_or(TargetOwnership::Managed),
            }),
        },
    )
}

pub(super) async fn v3_target_remove(
    AxumPath(target_id): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "target.remove") {
        return response;
    }
    run_panel_command(
        &state,
        "target.remove",
        StatusCode::OK,
        Command::Target {
            command: TargetCommand::Remove(crate::cli::TargetShowArgs { target_id }),
        },
    )
}

pub(super) async fn v3_binding_add(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<BindingAddRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) =
        ensure_mutation_authorized(&state, peer, &headers, "workspace.binding.add")
    {
        return response;
    }
    let policy_profile = req
        .policy_profile
        .unwrap_or_else(|| "safe-capture".to_string());
    if !policy_profile_looks_sane(&policy_profile) {
        let request_id = uuid::Uuid::new_v4().to_string();
        return (
            StatusCode::BAD_REQUEST,
            Json(error_envelope(
                "workspace.binding.add",
                &request_id,
                "ARG_INVALID",
                "policy_profile must match [a-z0-9_-]{1,64}",
            )),
        );
    }
    run_panel_command(
        &state,
        "workspace.binding.add",
        StatusCode::CREATED,
        Command::Workspace {
            command: WorkspaceCommand::Binding {
                command: WorkspaceBindingCommand::Add(crate::cli::BindingAddArgs {
                    agent: req.agent,
                    profile: req.profile,
                    matcher_kind: req.matcher_kind,
                    matcher_value: req.matcher_value,
                    target: req.target,
                    policy_profile,
                }),
            },
        },
    )
}

pub(super) async fn v3_binding_remove(
    AxumPath(binding_id): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) =
        ensure_mutation_authorized(&state, peer, &headers, "workspace.binding.remove")
    {
        return response;
    }
    run_panel_command(
        &state,
        "workspace.binding.remove",
        StatusCode::OK,
        Command::Workspace {
            command: WorkspaceCommand::Binding {
                command: WorkspaceBindingCommand::Remove(crate::cli::BindingShowArgs {
                    binding_id,
                }),
            },
        },
    )
}

pub(super) async fn v3_project(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<ProjectRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.project") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.project",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Project(ProjectArgs {
                skill: req.skill,
                binding: req.binding,
                target: req.target,
                method: req.method.unwrap_or(ProjectionMethod::Symlink),
            }),
        },
    )
}

pub(super) async fn v3_capture(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<CaptureRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.capture") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.capture",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Capture(CaptureArgs {
                skill: req.skill,
                binding: req.binding,
                instance: req.instance,
                message: req.message,
            }),
        },
    )
}

// Sync handlers wrap `App::cmd_sync` one-to-one with the corresponding
// `SyncCommand` variant so the panel exposes the same git-backed flow as
// the `loom sync {push,pull,replay}` CLI. Each route goes through
// `ensure_mutation_authorized` + `run_panel_command`, so the JSON envelope,
// error-code mapping, and audit-log semantics match other mutations.

pub(super) async fn sync_push(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "sync.push") {
        return response;
    }
    run_panel_command(
        &state,
        "sync.push",
        StatusCode::OK,
        Command::Sync {
            command: SyncCommand::Push,
        },
    )
}

pub(super) async fn sync_pull(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "sync.pull") {
        return response;
    }
    run_panel_command(
        &state,
        "sync.pull",
        StatusCode::OK,
        Command::Sync {
            command: SyncCommand::Pull,
        },
    )
}

pub(super) async fn sync_replay(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "sync.replay") {
        return response;
    }
    run_panel_command(
        &state,
        "sync.replay",
        StatusCode::OK,
        Command::Sync {
            command: SyncCommand::Replay,
        },
    )
}

pub(super) async fn remote_status(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match remote_status_payload(&state.ctx) {
        Ok((remote, meta)) => Json(json!({"remote": remote, "warnings": meta.warnings})),
        Err(err) => Json(json!({"error": err.message, "code": err.code.as_str()})),
    }
}

fn is_valid_git_rev(rev: &str) -> bool {
    let len = rev.len();
    (7..=40).contains(&len) && rev.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_'))
}

/// Returns the SHA of the second-newest commit that touched `skill_path`, if any.
fn skill_parent_rev(root: &std::path::Path, skill_path: &str) -> Option<String> {
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

fn parse_unified_diff(diff_text: &str) -> Vec<serde_json::Value> {
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
        if line.starts_with("diff --git a/") {
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
            f_path = line
                .strip_prefix("diff --git a/")
                .and_then(|r| r.rfind(" b/").map(|i| r[..i].to_string()))
                .unwrap_or_default();
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
        } else if line.starts_with('+') && !line.starts_with("+++") {
            f_added += 1;
            if h_count < MAX_HUNK_LINES {
                h_lines.push(line.to_string());
                h_count += 1;
            }
        } else if line.starts_with('-') && !line.starts_with("---") {
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
                "skill name must contain only [a-zA-Z0-9_-]".to_string(),
            ),
        );
    }

    if let Some(ref r) = params.rev_a
        && !is_valid_git_rev(r)
    {
        return (
            StatusCode::BAD_REQUEST,
            v3_error("GIT_DIFF_FAILED", "rev_a must match [a-f0-9]{7,40}".to_string()),
        );
    }
    if let Some(ref r) = params.rev_b
        && !is_valid_git_rev(r)
    {
        return (
            StatusCode::BAD_REQUEST,
            v3_error("GIT_DIFF_FAILED", "rev_b must match [a-f0-9]{7,40}".to_string()),
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

    let output = match std::process::Command::new("git")
        .arg("-C")
        .arg(&state.ctx.root)
        .arg("diff")
        .arg("--unified=3")
        .arg(&range)
        .arg("--")
        .arg(&skill_path)
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                v3_error("GIT_DIFF_FAILED", format!("git process error: {e}")),
            );
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return (
            StatusCode::BAD_REQUEST,
            v3_error("GIT_DIFF_FAILED", stderr.trim().to_string()),
        );
    }

    const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024; // 4 MiB
    if output.stdout.len() > MAX_DIFF_BYTES {
        return (
            StatusCode::BAD_REQUEST,
            v3_error(
                "GIT_DIFF_FAILED",
                format!("diff exceeds {MAX_DIFF_BYTES} bytes; narrow the revision range"),
            ),
        );
    }

    let diff_text = String::from_utf8_lossy(&output.stdout);
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

pub(super) async fn pending(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match state.ctx.read_pending_report() {
        Ok(report) => Json(json!({
            "count": report.ops.len(),
            "ops": report.ops,
            "journal_events": report.journal_events,
            "history_events": report.history_events,
            "warnings": report.warnings
        })),
        Err(err) => Json(json!({"count": 0, "ops": [], "error": err.to_string()})),
    }
}

#[cfg(test)]
mod tests {
    use super::{is_valid_git_rev, parse_unified_diff, v3_skill_diff};
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
