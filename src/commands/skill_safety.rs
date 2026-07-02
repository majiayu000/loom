use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::{SkillQuarantineArgs, SkillScanArgs, SkillTrustArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::state::AppContext;
use crate::state_model::{
    REGISTRY_SCHEMA_VERSION, RegistryStatePaths, RegistryTrustFile, RegistryTrustRecord,
};
use crate::types::ErrorCode;

use super::helpers::{
    commit_registry_state, ensure_skill_exists, map_arg, map_git, map_lock, map_registry_state,
    validate_skill_name,
};
use super::projections::{maybe_autosync_or_queue, record_registry_operation};
use super::skill_policy::{SkillPolicyReport, evaluate_skill_policy};
use super::skill_safety_findings::{
    dedupe_findings, is_metadata_path, is_security_relevant_path, policy_findings, push_finding,
    push_text_safety_findings, push_trust_findings, scan_skill_safety_files, summarize_findings,
};
use super::telemetry::record_skill_safety_telemetry;
use super::{App, CommandFailure};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillSafetyReport {
    pub skill: String,
    pub mode: String,
    pub strict: bool,
    pub decision: String,
    pub trust: SkillTrustMetadata,
    pub summary: SafetySummary,
    pub findings: Vec<SafetyFinding>,
    pub activation_allowed: bool,
}

pub(crate) struct SkillSafetyEvaluation {
    pub report: SkillSafetyReport,
    pub policy: SkillPolicyReport,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillTrustMetadata {
    pub skill_id: String,
    pub trust: String,
    pub quarantined: bool,
    pub reason: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
    pub updated_by: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct SafetySummary {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SafetyFinding {
    pub id: String,
    pub severity: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub message: String,
    pub suggested_action: String,
}

impl App {
    pub fn cmd_skill_scan(
        &self,
        args: &SkillScanArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let mode = normalize_safety_mode(&args.mode)?;
        let report = evaluate_skill_safety(&self.ctx, &args.skill, mode, args.strict)?;
        record_skill_safety_telemetry(
            &self.ctx,
            &args.skill,
            report.findings.len() as u64,
            report.activation_allowed,
        )?;
        Ok((json!(report), Meta::default()))
    }

    pub fn cmd_skill_trust(
        &self,
        args: &SkillTrustArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let trust = normalize_trust_level(&args.level)?;
        self.write_trust_metadata(
            &args.skill,
            trust,
            trust == "quarantined",
            None,
            "skill.trust",
            request_id,
        )
    }

    pub fn cmd_skill_quarantine(
        &self,
        args: &SkillQuarantineArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        self.write_trust_metadata(
            &args.skill,
            "quarantined",
            true,
            args.reason.clone(),
            "skill.quarantine",
            request_id,
        )
    }

    pub fn cmd_skill_unquarantine(
        &self,
        skill: &str,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(skill).map_err(map_arg)?;
        ensure_skill_exists(&self.ctx, skill)?;
        let current = trust_metadata_for_skill(&self.ctx, skill)?;
        if !current.quarantined && current.trust != "quarantined" {
            return Ok((
                json!({
                    "skill": skill,
                    "trust": current,
                    "active_projection_cleanup_required": [],
                    "commit": null,
                    "noop": true
                }),
                Meta::default(),
            ));
        }
        let next_trust = if current.trust == "quarantined" {
            "local-draft"
        } else {
            current.trust.as_str()
        };
        self.write_trust_metadata(
            skill,
            next_trust,
            false,
            None,
            "skill.unquarantine",
            request_id,
        )
    }

    fn write_trust_metadata(
        &self,
        skill: &str,
        trust: &str,
        quarantined: bool,
        reason: Option<String>,
        intent: &str,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let original = paths.load_trust().map_err(map_registry_state)?;
        let mut trust_file = original.clone();
        let record = upsert_trust_record(&mut trust_file, skill, trust, quarantined, reason);
        paths.save_trust(&trust_file).map_err(map_registry_state)?;
        let active_projection_cleanup_required = active_projection_cleanup(&paths, skill);
        let op_id = match record_registry_operation(
            &paths,
            intent,
            json!({
                "skill_id": skill,
                "trust": trust,
                "quarantined": quarantined,
                "request_id": request_id
            }),
            json!({
                "trust": record,
                "active_projection_cleanup_required": active_projection_cleanup_required
            }),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                paths.save_trust(&original).map_err(map_registry_state)?;
                return Err(map_registry_state(err));
            }
        };
        let commit = commit_registry_state(
            &self.ctx,
            &format!("{intent}({skill}): update trust metadata"),
        )?;
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                intent,
                request_id,
                json!({"skill": skill, "trust": trust, "commit": commit}),
                &mut meta,
            )?;
        }
        Ok((
            json!({
                "skill": skill,
                "trust": record,
                "active_projection_cleanup_required": active_projection_cleanup_required,
                "commit": commit
            }),
            meta,
        ))
    }
}

pub(crate) fn evaluate_skill_safety(
    ctx: &AppContext,
    skill: &str,
    mode: &str,
    strict: bool,
) -> std::result::Result<SkillSafetyReport, CommandFailure> {
    let policy_profile = if strict { "deny-risky" } else { "audit-only" };
    Ok(evaluate_skill_safety_with_policy(ctx, skill, mode, strict, policy_profile)?.report)
}

pub(crate) fn evaluate_skill_safety_with_policy(
    ctx: &AppContext,
    skill: &str,
    mode: &str,
    strict: bool,
    policy_profile: &str,
) -> std::result::Result<SkillSafetyEvaluation, CommandFailure> {
    ensure_skill_exists(ctx, skill)?;
    let policy = evaluate_skill_policy(ctx, skill, policy_profile)?;
    let trust = trust_metadata_for_skill(ctx, skill)?;
    let mut findings = policy_findings(&policy);
    scan_skill_safety_files(&ctx.skill_path(skill), &mut findings)?;
    push_trust_findings(&trust, &mut findings);
    dedupe_findings(&mut findings);
    let summary = summarize_findings(&findings);
    let has_high = summary.critical + summary.high > 0;
    let strict_profile = matches!(policy_profile, "deny-risky" | "strict");
    let (decision, activation_allowed) = if trust.quarantined || trust.trust == "quarantined" {
        ("quarantined", false)
    } else if trust.trust == "blocked"
        || !policy.allowed
        || ((strict || strict_profile) && has_high)
    {
        ("blocked", false)
    } else if trust.trust == "third-party-unreviewed" && has_high {
        ("review_required", false)
    } else if has_high {
        ("review_required", true)
    } else {
        ("allowed", true)
    };
    Ok(SkillSafetyEvaluation {
        report: SkillSafetyReport {
            skill: skill.to_string(),
            mode: mode.to_string(),
            strict,
            decision: decision.to_string(),
            trust,
            summary,
            findings,
            activation_allowed,
        },
        policy,
    })
}

pub(crate) fn enforce_skill_safety(
    ctx: &AppContext,
    skill: &str,
    policy_profile: &str,
) -> std::result::Result<SkillSafetyReport, CommandFailure> {
    let evaluation =
        evaluate_skill_safety_with_policy(ctx, skill, "activate", false, policy_profile)?;
    if evaluation.report.activation_allowed {
        return Ok(evaluation.report);
    }
    let mut failure = CommandFailure::new(
        ErrorCode::PolicyBlocked,
        format!(
            "safety decision '{}' blocked skill '{}'",
            evaluation.report.decision, skill
        ),
    );
    failure.details = json!({
        "report": evaluation.policy,
        "safety": evaluation.report,
        "suggested_actions": [
            "run loom skill scan <skill> --mode activate",
            "review safety findings or update trust metadata explicitly"
        ]
    });
    Err(failure)
}

pub(crate) fn trust_metadata_for_skill(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<SkillTrustMetadata, CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    let trust = paths.load_trust().map_err(map_registry_state)?;
    Ok(trust
        .skills
        .into_iter()
        .find(|record| record.skill_id == skill)
        .map(record_to_metadata)
        .unwrap_or_else(|| unknown_trust(skill)))
}

pub(crate) fn security_diff_report(
    ctx: &AppContext,
    skill: &str,
    from: &str,
    to: &str,
) -> std::result::Result<Value, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let skill_rel = format!("skills/{skill}");
    let raw = gitops::run_git(ctx, &["diff", "--name-status", from, to, "--", &skill_rel])
        .map_err(map_git)?;
    let mut changed_paths = Vec::new();
    let mut findings = Vec::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let parts = line.split('\t').collect::<Vec<_>>();
        let Some(path) = parts.last().copied() else {
            continue;
        };
        let rel = path
            .strip_prefix(&format!("{skill_rel}/"))
            .unwrap_or(path)
            .to_string();
        if !is_security_relevant_path(&rel) {
            continue;
        }
        changed_paths.push(path.to_string());
        if is_metadata_path(&rel) {
            push_finding(
                &mut findings,
                "security_metadata_changed",
                "low",
                Some(rel.clone()),
                None,
                "security-relevant metadata changed",
                "review changed frontmatter, manifest, compatibility, and tool declarations",
            );
        }
        let spec = format!("{to}:{path}");
        match gitops::run_git_allow_failure(ctx, &["show", &spec]).map_err(map_git)? {
            output if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                let mut next_findings = Vec::new();
                push_text_safety_findings(&rel, &text, &mut next_findings);
                if let Some(previous) = show_revision_file(ctx, from, path)? {
                    let mut previous_findings = Vec::new();
                    push_text_safety_findings(&rel, &previous, &mut previous_findings);
                    let previous_keys = finding_keys(&previous_findings);
                    next_findings.retain(|finding| !previous_keys.contains(&finding_key(finding)));
                }
                findings.extend(next_findings);
            }
            _ => push_finding(
                &mut findings,
                "security_relevant_file_removed",
                "low",
                Some(rel),
                None,
                "security-relevant file was removed",
                "confirm removal does not hide required review context",
            ),
        }
    }
    dedupe_findings(&mut findings);
    Ok(json!({
        "skill": skill,
        "from": from,
        "to": to,
        "security": true,
        "changed_paths": changed_paths,
        "summary": summarize_findings(&findings),
        "findings": findings
    }))
}

fn show_revision_file(
    ctx: &AppContext,
    revision: &str,
    path: &str,
) -> std::result::Result<Option<String>, CommandFailure> {
    let spec = format!("{revision}:{path}");
    match gitops::run_git_allow_failure(ctx, &["show", &spec]).map_err(map_git)? {
        output if output.status.success() => {
            Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
        }
        _ => Ok(None),
    }
}

fn finding_keys(findings: &[SafetyFinding]) -> BTreeSet<(String, String, String, String)> {
    findings.iter().map(finding_key).collect()
}

fn finding_key(finding: &SafetyFinding) -> (String, String, String, String) {
    (
        finding.id.clone(),
        finding.severity.clone(),
        finding.message.clone(),
        finding.suggested_action.clone(),
    )
}

pub(crate) fn upsert_trust_record(
    file: &mut RegistryTrustFile,
    skill: &str,
    trust: &str,
    quarantined: bool,
    reason: Option<String>,
) -> RegistryTrustRecord {
    file.schema_version = REGISTRY_SCHEMA_VERSION;
    let record = RegistryTrustRecord {
        skill_id: skill.to_string(),
        trust: trust.to_string(),
        quarantined,
        reason,
        updated_at: Utc::now(),
        updated_by: updated_by(),
    };
    if let Some(existing) = file.skills.iter_mut().find(|item| item.skill_id == skill) {
        *existing = record.clone();
    } else {
        file.skills.push(record.clone());
    }
    file.skills
        .sort_by(|left, right| left.skill_id.cmp(&right.skill_id));
    record
}

fn active_projection_cleanup(paths: &RegistryStatePaths, skill: &str) -> Vec<Value> {
    paths
        .load_snapshot()
        .ok()
        .map(|snapshot| {
            snapshot
                .projections
                .projections
                .into_iter()
                .filter(|projection| projection.skill_id == skill)
                .map(|projection| {
                    json!({
                        "instance_id": projection.instance_id,
                        "target_id": projection.target_id,
                        "materialized_path": projection.materialized_path,
                        "cleanup": "manual_review_required"
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn record_to_metadata(record: RegistryTrustRecord) -> SkillTrustMetadata {
    SkillTrustMetadata {
        skill_id: record.skill_id,
        trust: record.trust,
        quarantined: record.quarantined,
        reason: record.reason,
        updated_at: Some(record.updated_at),
        updated_by: Some(record.updated_by),
    }
}

fn unknown_trust(skill: &str) -> SkillTrustMetadata {
    SkillTrustMetadata {
        skill_id: skill.to_string(),
        trust: "unknown".to_string(),
        quarantined: false,
        reason: None,
        updated_at: None,
        updated_by: None,
    }
}

fn updated_by() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "local-user".to_string())
}

fn normalize_safety_mode(mode: &str) -> std::result::Result<&'static str, CommandFailure> {
    match mode {
        "install" => Ok("install"),
        "activate" => Ok("activate"),
        "release" => Ok("release"),
        _ => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "mode must be one of install, activate, release",
        )),
    }
}

fn normalize_trust_level(level: &str) -> std::result::Result<&'static str, CommandFailure> {
    match level {
        "local-draft" => Ok("local-draft"),
        "reviewed" => Ok("reviewed"),
        "team-approved" => Ok("team-approved"),
        "third-party-unreviewed" => Ok("third-party-unreviewed"),
        "blocked" => Ok("blocked"),
        "quarantined" => Ok("quarantined"),
        _ => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "level must be one of local-draft, reviewed, team-approved, third-party-unreviewed, blocked, quarantined",
        )),
    }
}
