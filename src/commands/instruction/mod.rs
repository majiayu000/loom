mod analysis;
mod model;

use std::path::Path;

use serde_json::{Value, json};

use crate::cli::{
    InstructionClassifyArgs, InstructionCommand, InstructionDoctorArgs, InstructionMigratePlanArgs,
    InstructionScanArgs, InstructionShowArgs,
};
use crate::envelope::Meta;
use crate::types::ErrorCode;

use super::helpers::agent_kind_as_str;
use super::{App, CommandFailure};
use analysis::{
    doctor_findings, load_skill_for_doctor, migration_plan, proposed_skill_name, read_text_body,
    validate_migration_name,
};
use model::{
    adapter_metadata_json, classify_path, resolve_file, resolve_workspace, scan_workspace,
};

impl App {
    pub(crate) fn cmd_instruction(
        &self,
        command: &InstructionCommand,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            InstructionCommand::Scan(args) => self.cmd_instruction_scan(args),
            InstructionCommand::Show(args) => self.cmd_instruction_show(args),
            InstructionCommand::Classify(args) => self.cmd_instruction_classify(args),
            InstructionCommand::Doctor(args) => self.cmd_instruction_doctor(args),
            InstructionCommand::MigratePlan(args) => self.cmd_instruction_migrate_plan(args),
        }
    }

    fn cmd_instruction_scan(
        &self,
        args: &InstructionScanArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let workspace = resolve_workspace(args.workspace.as_deref())?;
        let scan = scan_workspace(&workspace, args.agent)?;
        Ok((
            json!({
                "workspace": workspace.display().to_string(),
                "agent_filter": args.agent.map(agent_kind_as_str),
                "adapter_metadata": adapter_metadata_json(args.agent),
                "surfaces": scan.surfaces,
                "unsupported_surfaces": scan.unsupported_surfaces,
                "warnings": scan.warnings,
                "summary": {
                    "surface_count": scan.surfaces.len(),
                    "unsupported_count": scan.unsupported_surfaces.len(),
                    "warning_count": scan.warnings.len(),
                }
            }),
            Meta::default(),
        ))
    }

    fn cmd_instruction_show(
        &self,
        args: &InstructionShowArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let workspace = resolve_workspace(args.workspace.as_deref())?;
        let scan = scan_workspace(&workspace, None)?;
        let surface = scan
            .surfaces
            .into_iter()
            .find(|surface| surface.instruction_id == args.instruction_id)
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "instruction '{}' was not found in the current workspace",
                        args.instruction_id
                    ),
                )
            })?;

        Ok((
            json!({
                "workspace": workspace.display().to_string(),
                "surface": surface,
            }),
            Meta::default(),
        ))
    }

    fn cmd_instruction_classify(
        &self,
        args: &InstructionClassifyArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let workspace = resolve_workspace(None)?;
        let path = resolve_file(&args.path)?;
        let surface = classify_path(&workspace, &path)?;
        Ok((
            json!({
                "workspace": workspace.display().to_string(),
                "surface": surface,
            }),
            Meta::default(),
        ))
    }

    fn cmd_instruction_doctor(
        &self,
        args: &InstructionDoctorArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let workspace = resolve_workspace(args.workspace.as_deref())?;
        let scan = scan_workspace(&workspace, args.agent)?;
        let skill = match args.skill.as_deref() {
            Some(skill) => Some(load_skill_for_doctor(self, skill)?),
            None => None,
        };
        let findings = doctor_findings(&scan, skill.as_ref())?;

        Ok((
            json!({
                "workspace": workspace.display().to_string(),
                "agent_filter": args.agent.map(agent_kind_as_str),
                "skill": skill.as_ref().map(|skill| json!({
                    "name": skill.name,
                    "path": skill.path,
                })),
                "surfaces": scan.surfaces,
                "unsupported_surfaces": scan.unsupported_surfaces,
                "findings": findings,
                "summary": {
                    "surface_count": scan.surfaces.len(),
                    "finding_count": findings.len(),
                }
            }),
            Meta::default(),
        ))
    }

    fn cmd_instruction_migrate_plan(
        &self,
        args: &InstructionMigratePlanArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        if !args.dry_run {
            return Err(CommandFailure::new(
                ErrorCode::PolicyBlocked,
                "instruction migration apply is deferred; rerun with --dry-run",
            ));
        }
        validate_migration_name(args.to, args.name.as_deref())?;

        let workspace = resolve_workspace(args.workspace.as_deref())?;
        let scan = scan_workspace(&workspace, None)?;
        let surface = scan
            .surfaces
            .into_iter()
            .find(|surface| surface.instruction_id == args.instruction_id)
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "instruction '{}' was not found in the current workspace",
                        args.instruction_id
                    ),
                )
            })?;
        let proposed_name = args
            .name
            .clone()
            .unwrap_or_else(|| proposed_skill_name(&surface));
        let _ = read_text_body(Path::new(&surface.path))?;
        let plan = migration_plan(&surface, args.to, &proposed_name);

        Ok((
            json!({
                "workspace": workspace.display().to_string(),
                "dry_run": true,
                "instruction": surface,
                "plan": plan,
            }),
            Meta::default(),
        ))
    }
}
