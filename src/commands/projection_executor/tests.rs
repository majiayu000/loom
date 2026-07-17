use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::*;
use crate::core::vocab::{Health, MatcherKind, Ownership};
use crate::state_model::{RegistryTargetCapabilities, RegistryWorkspaceMatcher};

struct ConvergenceProjectionFixture {
    root: PathBuf,
    ctx: AppContext,
    paths: RegistryStatePaths,
    snapshot: RegistrySnapshot,
}

impl Drop for ConvergenceProjectionFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("remove projection executor fixture");
    }
}

fn convergence_projection_fixture() -> ConvergenceProjectionFixture {
    let root = std::env::temp_dir().join(format!(
        "loom-convergence-projection-executor-{}",
        Uuid::new_v4().simple()
    ));
    fs::create_dir_all(root.join("skills/demo")).expect("create skill");
    fs::write(
        root.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Use when testing convergence projection execution.\n---\n# demo\n",
    )
    .expect("write skill");
    fs::write(root.join("skills/demo/details.txt"), "canonical\n").expect("write details");
    #[cfg(unix)]
    std::os::unix::fs::symlink("details.txt", root.join("skills/demo/current.txt"))
        .expect("create internal source symlink");

    let ctx = AppContext::new(Some(root.clone())).expect("app context");
    ctx.ensure_state_layout().expect("state layout");
    let paths = RegistryStatePaths::from_app_context(&ctx);
    paths.ensure_layout().expect("registry layout");
    gitops::ensure_repo_initialized(&ctx).expect("initialize git repository");
    gitops::run_git(&ctx, &["add", "."]).expect("stage fixture");
    gitops::run_git(&ctx, &["commit", "-m", "fixture"]).expect("commit fixture");
    let snapshot = paths.load_snapshot().expect("snapshot");
    ConvergenceProjectionFixture {
        root,
        ctx,
        paths,
        snapshot,
    }
}

fn execution_input(
    fixture: &ConvergenceProjectionFixture,
    method: ProjectionMethod,
    materialized_path: PathBuf,
) -> ProjectionExecutionInput {
    let method_name = method.as_str();
    let target_path = materialized_path
        .parent()
        .expect("projection parent")
        .display()
        .to_string();
    ProjectionExecutionInput {
        context: ProjectionExecutionContext::Convergence,
        skill: "demo".to_string(),
        binding: RegistryWorkspaceBinding {
            binding_id: format!("binding_{method_name}"),
            agent: "claude".into(),
            profile_id: "default".to_string(),
            workspace_matcher: RegistryWorkspaceMatcher {
                kind: MatcherKind::PathPrefix,
                value: fixture.root.display().to_string(),
            },
            default_target_id: format!("target_{method_name}"),
            policy_profile: "safe-capture".to_string(),
            active: true,
            created_at: Some(Utc::now()),
        },
        binding_is_new: true,
        target: RegistryProjectionTarget {
            target_id: format!("target_{method_name}"),
            agent: "claude".into(),
            path: target_path,
            ownership: Ownership::Managed,
            capabilities: RegistryTargetCapabilities {
                symlink: true,
                copy: true,
                watch: true,
            },
            created_at: Some(Utc::now()),
        },
        target_is_new: true,
        materialized_path,
        method,
        operation_intent: "skill.converge.child",
        operation_payload: json!({"must_not_persist": true}),
        observation_kind: "converged_child",
        request_id: "convergence-child-must-not-run".to_string(),
        commit_message: "must not commit convergence child".to_string(),
        replace_existing: false,
        safe_existing_noop: false,
        after_materialize_fault: None,
        after_state_save_fault: None,
        after_observation_fault: None,
        activation_after_projection_fault: false,
    }
}

fn registry_snapshot(paths: &RegistryStatePaths) -> Value {
    serde_json::to_value(paths.load_snapshot().expect("load registry snapshot"))
        .expect("serialize registry snapshot")
}

fn filesystem_snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(base: &Path, path: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        if !path.exists() {
            return;
        }
        for entry in fs::read_dir(path).expect("read snapshot directory") {
            let entry = entry.expect("read snapshot entry");
            let child = entry.path();
            if entry.file_type().expect("snapshot file type").is_dir() {
                visit(base, &child, files);
            } else {
                let relative = child
                    .strip_prefix(base)
                    .expect("relative snapshot path")
                    .display()
                    .to_string();
                files.insert(relative, fs::read(&child).expect("read snapshot file"));
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[cfg(unix)]
#[test]
fn symlink_copy_materialize_convergence_mode_has_no_child_persistence() {
    let fixture = convergence_projection_fixture();
    let head_before = gitops::head(&fixture.ctx).expect("head before");
    let registry_before = registry_snapshot(&fixture.paths);
    let operations_before = fs::read(&fixture.paths.operations_file).expect("operations before");
    let durable_state_before = filesystem_snapshot(&fixture.ctx.state_dir);

    for method in [
        ProjectionMethod::Symlink,
        ProjectionMethod::Copy,
        ProjectionMethod::Materialize,
    ] {
        let target = fixture.root.join("live").join(method.as_str());
        let projection_path = target.join("demo");
        fs::create_dir_all(&projection_path).expect("create stale projection");
        fs::write(projection_path.join("stale.txt"), "stale\n").expect("write stale data");

        let output = execute_projection(
            &fixture.ctx,
            &fixture.paths,
            &fixture.snapshot,
            execution_input(&fixture, method, projection_path.clone()),
        )
        .expect("execute convergence projection");
        let projection = output.projection.expect("projection delta");

        assert_eq!(projection.health, Health::Healthy);
        assert_eq!(projection.observed_drift, Some(false));
        let backup = output.backup.expect("transaction rollback artifact");
        assert_eq!(backup["kind"], json!("atomic_exchange"));
        let backup_path = PathBuf::from(backup["backup_path"].as_str().expect("backup path"));
        assert_eq!(backup_path.parent(), projection_path.parent());
        assert!(backup_path.join("stale.txt").is_file());
        assert!(!backup_path.starts_with(&fixture.ctx.state_dir));
        assert!(output.commit.is_none(), "convergence child must not commit");
        assert!(
            output.meta.op_id.is_none(),
            "convergence child must not record op"
        );
        assert!(!output.noop, "stale target must be rebuilt");
        assert!(!projection_path.join("stale.txt").exists());

        match method {
            ProjectionMethod::Symlink => assert!(
                projection_path_is_safe_symlink(&projection_path, &fixture.ctx.skill_path("demo")),
                "symlink projection must point to canonical source"
            ),
            ProjectionMethod::Copy => assert!(
                fs::symlink_metadata(projection_path.join("current.txt"))
                    .expect("copied internal symlink")
                    .file_type()
                    .is_symlink(),
                "copy must preserve contained symlinks"
            ),
            ProjectionMethod::Materialize => assert!(
                fs::symlink_metadata(projection_path.join("current.txt"))
                    .expect("materialized file")
                    .is_file(),
                "materialize must rebuild a dereferenced tree"
            ),
        }
        assert_eq!(
            fs::read_to_string(projection_path.join("details.txt")).expect("projected details"),
            "canonical\n"
        );
        let transaction_artifacts = fs::read_dir(&target)
            .expect("target entries")
            .filter_map(|entry| {
                let path = entry.expect("target entry").path();
                path.file_name()
                    .is_some_and(|name| {
                        name.to_string_lossy()
                            .starts_with(".loom-projection-stage-")
                    })
                    .then_some(path)
            })
            .collect::<Vec<_>>();
        assert_eq!(transaction_artifacts, vec![backup_path]);
    }

    assert_eq!(gitops::head(&fixture.ctx).expect("head after"), head_before);
    assert_eq!(registry_snapshot(&fixture.paths), registry_before);
    assert_eq!(
        filesystem_snapshot(&fixture.ctx.state_dir),
        durable_state_before,
        "convergence child must not persist backups or other durable state"
    );
    assert!(!fixture.ctx.state_dir.join("backups").exists());
    assert_eq!(
        fs::read(&fixture.paths.operations_file).expect("operations after"),
        operations_before
    );
}

#[cfg(unix)]
#[test]
fn convergence_observation_failure_atomically_restores_live_projection() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create live projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write live data");
    let state_before = filesystem_snapshot(&fixture.ctx.state_dir);
    let head_before = gitops::head(&fixture.ctx).expect("head before");
    let mut input = execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone());
    input.after_materialize_fault = Some("test_convergence_observation_mismatch");

    let error = match execute_projection(&fixture.ctx, &fixture.paths, &fixture.snapshot, input) {
        Ok(_) => panic!("post-materialization digest mismatch must fail closed"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).expect("restored live data"),
        "keep\n"
    );
    assert!(!projection_path.join("details.txt").exists());
    assert!(
        fs::read_dir(projection_path.parent().expect("projection parent"))
            .expect("target entries")
            .all(|entry| !entry
                .expect("target entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".loom-projection-stage-")),
        "failed convergence must consume its rollback artifact"
    );
    assert_eq!(filesystem_snapshot(&fixture.ctx.state_dir), state_before);
    assert_eq!(gitops::head(&fixture.ctx).expect("head after"), head_before);
}

#[cfg(unix)]
#[test]
fn convergence_staging_failure_preserves_existing_projection() {
    let fixture = convergence_projection_fixture();
    let outside = fixture.root.join("outside.txt");
    fs::write(&outside, "outside\n").expect("write outside file");
    std::os::unix::fs::symlink(&outside, fixture.ctx.skill_path("demo").join("escape.txt"))
        .expect("create escaping symlink");
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create live projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write live data");

    let input = execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone());
    let error = match materialize_projection(&fixture.ctx, &input, None) {
        Ok(_) => panic!("escaping source symlink must fail closed before replacement"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::IoError);
    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).expect("preserved live data"),
        "keep\n"
    );
    assert_eq!(
        fixture
            .paths
            .load_snapshot()
            .expect("unchanged registry")
            .operations
            .len(),
        0
    );
}

#[test]
fn standalone_replacement_retains_portable_persistent_backup_path() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create live projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write live data");
    let mut input = execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone());
    input.context = ProjectionExecutionContext::Standalone;
    input.replace_existing = true;

    let output = materialize_projection(&fixture.ctx, &input, None)
        .expect("standalone replacement remains portable");
    let backup = output.backup.expect("persistent standalone backup");

    assert_eq!(backup["kind"], "dir");
    assert!(
        PathBuf::from(backup["backup_path"].as_str().expect("backup path"))
            .starts_with(fixture.ctx.state_dir.join("backups"))
    );
    assert!(!projection_path.join("keep.txt").exists());
    assert!(projection_path.join("details.txt").exists());
    assert!(
        fs::read_dir(projection_path.parent().expect("projection parent"))
            .expect("target entries")
            .all(|entry| !entry
                .expect("target entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".loom-projection-stage-"))
    );
}
