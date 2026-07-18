use std::fs;

use serde_json::{Value, json};

use super::skill_convergence_executor::apply_plan;
use super::*;

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

    for lock_name in ["workspace.lock", "skill-demo.lock"] {
        let locked_fixture = projected_fixture();
        let (output, locked_plan) = plan_converge(&locked_fixture, &[]);
        assert!(output.status.success(), "plan failed: {locked_plan}");
        let locks = locked_fixture.root.path().join("state/locks");
        fs::create_dir_all(&locks).expect("create locks");
        fs::write(
            locks.join(lock_name),
            format!(
                "{{\"pid\":{},\"owner_id\":\"held\",\"host\":\"other-host\",\"created_at\":\"{}\"}}\n",
                std::process::id(),
                chrono::Utc::now().to_rfc3339()
            ),
        )
        .expect("hold convergence lock");
        let (output, busy) = apply_plan(&locked_fixture, &locked_plan, lock_name, &[]);
        assert!(
            !output.status.success(),
            "held {lock_name} bypassed: {busy}"
        );
        assert!(
            busy["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("LOCK_BUSY"))
        );
    }

    let head_fixture = projected_fixture();
    let (output, head_plan) = plan_converge(&head_fixture, &[]);
    assert!(output.status.success(), "plan failed: {head_plan}");
    git(
        head_fixture.root.path(),
        &["commit", "--allow-empty", "-m", "advance unrelated HEAD"],
    );
    let changed_head = git(head_fixture.root.path(), &["rev-parse", "HEAD"]);
    let (output, stale) = apply_plan(&head_fixture, &head_plan, "head-drift", &[]);
    assert!(!output.status.success(), "HEAD drift applied: {stale}");
    assert_eq!(
        stale["error"]["details"]["conflict"]["code"],
        json!("PLAN_STALE")
    );
    assert_eq!(
        git(head_fixture.root.path(), &["rev-parse", "HEAD"]),
        changed_head
    );

    let checkpoint_fixture = projected_fixture();
    let (output, checkpoint_plan) = plan_converge(&checkpoint_fixture, &[]);
    assert!(output.status.success(), "plan failed: {checkpoint_plan}");
    let checkpoint_path = checkpoint_fixture
        .root
        .path()
        .join("state/registry/ops/checkpoint.json");
    let mut checkpoint: Value =
        serde_json::from_slice(&fs::read(&checkpoint_path).expect("read registry checkpoint"))
            .expect("parse registry checkpoint");
    checkpoint["updated_at"] = json!("2000-01-01T00:00:00Z");
    let changed_checkpoint = serde_json::to_vec_pretty(&checkpoint).expect("encode checkpoint");
    fs::write(&checkpoint_path, &changed_checkpoint).expect("mutate registry checkpoint");
    let (output, stale) = apply_plan(
        &checkpoint_fixture,
        &checkpoint_plan,
        "checkpoint-drift",
        &[],
    );
    assert!(
        !output.status.success(),
        "checkpoint drift applied: {stale}"
    );
    assert_eq!(
        stale["error"]["details"]["conflict"]["code"],
        json!("PLAN_CHECKPOINT_DRIFT")
    );
    assert_eq!(
        fs::read(&checkpoint_path).expect("checkpoint after apply"),
        changed_checkpoint
    );

    let projection_fixture = projected_fixture();
    let (output, projection_plan) = plan_converge(&projection_fixture, &[]);
    assert!(output.status.success(), "plan failed: {projection_plan}");
    let live_marker = projection_fixture.target.path().join("demo/external.txt");
    fs::write(&live_marker, "external\n").expect("mutate live projection");
    let (output, stale) = apply_plan(
        &projection_fixture,
        &projection_plan,
        "projection-drift",
        &[],
    );
    assert!(
        !output.status.success(),
        "projection drift applied: {stale}"
    );
    assert_eq!(
        stale["error"]["details"]["conflict"]["code"],
        json!("PLAN_PROJECTION_DRIFT")
    );
    assert_eq!(
        fs::read_to_string(live_marker).expect("live marker"),
        "external\n"
    );
}
