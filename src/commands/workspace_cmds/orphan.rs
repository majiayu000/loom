use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::json;

use crate::cli::OrphanCleanArgs;
use crate::envelope::Meta;
use crate::state_model::RegistryProjectionInstance;

use super::super::helpers::{map_io, map_registry_state};
use super::super::projections::record_registry_operation;
use super::super::{App, CommandFailure};

pub(super) enum LivePathCleanup {
    Deleted(String),
    Skipped { path: String, reason: &'static str },
}

pub(super) fn cleanup_orphan_live_path(
    projection: &RegistryProjectionInstance,
    target_paths: &HashMap<String, String>,
) -> std::result::Result<LivePathCleanup, CommandFailure> {
    let path = Path::new(&projection.materialized_path);
    if !path.exists() {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_missing",
        });
    }

    let Some(target_path) = target_paths.get(&projection.target_id) else {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "target_not_registered",
        });
    };

    let metadata = fs::symlink_metadata(path).map_err(map_io)?;
    if metadata.file_type().is_symlink() {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_is_symlink",
        });
    }
    if !metadata.is_dir() {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_not_directory",
        });
    }

    let target_root = match fs::canonicalize(PathBuf::from(target_path)) {
        Ok(root) => root,
        Err(_) => {
            return Ok(LivePathCleanup::Skipped {
                path: projection.materialized_path.clone(),
                reason: "target_path_missing",
            });
        }
    };
    let live_path = fs::canonicalize(path).map_err(map_io)?;

    if live_path == target_root {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_is_target_root",
        });
    }
    if !live_path.starts_with(&target_root) {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_outside_target",
        });
    }

    fs::remove_dir_all(path).map_err(map_io)?;
    Ok(LivePathCleanup::Deleted(
        projection.materialized_path.clone(),
    ))
}

pub(super) fn is_orphan_projection(projection: &RegistryProjectionInstance) -> bool {
    projection.binding_id.is_none() && projection.health == crate::core::vocab::Health::Orphaned
}

impl App {
    pub fn cmd_skill_orphan_clean(
        &self,
        args: &OrphanCleanArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self
            .ctx
            .lock_workspace()
            .map_err(super::super::helpers::map_lock)?;
        self.ensure_write_layout()?;
        let paths = self.ensure_registry_layout()?;
        let mut snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let original_projections = snapshot.projections.clone();
        let target_paths = snapshot
            .targets
            .targets
            .iter()
            .map(|target| (target.target_id.clone(), target.path.clone()))
            .collect::<HashMap<_, _>>();

        let mut cleaned_ids: Vec<String> = Vec::new();
        let mut deleted_projection_ids: Vec<String> = Vec::new();
        let mut deleted_paths: Vec<String> = Vec::new();
        let mut skipped_paths: Vec<serde_json::Value> = Vec::new();
        let mut retained = Vec::new();

        for proj in snapshot.projections.projections.drain(..) {
            if is_orphan_projection(&proj) {
                if args.delete_live_paths {
                    match cleanup_orphan_live_path(&proj, &target_paths)? {
                        LivePathCleanup::Deleted(path) => {
                            deleted_projection_ids.push(proj.instance_id.clone());
                            deleted_paths.push(path);
                        }
                        LivePathCleanup::Skipped { path, reason } => {
                            skipped_paths.push(json!({
                                "projection_id": proj.instance_id.clone(),
                                "path": path,
                                "reason": reason,
                            }));
                        }
                    }
                } else if Path::new(&proj.materialized_path).exists() {
                    skipped_paths.push(json!({
                        "projection_id": proj.instance_id.clone(),
                        "path": proj.materialized_path.clone(),
                        "reason": "delete_live_paths_not_requested",
                    }));
                }
                cleaned_ids.push(proj.instance_id.clone());
            } else {
                retained.push(proj);
            }
        }
        snapshot.projections.projections = retained;

        paths
            .save_projections(&snapshot.projections)
            .map_err(map_registry_state)?;

        let op_id = match record_registry_operation(
            &paths,
            "skill.orphan.clean",
            json!({ "request_id": request_id }),
            json!({
                "cleaned_projection_ids": cleaned_ids,
                "cleaned_paths": deleted_paths,
                "deleted_paths": deleted_paths,
                "skipped_paths": skipped_paths,
                "delete_live_paths": args.delete_live_paths,
            }),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                let mut restored_projections = original_projections;
                if !deleted_projection_ids.is_empty() {
                    restored_projections.projections.retain(|projection| {
                        !deleted_projection_ids.contains(&projection.instance_id)
                    });
                }
                paths
                    .save_projections(&restored_projections)
                    .with_context(|| {
                        format!(
                            "failed to rollback projections after operation-log failure: {}",
                            err
                        )
                    })
                    .map_err(map_registry_state)?;

                if !deleted_projection_ids.is_empty() {
                    return Err(map_registry_state(err.context(
                        "operation log failed after live path deletion; leaving metadata removed for deleted live paths",
                    )));
                } else {
                    return Err(map_registry_state(err));
                }
            }
        };

        Ok((
            json!({
                "cleaned_count": cleaned_ids.len(),
                "cleaned_projection_ids": cleaned_ids,
                "cleaned_paths": deleted_paths,
                "deleted_paths": deleted_paths,
                "skipped_paths": skipped_paths,
                "delete_live_paths": args.delete_live_paths,
            }),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_skill_orphan_list(
        &self,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let snapshot = self.require_registry_snapshot()?;
        let projections = snapshot
            .projections
            .projections
            .iter()
            .filter(|projection| is_orphan_projection(projection))
            .map(|projection| {
                json!({
                    "instance_id": projection.instance_id,
                    "skill_id": projection.skill_id,
                    "binding_id": projection.binding_id,
                    "target_id": projection.target_id,
                    "materialized_path": projection.materialized_path,
                    "method": projection.method,
                    "last_applied_rev": projection.last_applied_rev,
                    "health": projection.health,
                    "observed_drift": projection.observed_drift,
                    "live_path_exists": Path::new(&projection.materialized_path).exists(),
                    "updated_at": projection.updated_at,
                })
            })
            .collect::<Vec<_>>();
        let orphaned_projection_ids = projections
            .iter()
            .filter_map(|projection| projection["instance_id"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        let orphaned_paths = projections
            .iter()
            .filter_map(|projection| projection["materialized_path"].as_str().map(str::to_string))
            .collect::<Vec<_>>();

        Ok((
            json!({
                "count": projections.len(),
                "orphaned_projection_ids": orphaned_projection_ids,
                "orphaned_paths": orphaned_paths,
                "projections": projections,
            }),
            Meta::default(),
        ))
    }
}

#[cfg(test)]
mod orphan_tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    use chrono::Utc;

    use crate::state::AppContext;
    use crate::state_model::{
        REGISTRY_SCHEMA_VERSION, RegistryBindingsFile, RegistryOpsCheckpoint,
        RegistryProjectionInstance, RegistryProjectionTarget, RegistryProjectionsFile,
        RegistryRulesFile, RegistrySchemaFile, RegistryStatePaths, RegistryTargetCapabilities,
        RegistryTargetsFile, RegistryWorkspaceBinding, RegistryWorkspaceMatcher,
    };

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loom-orphan-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_minimal_registry(root: &Path) -> RegistryStatePaths {
        let paths = RegistryStatePaths::from_root(root);
        paths.ensure_layout().unwrap();
        let now = Utc::now();
        fs::write(
            &paths.schema_file,
            serde_json::to_vec_pretty(&RegistrySchemaFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                created_at: now,
                writer: "test".into(),
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.targets_file,
            serde_json::to_vec_pretty(&RegistryTargetsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                targets: vec![RegistryProjectionTarget {
                    target_id: "target1".into(),
                    agent: "claude".into(),
                    path: root.display().to_string(),
                    ownership: crate::core::vocab::Ownership::Observed,
                    capabilities: RegistryTargetCapabilities {
                        symlink: false,
                        copy: true,
                        watch: true,
                    },
                    created_at: None,
                }],
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.rules_file,
            serde_json::to_vec_pretty(&RegistryRulesFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                rules: vec![],
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.checkpoint_file,
            serde_json::to_vec_pretty(&RegistryOpsCheckpoint {
                schema_version: REGISTRY_SCHEMA_VERSION,
                last_scanned_op_id: None,
                last_acked_op_id: None,
                updated_at: now,
            })
            .unwrap(),
        )
        .unwrap();
        paths
    }

    fn make_binding(id: &str) -> RegistryWorkspaceBinding {
        RegistryWorkspaceBinding {
            binding_id: id.into(),
            agent: "claude".into(),
            profile_id: "default".into(),
            workspace_matcher: RegistryWorkspaceMatcher {
                kind: crate::core::vocab::MatcherKind::Name,
                value: "test".into(),
            },
            default_target_id: "target1".into(),
            policy_profile: "safe-capture".into(),
            active: true,
            created_at: None,
        }
    }

    fn make_projection(
        instance_id: &str,
        binding_id: &str,
        health: &str,
        mat_path: &str,
    ) -> RegistryProjectionInstance {
        RegistryProjectionInstance {
            instance_id: instance_id.into(),
            skill_id: "skill1".into(),
            binding_id: Some(binding_id.into()),
            target_id: "target1".into(),
            materialized_path: mat_path.into(),
            method: crate::core::vocab::ProjectionMethod::Copy,
            last_applied_rev: "abc123".into(),
            health: match health {
                "healthy" => crate::core::vocab::Health::Healthy,
                "drifted" => crate::core::vocab::Health::Drifted,
                "missing" => crate::core::vocab::Health::Missing,
                "conflict" => crate::core::vocab::Health::Conflict,
                "orphaned" => crate::core::vocab::Health::Orphaned,
                other => panic!("unsupported test health {other}"),
            },
            observed_drift: Some(false),
            updated_at: None,
        }
    }

    fn orphan_clean_args(delete_live_paths: bool) -> crate::cli::OrphanCleanArgs {
        crate::cli::OrphanCleanArgs {
            delete_live_paths,
            dry_run: false,
        }
    }

    fn binding_remove_orphan_args(binding_id: &str) -> crate::cli::BindingRemoveArgs {
        crate::cli::BindingRemoveArgs {
            binding_id: binding_id.to_string(),
            orphan_projections: true,
        }
    }

    fn setup_with_binding_and_projection(
        root: &Path,
        mat_path: &str,
        health: &str,
    ) -> RegistryStatePaths {
        let paths = write_minimal_registry(root);

        fs::write(
            &paths.bindings_file,
            serde_json::to_vec_pretty(&RegistryBindingsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                bindings: vec![make_binding("bind1")],
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.projections_file,
            serde_json::to_vec_pretty(&RegistryProjectionsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                projections: vec![make_projection("inst1", "bind1", health, mat_path)],
            })
            .unwrap(),
        )
        .unwrap();
        crate::gitops::ensure_repo_initialized(&AppContext::new(Some(root.to_path_buf())).unwrap())
            .ok();
        paths
    }

    #[test]
    fn binding_removal_marks_projection_orphaned_not_deleted() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(&binding_remove_orphan_args("bind1"), "req-test")
            .unwrap();

        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        assert_eq!(
            snapshot.projections.projections.len(),
            1,
            "projection must not be deleted"
        );
        let proj = &snapshot.projections.projections[0];
        assert_eq!(proj.health, "orphaned");
        assert!(proj.binding_id.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn binding_removal_effects_use_orphaned_projection_ids_key() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        let (data, _meta) = app
            .cmd_workspace_binding_remove(&binding_remove_orphan_args("bind1"), "req-test")
            .unwrap();

        let ids = data["orphaned_projection_ids"]
            .as_array()
            .expect("orphaned_projection_ids array");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "inst1");
        assert!(
            data.get("removed_projection_ids").is_none(),
            "old key must not appear"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn binding_removal_orphans_drifted_projection() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "drifted");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(&binding_remove_orphan_args("bind1"), "req-test")
            .unwrap();

        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        let proj = &snapshot.projections.projections[0];
        assert_eq!(proj.health, "orphaned");
        assert!(proj.binding_id.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn binding_removal_orphans_conflict_projection() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "conflict");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(&binding_remove_orphan_args("bind1"), "req-test")
            .unwrap();

        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        let proj = &snapshot.projections.projections[0];
        assert_eq!(proj.health, "orphaned");
        assert!(proj.binding_id.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn orphan_clean_removes_metadata_without_deleting_paths_by_default() {
        let root = temp_root();
        let mat_dir = root.join("mat_proj");
        fs::create_dir_all(&mat_dir).unwrap();
        let mat_path = mat_dir.display().to_string();

        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        // First orphan the projection via binding removal
        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(&binding_remove_orphan_args("bind1"), "req1")
            .unwrap();

        // Now clean orphans
        let ctx2 = AppContext::new(Some(root.clone())).unwrap();
        let app2 = crate::commands::App { ctx: ctx2 };
        let (data, _meta) = app2
            .cmd_skill_orphan_clean(&orphan_clean_args(false), "req2")
            .unwrap();

        assert_eq!(data["cleaned_count"], 1);
        assert_eq!(data["deleted_paths"].as_array().unwrap().len(), 0);
        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        assert!(
            snapshot.projections.projections.is_empty(),
            "orphaned record must be removed"
        );
        assert!(
            mat_dir.exists(),
            "materialized path must be preserved by default"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn orphan_clean_deletes_live_path_only_with_explicit_flag() {
        let root = temp_root();
        let mat_dir = root.join("mat_proj");
        fs::create_dir_all(&mat_dir).unwrap();
        let mat_path = mat_dir.display().to_string();

        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(&binding_remove_orphan_args("bind1"), "req1")
            .unwrap();

        let ctx2 = AppContext::new(Some(root.clone())).unwrap();
        let app2 = crate::commands::App { ctx: ctx2 };
        let (data, _meta) = app2
            .cmd_skill_orphan_clean(&orphan_clean_args(true), "req2")
            .unwrap();

        assert_eq!(data["cleaned_count"], 1);
        assert_eq!(data["deleted_paths"].as_array().unwrap().len(), 1);
        assert!(!mat_dir.exists(), "validated live path must be deleted");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn orphan_clean_refuses_to_delete_paths_outside_registered_target() {
        let root = temp_root();
        let outside = temp_root();
        let mat_dir = outside.join("mat_proj");
        fs::create_dir_all(&mat_dir).unwrap();
        let mat_path = mat_dir.display().to_string();

        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(&binding_remove_orphan_args("bind1"), "req1")
            .unwrap();

        let ctx2 = AppContext::new(Some(root.clone())).unwrap();
        let app2 = crate::commands::App { ctx: ctx2 };
        let (data, _meta) = app2
            .cmd_skill_orphan_clean(&orphan_clean_args(true), "req2")
            .unwrap();

        assert_eq!(data["cleaned_count"], 1);
        assert!(mat_dir.exists(), "outside path must not be deleted");
        assert_eq!(
            data["skipped_paths"][0]["reason"],
            serde_json::Value::String("path_outside_target".into())
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn orphan_clean_audit_records_skill_orphan_clean_intent() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(&binding_remove_orphan_args("bind1"), "req1")
            .unwrap();

        let ctx2 = AppContext::new(Some(root.clone())).unwrap();
        let app2 = crate::commands::App { ctx: ctx2 };
        let (_data, meta) = app2
            .cmd_skill_orphan_clean(&orphan_clean_args(false), "req2")
            .unwrap();
        assert!(meta.op_id.is_some(), "op_id must be recorded");

        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        let clean_op = snapshot
            .operations
            .iter()
            .find(|op| op.intent == "skill.orphan.clean")
            .expect("skill.orphan.clean op must exist");
        assert!(
            clean_op.effects["cleaned_projection_ids"]
                .as_array()
                .is_some()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn removing_one_binding_does_not_orphan_other_binding_projections() {
        let root = temp_root();
        let paths = write_minimal_registry(&root);
        let now = Utc::now();

        fs::write(
            &paths.bindings_file,
            serde_json::to_vec_pretty(&RegistryBindingsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                bindings: vec![make_binding("bind1"), make_binding("bind2")],
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.projections_file,
            serde_json::to_vec_pretty(&RegistryProjectionsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                projections: vec![
                    make_projection("inst1", "bind1", "healthy", "/tmp/p1"),
                    make_projection("inst2", "bind2", "healthy", "/tmp/p2"),
                ],
            })
            .unwrap(),
        )
        .unwrap();
        crate::gitops::ensure_repo_initialized(&AppContext::new(Some(root.clone())).unwrap()).ok();
        let _ = now;

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(&binding_remove_orphan_args("bind1"), "req-test")
            .unwrap();

        let snap = RegistryStatePaths::from_root(&root)
            .load_snapshot()
            .unwrap();
        assert_eq!(snap.projections.projections.len(), 2);
        let inst2 = snap
            .projections
            .projections
            .iter()
            .find(|p| p.instance_id == "inst2")
            .unwrap();
        assert_eq!(inst2.health, "healthy");
        assert_eq!(inst2.binding_id.as_deref(), Some("bind2"));

        let _ = fs::remove_dir_all(&root);
    }
}
