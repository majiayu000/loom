use std::os::unix::fs::symlink;
use std::process::Stdio;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

use super::*;

fn nested_projected_fixture() -> (Fixture, std::path::PathBuf) {
    let fixture = Fixture {
        root: TestDir::new("convergence-recovery-scope-root"),
        workspace: TestDir::new("convergence-recovery-scope-workspace"),
        target: TestDir::new("convergence-recovery-scope-target"),
    };
    let target_root = fixture.target.path().join("ancestor/live");
    fs::create_dir_all(&target_root).expect("nested target root");
    write_skill(
        fixture.root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing recovery scope.\n---\n# demo\n",
    );
    let (output, saved) = save_skill(fixture.root.path(), "demo");
    assert!(output.status.success(), "save skill failed: {saved}");
    let (output, target) = target_add(fixture.root.path(), "claude", &target_root, "managed");
    assert!(output.status.success(), "target add failed: {target}");
    let target_id = target["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let (output, binding) = binding_add(
        fixture.root.path(),
        "claude",
        "default",
        "exact-path",
        workspace,
        target_id,
    );
    assert!(output.status.success(), "binding add failed: {binding}");
    let binding_id = binding["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id");
    let (output, projected) = skill_project(fixture.root.path(), "demo", binding_id, Some("copy"));
    assert!(output.status.success(), "project failed: {projected}");
    (fixture, target_root)
}

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir_all(destination).expect("copy destination");
    for entry in fs::read_dir(source).expect("copy source") {
        let entry = entry.expect("copy entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("copy file type");
        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path);
        } else if file_type.is_symlink() {
            symlink(
                fs::read_link(&source_path).expect("copy symlink target"),
                &destination_path,
            )
            .expect("copy symlink");
        } else {
            fs::copy(&source_path, &destination_path).expect("copy file");
        }
    }
}

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

fn spawn_paused_recovery(fixture: &Fixture, plan: &Value, key: &str, pause: &TestDir) -> Child {
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
        .env(
            "LOOM_TEST_CONVERGENCE_RECOVERY_SCOPE_PAUSE_DIR",
            pause.path(),
        )
        .env(
            "LOOM_TEST_CONVERGENCE_RECOVERY_SCOPE_PAUSE_POINT",
            "before_restore_preparation_mutation",
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn paused convergence recovery")
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

#[test]
fn recovery_ancestor_swap_does_not_mutate_the_replacement_tree() {
    let (fixture, target_root) = nested_projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/recovery.txt"),
        "reviewed recovery bytes\n",
    )
    .expect("source edit");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "recovery-ancestor-swap";

    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        key,
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_projection_swap",
        )],
    );
    assert!(
        !output.status.success(),
        "projection swap did not interrupt: {interrupted}"
    );
    let pause = TestDir::new("convergence-recovery-scope-pause");
    let child = spawn_paused_recovery(&fixture, &plan, key, &pause);
    wait_until_ready(&pause);
    let ancestor = target_root.parent().expect("target ancestor");
    let held = fixture.target.path().join("held-ancestor");
    let replacement = TestDir::new("convergence-recovery-scope-replacement");
    fs::rename(ancestor, &held).expect("hold reviewed target ancestor");
    copy_tree(&held, replacement.path());
    let replacement_before = snapshot_tree(replacement.path());
    symlink(replacement.path(), ancestor).expect("replace target ancestor");
    fs::write(pause.path().join("release"), b"release\n").expect("release recovery");
    let output = child.wait_with_output().expect("wait for paused recovery");
    let replacement_after = snapshot_tree(replacement.path());
    fs::remove_file(ancestor).expect("remove replacement symlink");
    fs::rename(&held, ancestor).expect("restore reviewed target ancestor");
    let recovered: Value = serde_json::from_slice(&output.stdout).expect("parse recovery output");
    assert_eq!(
        replacement_after, replacement_before,
        "recovery mutated the replacement target tree: {recovered}"
    );
}
