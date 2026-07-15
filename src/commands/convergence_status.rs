use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::core::convergence_status::{
    AxisError, ConvergenceStatus, ProjectionConvergenceItem, ProjectionConvergenceState,
    ProjectionConvergenceStatus, RegistryTransportState, RegistryTransportStatus, VisibilityState,
    VisibilityStatus,
};
use crate::gitops;
use crate::state::AppContext;
use crate::state_model::{RegistryProjectionInstance, RegistrySnapshot, RegistryStatePaths};
use crate::types::SyncState;

use super::codex_visibility::{CodexVisibilityReport, build_agent_visibility_report};
use super::projections::{observe_projection, remote_status_payload};

#[derive(Clone, Copy, Default)]
pub(crate) struct ConvergenceRequest<'a> {
    pub skill: Option<&'a str>,
    pub agent: Option<&'a str>,
    pub workspace: Option<&'a Path>,
    pub profile: Option<&'a str>,
}

pub(crate) struct ConvergenceCollection {
    pub status: ConvergenceStatus,
    pub remote: Option<Value>,
    pub sync_state: Option<SyncState>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct Freshness {
    revision: Option<String>,
    checkpoint: Option<DateTime<Utc>>,
}

struct LiveEvidenceRecheck {
    registry_transport: RegistryTransportStatus,
    projections: ProjectionConvergenceStatus,
    visibility: VisibilityStatus,
}

pub(crate) fn collect_convergence_status(
    ctx: &AppContext,
    request: ConvergenceRequest<'_>,
) -> ConvergenceCollection {
    collect_convergence_status_with_recheck(ctx, request, collect_live_evidence_recheck)
}

fn collect_convergence_status_with_recheck<F>(
    ctx: &AppContext,
    request: ConvergenceRequest<'_>,
    recheck: F,
) -> ConvergenceCollection
where
    F: FnOnce(
        &AppContext,
        ConvergenceRequest<'_>,
        DateTime<Utc>,
        &Freshness,
    ) -> LiveEvidenceRecheck,
{
    let observed_at = Utc::now();
    let start = capture_freshness(ctx);
    let (registry_transport, remote, sync_state, warnings) = collect_registry_transport(
        ctx,
        observed_at,
        start.revision.as_deref(),
        start.checkpoint,
    );
    let projections = collect_projections(ctx, request, observed_at, &start);
    let visibility = collect_visibility(ctx, request, observed_at, &start);
    let mut status = ConvergenceStatus {
        registry_transport,
        projections,
        visibility,
        observed_at,
        complete: false,
        incomplete_axes: Vec::new(),
    };
    let recheck = recheck(ctx, request, Utc::now(), &start);
    let end = capture_freshness(ctx);
    if start != end {
        status.mark_stale("registry revision or operation checkpoint changed during collection");
    } else {
        apply_live_evidence_recheck(
            &mut status,
            &recheck.registry_transport,
            &recheck.projections,
            &recheck.visibility,
        );
    }
    ConvergenceCollection {
        status,
        remote,
        sync_state,
        warnings,
    }
}

fn collect_live_evidence_recheck(
    ctx: &AppContext,
    request: ConvergenceRequest<'_>,
    observed_at: DateTime<Utc>,
    freshness: &Freshness,
) -> LiveEvidenceRecheck {
    let (registry_transport, _, _, _) = collect_registry_transport(
        ctx,
        observed_at,
        freshness.revision.as_deref(),
        freshness.checkpoint,
    );
    LiveEvidenceRecheck {
        registry_transport,
        projections: collect_projections(ctx, request, observed_at, freshness),
        visibility: collect_visibility(ctx, request, observed_at, freshness),
    }
}

fn apply_live_evidence_recheck(
    status: &mut ConvergenceStatus,
    registry_recheck: &RegistryTransportStatus,
    projections_recheck: &ProjectionConvergenceStatus,
    visibility_recheck: &VisibilityStatus,
) {
    if !same_registry_evidence(&status.registry_transport, registry_recheck) {
        status
            .mark_registry_transport_stale("registry transport evidence changed during collection");
    }
    if !same_projection_evidence(&status.projections, projections_recheck) {
        status.mark_projections_stale("projection evidence changed during collection");
    }
    if !same_visibility_evidence(&status.visibility, visibility_recheck) {
        status.mark_visibility_stale("visibility evidence changed during collection");
    }
    status.refresh_completeness();
}

fn same_registry_evidence(
    first: &RegistryTransportStatus,
    second: &RegistryTransportStatus,
) -> bool {
    first.state == second.state
        && first.evidence.get("remote") == second.evidence.get("remote")
        && first.errors == second.errors
}

fn same_projection_evidence(
    first: &ProjectionConvergenceStatus,
    second: &ProjectionConvergenceStatus,
) -> bool {
    first.state == second.state
        && first.evidence.get("selected_count") == second.evidence.get("selected_count")
        && first.errors == second.errors
        && first.items.len() == second.items.len()
        && first.items.iter().zip(&second.items).all(|(left, right)| {
            left.instance_id == right.instance_id
                && left.skill_id == right.skill_id
                && left.target_id == right.target_id
                && left.method == right.method
                && left.state == right.state
                && left.source_digest == right.source_digest
                && left.materialized_digest == right.materialized_digest
                && left.errors == right.errors
        })
}

fn same_visibility_evidence(first: &VisibilityStatus, second: &VisibilityStatus) -> bool {
    first.state == second.state
        && first.agent == second.agent
        && first.evidence.get("reason") == second.evidence.get("reason")
        && first.evidence.get("report") == second.evidence.get("report")
        && first.errors == second.errors
}

pub(crate) fn registry_transport_status(
    remote: &Value,
    sync_state: &SyncState,
    observed_at: DateTime<Utc>,
    observed_revision: Option<&str>,
) -> RegistryTransportStatus {
    RegistryTransportStatus {
        state: RegistryTransportState::from(sync_state),
        evidence: json!({
            "remote": remote,
            "observed_revision": observed_revision,
            "observed_at": observed_at,
        }),
        observed_at,
        stale: false,
        errors: Vec::new(),
    }
}

fn collect_registry_transport(
    ctx: &AppContext,
    observed_at: DateTime<Utc>,
    observed_revision: Option<&str>,
    checkpoint: Option<DateTime<Utc>>,
) -> (
    RegistryTransportStatus,
    Option<Value>,
    Option<SyncState>,
    Vec<String>,
) {
    match remote_status_payload(ctx) {
        Ok((remote, meta)) => match meta.sync_state {
            Some(sync_state) => {
                let mut status =
                    registry_transport_status(&remote, &sync_state, observed_at, observed_revision);
                status.evidence["checkpoint_updated_at"] = json!(checkpoint);
                (status, Some(remote), Some(sync_state), meta.warnings)
            }
            None => (
                RegistryTransportStatus {
                    state: RegistryTransportState::Error,
                    evidence: json!({
                        "observed_revision": observed_revision,
                        "checkpoint_updated_at": checkpoint,
                    }),
                    observed_at,
                    stale: false,
                    errors: vec![AxisError::new(
                        "sync_state_missing",
                        "registry transport did not produce a sync state",
                    )],
                },
                Some(remote),
                None,
                meta.warnings,
            ),
        },
        Err(err) => (
            RegistryTransportStatus {
                state: RegistryTransportState::Error,
                evidence: json!({
                    "observed_revision": observed_revision,
                    "checkpoint_updated_at": checkpoint,
                }),
                observed_at,
                stale: false,
                errors: vec![AxisError::new(err.code.as_str(), err.message)],
            },
            None,
            None,
            Vec::new(),
        ),
    }
}

fn collect_projections(
    ctx: &AppContext,
    request: ConvergenceRequest<'_>,
    observed_at: DateTime<Utc>,
    freshness: &Freshness,
) -> ProjectionConvergenceStatus {
    let paths = RegistryStatePaths::from_app_context(ctx);
    let snapshot = match paths.maybe_load_snapshot() {
        Ok(snapshot) => snapshot,
        Err(err) => {
            return ProjectionConvergenceStatus {
                state: ProjectionConvergenceState::Error,
                items: Vec::new(),
                evidence: projection_evidence(freshness, 0),
                observed_at,
                stale: false,
                errors: vec![AxisError::new(
                    "projection_state_read_failed",
                    err.to_string(),
                )],
            };
        }
    };
    let Some(snapshot) = snapshot else {
        return ProjectionConvergenceStatus {
            state: ProjectionConvergenceState::NotApplicable,
            items: Vec::new(),
            evidence: projection_evidence(freshness, 0),
            observed_at,
            stale: false,
            errors: Vec::new(),
        };
    };
    let selected = snapshot
        .projections
        .projections
        .iter()
        .filter(|projection| projection_matches(&snapshot, projection, request))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return ProjectionConvergenceStatus {
            state: ProjectionConvergenceState::NotApplicable,
            items: Vec::new(),
            evidence: projection_evidence(freshness, 0),
            observed_at,
            stale: false,
            errors: Vec::new(),
        };
    }
    let items = selected
        .into_iter()
        .map(|projection| projection_item(ctx, projection))
        .collect::<Vec<_>>();
    let state = aggregate_projection_state(&items);
    ProjectionConvergenceStatus {
        state,
        evidence: projection_evidence(freshness, items.len()),
        items,
        observed_at,
        stale: false,
        errors: Vec::new(),
    }
}

fn projection_item(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
) -> ProjectionConvergenceItem {
    let observation = observe_projection(ctx, projection);
    let state = match observation.status {
        "healthy" => ProjectionConvergenceState::Converged,
        "drifted" => ProjectionConvergenceState::Drifted,
        "missing" => ProjectionConvergenceState::Missing,
        "conflict" => ProjectionConvergenceState::Conflict,
        "unreadable" => ProjectionConvergenceState::Error,
        _ => ProjectionConvergenceState::Unknown,
    };
    let errors = observation
        .error_code
        .map(|code| {
            vec![AxisError::new(
                code,
                observation
                    .error_message
                    .clone()
                    .unwrap_or_else(|| format!("projection observation reported {code}")),
            )]
        })
        .unwrap_or_default();
    ProjectionConvergenceItem {
        instance_id: projection.instance_id.clone(),
        skill_id: projection.skill_id.clone(),
        target_id: projection.target_id.clone(),
        method: projection.method,
        state,
        source_digest: observation.source_tree_digest,
        materialized_digest: observation.materialized_tree_digest,
        observed_at: observation.observed_at,
        errors,
    }
}

fn aggregate_projection_state(items: &[ProjectionConvergenceItem]) -> ProjectionConvergenceState {
    for candidate in [
        ProjectionConvergenceState::Error,
        ProjectionConvergenceState::Unknown,
        ProjectionConvergenceState::Conflict,
        ProjectionConvergenceState::Missing,
        ProjectionConvergenceState::Drifted,
    ] {
        if items.iter().any(|item| item.state == candidate) {
            return candidate;
        }
    }
    ProjectionConvergenceState::Converged
}

fn collect_visibility(
    ctx: &AppContext,
    request: ConvergenceRequest<'_>,
    observed_at: DateTime<Utc>,
    freshness: &Freshness,
) -> VisibilityStatus {
    let evidence_base = || {
        json!({
            "observed_revision": freshness.revision,
            "checkpoint_updated_at": freshness.checkpoint,
            "observed_at": observed_at,
        })
    };
    let (Some(skill), Some(agent)) = (request.skill, request.agent) else {
        let skill_selected = request.skill.is_some();
        let reason = if skill_selected {
            "agent_not_selected"
        } else {
            "skill_not_selected"
        };
        let mut evidence = evidence_base();
        evidence["reason"] = json!(reason);
        return VisibilityStatus {
            state: if skill_selected {
                VisibilityState::Unknown
            } else {
                VisibilityState::Unsupported
            },
            agent: request.agent.map(str::to_string),
            evidence,
            observed_at,
            stale: false,
            errors: Vec::new(),
        };
    };
    match build_agent_visibility_report(ctx, skill, agent, request.workspace, request.profile) {
        Ok(report) => visibility_from_report(report, observed_at, freshness),
        Err(err) => VisibilityStatus {
            state: VisibilityState::Error,
            agent: Some(agent.to_string()),
            evidence: evidence_base(),
            observed_at,
            stale: false,
            errors: vec![AxisError::new(err.code.as_str(), err.message)],
        },
    }
}

fn visibility_from_report(
    report: CodexVisibilityReport,
    observed_at: DateTime<Utc>,
    freshness: &Freshness,
) -> VisibilityStatus {
    let unsupported = report
        .checks
        .iter()
        .any(|check| check.id == "visibility_unsupported");
    let evidence_failures = report
        .checks
        .iter()
        .filter(|check| !check.ok && check.id == "codex_config_parse")
        .map(|check| AxisError::new(&check.id, &check.message))
        .collect::<Vec<_>>();
    let restart_required = report.restart_required
        || report
            .next_actions
            .iter()
            .any(|action| action.to_ascii_lowercase().contains("restart"));
    let state = if !evidence_failures.is_empty() {
        VisibilityState::Error
    } else if unsupported {
        VisibilityState::Unsupported
    } else if restart_required {
        VisibilityState::RestartRequired
    } else if report.visible {
        VisibilityState::Visible
    } else {
        VisibilityState::NotVisible
    };
    VisibilityStatus {
        state,
        agent: Some(report.agent.clone()),
        evidence: json!({
            "observed_revision": freshness.revision,
            "checkpoint_updated_at": freshness.checkpoint,
            "observed_at": observed_at,
            "report": report,
        }),
        observed_at,
        stale: false,
        errors: evidence_failures,
    }
}

fn projection_evidence(freshness: &Freshness, selected_count: usize) -> Value {
    json!({
        "observed_revision": freshness.revision,
        "checkpoint_updated_at": freshness.checkpoint,
        "selected_count": selected_count,
    })
}

fn capture_freshness(ctx: &AppContext) -> Freshness {
    let revision = gitops::head(ctx).ok();
    let checkpoint = RegistryStatePaths::from_app_context(ctx)
        .maybe_load_snapshot()
        .ok()
        .flatten()
        .map(|snapshot| snapshot.checkpoint.updated_at);
    Freshness {
        revision,
        checkpoint,
    }
}

fn projection_matches(
    snapshot: &RegistrySnapshot,
    projection: &RegistryProjectionInstance,
    request: ConvergenceRequest<'_>,
) -> bool {
    if request
        .skill
        .is_some_and(|skill| projection.skill_id != skill)
    {
        return false;
    }
    if let Some(agent) = request.agent {
        let projection_agent = snapshot
            .target(&projection.target_id)
            .map(|target| target.agent.as_str())
            .or_else(|| {
                projection
                    .binding_id
                    .as_deref()
                    .and_then(|id| snapshot.binding(id))
                    .map(|binding| binding.agent.as_str())
            });
        if projection_agent != Some(agent) {
            return false;
        }
    }
    let binding = projection
        .binding_id
        .as_deref()
        .and_then(|id| snapshot.binding(id));
    if let Some(profile) = request.profile
        && binding.is_none_or(|binding| binding.profile_id != profile)
    {
        return false;
    }
    if let Some(workspace) = request.workspace
        && binding.is_none_or(|binding| !binding_matches_workspace(binding, workspace))
    {
        return false;
    }
    true
}

fn binding_matches_workspace(
    binding: &crate::state_model::RegistryWorkspaceBinding,
    workspace: &Path,
) -> bool {
    let matcher = &binding.workspace_matcher;
    match matcher.kind.as_str() {
        "path_prefix" => workspace.starts_with(Path::new(&matcher.value)),
        "exact_path" => workspace == Path::new(&matcher.value),
        "name" => workspace.file_name().and_then(|name| name.to_str()) == Some(&matcher.value),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
