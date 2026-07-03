use std::fs;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::state::AppContext;
use crate::state_model::{RegistryProjectionInstance, RegistryProjectionsFile};

use super::super::provenance::{materialized_tree_digest, skill_tree_digest};

#[derive(Debug, Clone)]
pub(crate) struct ProjectionObservation {
    pub observed_at: DateTime<Utc>,
    pub health: crate::core::vocab::Health,
    pub observed_drift: bool,
    pub source_tree_digest: Option<String>,
    pub materialized_tree_digest: Option<String>,
    pub error_code: Option<String>,
    pub status: String,
    pub details: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectionObservationUpdate {
    pub instance_id: String,
    pub observation: ProjectionObservation,
}

pub(crate) fn observe_projection(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
) -> ProjectionObservation {
    let observed_at = Utc::now();
    let source = ctx.skill_path(&projection.skill_id);
    let materialized = Path::new(&projection.materialized_path);

    let make = |health,
                observed_drift,
                source_tree_digest: Option<String>,
                materialized_tree_digest: Option<String>,
                error_code: Option<&str>,
                status: &str,
                details: Value| {
        ProjectionObservation {
            observed_at,
            health,
            observed_drift,
            source_tree_digest,
            materialized_tree_digest,
            error_code: error_code.map(ToString::to_string),
            status: status.to_string(),
            details,
        }
    };

    if !source.exists() {
        return make(
            crate::core::vocab::Health::Missing,
            true,
            None,
            None,
            Some("source_missing"),
            "missing",
            json!({
                "status": "missing",
                "error": "source_missing",
                "instance_id": projection.instance_id,
                "skill_id": projection.skill_id,
                "source_path": source.display().to_string(),
                "materialized_path": projection.materialized_path,
                "method": projection.method,
            }),
        );
    }

    if !path_exists_or_symlink(materialized) {
        return make(
            crate::core::vocab::Health::Missing,
            true,
            None,
            None,
            Some("materialized_missing"),
            "missing",
            json!({
                "status": "missing",
                "error": "materialized_missing",
                "instance_id": projection.instance_id,
                "skill_id": projection.skill_id,
                "target_id": projection.target_id,
                "materialized_path": projection.materialized_path,
                "method": projection.method,
            }),
        );
    }

    match projection.method {
        crate::core::vocab::ProjectionMethod::Symlink => {
            observe_symlink_projection(ctx, projection, materialized, observed_at)
        }
        crate::core::vocab::ProjectionMethod::Copy => {
            observe_tree_projection(projection, &source, materialized, false, observed_at)
        }
        crate::core::vocab::ProjectionMethod::Materialize => {
            observe_tree_projection(projection, &source, materialized, true, observed_at)
        }
    }
}

pub(crate) fn apply_projection_observation(
    projection: &mut RegistryProjectionInstance,
    observation: &ProjectionObservation,
) {
    if projection.health == crate::core::vocab::Health::Orphaned {
        return;
    }
    projection.health = observation.health;
    projection.observed_drift = Some(observation.observed_drift);
    projection.source_tree_digest = observation.source_tree_digest.clone();
    projection.materialized_tree_digest = observation.materialized_tree_digest.clone();
    projection.last_observed_at = Some(observation.observed_at);
    projection.last_observed_error = observation.error_code.clone();
    projection.updated_at = Some(observation.observed_at);
}

pub(crate) fn apply_projection_observation_updates(
    projections: &mut RegistryProjectionsFile,
    updates: &[ProjectionObservationUpdate],
) {
    for update in updates {
        if let Some(projection) = projections
            .projections
            .iter_mut()
            .find(|projection| projection.instance_id == update.instance_id)
        {
            apply_projection_observation(projection, &update.observation);
        }
    }
}

pub(crate) fn projection_content_observation_check(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
) -> (Value, ProjectionObservationUpdate) {
    let observation = observe_projection(ctx, projection);
    let ok = observation.status == "healthy";
    let severity = if ok {
        "ok"
    } else if observation.status == "drifted" {
        "warning"
    } else {
        "error"
    };
    let check = json!({
        "section": "projection",
        "id": format!("projection_content_digest:{}", projection.instance_id),
        "ok": ok,
        "severity": severity,
        "message": if ok {
            "projection content matches source"
        } else {
            "projection content does not match source"
        },
        "next_action": if ok {
            Value::Null
        } else {
            json!("capture or re-project the skill")
        },
        "details": observation.details.clone()
    });
    (
        check,
        ProjectionObservationUpdate {
            instance_id: projection.instance_id.clone(),
            observation,
        },
    )
}

fn observe_tree_projection(
    projection: &RegistryProjectionInstance,
    source: &Path,
    materialized: &Path,
    materialize_view: bool,
    observed_at: DateTime<Utc>,
) -> ProjectionObservation {
    let source_digest = match digest_for_projection(source, materialize_view) {
        Ok(digest) => digest,
        Err(err) => {
            return observation_failure(
                projection,
                observed_at,
                crate::core::vocab::Health::Conflict,
                "source_unreadable",
                "unreadable",
                json!({
                    "source_path": source.display().to_string(),
                    "error_message": err.to_string(),
                }),
            );
        }
    };
    let materialized_digest = match digest_for_projection(materialized, materialize_view) {
        Ok(digest) => digest,
        Err(err) => {
            return ProjectionObservation {
                observed_at,
                health: crate::core::vocab::Health::Conflict,
                observed_drift: true,
                source_tree_digest: Some(source_digest),
                materialized_tree_digest: None,
                error_code: Some("materialized_unreadable".to_string()),
                status: "unreadable".to_string(),
                details: json!({
                    "status": "unreadable",
                    "error": "materialized_unreadable",
                    "instance_id": projection.instance_id,
                    "skill_id": projection.skill_id,
                    "target_id": projection.target_id,
                    "materialized_path": projection.materialized_path,
                    "method": projection.method,
                    "error_message": err.to_string(),
                }),
            };
        }
    };
    let matches = source_digest == materialized_digest;
    ProjectionObservation {
        observed_at,
        health: if matches {
            crate::core::vocab::Health::Healthy
        } else {
            crate::core::vocab::Health::Drifted
        },
        observed_drift: !matches,
        source_tree_digest: Some(source_digest.clone()),
        materialized_tree_digest: Some(materialized_digest.clone()),
        error_code: (!matches).then(|| "digest_mismatch".to_string()),
        status: if matches { "healthy" } else { "drifted" }.to_string(),
        details: json!({
            "status": if matches { "healthy" } else { "drifted" },
            "error": if matches { Value::Null } else { json!("digest_mismatch") },
            "instance_id": projection.instance_id,
            "skill_id": projection.skill_id,
            "target_id": projection.target_id,
            "materialized_path": projection.materialized_path,
            "method": projection.method,
            "source_tree_digest": source_digest,
            "materialized_tree_digest": materialized_digest,
        }),
    }
}

fn observe_symlink_projection(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
    materialized: &Path,
    observed_at: DateTime<Utc>,
) -> ProjectionObservation {
    let expected = ctx.skill_path(&projection.skill_id);
    let link_target = match fs::read_link(materialized) {
        Ok(target) => target,
        Err(err) => {
            return observation_failure(
                projection,
                observed_at,
                crate::core::vocab::Health::Conflict,
                "not_symlink",
                "unreadable",
                json!({"error_message": err.to_string()}),
            );
        }
    };
    let resolved = if link_target.is_absolute() {
        link_target.clone()
    } else {
        materialized
            .parent()
            .map(|parent| parent.join(&link_target))
            .unwrap_or_else(|| link_target.clone())
    };
    let matches =
        resolved.exists() && fs::canonicalize(&resolved).ok() == fs::canonicalize(&expected).ok();
    ProjectionObservation {
        observed_at,
        health: if matches {
            crate::core::vocab::Health::Healthy
        } else {
            crate::core::vocab::Health::Drifted
        },
        observed_drift: !matches,
        source_tree_digest: None,
        materialized_tree_digest: None,
        error_code: (!matches).then(|| "symlink_target_mismatch".to_string()),
        status: if matches { "healthy" } else { "drifted" }.to_string(),
        details: json!({
            "status": if matches { "healthy" } else { "drifted" },
            "error": if matches { Value::Null } else { json!("symlink_target_mismatch") },
            "instance_id": projection.instance_id,
            "skill_id": projection.skill_id,
            "target_id": projection.target_id,
            "materialized_path": projection.materialized_path,
            "method": projection.method,
            "link_target": link_target.display().to_string(),
            "resolved_target": resolved.display().to_string(),
            "expected_target": expected.display().to_string(),
        }),
    }
}

fn observation_failure(
    projection: &RegistryProjectionInstance,
    observed_at: DateTime<Utc>,
    health: crate::core::vocab::Health,
    error: &str,
    status: &str,
    extra: Value,
) -> ProjectionObservation {
    let mut details = json!({
        "status": status,
        "error": error,
        "instance_id": projection.instance_id,
        "skill_id": projection.skill_id,
        "target_id": projection.target_id,
        "materialized_path": projection.materialized_path,
        "method": projection.method,
    });
    merge_json_object(&mut details, extra);
    ProjectionObservation {
        observed_at,
        health,
        observed_drift: true,
        source_tree_digest: None,
        materialized_tree_digest: None,
        error_code: Some(error.to_string()),
        status: status.to_string(),
        details,
    }
}

fn digest_for_projection(path: &Path, materialize_view: bool) -> Result<String> {
    if materialize_view {
        materialized_tree_digest(path)
    } else {
        skill_tree_digest(path)
    }
}

fn path_exists_or_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn merge_json_object(base: &mut Value, extra: Value) {
    let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) else {
        return;
    };
    for (key, value) in extra {
        base.insert(key.clone(), value.clone());
    }
}
