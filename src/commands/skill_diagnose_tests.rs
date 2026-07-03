use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::cli::{SkillDiagnoseArgs, SkillDiagnoseCheck, SkillOnlyArgs};
use crate::commands::App;
use crate::state_model::{
    REGISTRY_SCHEMA_VERSION, RegistryBindingRule, RegistryBindingsFile, RegistryOperationRecord,
    RegistryOpsCheckpoint, RegistryProjectionInstance, RegistryProjectionTarget,
    RegistryProjectionsFile, RegistryRulesFile, RegistryStatePaths, RegistryTargetCapabilities,
    RegistryTargetsFile, RegistryWorkspaceBinding, RegistryWorkspaceMatcher,
};

fn test_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("loom-skill-diagnose-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create root");
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "loom@example.com"]);
    git(&root, &["config", "user.name", "Loom Test"]);
    root
}

fn app(root: &Path) -> App {
    App::new(Some(root.to_path_buf())).expect("app")
}

fn diagnose_args(skill: &str) -> SkillDiagnoseArgs {
    SkillDiagnoseArgs {
        skill: skill.to_string(),
        agent: None,
        check: SkillDiagnoseCheck::All,
    }
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

fn commit_all(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "test setup"]);
}

fn write_skill(root: &Path, skill: &str) {
    let skill_dir = root.join("skills").join(skill);
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: Demo skill\n---\nbody\n",
    )
    .expect("skill file");
}

fn write_snapshot(root: &Path, target_path: &Path, projection_path: &Path, skill: &str) {
    write_projection_snapshot(
        root,
        target_path,
        projection_path,
        skill,
        crate::core::vocab::ProjectionMethod::Symlink,
    );
}

fn write_projection_snapshot(
    root: &Path,
    target_path: &Path,
    projection_path: &Path,
    skill: &str,
    method: crate::core::vocab::ProjectionMethod,
) {
    let paths = RegistryStatePaths::from_root(root);
    paths.ensure_layout().expect("layout");
    paths
        .save_targets(&RegistryTargetsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            targets: vec![RegistryProjectionTarget {
                target_id: "target-1".to_string(),
                agent: "claude".into(),
                path: target_path.display().to_string(),
                ownership: crate::core::vocab::Ownership::Managed,
                capabilities: RegistryTargetCapabilities {
                    symlink: true,
                    copy: true,
                    watch: true,
                },
                created_at: Some(Utc::now()),
            }],
        })
        .expect("targets");
    paths
        .save_bindings(&RegistryBindingsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            bindings: vec![RegistryWorkspaceBinding {
                binding_id: "binding-1".to_string(),
                agent: "claude".into(),
                profile_id: "default".to_string(),
                workspace_matcher: RegistryWorkspaceMatcher {
                    kind: crate::core::vocab::MatcherKind::PathPrefix,
                    value: root.display().to_string(),
                },
                default_target_id: "target-1".to_string(),
                policy_profile: "safe-capture".to_string(),
                active: true,
                created_at: Some(Utc::now()),
            }],
        })
        .expect("bindings");
    paths
        .save_rules(&RegistryRulesFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            rules: vec![RegistryBindingRule {
                binding_id: "binding-1".to_string(),
                skill_id: skill.to_string(),
                target_id: "target-1".to_string(),
                method,
                watch_policy: "manual".to_string(),
                created_at: Some(Utc::now()),
            }],
        })
        .expect("rules");
    paths
        .save_projections(&RegistryProjectionsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            projections: vec![RegistryProjectionInstance {
                instance_id: "inst-1".to_string(),
                skill_id: skill.to_string(),
                binding_id: Some("binding-1".to_string()),
                target_id: "target-1".to_string(),
                materialized_path: projection_path.display().to_string(),
                method,
                last_applied_rev: "HEAD".to_string(),
                health: crate::core::vocab::Health::Healthy,
                observed_drift: Some(false),
                source_tree_digest: None,
                materialized_tree_digest: None,
                last_observed_at: None,
                last_observed_error: None,
                updated_at: Some(Utc::now()),
            }],
        })
        .expect("projections");
    paths
        .save_checkpoint(&RegistryOpsCheckpoint {
            schema_version: REGISTRY_SCHEMA_VERSION,
            last_scanned_op_id: None,
            last_acked_op_id: None,
            updated_at: Utc::now(),
        })
        .expect("checkpoint");
}

fn read_persisted_projection(root: &Path) -> serde_json::Value {
    let raw =
        fs::read_to_string(root.join("state/registry/projections.json")).expect("read projections");
    let persisted: serde_json::Value = serde_json::from_str(&raw).expect("parse projections");
    persisted["projections"][0].clone()
}

#[test]
fn skill_diagnose_persists_copy_projection_digest_drift() {
    let root = test_root();
    write_skill(&root, "demo");
    commit_all(&root);

    let target = root.join("target");
    let projection = target.join("demo");
    fs::create_dir_all(&projection).expect("projection dir");
    fs::copy(
        root.join("skills/demo/SKILL.md"),
        projection.join("SKILL.md"),
    )
    .expect("seed projection");
    write_projection_snapshot(
        &root,
        &target,
        &projection,
        "demo",
        crate::core::vocab::ProjectionMethod::Copy,
    );
    fs::write(
        projection.join("SKILL.md"),
        "---\ndescription: Demo skill\n---\nlive drift\n",
    )
    .expect("drift projection");

    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&diagnose_args("demo"))
        .expect("diagnose");
    let digest_check = payload["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "projection_content_digest:inst-1")
        .expect("projection digest check");
    assert_eq!(digest_check["ok"], json!(false));
    assert_eq!(digest_check["severity"], json!("warning"));
    assert_eq!(digest_check["details"]["status"], json!("drifted"));

    let projection_record = read_persisted_projection(&root);
    assert_eq!(projection_record["health"], json!("drifted"));
    assert_eq!(projection_record["observed_drift"], json!(true));
    assert_eq!(
        projection_record["last_observed_error"],
        json!("digest_mismatch")
    );
    assert_ne!(
        projection_record["source_tree_digest"],
        projection_record["materialized_tree_digest"]
    );
    assert!(
        projection_record["source_tree_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    assert!(
        projection_record["last_observed_at"].as_str().is_some(),
        "diagnose should persist observation timestamp"
    );

    let (status, _) = app(&root).cmd_status().expect("status");
    assert_eq!(
        status["registry"]["counts"]["drifted_projections"],
        json!(1)
    );
    assert_eq!(
        status["registry"]["projections"][0]["observation_status"],
        json!("drifted")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_persists_symlink_projection_target_drift() {
    let root = test_root();
    write_skill(&root, "demo");
    commit_all(&root);

    let target = root.join("target");
    let wrong_target = root.join("wrong-demo");
    fs::create_dir_all(&target).expect("target");
    fs::create_dir_all(&wrong_target).expect("wrong target");
    let link = target.join("demo");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&wrong_target, &link).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&wrong_target, &link).expect("symlink");
    write_snapshot(&root, &target, &link, "demo");

    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&diagnose_args("demo"))
        .expect("diagnose");
    let symlink_check = payload["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "projection_symlink_target:inst-1")
        .expect("symlink check");
    assert_eq!(symlink_check["ok"], json!(false));
    assert_eq!(symlink_check["severity"], json!("warning"));
    assert_eq!(symlink_check["details"]["status"], json!("drifted"));

    let projection_record = read_persisted_projection(&root);
    assert_eq!(projection_record["health"], json!("drifted"));
    assert_eq!(projection_record["observed_drift"], json!(true));
    assert_eq!(
        projection_record["last_observed_error"],
        json!("symlink_target_mismatch")
    );
    let (status, _) = app(&root).cmd_status().expect("status");
    assert_eq!(
        status["registry"]["counts"]["drifted_projections"],
        json!(1)
    );
    assert_eq!(
        status["registry"]["projections"][0]["observation_status"],
        json!("drifted")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_persists_materialize_projection_missing_live_path() {
    let root = test_root();
    write_skill(&root, "demo");
    commit_all(&root);

    let target = root.join("target");
    let projection = target.join("demo");
    fs::create_dir_all(&target).expect("target");
    write_projection_snapshot(
        &root,
        &target,
        &projection,
        "demo",
        crate::core::vocab::ProjectionMethod::Materialize,
    );

    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&diagnose_args("demo"))
        .expect("diagnose");
    let digest_check = payload["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "projection_content_digest:inst-1")
        .expect("projection digest check");
    assert_eq!(digest_check["ok"], json!(false));
    assert_eq!(digest_check["severity"], json!("error"));
    assert_eq!(digest_check["details"]["status"], json!("missing"));
    assert_eq!(
        digest_check["details"]["error"],
        json!("materialized_missing")
    );

    let projection_record = read_persisted_projection(&root);
    assert_eq!(projection_record["health"], json!("missing"));
    assert_eq!(projection_record["observed_drift"], json!(true));
    assert_eq!(
        projection_record["last_observed_error"],
        json!("materialized_missing")
    );
    let (status, _) = app(&root).cmd_status().expect("status");
    assert_eq!(
        status["registry"]["counts"]["drifted_projections"],
        json!(1)
    );
    assert_eq!(
        status["registry"]["projections"][0]["observation_status"],
        json!("missing")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_unknown_skill_returns_not_found() {
    let root = test_root();
    let err = app(&root)
        .cmd_skill_diagnose(&SkillOnlyArgs {
            skill: "missing".to_string(),
        })
        .expect_err("missing skill");
    assert_eq!(err.code.as_str(), "SKILL_NOT_FOUND");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_reports_missing_source_for_referenced_skill() {
    let root = test_root();
    let target = root.join("target");
    fs::create_dir_all(&target).expect("target");
    write_snapshot(&root, &target, &target.join("demo"), "demo");
    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&SkillOnlyArgs {
            skill: "demo".to_string(),
        })
        .expect("diagnose");
    assert_eq!(payload["status"], json!("blocked"));
    assert!(
        payload["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["id"] == "source_directory_exists" && check["ok"] == false)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_recognizes_observed_import_operation_reference() {
    let root = test_root();
    let paths = RegistryStatePaths::from_root(&root);
    paths.ensure_layout().expect("layout");
    paths
        .append_operation(&RegistryOperationRecord {
            op_id: "op-observed".to_string(),
            intent: "skill.import_observed".to_string(),
            status: "succeeded".to_string(),
            ack: true,
            payload: json!({}),
            effects: json!({"imported": [{"skill": "observed-skill"}]}),
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .expect("append op");

    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&SkillOnlyArgs {
            skill: "observed-skill".to_string(),
        })
        .expect("diagnose");

    assert_eq!(payload["status"], json!("blocked"));
    assert_eq!(
        payload["related"]["recent_operations"][0]["op_id"],
        json!("op-observed")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_resolves_relative_symlink_from_link_parent() {
    let root = test_root();
    write_skill(&root, "demo");
    commit_all(&root);
    let target = root.join("target");
    fs::create_dir_all(&target).expect("target");
    let link = target.join("demo");
    #[cfg(unix)]
    std::os::unix::fs::symlink("../skills/demo", &link).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir("..\\skills\\demo", &link).expect("symlink");
    write_snapshot(&root, &target, &link, "demo");
    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&SkillOnlyArgs {
            skill: "demo".to_string(),
        })
        .expect("diagnose");
    let symlink_check = payload["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "projection_symlink_target:inst-1")
        .expect("symlink check");
    assert_eq!(symlink_check["ok"], json!(true));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_reports_unsaved_source_drift() {
    let root = test_root();
    write_skill(&root, "demo");
    commit_all(&root);
    fs::write(root.join("skills/demo/notes.md"), "new").expect("write drift");
    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&SkillOnlyArgs {
            skill: "demo".to_string(),
        })
        .expect("diagnose");
    let drift_check = payload["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "source_drift")
        .expect("drift check");
    assert_eq!(drift_check["ok"], json!(false));
    assert_eq!(payload["status"], json!("attention"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_status_blocks_when_errors_and_warnings_coexist() {
    let root = test_root();
    write_skill(&root, "demo");
    commit_all(&root);
    let dirty_write = fs::write(
        root.join("skills").join("demo").join("SKILL.md"),
        "---\ndescription: Changed demo skill\n---\nbody\n",
    );
    assert!(dirty_write.is_ok(), "dirty skill file: {dirty_write:?}");
    let missing_target = root.join("missing-target");
    write_snapshot(&root, &missing_target, &missing_target.join("demo"), "demo");

    let diagnosis = app(&root).cmd_skill_diagnose(&SkillOnlyArgs {
        skill: "demo".to_string(),
    });
    assert!(diagnosis.is_ok(), "diagnose failed: {diagnosis:?}");
    let (payload, _) = match diagnosis {
        Ok(value) => value,
        Err(err) => panic!("diagnose failed: {err:?}"),
    };

    assert_eq!(payload["status"], json!("blocked"));
    assert!(
        payload["summary"]["failed_check_count"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
    assert!(
        payload["summary"]["warning_check_count"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
    let checks = match payload["checks"].as_array() {
        Some(value) => value,
        None => panic!("diagnosis checks are not an array: {}", payload["checks"]),
    };
    assert!(
        checks
            .iter()
            .any(|check| check["id"] == "source_drift" && check["severity"] == "warning")
    );
    assert!(checks.iter().any(|check| {
        check["id"] == "target_path_exists:target-1" && check["severity"] == "error"
    }));
    let cleanup = fs::remove_dir_all(root);
    assert!(cleanup.is_ok(), "cleanup failed: {cleanup:?}");
}

#[test]
fn skill_diagnose_reports_total_drift_count_when_paths_are_truncated() {
    let root = test_root();
    write_skill(&root, "demo");
    commit_all(&root);
    for index in 0..101 {
        fs::write(root.join(format!("skills/demo/drift-{index}.md")), "new").expect("write drift");
    }

    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&SkillOnlyArgs {
            skill: "demo".to_string(),
        })
        .expect("diagnose");
    let drift_check = payload["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "source_drift")
        .expect("drift check");

    assert_eq!(payload["summary"]["drifted_path_count"], json!(101));
    assert_eq!(drift_check["details"]["drifted_path_count"], json!(101));
    assert_eq!(
        drift_check["details"]["drifted_paths_truncated"],
        json!(true)
    );
    assert_eq!(
        drift_check["details"]["drifted_paths"]
            .as_array()
            .unwrap()
            .len(),
        100
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_matches_operation_backlog_by_structured_skill_fields_only() {
    let root = test_root();
    write_skill(&root, "demo");
    commit_all(&root);
    let paths = RegistryStatePaths::from_root(&root);
    paths.ensure_layout().expect("layout");
    paths
        .append_operation(&RegistryOperationRecord {
            op_id: "op-other".to_string(),
            intent: "skill.save demo".to_string(),
            status: "succeeded".to_string(),
            ack: false,
            payload: json!({"skill": "other"}),
            effects: json!({}),
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .expect("append op");

    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&SkillOnlyArgs {
            skill: "demo".to_string(),
        })
        .expect("diagnose");
    let backlog = payload["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "recent_operation_backlog")
        .expect("operation backlog check");

    assert_eq!(backlog["ok"], json!(true));
    assert!(
        payload["related"]["operation_backlog"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_reports_source_drift_git_read_errors() {
    let root = std::env::temp_dir().join(format!("loom-skill-diagnose-no-git-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).expect("root");
    write_skill(&root, "demo");

    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&SkillOnlyArgs {
            skill: "demo".to_string(),
        })
        .expect("diagnose");
    let drift = payload["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "source_drift")
        .expect("drift check");

    assert_eq!(payload["status"], json!("blocked"));
    assert_eq!(drift["ok"], json!(false));
    assert_eq!(drift["severity"], json!("error"));
    assert!(drift["details"]["error"].as_str().is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skill_diagnose_checks_projection_only_targets() {
    let root = test_root();
    write_skill(&root, "demo");
    commit_all(&root);
    let paths = RegistryStatePaths::from_root(&root);
    paths.ensure_layout().expect("layout");
    paths
        .save_targets(&RegistryTargetsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            targets: vec![],
        })
        .expect("targets");
    paths
        .save_bindings(&RegistryBindingsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            bindings: vec![],
        })
        .expect("bindings");
    paths
        .save_rules(&RegistryRulesFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            rules: vec![],
        })
        .expect("rules");
    paths
        .save_projections(&RegistryProjectionsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            projections: vec![RegistryProjectionInstance {
                instance_id: "inst-orphan".to_string(),
                skill_id: "demo".to_string(),
                binding_id: None,
                target_id: "deleted-target".to_string(),
                materialized_path: root.join("deleted/demo").display().to_string(),
                method: crate::core::vocab::ProjectionMethod::Copy,
                last_applied_rev: "HEAD".to_string(),
                health: crate::core::vocab::Health::Orphaned,
                observed_drift: Some(false),
                source_tree_digest: None,
                materialized_tree_digest: None,
                last_observed_at: None,
                last_observed_error: None,
                updated_at: Some(Utc::now()),
            }],
        })
        .expect("projections");
    paths
        .save_checkpoint(&RegistryOpsCheckpoint {
            schema_version: REGISTRY_SCHEMA_VERSION,
            last_scanned_op_id: None,
            last_acked_op_id: None,
            updated_at: Utc::now(),
        })
        .expect("checkpoint");

    let (payload, _) = app(&root)
        .cmd_skill_diagnose(&SkillOnlyArgs {
            skill: "demo".to_string(),
        })
        .expect("diagnose");
    let target_check = payload["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "target_path_exists:deleted-target")
        .expect("projection-only target check");

    assert_eq!(payload["status"], json!("blocked"));
    assert_eq!(target_check["ok"], json!(false));
    assert_eq!(target_check["severity"], json!("error"));
    assert_eq!(
        target_check["details"]["target_id"],
        json!("deleted-target")
    );
    let _ = fs::remove_dir_all(root);
}
