use std::fs;

use serde_json::{Value, json};

use super::skill_convergence_executor::apply_plan;
use super::*;

fn source_only_fixture() -> Fixture {
    let root = TestDir::new("convergence-source-only");
    let workspace = TestDir::new("convergence-source-only-workspace");
    let target = TestDir::new("convergence-source-only-target");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing source-only convergence.\n---\n# demo\n",
    );
    git(root.path(), &["init"]);
    git(root.path(), &["config", "user.email", "tests@example.com"]);
    git(root.path(), &["config", "user.name", "Loom Tests"]);
    git(root.path(), &["add", "skills/demo"]);
    git(
        root.path(),
        &["commit", "-m", "test: add source-only skill"],
    );
    Fixture {
        root,
        workspace,
        target,
    }
}

fn source_only_plan(fixture: &Fixture) -> (std::process::Output, Value) {
    run_loom(fixture.root.path(), &["plan", "converge", "demo"])
}

#[test]
fn source_only_and_required_runtime() {
    let fixture = source_only_fixture();
    let (output, source_only) = source_only_plan(&fixture);
    assert!(
        output.status.success(),
        "source-only plan failed: {source_only}"
    );
    assert_eq!(source_only["data"]["effects"], json!([]));
    assert_eq!(
        source_only["data"]["projection_state"],
        json!("not_applicable")
    );

    let (output, required) = run_loom(
        fixture.root.path(),
        &["plan", "converge", "demo", "--require-runtime"],
    );
    assert!(
        output.status.success(),
        "blocked plan should remain reviewable: {required}"
    );
    assert_eq!(required["data"]["safe_to_apply"], json!(false));
    assert_eq!(
        required["data"]["conflicts"][0]["code"],
        json!("RUNTIME_PROJECTION_REQUIRED")
    );
    assert_eq!(
        required["data"]["required_axes"],
        json!(["projections", "visibility"])
    );
}

#[test]
fn uninitialized_source_only_apply_and_recovery() {
    for interrupt in [false, true] {
        let fixture = source_only_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Use when testing source-only convergence.\n---\n# changed\n",
        )
        .expect("edit source-only skill");
        let (output, plan) = source_only_plan(&fixture);
        assert!(output.status.success(), "source-only plan failed: {plan}");
        assert_eq!(plan["data"]["safe_to_apply"], json!(true));
        assert!(!fixture.root.path().join("state/registry").exists());

        let key = if interrupt {
            "source-interrupt"
        } else {
            "source-apply"
        };
        if interrupt {
            let (output, stopped) = apply_plan(
                &fixture,
                &plan,
                key,
                &[(
                    "LOOM_FAULT_INJECT",
                    "convergence_interrupt_after_source_commit",
                )],
            );
            assert!(!output.status.success(), "interrupt applied: {stopped}");
        }
        let (output, applied) = apply_plan(&fixture, &plan, key, &[]);
        assert!(
            output.status.success(),
            "source-only apply failed: {applied}"
        );
        assert_eq!(
            applied["data"]["applied"]["projection_instances"],
            json!([])
        );
        assert_eq!(
            git(
                fixture.root.path(),
                &[
                    "rev-list",
                    "--count",
                    "--grep=skill(demo): converge source",
                    "HEAD"
                ]
            )
            .trim(),
            "1"
        );
        assert!(!fixture.root.path().join("state/registry").exists());
        super::skill_convergence_ledger_assertions::assert_exact_retained_ledger(
            &fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json"),
            "committed_artifacts_retained",
        );
    }
}

#[test]
fn initialized_source_only_apply_uses_semantic_noop_registry_cas() {
    let fixture = source_only_fixture();
    let (output, target) = target_add(
        fixture.root.path(),
        "claude",
        fixture.target.path(),
        "managed",
    );
    assert!(
        output.status.success(),
        "registry initialization failed: {target}"
    );
    fs::write(
        fixture.root.path().join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Use when testing source-only convergence.\n---\n# initialized change\n",
    )
    .expect("source edit");
    let projections_path = fixture.root.path().join("state/registry/projections.json");
    let projections_before = fs::read(&projections_path).expect("projections before");
    let (output, plan) = source_only_plan(&fixture);
    assert!(output.status.success(), "source-only plan failed: {plan}");
    assert_eq!(plan["data"]["effects"], json!([]));
    assert_eq!(plan["data"]["safe_to_apply"], json!(true));

    let (output, applied) = apply_plan(&fixture, &plan, "initialized-source-only", &[]);
    assert!(
        output.status.success(),
        "source-only apply failed: {applied}"
    );
    assert!(applied["data"]["applied"]["source_commit"].is_string());
    assert!(applied["data"]["applied"]["registry_commit"].is_null());
    assert_eq!(
        fs::read(&projections_path).expect("projections after"),
        projections_before
    );
    assert!(
        !projections_path
            .with_extension("loom-cas-candidate")
            .exists()
    );
    assert!(!projections_path.with_extension("loom-cas-journal").exists());
}

#[test]
fn source_only_policy_drift_is_rejected_before_mutation() {
    for relative in [
        "state/registry/trust.json",
        "state/registry/sources.json",
        "loom.lock",
    ] {
        let fixture = source_only_fixture();
        let (output, target) = target_add(
            fixture.root.path(),
            "claude",
            fixture.target.path(),
            "managed",
        );
        assert!(output.status.success(), "registry setup failed: {target}");
        fs::write(
            fixture.root.path().join("state/registry/trust.json"),
            "{\"schema_version\":1,\"skills\":[]}\n",
        )
        .expect("trust baseline");
        fs::write(
            fixture.root.path().join("state/registry/sources.json"),
            "{\"schema_version\":1,\"sources\":[]}\n",
        )
        .expect("sources baseline");
        fs::write(
            fixture.root.path().join("loom.lock"),
            "{\"version\":1,\"skills\":{}}\n",
        )
        .expect("lock baseline");
        git(
            fixture.root.path(),
            &[
                "add",
                "state/registry/trust.json",
                "state/registry/sources.json",
                "loom.lock",
            ],
        );
        git(
            fixture.root.path(),
            &["commit", "-m", "test: add policy evidence"],
        );
        let (output, plan) = source_only_plan(&fixture);
        assert!(output.status.success(), "plan failed: {plan}");
        let path = fixture.root.path().join(relative);
        let mut bytes = fs::read(&path).expect("policy evidence");
        bytes.push(b'\n');
        fs::write(&path, bytes).expect("drift policy evidence");

        let (output, rejected) = apply_plan(&fixture, &plan, relative, &[]);
        assert!(!output.status.success(), "policy drift applied: {rejected}");
        assert_eq!(
            rejected["error"]["details"]["conflict"]["code"],
            json!("PLAN_CHECKPOINT_DRIFT")
        );
        assert!(
            !fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json")
                .exists()
        );
    }
}

#[test]
fn retained_replay_reproves_source_and_allows_unrelated_descendants() {
    for source_drift in [false, true] {
        let fixture = source_only_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: retained replay proof.\n---\n# committed\n",
        )
        .expect("source edit");
        let (output, plan) = source_only_plan(&fixture);
        assert!(output.status.success(), "plan failed: {plan}");
        let key = if source_drift {
            "retained-drift"
        } else {
            "retained-descendant"
        };
        let (output, applied) = apply_plan(&fixture, &plan, key, &[]);
        assert!(output.status.success(), "apply failed: {applied}");
        let journal_path = fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json");
        let retained = fs::read(&journal_path).expect("retained journal");

        if source_drift {
            fs::write(
                fixture.root.path().join("skills/demo/SKILL.md"),
                "---\nname: demo\ndescription: retained replay proof.\n---\n# drift\n",
            )
            .expect("source drift");
        } else {
            fs::write(fixture.root.path().join("unrelated.txt"), "unrelated\n")
                .expect("unrelated file");
            git(fixture.root.path(), &["add", "unrelated.txt"]);
            git(
                fixture.root.path(),
                &["commit", "-m", "test: unrelated retained descendant"],
            );
        }
        let replay_key = format!("{key}-replay");
        let (output, replayed) = apply_plan(&fixture, &plan, &replay_key, &[]);
        assert_eq!(
            output.status.success(),
            !source_drift,
            "unexpected retained replay result: {replayed}"
        );
        assert_eq!(
            fs::read(&journal_path).expect("journal after replay"),
            retained
        );
    }
}

#[test]
fn durable_registry_noop_accepts_only_unchanged_descendants() {
    for managed_drift in [false, true] {
        let fixture = source_only_fixture();
        let (output, target) = target_add(
            fixture.root.path(),
            "claude",
            fixture.target.path(),
            "managed",
        );
        assert!(output.status.success(), "registry setup failed: {target}");
        fs::write(
            fixture.root.path().join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: durable registry no-op.\n---\n# changed\n",
        )
        .expect("source edit");
        let (output, plan) = source_only_plan(&fixture);
        assert!(output.status.success(), "plan failed: {plan}");
        let key = if managed_drift {
            "noop-managed-drift"
        } else {
            "noop-descendant"
        };
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
        let journal_path = fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json");
        let retained = fs::read(&journal_path).expect("interrupted journal");
        let relative = if managed_drift {
            "state/registry/trust.json"
        } else {
            "unrelated.txt"
        };
        let path = fixture.root.path().join(relative);
        if managed_drift {
            fs::write(&path, "{\"schema_version\":1,\"skills\":[]}\n").expect("trust drift");
        } else {
            fs::write(&path, "unrelated\n").expect("unrelated file");
        }
        git(fixture.root.path(), &["add", relative]);
        git(
            fixture.root.path(),
            &["commit", "-m", "test: descendant during no-op"],
        );

        let replay_key = format!("{key}-replay");
        let (output, recovered) = apply_plan(&fixture, &plan, &replay_key, &[]);
        assert_eq!(
            output.status.success(),
            !managed_drift,
            "unexpected no-op recovery result: {recovered}; journal: {}",
            String::from_utf8_lossy(&retained)
        );
        if managed_drift {
            assert_eq!(
                fs::read(&journal_path).expect("preserved journal"),
                retained
            );
        } else {
            assert!(recovered["data"]["applied"]["registry_commit"].is_null());
        }
    }
}
