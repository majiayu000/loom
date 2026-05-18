use serde::Deserialize;

mod legacy;
mod ops;
mod registry_mutations;
mod registry_read;
mod remote;
mod shared;
mod skill_ops;
mod skill_read;
mod sync;
mod v1_routes;

#[derive(Debug, Default, Deserialize)]
pub(super) struct ProjectionsQuery {
    #[serde(default)]
    pub(super) health: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct OpsQuery {
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) offset: Option<usize>,
}

// v1 routes
pub(super) use v1_routes::health;
pub(super) use v1_routes::v1_health;
pub(super) use v1_routes::v1_overview;
pub(super) use v1_routes::v1_registry_bindings;
pub(super) use v1_routes::v1_registry_ops;
pub(super) use v1_routes::v1_registry_projections;
pub(super) use v1_routes::v1_registry_targets;
pub(super) use v1_routes::v1_skills;
pub(super) use v1_routes::v1_sync_status;
pub(super) use v1_routes::v1_workspace_doctor;
pub(super) use v1_routes::v1_workspace_init;
pub(super) use v1_routes::v1_workspace_status;

// legacy read routes
pub(super) use legacy::info;
pub(super) use legacy::skills;

// registry read routes
pub(super) use registry_read::registry_binding_show;
pub(super) use registry_read::registry_bindings;
pub(super) use registry_read::registry_ops;
pub(super) use registry_read::registry_projections;
pub(super) use registry_read::registry_status;
pub(super) use registry_read::registry_target_show;
pub(super) use registry_read::registry_targets;

// registry mutation routes
pub(super) use registry_mutations::registry_binding_add;
pub(super) use registry_mutations::registry_binding_remove;
pub(super) use registry_mutations::registry_capture;
pub(super) use registry_mutations::registry_orphan_clean;
pub(super) use registry_mutations::registry_project;
pub(super) use registry_mutations::registry_target_add;
pub(super) use registry_mutations::registry_target_remove;

// skill ops routes
pub(super) use skill_ops::registry_skill_add;
pub(super) use skill_ops::registry_skill_release;
pub(super) use skill_ops::registry_skill_rollback;
pub(super) use skill_ops::registry_skill_save;
pub(super) use skill_ops::registry_skill_snapshot;

// ops routes
pub(super) use ops::ops_history_repair;
pub(super) use ops::ops_purge;
pub(super) use ops::ops_retry;
pub(super) use ops::pending;
pub(super) use ops::registry_ops_diagnose;

// sync routes
pub(super) use sync::sync_pull;
pub(super) use sync::sync_push;
pub(super) use sync::sync_replay;

// remote routes
pub(super) use remote::remote_set;
pub(super) use remote::remote_status;
