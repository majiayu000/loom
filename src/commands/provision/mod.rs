mod model;
mod planner;
mod utils;

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::cli::{
    ProvisionApplyArgs, ProvisionCommand, ProvisionDoctorArgs, ProvisionExportArgs,
    ProvisionImportArgs, ProvisionPlanArgs,
};
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::types::ErrorCode;

use super::helpers::{map_io, validate_non_empty};
use super::{App, CommandFailure};
use model::ProvisionPlan;
use planner::{build_provision_plan, provision_export_format_name, provision_target_name};
use utils::{digest_file, provision_next_actions, resolve_workspace, validate_provision_agent};

impl App {
    pub fn cmd_provision(
        &self,
        command: &ProvisionCommand,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            ProvisionCommand::Plan(args) => self.cmd_provision_plan(args),
            ProvisionCommand::Apply(args) => self.cmd_provision_apply(args),
            ProvisionCommand::Doctor(args) => self.cmd_provision_doctor(args),
            ProvisionCommand::Export(args) => self.cmd_provision_export(args),
            ProvisionCommand::Import(args) => self.cmd_provision_import(args),
        }
    }

    fn cmd_provision_plan(
        &self,
        args: &ProvisionPlanArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_provision_agent(&args.agent)?;
        let workspace = resolve_workspace(self, args.workspace.as_deref())?;
        let plan = build_provision_plan(&self.ctx, args.target, &workspace, &args.agent)?;
        if let Some(output_plan) = &args.output_plan {
            let mut body = serde_json::to_string_pretty(&plan).map_err(map_io)?;
            body.push('\n');
            write_atomic(output_plan, &body).map_err(map_io)?;
        }

        Ok((
            json!({
                "plan": plan,
                "output_plan": args.output_plan.as_ref().map(|path| path.display().to_string()),
                "artifact_written": args.output_plan.is_some(),
                "target_writes_performed": false,
            }),
            Meta::default(),
        ))
    }

    fn cmd_provision_doctor(
        &self,
        args: &ProvisionDoctorArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_provision_agent(&args.agent)?;
        let (plan, workspace, plan_source) = self.provision_doctor_plan(args)?;
        let generated_files = generated_file_statuses(&workspace, &plan.files_to_write);
        let dependency_status = if plan
            .dependency_readiness
            .iter()
            .all(|dependency| dependency.ready)
        {
            "pass"
        } else {
            "fail"
        };
        let generated_status = if generated_files
            .iter()
            .all(|file| file["status"].as_str() == Some("present"))
        {
            "pass"
        } else {
            "warning"
        };
        let healthy = dependency_status == "pass" && generated_status == "pass";

        Ok((
            json!({
                "target_kind": plan.target_kind,
                "workspace": plan.workspace,
                "plan": args.plan,
                "plan_id": plan.plan_id,
                "plan_source": plan_source,
                "healthy": healthy,
                "status": if healthy { "ready" } else { "action_required" },
                "checks": {
                    "target": { "status": "pass", "kind": plan.target_kind },
                    "workspace": {
                        "status": if workspace.is_dir() { "pass" } else { "fail" },
                        "path": workspace.display().to_string(),
                    },
                    "generated_files": {
                        "status": generated_status,
                        "files": generated_files,
                    },
                    "adapter_paths": {
                        "status": "pass",
                        "active_views": plan.active_views,
                    },
                    "dependencies": {
                        "status": dependency_status,
                        "readiness": plan.dependency_readiness,
                    },
                    "secrets": {
                        "status": if plan.secrets_required.iter().all(|secret| secret.present) { "pass" } else { "warning" },
                        "required": plan.secrets_required,
                    },
                    "policy": {
                        "status": "pass",
                        "apply_deferred": true,
                    },
                },
                "findings": plan.findings,
                "next_actions": provision_next_actions(
                    provision_target_name(args.target),
                    &workspace,
                    &args.agent,
                ),
                "target_writes_performed": false,
            }),
            Meta::default(),
        ))
    }

    fn provision_doctor_plan(
        &self,
        args: &ProvisionDoctorArgs,
    ) -> std::result::Result<(ProvisionPlan, std::path::PathBuf, &'static str), CommandFailure>
    {
        if let Some(plan_arg) = &args.plan {
            let plan_path = Path::new(plan_arg);
            if plan_path.is_file() {
                let raw = fs::read_to_string(plan_path).map_err(map_io)?;
                let plan: ProvisionPlan = serde_json::from_str(&raw).map_err(map_io)?;
                let expected_target = provision_target_name(args.target);
                if plan.target_kind != expected_target {
                    return Err(CommandFailure::new(
                        ErrorCode::ArgInvalid,
                        format!(
                            "plan target '{}' does not match requested target '{}'",
                            plan.target_kind, expected_target
                        ),
                    ));
                }
                let workspace = match args.workspace.as_deref() {
                    Some(workspace) => resolve_workspace(self, Some(workspace))?,
                    None => Path::new(&plan.workspace).to_path_buf(),
                };
                return Ok((plan, workspace, "artifact"));
            }
        }

        let workspace = resolve_workspace(self, args.workspace.as_deref())?;
        let plan = build_provision_plan(&self.ctx, args.target, &workspace, &args.agent)?;
        Ok((plan, workspace, "generated"))
    }

    fn cmd_provision_apply(
        &self,
        args: &ProvisionApplyArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_non_empty("idempotency-key", &args.idempotency_key)?;
        Err(deferred_failure(
            "provision apply is deferred until plan revalidation, idempotency, approval, and target-write gates are implemented",
            json!({
                "plan": args.plan,
                "approvals": args.approvals,
                "target_writes_performed": false,
            }),
        ))
    }

    fn cmd_provision_export(
        &self,
        args: &ProvisionExportArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        Err(deferred_failure(
            "provision export is deferred until devcontainer, shell, and tar artifact gates are implemented",
            json!({
                "plan": args.plan,
                "format": provision_export_format_name(args.format),
                "output": args.output.display().to_string(),
                "target_writes_performed": false,
            }),
        ))
    }

    fn cmd_provision_import(
        &self,
        args: &ProvisionImportArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        Err(deferred_failure(
            "provision import is deferred until artifact validation and dry-run diff gates are implemented",
            json!({
                "artifact": args.artifact.display().to_string(),
                "dry_run": args.dry_run,
                "target_writes_performed": false,
            }),
        ))
    }
}

fn generated_file_statuses(workspace: &Path, files: &[model::ProvisionFilePlan]) -> Vec<Value> {
    files
        .iter()
        .map(|file| {
            let absolute = workspace.join(&file.path);
            let status = match digest_file(&absolute) {
                Some(current) if current == file.content_digest => "present",
                Some(_) => "different",
                None => "missing",
            };
            json!({
                "path": file.path,
                "status": status,
                "preimage_digest": file.preimage_digest,
                "content_digest": file.content_digest,
            })
        })
        .collect()
}

fn deferred_failure(message: &str, details: Value) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    failure.details = details;
    failure
}
