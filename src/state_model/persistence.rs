use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde_json::Value;

use super::json_io::{
    append_json_line, ensure_json_file, ensure_text_file, read_json_file, read_json_lines,
    serialize_json_file, write_atomic_batch, write_json_file, write_json_lines,
};
use super::{
    REGISTRY_SCHEMA_VERSION, RegistryBindingsFile, RegistryObservationEvent,
    RegistryOperationRecord, RegistryOpsCheckpoint, RegistryProjectionsFile, RegistryRulesFile,
    RegistrySchemaFile, RegistrySnapshot, RegistryStatePaths, RegistryTargetsFile,
    RegistryTrustFile, empty_bindings_file, empty_projections_file, empty_rules_file,
    empty_targets_file, empty_trust_file,
};

impl RegistryStatePaths {
    /// Derive Registry paths from a bare root path.
    ///
    /// Prefer [`RegistryStatePaths::from_app_context`] in production code — it
    /// inherits the state directory decision from [`crate::state::AppContext`]
    /// so any future change to how `state_dir` is computed lands in exactly
    /// one place. This entry point remains for tests and ad-hoc path
    /// derivation where an AppContext isn't constructed.
    pub fn from_root(root: &Path) -> Self {
        let state_dir = root.join("state");
        Self::from_parts(root.to_path_buf(), state_dir)
    }

    /// Derive Registry paths from an existing [`crate::state::AppContext`].
    ///
    /// This avoids re-deriving `state_dir` from `root`, keeping the AppContext
    /// as the single source of truth for where state lives on disk.
    pub fn from_app_context(ctx: &crate::state::AppContext) -> Self {
        Self::from_parts(ctx.root.clone(), ctx.state_dir.clone())
    }

    fn from_parts(root: std::path::PathBuf, state_dir: std::path::PathBuf) -> Self {
        let registry_dir = state_dir.join("registry");
        let ops_dir = registry_dir.join("ops");
        let observations_dir = registry_dir.join("observations");
        Self {
            root,
            state_dir,
            registry_dir: registry_dir.clone(),
            schema_file: registry_dir.join("schema.json"),
            targets_file: registry_dir.join("targets.json"),
            bindings_file: registry_dir.join("bindings.json"),
            rules_file: registry_dir.join("rules.json"),
            projections_file: registry_dir.join("projections.json"),
            trust_file: registry_dir.join("trust.json"),
            ops_dir: ops_dir.clone(),
            operations_file: ops_dir.join("operations.jsonl"),
            checkpoint_file: ops_dir.join("checkpoint.json"),
            observations_dir,
        }
    }

    pub fn exists(&self) -> bool {
        self.schema_file.exists()
    }

    pub fn legacy_state_dir_exists(&self) -> bool {
        self.state_dir.join("v3").exists()
    }

    pub fn ensure_layout(&self) -> Result<()> {
        self.migrate_legacy_state_dir()?;
        fs::create_dir_all(&self.registry_dir)
            .with_context(|| format!("failed to create {}", self.registry_dir.display()))?;
        fs::create_dir_all(&self.ops_dir)
            .with_context(|| format!("failed to create {}", self.ops_dir.display()))?;
        fs::create_dir_all(&self.observations_dir)
            .with_context(|| format!("failed to create {}", self.observations_dir.display()))?;

        ensure_json_file(
            &self.schema_file,
            &RegistrySchemaFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                created_at: Utc::now(),
                writer: format!("loom/{}", env!("CARGO_PKG_VERSION")),
            },
        )?;
        ensure_json_file(&self.targets_file, &empty_targets_file())?;
        ensure_json_file(&self.bindings_file, &empty_bindings_file())?;
        ensure_json_file(&self.rules_file, &empty_rules_file())?;
        ensure_json_file(&self.projections_file, &empty_projections_file())?;
        ensure_json_file(
            &self.checkpoint_file,
            &RegistryOpsCheckpoint {
                schema_version: REGISTRY_SCHEMA_VERSION,
                last_scanned_op_id: None,
                last_acked_op_id: None,
                updated_at: Utc::now(),
            },
        )?;
        ensure_text_file(&self.operations_file, "")?;
        Ok(())
    }

    pub fn load_or_init_snapshot(&self) -> Result<RegistrySnapshot> {
        self.ensure_layout()?;
        self.load_snapshot()
    }

    pub fn load_snapshot(&self) -> Result<RegistrySnapshot> {
        let schema = self.load_schema()?;
        validate_schema_version(schema.schema_version)?;
        let targets = self.load_targets()?;
        validate_schema_version(targets.schema_version)?;
        let bindings = self.load_bindings()?;
        validate_schema_version(bindings.schema_version)?;
        let rules = self.load_rules()?;
        validate_schema_version(rules.schema_version)?;
        let projections = self.load_projections()?;
        validate_schema_version(projections.schema_version)?;
        let checkpoint = self.load_checkpoint()?;
        validate_schema_version(checkpoint.schema_version)?;
        Ok(RegistrySnapshot {
            schema,
            targets,
            bindings,
            rules,
            projections,
            operations: self.load_operations()?,
            checkpoint,
        })
    }

    pub fn maybe_load_snapshot(&self) -> Result<Option<RegistrySnapshot>> {
        if !self.exists() {
            return Ok(None);
        }
        self.load_snapshot().map(Some)
    }

    fn migrate_legacy_state_dir(&self) -> Result<()> {
        let legacy_dir = self.state_dir.join("v3");
        if self.registry_dir.exists() || !legacy_dir.exists() {
            return Ok(());
        }

        fs::rename(&legacy_dir, &self.registry_dir).with_context(|| {
            format!(
                "failed to rename legacy state directory {} to {}",
                legacy_dir.display(),
                self.registry_dir.display()
            )
        })?;
        self.normalize_schema_versions()
    }

    fn normalize_schema_versions(&self) -> Result<()> {
        for path in [
            &self.schema_file,
            &self.targets_file,
            &self.bindings_file,
            &self.rules_file,
            &self.projections_file,
            &self.checkpoint_file,
        ] {
            if !path.exists() {
                continue;
            }
            let mut value: Value = read_json_file(path)?;
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "schema_version".to_string(),
                    Value::from(REGISTRY_SCHEMA_VERSION),
                );
                write_json_file(path, &value)?;
            }
        }
        Ok(())
    }

    pub fn load_schema(&self) -> Result<RegistrySchemaFile> {
        read_json_file(&self.schema_file)
    }

    pub fn load_targets(&self) -> Result<RegistryTargetsFile> {
        read_json_file(&self.targets_file)
    }

    pub fn load_bindings(&self) -> Result<RegistryBindingsFile> {
        read_json_file(&self.bindings_file)
    }

    pub fn load_rules(&self) -> Result<RegistryRulesFile> {
        read_json_file(&self.rules_file)
    }

    pub fn load_projections(&self) -> Result<RegistryProjectionsFile> {
        read_json_file(&self.projections_file)
    }

    pub fn load_trust(&self) -> Result<RegistryTrustFile> {
        if !self.trust_file.exists() {
            return Ok(empty_trust_file());
        }
        let trust: RegistryTrustFile = read_json_file(&self.trust_file)?;
        validate_schema_version(trust.schema_version)?;
        Ok(trust)
    }

    pub fn load_operations(&self) -> Result<Vec<RegistryOperationRecord>> {
        read_json_lines(&self.operations_file)
    }

    pub fn load_checkpoint(&self) -> Result<RegistryOpsCheckpoint> {
        read_json_file(&self.checkpoint_file)
    }

    pub fn load_observations_file(&self, name: &str) -> Result<Vec<RegistryObservationEvent>> {
        let instance_id = name.strip_suffix(".jsonl").unwrap_or(name);
        read_json_lines(&self.observation_file_for_instance(instance_id)?)
    }

    pub fn observation_file_for_instance(&self, instance_id: &str) -> Result<PathBuf> {
        validate_observation_instance_id(instance_id)?;
        Ok(self.observations_dir.join(format!("{instance_id}.jsonl")))
    }

    pub fn append_observation(&self, value: &RegistryObservationEvent) -> Result<()> {
        append_json_line(
            &self.observation_file_for_instance(&value.instance_id)?,
            value,
        )
    }

    pub fn save_targets(&self, value: &RegistryTargetsFile) -> Result<()> {
        write_json_file(&self.targets_file, value)
    }

    pub fn save_bindings(&self, value: &RegistryBindingsFile) -> Result<()> {
        write_json_file(&self.bindings_file, value)
    }

    pub fn save_rules(&self, value: &RegistryRulesFile) -> Result<()> {
        write_json_file(&self.rules_file, value)
    }

    pub fn save_projections(&self, value: &RegistryProjectionsFile) -> Result<()> {
        write_json_file(&self.projections_file, value)
    }

    pub fn save_trust(&self, value: &RegistryTrustFile) -> Result<()> {
        write_json_file(&self.trust_file, value)
    }

    /// Two-phase batch write: write all temp files first, then rename all.
    /// Minimizes the crash window for multi-file state updates.
    pub fn save_bindings_rules_projections(
        &self,
        bindings: &RegistryBindingsFile,
        rules: &RegistryRulesFile,
        projections: &RegistryProjectionsFile,
    ) -> Result<()> {
        let bindings_json = serialize_json_file(bindings)?;
        let rules_json = serialize_json_file(rules)?;
        let projections_json = serialize_json_file(projections)?;
        write_atomic_batch(&[
            (&self.bindings_file, &bindings_json),
            (&self.rules_file, &rules_json),
            (&self.projections_file, &projections_json),
        ])
    }

    pub fn save_checkpoint(&self, value: &RegistryOpsCheckpoint) -> Result<()> {
        write_json_file(&self.checkpoint_file, value)
    }

    pub fn append_operation(&self, value: &RegistryOperationRecord) -> Result<()> {
        append_json_line(&self.operations_file, value)
    }

    pub fn save_operations(&self, value: &[RegistryOperationRecord]) -> Result<()> {
        write_json_lines(&self.operations_file, value)
    }
}

fn validate_schema_version(version: u32) -> Result<()> {
    if version != REGISTRY_SCHEMA_VERSION {
        return Err(anyhow!(
            "registry schema version mismatch: expected {}, got {}",
            REGISTRY_SCHEMA_VERSION,
            version
        ));
    }
    Ok(())
}

fn validate_observation_instance_id(instance_id: &str) -> Result<()> {
    if instance_id.is_empty() {
        return Err(anyhow!("observation instance_id must not be empty"));
    }
    if !instance_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(anyhow!(
            "observation instance_id '{}' contains unsafe filename characters",
            instance_id
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{
        RegistryBindingRule, RegistryBindingsFile, RegistryObservationEvent,
        RegistryOperationRecord, RegistryOpsCheckpoint, RegistryProjectionInstance,
        RegistryProjectionTarget, RegistryProjectionsFile, RegistryRulesFile, RegistrySchemaFile,
        RegistrySnapshot, RegistryStatePaths, RegistryTargetCapabilities, RegistryTargetsFile,
        RegistryWorkspaceBinding, RegistryWorkspaceMatcher,
    };
    use chrono::Utc;
    use serde_json::json;
    use std::{fs, path::Path};
    use uuid::Uuid;

    #[test]
    fn builds_expected_registry_paths() {
        let paths = RegistryStatePaths::from_root(Path::new("/tmp/loom"));
        assert_eq!(paths.registry_dir, Path::new("/tmp/loom/state/registry"));
        assert_eq!(
            paths.operations_file,
            Path::new("/tmp/loom/state/registry/ops/operations.jsonl")
        );
        assert_eq!(
            paths.observations_dir,
            Path::new("/tmp/loom/state/registry/observations")
        );
    }

    #[test]
    fn observation_file_rejects_path_like_instance_ids() {
        let root = std::env::temp_dir().join(format!("loom-observation-safe-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temp root");
        let paths = RegistryStatePaths::from_root(&root);
        paths.ensure_layout().expect("ensure layout");

        let now = Utc::now();
        let event = RegistryObservationEvent {
            event_id: "event_1".to_string(),
            instance_id: "../escaped".to_string(),
            kind: "projected".to_string(),
            path: None,
            from: None,
            to: None,
            observed_at: now,
        };

        let err = paths
            .append_observation(&event)
            .expect_err("path-like instance id must be rejected");
        assert!(err.to_string().contains("unsafe filename characters"));
        assert!(
            !root.join("state/registry/escaped.jsonl").exists(),
            "unsafe instance id must not write outside observations dir"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn query_helpers_link_bindings_targets_and_projections() {
        let now = Utc::now();
        let snapshot = RegistrySnapshot {
            schema: RegistrySchemaFile {
                schema_version: 1,
                created_at: now,
                writer: "loom-test".to_string(),
            },
            targets: RegistryTargetsFile {
                schema_version: 1,
                targets: vec![RegistryProjectionTarget {
                    target_id: "target_claude".to_string(),
                    agent: "claude".into(),
                    path: "/tmp/claude/skills".to_string(),
                    ownership: crate::core::vocab::Ownership::Managed,
                    capabilities: RegistryTargetCapabilities {
                        symlink: true,
                        copy: true,
                        watch: true,
                    },
                    created_at: Some(now),
                }],
            },
            bindings: RegistryBindingsFile {
                schema_version: 1,
                bindings: vec![
                    RegistryWorkspaceBinding {
                        binding_id: "binding_project_a".to_string(),
                        agent: "claude".into(),
                        profile_id: "default".to_string(),
                        workspace_matcher: RegistryWorkspaceMatcher {
                            kind: crate::core::vocab::MatcherKind::PathPrefix,
                            value: "/tmp/project-a".to_string(),
                        },
                        default_target_id: "target_claude".to_string(),
                        policy_profile: "safe-capture".to_string(),
                        active: true,
                        created_at: Some(now),
                    },
                    RegistryWorkspaceBinding {
                        binding_id: "binding_project_b".to_string(),
                        agent: "claude".into(),
                        profile_id: "default".to_string(),
                        workspace_matcher: RegistryWorkspaceMatcher {
                            kind: crate::core::vocab::MatcherKind::PathPrefix,
                            value: "/tmp/project-b".to_string(),
                        },
                        default_target_id: "target_other".to_string(),
                        policy_profile: "safe-capture".to_string(),
                        active: true,
                        created_at: Some(now),
                    },
                    RegistryWorkspaceBinding {
                        binding_id: "binding_project_c".to_string(),
                        agent: "claude".into(),
                        profile_id: "default".to_string(),
                        workspace_matcher: RegistryWorkspaceMatcher {
                            kind: crate::core::vocab::MatcherKind::PathPrefix,
                            value: "/tmp/project-c".to_string(),
                        },
                        default_target_id: "target_other".to_string(),
                        policy_profile: "safe-capture".to_string(),
                        active: true,
                        created_at: Some(now),
                    },
                    RegistryWorkspaceBinding {
                        binding_id: "binding_project_d".to_string(),
                        agent: "claude".into(),
                        profile_id: "default".to_string(),
                        workspace_matcher: RegistryWorkspaceMatcher {
                            kind: crate::core::vocab::MatcherKind::PathPrefix,
                            value: "/tmp/project-d".to_string(),
                        },
                        default_target_id: "target_other".to_string(),
                        policy_profile: "safe-capture".to_string(),
                        active: true,
                        created_at: Some(now),
                    },
                ],
            },
            rules: RegistryRulesFile {
                schema_version: 1,
                rules: vec![
                    RegistryBindingRule {
                        binding_id: "binding_project_a".to_string(),
                        skill_id: "model-onboarding".to_string(),
                        target_id: "target_claude".to_string(),
                        method: crate::core::vocab::ProjectionMethod::Symlink,
                        watch_policy: "observe_only".to_string(),
                        created_at: Some(now),
                    },
                    RegistryBindingRule {
                        binding_id: "binding_project_b".to_string(),
                        skill_id: "model-onboarding".to_string(),
                        target_id: "target_claude".to_string(),
                        method: crate::core::vocab::ProjectionMethod::Symlink,
                        watch_policy: "observe_only".to_string(),
                        created_at: Some(now),
                    },
                ],
            },
            projections: RegistryProjectionsFile {
                schema_version: 1,
                projections: vec![
                    RegistryProjectionInstance {
                        instance_id: "instance_1".to_string(),
                        skill_id: "model-onboarding".to_string(),
                        binding_id: Some("binding_project_a".to_string()),
                        target_id: "target_claude".to_string(),
                        materialized_path: "/tmp/claude/skills/model-onboarding".to_string(),
                        method: crate::core::vocab::ProjectionMethod::Symlink,
                        last_applied_rev: "abc123".to_string(),
                        health: crate::core::vocab::Health::Healthy,
                        observed_drift: Some(false),
                        source_tree_digest: None,
                        materialized_tree_digest: None,
                        last_observed_at: None,
                        last_observed_error: None,
                        updated_at: Some(now),
                    },
                    RegistryProjectionInstance {
                        instance_id: "instance_2".to_string(),
                        skill_id: "model-onboarding".to_string(),
                        binding_id: Some("binding_project_c".to_string()),
                        target_id: "target_claude".to_string(),
                        materialized_path: "/tmp/claude/skills/model-onboarding".to_string(),
                        method: crate::core::vocab::ProjectionMethod::Symlink,
                        last_applied_rev: "abc456".to_string(),
                        health: crate::core::vocab::Health::Healthy,
                        observed_drift: Some(false),
                        source_tree_digest: None,
                        materialized_tree_digest: None,
                        last_observed_at: None,
                        last_observed_error: None,
                        updated_at: Some(now),
                    },
                ],
            },
            operations: vec![RegistryOperationRecord {
                op_id: "op_001".to_string(),
                intent: "skill.project".to_string(),
                status: "succeeded".to_string(),
                ack: false,
                payload: json!({"skill_id": "model-onboarding"}),
                effects: json!({"instance_id": "instance_1"}),
                last_error: None,
                created_at: now,
                updated_at: now,
            }],
            checkpoint: RegistryOpsCheckpoint {
                schema_version: 1,
                last_scanned_op_id: Some("op_001".to_string()),
                last_acked_op_id: None,
                updated_at: now,
            },
        };

        assert!(snapshot.binding("binding_project_a").is_some());
        assert!(snapshot.target("target_claude").is_some());
        assert_eq!(snapshot.binding_rules("binding_project_a").len(), 1);
        assert_eq!(snapshot.binding_projections("binding_project_a").len(), 1);
        assert_eq!(snapshot.target_rules("target_claude").len(), 2);
        assert_eq!(snapshot.target_projections("target_claude").len(), 2);

        let target_relations = snapshot.target_relations("target_claude");
        let target_binding_ids: Vec<_> = target_relations
            .bindings
            .iter()
            .map(|binding| binding.binding_id.clone())
            .collect();
        assert_eq!(
            target_binding_ids,
            vec![
                "binding_project_a".to_string(),
                "binding_project_b".to_string(),
                "binding_project_c".to_string(),
            ]
        );
        assert_eq!(target_relations.rules.len(), 2);
        assert_eq!(target_relations.projections.len(), 2);

        let status = snapshot.status_view();
        assert_eq!(status["counts"]["skills"], json!(1));
        assert_eq!(status["counts"]["active_bindings"], json!(4));
        assert_eq!(status["counts"]["drifted_projections"], json!(0));
    }
}
