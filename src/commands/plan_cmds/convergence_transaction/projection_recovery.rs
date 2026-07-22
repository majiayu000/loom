use super::super::super::projection_executor::convergence_projection_fingerprint;
use super::recovery_evidence::corrupt;
use super::*;
use crate::fs_util::DirectoryHandle;

struct ProjectionRecoveryScope {
    target: DirectoryHandle,
    owner: DirectoryHandle,
    live_name: PathBuf,
    stage_name: PathBuf,
    owner_name: PathBuf,
}

impl ProjectionRecoveryScope {
    fn open(artifact: &ProjectionBackup) -> std::result::Result<Self, CommandFailure> {
        let live = Path::new(&artifact.materialized_path);
        let stage = Path::new(&artifact.staging_path);
        let target_path = live
            .parent()
            .ok_or_else(|| corrupt("projection has no target root"))?;
        let owner_path = stage
            .parent()
            .ok_or_else(|| corrupt("projection has no stage owner"))?;
        if owner_path.parent() != Some(target_path) {
            return Err(corrupt("projection stage owner escaped its target root"));
        }
        let target = DirectoryHandle::open(target_path).map_err(map_io)?;
        let owner_name = owner_path
            .file_name()
            .map(PathBuf::from)
            .ok_or_else(|| corrupt("projection stage owner has no file name"))?;
        let owner = target.open_dir(&owner_name).map_err(map_io)?;
        let live_name = live
            .file_name()
            .map(PathBuf::from)
            .ok_or_else(|| corrupt("projection has no live file name"))?;
        let stage_name = stage
            .file_name()
            .map(PathBuf::from)
            .ok_or_else(|| corrupt("projection has no stage file name"))?;
        Ok(Self {
            target,
            owner,
            live_name,
            stage_name,
            owner_name,
        })
    }

    fn validate_owner(
        &self,
        plan_id: &str,
        proof: &str,
    ) -> std::result::Result<(), CommandFailure> {
        ownership::validate_owned_staging_at(&self.target, &self.owner_name, plan_id, proof)
    }

    fn exchange(&self) -> std::result::Result<(), CommandFailure> {
        self.owner
            .exchange_to(&self.stage_name, &self.target, &self.live_name)
            .map_err(map_io)
    }

    fn retire_created(&self) -> std::result::Result<(), CommandFailure> {
        self.target
            .rename_no_replace_to(&self.live_name, &self.owner, &self.stage_name)
            .map_err(map_io)
    }

    fn staging_exists(&self) -> std::result::Result<bool, CommandFailure> {
        self.owner.entry_exists(&self.stage_name).map_err(map_io)
    }

    fn remove_staging(&self) -> std::result::Result<(), CommandFailure> {
        self.owner.remove_tree(&self.stage_name).map_err(map_io)
    }

    fn restore_staging_from_backup(
        &self,
        backup: &Value,
    ) -> std::result::Result<(), CommandFailure> {
        if self.staging_exists()? {
            return Err(recovery_conflict(
                Path::new(&self.stage_name),
                "refusing to replace recovery staging path",
            ));
        }
        let candidate = Path::new(".rollback-restore");
        if self.owner.entry_exists(candidate).map_err(map_io)? {
            self.owner.remove_tree(candidate).map_err(map_io)?;
        }
        let backup_path = backup["backup_path"]
            .as_str()
            .map(Path::new)
            .ok_or_else(|| corrupt("projection backup path is missing"))?;
        match backup["kind"].as_str() {
            Some("dir") => {
                self.owner.create_dir(candidate).map_err(map_io)?;
                let destination = self.owner.open_dir(candidate).map_err(map_io)?;
                crate::commands::file_ops::copy_dir_recursive_to_handle(
                    backup_path,
                    &destination,
                    true,
                )
                .map_err(|error| CommandFailure::new(ErrorCode::IoError, error.to_string()))?;
            }
            Some("file") => self
                .owner
                .copy_file(backup_path, candidate)
                .map_err(map_io)?,
            Some("symlink") => {
                let raw = fs::read_to_string(backup_path.join("symlink.json")).map_err(map_io)?;
                let payload: Value = serde_json::from_str(&raw).map_err(map_io)?;
                let target = payload["target"]
                    .as_str()
                    .map(Path::new)
                    .ok_or_else(|| corrupt("projection symlink backup target is missing"))?;
                self.owner.symlink(target, candidate).map_err(map_io)?;
            }
            _ => return Err(corrupt("projection backup kind is invalid")),
        }
        self.owner
            .rename_no_replace_to(candidate, &self.owner, &self.stage_name)
            .map_err(map_io)?;
        self.owner.sync().map_err(map_io)
    }
}

pub(super) fn restore_projection_from_evidence(
    artifact: &ProjectionBackup,
    plan_id: &str,
) -> std::result::Result<(), CommandFailure> {
    restore_projection_with_hook(artifact, plan_id, |_| {})
}

pub(super) fn validate_projection_staging_fingerprint(
    artifact: &ProjectionBackup,
) -> std::result::Result<(), CommandFailure> {
    let Some(expected) = artifact.fingerprint() else {
        return Ok(());
    };
    require_fingerprint(
        Path::new(&artifact.staging_path),
        expected,
        "prepared projection staging",
    )
}

pub(super) fn prepare_projection_restore_fingerprint(
    artifact: &ProjectionBackup,
    plan_id: &str,
) -> std::result::Result<Option<String>, CommandFailure> {
    let Some(backup) = artifact.backup.as_ref() else {
        return Ok(None);
    };
    if artifact.restored_fingerprint.is_some() {
        return Ok(None);
    }
    let live = Path::new(&artifact.materialized_path);
    let staging = Path::new(&artifact.staging_path);
    let scope = ProjectionRecoveryScope::open(artifact)?;
    let expected = artifact
        .fingerprint()
        .ok_or_else(|| corrupt("projection activation fingerprint is missing"))?;
    scope.validate_owner(plan_id, &artifact.owner_proof)?;
    require_fingerprint(
        live,
        expected,
        "live projection before rollback preparation",
    )?;
    test_projection_recovery_pause("before_restore_preparation_mutation")?;
    if scope.staging_exists()? {
        let fingerprint = convergence_projection_fingerprint(staging)?;
        if artifact.original_fingerprint.as_deref() == Some(fingerprint.as_str()) {
            if !path_matches_backup(staging, backup)? {
                return Err(recovery_conflict(
                    staging,
                    "retained original staging does not match durable backup evidence",
                ));
            }
            return Ok(Some(fingerprint));
        }
        // The owner proof and exact live activation fingerprint above make this
        // declared private path safe to rebuild. With no persisted restore
        // fingerprint, anything else here is an interrupted restore candidate.
        scope.remove_staging()?;
    }
    scope.restore_staging_from_backup(backup)?;
    if !path_matches_backup(staging, backup)? {
        return Err(recovery_conflict(
            staging,
            "staged rollback backup does not match durable evidence",
        ));
    }
    Ok(Some(convergence_projection_fingerprint(staging)?))
}

#[cfg(debug_assertions)]
fn test_projection_recovery_pause(point: &str) -> std::result::Result<(), CommandFailure> {
    if std::env::var("LOOM_TEST_CONVERGENCE_RECOVERY_SCOPE_PAUSE_POINT")
        .ok()
        .as_deref()
        != Some(point)
    {
        return Ok(());
    }
    let directory = std::env::var_os("LOOM_TEST_CONVERGENCE_RECOVERY_SCOPE_PAUSE_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::IoError,
                "projection recovery scope pause directory is absent",
            )
        })?;
    fs::create_dir_all(&directory).map_err(map_io)?;
    fs::write(directory.join("ready"), point).map_err(map_io)?;
    for _ in 0..2_000 {
        if directory.join("release").try_exists().map_err(map_io)? {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Err(CommandFailure::new(
        ErrorCode::IoError,
        "projection recovery scope test pause timed out",
    ))
}

#[cfg(not(debug_assertions))]
fn test_projection_recovery_pause(_point: &str) -> std::result::Result<(), CommandFailure> {
    Ok(())
}

fn restore_projection_with_hook<F>(
    artifact: &ProjectionBackup,
    plan_id: &str,
    before_atomic_restore: F,
) -> std::result::Result<(), CommandFailure>
where
    F: FnOnce(&Path),
{
    let scope = ProjectionRecoveryScope::open(artifact)?;
    let live = Path::new(&artifact.materialized_path);
    let staging = Path::new(&artifact.staging_path);
    let expected = artifact
        .fingerprint()
        .ok_or_else(|| corrupt("projection activation fingerprint is missing"))?;
    let restored = if artifact.backup.is_some() {
        Some(
            artifact
                .restored_fingerprint
                .as_deref()
                .ok_or_else(|| corrupt("projection restore fingerprint is missing"))?,
        )
    } else {
        None
    };
    scope.validate_owner(plan_id, &artifact.owner_proof)?;

    let live_exists = live.try_exists().map_err(map_io)?;
    let staging_exists = staging.try_exists().map_err(map_io)?;
    if staging_exists {
        if let Some(backup) = artifact.backup.as_ref()
            && live_exists
            && path_matches_backup(staging, backup)?
        {
            let restored =
                restored.ok_or_else(|| corrupt("projection restore fingerprint is missing"))?;
            require_fingerprint(staging, restored, "prepared projection rollback staging")?;
            require_fingerprint(
                live,
                expected,
                "live projection after interrupted activation",
            )?;
            before_atomic_restore(live);
            scope.exchange()?;
            require_fingerprint(staging, expected, "projection exchanged during rollback")?;
            require_fingerprint(live, restored, "restored live projection")?;
            if !path_matches_backup(live, backup)? {
                return Err(recovery_conflict(
                    live,
                    "restored live projection does not match durable backup",
                ));
            }
            return Ok(());
        }
        require_fingerprint(staging, expected, "retained rollback artifact")?;
        let rollback_complete = match artifact.backup.as_ref() {
            Some(backup) => live_exists && path_matches_backup(live, backup)?,
            None => !live_exists,
        };
        if !rollback_complete {
            return Err(recovery_conflict(
                staging,
                "rollback artifact is present but the live projection is not restored",
            ));
        }
        if let Some(restored) = restored {
            require_fingerprint(live, restored, "restored live projection")?;
        }
        return Ok(());
    }
    if !live_exists {
        return if artifact.backup.is_none() {
            Ok(())
        } else {
            Err(recovery_conflict(
                live,
                "refresh rollback has neither live nor retained transaction bytes",
            ))
        };
    }

    require_fingerprint(live, expected, "live projection before rollback")?;
    before_atomic_restore(live);
    if artifact.backup.is_some() {
        return Err(recovery_conflict(
            staging,
            "refresh rollback staging was not durably prepared",
        ));
    }
    scope.retire_created()?;
    require_fingerprint(staging, expected, "projection removed during rollback")?;
    Ok(())
}

fn require_fingerprint(
    path: &Path,
    expected: &str,
    label: &str,
) -> std::result::Result<(), CommandFailure> {
    let actual = convergence_projection_fingerprint(path)?;
    if actual == expected {
        Ok(())
    } else {
        Err(recovery_conflict(
            path,
            &format!("{label} changed after activation; concurrent data was preserved"),
        ))
    }
}

pub(super) fn path_matches_backup(
    path: &Path,
    backup: &Value,
) -> std::result::Result<bool, CommandFailure> {
    let backup_path = backup["backup_path"]
        .as_str()
        .map(Path::new)
        .ok_or_else(|| corrupt("projection backup path is missing"))?;
    match backup["kind"].as_str() {
        Some("dir") => {
            let view = backup["view"].as_str();
            let digest = |candidate: &Path| match view {
                Some(method) => projection_view_digest(candidate, method),
                None => skill_tree_digest(candidate).map_err(map_io),
            };
            Ok(digest(path)? == digest(backup_path)?)
        }
        Some("file") => {
            Ok(fs::read(path).map_err(map_io)? == fs::read(backup_path).map_err(map_io)?)
        }
        Some("symlink") => {
            let raw = fs::read_to_string(backup_path.join("symlink.json")).map_err(map_io)?;
            let payload: Value = serde_json::from_str(&raw).map_err(map_io)?;
            let target = payload["target"]
                .as_str()
                .map(Path::new)
                .ok_or_else(|| corrupt("projection symlink backup target is missing"))?;
            Ok(fs::read_link(path).map_err(map_io)? == target)
        }
        _ => Err(corrupt("projection backup kind is invalid")),
    }
}

fn recovery_conflict(path: &Path, message: &str) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::ProjectionConflict, message);
    failure.details = json!({
        "path": path.display().to_string(),
        "recovery": "preserve the reported path and retry after resolving concurrent data",
    });
    failure
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl Drop for TestRoot {
        fn drop(&mut self) {
            if let Err(err) = fs::remove_dir_all(&self.0) {
                eprintln!("failed to remove test root '{}': {err}", self.0.display());
            }
        }
    }

    fn create_artifact(root: &TestRoot, contents: &str) -> ProjectionBackup {
        let live = root.0.join("live/demo");
        fs::create_dir_all(&live).expect("live projection");
        fs::write(live.join("details.txt"), contents).expect("projection bytes");
        let plan_id = "plan-concurrent-live";
        let owner = root.0.join("live/.loom-projection-stage-owner");
        let owner_proof = new_owner_proof(plan_id);
        reserve_owned_dir(&owner, plan_id, &owner_proof).expect("owned staging");
        ProjectionBackup {
            materialized_path: live.display().to_string(),
            backup: None,
            staging_owner: owner.display().to_string(),
            owner_proof,
            staging_path: owner.join("stage").display().to_string(),
            activated_fingerprint: Some(
                convergence_projection_fingerprint(&live).expect("fingerprint"),
            ),
            activated: true,
            activation_pending: false,
            original_fingerprint: None,
            restored_fingerprint: None,
            restore_pending: false,
        }
    }

    fn test_root() -> TestRoot {
        TestRoot(
            fs::canonicalize(std::env::temp_dir())
                .expect("canonical temp directory")
                .join(format!(
                    "loom-convergence-recovery-test-{}",
                    uuid::Uuid::new_v4()
                )),
        )
    }

    #[test]
    fn create_rollback_preserves_unowned_live_projection() {
        let root = test_root();
        let mut artifact = create_artifact(&root, "transaction\n");
        artifact.activated_fingerprint = Some("not-the-live-fingerprint".to_string());
        let live = Path::new(&artifact.materialized_path);

        let error = restore_projection_from_evidence(&artifact, "plan-concurrent-live")
            .expect_err("unowned live projection must fail closed");

        assert_eq!(error.code, ErrorCode::ProjectionConflict);
        assert_eq!(
            fs::read_to_string(live.join("details.txt")).expect("preserved bytes"),
            "transaction\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn materialize_backup_comparison_uses_follow_symlink_view() {
        let root = test_root();
        let live = root.0.join("live");
        let backup = root.0.join("backup");
        fs::create_dir_all(&live).expect("live");
        fs::create_dir_all(&backup).expect("backup");
        fs::write(live.join("payload"), "same\n").expect("live payload");
        fs::write(backup.join("payload"), "same\n").expect("backup target");
        std::os::unix::fs::symlink("payload", live.join("entry")).expect("live symlink");
        std::os::unix::fs::symlink("./payload", backup.join("entry")).expect("backup symlink");
        let evidence = json!({
            "kind": "dir",
            "backup_path": backup.display().to_string(),
            "view": "materialize",
        });

        assert!(path_matches_backup(&live, &evidence).expect("materialize comparison"));
    }

    #[test]
    fn create_rollback_retains_data_racing_after_validation() {
        let root = test_root();
        let artifact = create_artifact(&root, "transaction\n");

        let error = restore_projection_with_hook(&artifact, "plan-concurrent-live", |path| {
            fs::write(path.join("external.txt"), "external\n").expect("race write")
        })
        .expect_err("racing live projection must fail closed");

        assert_eq!(error.code, ErrorCode::ProjectionConflict);
        let retained = Path::new(&artifact.staging_path);
        assert_eq!(
            fs::read_to_string(retained.join("external.txt")).expect("retained race bytes"),
            "external\n"
        );
    }

    #[test]
    fn refresh_rollback_retains_data_racing_after_validation() {
        let root = test_root();
        let mut artifact = create_artifact(&root, "transaction\n");
        let live = Path::new(&artifact.materialized_path);
        let backup = root.0.join("backup");
        fs::create_dir_all(&backup).expect("backup directory");
        fs::write(backup.join("details.txt"), "original\n").expect("backup bytes");
        artifact.backup = Some(json!({
            "kind": "dir",
            "original_path": live.display().to_string(),
            "backup_path": backup.display().to_string(),
        }));
        artifact.restored_fingerprint =
            prepare_projection_restore_fingerprint(&artifact, "plan-concurrent-live")
                .expect("prepare restore fingerprint");

        let error = restore_projection_with_hook(&artifact, "plan-concurrent-live", |path| {
            fs::write(path.join("external.txt"), "external\n").expect("race write")
        })
        .expect_err("racing refreshed projection must fail closed");

        assert_eq!(error.code, ErrorCode::ProjectionConflict);
        assert_eq!(
            fs::read_to_string(live.join("details.txt")).expect("restored original bytes"),
            "original\n"
        );
        let retained = Path::new(&artifact.staging_path);
        assert_eq!(
            fs::read_to_string(retained.join("external.txt")).expect("retained race bytes"),
            "external\n"
        );
    }
}
