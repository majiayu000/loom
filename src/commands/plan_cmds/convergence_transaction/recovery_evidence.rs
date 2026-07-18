use std::fs;
use std::io::Cursor;
use std::path::Path;

use super::recovery_support::{recovery_stale, verify_commit};
use super::*;

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
                    && item.source_tree_digest.as_deref()
                        == Some(plan.input.selected_input_tree_digest.as_str())
                    && if effect.method == "symlink" {
                        item.materialized_tree_digest.is_none()
                    } else {
                        item.materialized_tree_digest.as_deref()
                            == Some(plan.input.selected_input_tree_digest.as_str())
                    }
            })
    })
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
        if effect.effect == "create" && !path.exists() {
            return Ok(ProjectionState::Old);
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
            let digest = skill_tree_digest(path).map_err(map_io)?;
            let old = effect.materialized_tree_digest.as_deref();
            let new = plan.input.selected_input_tree_digest.as_str();
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
    require_clean_path(app, "state/registry/projections.json")
}

fn committed_skill_digest(
    app: &App,
    head: &str,
    skill: &str,
) -> std::result::Result<String, CommandFailure> {
    let rel = format!("skills/{skill}");
    let output =
        gitops::run_git_allow_failure(&app.ctx, &["archive", "--format=tar", head, "--", &rel])
            .map_err(map_git)?;
    if !output.status.success() {
        return Err(map_git(anyhow::anyhow!(
            String::from_utf8_lossy(&output.stderr).to_string()
        )));
    }
    let root =
        std::env::temp_dir().join(format!("loom-convergence-proof-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).map_err(map_io)?;
    let result = (|| {
        tar::Archive::new(Cursor::new(output.stdout))
            .unpack(&root)
            .map_err(map_io)?;
        skill_tree_digest(&root.join(&rel)).map_err(map_io)
    })();
    let cleanup = fs::remove_dir_all(&root);
    match (result, cleanup) {
        (Ok(digest), Ok(())) => Ok(digest),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(map_io(err)),
    }
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

fn corrupt(message: &str) -> CommandFailure {
    CommandFailure::new(ErrorCode::StateCorrupt, message)
}
