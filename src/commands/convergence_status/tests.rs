use std::fs;

use serde_json::json;
use uuid::Uuid;

use super::*;

fn collect_with_changed_axis(mutate: impl FnOnce(&mut LiveEvidenceRecheck)) -> ConvergenceStatus {
    let root = std::env::temp_dir().join(format!("loom-convergence-race-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create convergence test directory");
    let ctx = AppContext::new(Some(root.clone())).expect("create app context");
    let collection = collect_convergence_status_with_recheck(
        &ctx,
        ConvergenceRequest::default(),
        |ctx, request, observed_at, freshness| {
            let mut evidence = collect_live_evidence_recheck(ctx, request, observed_at, freshness);
            mutate(&mut evidence);
            evidence
        },
    );
    fs::remove_dir_all(root).expect("remove convergence test directory");
    collection.status
}

#[test]
fn collector_recheck_marks_only_changed_registry_transport_stale() {
    let status = collect_with_changed_axis(|evidence| {
        evidence.registry_transport.state = RegistryTransportState::PendingPush;
        evidence.registry_transport.evidence["remote"] = json!({"operation_backlog": 1});
    });

    assert!(status.registry_transport.stale);
    assert!(!status.projections.stale);
    assert!(!status.visibility.stale);
    assert_eq!(status.incomplete_axes, vec!["registry_transport"]);
}

#[test]
fn collector_recheck_marks_only_changed_projection_stale() {
    let status = collect_with_changed_axis(|evidence| {
        evidence.projections.state = ProjectionConvergenceState::Drifted;
        evidence.projections.evidence["selected_count"] = json!(1);
    });

    assert!(!status.registry_transport.stale);
    assert!(status.projections.stale);
    assert!(!status.visibility.stale);
    assert_eq!(status.incomplete_axes, vec!["projections"]);
}

#[test]
fn collector_recheck_marks_only_changed_visibility_stale() {
    let status = collect_with_changed_axis(|evidence| {
        evidence.visibility.state = VisibilityState::Visible;
        evidence.visibility.evidence["report"] =
            json!({"visible": true, "config_digest": "config-b"});
    });

    assert!(!status.registry_transport.stale);
    assert!(!status.projections.stale);
    assert!(status.visibility.stale);
    assert_eq!(status.incomplete_axes, vec!["visibility"]);
}
