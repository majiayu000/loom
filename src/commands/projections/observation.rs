use std::fs;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::state::AppContext;
use crate::state_model::RegistryProjectionInstance;

use super::super::provenance::{materialized_tree_digest, skill_tree_digest};

#[derive(Debug, Clone)]
pub(crate) struct ProjectionObservation {
    pub observed_at: DateTime<Utc>,
    pub health: crate::core::vocab::Health,
    pub observed_drift: bool,
    pub source_tree_digest: Option<String>,
    pub materialized_tree_digest: Option<String>,
    pub error_code: Option<&'static str>,
    pub status: &'static str,
    pub error_message: Option<String>,
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
                error_code: Option<&'static str>,
                status: &'static str,
                error_message: Option<String>| {
        ProjectionObservation {
            observed_at,
            health,
            observed_drift,
            source_tree_digest,
            materialized_tree_digest,
            error_code,
            status,
            error_message,
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
            None,
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
            None,
        );
    }

    match projection.method {
        crate::core::vocab::ProjectionMethod::Symlink => {
            observe_symlink_projection(ctx, projection, materialized, observed_at)
        }
        crate::core::vocab::ProjectionMethod::Copy => {
            observe_tree_projection(&source, materialized, false, observed_at)
        }
        crate::core::vocab::ProjectionMethod::Materialize => {
            observe_tree_projection(&source, materialized, true, observed_at)
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
    projection.last_observed_error = observation.error_code.map(str::to_string);
    projection.updated_at = Some(observation.observed_at);
}

pub(crate) fn projection_observation_check(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
) -> Option<(Value, ProjectionObservationUpdate)> {
    if projection_is_compiled_artifact_view(projection) {
        return None;
    }
    let observation = observe_projection(ctx, projection);
    let ok = observation.status == "healthy";
    let severity = if ok {
        "ok"
    } else if observation.status == "drifted" {
        "warning"
    } else {
        "error"
    };
    let (id, ok_message, fail_message) = match projection.method {
        crate::core::vocab::ProjectionMethod::Symlink => (
            format!("projection_symlink_target:{}", projection.instance_id),
            "symlink projection points at source skill",
            "symlink projection does not point at source skill",
        ),
        crate::core::vocab::ProjectionMethod::Copy
        | crate::core::vocab::ProjectionMethod::Materialize => (
            format!("projection_content_digest:{}", projection.instance_id),
            "projection content matches source",
            "projection content does not match source",
        ),
    };
    let check = json!({
        "section": "projection",
        "id": id,
        "ok": ok,
        "severity": severity,
        "message": if ok { ok_message } else { fail_message },
        "next_action": if ok {
            Value::Null
        } else {
            json!("capture or re-project the skill")
        },
        "details": projection_observation_details(ctx, projection, &observation)
    });
    Some((
        check,
        ProjectionObservationUpdate {
            instance_id: projection.instance_id.clone(),
            observation,
        },
    ))
}

fn observe_tree_projection(
    source: &Path,
    materialized: &Path,
    materialize_view: bool,
    observed_at: DateTime<Utc>,
) -> ProjectionObservation {
    let source_digest = match digest_for_projection(source, materialize_view) {
        Ok(digest) => digest,
        Err(err) => {
            return observation_failure(
                observed_at,
                crate::core::vocab::Health::Conflict,
                "source_unreadable",
                "unreadable",
                Some(err.to_string()),
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
                error_code: Some("materialized_unreadable"),
                status: "unreadable",
                error_message: Some(err.to_string()),
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
        source_tree_digest: Some(source_digest),
        materialized_tree_digest: Some(materialized_digest),
        error_code: (!matches).then_some("digest_mismatch"),
        status: if matches { "healthy" } else { "drifted" },
        error_message: None,
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
                observed_at,
                crate::core::vocab::Health::Conflict,
                "not_symlink",
                "unreadable",
                Some(err.to_string()),
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
        error_code: (!matches).then_some("symlink_target_mismatch"),
        status: if matches { "healthy" } else { "drifted" },
        error_message: None,
    }
}

fn observation_failure(
    observed_at: DateTime<Utc>,
    health: crate::core::vocab::Health,
    error: &'static str,
    status: &'static str,
    error_message: Option<String>,
) -> ProjectionObservation {
    ProjectionObservation {
        observed_at,
        health,
        observed_drift: true,
        source_tree_digest: None,
        materialized_tree_digest: None,
        error_code: Some(error),
        status,
        error_message,
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

fn projection_is_compiled_artifact_view(projection: &RegistryProjectionInstance) -> bool {
    if projection.method != crate::core::vocab::ProjectionMethod::Materialize {
        return false;
    }
    let marker = Path::new(&projection.materialized_path)
        .join(".loom")
        .join("compiled")
        .join("projection.json");
    let Ok(raw) = fs::read_to_string(marker) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return false;
    };
    value["schema_version"] == json!(1)
        && value["kind"] == json!("compiled_activation")
        && value["entrypoint"] == json!("SKILL.md")
}

fn projection_observation_details(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
    observation: &ProjectionObservation,
) -> Value {
    let mut details = json!({
        "status": observation.status,
        "error": observation.error_code,
        "instance_id": projection.instance_id,
        "skill_id": projection.skill_id,
        "target_id": projection.target_id,
        "materialized_path": projection.materialized_path,
        "method": projection.method,
    });
    if let Some(digest) = &observation.source_tree_digest {
        details["source_tree_digest"] = json!(digest);
    }
    if let Some(digest) = &observation.materialized_tree_digest {
        details["materialized_tree_digest"] = json!(digest);
    }
    if matches!(
        observation.error_code,
        Some("source_missing" | "source_unreadable")
    ) {
        details["source_path"] = json!(ctx.skill_path(&projection.skill_id).display().to_string());
    }
    if let Some(message) = &observation.error_message {
        details["error_message"] = json!(message);
    }
    details
}
