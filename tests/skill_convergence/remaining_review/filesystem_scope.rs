use std::os::unix::fs::symlink;
use std::process::Stdio;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

use super::*;

fn spawn_paused_apply(
    fixture: &Fixture,
    plan: &Value,
    key: &str,
    pause: &TestDir,
    point: &str,
) -> Child {
    Command::new(env!("CARGO_BIN_EXE_loom"))
        .current_dir(std::env::current_dir().expect("test cwd"))
        .arg("--json")
        .arg("--root")
        .arg(fixture.root.path())
        .args([
            "apply",
            plan["data"]["plan_id"].as_str().expect("plan id"),
            "--plan-digest",
            plan["data"]["plan_digest"].as_str().expect("plan digest"),
            "--idempotency-key",
            key,
        ])
        .env("LOOM_TEST_CONVERGENCE_TARGET_SCOPE_PAUSE_DIR", pause.path())
        .env("LOOM_TEST_CONVERGENCE_TARGET_SCOPE_PAUSE_POINT", point)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn paused convergence apply")
}

fn wait_until_ready(pause: &TestDir) {
    for _ in 0..2_000 {
        if pause.path().join("ready").is_file() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("convergence apply did not reach target scope pause");
}

fn assert_root_swap_is_confined(point: &str) {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/scope.txt"),
        format!("reviewed bytes for {point}\n"),
    )
    .expect("source edit");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let pause = TestDir::new("convergence-target-scope-pause");
    let outside = TestDir::new("convergence-target-scope-outside");
    let outside_before = snapshot_tree(outside.path());
    let original_root = fixture.target.path().to_path_buf();
    let held_root = original_root.with_extension(format!("held-{}", uuid::Uuid::new_v4()));
    let child = spawn_paused_apply(&fixture, &plan, point, &pause, point);
    wait_until_ready(&pause);

    fs::rename(&original_root, &held_root).expect("move reviewed target root");
    symlink(outside.path(), &original_root).expect("replace target root with outside symlink");
    fs::write(pause.path().join("release"), b"release\n").expect("release apply");
    let output = child.wait_with_output().expect("wait for paused apply");

    fs::remove_file(&original_root).expect("remove target root symlink");
    fs::rename(&held_root, &original_root).expect("restore reviewed target root");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("parse apply output");
    assert!(
        !output.status.success(),
        "target root swap was accepted at {point}: {envelope}"
    );
    assert_eq!(
        snapshot_tree(outside.path()),
        outside_before,
        "target root swap wrote outside the reviewed scope at {point}"
    );
}

#[test]
fn target_root_swap_between_guard_and_owner_create_is_confined() {
    assert_root_swap_is_confined("before_owner_create");
}

#[test]
fn target_root_swap_between_owner_ready_and_stage_is_confined() {
    assert_root_swap_is_confined("after_owner_ready");
}
