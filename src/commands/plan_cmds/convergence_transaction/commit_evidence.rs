use super::recovery_support::{recovery_stale, verify_commit};
use super::*;
use crate::sha256::{Sha256, to_hex};

pub(super) fn verify_registry_commit(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    head: &str,
    source_head: &str,
) -> std::result::Result<(), CommandFailure> {
    verify_commit(
        app,
        head,
        source_head,
        &format!("skill({}): record convergence projections", plan.skill),
        super::aggregate_audit::is_registry_commit_path,
    )?;
    let expected = journal
        .expected_projections
        .as_ref()
        .ok_or_else(|| corrupt("missing expected projections"))?;
    let raw = gitops::run_git(
        &app.ctx,
        &["show", &format!("{head}:state/registry/projections.json")],
    )
    .map_err(map_git)?;
    let committed: RegistryProjectionsFile = serde_json::from_str(&raw)
        .map_err(|_| corrupt("registry commit projections are invalid"))?;
    if committed != *expected {
        return Err(recovery_stale(
            "registry commit tree differs from transaction evidence",
        ));
    }
    verify_committed_audit(app, journal, head)?;
    if matches!(
        journal.phase,
        TransactionPhase::CommittingRegistry | TransactionPhase::CommittedCleanupPending
    ) && journal.registry_commit.as_deref() == Some(head)
        && journal.registry_staged_index_digest.is_some()
    {
        Ok(())
    } else {
        require_clean_path(app, "state/registry/projections.json")
    }
}

fn verify_committed_audit(
    app: &App,
    journal: &TransactionJournal,
    head: &str,
) -> std::result::Result<(), CommandFailure> {
    let operation = journal
        .aggregate_operation
        .as_ref()
        .ok_or_else(|| corrupt("missing aggregate operation evidence"))?;
    let checkpoint = journal
        .aggregate_checkpoint
        .as_ref()
        .ok_or_else(|| corrupt("missing aggregate checkpoint evidence"))?;
    let original_operations = journal
        .original_operations
        .as_ref()
        .ok_or_else(|| corrupt("missing original operations evidence"))?;
    let raw = gitops::run_git(
        &app.ctx,
        &[
            "show",
            &format!("{head}:{}", super::aggregate_audit::OPERATIONS_PATH),
        ],
    )
    .map_err(map_git)?;
    let committed_operations = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<crate::state_model::RegistryOperationRecord>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| corrupt("registry commit operations are invalid"))?;
    let operations_match = committed_operations.len() == original_operations.len() + 1
        && same_value(
            &committed_operations[..original_operations.len()],
            original_operations,
        )?
        && same_value(&committed_operations[original_operations.len()], operation)?;
    if !operations_match {
        return Err(recovery_stale(
            "registry commit operations differ from transaction evidence",
        ));
    }
    let raw = gitops::run_git(
        &app.ctx,
        &[
            "show",
            &format!("{head}:{}", super::aggregate_audit::CHECKPOINT_PATH),
        ],
    )
    .map_err(map_git)?;
    let committed_checkpoint: crate::state_model::RegistryOpsCheckpoint =
        serde_json::from_str(&raw).map_err(|_| corrupt("registry commit checkpoint is invalid"))?;
    if !same_value(&committed_checkpoint, checkpoint)? {
        return Err(recovery_stale(
            "registry commit checkpoint differs from transaction evidence",
        ));
    }
    Ok(())
}

fn same_value<T: serde::Serialize + ?Sized>(
    left: &T,
    right: &T,
) -> std::result::Result<bool, CommandFailure> {
    Ok(serde_json::to_value(left).map_err(map_io)?
        == serde_json::to_value(right).map_err(map_io)?)
}

pub(super) fn committed_skill_digest(
    app: &App,
    head: &str,
    skill: &str,
) -> std::result::Result<String, CommandFailure> {
    let prefix = format!("skills/{skill}/");
    let output = gitops::run_git_allow_failure(
        &app.ctx,
        &[
            "ls-tree",
            "-rz",
            "-r",
            head,
            "--",
            prefix.trim_end_matches('/'),
        ],
    )
    .map_err(map_git)?;
    if !output.status.success() {
        return Err(map_git(anyhow::anyhow!(
            String::from_utf8_lossy(&output.stderr).to_string()
        )));
    }
    let mut entries = Vec::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| corrupt("invalid git tree record"))?;
        let header = std::str::from_utf8(&record[..tab])
            .map_err(|_| corrupt("non-UTF-8 git tree header"))?;
        let mut fields = header.split_whitespace();
        let mode = fields
            .next()
            .ok_or_else(|| corrupt("git tree record has no mode"))?;
        if fields.next() != Some("blob") {
            return Err(corrupt("skill commit tree contains a non-blob leaf"));
        }
        let oid = fields
            .next()
            .ok_or_else(|| corrupt("git tree record has no object id"))?;
        let path =
            std::str::from_utf8(&record[tab + 1..]).map_err(|_| corrupt("non-UTF-8 skill path"))?;
        let relative = path
            .strip_prefix(&prefix)
            .ok_or_else(|| corrupt("skill commit path escaped prefix"))?;
        entries.push((relative.to_string(), mode == "120000", oid.to_string()));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (relative, symlink, oid) in entries {
        let blob = gitops::run_git_allow_failure(&app.ctx, &["cat-file", "blob", &oid])
            .map_err(map_git)?;
        if !blob.status.success() {
            return Err(map_git(anyhow::anyhow!(
                String::from_utf8_lossy(&blob.stderr).to_string()
            )));
        }
        hasher.update(b"path\0");
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        if symlink {
            hasher.update(b"symlink\0");
            hasher.update(&blob.stdout);
        } else {
            hasher.update(b"file\0");
            hasher.update(&(blob.stdout.len() as u64).to_be_bytes());
            hasher.update(&blob.stdout);
        }
        hasher.update(b"\0");
    }
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

pub(super) fn require_clean_path(app: &App, path: &str) -> std::result::Result<(), CommandFailure> {
    for args in [
        vec!["diff", "--quiet", "--", path],
        vec!["diff", "--cached", "--quiet", "--", path],
    ] {
        let output = gitops::run_git_allow_failure(&app.ctx, &args).map_err(map_git)?;
        if !output.status.success() {
            return Err(recovery_stale(
                "transaction path has index or working-tree drift",
            ));
        }
    }
    Ok(())
}

pub(super) fn corrupt(message: &str) -> CommandFailure {
    CommandFailure::new(ErrorCode::StateCorrupt, message)
}
