use super::recovery_evidence::file_digest;
use super::registry_recovery::{validate_registry_result, verify_commit};
use super::*;

const REGISTRY_PATH: &str = "state/registry/projections.json";

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct RegistryIndexAttempt {
    purpose: String,
    generation: String,
    base_index: String,
    prepared_index: String,
    commit_index: String,
    base_digest: Option<String>,
    prepared_digest: Option<String>,
    commit_digest: Option<String>,
    #[serde(default)]
    changed: Option<bool>,
    state: RegistryIndexAttemptState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RegistryIndexAttemptState {
    Allocated,
    Ready,
    Abandoned,
    Retained,
}

pub(super) fn durable_registry_noop(journal: &TransactionJournal) -> bool {
    journal.registry_commit.is_none()
        && journal.registry_staged_index_digest.is_none()
        && journal
            .expected_projections
            .as_ref()
            .is_some_and(|expected| {
                serde_json::to_value(expected).ok()
                    == serde_json::to_value(&journal.original_projections).ok()
            })
        && registry_index_attempts_valid(journal)
        && journal.registry_index_attempts.iter().any(|attempt| {
            attempt.purpose == "commit"
                && attempt.state == RegistryIndexAttemptState::Retained
                && attempt.changed == Some(false)
                && attempt.base_digest.is_some()
                && attempt.prepared_digest.is_some()
        })
}

pub(super) fn discard_retained_registry_index_locks(
    app: &App,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    for attempt in &journal.registry_index_attempts {
        let prepared = Path::new(&attempt.prepared_index);
        if gitops::prepared_index_claim_exists(&app.ctx, prepared).map_err(map_git)? {
            gitops::discard_prepared_index_lock(&app.ctx, prepared)
                .map_err(super::index_lock_failure::map_install_error)?;
        }
    }
    Ok(())
}

pub(super) fn commit_convergence_registry(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<Option<String>, CommandFailure> {
    let source_head = journal.source_head.clone().ok_or_else(|| {
        CommandFailure::new(ErrorCode::StateCorrupt, "journal is missing source head")
    })?;
    validate_registry_result(app, plan, journal)?;
    require_head(
        app,
        &source_head,
        "registry commit parent changed before preparation",
    )?;
    if let Some(commit) =
        resume_ready_registry_index_lock(app, plan, journal_path, journal, &source_head)?
    {
        return Ok(Some(commit));
    }

    let (attempt, base_index, prepared_index, commit_index) =
        allocate_registry_indexes(journal_path, journal, "commit")?;
    gitops::snapshot_index_to(&app.ctx, &base_index).map_err(map_git)?;
    sync_registry_index(&base_index)?;
    let base_index_digest = file_digest(&base_index)?;
    journal.registry_index_attempts[attempt].base_digest = Some(base_index_digest.clone());
    save_journal(journal_path, journal)?;
    let changed =
        gitops::prepare_index_for_paths(&app.ctx, &base_index, &prepared_index, &[REGISTRY_PATH])
            .map_err(map_git)?;
    sync_registry_index(&prepared_index)?;
    let expected_index = file_digest(&prepared_index)?;
    journal.registry_index_attempts[attempt].prepared_digest = Some(expected_index.clone());
    journal.registry_index_attempts[attempt].changed = Some(changed);
    if !changed {
        journal.registry_index_attempts[attempt].state = RegistryIndexAttemptState::Ready;
    }
    save_journal(journal_path, journal)?;
    if !changed {
        require_head(app, &source_head, "no-op registry commit changed HEAD")?;
        validate_registry_result(app, plan, journal)?;
        retain_registry_index_attempt(journal_path, journal, attempt)?;
        return Ok(None);
    }

    let message = format!("skill({}): record convergence projections", plan.skill);
    let commit = gitops::create_prepared_commit_retaining_index(
        &app.ctx,
        &prepared_index,
        &commit_index,
        &[REGISTRY_PATH],
        &source_head,
        &message,
    )
    .map_err(map_git)?;
    verify_commit(app, &commit, &source_head, &message, |path| {
        path == REGISTRY_PATH
    })?;
    journal.registry_index_attempts[attempt].commit_digest = Some(file_digest(&commit_index)?);
    journal.registry_index_attempts[attempt].state = RegistryIndexAttemptState::Ready;
    journal.registry_commit = Some(commit.clone());
    journal.registry_staged_index_digest = Some(expected_index.clone());
    save_journal(journal_path, journal)?;
    #[cfg(debug_assertions)]
    if let Some(milliseconds) = std::env::var("LOOM_TEST_CONVERGENCE_REGISTRY_CAS_PAUSE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(milliseconds.min(2_000)));
    }
    let install =
        gitops::install_prepared_index_with_guard(&app.ctx, &prepared_index, &|candidate| {
            validate_registry_result(app, plan, journal)
                .map_err(|error| anyhow::anyhow!(error.message))?;
            validate_recovery_routing(app, plan).map_err(|error| anyhow::anyhow!(error.message))?;
            validate_index_install(
                app,
                candidate,
                &expected_index,
                &base_index_digest,
                &source_head,
            )?;
            maybe_skill_fault("convergence_interrupt_before_registry_cas")
                .map_err(|error| anyhow::anyhow!(error.message))?;
            gitops::move_head_if_unchanged(&app.ctx, &commit, &source_head)
        });
    if let Err(error) = install {
        let failure = super::index_lock_failure::map_install_error(error);
        if super::index_lock_failure::retained(&failure) {
            return Err(failure);
        }
        if gitops::head(&app.ctx).map_err(map_git)? == commit {
            align_registry_index(app, plan, journal_path, journal, &commit)?;
        } else {
            return Err(failure);
        }
    }
    require_head(
        app,
        &commit,
        "registry commit compare-and-swap did not persist",
    )?;
    validate_registry_result(app, plan, journal)?;
    retain_registry_index_attempt(journal_path, journal, attempt)?;
    Ok(Some(commit))
}

pub(super) fn resume_ready_registry_index_lock(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
    source_head: &str,
) -> std::result::Result<Option<String>, CommandFailure> {
    let Some((attempt, ready)) = journal
        .registry_index_attempts
        .iter()
        .enumerate()
        .rev()
        .find(|(_, attempt)| {
            attempt.purpose == "commit"
                && attempt.state == RegistryIndexAttemptState::Ready
                && attempt.changed == Some(true)
        })
    else {
        return Ok(None);
    };
    let prepared_index = PathBuf::from(&ready.prepared_index);
    let expected_index = ready.prepared_digest.clone().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "prepared registry index digest is missing",
        )
    })?;
    let base_index_digest = ready.base_digest.clone().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "base registry index digest is missing",
        )
    })?;
    let commit = journal.registry_commit.clone().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "prepared registry commit is missing",
        )
    })?;
    super::registry_commit_evidence::verify_registry_commit(
        app,
        plan,
        journal,
        &commit,
        source_head,
    )?;
    let guard = |candidate: &Path| {
        validate_registry_result(app, plan, journal)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        validate_recovery_routing(app, plan).map_err(|error| anyhow::anyhow!(error.message))?;
        let head = gitops::head(&app.ctx)?;
        if head != source_head && head != commit {
            return Err(anyhow::anyhow!("registry recovery HEAD changed"));
        }
        validate_index_install(app, candidate, &expected_index, &base_index_digest, &head)?;
        if head == source_head {
            gitops::move_head_if_unchanged(&app.ctx, &commit, source_head)?;
        }
        Ok(())
    };
    if !gitops::recover_prepared_index_lock_with_guard(&app.ctx, &prepared_index, &guard)
        .map_err(super::index_lock_failure::map_install_error)?
    {
        return Ok(None);
    }
    require_head(app, &commit, "recovered registry commit did not persist")?;
    retain_registry_index_attempt(journal_path, journal, attempt)?;
    Ok(Some(commit))
}

pub(super) fn align_registry_index(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
    expected_head: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_registry_result(app, plan, journal)?;
    if let Some(recorded) = journal.registry_commit.as_deref()
        && recorded != expected_head
    {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "recorded registry commit differs from HEAD",
        ));
    }
    if let Some(attempt) = recover_recorded_registry_index_lock(app, journal, expected_head)? {
        retain_registry_index_attempt(journal_path, journal, attempt)?;
        return Ok(());
    }
    let staged = gitops::run_git_allow_failure(
        &app.ctx,
        &["diff", "--cached", "--quiet", "--", REGISTRY_PATH],
    )
    .map_err(map_git)?;
    if staged.status.success() {
        retain_current_registry_index_attempt(journal_path, journal)?;
        return Ok(());
    }
    let (attempt, base_index, prepared_index, _) =
        allocate_registry_indexes(journal_path, journal, "repair")?;
    gitops::snapshot_index_to(&app.ctx, &base_index).map_err(map_git)?;
    sync_registry_index(&base_index)?;
    let base_index_digest = file_digest(&base_index)?;
    journal.registry_index_attempts[attempt].base_digest = Some(base_index_digest.clone());
    save_journal(journal_path, journal)?;
    let changed =
        gitops::prepare_index_for_paths(&app.ctx, &base_index, &prepared_index, &[REGISTRY_PATH])
            .map_err(super::index_lock_failure::map_install_error)?;
    sync_registry_index(&prepared_index)?;
    let expected_index = file_digest(&prepared_index)?;
    journal.registry_index_attempts[attempt].prepared_digest = Some(expected_index.clone());
    journal.registry_index_attempts[attempt].changed = Some(changed);
    journal.registry_index_attempts[attempt].state = RegistryIndexAttemptState::Ready;
    save_journal(journal_path, journal)?;
    if let Some(recorded) = journal.registry_staged_index_digest.as_deref()
        && recorded != expected_index
    {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "registry recovery index differs from durable transaction evidence",
        ));
    }
    let guard = |candidate: &Path| {
        validate_index_install(
            app,
            candidate,
            &expected_index,
            &base_index_digest,
            expected_head,
        )
    };
    let recovered_lock =
        gitops::recover_prepared_index_lock_with_guard(&app.ctx, &prepared_index, &guard)
            .map_err(super::index_lock_failure::map_install_error)?;
    if recovered_lock {
        retain_registry_index_attempt(journal_path, journal, attempt)?;
        return Ok(());
    }
    gitops::install_prepared_index_with_guard(&app.ctx, &prepared_index, &guard)
        .map_err(super::index_lock_failure::map_install_error)?;
    retain_registry_index_attempt(journal_path, journal, attempt)
}

fn recover_recorded_registry_index_lock(
    app: &App,
    journal: &TransactionJournal,
    expected_head: &str,
) -> std::result::Result<Option<usize>, CommandFailure> {
    for (index, attempt) in journal.registry_index_attempts.iter().enumerate().rev() {
        if attempt.state != RegistryIndexAttemptState::Ready || attempt.changed != Some(true) {
            continue;
        }
        let prepared = Path::new(&attempt.prepared_index);
        if !gitops::prepared_index_claim_exists(&app.ctx, prepared).map_err(map_git)? {
            continue;
        }
        let expected_index = attempt.prepared_digest.as_deref().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "recorded registry index claim has no prepared digest",
            )
        })?;
        let base_index = attempt.base_digest.as_deref().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "recorded registry index claim has no base digest",
            )
        })?;
        let recovered =
            gitops::recover_prepared_index_lock_with_guard(&app.ctx, prepared, &|candidate| {
                validate_index_install(app, candidate, expected_index, base_index, expected_head)
            })
            .map_err(super::index_lock_failure::map_install_error)?;
        if recovered {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn allocate_registry_indexes(
    journal_path: &Path,
    journal: &mut TransactionJournal,
    purpose: &str,
) -> std::result::Result<(usize, PathBuf, PathBuf, PathBuf), CommandFailure> {
    let generation = uuid::Uuid::new_v4().hyphenated().to_string();
    let root = Path::new(&journal.artifact_root);
    let base_index = root.join(format!("registry-{purpose}-{generation}-base-index"));
    let prepared_index = root.join(format!("registry-{purpose}-{generation}-prepared-index"));
    let commit_index = root.join(format!("registry-{purpose}-{generation}-commit-index"));
    for attempt in &mut journal.registry_index_attempts {
        if !matches!(
            attempt.state,
            RegistryIndexAttemptState::Abandoned | RegistryIndexAttemptState::Retained
        ) {
            attempt.state = RegistryIndexAttemptState::Abandoned;
        }
    }
    journal.registry_index_attempts.push(RegistryIndexAttempt {
        purpose: purpose.to_string(),
        generation,
        base_index: base_index.display().to_string(),
        prepared_index: prepared_index.display().to_string(),
        commit_index: commit_index.display().to_string(),
        base_digest: None,
        prepared_digest: None,
        commit_digest: None,
        changed: None,
        state: RegistryIndexAttemptState::Allocated,
    });
    let attempt = journal.registry_index_attempts.len() - 1;
    save_journal(journal_path, journal)?;
    Ok((attempt, base_index, prepared_index, commit_index))
}

fn retain_registry_index_attempt(
    journal_path: &Path,
    journal: &mut TransactionJournal,
    attempt: usize,
) -> std::result::Result<(), CommandFailure> {
    if journal.registry_index_attempts[attempt].state != RegistryIndexAttemptState::Ready {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "registry index attempt is not ready for retention",
        ));
    }
    journal.registry_index_attempts[attempt].state = RegistryIndexAttemptState::Retained;
    save_journal(journal_path, journal)
}

fn retain_current_registry_index_attempt(
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    if let Some(attempt) = journal
        .registry_index_attempts
        .iter_mut()
        .rev()
        .find(|attempt| {
            !matches!(
                attempt.state,
                RegistryIndexAttemptState::Abandoned | RegistryIndexAttemptState::Retained
            )
        })
    {
        attempt.state = if attempt.state == RegistryIndexAttemptState::Ready {
            RegistryIndexAttemptState::Retained
        } else {
            RegistryIndexAttemptState::Abandoned
        };
        save_journal(journal_path, journal)?;
    }
    Ok(())
}

pub(super) fn terminalize_registry_index_attempts(
    journal: &mut TransactionJournal,
    committed: bool,
) {
    for attempt in &mut journal.registry_index_attempts {
        if !matches!(
            attempt.state,
            RegistryIndexAttemptState::Abandoned | RegistryIndexAttemptState::Retained
        ) {
            attempt.state = if committed && attempt.state == RegistryIndexAttemptState::Ready {
                RegistryIndexAttemptState::Retained
            } else {
                RegistryIndexAttemptState::Abandoned
            };
        }
    }
}

pub(super) fn registry_index_attempts_valid(journal: &TransactionJournal) -> bool {
    let root = Path::new(&journal.artifact_root);
    let mut generations = std::collections::BTreeSet::new();
    let paths_valid = journal.registry_index_attempts.iter().all(|attempt| {
        matches!(attempt.purpose.as_str(), "commit" | "repair")
            && uuid::Uuid::parse_str(&attempt.generation)
                .ok()
                .is_some_and(|value| value.hyphenated().to_string() == attempt.generation)
            && generations.insert(attempt.generation.as_str())
            && Path::new(&attempt.base_index)
                == root.join(format!(
                    "registry-{}-{}-base-index",
                    attempt.purpose, attempt.generation
                ))
            && Path::new(&attempt.prepared_index)
                == root.join(format!(
                    "registry-{}-{}-prepared-index",
                    attempt.purpose, attempt.generation
                ))
            && Path::new(&attempt.commit_index)
                == root.join(format!(
                    "registry-{}-{}-commit-index",
                    attempt.purpose, attempt.generation
                ))
            && [
                attempt.base_digest.as_deref(),
                attempt.prepared_digest.as_deref(),
                attempt.commit_digest.as_deref(),
            ]
            .into_iter()
            .flatten()
            .all(valid_digest)
            && registry_index_attempt_evidence_valid(attempt)
    });
    let evidence_valid = journal.registry_commit.is_none()
        || journal
            .registry_staged_index_digest
            .as_deref()
            .is_some_and(|digest| {
                journal.registry_index_attempts.iter().any(|attempt| {
                    attempt.purpose == "commit"
                        && attempt.prepared_digest.as_deref() == Some(digest)
                })
            });
    let terminal_valid = !matches!(
        journal.phase,
        TransactionPhase::CommittedCleanupPending
            | TransactionPhase::CommittedArtifactsRetained
            | TransactionPhase::RolledBackCleanupPending
            | TransactionPhase::RolledBackArtifactsRetained
    ) || journal.registry_index_attempts.iter().all(|attempt| {
        matches!(
            attempt.state,
            RegistryIndexAttemptState::Abandoned | RegistryIndexAttemptState::Retained
        )
    });
    paths_valid && evidence_valid && terminal_valid
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn registry_index_attempt_evidence_valid(attempt: &RegistryIndexAttempt) -> bool {
    if attempt.state == RegistryIndexAttemptState::Abandoned {
        return true;
    }
    let recorded_bytes_are_exact = [
        (&attempt.base_index, attempt.base_digest.as_deref()),
        (&attempt.prepared_index, attempt.prepared_digest.as_deref()),
        (&attempt.commit_index, attempt.commit_digest.as_deref()),
    ]
    .into_iter()
    .all(|(path, digest)| {
        digest.is_none_or(|digest| file_digest(Path::new(path)).ok().as_deref() == Some(digest))
    });
    if !recorded_bytes_are_exact {
        return false;
    }
    match attempt.state {
        RegistryIndexAttemptState::Allocated => true,
        RegistryIndexAttemptState::Ready | RegistryIndexAttemptState::Retained => {
            attempt.base_digest.is_some()
                && attempt.prepared_digest.is_some()
                && attempt.changed.is_some()
                && if attempt.purpose == "commit" && attempt.changed == Some(true) {
                    attempt.commit_digest.is_some()
                } else {
                    attempt.commit_digest.is_none()
                }
        }
        RegistryIndexAttemptState::Abandoned => true,
    }
}

fn sync_registry_index(path: &Path) -> std::result::Result<(), CommandFailure> {
    crate::fs_util::sync_file_and_parent(path).map_err(map_io)
}

fn validate_index_install(
    app: &App,
    candidate: &Path,
    expected_candidate: &str,
    expected_active: &str,
    expected_head: &str,
) -> anyhow::Result<()> {
    let actual = file_digest(candidate).map_err(|error| anyhow::anyhow!(error.message))?;
    let active = active_index_digest(app).map_err(|error| anyhow::anyhow!(error.message))?;
    let head = gitops::head(&app.ctx)?;
    if actual != expected_candidate || active != expected_active || head != expected_head {
        return Err(anyhow::anyhow!("registry index install guard changed"));
    }
    Ok(())
}

pub(super) fn require_head(
    app: &App,
    expected: &str,
    message: &str,
) -> std::result::Result<(), CommandFailure> {
    if gitops::head(&app.ctx).map_err(map_git)? == expected {
        Ok(())
    } else {
        Err(CommandFailure::new(ErrorCode::StateCorrupt, message))
    }
}
