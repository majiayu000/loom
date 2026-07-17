use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tar::Archive;
use uuid::Uuid;

use crate::cli::{ReleaseArgs, SkillEvalOfflineArgs, SkillImproveArgs, SkillRegressionArgs};
use crate::core::convergence::{ConvergenceInputDirection, ConvergencePreflightEvidence};
use crate::envelope::Meta;
use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::file_ops::copy_dir_recursive_without_symlinks;
use super::helpers::{ensure_skill_exists, map_arg, map_git, map_io, validate_skill_name};
use super::skill_deps::skill_dependency_report;
use super::skill_eval::build_skill_eval_offline_report;
use super::skill_lint::{SkillLintMode, lint_skill_source, lint_skill_source_for_agent};
use super::skill_policy::{SkillPolicyReport, evaluate_skill_policy};
use super::skill_safety::evaluate_skill_safety;
use super::{App, CommandFailure};

mod evidence;

use evidence::{
    baseline_eval_evidence_report, drift_report, ensure_selected_skill_clean, ensure_skill_in_ref,
    reject_release_candidate_baseline, security_diff_status,
};

const PREFLIGHT_SCHEMA_VERSION: u32 = 1;
const SKILL_MD_LINE_LIMIT: usize = 800;

pub(crate) struct SkillPreflightReport {
    value: Value,
    mutation_allowed: bool,
    has_regressions: bool,
}

struct PreflightRequest<'a> {
    skill: &'a str,
    agent: Option<&'a str>,
    workspace: Option<&'a Path>,
    baseline: &'a str,
    target: &'a str,
    real_eval: bool,
    mode: &'a str,
    candidate_path: Option<&'a Path>,
}

impl SkillPreflightReport {
    fn into_value(self) -> Value {
        self.value
    }
}

struct MaterializedTarget {
    ctx: AppContext,
    root: Option<PathBuf>,
}

impl Drop for MaterializedTarget {
    fn drop(&mut self) {
        if let Some(root) = self.root.take()
            && let Err(err) = fs::remove_dir_all(&root)
        {
            eprintln!(
                "failed to clean temporary preflight context '{}': {err}",
                root.display()
            );
        }
    }
}

impl MaterializedTarget {
    fn cleanup(mut self) -> std::result::Result<(), CommandFailure> {
        let Some(root) = self.root.take() else {
            return Ok(());
        };
        fs::remove_dir_all(root).map_err(map_io)
    }
}

impl App {
    pub fn cmd_skill_improve(
        &self,
        args: &SkillImproveArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let report = self.skill_preflight_report(PreflightRequest {
            skill: &args.skill,
            agent: args.agent.as_deref(),
            workspace: args.workspace.as_deref(),
            baseline: &args.baseline,
            target: "working-tree",
            real_eval: args.real_eval,
            mode: "improve",
            candidate_path: None,
        })?;
        Ok((report.into_value(), Meta::default()))
    }

    pub fn cmd_skill_regression(
        &self,
        args: &SkillRegressionArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let report = self.skill_preflight_report(PreflightRequest {
            skill: &args.skill,
            agent: args.agent.as_deref(),
            workspace: None,
            baseline: &args.from_ref,
            target: &args.to_ref,
            real_eval: false,
            mode: "regression",
            candidate_path: None,
        })?;
        if report.has_regressions {
            return Err(preflight_blocked(
                "skill regression detected blocking changes",
                report,
            ));
        }
        Ok((report.into_value(), Meta::default()))
    }

    pub(crate) fn ensure_save_preflight(
        &self,
        skill: &str,
    ) -> std::result::Result<SkillPreflightReport, CommandFailure> {
        let report = self.skill_preflight_report(PreflightRequest {
            skill,
            agent: None,
            workspace: None,
            baseline: "HEAD",
            target: "working-tree",
            real_eval: false,
            mode: "save_preflight",
            candidate_path: None,
        })?;
        if !report.mutation_allowed {
            return Err(preflight_blocked("skill commit preflight failed", report));
        }
        Ok(report)
    }

    pub(crate) fn ensure_release_preflight(
        &self,
        args: &ReleaseArgs,
    ) -> std::result::Result<SkillPreflightReport, CommandFailure> {
        let Some(baseline) = args.baseline.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "skill release --preflight requires --baseline <ref>",
            ));
        };
        if matches!(baseline, "HEAD" | "working-tree") {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "release preflight baseline must not be the release candidate ref",
            ));
        }
        reject_release_candidate_baseline(&self.ctx, baseline)?;
        ensure_selected_skill_clean(&self.ctx, &args.skill)?;
        let report = self.skill_preflight_report(PreflightRequest {
            skill: &args.skill,
            agent: None,
            workspace: None,
            baseline,
            target: "HEAD",
            real_eval: false,
            mode: "release_preflight",
            candidate_path: None,
        })?;
        if !report.mutation_allowed || report.has_regressions {
            return Err(preflight_blocked("skill release preflight failed", report));
        }
        Ok(report)
    }

    pub(crate) fn convergence_preflight_evidence(
        &self,
        skill: &str,
        direction: ConvergenceInputDirection,
        input_tree_digest: &str,
        candidate_path: Option<&Path>,
    ) -> std::result::Result<ConvergencePreflightEvidence, CommandFailure> {
        let report = self.skill_preflight_report(PreflightRequest {
            skill,
            agent: None,
            workspace: None,
            baseline: "HEAD",
            target: "working-tree",
            real_eval: false,
            mode: "convergence_plan",
            candidate_path,
        })?;
        let checks =
            serde_json::from_value::<BTreeMap<String, String>>(report.value["checks"].clone())
                .map_err(map_io)?;
        let mut regression_ids = report.value["regressions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|regression| regression["id"].as_str())
            .map(str::to_string)
            .collect::<Vec<_>>();
        regression_ids.sort();
        regression_ids.dedup();
        Ok(ConvergencePreflightEvidence {
            input_direction: direction,
            input_tree_digest: input_tree_digest.to_string(),
            checks,
            regression_ids,
            mutation_allowed: report.mutation_allowed,
        })
    }

    fn skill_preflight_report(
        &self,
        request: PreflightRequest<'_>,
    ) -> std::result::Result<SkillPreflightReport, CommandFailure> {
        if request.target == "working-tree" {
            ensure_skill_exists(&self.ctx, request.skill)?;
        } else {
            ensure_skill_in_ref(&self.ctx, request.skill, request.baseline)?;
            ensure_skill_in_ref(&self.ctx, request.skill, request.target)?;
        }
        gitops::resolve_ref(&self.ctx, request.baseline).map_err(map_git)?;
        if request.target != "working-tree" {
            gitops::resolve_ref(&self.ctx, request.target).map_err(map_git)?;
        }

        let skill_rel = format!("skills/{}", request.skill);
        let pathspec = Path::new(&skill_rel);
        let materialized = if let Some(candidate_path) = request.candidate_path {
            Some(materialize_candidate_context(
                &self.ctx,
                request.skill,
                candidate_path,
            )?)
        } else {
            materialize_target_context(&self.ctx, request.skill, request.target)?
        };
        let check_ctx = materialized
            .as_ref()
            .map(|target| &target.ctx)
            .unwrap_or(&self.ctx);
        let skill_path = check_ctx.skill_path(request.skill);
        let mut checks = BTreeMap::new();
        let mut regressions = Vec::new();
        let mut details = json!({});

        let drift = if request.candidate_path.is_some() {
            json!({
                "changed": true,
                "baseline": request.baseline,
                "target": "projection-input",
            })
        } else {
            drift_report(&self.ctx, request.baseline, request.target, pathspec)?
        };
        checks.insert(
            "source_drift".to_string(),
            if drift["changed"].as_bool().unwrap_or(false) {
                "warning"
            } else {
                "pass"
            }
            .to_string(),
        );
        details["drift"] = drift;

        let lint_report = if let Some(agent) = request.agent {
            lint_skill_source_for_agent(
                &check_ctx.root,
                &skill_path,
                request.skill,
                SkillLintMode::Strict,
                agent,
            )
        } else {
            lint_skill_source(&skill_path, request.skill, SkillLintMode::Strict)
        };
        let lint_status = lint_status(&lint_report);
        checks.insert("lint".to_string(), lint_status.to_string());
        details["lint"] = json!({
            "summary": lint_report.summary,
            "sections": lint_report.sections,
            "findings": lint_report.findings
        });
        if lint_status == "fail" {
            regressions.push(regression(
                "lint_regression",
                "portable or agent lint failed",
                "run loom skill lint and fix schema or quality errors",
                json!({"summary": details["lint"]["summary"]}),
            ));
        }

        let size = skill_size_status(&skill_path);
        checks.insert("skill_size".to_string(), size.0.to_string());
        details["skill_size"] = size.1;
        if size.0 == "fail" {
            regressions.push(regression(
                "skill_size_regression",
                "SKILL.md exceeds the configured size threshold",
                "move detailed content into references before saving or releasing",
                details["skill_size"].clone(),
            ));
        }

        let safety = evaluate_skill_safety(check_ctx, request.skill, "improve", false)?;
        let safety_status = if safety.summary.critical + safety.summary.high > 0 {
            "fail"
        } else if safety.summary.medium + safety.summary.low > 0 || safety.decision != "allowed" {
            "warning"
        } else {
            "pass"
        };
        checks.insert("safety".to_string(), safety_status.to_string());
        details["safety"] = json!({
            "decision": safety.decision,
            "summary": safety.summary,
            "findings": safety.findings
        });
        if safety_status == "fail" {
            regressions.push(regression(
                "safety_regression",
                "high or critical safety findings are present",
                "run loom skill scan and remove or explicitly review risky content",
                details["safety"].clone(),
            ));
        }

        let deps =
            skill_dependency_report(check_ctx, request.skill, request.agent, request.workspace)?;
        let deps_status = match deps.status.as_str() {
            "ready" => "pass",
            "unknown" => "unknown",
            _ => "fail",
        };
        checks.insert("dependencies".to_string(), deps_status.to_string());
        details["dependencies"] = json!(deps);
        if deps_status != "pass" {
            regressions.push(regression(
                "dependency_regression",
                "dependency readiness is not ready",
                "run loom skill deps and resolve missing tools, env, MCP, or network policy",
                details["dependencies"].clone(),
            ));
        }

        let (eval_status, eval_details) =
            offline_eval_status_for_request(&self.ctx, check_ctx, &request)?;
        checks.insert("offline_eval".to_string(), eval_status.to_string());
        details["offline_eval"] = eval_details;
        if eval_status == "fail" {
            regressions.push(regression(
                "eval_regression",
                "offline eval fixtures failed",
                "run loom skill eval and fix failing cases",
                details["offline_eval"].clone(),
            ));
        }

        let security_diff = if request.target != "working-tree" {
            details["target_materialization"] =
                json!({"status": "materialized", "target": request.target});
            super::skill_safety::security_diff_report(
                &self.ctx,
                request.skill,
                request.baseline,
                request.target,
            )?
        } else {
            json!({"status": "skipped", "reason": "target is working-tree"})
        };
        let security_diff_status = security_diff_status(&security_diff);
        checks.insert(
            "security_diff".to_string(),
            security_diff_status.to_string(),
        );
        details["security_diff"] = security_diff;
        if security_diff_status == "fail" {
            regressions.push(regression(
                "security_diff_regression",
                "security diff contains high or critical findings",
                "review security diff findings before release or regression acceptance",
                details["security_diff"].clone(),
            ));
        }

        let real_eval_status = if request.real_eval {
            "unknown"
        } else {
            "skipped"
        };
        checks.insert("real_eval".to_string(), real_eval_status.to_string());
        details["real_eval"] = if request.real_eval {
            json!({"reason": "real-agent eval is not executed by read-only preflight v1; run loom skill eval compare explicitly"})
        } else {
            json!({"reason": "not requested"})
        };

        let mutation_allowed = checks
            .iter()
            .filter(|(name, _)| name.as_str() != "source_drift")
            .all(|(_, status)| matches!(status.as_str(), "pass" | "warning" | "skipped"));
        let recommendation = recommendation(request.skill, request.mode, &checks, &details);

        let has_regressions = !regressions.is_empty();
        let report = SkillPreflightReport {
            value: json!({
                "schema_version": PREFLIGHT_SCHEMA_VERSION,
                "skill": request.skill,
                "mode": request.mode,
                "baseline": request.baseline,
                "target": request.target,
                "checks": checks,
                "regressions": regressions,
                "recommendation": recommendation,
                "mutation_allowed": mutation_allowed,
                "details": details,
            }),
            mutation_allowed,
            has_regressions,
        };
        if let Some(materialized) = materialized {
            materialized.cleanup()?;
        }
        Ok(report)
    }
}

fn offline_eval_status_for_request(
    source_ctx: &AppContext,
    check_ctx: &AppContext,
    request: &PreflightRequest<'_>,
) -> std::result::Result<(&'static str, Value), CommandFailure> {
    let (target_status, target_details) =
        offline_eval_status(check_ctx, request.skill, request.agent)?;
    if request.mode != "regression" {
        return Ok((target_status, target_details));
    }

    let evidence =
        baseline_eval_evidence_report(source_ctx, request.skill, request.baseline, request.target)?;
    let evidence_status = evidence["status"].as_str().unwrap_or("pass");
    let status = if target_status == "fail" || evidence_status == "fail" {
        "fail"
    } else if target_status == "warning" || evidence_status == "warning" {
        "warning"
    } else {
        "pass"
    };
    Ok((
        status,
        json!({
            "target": target_details,
            "baseline_evidence": evidence,
        }),
    ))
}

fn offline_eval_status(
    ctx: &AppContext,
    skill: &str,
    agent: Option<&str>,
) -> std::result::Result<(&'static str, Value), CommandFailure> {
    let args = SkillEvalOfflineArgs {
        skill: skill.to_string(),
        agent: agent.map(str::to_string),
        matrix: Vec::new(),
        model: None,
    };
    match build_skill_eval_offline_report(ctx, &args) {
        Ok(result) if result.failed == 0 => {
            let status = if result.warnings.is_empty() {
                "pass"
            } else {
                "warning"
            };
            Ok((
                status,
                json!({"report": result.report, "warnings": result.warnings}),
            ))
        }
        Ok(result) => Ok((
            "fail",
            json!({
                "code": ErrorCode::EvalFailed.as_str(),
                "message": format!("skill eval failed with {} failing case(s)", result.failed),
                "details": {
                    "failed": result.failed,
                    "report": result.report,
                }
            }),
        )),
        Err(failure) => Ok((
            "fail",
            json!({
                "code": failure.code.as_str(),
                "message": failure.message,
                "details": failure.details
            }),
        )),
    }
}

fn materialize_target_context(
    ctx: &AppContext,
    skill: &str,
    target: &str,
) -> std::result::Result<Option<MaterializedTarget>, CommandFailure> {
    if target == "working-tree" {
        return Ok(None);
    }
    let root = std::env::temp_dir().join(format!("loom-preflight-ref-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).map_err(map_io)?;
    if let Err(failure) = materialize_target_paths(ctx, skill, target, &root) {
        drop(fs::remove_dir_all(&root));
        return Err(failure);
    }
    let target_ctx = AppContext::new(Some(root.clone())).map_err(map_io)?;
    Ok(Some(MaterializedTarget {
        ctx: target_ctx,
        root: Some(root),
    }))
}

fn materialize_candidate_context(
    ctx: &AppContext,
    skill: &str,
    candidate_path: &Path,
) -> std::result::Result<MaterializedTarget, CommandFailure> {
    let root = std::env::temp_dir().join(format!(
        "loom-preflight-candidate-{}",
        Uuid::new_v4().simple()
    ));
    fs::create_dir_all(root.join("skills")).map_err(map_io)?;
    let prepared = (|| {
        copy_dir_recursive_without_symlinks(candidate_path, &root.join("skills").join(skill))
            .map_err(map_io)?;
        for rel in [
            "state/registry/trust.json",
            "state/registry/sources.json",
            "loom.lock",
        ] {
            let source = ctx.root.join(rel);
            if !source.is_file() {
                continue;
            }
            let target = root.join(rel);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(map_io)?;
            }
            fs::copy(&source, &target).map_err(map_io)?;
        }
        AppContext::new(Some(root.clone())).map_err(map_io)
    })();
    let candidate_ctx = match prepared {
        Ok(candidate_ctx) => candidate_ctx,
        Err(failure) => {
            if let Err(cleanup) = fs::remove_dir_all(&root) {
                return Err(failure.with_rollback_errors(vec![json!({
                    "step": "cleanup_preflight_candidate",
                    "path": root.display().to_string(),
                    "error": cleanup.to_string(),
                })]));
            }
            return Err(failure);
        }
    };
    Ok(MaterializedTarget {
        ctx: candidate_ctx,
        root: Some(root),
    })
}

pub(crate) struct PreparedConvergenceInput {
    policy: SkillPolicyReport,
    materialized: Option<MaterializedTarget>,
}

impl PreparedConvergenceInput {
    pub(crate) fn policy(&self) -> &SkillPolicyReport {
        &self.policy
    }

    pub(crate) fn candidate_path(&self, skill: &str) -> Option<PathBuf> {
        self.materialized
            .as_ref()
            .map(|target| target.ctx.skill_path(skill))
    }
}

pub(crate) fn prepare_convergence_skill_input(
    ctx: &AppContext,
    skill: &str,
    candidate_path: Option<&Path>,
) -> std::result::Result<PreparedConvergenceInput, CommandFailure> {
    let materialized = candidate_path
        .map(|path| materialize_candidate_context(ctx, skill, path))
        .transpose()?;
    let policy_ctx = materialized
        .as_ref()
        .map(|target| &target.ctx)
        .unwrap_or(ctx);
    let policy = evaluate_skill_policy(policy_ctx, skill, "safe-capture")?;
    Ok(PreparedConvergenceInput {
        policy,
        materialized,
    })
}

fn materialize_target_paths(
    ctx: &AppContext,
    skill: &str,
    target: &str,
    root: &Path,
) -> std::result::Result<(), CommandFailure> {
    let skill_rel = format!("skills/{skill}");
    let output =
        gitops::run_git_allow_failure(ctx, &["archive", "--format=tar", target, "--", &skill_rel])
            .map_err(map_git)?;
    if !output.status.success() {
        return Err(map_git(anyhow::anyhow!(
            "git archive failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Archive::new(&output.stdout[..])
        .unpack(root)
        .map_err(map_io)?;
    for rel in [
        "state/registry/trust.json",
        "state/registry/sources.json",
        "loom.lock",
    ] {
        materialize_optional_file(ctx, target, rel, root)?;
    }
    Ok(())
}

fn materialize_optional_file(
    ctx: &AppContext,
    target: &str,
    rel: &str,
    root: &Path,
) -> std::result::Result<(), CommandFailure> {
    let spec = format!("{target}:{rel}");
    let output = gitops::run_git_allow_failure(ctx, &["show", &spec]).map_err(map_git)?;
    if !output.status.success() {
        return Ok(());
    }
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    fs::write(path, &output.stdout).map_err(map_io)?;
    Ok(())
}

fn lint_status(report: &super::SkillLintReport) -> &'static str {
    if report.summary.error_count > 0 {
        "fail"
    } else if report.summary.warning_count > 0 {
        "warning"
    } else {
        "pass"
    }
}

fn skill_size_status(skill_path: &Path) -> (&'static str, Value) {
    let entrypoint = skill_path.join("SKILL.md");
    let lines = std::fs::read_to_string(&entrypoint)
        .map(|raw| raw.lines().count())
        .unwrap_or(0);
    let references = skill_path.join("references").is_dir();
    let status = if lines > SKILL_MD_LINE_LIMIT && !references {
        "fail"
    } else if lines > SKILL_MD_LINE_LIMIT {
        "warning"
    } else {
        "pass"
    };
    (
        status,
        json!({"main_line_count": lines, "limit": SKILL_MD_LINE_LIMIT, "references_dir": references}),
    )
}

fn recommendation(
    skill: &str,
    mode: &str,
    checks: &BTreeMap<String, String>,
    details: &Value,
) -> Value {
    let blocking = checks.iter().any(|(name, status)| {
        name != "source_drift" && matches!(status.as_str(), "fail" | "unknown")
    });
    if blocking {
        return json!({
            "action": "keep_editing",
            "command": format!("loom skill improve {skill}"),
            "reason": "one or more preflight gates are failing or unknown",
        });
    }
    if details["drift"]["changed"].as_bool().unwrap_or(false) && mode != "release_preflight" {
        return json!({
                "action": "commit",
            "command": format!(
                "loom skill commit {skill} --from-source --preflight --message 'improve {skill}'"
            ),
            "reason": "local skill changes passed preflight",
        });
    }
    json!({
        "action": "none",
        "command": Value::Null,
        "reason": "no blocking regression or unsaved source drift was detected",
    })
}

fn regression(id: &str, message: &str, suggested_action: &str, details: Value) -> Value {
    json!({
        "id": id,
        "severity": "error",
        "message": message,
        "suggested_action": suggested_action,
        "details": details,
    })
}

fn preflight_blocked(message: &str, report: SkillPreflightReport) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    failure.details = json!({ "report": report.into_value() });
    failure
}
