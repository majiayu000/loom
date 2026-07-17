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
    assert!(
        fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json")
            .is_file()
    );

    let (output, recovered) = apply_plan(&fixture, &plan, "interrupt", &[]);
    assert!(output.status.success(), "recovery failed: {recovered}");
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
