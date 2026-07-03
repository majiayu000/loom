use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{ProjectionMethod, SkillActivateArgs};
use crate::envelope::Meta;
use crate::fs_util::{remove_path_if_exists, rename_atomic, write_atomic};
use crate::gitops;
use crate::state_model::{RegistryBindingRule, RegistryProjectionInstance};
use crate::types::ErrorCode;

use super::super::helpers::{
    commit_registry_state, map_git, map_io, map_lock, map_registry_state, projection_instance_id,
    projection_method_as_str, shell_arg,
};
use super::super::projections::{
    maybe_autosync_or_queue, record_registry_observation, record_registry_operation,
    upsert_projection, upsert_rule,
};
use super::super::telemetry::{record_skill_activation_telemetry, telemetry_warning};
use super::super::{App, CommandFailure};
use super::apply::{restore_activation_state, save_activation_state};
use super::plan::{activation_plan, activation_state_changed};
use super::resolve::{
    ActivationResolved, ActivationSelection, optional_snapshot, resolve_activation, scope_str,
};
use crate::commands::skill_compile::CompiledActivationCandidate;

const COMPILED_PROJECTION_SCHEMA_VERSION: u32 = 1;
const COMPILED_PROJECTION_KIND: &str = "compiled_activation";
const COMPILED_METADATA_DIR: &str = ".loom/compiled";

impl App {
    pub(super) fn cmd_skill_activate_compiled(
        &self,
        args: &SkillActivateArgs,
        selection: ActivationSelection,
        candidates: Vec<CompiledActivationCandidate>,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let Some(candidate) = select_compiled_activation_candidate(&selection, &candidates) else {
            return Err(compiled_activation_failure(
                &selection,
                args.artifact.as_deref(),
                &candidates,
            ));
        };
        let selection = compiled_activation_projection_selection(&selection);
        if args.dry_run {
            let snapshot = optional_snapshot(&self.ctx)?;
            let resolved = resolve_activation(&self.ctx, &snapshot, selection)?;
            return Ok((
                json!({
                    "plan": activation_plan(&resolved, true),
                    "compiled": compiled_activation_plan_details(candidate),
                    "dry_run": true
                }),
                Meta::default(),
            ));
        }

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let resolved = resolve_activation(&self.ctx, &snapshot, selection)?;
        let plan = activation_plan(&resolved, false);

        let projection_changed =
            apply_compiled_activation_projection(&self.ctx, &resolved, candidate)?;
        let state_changed = activation_state_changed(&resolved) || projection_changed;
        if !state_changed {
            return Ok((
                json!({
                    "plan": plan,
                    "compiled": compiled_activation_plan_details(candidate),
                    "noop": true
                }),
                Meta::default(),
            ));
        }

        let original_targets = snapshot.targets.clone();
        let original_bindings = snapshot.bindings.clone();
        let original_rules = snapshot.rules.clone();
        let original_projections = snapshot.projections.clone();
        let mut targets = original_targets.clone();
        let mut bindings = original_bindings.clone();
        let mut rules = original_rules.clone();
        let mut projections = original_projections.clone();

        if resolved.target_is_new {
            targets.targets.push(resolved.target.clone());
            targets
                .targets
                .sort_by(|left, right| left.target_id.cmp(&right.target_id));
        }
        if resolved.binding_is_new {
            bindings.bindings.push(resolved.binding.clone());
            bindings
                .bindings
                .sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
        }

        let rule = RegistryBindingRule {
            binding_id: resolved.binding.binding_id.clone(),
            skill_id: resolved.selection.skill.clone(),
            target_id: resolved.target.target_id.clone(),
            method: resolved.selection.method,
            watch_policy: "observe_only".to_string(),
            created_at: resolved
                .existing_rule
                .as_ref()
                .and_then(|rule| rule.created_at)
                .or_else(|| Some(Utc::now())),
        };
        upsert_rule(&mut rules, rule);

        let head = gitops::head(&self.ctx).map_err(map_git)?;
        let instance_id = projection_instance_id(
            &resolved.selection.skill,
            &resolved.binding.binding_id,
            &resolved.target.target_id,
        );
        let projection = RegistryProjectionInstance {
            instance_id: instance_id.clone(),
            skill_id: resolved.selection.skill.clone(),
            binding_id: Some(resolved.binding.binding_id.clone()),
            target_id: resolved.target.target_id.clone(),
            materialized_path: resolved.materialized_path.display().to_string(),
            method: resolved.selection.method,
            last_applied_rev: head.clone(),
            health: crate::core::vocab::Health::Healthy,
            observed_drift: Some(false),
            source_tree_digest: None,
            materialized_tree_digest: None,
            last_observed_at: None,
            last_observed_error: None,
            updated_at: Some(Utc::now()),
        };
        upsert_projection(&mut projections, projection.clone());

        save_activation_state(
            &paths,
            &targets,
            &bindings,
            &rules,
            &projections,
            &original_targets,
        )?;
        let compiled = compiled_activation_plan_details(candidate);
        let op_id = match record_registry_operation(
            &paths,
            "skill.activate",
            json!({
                "skill_id": resolved.selection.skill,
                "agent": resolved.selection.agent,
                "scope": scope_str(resolved.selection.scope),
                "profile": resolved.selection.profile,
                "binding_id": resolved.binding.binding_id,
                "target_id": resolved.target.target_id,
                "method": projection_method_as_str(resolved.selection.method),
                "compiled": compiled,
                "request_id": request_id
            }),
            json!({"instance_id": instance_id}),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                restore_activation_state(
                    &paths,
                    &original_targets,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                )?;
                return Err(map_registry_state(err));
            }
        };
        record_registry_observation(
            &paths,
            &instance_id,
            "activated",
            Some(projection.materialized_path.clone()),
            None,
            Some(head),
        )
        .map_err(map_registry_state)?;

        let commit = commit_registry_state(
            &self.ctx,
            &format!(
                "activate({}): record compiled skill activation",
                projection.skill_id
            ),
        )?;
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "skill.activate",
                request_id,
                json!({
                    "skill": projection.skill_id,
                    "binding_id": projection.binding_id,
                    "target_id": projection.target_id,
                    "commit": commit,
                    "compiled": compiled_activation_plan_details(candidate)
                }),
                &mut meta,
            )?;
        }
        if let Err(err) = record_skill_activation_telemetry(
            &self.ctx,
            &projection.skill_id,
            &resolved.selection.agent,
            true,
            resolved.selection.workspace.as_deref(),
        ) {
            meta.warnings
                .push(telemetry_warning("skill activation", &err));
        }

        Ok((
            json!({
                "plan": plan,
                "compiled": compiled_activation_plan_details(candidate),
                "projection": projection,
                "target": resolved.target,
                "binding": resolved.binding,
                "commit": commit,
                "noop": false
            }),
            meta,
        ))
    }
}

pub(super) fn compiled_activation_projection_selection(
    selection: &ActivationSelection,
) -> ActivationSelection {
    let mut compiled = selection.clone();
    compiled.method = ProjectionMethod::Materialize;
    compiled
}

pub(super) fn select_compiled_activation_candidate<'a>(
    selection: &ActivationSelection,
    candidates: &'a [CompiledActivationCandidate],
) -> Option<&'a CompiledActivationCandidate> {
    candidates.iter().find(|candidate| {
        candidate.valid
            && candidate.status == "valid"
            && !candidate.source_stale
            && candidate_matches_selection(candidate, selection)
    })
}

pub(super) fn compiled_activation_plan_details(candidate: &CompiledActivationCandidate) -> Value {
    json!({
        "projection_kind": COMPILED_PROJECTION_KIND,
        "artifact_id": candidate.artifact_id,
        "artifact_path": candidate.path,
        "materialized_entrypoint": "SKILL.md",
        "metadata_dir": COMPILED_METADATA_DIR,
    })
}

pub(super) fn apply_compiled_activation_projection(
    ctx: &crate::state::AppContext,
    resolved: &ActivationResolved,
    candidate: &CompiledActivationCandidate,
) -> std::result::Result<bool, CommandFailure> {
    if !candidate.valid || candidate.status != "valid" || candidate.source_stale {
        return Err(CommandFailure::new(
            ErrorCode::PolicyBlocked,
            "compiled activation requires a fresh valid artifact",
        ));
    }

    let target_base = PathBuf::from(&resolved.target.path);
    fs::create_dir_all(&target_base).map_err(map_io)?;
    if resolved.materialized_path.exists()
        || fs::symlink_metadata(&resolved.materialized_path).is_ok()
    {
        if compiled_projection_matches(&resolved.materialized_path, candidate) {
            return Ok(false);
        }
        return Err(CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "projection path '{}' already exists and is not the selected compiled artifact '{}'",
                resolved.materialized_path.display(),
                candidate.artifact_id
            ),
        ));
    }

    let tmp_dir = target_base.join(format!(".loom-compiled-tmp-{}", Uuid::new_v4()));
    if let Err(failure) = write_compiled_projection_dir(ctx, resolved, candidate, &tmp_dir) {
        return Err(with_tmp_cleanup(failure, &tmp_dir));
    }
    if let Err(err) = rename_atomic(&tmp_dir, &resolved.materialized_path) {
        return Err(with_tmp_cleanup(map_io(err), &tmp_dir));
    }
    Ok(true)
}

pub(super) fn compiled_activation_failure(
    selection: &ActivationSelection,
    artifact: Option<&str>,
    candidates: &[CompiledActivationCandidate],
) -> CommandFailure {
    let reason = compiled_activation_block_reason(selection, candidates);
    let message = match reason {
        "compiled_artifact_missing" => format!(
            "compiled activation requires a compiled artifact for skill '{}' agent '{}' profile '{}'",
            selection.skill, selection.agent, selection.profile
        ),
        "compiled_artifact_agent_profile_mismatch" => format!(
            "compiled activation artifact does not match agent '{}' profile '{}'",
            selection.agent, selection.profile
        ),
        _ => format!(
            "compiled activation requires a valid compiled artifact for skill '{}' agent '{}' profile '{}'",
            selection.skill, selection.agent, selection.profile
        ),
    };
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    let reports = candidates
        .iter()
        .map(|candidate| candidate.report.clone())
        .collect::<Vec<_>>();
    failure.details = json!({
        "reason": reason,
        "skill": selection.skill,
        "agent": selection.agent,
        "profile": selection.profile,
        "artifact": artifact,
        "reports": reports,
        "next_actions": [
            compile_write_action(selection),
            verify_action(selection, artifact),
            activation_fallback_action(selection),
        ],
    });
    failure
}

fn write_compiled_projection_dir(
    ctx: &crate::state::AppContext,
    resolved: &ActivationResolved,
    candidate: &CompiledActivationCandidate,
    tmp_dir: &Path,
) -> std::result::Result<(), CommandFailure> {
    fs::create_dir_all(tmp_dir).map_err(map_io)?;
    let artifact_dir = PathBuf::from(&candidate.path);
    let activation = fs::read_to_string(artifact_dir.join("activation.md")).map_err(map_io)?;
    write_atomic(
        &tmp_dir.join("SKILL.md"),
        &compiled_skill_md(ctx, resolved, candidate, &activation),
    )
    .map_err(map_io)?;

    let metadata_dir = tmp_dir.join(COMPILED_METADATA_DIR);
    fs::create_dir_all(&metadata_dir).map_err(map_io)?;
    for file in [
        "manifest.json",
        "activation.md",
        "catalog.json",
        "boundaries.json",
        "tool-interface.json",
        "references.index.json",
        "source-digest.txt",
    ] {
        fs::copy(artifact_dir.join(file), metadata_dir.join(file)).map_err(map_io)?;
    }
    let projection = json!({
        "schema_version": COMPILED_PROJECTION_SCHEMA_VERSION,
        "kind": COMPILED_PROJECTION_KIND,
        "skill": resolved.selection.skill,
        "agent": resolved.selection.agent,
        "profile": resolved.selection.profile,
        "artifact_id": candidate.artifact_id,
        "artifact_path": candidate.path,
        "source_skill_path": ctx.skill_path(&resolved.selection.skill).display().to_string(),
        "materialized_at": Utc::now().to_rfc3339(),
        "entrypoint": "SKILL.md",
        "manifest": format!("{COMPILED_METADATA_DIR}/manifest.json"),
    });
    let mut raw = serde_json::to_string_pretty(&projection).map_err(|err| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("failed to serialize compiled projection metadata: {err}"),
        )
    })?;
    raw.push('\n');
    write_atomic(&metadata_dir.join("projection.json"), &raw).map_err(map_io)?;
    Ok(())
}

fn compiled_skill_md(
    ctx: &crate::state::AppContext,
    resolved: &ActivationResolved,
    candidate: &CompiledActivationCandidate,
    activation: &str,
) -> String {
    let skill = &resolved.selection.skill;
    let mut body = String::new();
    body.push_str("---\n");
    body.push_str(&format!("name: {skill}\n"));
    body.push_str(&format!(
        "description: Compiled activation projection for {skill}.\n"
    ));
    body.push_str("---\n\n");
    body.push_str("<!-- loom-compiled-activation\n");
    body.push_str(&format!("artifact_id: {}\n", candidate.artifact_id));
    body.push_str(&format!("artifact_path: {}\n", candidate.path));
    body.push_str(&format!(
        "source_skill_path: {}\n",
        ctx.skill_path(skill).display()
    ));
    body.push_str(&format!(
        "manifest: {COMPILED_METADATA_DIR}/manifest.json\n"
    ));
    body.push_str("-->\n\n");
    body.push_str(activation.trim_end());
    body.push('\n');
    body
}

fn compiled_projection_matches(path: &Path, candidate: &CompiledActivationCandidate) -> bool {
    let metadata_path = path.join(COMPILED_METADATA_DIR).join("projection.json");
    let Ok(raw) = fs::read_to_string(metadata_path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return false;
    };
    value["schema_version"] == json!(COMPILED_PROJECTION_SCHEMA_VERSION)
        && value["kind"] == json!(COMPILED_PROJECTION_KIND)
        && value["artifact_id"] == json!(candidate.artifact_id)
        && path.join("SKILL.md").is_file()
}

fn with_tmp_cleanup(failure: CommandFailure, tmp_dir: &Path) -> CommandFailure {
    match remove_path_if_exists(tmp_dir) {
        Ok(()) => failure,
        Err(err) => failure.with_rollback_errors(vec![json!({
            "path": tmp_dir.display().to_string(),
            "error": err.to_string(),
        })]),
    }
}

fn compiled_activation_block_reason(
    selection: &ActivationSelection,
    candidates: &[CompiledActivationCandidate],
) -> &'static str {
    if candidates.is_empty()
        || candidates
            .iter()
            .all(|candidate| candidate.status == "missing")
    {
        return "compiled_artifact_missing";
    }
    let all_candidates_have_identity = candidates
        .iter()
        .all(|candidate| candidate.agent.is_some() && candidate.profile.is_some());
    if all_candidates_have_identity
        && !candidates
            .iter()
            .any(|candidate| candidate_matches_selection(candidate, selection))
    {
        return "compiled_artifact_agent_profile_mismatch";
    }
    "compiled_artifact_not_valid"
}

fn candidate_matches_selection(
    candidate: &CompiledActivationCandidate,
    selection: &ActivationSelection,
) -> bool {
    candidate
        .agent
        .as_deref()
        .is_some_and(|agent| agent.eq_ignore_ascii_case(selection.agent.as_str()))
        && candidate.profile.as_deref() == Some(selection.profile.as_str())
}

fn activation_fallback_action(selection: &ActivationSelection) -> String {
    let mut command = format!(
        "loom skill activate {} --agent {} --scope {} --profile {}",
        shell_arg(&selection.skill),
        shell_arg(&selection.agent),
        scope_str(selection.scope),
        shell_arg(&selection.profile)
    );
    if let Some(workspace) = &selection.workspace {
        command.push_str(&format!(" --workspace {}", shell_arg(workspace)));
    }
    if let Some(target_id) = &selection.target_id {
        command.push_str(&format!(" --target {}", shell_arg(target_id)));
    }
    if !matches!(selection.method, ProjectionMethod::Symlink) {
        command.push_str(&format!(
            " --method {}",
            projection_method_as_str(selection.method)
        ));
    }
    command
}

fn compile_write_action(selection: &ActivationSelection) -> String {
    let skill_selector = if matches!(selection.skill.as_str(), "list" | "verify") {
        format!("--skill {}", shell_arg(&selection.skill))
    } else {
        shell_arg(&selection.skill)
    };
    format!(
        "loom skill compile {} --agent {} --profile {}",
        skill_selector,
        shell_arg(&selection.agent),
        shell_arg(&selection.profile)
    )
}

fn verify_action(selection: &ActivationSelection, artifact: Option<&str>) -> String {
    match artifact {
        Some(artifact) => format!(
            "loom skill compile verify {} --artifact {}",
            shell_arg(&selection.skill),
            shell_arg(artifact)
        ),
        None => format!("loom skill compile verify {}", shell_arg(&selection.skill)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use crate::cli::ActivationScope;

    use super::*;

    fn selection() -> ActivationSelection {
        ActivationSelection {
            skill: "demo".to_string(),
            agent: "codex".to_string(),
            scope: ActivationScope::User,
            profile: "team".to_string(),
            workspace: None,
            target_id: None,
            method: ProjectionMethod::Symlink,
        }
    }

    #[test]
    fn candidate_match_treats_agent_case_as_normalized() {
        let candidate = CompiledActivationCandidate {
            artifact_id: "artifact-a".to_string(),
            path: "/tmp/artifact-a".to_string(),
            valid: true,
            status: "valid".to_string(),
            source_stale: false,
            agent: Some("Codex".to_string()),
            profile: Some("team".to_string()),
            report: json!({}),
        };

        assert!(candidate_matches_selection(&candidate, &selection()));
    }

    #[test]
    fn compiled_selection_materializes_projection() {
        let compiled = compiled_activation_projection_selection(&selection());

        assert_eq!(compiled.method, ProjectionMethod::Materialize);
    }

    #[test]
    fn activation_fallback_action_preserves_selectors() {
        let selection = ActivationSelection {
            skill: "demo".to_string(),
            agent: "codex".to_string(),
            scope: ActivationScope::Project,
            profile: "team".to_string(),
            workspace: Some(PathBuf::from("/tmp/project space")),
            target_id: Some("project-target".to_string()),
            method: ProjectionMethod::Copy,
        };

        assert_eq!(
            activation_fallback_action(&selection),
            "loom skill activate demo --agent codex --scope project --profile team --workspace '/tmp/project space' --target project-target --method copy"
        );
    }

    #[test]
    fn compile_write_action_disambiguates_compile_subcommand_names() {
        let mut selection = selection();
        selection.skill = "verify".to_string();
        selection.profile = "default".to_string();

        assert_eq!(
            compile_write_action(&selection),
            "loom skill compile --skill verify --agent codex --profile default"
        );
    }
}
