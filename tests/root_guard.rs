use std::path::Path;

use serde_json::Value;

mod common;

use common::run_loom;

#[test]
fn write_commands_are_rejected_for_loom_tool_repo_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_path = std::env::temp_dir().join(format!(
        "loom-root-guard-target-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let target_path = target_path.display().to_string();

    let (output, env) = run_loom(
        root,
        &[
            "target",
            "add",
            "--agent",
            "claude",
            "--path",
            &target_path,
            "--ownership",
            "managed",
        ],
    );

    assert!(
        !output.status.success(),
        "write command unexpectedly succeeded"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    let message = env["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("tool repository root")
            && message.contains("separate skill registry repo"),
        "unexpected guard error message: {}",
        message
    );
}
