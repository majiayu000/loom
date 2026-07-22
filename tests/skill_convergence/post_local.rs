use std::{collections::BTreeSet, fs};

use serde_json::{Value, json};

use super::skill_convergence_executor::apply_plan;
use super::*;

#[test]
fn visibility_and_restart_states() {
    let fixture = projected_fixture();
    change_source(&fixture, "visibility reread\n");
    let (output, plan) = plan_converge(&fixture, &["--require-runtime"]);
    assert!(output.status.success(), "plan failed: {plan}");
    assert_eq!(plan["data"]["safe_to_apply"], json!(true));

    let (output, applied) = apply_plan(&fixture, &plan, "visibility-state", &[]);
    assert!(output.status.success(), "apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(data["local_state"], json!("complete"));
    assert_eq!(
        data["convergence"]["visibility"]["state"],
        json!("restart_required")
    );
    assert_eq!(data["complete"], json!(false));
    assert_eq!(data["outcome"], json!("local_complete_restart_required"));
    assert_eq!(
        data["completion_blockers"],
        json!(["visibility.restart_required"]),
        "unexpected blockers: {applied}"
    );
    assert_eq!(
        data["convergence"]["registry_transport"]["state"],
        json!("not_requested")
    );
    let transport = &data["convergence"]["registry_transport"];
    assert_eq!(transport["stale"], json!(false));
    assert!(transport["observed_at"].as_str().is_some());
    assert!(transport["evidence"]["observed_at"].as_str().is_some());
    assert!(
        transport["evidence"]["observed_revision"]
            .as_str()
            .is_some()
    );
    assert!(
        transport["evidence"]["checkpoint_updated_at"]
            .as_str()
            .is_some()
    );
    assert!(
        data["convergence"]["visibility"]["evidence"]["report"]["checks"]
            .as_array()
            .is_some_and(|checks| !checks.is_empty()),
        "visibility must come from an adapter reread: {applied}"
    );
    let head = Command::new("git")
        .current_dir(fixture.root.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("read final head");
    assert!(head.status.success(), "read final head failed");
    let head = String::from_utf8(head.stdout)
        .expect("head utf8")
        .trim()
        .to_string();
    let checkpoint: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .path()
                .join("state/registry/ops/checkpoint.json"),
        )
        .expect("read final checkpoint"),
    )
    .expect("parse final checkpoint");
    for axis in ["registry_transport", "projections", "visibility"] {
        assert_eq!(
            data["convergence"][axis]["evidence"]["observed_revision"],
            json!(head),
            "{axis} revision must describe final live HEAD"
        );
        assert_eq!(
            data["convergence"][axis]["evidence"]["checkpoint_updated_at"],
            checkpoint["updated_at"],
            "{axis} checkpoint must describe final live registry state"
        );
    }
}

#[test]
fn restart_required_acceptance_is_explicit() {
    let fixture = projected_fixture();
    change_source(&fixture, "accepted restart\n");
    let (output, plan) = plan_converge(
        &fixture,
        &["--require-runtime", "--accept-restart-required"],
    );
    assert!(output.status.success(), "plan failed: {plan}");
    assert_eq!(plan["data"]["accept_restart_required"], json!(true));

    let (output, applied) = apply_plan(&fixture, &plan, "accepted-restart", &[]);
    assert!(output.status.success(), "apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(
        data["convergence"]["visibility"]["state"],
        json!("restart_required")
    );
    assert_eq!(data["completion_blockers"], json!([]));
    assert_eq!(data["complete"], json!(true));
    assert_eq!(data["outcome"], json!("complete_with_restart_required"));
}

#[test]
fn interrupted_registry_recovery_retains_complete_b_evidence() {
    let fixture = projected_fixture();
    let convergence_operations_before = convergence_operation_count(fixture.root.path());
    change_source(&fixture, "recovered post-local evidence\n");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "recovered-post-local-evidence";

    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        key,
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_committing_registry",
        )],
    );
    assert!(!output.status.success(), "fault passed: {interrupted}");

    let (output, recovered) = apply_plan(&fixture, &plan, key, &[]);
    assert!(output.status.success(), "recovery failed: {recovered}");
    let data = &recovered["data"];
    assert_eq!(data["complete"], json!(true), "incomplete: {recovered}");
    assert_eq!(data["outcome"], json!("complete"));
    assert_eq!(
        data["evidence"]["registry_operation"],
        json!({"state": "not_applicable", "reason": "convergence_mode"})
    );
    for field in ["source", "projections", "visibility", "remote", "recovery"] {
        assert!(
            data["evidence"][field].is_object(),
            "missing {field} evidence: {recovered}"
        );
    }
    let journal: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json"),
        )
        .expect("retained journal"),
    )
    .expect("parse retained journal");
    assert_eq!(journal["phase"], json!("committed_artifacts_retained"));
    assert_eq!(journal["result"]["evidence"], data["evidence"]);
    assert_eq!(
        convergence_operation_count(fixture.root.path()),
        convergence_operations_before,
        "recovery must not append a registry ops ledger row"
    );
}

#[test]
fn remote_transport_excludes_unplanned_broad_sync_paths() {
    let fixture = projected_fixture();
    let remote = common::TestDir::new("convergence-remote-exact-scope");
    git(remote.path(), &["init", "--bare"]);
    let remote_path = remote.path().to_str().expect("remote path");
    git(
        fixture.root.path(),
        &["remote", "add", "origin", remote_path],
    );
    change_source(&fixture, "remote exact scope\n");
    let (output, plan) = plan_converge(&fixture, &["--push-remote"]);
    assert!(output.status.success(), "plan failed: {plan}");
    let reviewed_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);

    let gitignore = fixture.root.path().join(".gitignore");
    let mut gitignore_bytes = fs::read_to_string(&gitignore).unwrap_or_default();
    gitignore_bytes.push_str("unplanned-ignore\n");
    fs::write(&gitignore, &gitignore_bytes).expect("dirty gitignore");
    let gitattributes = fixture.root.path().join(".gitattributes");
    fs::write(&gitattributes, "unplanned/** binary\n").expect("dirty gitattributes");
    let registry_extra = fixture.root.path().join("state/registry/unplanned.json");
    fs::write(&registry_extra, "{\"unplanned\":true}\n").expect("dirty registry");
    let v3_extra = fixture.root.path().join("state/v3/unplanned");
    fs::create_dir_all(v3_extra.parent().expect("v3 parent")).expect("create v3");
    fs::write(&v3_extra, "unplanned v3\n").expect("dirty v3");
    git(
        fixture.root.path(),
        &["add", ".gitattributes", "state/v3/unplanned"],
    );

    let (output, applied) = apply_plan(&fixture, &plan, "remote-exact-scope", &[]);
    assert!(output.status.success(), "local apply failed: {applied}");
    assert_eq!(applied["data"]["complete"], json!(false));
    assert_eq!(
        applied["data"]["completion_blockers"],
        json!(["registry.remote_pending"])
    );
    assert_eq!(
        applied["data"]["convergence"]["registry_transport"]["errors"][0]["code"],
        json!("DEPENDENCY_CONFLICT")
    );
    let recorded_boundary = applied["data"]["applied"]["registry_commit"]
        .as_str()
        .expect("recorded registry commit")
        .to_string();

    let committed = git(
        fixture.root.path(),
        &["diff", "--name-only", reviewed_head.trim(), "HEAD"],
    );
    for path in [
        ".gitignore",
        ".gitattributes",
        "state/registry/unplanned.json",
        "state/v3/unplanned",
    ] {
        assert!(!committed.lines().any(|line| line == path));
    }
    assert_eq!(
        fs::read_to_string(&gitignore).expect("gitignore"),
        gitignore_bytes
    );
    assert_eq!(
        fs::read_to_string(&gitattributes).expect("gitattributes"),
        "unplanned/** binary\n"
    );
    assert_eq!(
        fs::read_to_string(&registry_extra).expect("registry extra"),
        "{\"unplanned\":true}\n"
    );
    assert_eq!(
        fs::read_to_string(&v3_extra).expect("v3 extra"),
        "unplanned v3\n"
    );
    let staged = git(fixture.root.path(), &["diff", "--cached", "--name-only"]);
    assert!(staged.lines().any(|line| line == ".gitattributes"));
    assert!(staged.lines().any(|line| line == "state/v3/unplanned"));
    let remote_head = Command::new("git")
        .arg("--git-dir")
        .arg(remote.path())
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .expect("inspect remote main");
    assert!(
        !remote_head.status.success(),
        "unplanned bytes reached remote main"
    );

    git(
        fixture.root.path(),
        &[
            "add",
            ".gitignore",
            ".gitattributes",
            "state/registry/unplanned.json",
            "state/v3/unplanned",
        ],
    );
    git(
        fixture.root.path(),
        &["commit", "-m", "test: commit unplanned transport paths"],
    );
    let later_local_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let (retry_output, retry) = apply_plan(&fixture, &plan, "remote-exact-scope", &[]);
    assert!(
        retry_output.status.success(),
        "exact-boundary retry failed: {retry}"
    );
    assert_eq!(retry["data"]["complete"], json!(true));
    assert_eq!(
        retry["data"]["convergence"]["registry_transport"]["evidence"]["pushed_commit"],
        json!(recorded_boundary)
    );
    let remote_head = Command::new("git")
        .arg("--git-dir")
        .arg(remote.path())
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .expect("inspect remote main after committed drift");
    assert!(remote_head.status.success(), "remote main was not created");
    assert_eq!(
        String::from_utf8(remote_head.stdout)
            .expect("remote head utf8")
            .trim(),
        recorded_boundary
    );
    assert_ne!(later_local_head.trim(), recorded_boundary);
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]).trim(),
        later_local_head.trim(),
        "exact-boundary transport rewrote the later local HEAD"
    );
    let remote_tree = Command::new("git")
        .arg("--git-dir")
        .arg(remote.path())
        .args(["ls-tree", "-r", "--name-only", "refs/heads/main"])
        .output()
        .expect("inspect remote main tree");
    assert!(
        remote_tree.status.success(),
        "remote main tree is unreadable"
    );
    let remote_paths = String::from_utf8(remote_tree.stdout).expect("remote tree utf8");
    for path in ["state/registry/unplanned.json", "state/v3/unplanned"] {
        assert!(
            !remote_paths.lines().any(|line| line == path),
            "unplanned path reached exact remote boundary: {path}"
        );
    }
}

#[test]
fn remote_pending_and_restart_blockers_compose() {
    let fixture = projected_fixture();
    change_source(&fixture, "two independent blockers\n");
    let (output, plan) = plan_converge(&fixture, &["--push-remote", "--require-runtime"]);
    assert!(output.status.success(), "plan failed: {plan}");

    let (output, applied) = apply_plan(&fixture, &plan, "combined-blockers", &[]);
    assert!(output.status.success(), "partial apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(
        data["outcome"],
        json!("local_complete_remote_pending_restart_required")
    );
    assert_eq!(
        data["completion_blockers"],
        json!(["registry.remote_pending", "visibility.restart_required"])
    );
    assert_eq!(data["next_actions"].as_array().map(Vec::len), Some(2));
    assert!(
        data["next_actions"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("transport"))
    );
    assert!(
        data["next_actions"][1]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("restart"))
    );
}

#[test]
fn remote_retry_rechecks_live_axes_before_push() {
    let fixture = projected_fixture();
    let remote = common::TestDir::new("convergence-retry-live-axes-remote");
    change_source(&fixture, "pending retry must recheck projections\n");
    let (output, plan) = plan_converge(&fixture, &["--push-remote"]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, pending) = apply_plan(&fixture, &plan, "retry-live-axes", &[]);
    assert!(output.status.success(), "pending apply failed: {pending}");
    assert_eq!(
        pending["data"]["completion_blockers"],
        json!(["registry.remote_pending"])
    );

    fs::remove_dir_all(fixture.target.path().join("demo")).expect("remove live projection");
    git(remote.path(), &["init", "--bare"]);
    let remote_path = remote.path().to_str().expect("remote path");
    git(
        fixture.root.path(),
        &["remote", "add", "origin", remote_path],
    );

    let (output, retried) = apply_plan(&fixture, &plan, "retry-live-axes", &[]);
    assert!(output.status.success(), "partial retry failed: {retried}");
    assert_eq!(retried["data"]["complete"], json!(false));
    assert_eq!(
        retried["data"]["completion_blockers"],
        json!(["registry.remote_pending", "projections.evidence_incomplete"])
    );
    assert_eq!(
        retried["data"]["convergence"]["registry_transport"]["errors"][0]["code"],
        json!("local_evidence_incomplete")
    );
    let remote_head = Command::new("git")
        .arg("--git-dir")
        .arg(remote.path())
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .expect("inspect remote main");
    assert!(
        !remote_head.status.success(),
        "retry pushed before revalidating the live projection"
    );
}

#[test]
fn complete_requires_declared_evidence() {
    let fixture = projected_fixture();
    let remote = common::TestDir::new("convergence-required-visibility-remote");
    git(remote.path(), &["init", "--bare"]);
    let remote_path = remote.path().to_str().expect("remote path");
    git(
        fixture.root.path(),
        &["remote", "add", "origin", remote_path],
    );
    rewrite_fixture_agent(&fixture, "cursor");
    change_source(&fixture, "unsupported visibility evidence\n");
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let (output, plan) = common::run_loom(
        fixture.root.path(),
        &[
            "plan",
            "converge",
            "demo",
            "--agent",
            "cursor",
            "--workspace",
            workspace,
            "--profile",
            "default",
            "--require-runtime",
            "--push-remote",
        ],
    );
    assert!(output.status.success(), "plan failed: {plan}");

    let (output, applied) = apply_plan(&fixture, &plan, "missing-evidence", &[]);
    assert!(output.status.success(), "partial apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(
        data["convergence"]["visibility"]["state"],
        json!("unsupported")
    );
    assert_eq!(data["complete"], json!(false));
    assert_eq!(
        data["completion_blockers"],
        json!(["registry.remote_pending", "visibility.evidence_incomplete"])
    );
    assert_eq!(data["outcome"], json!("local_complete_evidence_incomplete"));
    assert_eq!(
        data["convergence"]["registry_transport"]["errors"][0]["code"],
        json!("local_evidence_incomplete")
    );
    let remote_head = Command::new("git")
        .arg("--git-dir")
        .arg(remote.path())
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .expect("inspect remote main");
    assert!(
        !remote_head.status.success(),
        "required visibility failure must block remote transport"
    );
}

#[test]
fn complete_requires_the_exact_planned_projection_set() {
    let fixture = projected_fixture();
    let (_, second_instance) = add_copy_projection(&fixture, "second-post-local-target");
    change_source(&fixture, "two exact projection effects\n");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    assert_eq!(plan["data"]["effects"].as_array().map(Vec::len), Some(2));

    let planned_ids = plan["data"]["effects"]
        .as_array()
        .expect("planned effects")
        .iter()
        .map(|effect| effect["instance_id"].as_str().expect("planned instance"))
        .collect::<BTreeSet<_>>();
    assert!(planned_ids.contains(second_instance.as_str()));

    let (output, applied) = apply_plan(&fixture, &plan, "exact-projection-set", &[]);
    assert!(output.status.success(), "apply failed: {applied}");
    let projections = &applied["data"]["convergence"]["projections"];
    assert_eq!(applied["data"]["complete"], json!(true));
    assert_eq!(projections["evidence"]["selected_count"], json!(2));
    let observed_ids = projections["items"]
        .as_array()
        .expect("projection evidence")
        .iter()
        .map(|item| item["instance_id"].as_str().expect("observed instance"))
        .collect::<BTreeSet<_>>();
    assert_eq!(observed_ids, planned_ids);
}

#[test]
fn runtime_required_derives_a_single_unambiguous_agent() {
    let fixture = projected_fixture();
    change_source(&fixture, "derived runtime agent\n");
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let (output, plan) = common::run_loom(
        fixture.root.path(),
        &[
            "plan",
            "converge",
            "demo",
            "--workspace",
            workspace,
            "--profile",
            "default",
            "--require-runtime",
        ],
    );
    assert!(output.status.success(), "plan failed: {plan}");
    assert_eq!(plan["data"]["selectors"]["agent"], json!("claude"));
    assert_eq!(plan["data"]["safe_to_apply"], json!(true));

    let (output, applied) = apply_plan(&fixture, &plan, "derived-runtime-agent", &[]);
    assert!(output.status.success(), "apply failed: {applied}");
    assert_eq!(
        applied["data"]["completion_blockers"],
        json!(["visibility.restart_required"])
    );
    assert_eq!(
        applied["data"]["convergence"]["visibility"]["state"],
        json!("restart_required")
    );
}

#[test]
fn runtime_required_rejects_multiple_agents_without_a_selector() {
    let fixture = projected_fixture();
    let cursor_target = common::TestDir::new("convergence-cursor-target");
    let (output, target) = target_add(
        fixture.root.path(),
        "cursor",
        cursor_target.path(),
        "managed",
    );
    assert!(output.status.success(), "target add failed: {target}");
    let target_id = target["data"]["target"]["target_id"]
        .as_str()
        .expect("cursor target id");
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let (output, binding) = binding_add(
        fixture.root.path(),
        "cursor",
        "default",
        "exact-path",
        workspace,
        target_id,
    );
    assert!(output.status.success(), "binding add failed: {binding}");
    let binding_id = binding["data"]["binding"]["binding_id"]
        .as_str()
        .expect("cursor binding id");
    let (output, projection) = skill_project(fixture.root.path(), "demo", binding_id, Some("copy"));
    assert!(output.status.success(), "project failed: {projection}");

    change_source(&fixture, "ambiguous runtime agents\n");
    let (output, plan) = common::run_loom(
        fixture.root.path(),
        &[
            "plan",
            "converge",
            "demo",
            "--workspace",
            workspace,
            "--profile",
            "default",
            "--require-runtime",
        ],
    );
    assert!(output.status.success(), "plan failed: {plan}");
    assert_eq!(plan["data"]["safe_to_apply"], json!(false));
    assert!(
        plan["data"]["conflicts"]
            .as_array()
            .is_some_and(|conflicts| conflicts
                .iter()
                .any(|conflict| conflict["code"] == json!("RUNTIME_AGENT_AMBIGUOUS"))),
        "missing runtime-agent conflict: {plan}"
    );
}

#[test]
fn remote_ahead_preserves_recorded_commit_evidence() {
    let fixture = projected_fixture();
    let remote = common::TestDir::new("convergence-ahead-remote");
    let peer = common::TestDir::new("convergence-ahead-peer");
    git(remote.path(), &["init", "--bare"]);
    let remote_path = remote.path().to_str().expect("remote path");
    git(
        fixture.root.path(),
        &["remote", "add", "origin", remote_path],
    );
    git(fixture.root.path(), &["push", "origin", "HEAD:main"]);
    git(
        peer.path(),
        &["clone", "--branch", "main", remote_path, "."],
    );
    git(peer.path(), &["config", "user.email", "test@example.com"]);
    git(peer.path(), &["config", "user.name", "Test User"]);
    fs::write(peer.path().join("remote-only.txt"), "remote advanced\n").expect("remote edit");
    git(peer.path(), &["add", "remote-only.txt"]);
    git(peer.path(), &["commit", "-m", "test: advance remote"]);
    git(peer.path(), &["push", "origin", "HEAD:main"]);

    change_source(&fixture, "local convergence after remote advance\n");
    let (output, plan) = plan_converge(&fixture, &["--push-remote"]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, applied) = apply_plan(&fixture, &plan, "remote-ahead", &[]);
    assert!(output.status.success(), "partial apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(data["complete"], json!(false));
    assert_eq!(
        data["convergence"]["registry_transport"]["state"],
        json!("PENDING_PUSH")
    );
    assert_eq!(
        data["convergence"]["registry_transport"]["errors"][0]["code"],
        json!("REMOTE_DIVERGED")
    );
    let source_commit = data["source"]["commit"].as_str().expect("source commit");
    let registry_commit = data["applied"]["registry_commit"]
        .as_str()
        .expect("registry commit");
    let ancestor = Command::new("git")
        .current_dir(fixture.root.path())
        .args(["merge-base", "--is-ancestor", source_commit, "HEAD"])
        .output()
        .expect("check local source ancestry");
    assert!(
        ancestor.status.success(),
        "recorded source commit was rewritten"
    );
    let registry_ancestor = Command::new("git")
        .current_dir(fixture.root.path())
        .args(["merge-base", "--is-ancestor", registry_commit, "HEAD"])
        .output()
        .expect("check local registry ancestry");
    assert!(
        registry_ancestor.status.success(),
        "recorded registry commit was rewritten"
    );
    let remote_ancestor = Command::new("git")
        .arg("--git-dir")
        .arg(remote.path())
        .args([
            "merge-base",
            "--is-ancestor",
            source_commit,
            "refs/heads/main",
        ])
        .output()
        .expect("check remote source ancestry");
    assert!(
        !remote_ancestor.status.success(),
        "diverged transport must not push or rewrite convergence evidence"
    );
    let remote_registry_ancestor = Command::new("git")
        .arg("--git-dir")
        .arg(remote.path())
        .args([
            "merge-base",
            "--is-ancestor",
            registry_commit,
            "refs/heads/main",
        ])
        .output()
        .expect("check remote registry ancestry");
    assert!(
        !remote_registry_ancestor.status.success(),
        "diverged transport must not push recorded registry evidence"
    );

    let (sync_output, sync) = common::run_loom(fixture.root.path(), &["sync", "pull"]);
    assert!(
        sync_output.status.success(),
        "ordinary sync pull should reproduce the evidence rewrite risk: {sync}"
    );
    let rewritten_source = Command::new("git")
        .current_dir(fixture.root.path())
        .args(["merge-base", "--is-ancestor", source_commit, "HEAD"])
        .output()
        .expect("check rewritten source ancestry");
    assert!(
        !rewritten_source.status.success(),
        "fixture must rewrite the original convergence commit"
    );

    let (retry_output, retry) = apply_plan(&fixture, &plan, "remote-ahead", &[]);
    assert!(
        !retry_output.status.success(),
        "remote retry pushed rewritten evidence: {retry}"
    );
    assert_eq!(
        retry["error"]["details"]["conflict"]["code"],
        json!("CONVERGENCE_COMMIT_EVIDENCE_STALE")
    );
}

fn change_source(fixture: &Fixture, body: &str) {
    fs::write(fixture.root.path().join("skills/demo/details.txt"), body).expect("edit source");
}

fn convergence_operation_count(root: &Path) -> usize {
    common::operations_log(root)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|operation| operation["intent"] == json!("skill.converge"))
        .count()
}

fn rewrite_fixture_agent(fixture: &Fixture, agent: &str) {
    for (file, key) in [("targets.json", "targets"), ("bindings.json", "bindings")] {
        let path = fixture.root.path().join("state/registry").join(file);
        let mut value: Value =
            serde_json::from_slice(&fs::read(&path).expect("read registry file"))
                .expect("parse registry file");
        for row in value[key].as_array_mut().expect("registry rows") {
            row["agent"] = json!(agent);
        }
        fs::write(
            &path,
            serde_json::to_vec_pretty(&value).expect("encode registry file"),
        )
        .expect("rewrite registry file");
    }
    git(fixture.root.path(), &["add", "state/registry"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: use generic visibility adapter"],
    );
}
