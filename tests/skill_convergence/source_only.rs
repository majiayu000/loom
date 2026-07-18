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
