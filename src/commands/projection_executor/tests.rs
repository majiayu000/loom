use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::*;

mod convergence_transaction;
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
        source_path: None,
        staging_path: None,
        materialized_path,
        method,
        operation_intent: "skill.converge.child",
        operation_payload: json!({"must_not_persist": true}),
        observation_kind: "converged_child",
        request_id: "convergence-child-must-not-run".to_string(),
        commit_message: "must not commit convergence child".to_string(),
        replace_existing: true,
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

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn symlink_copy_materialize_convergence_mode_has_no_child_persistence() {
    let fixture = convergence_projection_fixture();
    let head_before = gitops::head(&fixture.ctx).expect("head before");
    let registry_before = registry_snapshot(&fixture.paths);
    let operations_before = fs::read(&fixture.paths.operations_file).expect("operations before");
    let durable_state_before = filesystem_snapshot(&fixture.ctx.state_dir);

    let mut prepared_outputs = Vec::new();
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
        let prepared = output.prepared.expect("validated staging artifact");

        assert_eq!(projection.health, Health::Healthy);
        assert_eq!(projection.observed_drift, Some(false));
        assert!(output.backup.is_none(), "prepare must not expose a backup");
        assert!(projection_path.join("stale.txt").is_file());
        assert!(output.commit.is_none(), "convergence child must not commit");
        assert!(
            output.meta.op_id.is_none(),
            "convergence child must not record op"
        );
        assert!(!output.noop, "stale target must be rebuilt");
        prepared_outputs.push((method, target, projection_path, projection, prepared));
    }

    assert!(
        prepared_outputs
            .iter()
            .all(|(_, _, path, _, _)| path.join("stale.txt").is_file()),
        "every projection must remain live until all staging is validated"
    );

    for (method, target, projection_path, _projection, prepared) in prepared_outputs {
        let mut activated = activate_prepared_projection(&fixture.ctx, prepared)
            .expect("activate validated convergence projection");
        let backup = activated.rollback_evidence();
        assert_eq!(backup["kind"], json!("atomic_exchange"));
        let backup_path = PathBuf::from(backup["backup_path"].as_str().expect("backup path"));
        assert_eq!(backup_path.parent(), projection_path.parent());
        assert!(backup_path.join("stale.txt").is_file());
        assert!(!backup_path.starts_with(&fixture.ctx.state_dir));
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
        let activated_projection = activated
            .finalize()
            .expect("finalize activated convergence projection");
        assert_eq!(
            activated_projection.materialized_path,
            projection_path.display().to_string()
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
        assert!(transaction_artifacts.is_empty());
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

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn convergence_post_activation_failure_atomically_restores_live_projection() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create live projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write live data");
    let state_before = filesystem_snapshot(&fixture.ctx.state_dir);
    let head_before = gitops::head(&fixture.ctx).expect("head before");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare convergence projection");
    let prepared = output.prepared.expect("staging artifact");
    let staging_path = prepared.staging_path().to_path_buf();
    fs::write(prepared.staging_path().join("details.txt"), "tampered\n")
        .expect("tamper validated staging before activation");

    let error = match activate_prepared_projection(&fixture.ctx, prepared) {
        Ok(_) => panic!("post-activation digest mismatch must fail closed"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).expect("restored live data"),
        "keep\n"
    );
    assert!(!projection_path.join("details.txt").exists());
    assert!(staging_path.join("details.txt").is_file());
    assert_eq!(error.details["recovery_required"], true);
    assert_eq!(filesystem_snapshot(&fixture.ctx.state_dir), state_before);
    assert_eq!(gitops::head(&fixture.ctx).expect("head after"), head_before);
}

#[cfg(unix)]
#[test]
fn convergence_source_staging_failure_preserves_existing_projection() {
    let fixture = convergence_projection_fixture();
    let outside = fixture.root.join("outside.txt");
    fs::write(&outside, "outside\n").expect("write outside file");
    std::os::unix::fs::symlink(&outside, fixture.ctx.skill_path("demo").join("escape.txt"))
        .expect("create escaping symlink");
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create live projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write live data");

    let input = execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone());
    let error = match execute_projection(&fixture.ctx, &fixture.paths, &fixture.snapshot, input) {
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

#[cfg(unix)]
#[test]
fn project_failure_preserves_unanchored_caller_staging_replacement() {
    let fixture = convergence_projection_fixture();
    let outside = fixture.root.join("outside.txt");
    fs::write(&outside, "outside\n").unwrap();
    std::os::unix::fs::symlink(&outside, fixture.ctx.skill_path("demo").join("escape.txt"))
        .unwrap();
    let staging = fixture.root.join("live/copy/caller-stage");
    fs::create_dir_all(staging.parent().unwrap()).unwrap();
    let mut input = execution_input(
        &fixture,
        ProjectionMethod::Copy,
        fixture.root.join("live/copy/demo"),
    );
    input.staging_path = Some(staging.clone());
    input.after_materialize_fault = Some("test_convergence_project_failure_replacement");

    let error = match execute_projection(&fixture.ctx, &fixture.paths, &fixture.snapshot, input) {
        Err(error) => error,
        Ok(_) => panic!("project failure must preserve unverified replacement"),
    };

    assert!(staging.join("external.txt").is_file());
    assert_eq!(
        error.details["rollback_errors"][0]["recovery_required"],
        true
    );
}

#[test]
fn fingerprint_failure_preserves_unanchored_caller_staging_replacement() {
    let fixture = convergence_projection_fixture();
    let staging = fixture.root.join("live/copy/caller-stage");
    let mut input = execution_input(
        &fixture,
        ProjectionMethod::Copy,
        fixture.root.join("live/copy/demo"),
    );
    input.staging_path = Some(staging.clone());
    input.after_materialize_fault = Some("test_convergence_fingerprint_failure_replacement");

    let error = match execute_projection(&fixture.ctx, &fixture.paths, &fixture.snapshot, input) {
        Err(error) => error,
        Ok(_) => panic!("fingerprint failure must preserve unverified replacement"),
    };

    assert!(staging.join("external.txt").is_file());
    assert_eq!(
        error.details["rollback_errors"][0]["recovery_required"],
        true
    );
}

#[test]
fn observation_failure_claims_and_preserves_changed_caller_staging() {
    let fixture = convergence_projection_fixture();
    let staging = fixture.root.join("live/copy/caller-stage");
    let mut input = execution_input(
        &fixture,
        ProjectionMethod::Copy,
        fixture.root.join("live/copy/demo"),
    );
    input.staging_path = Some(staging.clone());
    input.after_materialize_fault = Some("test_convergence_observation_failure_replacement");

    let error = match execute_projection(&fixture.ctx, &fixture.paths, &fixture.snapshot, input) {
        Err(error) => error,
        Ok(_) => panic!("changed staging must fail observation"),
    };
    let claim = PathBuf::from(
        error.details["rollback_errors"][0]["details"]["claim_path"]
            .as_str()
            .expect("preserved claim path"),
    );

    assert!(!staging.exists());
    assert_eq!(
        fs::read_to_string(claim.join("external.txt")).unwrap(),
        "external\n"
    );
}

#[test]
fn convergence_activation_preserves_destination_created_after_prepare() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare absent convergence projection");
    let prepared = output.prepared.expect("staging artifact");
    let staging_path = prepared.staging_path().to_path_buf();
    fs::create_dir_all(&projection_path).expect("create concurrent destination");
    fs::write(projection_path.join("concurrent.txt"), "concurrent\n")
        .expect("write concurrent destination");

    let error = match activate_prepared_projection(&fixture.ctx, prepared) {
        Ok(_) => panic!("concurrent destination must fail closed"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(
        fs::read_to_string(projection_path.join("concurrent.txt")).unwrap(),
        "concurrent\n"
    );
    assert!(!projection_path.join("details.txt").exists());
    assert!(
        !staging_path.exists(),
        "failed activation must clean staging"
    );
}

#[test]
fn convergence_prepared_projection_owns_activation_identity() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let mut output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare convergence projection");
    let mut detached_projection = output.projection.take().expect("projection delta");
    detached_projection.materialized_path = fixture.root.join("wrong/demo").display().to_string();

    let mut activated = activate_prepared_projection(
        &fixture.ctx,
        output.prepared.expect("identity-bound staging artifact"),
    )
    .expect("activate identity-bound prepared projection");

    assert_eq!(
        activated.projection().materialized_path,
        projection_path.display().to_string()
    );
    activated.finalize().expect("finalize projection");
    assert!(!fixture.root.join("wrong/demo").exists());
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn convergence_activation_rollback_restores_typed_artifact() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create existing projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write existing projection");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare replacement");
    let mut activated =
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate replacement");
    assert!(projection_path.join("details.txt").is_file());

    activated.rollback().expect("rollback typed artifact");

    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert!(!projection_path.join("details.txt").exists());
}

#[test]
fn abandoned_prepared_projection_cleans_staging_on_drop() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare convergence projection");
    let prepared = output.prepared.expect("staging artifact");
    let staging_path = prepared.staging_path().to_path_buf();
    assert!(staging_path.exists());

    drop(prepared);

    assert!(!staging_path.exists());
    assert!(!projection_path.exists());
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn abandoned_activation_rolls_back_on_drop() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create existing projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write existing projection");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare replacement");
    let activated =
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate replacement");
    assert!(projection_path.join("details.txt").is_file());

    drop(activated);

    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert!(!projection_path.join("details.txt").exists());
}

#[test]
fn convergence_staging_validation_rejects_bytes_before_live_swap() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create live projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write live data");
    let mut input = execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone());
    input.after_materialize_fault = Some("test_convergence_staging_mismatch");

    let error = match execute_projection(&fixture.ctx, &fixture.paths, &fixture.snapshot, input) {
        Ok(_) => panic!("invalid staging must fail before publication"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).unwrap(),
        "keep\n"
    );
    let preserved = fs::read_dir(projection_path.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .ends_with(".staging-cleanup-claim")
        })
        .expect("mismatched staging must remain atomically claimed for recovery");
    assert!(preserved.path().join("details.txt").is_file());
}

#[test]
fn convergence_head_failure_happens_before_staging_or_live_mutation() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create live projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write live data");
    let git_head = fixture.root.join(".git/HEAD");
    let original_head = fs::read(&git_head).expect("read HEAD");
    fs::write(&git_head, "broken-head\n").expect("break HEAD");

    let result = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    );
    fs::write(&git_head, original_head).expect("restore HEAD");

    assert!(result.is_err(), "broken HEAD must fail closed");
    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert_eq!(
        fs::read_dir(projection_path.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn convergence_safe_existing_flag_cannot_skip_copy_rebuild() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create live projection");
    fs::write(projection_path.join("stale.txt"), "stale\n").expect("write stale data");
    let mut input = execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone());
    input.safe_existing_noop = true;

    let output = execute_projection(&fixture.ctx, &fixture.paths, &fixture.snapshot, input)
        .expect("prepare copy rebuild");

    assert!(output.prepared.is_some(), "copy must still be rebuilt");
    assert!(projection_path.join("stale.txt").is_file());
    discard_prepared_projection(output.prepared.expect("prepared copy")).expect("discard staging");
}

#[cfg(unix)]
#[test]
fn convergence_canonical_symlink_noop_does_not_require_writable_parent() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = convergence_projection_fixture();
    let target = fixture.root.join("live/symlink");
    let projection_path = target.join("demo");
    fs::create_dir_all(&target).expect("create target");
    std::os::unix::fs::symlink(fixture.ctx.skill_path("demo"), &projection_path)
        .expect("create canonical symlink");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o555)).expect("lock target");

    let result = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Symlink, projection_path),
    );
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).expect("unlock target");

    let output = result.expect("canonical symlink validation must be read-only");
    assert!(output.prepared.is_none());
    assert_eq!(
        output.projection.expect("projection").health,
        Health::Healthy
    );
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
)))]
#[test]
fn convergence_existing_path_fails_closed_without_atomic_exchange() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create live projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write live data");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare projection");

    let error = match activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging"))
    {
        Ok(_) => panic!("unsupported exchange must fail closed"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::ProjectionMethodUnsupported);
    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).unwrap(),
        "keep\n"
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

    let output = execute_projection(&fixture.ctx, &fixture.paths, &fixture.snapshot, input)
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
