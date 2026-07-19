use super::*;
use crate::skill_convergence_executor::apply_plan;

fn apply_plan_without_json(
    fixture: &Fixture,
    plan: &Value,
    key: &str,
    envs: &[(&str, &str)],
) -> std::process::Output {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("plan digest");
    let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
    command
        .current_dir(fixture.root.path())
        .arg("--json")
        .arg("--root")
        .arg(fixture.root.path())
        .args([
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            key,
        ]);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run crashing loom apply")
}

#[test]
fn post_publication_failure_keeps_recovery_phase_and_replays_the_lock() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "retained source index lock\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let key = "retained-source-index-lock";
    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        key,
        &[("LOOM_TEST_PREPARED_INDEX_FAIL_AFTER_PUBLICATION", "1")],
    );
    assert!(
        !output.status.success(),
        "test failure was ignored: {failed}"
    );
    assert_eq!(
        failed["error"]["details"]["index_lock_retained"],
        json!(true)
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("committing_source"));
    assert!(fixture.root.path().join(".git/index.lock").is_file());

    let (output, recovered) = apply_plan(&fixture, &plan, key, &[]);
    assert!(output.status.success(), "lock recovery failed: {recovered}");
    assert!(!fixture.root.path().join(".git/index.lock").exists());
}

#[test]
fn registry_post_guard_failure_recovers_the_moved_head_and_published_lock() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "retained registry index lock\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let key = "retained-registry-index-lock";
    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        key,
        &[("LOOM_TEST_REGISTRY_INDEX_FAIL_AFTER_GUARD", "1")],
    );
    assert!(
        !output.status.success(),
        "test failure was ignored: {failed}"
    );
    assert_eq!(
        failed["error"]["details"]["index_lock_retained"],
        json!(true)
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("committing_registry"));
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]).trim(),
        journal["registry_commit"]
            .as_str()
            .expect("registry commit")
    );
    assert!(fixture.root.path().join(".git/index.lock").is_file());

    let (output, recovered) = apply_plan(&fixture, &plan, key, &[]);
    assert!(
        output.status.success(),
        "registry recovery failed: {recovered}"
    );
    assert!(!fixture.root.path().join(".git/index.lock").exists());
}

#[test]
fn registry_lock_recovery_rejects_a_tampered_commit_boundary() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "tampered registry recovery boundary\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let key = "tampered-registry-recovery-boundary";
    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        key,
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_before_registry_cas",
        )],
    );
    assert!(
        !output.status.success(),
        "test failure was ignored: {failed}"
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let mut journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("committing_registry"));
    let source_head = journal["source_head"]
        .as_str()
        .expect("source head")
        .to_string();
    let previous_head = journal["previous_head"]
        .as_str()
        .expect("previous head")
        .to_string();
    assert_ne!(source_head, previous_head);
    journal["registry_commit"] = json!(previous_head);
    fs::write(
        &journal_path,
        serde_json::to_vec_pretty(&journal).expect("encode journal"),
    )
    .expect("tamper journal registry commit");

    let (output, rejected) = apply_plan(&fixture, &plan, key, &[]);
    assert!(
        !output.status.success(),
        "tampered registry commit was adopted: {rejected}"
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]).trim(),
        source_head
    );
    assert!(fixture.root.path().join(".git/index.lock").is_file());
    assert!(journal_path.is_file());
}

#[test]
fn rolling_back_replays_a_published_index_restore_lock() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "rollback retained index lock\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let key = "rollback-retained-index-lock";
    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        key,
        &[
            ("LOOM_FAULT_INJECT", "convergence_after_registry_save"),
            ("LOOM_TEST_ROLLBACK_INDEX_FAIL_AFTER_PUBLICATION", "1"),
        ],
    );
    assert!(
        !output.status.success(),
        "rollback fault was ignored: {failed}"
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("rolling_back"));
    assert!(fixture.root.path().join(".git/index.lock").is_file());

    let (output, recovered) = apply_plan(&fixture, &plan, key, &[]);
    assert!(
        output.status.success(),
        "rollback recovery failed: {recovered}"
    );
    assert!(!fixture.root.path().join(".git/index.lock").exists());
}

#[test]
fn post_rename_crash_windows_resume_the_transaction() {
    for point in [
        "after_index_rename",
        "after_lock_capture",
        "after_claim_remove",
    ] {
        let fixture = projected_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            format!("post-rename crash at {point}\n"),
        )
        .expect("edit source");
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let key = format!("post-rename-crash-{point}");

        let crashed = apply_plan_without_json(
            &fixture,
            &plan,
            &key,
            &[("LOOM_TEST_PREPARED_INDEX_CRASH_POINT", point)],
        );
        assert_eq!(
            crashed.status.code(),
            Some(93),
            "crash point {point} did not fire: {}",
            String::from_utf8_lossy(&crashed.stderr)
        );

        let (output, recovered) = apply_plan(&fixture, &plan, &key, &[]);
        assert!(
            output.status.success(),
            "transaction did not recover after {point}: {recovered}"
        );
        assert!(!fixture.root.path().join(".git/index.lock").exists());
        assert!(
            fs::read_dir(fixture.root.path().join(".git"))
                .expect("read git directory")
                .all(|entry| !entry
                    .expect("git directory entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".loom-index-")),
            "private index state remained after {point}"
        );
    }
}

#[test]
fn post_publication_io_failures_keep_the_typed_recovery_route() {
    let scenarios: &[(&str, &[(&str, &str)])] = &[
        (
            "after-lock-link",
            &[("LOOM_TEST_PREPARED_INDEX_FAILURE_POINT", "after_lock_link")],
        ),
        (
            "before-guard-create",
            &[(
                "LOOM_TEST_PREPARED_INDEX_FAILURE_POINT",
                "before_guard_create",
            )],
        ),
        (
            "guard-cleanup",
            &[
                ("LOOM_TEST_PREPARED_INDEX_FAIL_AFTER_PUBLICATION", "1"),
                ("LOOM_TEST_PREPARED_INDEX_FAILURE_POINT", "guard_cleanup"),
            ],
        ),
    ];
    for (name, envs) in scenarios {
        let fixture = projected_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            format!("post-publication I/O failure at {name}\n"),
        )
        .expect("edit source");
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let key = format!("post-publication-io-{name}");

        let (output, failed) = apply_plan(&fixture, &plan, &key, envs);
        assert!(
            !output.status.success(),
            "failure {name} was ignored: {failed}"
        );
        assert_eq!(
            failed["error"]["details"]["index_lock_retained"],
            json!(true),
            "failure {name} lost the typed retained marker"
        );
        assert!(fixture.root.path().join(".git/index.lock").is_file());

        let (output, recovered) = apply_plan(&fixture, &plan, &key, &[]);
        assert!(
            output.status.success(),
            "failure {name} did not recover: {recovered}"
        );
        assert!(!fixture.root.path().join(".git/index.lock").exists());
    }
}
