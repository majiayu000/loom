use std::fs;

use common::run_loom_with_env;

use super::*;

fn apply_with_fault(fixture: &Fixture, plan: &Value, key: &str, fault: Option<&str>) -> Value {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("plan digest");
    let env = fault
        .map(|value| vec![("LOOM_FAULT_INJECT", value)])
        .unwrap_or_default();
    let (_, body) = run_loom_with_env(
        fixture.root.path(),
        &env,
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            key,
        ],
    );
    body
}

#[test]
fn committing_source_rejects_same_subject_wrong_tree() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "intended source\n",
    )
    .expect("source edit");
    let (_, plan) = plan_converge(&fixture, &[]);
    let fault = "convergence_interrupt_committing_source";
    let interrupted = apply_with_fault(&fixture, &plan, "same-subject-source", Some(fault));
    assert!(
        interrupted.get("error").is_some(),
        "fault did not interrupt"
    );
    let parent = git(fixture.root.path(), &["rev-parse", "HEAD^"]);
    git(fixture.root.path(), &["reset", "--soft", parent.trim()]);
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "wrong source tree\n",
    )
    .expect("wrong source");
    git(fixture.root.path(), &["add", "skills/demo"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "skill(demo): converge source"],
    );
    let wrong_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let rejected = apply_with_fault(&fixture, &plan, "same-subject-source", None);
    assert!(
        rejected.get("error").is_some(),
        "wrong source commit recovered"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), wrong_head);
}

#[test]
fn committing_registry_rejects_same_subject_extra_scoped_path() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "registry boundary source\n",
    )
    .expect("source edit");
    let (_, plan) = plan_converge(&fixture, &[]);
    let fault = "convergence_interrupt_committing_registry";
    let interrupted = apply_with_fault(&fixture, &plan, "same-subject-registry", Some(fault));
    assert!(
        interrupted.get("error").is_some(),
        "fault did not interrupt"
    );
    let source_head = git(fixture.root.path(), &["rev-parse", "HEAD^"]);
    git(
        fixture.root.path(),
        &["reset", "--soft", source_head.trim()],
    );
    fs::write(
        fixture.root.path().join("state/registry/unexpected.json"),
        "{}\n",
    )
    .expect("extra registry path");
    git(
        fixture.root.path(),
        &["add", "state/registry/unexpected.json"],
    );
    git(
        fixture.root.path(),
        &[
            "commit",
            "-m",
            "skill(demo): record convergence projections",
        ],
    );
    let wrong_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let rejected = apply_with_fault(&fixture, &plan, "same-subject-registry", None);
    assert!(
        rejected.get("error").is_some(),
        "wrong registry commit recovered"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), wrong_head);
}
