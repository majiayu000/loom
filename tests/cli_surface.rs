use std::process::Command;

mod common;

use common::TestDir;

#[test]
fn migrate_subcommand_is_removed() {
    let root = TestDir::new("cli-no-migrate");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(root.path())
        .args(["migrate", "v2-to-v3", "--plan"])
        .output()
        .expect("run loom");

    assert!(
        !output.status.success(),
        "migrate unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        stderr.contains("unrecognized subcommand")
            || stderr.contains("unexpected argument")
            || stderr.contains("found argument"),
        "stderr did not indicate migrate removal: {}",
        stderr
    );
}

#[test]
fn skill_orphan_clean_nested_command_is_available() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "orphan", "clean", "--help"])
        .output()
        .expect("run loom");

    assert!(
        output.status.success(),
        "orphan clean help failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--delete-live-paths"),
        "orphan clean help must expose explicit live-path deletion flag: {}",
        stdout
    );
}
