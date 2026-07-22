use super::projection_recovery::path_matches_backup;
use super::recovery_evidence::corrupt;
use super::registry_recovery::recovery_stale;
use super::*;

pub(super) fn restore_source_from_evidence(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let backup = journal
        .source_backup
        .as_ref()
        .ok_or_else(|| corrupt("projection-input transaction has no source backup"))?;
    let staging = journal
        .source_staging
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| corrupt("projection-input transaction has no source staging"))?;
    let owner_proof = journal
        .source_owner_proof
        .as_deref()
        .ok_or_else(|| corrupt("projection-input transaction has no source owner proof"))?;
    let activated = journal
        .source_activated_fingerprint
        .as_deref()
        .ok_or_else(|| corrupt("source activation fingerprint is missing"))?;
    restore_source_with_hook(
        &app.ctx.skill_path(&plan.skill),
        backup,
        staging,
        &journal.plan_id,
        owner_proof,
        activated,
        |_| {},
    )
}

pub(super) fn finish_uncommitted_source_recovery(
    journal_path: &Path,
    journal: &TransactionJournal,
    errors: Vec<Value>,
) -> std::result::Result<Option<Value>, CommandFailure> {
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "uncommitted source cleanup failed",
        )
        .with_rollback_errors(errors));
    }
    archive_rolled_back_journal(journal_path, journal)?;
    Ok(None)
}

pub(super) fn validate_source_staging_fingerprint(
    live: &Path,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let staging = journal
        .source_staging
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| corrupt("projection-input transaction has no source staging"))?;
    let proof = journal
        .source_owner_proof
        .as_deref()
        .ok_or_else(|| corrupt("projection-input transaction has no source owner proof"))?;
    let expected = journal
        .source_activated_fingerprint
        .as_deref()
        .ok_or_else(|| corrupt("source activation fingerprint is missing"))?;
    validate_owned_staging(live, staging, &journal.plan_id, proof)?;
    require_activated(staging, expected, "source staging before exchange")
}

pub(super) fn validate_activated_source_fingerprint(
    live: &Path,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let expected = journal
        .source_activated_fingerprint
        .as_deref()
        .ok_or_else(|| corrupt("source activation fingerprint is missing"))?;
    require_activated(live, expected, "canonical source after exchange")
}

pub(super) fn restore_source_after_activation_guard(
    live: &Path,
    staging: &Path,
    reviewed: &str,
    mut failure: CommandFailure,
) -> CommandFailure {
    let error = match skill_tree_digest(staging) {
        Ok(actual) if actual == reviewed => exchange_paths_atomic(staging, live)
            .err()
            .map(|error| error.to_string()),
        Ok(_) => Some("displaced source changed; concurrent data was preserved".to_string()),
        Err(error) => Some(format!("cannot validate displaced source: {error}")),
    };
    if let Some(message) = error {
        failure = failure.with_rollback_errors(vec![json!({
            "step": "restore_source_after_activation_guard_failure",
            "message": message,
        })]);
    }
    failure
}

fn restore_source_with_hook<F>(
    live: &Path,
    backup: &Value,
    staging: &Path,
    plan_id: &str,
    owner_proof: &str,
    activated: &str,
    before_exchange: F,
) -> std::result::Result<(), CommandFailure>
where
    F: FnOnce(&Path),
{
    validate_owned_staging(live, staging, plan_id, owner_proof)?;
    let live_exists = live.try_exists().map_err(map_io)?;
    let staging_exists = staging.try_exists().map_err(map_io)?;
    if !live_exists {
        return Err(source_conflict(
            live,
            "live source is missing during rollback",
        ));
    }
    if path_matches_backup(live, backup)? {
        if staging_exists {
            require_activated(staging, activated, "retained source rollback artifact")?;
        }
        return Ok(());
    }
    if staging_exists {
        if !path_matches_backup(staging, backup)? {
            return Err(source_conflict(
                staging,
                "source rollback staging does not match durable backup evidence",
            ));
        }
    } else {
        let candidate = staging.with_file_name(".rollback-restore");
        restore_path_from_backup_if_absent(staging, &candidate, backup).map_err(map_io)?;
        if !path_matches_backup(staging, backup)? {
            return Err(source_conflict(
                staging,
                "restored source staging does not match durable backup evidence",
            ));
        }
    }
    require_activated(live, activated, "live source before rollback")?;
    before_exchange(live);
    exchange_paths_atomic(staging, live).map_err(map_io)?;
    require_activated(staging, activated, "source exchanged during rollback")?;
    if !path_matches_backup(live, backup)? {
        return Err(source_conflict(
            live,
            "restored live source does not match durable backup evidence",
        ));
    }
    Ok(())
}

fn require_activated(
    path: &Path,
    expected: &str,
    label: &str,
) -> std::result::Result<(), CommandFailure> {
    let actual = convergence_projection_fingerprint(path)?;
    if actual == expected {
        Ok(())
    } else {
        Err(source_conflict(
            path,
            &format!("{label} changed after activation; concurrent data was preserved"),
        ))
    }
}

fn source_conflict(path: &Path, message: &str) -> CommandFailure {
    recovery_stale(&format!("{message} at {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl Drop for TestRoot {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.0) {
                eprintln!("failed to remove test root '{}': {error}", self.0.display());
            }
        }
    }

    #[test]
    fn source_rollback_retains_data_racing_after_validation() {
        let root = TestRoot(std::env::temp_dir().join(format!(
            "loom-source-recovery-test-{}",
            uuid::Uuid::new_v4()
        )));
        let live = root.0.join("skills/demo");
        let backup_path = root.0.join("backup");
        fs::create_dir_all(&live).expect("live source");
        fs::create_dir_all(&backup_path).expect("backup source");
        fs::write(live.join("details.txt"), "transaction\n").expect("transaction source");
        fs::write(backup_path.join("details.txt"), "original\n").expect("backup source");
        let plan_id = "plan-source-race";
        let owner = root.0.join("skills/.loom-source-stage.owner");
        let proof = new_owner_proof(plan_id);
        reserve_owned_dir(&owner, plan_id, &proof).expect("source staging owner");
        let staging = owner.join("stage");
        let backup = json!({
            "kind": "dir",
            "original_path": live.display().to_string(),
            "backup_path": backup_path.display().to_string(),
        });
        let activated = convergence_projection_fingerprint(&live).expect("activated source");

        let error = restore_source_with_hook(
            &live,
            &backup,
            &staging,
            plan_id,
            &proof,
            &activated,
            |path| fs::write(path.join("external.txt"), "external\n").expect("race write"),
        )
        .expect_err("racing source rollback must fail closed");

        assert_eq!(error.code, ErrorCode::DependencyConflict);
        assert_eq!(
            fs::read_to_string(live.join("details.txt")).expect("restored source"),
            "original\n"
        );
        assert_eq!(
            fs::read_to_string(staging.join("external.txt")).expect("retained race"),
            "external\n"
        );
    }
}
