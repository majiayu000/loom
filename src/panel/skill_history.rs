use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::StatusCode,
};
use serde_json::json;

use super::PanelState;
use super::auth::{load_v3_snapshot, status_for_error_code, v3_error, v3_ok};
use super::skill_diff::is_valid_skill_name;
use crate::state_model::V3StatePaths;

pub(super) async fn v3_skill_history(
    AxumPath(skill_name): AxumPath<String>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !is_valid_skill_name(&skill_name) {
        return (
            StatusCode::BAD_REQUEST,
            v3_error(
                "ARG_INVALID",
                "skill name must contain only [a-zA-Z0-9._-]".to_string(),
            ),
        );
    }

    let snapshot = match load_v3_snapshot(&state.ctx) {
        Ok(s) => s,
        Err(err_json) => {
            let code = err_json.0["error"]["code"].as_str();
            return (status_for_error_code(code), err_json);
        }
    };

    let instance_ids: Vec<String> = snapshot
        .projections
        .projections
        .iter()
        .filter(|p| p.skill_id == skill_name)
        .map(|p| p.instance_id.clone())
        .collect();

    let skill_in_rules = snapshot
        .rules
        .rules
        .iter()
        .any(|r| r.skill_id == skill_name);

    if instance_ids.is_empty() && !skill_in_rules {
        return (
            StatusCode::NOT_FOUND,
            v3_error("SKILL_NOT_FOUND", format!("skill '{skill_name}' not found")),
        );
    }

    let paths = V3StatePaths::from_app_context(&state.ctx);
    let mut events = Vec::new();
    for instance_id in &instance_ids {
        let filename = format!("{instance_id}.jsonl");
        match paths.load_observations_file(&filename) {
            Ok(mut obs) => {
                // Cap per-file before merging so we never hold more than
                // instances×200 events in memory regardless of file size.
                obs.sort_by(|a, b| b.observed_at.cmp(&a.observed_at));
                obs.truncate(200);
                events.extend(obs);
            }
            Err(e) => {
                let is_not_found = e
                    .root_cause()
                    .downcast_ref::<std::io::Error>()
                    .map_or(false, |io| io.kind() == std::io::ErrorKind::NotFound);
                if !is_not_found {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        v3_error(
                            "OBS_READ_ERROR",
                            format!("failed to read observations for {instance_id}: {e:#}"),
                        ),
                    );
                }
            }
        }
    }

    events.sort_by(|a, b| b.observed_at.cmp(&a.observed_at));
    events.truncate(200);

    let count = events.len();
    (
        StatusCode::OK,
        v3_ok(json!({
            "skill": skill_name,
            "count": count,
            "events": events,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppContext;
    use crate::state_model::{
        V3BindingRule, V3ObservationEvent, V3ProjectionInstance, V3RulesFile,
        V3StatePaths,
    };
    use axum::http::StatusCode;
    use axum::{
        Json,
        extract::{Path as AxumPath, State},
    };
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::{fs, io::Write, sync::Arc};
    use uuid::Uuid;

    fn make_state(root: &std::path::Path) -> PanelState {
        let ctx = AppContext::new(Some(root.to_path_buf())).expect("AppContext");
        PanelState {
            ctx: Arc::new(ctx),
            dist_dir: root.join("panel/dist"),
            panel_origin: "http://127.0.0.1:43117".to_string(),
        }
    }

    fn setup_v3(root: &std::path::Path) -> V3StatePaths {
        let paths = V3StatePaths::from_root(root);
        paths.ensure_layout().expect("ensure_layout");
        paths
    }

    fn add_skill_rule(paths: &V3StatePaths, skill_id: &str) {
        let now = Utc::now();
        let rules = V3RulesFile {
            schema_version: 3,
            rules: vec![V3BindingRule {
                binding_id: "binding_1".to_string(),
                skill_id: skill_id.to_string(),
                target_id: "target_1".to_string(),
                method: "symlink".to_string(),
                watch_policy: "observe_only".to_string(),
                created_at: Some(now),
            }],
        };
        paths.save_rules(&rules).expect("save_rules");
    }

    fn add_projection(paths: &V3StatePaths, skill_id: &str, instance_id: &str) {
        let now = Utc::now();
        let mut existing = paths
            .load_projections()
            .unwrap_or_else(|_| crate::state_model::empty_projections_file());
        existing.projections.push(V3ProjectionInstance {
            instance_id: instance_id.to_string(),
            skill_id: skill_id.to_string(),
            binding_id: "binding_1".to_string(),
            target_id: "target_1".to_string(),
            materialized_path: format!("/tmp/skills/{skill_id}"),
            method: "symlink".to_string(),
            last_applied_rev: "abc123".to_string(),
            health: "healthy".to_string(),
            observed_drift: Some(false),
            updated_at: Some(now),
        });
        paths.save_projections(&existing).expect("save_projections");
    }

    fn append_obs(paths: &V3StatePaths, instance_id: &str, event: &V3ObservationEvent) {
        let file_path = paths.observations_dir.join(format!("{instance_id}.jsonl"));
        let line = serde_json::to_string(event).unwrap() + "\n";
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .expect("open obs file");
        file.write_all(line.as_bytes()).expect("write obs");
    }

    fn obs(event_id: &str, instance_id: &str, kind: &str, ts: &str) -> V3ObservationEvent {
        V3ObservationEvent {
            event_id: event_id.to_string(),
            instance_id: instance_id.to_string(),
            kind: kind.to_string(),
            path: None,
            from: None,
            to: None,
            observed_at: ts.parse::<DateTime<Utc>>().unwrap(),
        }
    }

    #[tokio::test]
    async fn rejects_invalid_skill_name() {
        let root = std::env::temp_dir().join(format!("loom-hist-inv-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let state = make_state(&root);

        let (status, Json(payload)) =
            v3_skill_history(AxumPath("../etc".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["error"]["code"], json!("ARG_INVALID"));

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_skill() {
        let root = std::env::temp_dir().join(format!("loom-hist-nf-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let _paths = setup_v3(&root);
        let state = make_state(&root);

        let (status, Json(payload)) =
            v3_skill_history(AxumPath("no-such-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["error"]["code"], json!("SKILL_NOT_FOUND"));

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn returns_empty_events_when_no_obs_files() {
        let root = std::env::temp_dir().join(format!("loom-hist-em-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = setup_v3(&root);
        add_skill_rule(&paths, "my-skill");
        add_projection(&paths, "my-skill", "inst-1");
        let state = make_state(&root);

        let (status, Json(payload)) =
            v3_skill_history(AxumPath("my-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["ok"], json!(true));
        assert_eq!(payload["data"]["skill"], json!("my-skill"));
        assert_eq!(payload["data"]["count"], json!(0));
        assert!(payload["data"]["events"].as_array().unwrap().is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn returns_events_sorted_descending() {
        let root = std::env::temp_dir().join(format!("loom-hist-sort-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = setup_v3(&root);
        add_skill_rule(&paths, "my-skill");
        add_projection(&paths, "my-skill", "inst-1");

        append_obs(
            &paths,
            "inst-1",
            &obs("ev-1", "inst-1", "captured", "2024-01-01T10:00:00Z"),
        );
        append_obs(
            &paths,
            "inst-1",
            &obs("ev-2", "inst-1", "projected", "2024-01-02T10:00:00Z"),
        );

        let state = make_state(&root);
        let (status, Json(payload)) =
            v3_skill_history(AxumPath("my-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::OK);
        let events = payload["data"]["events"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event_id"], json!("ev-2"), "newer event first");
        assert_eq!(events[1]["event_id"], json!("ev-1"));

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn aggregates_events_from_multiple_instances() {
        let root = std::env::temp_dir().join(format!("loom-hist-agg-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = setup_v3(&root);
        add_skill_rule(&paths, "multi-skill");
        add_projection(&paths, "multi-skill", "inst-a");
        add_projection(&paths, "multi-skill", "inst-b");

        append_obs(
            &paths,
            "inst-a",
            &obs("ev-a", "inst-a", "captured", "2024-01-01T10:00:00Z"),
        );
        append_obs(
            &paths,
            "inst-b",
            &obs("ev-b", "inst-b", "projected", "2024-01-03T10:00:00Z"),
        );

        let state = make_state(&root);
        let (status, Json(payload)) =
            v3_skill_history(AxumPath("multi-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::OK);
        let events = payload["data"]["events"].as_array().unwrap();
        assert_eq!(events.len(), 2, "events from both instances must be merged");
        assert_eq!(payload["data"]["count"], json!(2));

        let _ = fs::remove_dir_all(&root);
    }
}
