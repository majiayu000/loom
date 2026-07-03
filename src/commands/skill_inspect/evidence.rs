use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::state::AppContext;
use crate::state_model::RegistrySnapshot;

use super::super::skill_eval::{SkillEvalVersion, skill_eval_version};
use super::super::skill_safety::{SkillTrustMetadata, evaluate_skill_safety_with_policy};
use super::Selector;

const DEFAULT_POLICY_PROFILE: &str = "safe-capture";

struct EvalCandidate {
    path: PathBuf,
    modified: SystemTime,
}

pub(super) fn build_quality_evidence(
    ctx: &AppContext,
    skill: &str,
    source_exists: bool,
    source_drifted_paths: &[String],
) -> Value {
    if !source_exists {
        return quality_unavailable("skill source directory is missing");
    }
    match latest_eval_candidate(&ctx.state_dir.join("registry/evals").join(skill)) {
        Ok(Some(candidate)) => quality_from_report(ctx, skill, candidate, source_drifted_paths),
        Ok(None) => quality_not_run(),
        Err(err) => quality_unavailable(err),
    }
}

pub(super) fn build_safety_evidence(
    ctx: &AppContext,
    skill: &str,
    trust: &SkillTrustMetadata,
    source_exists: bool,
    snapshot: Option<&RegistrySnapshot>,
    selector: Selector<'_>,
) -> Value {
    if !source_exists {
        return safety_unavailable(trust, "skill source directory is missing");
    }
    let profile = policy_profile_for_inspect(skill, snapshot, selector);
    match evaluate_skill_safety_with_policy(ctx, skill, "inspect", false, &profile) {
        Ok(evaluation) => {
            let report = evaluation.report;
            let trust = &report.trust;
            let policy =
                if trust.quarantined || matches!(trust.trust.as_str(), "blocked" | "quarantined") {
                    if evaluation.policy.allowed {
                        "allowed"
                    } else {
                        "blocked"
                    }
                } else if !evaluation.policy.allowed || report.decision == "blocked" {
                    "blocked"
                } else {
                    "allowed"
                };
            json!({
                "status": "available",
                "trust": trust.trust,
                "policy": policy,
                "policy_profile": evaluation.policy.policy_profile,
                "policy_allowed": evaluation.policy.allowed,
                "decision": report.decision,
                "finding_count": report.findings.len(),
                "summary": report.summary,
                "scripts_present": report.findings.iter().any(|finding| finding.id.starts_with("script_")),
                "network_requested": report.findings.iter().any(|finding| finding.id.contains("network")),
                "quarantined": trust.quarantined,
                "reason": trust.reason,
                "updated_at": trust.updated_at.map(|value| value.to_rfc3339()),
                "evidence_error": Value::Null,
            })
        }
        Err(err) => safety_unavailable(trust, err.message),
    }
}

fn quality_from_report(
    ctx: &AppContext,
    skill: &str,
    candidate: EvalCandidate,
    source_drifted_paths: &[String],
) -> Value {
    let raw = match fs::read_to_string(&candidate.path) {
        Ok(raw) => raw,
        Err(err) => {
            return quality_malformed(&candidate, format!("failed to read eval report: {err}"));
        }
    };
    let report = match serde_json::from_str::<Value>(&raw) {
        Ok(report) => report,
        Err(err) => {
            return quality_malformed(&candidate, format!("failed to parse eval report: {err}"));
        }
    };
    if report["skill"].as_str() != Some(skill) {
        return quality_malformed(
            &candidate,
            "eval report skill does not match inspected skill",
        );
    }
    let Some(summary) = report.get("summary").filter(|value| value.is_object()) else {
        return quality_malformed(&candidate, "eval report summary is missing");
    };
    let mut missing_metrics = Vec::new();
    for field in ["trigger_precision", "trigger_recall"] {
        if summary.get(field).and_then(Value::as_f64).is_none() {
            missing_metrics.push(field);
        }
    }
    let current_version = skill_eval_version(ctx, skill);
    let dirty_source_stale = !source_drifted_paths.is_empty();
    let stale = dirty_source_stale || report_version_stale(&report, &current_version);
    let no_cases = summary.get("case_count").and_then(Value::as_u64) == Some(0);
    let missing_pass_fail = summary.get("failed").and_then(Value::as_u64).is_none();
    let no_trigger_metrics =
        report["mode"].as_str() == Some("trigger_quality") && missing_metrics.len() == 2;
    let report_status = if no_cases || missing_pass_fail || no_trigger_metrics {
        "not_run"
    } else if summary["failed"].as_u64().unwrap_or(0) > 0 {
        "failed"
    } else {
        "passed"
    };
    let status = if stale { "stale" } else { report_status };
    let evidence_error = if dirty_source_stale {
        json!("eval report is stale because skill source has working tree drift")
    } else if stale {
        json!("eval report skill_version does not match current skill source")
    } else if no_cases {
        json!("eval report contains no executed cases")
    } else if missing_pass_fail {
        json!("eval report summary does not include pass/fail evidence")
    } else if no_trigger_metrics {
        json!("eval report has no trigger precision or recall evidence")
    } else {
        Value::Null
    };

    json!({
        "status": status,
        "last_eval": system_time_rfc3339(candidate.modified),
        "last_eval_status": report_status,
        "mode": report.get("mode").and_then(Value::as_str),
        "trigger_precision": summary.get("trigger_precision").and_then(Value::as_f64),
        "trigger_recall": summary.get("trigger_recall").and_then(Value::as_f64),
        "baseline_delta": summary.get("delta").and_then(Value::as_f64),
        "evidence_path": candidate.path.display().to_string(),
        "evidence_error": evidence_error,
        "missing_metrics": missing_metrics,
    })
}

fn latest_eval_candidate(dir: &Path) -> Result<Option<EvalCandidate>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("failed to read eval report directory: {err}")),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read eval report entry: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|err| format!("failed to inspect eval report metadata: {err}"))?;
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .map_err(|err| format!("failed to inspect eval report timestamp: {err}"))?;
        candidates.push(EvalCandidate { path, modified });
    }
    candidates.sort_by(|left, right| {
        left.modified
            .cmp(&right.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(candidates.pop())
}

fn quality_not_run() -> Value {
    json!({
        "status": "not_run",
        "last_eval": Value::Null,
        "last_eval_status": Value::Null,
        "mode": Value::Null,
        "trigger_precision": Value::Null,
        "trigger_recall": Value::Null,
        "baseline_delta": Value::Null,
        "evidence_path": Value::Null,
        "evidence_error": Value::Null,
        "missing_metrics": ["trigger_precision", "trigger_recall"],
    })
}

fn quality_unavailable(message: impl Into<String>) -> Value {
    json!({
        "status": "unavailable",
        "last_eval": Value::Null,
        "last_eval_status": Value::Null,
        "mode": Value::Null,
        "trigger_precision": Value::Null,
        "trigger_recall": Value::Null,
        "baseline_delta": Value::Null,
        "evidence_path": Value::Null,
        "evidence_error": message.into(),
        "missing_metrics": ["trigger_precision", "trigger_recall"],
    })
}

fn quality_malformed(candidate: &EvalCandidate, message: impl Into<String>) -> Value {
    json!({
        "status": "malformed",
        "last_eval": system_time_rfc3339(candidate.modified),
        "last_eval_status": Value::Null,
        "mode": Value::Null,
        "trigger_precision": Value::Null,
        "trigger_recall": Value::Null,
        "baseline_delta": Value::Null,
        "evidence_path": candidate.path.display().to_string(),
        "evidence_error": message.into(),
        "missing_metrics": ["trigger_precision", "trigger_recall"],
    })
}

fn safety_unavailable(trust: &SkillTrustMetadata, message: impl Into<String>) -> Value {
    json!({
        "status": "unavailable",
        "trust": trust.trust,
        "policy": "unavailable",
        "policy_profile": Value::Null,
        "policy_allowed": Value::Null,
        "decision": "unavailable",
        "finding_count": Value::Null,
        "summary": Value::Null,
        "scripts_present": Value::Null,
        "network_requested": Value::Null,
        "quarantined": trust.quarantined,
        "reason": trust.reason,
        "updated_at": trust.updated_at.map(|value| value.to_rfc3339()),
        "evidence_error": message.into(),
    })
}

fn report_version_stale(report: &Value, current: &SkillEvalVersion) -> bool {
    let Some(report_version) = report
        .get("skill_version")
        .or_else(|| report.pointer("/to/skill_version"))
    else {
        return report["mode"].as_str() == Some("version_compare");
    };
    version_field_stale(
        report_version.get("head_tree_oid").and_then(Value::as_str),
        current.head_tree_oid.as_deref(),
    ) || version_field_stale(
        report_version
            .get("last_source_commit")
            .and_then(Value::as_str),
        current.last_source_commit.as_deref(),
    )
}

fn version_field_stale(report: Option<&str>, current: Option<&str>) -> bool {
    (report.is_some() || current.is_some()) && report != current
}

fn system_time_rfc3339(time: SystemTime) -> String {
    DateTime::<Utc>::from(time).to_rfc3339()
}

fn policy_profile_for_inspect(
    skill: &str,
    snapshot: Option<&RegistrySnapshot>,
    selector: Selector<'_>,
) -> String {
    let Some(snapshot) = snapshot else {
        return DEFAULT_POLICY_PROFILE.to_string();
    };
    let mut profiles = BTreeSet::new();
    for projection in &snapshot.projections.projections {
        if projection.skill_id != skill || !projection_agent_matches(snapshot, projection, selector)
        {
            continue;
        }
        if let Some(binding) = projection
            .binding_id
            .as_deref()
            .and_then(|binding_id| snapshot.binding(binding_id))
            && super::binding_matches(Some(binding), &selector)
        {
            profiles.insert(binding.policy_profile.clone());
        }
    }
    for rule in &snapshot.rules.rules {
        if rule.skill_id != skill {
            continue;
        }
        if let Some(agent) = selector.agent
            && super::rule_agent(snapshot, rule).as_deref() != Some(agent)
        {
            continue;
        }
        let binding = snapshot.binding(&rule.binding_id);
        if super::binding_matches(binding, &selector)
            && let Some(binding) = binding
        {
            profiles.insert(binding.policy_profile.clone());
        }
    }
    if profiles.is_empty() && (selector.workspace.is_some() || selector.profile.is_some()) {
        for binding in &snapshot.bindings.bindings {
            if selector
                .agent
                .is_none_or(|agent| binding.agent.as_str() == agent)
                && super::binding_matches(Some(binding), &selector)
            {
                profiles.insert(binding.policy_profile.clone());
            }
        }
    }
    profile_by_priority(&profiles)
}

fn projection_agent_matches(
    snapshot: &RegistrySnapshot,
    projection: &crate::state_model::RegistryProjectionInstance,
    selector: Selector<'_>,
) -> bool {
    let projection_agent = snapshot
        .target(&projection.target_id)
        .map(|target| target.agent.as_str())
        .or_else(|| {
            projection
                .binding_id
                .as_deref()
                .and_then(|binding_id| snapshot.binding(binding_id))
                .map(|binding| binding.agent.as_str())
        });
    selector.agent.is_none_or(|agent| {
        projection_agent.is_none_or(|projection_agent| projection_agent == agent)
    })
}

fn profile_by_priority(profiles: &BTreeSet<String>) -> String {
    for profile in ["strict", "deny-risky", "safe-capture", "audit-only"] {
        if profiles.contains(profile) {
            return profile.to_string();
        }
    }
    profiles
        .iter()
        .next()
        .cloned()
        .unwrap_or_else(|| DEFAULT_POLICY_PROFILE.to_string())
}
