use std::collections::BTreeMap;

use super::*;

pub(super) fn validate_projection_transaction(
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    source: &Path,
) -> std::result::Result<(), CommandFailure> {
    if plan.projections.len() != journal.projections.len() {
        return Err(payload_corrupt(
            "journal projection count differs from its reviewed plan",
        ));
    }
    validate_global_paths(journal, source)?;
    for (effect, backup) in plan.projections.iter().zip(&journal.projections) {
        validate_projection_artifact_layout(
            Path::new(&effect.materialized_path),
            source,
            Path::new(&backup.staging_path),
            Path::new(&backup.staging_owner),
            backup.projection.as_ref(),
            backup.prepared.as_ref(),
            backup.rollback.as_ref(),
        )?;
        let Some(projection) = backup.projection.as_ref() else {
            continue;
        };
        validate_routing(plan, effect, projection)?;
        validate_observed_payload(plan, source, backup, projection)?;
        if let Some(prepared) = backup.prepared.as_ref()
            && projection_value(&prepared.projection)? != projection_value(projection)?
        {
            return Err(payload_corrupt(
                "nested prepared projection payload differs from outer journal payload",
            ));
        }
        if let Some(expected) = expected_projection(journal, &effect.instance_id)? {
            validate_expected_payload(journal, projection, expected)?;
        }
    }
    Ok(())
}

fn validate_routing(
    plan: &SkillConvergencePlan,
    effect: &crate::core::convergence::ProjectionEffectPlan,
    projection: &crate::state_model::RegistryProjectionInstance,
) -> std::result::Result<(), CommandFailure> {
    let valid = projection.instance_id == effect.instance_id
        && projection.skill_id == plan.skill
        && projection.binding_id.as_deref() == Some(effect.binding_id.as_str())
        && projection.target_id == effect.target_id
        && projection.materialized_path == effect.materialized_path
        && projection.method.as_str() == effect.method
        && projection.last_applied_rev == plan.source.registry_head;
    valid
        .then_some(())
        .ok_or_else(|| payload_corrupt("projection payload does not match its reviewed plan"))
}

fn validate_observed_payload(
    plan: &SkillConvergencePlan,
    source: &Path,
    backup: &ProjectionBackup,
    projection: &crate::state_model::RegistryProjectionInstance,
) -> std::result::Result<(), CommandFailure> {
    if matches!(backup.state, ProjectionTransactionState::RolledBack) {
        return validate_healthy_shape(plan, projection);
    }
    let live_observation = observe_projection_from_source(projection, source);
    let valid = if let Some(prepared) = backup.prepared.as_ref() {
        let mut staged_projection = projection.clone();
        staged_projection.materialized_path = prepared.staging_path.display().to_string();
        let staged_observation = observe_projection_from_source(&staged_projection, source);
        payload_matches_observation(projection, &staged_observation)
            || payload_matches_observation(projection, &live_observation)
    } else {
        payload_matches_observation(projection, &live_observation)
    };
    if !valid {
        return Err(payload_corrupt(
            "projection payload differs from fresh read-only observation",
        ));
    }
    validate_timestamps(projection)
}

fn payload_matches_observation(
    projection: &crate::state_model::RegistryProjectionInstance,
    observation: &super::super::super::projections::ProjectionObservation,
) -> bool {
    projection.health == observation.health
        && projection.observed_drift == Some(observation.observed_drift)
        && projection.source_tree_digest == observation.source_tree_digest
        && projection.materialized_tree_digest == observation.materialized_tree_digest
        && projection.last_observed_error == observation.error_code.map(str::to_string)
}

fn validate_healthy_shape(
    plan: &SkillConvergencePlan,
    projection: &crate::state_model::RegistryProjectionInstance,
) -> std::result::Result<(), CommandFailure> {
    let valid = projection.health == crate::core::vocab::Health::Healthy
        && projection.observed_drift == Some(false)
        && projection.source_tree_digest.as_deref()
            == Some(plan.input.selected_input_tree_digest.as_str())
        && if projection.method == ProjectionMethod::Symlink {
            projection.materialized_tree_digest.is_none()
        } else {
            projection.materialized_tree_digest == projection.source_tree_digest
        }
        && projection.last_observed_error.is_none();
    if !valid {
        return Err(payload_corrupt(
            "rolled-back projection payload is not a valid healthy observation",
        ));
    }
    validate_timestamps(projection)
}

fn validate_timestamps(
    projection: &crate::state_model::RegistryProjectionInstance,
) -> std::result::Result<(), CommandFailure> {
    (projection.last_observed_at.is_some() && projection.last_observed_at == projection.updated_at)
        .then_some(())
        .ok_or_else(|| payload_corrupt("projection observation timestamps are invalid"))
}

fn expected_projection<'a>(
    journal: &'a TransactionJournal,
    instance_id: &str,
) -> std::result::Result<Option<&'a crate::state_model::RegistryProjectionInstance>, CommandFailure>
{
    let Some(expected) = journal.expected_projections.as_ref() else {
        return Ok(None);
    };
    let mut matches = expected
        .projections
        .iter()
        .filter(|projection| projection.instance_id == instance_id);
    let first = matches
        .next()
        .ok_or_else(|| payload_corrupt("expected projection payload is absent"))?;
    if matches.next().is_some() {
        return Err(payload_corrupt("expected projection payload is duplicated"));
    }
    Ok(Some(first))
}

fn validate_expected_payload(
    journal: &TransactionJournal,
    projection: &crate::state_model::RegistryProjectionInstance,
    expected: &crate::state_model::RegistryProjectionInstance,
) -> std::result::Result<(), CommandFailure> {
    if expected.last_applied_rev != journal.source_head.as_deref().unwrap_or_default() {
        return Err(payload_corrupt("expected projection revision is invalid"));
    }
    let mut outer = projection_value(projection)?;
    let mut expected = projection_value(expected)?;
    outer
        .as_object_mut()
        .and_then(|value| value.remove("last_applied_rev"));
    expected
        .as_object_mut()
        .and_then(|value| value.remove("last_applied_rev"));
    (outer == expected).then_some(()).ok_or_else(|| {
        payload_corrupt("outer projection payload differs from expected projection state")
    })
}

fn projection_value(
    projection: &crate::state_model::RegistryProjectionInstance,
) -> std::result::Result<Value, CommandFailure> {
    serde_json::to_value(projection)
        .map_err(|err| payload_corrupt(&format!("projection payload cannot be encoded: {err}")))
}

fn validate_global_paths(
    journal: &TransactionJournal,
    source: &Path,
) -> std::result::Result<(), CommandFailure> {
    let live = journal
        .projections
        .iter()
        .map(|projection| PathBuf::from(&projection.materialized_path))
        .collect::<Vec<_>>();
    let mut controlled = BTreeMap::<PathBuf, String>::new();
    for (index, projection) in journal.projections.iter().enumerate() {
        let staging = PathBuf::from(&projection.staging_path);
        for (label, path) in [
            ("staging", staging.clone()),
            ("owner", PathBuf::from(&projection.staging_owner)),
            ("finalize claim", claim(&staging, ".finalize-claim")?),
            (
                "rollback cleanup claim",
                claim(&staging, ".pending-cleanup-claim")?,
            ),
            (
                "prepared cleanup claim",
                claim(&staging, ".prepared-cleanup-claim")?,
            ),
            (
                "finalize cleanup claim",
                claim(
                    &claim(&staging, ".finalize-claim")?,
                    ".pending-cleanup-claim",
                )?,
            ),
        ] {
            if path == source || live.iter().any(|candidate| candidate == &path) {
                return Err(payload_corrupt(
                    "controlled projection path aliases source or a live projection",
                ));
            }
            let identity = format!("projection {index} {label}");
            if controlled.insert(path, identity).is_some() {
                return Err(payload_corrupt(
                    "controlled projection paths collide across projections",
                ));
            }
        }
    }
    Ok(())
}

fn claim(path: &Path, suffix: &str) -> std::result::Result<PathBuf, CommandFailure> {
    let name = path
        .file_name()
        .ok_or_else(|| payload_corrupt("projection artifact has no claim name"))?;
    Ok(path.with_file_name(format!("{}{suffix}", name.to_string_lossy())))
}

fn payload_corrupt(message: &str) -> CommandFailure {
    CommandFailure::new(ErrorCode::StateCorrupt, message)
}
