use std::fs;
use std::path::Path;

use super::recovery_support::{recovery_stale, verify_commit};
use super::*;
use crate::sha256::{Sha256, to_hex};

pub(super) fn reprove_source_boundary(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let source_head = journal
        .source_head
        .as_deref()
        .ok_or_else(|| corrupt("missing source head"))?;
    if let Some(commit) = journal.source_commit.as_deref() {
        if commit != source_head {
            return Err(corrupt("source commit and source head differ"));
        }
        verify_commit(
            app,
            commit,
            &journal.previous_head,
            &format!("skill({}): converge source", plan.skill),
            |path| {
                path == format!("skills/{}", plan.skill)
                    || path.starts_with(&format!("skills/{}/", plan.skill))
            },
        )?;
        let committed_digest = committed_skill_digest(app, commit, &plan.skill)?;
        if committed_digest != plan.input.selected_input_tree_digest {
            return Err(recovery_stale(
                "source commit tree does not match the reviewed input",
            ));
        }
    } else if source_head != journal.previous_head {
        return Err(corrupt("no-op source head differs from previous head"));
    }

    let head = gitops::head(&app.ctx).map_err(map_git)?;
    let registry_boundary = matches!(
        journal.phase,
        TransactionPhase::CommittingRegistry | TransactionPhase::CommittedCleanupPending
    );
    if head != source_head {
        if !registry_boundary {
            return Err(recovery_stale(
                "an intervening commit followed the source boundary",
            ));
        }
        verify_registry_commit(app, plan, journal, &head, source_head)?;
    }
    if journal.phase == TransactionPhase::CommittedCleanupPending {
        let result = journal
            .result
            .as_ref()
            .ok_or_else(|| corrupt("missing committed result"))?;
        let expected_registry = (head != source_head).then_some(head.as_str());
        if result["skill"].as_str() != Some(plan.skill.as_str())
            || result["source_commit"]
                != journal
                    .source_commit
                    .as_ref()
                    .map_or(serde_json::Value::Null, |value| {
                        serde_json::Value::String(value.clone())
                    })
            || result["registry_commit"].as_str() != expected_registry
            || result["projection_instances"]
                != serde_json::json!(
                    plan.projections
                        .iter()
                        .map(|effect| effect.instance_id.clone())
                        .collect::<Vec<_>>()
                )
        {
            return Err(corrupt(
                "committed result does not match transaction evidence",
            ));
        }
    }
    require_clean_path(app, &format!("skills/{}", plan.skill))?;
    let live_digest = skill_tree_digest(&app.ctx.skill_path(&plan.skill)).map_err(map_io)?;
    if live_digest != plan.input.selected_input_tree_digest {
        return Err(recovery_stale(
            "source working tree differs from the committed boundary",
        ));
    }
    Ok(())
}

pub(super) fn validate_mutated_surfaces(
    app: &App,
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let mut contiguous_new = 0usize;
    let mut saw_old = false;
    for effect in &plan.projections {
        let state = projection_state(app, plan, effect)?;
        match state {
            ProjectionState::New if !saw_old => contiguous_new += 1,
            ProjectionState::Old => saw_old = true,
            ProjectionState::New => {
                return Err(recovery_stale(
                    "projection transaction progress is not contiguous",
                ));
            }
            ProjectionState::Same => {
                if contiguous_new < journal.installed_projections {
                    contiguous_new += 1;
                } else {
                    saw_old = true;
                }
            }
        }
    }
    if contiguous_new < journal.installed_projections
        && journal.phase != TransactionPhase::RollingBack
    {
        return Err(recovery_stale(
            "an installed projection no longer has transaction bytes",
        ));
    }
    journal.installed_projections = contiguous_new;

    if !plan.registry.initialized {
        if paths.exists() {
            return Err(recovery_stale(
                "source-only transaction unexpectedly initialized registry state",
            ));
        }
        return Ok(());
    }

    let live = paths.load_projections().map_err(map_registry_state)?;
    let live_value = serde_json::to_value(&live).map_err(map_io)?;
    let old_value = serde_json::to_value(&journal.original_projections).map_err(map_io)?;
    let expected_value = journal
        .expected_projections
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(map_io)?;
    if live_value != old_value && expected_value.as_ref() != Some(&live_value) {
        return Err(recovery_stale(
            "registry projections are neither old nor transaction-new",
        ));
    }
    Ok(())
}

pub(super) fn validate_expected_projections(
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> bool {
    let Some(expected) = journal.expected_projections.as_ref() else {
        return !matches!(
            journal.phase,
            TransactionPhase::ProjectionsSwapped
                | TransactionPhase::CommittingRegistry
                | TransactionPhase::CommittedCleanupPending
        );
    };
    let planned = plan
        .projections
        .iter()
        .map(|effect| effect.instance_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let old_unplanned = journal
        .original_projections
        .projections
        .iter()
        .filter(|item| !planned.contains(item.instance_id.as_str()))
        .map(|item| serde_json::to_value(item).ok())
        .collect::<Option<Vec<_>>>();
    let new_unplanned = expected
        .projections
        .iter()
        .filter(|item| !planned.contains(item.instance_id.as_str()))
        .map(|item| serde_json::to_value(item).ok())
        .collect::<Option<Vec<_>>>();
    if old_unplanned.is_none()
        || old_unplanned != new_unplanned
        || expected.schema_version != journal.original_projections.schema_version
        || expected.projections.len()
            != old_unplanned.as_ref().map_or(0, Vec::len) + plan.projections.len()
    {
        return false;
    }
    plan.projections.iter().all(|effect| {
        expected
            .projections
            .iter()
            .filter(|item| item.instance_id == effect.instance_id)
            .count()
            == 1
            && expected.projections.iter().any(|item| {
                item.instance_id == effect.instance_id
                    && item.skill_id == plan.skill
                    && item.binding_id.as_deref() == Some(effect.binding_id.as_str())
                    && item.target_id == effect.target_id
                    && item.materialized_path == effect.materialized_path
                    && item.method.as_str() == effect.method
                    && item.last_applied_rev == journal.source_head.as_deref().unwrap_or_default()
                    && item.health.as_str() == "healthy"
                    && item.observed_drift == Some(false)
                    && item.last_observed_error.is_none()
                    && item.last_observed_at.is_some()
                    && item.last_observed_at == item.updated_at
                    && if effect.method == "symlink" {
                        item.source_tree_digest.is_none() && item.materialized_tree_digest.is_none()
                    } else {
                        item.source_tree_digest.as_deref()
                            == Some(effect.source_tree_digest.as_str())
                            && item.materialized_tree_digest.as_deref()
                                == Some(effect.source_tree_digest.as_str())
                    }
            })
    })
}

pub(super) fn validate_rollback_evidence(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let index_backup = Path::new(&journal.index_backup);
    let expected_index_digest = journal
        .index_backup_digest
        .as_deref()
        .ok_or_else(|| corrupt("transaction Git index backup digest is missing"))?;
    if file_digest(index_backup)? != expected_index_digest {
        return Err(corrupt("transaction Git index backup digest is invalid"));
    }
    validate_index_backup(app, index_backup)?;
    if let Some(backup) = journal.source_backup.as_ref() {
        validate_tree_backup(backup, &plan.source.tree_digest, None)?;
    }
    for (effect, artifact) in plan.projections.iter().zip(&journal.projections) {
        match (effect.effect.as_str(), artifact.backup.as_ref()) {
            ("create", None) => {}
            ("refresh", Some(backup)) => validate_tree_backup(
                backup,
                effect
                    .materialized_tree_digest
                    .as_deref()
                    .unwrap_or_default(),
                (effect.method == "symlink").then(|| app.ctx.skill_path(&plan.skill)),
            )?,
            _ => return Err(corrupt("projection backup does not match its effect")),
        }
    }
    Ok(())
}

pub(super) fn rollback_uncommitted_source_only(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    if head != journal.previous_head {
        return Err(recovery_stale(
            "HEAD changed during an uncommitted source transaction",
        ));
    }
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    if plan.registry.initialized {
        let live_registry = paths.load_projections().map_err(map_registry_state)?;
        if serde_json::to_value(live_registry).map_err(map_io)?
            != serde_json::to_value(&journal.original_projections).map_err(map_io)?
        {
            return Err(recovery_stale(
                "registry changed during an uncommitted source transaction",
            ));
        }
    } else if paths.exists() {
        return Err(recovery_stale(
            "registry initialized during an uncommitted source transaction",
        ));
    }
    validate_rollback_evidence(app, plan, journal)?;
    if plan.source.direction == ConvergenceInputDirection::Projection {
        let source = app.ctx.skill_path(&plan.skill);
        let live_digest = skill_tree_digest(&source).map_err(map_io)?;
        if live_digest != plan.source.tree_digest {
            if live_digest != plan.input.selected_input_tree_digest {
                return Err(recovery_stale(
                    "source is neither old nor transaction-new during recovery",
                ));
            }
            restore_source_from_evidence(app, plan, journal)?;
        }
    }
    if journal.phase == TransactionPhase::CommittingSource {
        let live = active_index_digest(app)?;
        let original = journal
            .index_backup_digest
            .as_deref()
            .ok_or_else(|| corrupt("transaction Git index backup digest is missing"))?;
        if live != original && journal.source_staged_index_digest.as_deref() != Some(live.as_str())
        {
            return Err(recovery_stale(
                "Git index is neither old nor transaction-staged during source recovery",
            ));
        }
        gitops::restore_index_from_backup(&app.ctx, Path::new(&journal.index_backup))
            .map_err(map_git)?;
    }
    Ok(())
}

pub(super) fn validate_rolling_back_state(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    if head != journal.previous_head && journal.rollback_head.as_deref() != Some(head.as_str()) {
        return Err(recovery_stale(
            "HEAD is neither old nor transaction-new while rolling back",
        ));
    }
    let source_digest = skill_tree_digest(&app.ctx.skill_path(&plan.skill)).map_err(map_io)?;
    if source_digest != plan.source.tree_digest
        && source_digest != plan.input.selected_input_tree_digest
    {
        return Err(recovery_stale(
            "source is neither old nor transaction-new while rolling back",
        ));
    }
    let live = active_index_digest(app)?;
    let original = journal
        .index_backup_digest
        .as_deref()
        .ok_or_else(|| corrupt("transaction Git index backup digest is missing"))?;
    if live != original && journal.rollback_index_digest.as_deref() != Some(live.as_str()) {
        return Err(recovery_stale(
            "Git index is neither old nor transaction-new while rolling back",
        ));
    }
    Ok(())
}

pub(super) fn active_index_digest(app: &App) -> std::result::Result<String, CommandFailure> {
    let raw = gitops::run_git(&app.ctx, &["rev-parse", "--git-path", "index"]).map_err(map_git)?;
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        app.ctx.root.join(path)
    };
    file_digest(&path)
}

pub(super) fn file_digest(path: &Path) -> std::result::Result<String, CommandFailure> {
    let bytes = fs::read(path).map_err(map_io)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

fn validate_index_backup(app: &App, path: &Path) -> std::result::Result<(), CommandFailure> {
    let bytes = fs::read(path).map_err(map_io)?;
    if bytes.len() < 12 || &bytes[..4] != b"DIRC" {
        return Err(corrupt("transaction Git index backup is invalid"));
    }
    let version = u32::from_be_bytes(bytes[4..8].try_into().expect("four-byte index version"));
    if !(2..=4).contains(&version) {
        return Err(corrupt(
            "transaction Git index backup has an unsupported version",
        ));
    }
    gitops::validate_index_file(&app.ctx, path).map_err(map_git)
}

fn validate_tree_backup(
    backup: &serde_json::Value,
    expected_digest: &str,
    expected_symlink_target: Option<PathBuf>,
) -> std::result::Result<(), CommandFailure> {
    let backup_path = backup["backup_path"]
        .as_str()
        .map(Path::new)
        .ok_or_else(|| corrupt("backup has no path"))?;
    match backup["kind"].as_str() {
        Some("dir") => {
            let digest = skill_tree_digest(backup_path).map_err(map_io)?;
            if digest != expected_digest {
                return Err(corrupt("transaction directory backup digest is invalid"));
            }
        }
        Some("symlink") => {
            let expected = expected_symlink_target
                .ok_or_else(|| corrupt("source backup cannot be a symlink"))?;
            let raw = fs::read_to_string(backup_path.join("symlink.json")).map_err(map_io)?;
            let payload: serde_json::Value =
                serde_json::from_str(&raw).map_err(|_| corrupt("symlink backup is invalid"))?;
            let target = payload["target"]
                .as_str()
                .map(Path::new)
                .ok_or_else(|| corrupt("symlink backup has no target"))?;
            let resolved = if target.is_absolute() {
                target.to_path_buf()
            } else {
                Path::new(backup["original_path"].as_str().unwrap_or_default())
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join(target)
            };
            let expected = expected.canonicalize().map_err(map_io)?;
            let actual = resolved.canonicalize().map_err(map_io)?;
            if actual != expected {
                return Err(corrupt("symlink backup target is invalid"));
            }
        }
        _ => return Err(corrupt("transaction backup kind is invalid")),
    }
    Ok(())
}

enum ProjectionState {
    Old,
    New,
    Same,
}

fn projection_state(
    app: &App,
    plan: &SkillConvergencePlan,
    effect: &crate::core::convergence::ProjectionEffectPlan,
) -> std::result::Result<ProjectionState, CommandFailure> {
    let path = Path::new(&effect.materialized_path);
    if effect.method == "symlink" {
        if projection_path_is_safe_symlink(path, &app.ctx.skill_path(&plan.skill)) {
            return Ok(if effect.effect == "create" {
                ProjectionState::New
            } else {
                ProjectionState::Same
            });
        }
        if effect.effect == "create" {
            return match fs::symlink_metadata(path) {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ProjectionState::Old),
                Err(err) => Err(map_io(err)),
                Ok(_) => Err(recovery_stale(
                    "created symlink projection path has unexpected external content",
                )),
            };
        }
        return Err(recovery_stale(
            "symlink projection has unexpected target or path kind",
        ));
    }
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && effect.effect == "create" => {
            Ok(ProjectionState::Old)
        }
        Err(err) => Err(map_io(err)),
        Ok(_) => {
            let digest = projection_view_digest(path, &effect.method)?;
            let old = effect.materialized_tree_digest.as_deref();
            let new = effect.source_tree_digest.as_str();
            if old == Some(new) && digest == new {
                Ok(ProjectionState::Same)
            } else if old == Some(digest.as_str()) {
                Ok(ProjectionState::Old)
            } else if digest == new {
                Ok(ProjectionState::New)
            } else {
                Err(recovery_stale(
                    "projection bytes are neither old nor transaction-new",
                ))
            }
        }
    }
}

fn verify_registry_commit(
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
        |path| path == "state/registry/projections.json",
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
    if serde_json::to_value(committed).map_err(map_io)?
        != serde_json::to_value(expected).map_err(map_io)?
    {
        return Err(recovery_stale(
            "registry commit tree differs from transaction evidence",
        ));
    }
    if journal.phase == TransactionPhase::CommittingRegistry
        && journal.registry_commit.as_deref() == Some(head)
        && journal.registry_staged_index_digest.is_some()
    {
        Ok(())
    } else {
        require_clean_path(app, "state/registry/projections.json")
    }
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

fn require_clean_path(app: &App, path: &str) -> std::result::Result<(), CommandFailure> {
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
