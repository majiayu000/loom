use std::fs;

use serde_json::json;

use super::*;
use common::run_loom_with_env;

#[test]
fn symlink_copy_materialize() {
    let mut methods = vec!["copy", "materialize"];
    if cfg!(unix) {
        methods.push("symlink");
    }

    for method in methods {
        let fixture = projected_fixture_with_method(method);
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "{method} plan failed: {plan}");
        assert_eq!(plan["data"]["effects"][0]["method"], json!(method));
        assert_eq!(plan["data"]["effects"][0]["effect"], json!("refresh"));
        let (output, applied) = apply_plan(&fixture, &plan, &format!("apply-{method}"), &[]);
        assert!(output.status.success(), "{method} apply failed: {applied}");

        let projection = fixture.target.path().join("demo");
        match method {
            "symlink" => assert!(
                fs::symlink_metadata(&projection)
                    .expect("symlink projection")
                    .file_type()
                    .is_symlink()
            ),
            "copy" | "materialize" => {
                assert!(projection.is_dir(), "{method} projection must be a tree");
                assert!(projection.join("SKILL.md").is_file());
            }
            unexpected => panic!("unexpected projection method {unexpected}"),
        }
    }
}

fn apply_plan(
    fixture: &Fixture,
    plan: &Value,
    key: &str,
    envs: &[(&str, &str)],
) -> (std::process::Output, Value) {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("plan digest");
    run_loom_with_env(
        fixture.root.path(),
        envs,
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            key,
        ],
    )
}

#[test]
fn stale_plan_and_lock_contention() {
    let stale_fixture = projected_fixture();
    let (output, stale_plan) = plan_converge(&stale_fixture, &[]);
    assert!(output.status.success(), "plan failed: {stale_plan}");
    fs::write(
        stale_fixture.root.path().join("skills/demo/details.txt"),
        "source drift\n",
    )
    .expect("mutate source");
    let (output, stale) = apply_plan(&stale_fixture, &stale_plan, "stale", &[]);
    assert!(!output.status.success(), "stale plan applied: {stale}");
    assert_eq!(
        stale["error"]["details"]["conflict"]["code"],
        json!("PLAN_SOURCE_DRIFT")
    );

    let locked_fixture = projected_fixture();
    let (output, locked_plan) = plan_converge(&locked_fixture, &[]);
    assert!(output.status.success(), "plan failed: {locked_plan}");
    let locks = locked_fixture.root.path().join("state/locks");
    fs::create_dir_all(&locks).expect("create locks");
    fs::write(
        locks.join("workspace.lock"),
        format!(
            "{{\"pid\":{},\"owner_id\":\"held\",\"host\":\"other-host\",\"created_at\":\"{}\"}}\n",
            std::process::id(),
            chrono::Utc::now().to_rfc3339()
        ),
    )
    .expect("hold workspace lock");
    let (output, busy) = apply_plan(&locked_fixture, &locked_plan, "busy", &[]);
    assert!(!output.status.success(), "held lock bypassed: {busy}");
    assert!(
        busy["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("LOCK_BUSY"))
    );
}

#[test]
fn local_faults_restore_all_surfaces() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "locally edited source\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let status = git(fixture.root.path(), &["status", "--porcelain"]);
    let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let target = snapshot_tree(fixture.target.path());
    let registry = snapshot_tree(&fixture.root.path().join("state/registry"));

    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        "rollback",
        &[("LOOM_FAULT_INJECT", "convergence_after_registry_save")],
    );
    assert!(!output.status.success(), "fault did not fail: {failed}");
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(git(fixture.root.path(), &["status", "--porcelain"]), status);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        source
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry
    );
}

#[test]
fn interrupted_recovery_is_single_commit() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "interrupted source\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "interrupt",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_source_commit",
        )],
    );
    assert!(
        !output.status.success(),
        "interrupt did not stop: {interrupted}"
    );
    let interrupted_commit = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    assert!(
        fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json")
            .is_file()
    );

    let (output, recovered) = apply_plan(&fixture, &plan, "interrupt", &[]);
    assert!(output.status.success(), "recovery failed: {recovered}");
    assert_eq!(
        recovered["data"]["applied"]["source_commit"],
        json!(interrupted_commit.trim()),
        "recovery replaced the durable source commit"
    );
    git(
        fixture.root.path(),
        &[
            "merge-base",
            "--is-ancestor",
            interrupted_commit.trim(),
            "HEAD",
        ],
    );
    let count = git(
        fixture.root.path(),
        &[
            "rev-list",
            "--count",
            "--grep=skill(demo): converge source",
            "HEAD",
        ],
    );
    assert_eq!(count.trim(), "1", "source commit duplicated after recovery");
    assert!(
        !fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json")
            .exists()
    );
}

#[test]
fn pre_journal_backup_failure_cleans_artifacts() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "prepared source\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let status = git(fixture.root.path(), &["status", "--porcelain"]);
    let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let target = snapshot_tree(fixture.target.path());
    let registry = snapshot_tree(&fixture.root.path().join("state/registry"));
    let backups = snapshot_tree(&fixture.root.path().join("state/backups"));

    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        "prepare-fault",
        &[("LOOM_FAULT_INJECT", "convergence_during_backup_preparation")],
    );
    assert!(
        !output.status.success(),
        "preparation fault passed: {failed}"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(git(fixture.root.path(), &["status", "--porcelain"]), status);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        source
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/backups")),
        backups
    );
    assert!(
        snapshot_tree(&fixture.root.path().join("state/transactions")).is_empty(),
        "failed preparation left durable transaction artifacts"
    );
}

#[test]
fn partial_cleanup_retry_does_not_rollback_commit() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "cleanup-pending source\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "cleanup-pending",
        &[("LOOM_FAULT_INJECT", "convergence_interrupt_during_cleanup")],
    );
    assert!(
        !output.status.success(),
        "cleanup interrupt passed: {interrupted}"
    );
    let committed_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let committed_target = snapshot_tree(fixture.target.path());
    assert!(
        fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json")
            .is_file()
    );

    let (output, recovered) = apply_plan(&fixture, &plan, "cleanup-pending", &[]);
    assert!(
        output.status.success(),
        "cleanup recovery failed: {recovered}"
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        committed_head
    );
    assert_eq!(snapshot_tree(fixture.target.path()), committed_target);
    let count = git(
        fixture.root.path(),
        &[
            "rev-list",
            "--count",
            "--grep=skill(demo): converge source",
            "HEAD",
        ],
    );
    assert_eq!(
        count.trim(),
        "1",
        "cleanup recovery duplicated source commit"
    );
    assert!(
        snapshot_tree(&fixture.root.path().join("state/transactions")).is_empty(),
        "cleanup recovery left transaction artifacts"
    );
}

#[test]
fn missing_create_executes_with_effect_guard() {
    let fixture = projected_fixture();
    fs::remove_dir_all(fixture.target.path().join("demo")).expect("remove projection path");
    let path = fixture.root.path().join("state/registry/projections.json");
    let mut projections: Value =
        serde_json::from_slice(&fs::read(&path).expect("read projections")).expect("parse");
    projections["projections"] = json!([]);
    fs::write(
        &path,
        serde_json::to_vec_pretty(&projections).expect("encode"),
    )
    .expect("write");
    git(
        fixture.root.path(),
        &["add", "-A", "state/registry/projections.json"],
    );
    git(
        fixture.root.path(),
        &["commit", "-m", "test: remove projection record"],
    );
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "create plan failed: {plan}");
    assert_eq!(plan["data"]["effects"][0]["effect"], json!("create"));
    let (output, applied) = apply_plan(&fixture, &plan, "missing-create", &[]);
    assert!(
        output.status.success(),
        "missing/create apply failed: {applied}"
    );
    assert!(fixture.target.path().join("demo/SKILL.md").is_file());
}

#[test]
fn convergence_policy_is_fail_closed_before_t007() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    mutate_plan_event(fixture.root.path(), plan_id, |stored| {
        stored["safe_to_apply"] = json!(false);
    });
    let (output, blocked) = apply_plan(&fixture, &plan, "unsafe-policy", &[]);
    assert!(!output.status.success(), "unsafe policy applied: {blocked}");
    assert_eq!(
        blocked["error"]["details"]["conflict"]["code"],
        json!("CONVERGENCE_POLICY_WORKFLOW_REQUIRED")
    );
}

#[test]
fn required_approvals_are_fail_closed_before_t007() {
    let fixture = projected_fixture();
    write_skill(
        fixture.root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing convergence policy.\ncapabilities:\n  shell:\n    commands: [\"git\"]\n---\n# demo\n",
    );
    let (output, saved) = save_skill(fixture.root.path(), "demo");
    assert!(output.status.success(), "save failed: {saved}");
    let bindings: Value = serde_json::from_slice(
        &fs::read(fixture.root.path().join("state/registry/bindings.json")).expect("read bindings"),
    )
    .expect("parse bindings");
    let binding_id = bindings["bindings"][0]["binding_id"]
        .as_str()
        .expect("binding id");
    let (output, projected) = skill_project(fixture.root.path(), "demo", binding_id, Some("copy"));
    assert!(output.status.success(), "refresh failed: {projected}");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    assert!(
        !plan["data"]["required_approvals"]
            .as_array()
            .expect("approvals")
            .is_empty()
    );
    let (output, blocked) = apply_plan(&fixture, &plan, "approval-policy", &[]);
    assert!(
        !output.status.success(),
        "approval policy applied: {blocked}"
    );
    assert_eq!(
        blocked["error"]["details"]["conflict"]["code"],
        json!("CONVERGENCE_POLICY_WORKFLOW_REQUIRED")
    );
}
