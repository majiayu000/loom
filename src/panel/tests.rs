use super::PanelState;
use crate::panel::handlers::registry_status;
use crate::state::AppContext;
use crate::state_model::{
    RegistryBindingsFile, RegistryOpsCheckpoint, RegistryProjectionsFile, RegistryRulesFile,
    RegistrySchemaFile, RegistryStatePaths, RegistryTargetsFile,
};
use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use std::{fs, path::Path, sync::Arc};
use uuid::Uuid;

mod assets;
mod handlers;
mod security;

fn make_test_state() -> (std::path::PathBuf, PanelState) {
    let root = std::env::temp_dir().join(format!("loom-panel-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create panel test root");
    let ctx = AppContext::new(Some(root.clone())).expect("build app context");
    let state = PanelState {
        ctx: Arc::new(ctx),
        panel_origin: "http://127.0.0.1:43117".to_string(),
    };
    (root, state)
}

fn write_registry_snapshot(root: &Path, schema_version: u32) {
    let paths = RegistryStatePaths::from_root(root);
    fs::create_dir_all(&paths.registry_dir).expect("create registry dir");
    fs::create_dir_all(&paths.ops_dir).expect("create registry ops dir");
    fs::create_dir_all(&paths.observations_dir).expect("create registry observations dir");
    let now = Utc::now();

    fs::write(
        &paths.schema_file,
        serde_json::to_vec_pretty(&RegistrySchemaFile {
            schema_version,
            created_at: now,
            writer: "loom-test".to_string(),
        })
        .expect("serialize schema"),
    )
    .expect("write schema");
    fs::write(
        &paths.targets_file,
        serde_json::to_vec_pretty(&RegistryTargetsFile {
            schema_version,
            targets: Vec::new(),
        })
        .expect("serialize targets"),
    )
    .expect("write targets");
    fs::write(
        &paths.bindings_file,
        serde_json::to_vec_pretty(&RegistryBindingsFile {
            schema_version,
            bindings: Vec::new(),
        })
        .expect("serialize bindings"),
    )
    .expect("write bindings");
    fs::write(
        &paths.rules_file,
        serde_json::to_vec_pretty(&RegistryRulesFile {
            schema_version,
            rules: Vec::new(),
        })
        .expect("serialize rules"),
    )
    .expect("write rules");
    fs::write(
        &paths.projections_file,
        serde_json::to_vec_pretty(&RegistryProjectionsFile {
            schema_version,
            projections: Vec::new(),
        })
        .expect("serialize projections"),
    )
    .expect("write projections");
    fs::write(&paths.operations_file, []).expect("write operations");
    fs::write(
        &paths.checkpoint_file,
        serde_json::to_vec_pretty(&RegistryOpsCheckpoint {
            schema_version,
            last_scanned_op_id: None,
            last_acked_op_id: None,
            updated_at: now,
        })
        .expect("serialize checkpoint"),
    )
    .expect("write checkpoint");
}

async fn run_registry_status(state: PanelState) -> (StatusCode, serde_json::Value) {
    let (status, Json(payload)) = registry_status(State(state)).await;
    (status, payload)
}

fn status_code(payload: &serde_json::Value) -> Option<&str> {
    payload
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str)
}

fn cleanup_root(root: std::path::PathBuf) {
    let _ = fs::remove_dir_all(root);
}
