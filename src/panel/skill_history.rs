use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::StatusCode,
};
use serde_json::json;

use super::PanelState;
use super::auth::{load_registry_snapshot, registry_error, status_for_error_code};
use crate::commands::collect_skill_inventory;
use crate::state_model::{RegistryObservationEvent, RegistryStatePaths};

const MAX_HISTORY_WARNINGS: usize = 20;
const MAX_HISTORY_WARNING_DETAILS: usize = MAX_HISTORY_WARNINGS - 1;

fn skill_name_looks_sane(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Wrapper that orders `RegistryObservationEvent` by `observed_at` (ascending) so
/// that a `BinaryHeap<Reverse<OrdEvent>>` acts as a min-heap keyed by time.
/// `pop()` on such a heap removes the OLDEST event, letting us keep the newest.
struct OrdEvent(RegistryObservationEvent);

impl PartialEq for OrdEvent {
    fn eq(&self, other: &Self) -> bool {
        self.0.observed_at == other.0.observed_at
    }
}
impl Eq for OrdEvent {}
impl PartialOrd for OrdEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrdEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.observed_at.cmp(&other.0.observed_at)
    }
}

/// Stream `path` line-by-line and return at most `limit` events with the
/// newest `observed_at` timestamps. Allocates O(limit) memory regardless
/// of how many lines the file contains, and records malformed lines as
/// warnings instead of failing the whole file.
fn load_obs_bounded(
    path: &Path,
    limit: usize,
) -> Result<(Vec<RegistryObservationEvent>, Vec<String>), anyhow::Error> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), Vec::new())),
        Err(e) => return Err(anyhow::anyhow!("failed to open {}: {}", path.display(), e)),
    };
    if file
        .metadata()
        .map_err(|e| anyhow::anyhow!("failed to stat {}: {}", path.display(), e))?
        .len()
        == 0
    {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut warnings = Vec::new();
    let mut skipped_warning_count = 0usize;
    // Min-heap (via Reverse) capped at `limit`; pop discards the oldest entry.
    let mut heap: BinaryHeap<Reverse<OrdEvent>> = BinaryHeap::with_capacity(limit + 1);
    let reader = BufReader::new(file);
    for (idx, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| {
            anyhow::anyhow!(
                "failed to read line {} from {}: {}",
                idx + 1,
                path.display(),
                e
            )
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event: RegistryObservationEvent = match serde_json::from_str(trimmed) {
            Ok(event) => event,
            Err(e) => {
                if warnings.len() < MAX_HISTORY_WARNING_DETAILS {
                    warnings.push(format!(
                        "skipped malformed observation line {} from {}: {}",
                        idx + 1,
                        path.display(),
                        e
                    ));
                } else {
                    skipped_warning_count += 1;
                }
                continue;
            }
        };
        heap.push(Reverse(OrdEvent(event)));
        if heap.len() > limit {
            heap.pop();
        }
    }

    if skipped_warning_count > 0 {
        warnings.push(format!(
            "skipped {} additional malformed observation line(s) from {}",
            skipped_warning_count,
            path.display()
        ));
    }

    Ok((
        heap.into_iter().map(|Reverse(OrdEvent(e))| e).collect(),
        warnings,
    ))
}

pub(super) async fn registry_skill_history(
    AxumPath(skill_name): AxumPath<String>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !skill_name_looks_sane(&skill_name) {
        return (
            StatusCode::BAD_REQUEST,
            registry_error(
                "ARG_INVALID",
                "skill name must be a single path segment".to_string(),
            ),
        );
    }

    let snapshot = match load_registry_snapshot(&state.ctx) {
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
    let skill_in_inventory = collect_skill_inventory(&state.ctx)
        .source_skills
        .iter()
        .any(|skill| skill == &skill_name);

    if instance_ids.is_empty() && !skill_in_rules && !skill_in_inventory {
        return (
            StatusCode::NOT_FOUND,
            registry_error("SKILL_NOT_FOUND", format!("skill '{skill_name}' not found")),
        );
    }

    let paths = RegistryStatePaths::from_app_context(&state.ctx);
    let mut events = Vec::new();
    let mut warnings = Vec::new();
    let mut skipped_warning_count = 0usize;
    for instance_id in &instance_ids {
        let obs_path = paths.observations_dir.join(format!("{instance_id}.jsonl"));
        match load_obs_bounded(&obs_path, 200) {
            Ok((obs, obs_warnings)) => {
                events.extend(obs);
                for warning in obs_warnings {
                    if warnings.len() < MAX_HISTORY_WARNINGS {
                        warnings.push(warning);
                    } else {
                        skipped_warning_count += 1;
                    }
                }
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    registry_error(
                        "OBS_READ_ERROR",
                        format!("failed to read observations for {instance_id}: {e:#}"),
                    ),
                );
            }
        }
    }

    if skipped_warning_count > 0 {
        warnings.push(format!(
            "skipped {} additional warning(s) across observation files",
            skipped_warning_count
        ));
    }

    events.sort_by_key(|event| Reverse(event.observed_at));
    events.truncate(200);

    (
        StatusCode::OK,
        history_ok_payload(skill_name, events, warnings),
    )
}

fn history_ok_payload(
    skill_name: String,
    events: Vec<RegistryObservationEvent>,
    warnings: Vec<String>,
) -> Json<serde_json::Value> {
    let count = events.len();
    Json(json!({
        "ok": true,
        "data": {
            "skill": skill_name,
            "count": count,
            "events": events,
        },
        "meta": {
            "warnings": warnings,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppContext;
    use crate::state_model::{
        REGISTRY_SCHEMA_VERSION, RegistryBindingRule, RegistryObservationEvent,
        RegistryProjectionInstance, RegistryRulesFile, RegistryStatePaths,
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
            panel_origin: "http://127.0.0.1:43117".to_string(),
        }
    }

    fn setup_registry(root: &std::path::Path) -> RegistryStatePaths {
        let paths = RegistryStatePaths::from_root(root);
        paths.ensure_layout().expect("ensure_layout");
        paths
    }

    fn add_skill_rule(paths: &RegistryStatePaths, skill_id: &str) {
        let now = Utc::now();
        let rules = RegistryRulesFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            rules: vec![RegistryBindingRule {
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

    fn add_projection(paths: &RegistryStatePaths, skill_id: &str, instance_id: &str) {
        let now = Utc::now();
        let mut existing = paths
            .load_projections()
            .unwrap_or_else(|_| crate::state_model::empty_projections_file());
        existing.projections.push(RegistryProjectionInstance {
            instance_id: instance_id.to_string(),
            skill_id: skill_id.to_string(),
            binding_id: Some("binding_1".to_string()),
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

    fn add_inventory_skill(root: &std::path::Path, skill_id: &str) {
        let source_dir = root.join("source-skills");
        fs::write(
            root.join(".env"),
            format!("CLAUDE_SKILLS_DIR={}\n", source_dir.display()),
        )
        .expect("write dotenv");
        let skill_dir = source_dir.join(skill_id);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
    }

    fn append_obs(paths: &RegistryStatePaths, instance_id: &str, event: &RegistryObservationEvent) {
        let file_path = paths.observations_dir.join(format!("{instance_id}.jsonl"));
        let line = serde_json::to_string(event).unwrap() + "\n";
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .expect("open obs file");
        file.write_all(line.as_bytes()).expect("write obs");
    }

    fn obs(event_id: &str, instance_id: &str, kind: &str, ts: &str) -> RegistryObservationEvent {
        RegistryObservationEvent {
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
            registry_skill_history(AxumPath("../etc".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["error"]["code"], json!("ARG_INVALID"));

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_skill() {
        let root = std::env::temp_dir().join(format!("loom-hist-nf-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let _paths = setup_registry(&root);
        let state = make_state(&root);

        let (status, Json(payload)) =
            registry_skill_history(AxumPath("no-such-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["error"]["code"], json!("SKILL_NOT_FOUND"));

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn returns_empty_events_when_no_obs_files() {
        let root = std::env::temp_dir().join(format!("loom-hist-em-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = setup_registry(&root);
        add_skill_rule(&paths, "my-skill");
        add_projection(&paths, "my-skill", "inst-1");
        let state = make_state(&root);

        let (status, Json(payload)) =
            registry_skill_history(AxumPath("my-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["ok"], json!(true));
        assert_eq!(payload["data"]["skill"], json!("my-skill"));
        assert_eq!(payload["data"]["count"], json!(0));
        assert!(payload["data"]["events"].as_array().unwrap().is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn returns_empty_events_for_inventory_skill_without_registry_bindings() {
        let root = std::env::temp_dir().join(format!("loom-hist-inventory-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let _paths = setup_registry(&root);
        add_inventory_skill(&root, "inventory-skill");
        let state = make_state(&root);

        let (status, Json(payload)) =
            registry_skill_history(AxumPath("inventory-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["ok"], json!(true));
        assert_eq!(payload["data"]["skill"], json!("inventory-skill"));
        assert_eq!(payload["data"]["count"], json!(0));
        assert!(payload["data"]["events"].as_array().unwrap().is_empty());
        assert!(payload["meta"]["warnings"].as_array().unwrap().is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn accepts_inventory_skill_with_spaces_and_unicode() {
        let root = std::env::temp_dir().join(format!("loom-hist-unicode-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let _paths = setup_registry(&root);
        let skill_name = "多词 skill";
        add_inventory_skill(&root, skill_name);
        let state = make_state(&root);

        let (status, Json(payload)) =
            registry_skill_history(AxumPath(skill_name.to_string()), State(state)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["ok"], json!(true));
        assert_eq!(payload["data"]["skill"], json!(skill_name));
        assert_eq!(payload["data"]["count"], json!(0));
        assert!(payload["data"]["events"].as_array().unwrap().is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn skips_malformed_observation_lines_and_returns_valid_events() {
        let root = std::env::temp_dir().join(format!("loom-hist-bad-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = setup_registry(&root);
        add_skill_rule(&paths, "bad-skill");
        add_projection(&paths, "bad-skill", "inst-1");
        let file_path = paths.observations_dir.join("inst-1.jsonl");
        fs::write(&file_path, "{not valid json}\n").unwrap();
        append_obs(
            &paths,
            "inst-1",
            &obs("ev-1", "inst-1", "captured", "2024-01-01T10:00:00Z"),
        );
        let state = make_state(&root);

        let (status, Json(payload)) =
            registry_skill_history(AxumPath("bad-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["ok"], json!(true));
        assert_eq!(payload["data"]["count"], json!(1));
        let events = payload["data"]["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["event_id"], json!("ev-1"));
        let warnings = payload["meta"]["warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0]
                .as_str()
                .unwrap()
                .contains("skipped malformed observation line 1")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn caps_malformed_observation_warnings() {
        let root = std::env::temp_dir().join(format!("loom-hist-warn-cap-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = setup_registry(&root);
        add_skill_rule(&paths, "warn-cap-skill");
        add_projection(&paths, "warn-cap-skill", "inst-1");
        let file_path = paths.observations_dir.join("inst-1.jsonl");
        let mut contents = String::new();
        for _ in 0..(MAX_HISTORY_WARNINGS + 3) {
            contents.push_str("{not valid json}\n");
        }
        fs::write(&file_path, contents).unwrap();
        let state = make_state(&root);

        let (status, Json(payload)) =
            registry_skill_history(AxumPath("warn-cap-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::OK);
        let warnings = payload["meta"]["warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), MAX_HISTORY_WARNINGS);
        assert_eq!(
            warnings.last().unwrap(),
            &json!(format!(
                "skipped 4 additional malformed observation line(s) from {}",
                file_path.display()
            ))
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn still_returns_error_for_unreadable_observation_file() {
        let root = std::env::temp_dir().join(format!("loom-hist-ioerr-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = setup_registry(&root);
        add_skill_rule(&paths, "bad-skill");
        add_projection(&paths, "bad-skill", "inst-1");
        fs::create_dir_all(paths.observations_dir.join("inst-1.jsonl")).unwrap();
        let state = make_state(&root);

        let (status, Json(payload)) =
            registry_skill_history(AxumPath("bad-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["error"]["code"], json!("OBS_READ_ERROR"));

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn returns_events_sorted_descending() {
        let root = std::env::temp_dir().join(format!("loom-hist-sort-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let paths = setup_registry(&root);
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
            registry_skill_history(AxumPath("my-skill".to_string()), State(state)).await;

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
        let paths = setup_registry(&root);
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
            registry_skill_history(AxumPath("multi-skill".to_string()), State(state)).await;

        assert_eq!(status, StatusCode::OK);
        let events = payload["data"]["events"].as_array().unwrap();
        assert_eq!(events.len(), 2, "events from both instances must be merged");
        assert_eq!(payload["data"]["count"], json!(2));

        let _ = fs::remove_dir_all(&root);
    }
}
