use crate::commands::convergence_input::source_replacement_risk_paths;
use crate::commands::helpers::{
    map_arg, map_io, map_lock, map_registry_state, validate_skill_name,
};
use crate::commands::{App, CommandFailure};
use crate::envelope::Meta;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;
use serde_json::{Value, json};
use uuid::Uuid;

impl App {
    pub(super) fn cmd_plan_team_install(
        &self,
        args: &crate::cli::PlanTeamInstallArgs,
    ) -> Result<(Value, Meta), CommandFailure> {
        use crate::commands::team_package::{self, TeamArtifactManifest, TeamInput};
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace_lock = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let manifest: TeamArtifactManifest =
            serde_json::from_str(&std::fs::read_to_string(&args.manifest).map_err(map_io)?)
                .map_err(map_io)?;
        manifest.validate().map_err(map_io)?;
        if RegistryStatePaths::from_app_context(&self.ctx)
            .maybe_load_snapshot()
            .map_err(map_registry_state)?
            .is_none()
        {
            return Err(CommandFailure::new(
                ErrorCode::StateNotInitialized,
                "initialize the registry with loom workspace init before installing a team artifact",
            ));
        }
        let old_sources =
            team_package::read_optional(&self.ctx.root.join(team_package::METADATA_PATHS[0]))
                .map_err(map_io)?;
        let old_lock =
            team_package::read_optional(&self.ctx.root.join(team_package::METADATA_PATHS[1]))
                .map_err(map_io)?;
        let source_snapshot: Option<crate::commands::provenance::SkillSourcesFile> = old_sources
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(map_io)?;
        let record_snapshot = source_snapshot.as_ref().and_then(|sources| {
            sources
                .sources
                .iter()
                .find(|record| record.skill_id == args.skill)
        });
        let lock_snapshot: Option<Value> = old_lock
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(map_io)?;
        let lock = lock_snapshot
            .as_ref()
            .and_then(|lock| lock.get("skills"))
            .and_then(|skills| skills.get(&args.skill));
        let digest =
            team_package::source_digest(&self.ctx.skill_path(&args.skill)).map_err(map_io)?;
        if digest != "absent" {
            let record = record_snapshot.ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::DependencyConflict,
                    "same-name skill has no team provenance",
                )
            })?;
            if record.source.provider != "team"
                || !record
                    .source
                    .team
                    .as_ref()
                    .is_some_and(|old| manifest.same_source(old))
            {
                return Err(CommandFailure::new(
                    ErrorCode::DependencyConflict,
                    "same-name skill belongs to a different source",
                ));
            }
            let origin = record
                .source
                .team
                .as_ref()
                .expect("team source was checked");
            origin.validate().map_err(map_io)?;
            let mut expected_source = origin.descriptor();
            expected_source.team_tree_digest = Some(digest.clone());
            if record.source != expected_source {
                return Err(CommandFailure::new(
                    ErrorCode::DependencyConflict,
                    "team provenance identity is inconsistent",
                ));
            }
            if origin.version_id == manifest.version_id && origin.sha256 != manifest.sha256 {
                return Err(CommandFailure::new(
                    ErrorCode::DependencyConflict,
                    "immutable team version changed its archive digest",
                ));
            }
            if record.artifact.digest != digest
                || lock.is_none_or(|lock| {
                    lock["digest"].as_str() != Some(&digest)
                        || lock["team"] != json!(record.source.team)
                        || lock["provider"] != "team"
                        || lock["team_tree_digest"] != json!(digest)
                        || lock["source"] != json!(record.source.locator)
                        || lock["ref"] != json!(record.source.requested_ref)
                        || !lock["commit"].is_null()
                        || !lock["tree_sha"].is_null()
                })
            {
                return Err(CommandFailure::new(
                    ErrorCode::DependencyConflict,
                    "team source has local changes or inconsistent lock identity",
                ));
            }
            if !source_replacement_risk_paths(&self.ctx, &args.skill)?.is_empty() {
                return Err(CommandFailure::new(
                    ErrorCode::DependencyConflict,
                    "team source has uncommitted local changes",
                ));
            }
        } else if record_snapshot.is_some() {
            return Err(CommandFailure::new(
                ErrorCode::DependencyConflict,
                "recorded team source is missing; preserve and repair existing state",
            ));
        }
        if digest == "absent"
            && (!source_replacement_risk_paths(&self.ctx, &args.skill)?.is_empty()
                || lock.is_some())
        {
            return Err(CommandFailure::new(
                ErrorCode::DependencyConflict,
                "missing source still has local changes or a lock identity",
            ));
        }
        let input_root = self.ctx.state_dir.join("transactions");
        std::fs::create_dir_all(&input_root).map_err(map_io)?;
        let input_path = input_root.join(format!("team-input-{}", Uuid::new_v4()));
        team_package::extract(&args.archive, &manifest, &input_path).map_err(map_io)?;
        let mut input_guard = team_package::UnpublishedInput::new(input_path.clone());
        let mut record = crate::commands::provenance::provenance_record_for_skill(
            &args.skill,
            manifest.descriptor(),
            &input_path,
        )?;
        record.source.team_tree_digest = Some(record.artifact.digest.clone());
        let (new_sources, new_lock) = crate::commands::provenance::planned_record_files(
            &self.ctx,
            record,
            old_sources.as_deref(),
            old_lock.as_deref(),
        )
        .map_err(map_io)?;
        let input = TeamInput {
            manifest,
            input_path: input_path.display().to_string(),
            old_sources,
            old_lock,
            new_sources,
            new_lock,
        };
        let result = self.plan_converge_input(&args.convergence_args(), Some(input), Some(&digest));
        if result.is_ok() {
            input_guard.publish();
        }
        result
    }
}
