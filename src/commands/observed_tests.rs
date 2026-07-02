use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::cli::{ImportObservedArgs, MonitorObservedArgs};
use crate::commands::App;
use crate::state_model::{
    REGISTRY_SCHEMA_VERSION, RegistryBindingsFile, RegistryOpsCheckpoint, RegistryProjectionTarget,
    RegistryProjectionsFile, RegistryRulesFile, RegistryStatePaths, RegistryTargetCapabilities,
    RegistryTargetsFile,
};

fn observed_test_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("loom-observed-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create root");
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "loom@example.com"]);
    git(&root, &["config", "user.name", "Loom Test"]);
    root
}

fn observed_app(root: &Path) -> App {
    App::new(Some(root.to_path_buf())).expect("app")
}

fn git(root: &Path, args: &[&str]) {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_observed_target(root: &Path, target_path: &Path) {
    let paths = RegistryStatePaths::from_root(root);
    paths.ensure_layout().expect("layout");
    let now = Utc::now();
    paths
        .save_targets(&RegistryTargetsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            targets: vec![RegistryProjectionTarget {
                target_id: "observed-1".to_string(),
                agent: "claude".into(),
                path: target_path.display().to_string(),
                ownership: crate::core::vocab::Ownership::Observed,
                capabilities: RegistryTargetCapabilities {
                    symlink: false,
                    copy: true,
                    watch: true,
                },
                created_at: Some(now),
            }],
        })
        .expect("targets");
    paths
        .save_bindings(&RegistryBindingsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            bindings: Vec::new(),
        })
        .expect("bindings");
    paths
        .save_rules(&RegistryRulesFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            rules: Vec::new(),
        })
        .expect("rules");
    paths
        .save_projections(&RegistryProjectionsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            projections: Vec::new(),
        })
        .expect("projections");
    paths
        .save_checkpoint(&RegistryOpsCheckpoint {
            schema_version: REGISTRY_SCHEMA_VERSION,
            last_scanned_op_id: None,
            last_acked_op_id: None,
            updated_at: now,
        })
        .expect("checkpoint");
}

#[test]
fn import_observed_reports_missing_skill_entrypoint_skips() {
    let root = observed_test_root();
    let observed = root.join("observed");
    let missing = observed.join("missing-entrypoint");
    fs::create_dir_all(&missing).expect("missing skill dir");
    write_observed_target(&root, &observed);

    let (payload, _) = observed_app(&root)
        .cmd_import_observed(&ImportObservedArgs { target: None }, "req-import")
        .expect("import observed");

    assert_eq!(payload["count"], json!(0));
    assert_eq!(
        payload["skipped"][0]["reason"],
        json!("missing-skill-entrypoint")
    );
    assert_eq!(payload["skipped"][0]["target_id"], json!("observed-1"));
    assert_eq!(payload["skipped"][0]["name"], json!("missing-entrypoint"));
    assert_eq!(
        payload["skipped"][0]["copy_source"],
        json!(missing.display().to_string())
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn monitor_observed_reports_missing_skill_entrypoint_skips() {
    let root = observed_test_root();
    let observed = root.join("observed");
    let missing = observed.join("missing-entrypoint");
    fs::create_dir_all(&missing).expect("missing skill dir");
    write_observed_target(&root, &observed);

    let (payload, _) = observed_app(&root)
        .cmd_monitor_observed(
            &MonitorObservedArgs {
                target: None,
                once: true,
                interval_seconds: 0,
                max_cycles: None,
            },
            "req-monitor",
        )
        .expect("monitor observed");

    assert_eq!(payload["last_cycle"]["skipped_count"], json!(1));
    assert_eq!(
        payload["last_cycle"]["skipped"][0]["reason"],
        json!("missing-skill-entrypoint")
    );
    assert_eq!(
        payload["last_cycle"]["skipped"][0]["copy_source"],
        json!(missing.display().to_string())
    );

    let _ = fs::remove_dir_all(root);
}
