use super::super::super::projection_executor::convergence_projection_fingerprint;
use super::recovery_evidence::corrupt;
use super::*;

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
    let expected = artifact
        .fingerprint()
        .ok_or_else(|| corrupt("projection activation fingerprint is missing"))?;
    validate_owned_staging(live, staging, plan_id, &artifact.owner_proof)?;
    require_fingerprint(
        live,
        expected,
        "live projection before rollback preparation",
    )?;
    if staging.try_exists().map_err(map_io)? {
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
        crate::fs_util::remove_path_if_exists(staging).map_err(map_io)?;
    }
    restore_path_from_backup_if_absent(staging, backup).map_err(map_io)?;
    if !path_matches_backup(staging, backup)? {
        return Err(recovery_conflict(
            staging,
            "staged rollback backup does not match durable evidence",
        ));
    }
    Ok(Some(convergence_projection_fingerprint(staging)?))
}

fn restore_projection_with_hook<F>(
    artifact: &ProjectionBackup,
    plan_id: &str,
    before_atomic_restore: F,
) -> std::result::Result<(), CommandFailure>
where
    F: FnOnce(&Path),
{
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
    validate_owned_staging(live, staging, plan_id, &artifact.owner_proof)?;

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
            exchange_paths_atomic(staging, live).map_err(map_io)?;
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
    rename_no_replace_atomic(live, staging).map_err(map_io)?;
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
            original_fingerprint: None,
            restored_fingerprint: None,
        }
    }

    fn test_root() -> TestRoot {
        TestRoot(std::env::temp_dir().join(format!(
            "loom-convergence-recovery-test-{}",
            uuid::Uuid::new_v4()
        )))
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
