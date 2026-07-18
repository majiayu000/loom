use std::fs;

use serde_json::json;

use super::*;
use common::run_loom_with_env;

pub(super) fn all_paths(root: &Path) -> Vec<String> {
    fn visit(base: &Path, path: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("path entry").path();
            out.push(
                path.strip_prefix(base)
                    .expect("relative path")
                    .display()
                    .to_string(),
            );
            if path.is_dir() {
                visit(base, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    visit(root, root, &mut out);
    out.sort();
    out
}

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

#[test]
fn artifact_collisions_preserve_unowned_paths() {
    for kind in ["index", "backup", "projection-stage"] {
        let fixture = projected_fixture();
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
        let collision = match kind {
            "index" => fixture
                .root
                .path()
                .join(format!("state/transactions/{plan_id}-artifacts/index")),
            "backup" => fixture.root.path().join(format!(
                "state/transactions/{plan_id}-artifacts/projection-0"
            )),
            "projection-stage" => fixture
                .target
                .path()
                .join(format!(".loom-projection-stage-{plan_id}-0.owner/stage")),
            _ => unreachable!(),
        };
        fs::create_dir_all(collision.parent().expect("collision parent")).expect("parent");
        fs::write(&collision, format!("unowned-{kind}\n")).expect("collision");
        let (output, failed) = apply_plan(&fixture, &plan, kind, &[]);
        assert!(
            !output.status.success(),
            "{kind} collision applied: {failed}"
        );
        assert_eq!(
            fs::read_to_string(&collision).expect("collision preserved"),
            format!("unowned-{kind}\n")
        );
    }
}

#[test]
fn source_staging_collision_is_preserved() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    fs::write(
        fixture.target.path().join("demo/details.txt"),
        "projection input\n",
    )
    .expect("dirty projection");
    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "projection plan failed: {plan}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let collision = fixture.root.path().join(format!(
        "skills/.loom-convergence-source-stage-{plan_id}.owner/stage"
    ));
    fs::create_dir_all(collision.parent().expect("collision parent")).expect("parent");
    fs::write(&collision, "unowned-source-stage\n").expect("collision");
    let (output, failed) = apply_plan(&fixture, &plan, "source-stage", &[]);
    assert!(
        !output.status.success(),
        "source collision applied: {failed}"
    );
    assert_eq!(
        fs::read_to_string(&collision).expect("preserved"),
        "unowned-source-stage\n"
    );
}

#[test]
fn transaction_directories_are_removed() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, applied) = apply_plan(&fixture, &plan, "directory-cleanup", &[]);
    assert!(output.status.success(), "apply failed: {applied}");
    assert!(
        all_paths(&fixture.root.path().join("state/transactions")).is_empty(),
        "transaction directories remain"
    );
}

fn assert_unrelated_commit_is_not_recovery(boundary: &str) {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "source edit\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        boundary,
        &[("LOOM_FAULT_INJECT", boundary)],
    );
    assert!(
        !output.status.success(),
        "boundary did not interrupt: {interrupted}"
    );
    fs::write(fixture.root.path().join("unrelated.txt"), "unrelated\n").expect("unrelated");
    git(fixture.root.path(), &["add", "unrelated.txt"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: unrelated intervening commit"],
    );
    let unrelated_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let (output, rejected) = apply_plan(&fixture, &plan, boundary, &[]);
    assert!(
        !output.status.success(),
        "unrelated commit classified as recovery: {rejected}"
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        unrelated_head
    );
    assert!(
        fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json")
            .is_file()
    );
}

#[test]
fn committing_source_rejects_unrelated_head() {
    assert_unrelated_commit_is_not_recovery("convergence_interrupt_committing_source");
}

#[test]
fn committing_registry_rejects_unrelated_head() {
    assert_unrelated_commit_is_not_recovery("convergence_interrupt_committing_registry");
}

#[test]
fn owner_reservation_interruptions_recover_without_orphans() {
    for fault in [
        "convergence_interrupt_after_owner_root_creation",
        "convergence_interrupt_after_owner_marker_write",
    ] {
        let fixture = projected_fixture();
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let (output, interrupted) =
            apply_plan(&fixture, &plan, fault, &[("LOOM_FAULT_INJECT", fault)]);
        assert!(
            !output.status.success(),
            "reservation fault passed: {interrupted}"
        );
        assert!(
            fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json")
                .is_file()
        );
        let (output, recovered) = apply_plan(&fixture, &plan, fault, &[]);
        assert!(
            output.status.success(),
            "reservation recovery failed: {recovered}"
        );
        assert!(
            all_paths(&fixture.root.path().join("state/transactions")).is_empty(),
            "reservation recovery left transaction paths"
        );
        assert!(
            all_paths(&fixture.root.path().join("skills"))
                .iter()
                .all(|path| !path.contains("reservation-") && !path.contains("staging-")),
            "reservation recovery left source staging paths"
        );
    }
}

#[test]
fn journal_validation_precedes_all_cleanup() {
    for mutation in ["artifact-root", "unknown-phase"] {
        let fixture = projected_fixture();
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let fault = "convergence_interrupt_after_owner_marker_write";
        let (output, interrupted) =
            apply_plan(&fixture, &plan, mutation, &[("LOOM_FAULT_INJECT", fault)]);
        assert!(!output.status.success(), "fault passed: {interrupted}");
        let journal_path = fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json");
        let mut journal: Value = serde_json::from_slice(&fs::read(&journal_path).expect("journal"))
            .expect("parse journal");
        let sentinel = fixture.root.path().join("unowned-sentinel");
        fs::create_dir(&sentinel).expect("sentinel root");
        fs::write(
            sentinel.join(".owner"),
            format!("{}\n", plan["data"]["plan_id"].as_str().expect("plan id")),
        )
        .expect("sentinel owner bait");
        fs::write(sentinel.join("keep"), "unowned\n").expect("sentinel");
        if mutation == "artifact-root" {
            journal["artifact_root"] = json!(sentinel.display().to_string());
        } else {
            journal["phase"] = json!("future_cleanup_phase");
        }
        fs::write(
            &journal_path,
            serde_json::to_vec_pretty(&journal).expect("encode journal"),
        )
        .expect("mutate journal");
        let before = all_paths(fixture.root.path());
        let (output, rejected) = apply_plan(&fixture, &plan, mutation, &[]);
        assert!(
            !output.status.success(),
            "malicious journal applied: {rejected}"
        );
        assert_eq!(
            all_paths(fixture.root.path()),
            before,
            "validation mutated paths"
        );
        assert_eq!(
            fs::read_to_string(sentinel.join("keep")).expect("sentinel"),
            "unowned\n"
        );
    }
}

#[test]
fn rolled_back_cleanup_pending_retries_cleanup_only() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "rollback cleanup source\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let original_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        "rollback-cleanup",
        &[
            ("LOOM_FAULT_INJECT", "convergence_after_registry_save"),
            (
                "LOOM_CLEANUP_FAULT_INJECT",
                "convergence_interrupt_during_cleanup",
            ),
        ],
    );
    assert!(!output.status.success(), "rollback fault passed: {failed}");
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        original_head
    );
    let journal: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json"),
        )
        .expect("journal"),
    )
    .expect("parse journal");
    assert_eq!(journal["phase"], json!("rolled_back_cleanup_pending"));
    let (output, recovered) = apply_plan(&fixture, &plan, "rollback-cleanup", &[]);
    assert!(output.status.success(), "cleanup retry failed: {recovered}");
    assert_eq!(
        git(
            fixture.root.path(),
            &[
                "rev-list",
                "--count",
                "--grep=skill(demo): converge source",
                "HEAD"
            ],
        )
        .trim(),
        "1"
    );
}

#[test]
fn no_op_source_commit_matches_direct_and_recovery() {
    let direct = projected_fixture();
    let (output, direct_plan) = plan_converge(&direct, &[]);
    assert!(output.status.success(), "direct plan failed: {direct_plan}");
    let (output, direct_result) = apply_plan(&direct, &direct_plan, "direct-noop", &[]);
    assert!(
        output.status.success(),
        "direct apply failed: {direct_result}"
    );

    let recovered = projected_fixture();
    let (output, recovered_plan) = plan_converge(&recovered, &[]);
    assert!(
        output.status.success(),
        "recovery plan failed: {recovered_plan}"
    );
    let fault = "convergence_interrupt_committing_source";
    let (output, interrupted) = apply_plan(
        &recovered,
        &recovered_plan,
        "recovered-noop",
        &[("LOOM_FAULT_INJECT", fault)],
    );
    assert!(
        !output.status.success(),
        "no-op boundary passed: {interrupted}"
    );
    let (output, recovered_result) = apply_plan(&recovered, &recovered_plan, "recovered-noop", &[]);
    assert!(
        output.status.success(),
        "no-op recovery failed: {recovered_result}"
    );
    assert_eq!(
        direct_result["data"]["applied"]["source_commit"],
        Value::Null
    );
    assert_eq!(
        recovered_result["data"]["applied"]["source_commit"],
        Value::Null
    );
}
