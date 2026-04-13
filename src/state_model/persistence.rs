use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{
    V3BindingsFile, V3ObservationEvent, V3OperationRecord, V3OpsCheckpoint, V3ProjectionsFile,
    V3RulesFile, V3SchemaFile, V3Snapshot, V3StatePaths, V3TargetsFile, V3_SCHEMA_VERSION,
    empty_bindings_file, empty_projections_file, empty_rules_file, empty_targets_file,
};

impl V3StatePaths {
    pub fn from_root(root: &Path) -> Self {
        let state_dir = root.join("state");
        let v3_dir = state_dir.join("v3");
        let ops_dir = v3_dir.join("ops");
        let observations_dir = v3_dir.join("observations");
        Self {
            root: root.to_path_buf(),
            state_dir,
            v3_dir: v3_dir.clone(),
            schema_file: v3_dir.join("schema.json"),
            targets_file: v3_dir.join("targets.json"),
            bindings_file: v3_dir.join("bindings.json"),
            rules_file: v3_dir.join("rules.json"),
            projections_file: v3_dir.join("projections.json"),
            ops_dir: ops_dir.clone(),
            operations_file: ops_dir.join("operations.jsonl"),
            checkpoint_file: ops_dir.join("checkpoint.json"),
            observations_dir,
        }
    }

    pub fn exists(&self) -> bool {
        self.schema_file.exists()
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(&self.v3_dir)
            .with_context(|| format!("failed to create {}", self.v3_dir.display()))?;
        fs::create_dir_all(&self.ops_dir)
            .with_context(|| format!("failed to create {}", self.ops_dir.display()))?;
        fs::create_dir_all(&self.observations_dir)
            .with_context(|| format!("failed to create {}", self.observations_dir.display()))?;

        ensure_json_file(
            &self.schema_file,
            &V3SchemaFile {
                schema_version: V3_SCHEMA_VERSION,
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
            &V3OpsCheckpoint {
                schema_version: V3_SCHEMA_VERSION,
                last_scanned_op_id: None,
                last_acked_op_id: None,
                updated_at: Utc::now(),
            },
        )?;
        ensure_text_file(&self.operations_file, "")?;
        Ok(())
    }

    pub fn load_or_init_snapshot(&self) -> Result<V3Snapshot> {
        self.ensure_layout()?;
        self.load_snapshot()
    }

    pub fn load_snapshot(&self) -> Result<V3Snapshot> {
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
        Ok(V3Snapshot {
            schema,
            targets,
            bindings,
            rules,
            projections,
            operations: self.load_operations()?,
            checkpoint,
        })
    }

    pub fn maybe_load_snapshot(&self) -> Result<Option<V3Snapshot>> {
        if !self.exists() {
            return Ok(None);
        }
        self.load_snapshot().map(Some)
    }

    pub fn load_schema(&self) -> Result<V3SchemaFile> {
        read_json_file(&self.schema_file)
    }

    pub fn load_targets(&self) -> Result<V3TargetsFile> {
        read_json_file(&self.targets_file)
    }

    pub fn load_bindings(&self) -> Result<V3BindingsFile> {
        read_json_file(&self.bindings_file)
    }

    pub fn load_rules(&self) -> Result<V3RulesFile> {
        read_json_file(&self.rules_file)
    }

    pub fn load_projections(&self) -> Result<V3ProjectionsFile> {
        read_json_file(&self.projections_file)
    }

    pub fn load_operations(&self) -> Result<Vec<V3OperationRecord>> {
        read_json_lines(&self.operations_file)
    }

    pub fn load_checkpoint(&self) -> Result<V3OpsCheckpoint> {
        read_json_file(&self.checkpoint_file)
    }

    pub fn load_observations_file(&self, name: &str) -> Result<Vec<V3ObservationEvent>> {
        read_json_lines(&self.observations_dir.join(name))
    }

    pub fn save_targets(&self, value: &V3TargetsFile) -> Result<()> {
        write_json_file(&self.targets_file, value)
    }

    pub fn save_bindings(&self, value: &V3BindingsFile) -> Result<()> {
        write_json_file(&self.bindings_file, value)
    }

    pub fn save_rules(&self, value: &V3RulesFile) -> Result<()> {
        write_json_file(&self.rules_file, value)
    }

    pub fn save_projections(&self, value: &V3ProjectionsFile) -> Result<()> {
        write_json_file(&self.projections_file, value)
    }

    pub fn save_checkpoint(&self, value: &V3OpsCheckpoint) -> Result<()> {
        write_json_file(&self.checkpoint_file, value)
    }

    pub fn append_operation(&self, value: &V3OperationRecord) -> Result<()> {
        append_json_line(&self.operations_file, value)
    }
}

fn validate_schema_version(version: u32) -> Result<()> {
    if version != V3_SCHEMA_VERSION {
        return Err(anyhow!(
            "v3 schema version mismatch: expected {}, got {}",
            V3_SCHEMA_VERSION,
            version
        ));
    }
    Ok(())
}

fn ensure_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if path.exists() {
        return Ok(());
    }
    write_json_file(path, value)
}

fn ensure_text_file(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    write_atomic(path, contents)
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let raw = serde_json::to_string_pretty(value)
        .with_context(|| format!("failed to encode v3 json file {}", path.display()))?;
    write_atomic(path, &(raw + "\n"))
}

fn append_json_line<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let parent = path
        .parent()
        .context("cannot append v3 jsonl file without parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    let raw = serde_json::to_string(value)
        .with_context(|| format!("failed to encode v3 jsonl line {}", path.display()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open v3 jsonl file {}", path.display()))?;
    writeln!(file, "{}", raw)
        .with_context(|| format!("failed to append v3 jsonl file {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync v3 jsonl file {}", path.display()))?;
    Ok(())
}

fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read v3 json file {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse v3 json file {}", path.display()))
}

fn read_json_lines<T>(path: &Path) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open v3 jsonl file {}", path.display()))?;
    if file
        .metadata()
        .with_context(|| format!("failed to stat v3 jsonl file {}", path.display()))?
        .len()
        == 0
    {
        return Ok(Vec::new());
    }
    let reader = BufReader::new(file);
    let mut items = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!("failed to read line {} from {}", index + 1, path.display())
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let item = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "failed to parse line {} from v3 jsonl file {}",
                index + 1,
                path.display()
            )
        })?;
        items.push(item);
    }
    Ok(items)
}

fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("cannot write atomic v3 file without parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;

    let tmp_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));

    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
            .with_context(|| format!("failed to create temp file {}", tmp_path.display()))?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("failed to write temp file {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp file {}", tmp_path.display()))?;
    }

    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            tmp_path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{
        V3BindingRule, V3BindingsFile, V3OperationRecord, V3OpsCheckpoint, V3ProjectionInstance,
        V3ProjectionTarget, V3ProjectionsFile, V3RulesFile, V3SchemaFile, V3Snapshot, V3StatePaths,
        V3TargetCapabilities, V3TargetsFile, V3WorkspaceBinding, V3WorkspaceMatcher,
    };
    use chrono::Utc;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn builds_expected_v3_paths() {
        let paths = V3StatePaths::from_root(Path::new("/tmp/loom"));
        assert_eq!(paths.v3_dir, Path::new("/tmp/loom/state/v3"));
        assert_eq!(
            paths.operations_file,
            Path::new("/tmp/loom/state/v3/ops/operations.jsonl")
        );
        assert_eq!(
            paths.observations_dir,
            Path::new("/tmp/loom/state/v3/observations")
        );
    }

    #[test]
    fn query_helpers_link_bindings_targets_and_projections() {
        let now = Utc::now();
        let snapshot = V3Snapshot {
            schema: V3SchemaFile {
                schema_version: 3,
                created_at: now,
                writer: "loom-test".to_string(),
            },
            targets: V3TargetsFile {
                schema_version: 3,
                targets: vec![V3ProjectionTarget {
                    target_id: "target_claude".to_string(),
                    agent: "claude".to_string(),
                    path: "/tmp/claude/skills".to_string(),
                    ownership: "managed".to_string(),
                    capabilities: V3TargetCapabilities {
                        symlink: true,
                        copy: true,
                        watch: true,
                    },
                    created_at: Some(now),
                }],
            },
            bindings: V3BindingsFile {
                schema_version: 3,
                bindings: vec![
                    V3WorkspaceBinding {
                        binding_id: "binding_project_a".to_string(),
                        agent: "claude".to_string(),
                        profile_id: "default".to_string(),
                        workspace_matcher: V3WorkspaceMatcher {
                            kind: "path_prefix".to_string(),
                            value: "/tmp/project-a".to_string(),
                        },
                        default_target_id: "target_claude".to_string(),
                        policy_profile: "safe-capture".to_string(),
                        active: true,
                        created_at: Some(now),
                    },
                    V3WorkspaceBinding {
                        binding_id: "binding_project_b".to_string(),
                        agent: "claude".to_string(),
                        profile_id: "default".to_string(),
                        workspace_matcher: V3WorkspaceMatcher {
                            kind: "path_prefix".to_string(),
                            value: "/tmp/project-b".to_string(),
                        },
                        default_target_id: "target_other".to_string(),
                        policy_profile: "safe-capture".to_string(),
                        active: true,
                        created_at: Some(now),
                    },
                    V3WorkspaceBinding {
                        binding_id: "binding_project_c".to_string(),
                        agent: "claude".to_string(),
                        profile_id: "default".to_string(),
                        workspace_matcher: V3WorkspaceMatcher {
                            kind: "path_prefix".to_string(),
                            value: "/tmp/project-c".to_string(),
                        },
                        default_target_id: "target_other".to_string(),
                        policy_profile: "safe-capture".to_string(),
                        active: true,
                        created_at: Some(now),
                    },
                    V3WorkspaceBinding {
                        binding_id: "binding_project_d".to_string(),
                        agent: "claude".to_string(),
                        profile_id: "default".to_string(),
                        workspace_matcher: V3WorkspaceMatcher {
                            kind: "path_prefix".to_string(),
                            value: "/tmp/project-d".to_string(),
                        },
                        default_target_id: "target_other".to_string(),
                        policy_profile: "safe-capture".to_string(),
                        active: true,
                        created_at: Some(now),
                    },
                ],
            },
            rules: V3RulesFile {
                schema_version: 3,
                rules: vec![
                    V3BindingRule {
                        binding_id: "binding_project_a".to_string(),
                        skill_id: "model-onboarding".to_string(),
                        target_id: "target_claude".to_string(),
                        method: "symlink".to_string(),
                        watch_policy: "observe_only".to_string(),
                        created_at: Some(now),
                    },
                    V3BindingRule {
                        binding_id: "binding_project_b".to_string(),
                        skill_id: "model-onboarding".to_string(),
                        target_id: "target_claude".to_string(),
                        method: "symlink".to_string(),
                        watch_policy: "observe_only".to_string(),
                        created_at: Some(now),
                    },
                ],
            },
            projections: V3ProjectionsFile {
                schema_version: 3,
                projections: vec![
                    V3ProjectionInstance {
                        instance_id: "instance_1".to_string(),
                        skill_id: "model-onboarding".to_string(),
                        binding_id: "binding_project_a".to_string(),
                        target_id: "target_claude".to_string(),
                        materialized_path: "/tmp/claude/skills/model-onboarding".to_string(),
                        method: "symlink".to_string(),
                        last_applied_rev: "abc123".to_string(),
                        health: "healthy".to_string(),
                        observed_drift: Some(false),
                        updated_at: Some(now),
                    },
                    V3ProjectionInstance {
                        instance_id: "instance_2".to_string(),
                        skill_id: "model-onboarding".to_string(),
                        binding_id: "binding_project_c".to_string(),
                        target_id: "target_claude".to_string(),
                        materialized_path: "/tmp/claude/skills/model-onboarding".to_string(),
                        method: "symlink".to_string(),
                        last_applied_rev: "abc456".to_string(),
                        health: "healthy".to_string(),
                        observed_drift: Some(false),
                        updated_at: Some(now),
                    },
                ],
            },
            operations: vec![V3OperationRecord {
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
            checkpoint: V3OpsCheckpoint {
                schema_version: 3,
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
