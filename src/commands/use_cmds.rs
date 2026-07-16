use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::agent_adapters::{
    SOURCE_BUILT_IN, built_in_projection_root, load_agent_adapters, preferred_discovery_root,
};
use crate::cli::{
    BindingAddArgs, ProjectArgs, TargetAddArgs, TargetCommand, TargetOwnership, UseArgs, UseScope,
    WorkspaceBindingCommand, WorkspaceMatcherKind,
};
use crate::envelope::Meta;
use crate::error_actions::NextAction;
use crate::state::AppContext;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::helpers::{
    agent_kind_as_str, map_arg, projection_method_as_str, shell_arg, validate_skill_name,
};
use super::{App, CommandFailure};

impl App {
    pub fn cmd_use(
        &self,
        args: &UseArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_use_args(args)?;
        validate_skill_name(&args.skill).map_err(map_arg)?;
        if !self.ctx.skill_path(&args.skill).is_dir() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        let workspace = use_workspace(args)?;
        let steps = args
            .agents
            .iter()
            .map(|agent| {
                let agent_name = agent_kind_as_str(*agent);
                let target_path = target_path_for(&self.ctx, args, agent_name, workspace.as_deref())?;
                Ok(json!({
                    "agent": agent_name,
                    "scope": use_scope_as_str(args.scope),
                    "workspace": workspace.as_ref().map(|path| path.display().to_string()),
                    "binding_matcher": binding_matcher_json(args.scope, workspace.as_deref()),
                    "profile": args.profile,
                    "method": projection_method_as_str(args.method),
                    "target_path": target_path.display().to_string(),
                    "requires_adopt": target_path.exists() && !target_is_managed(&self.ctx, agent_name, &target_path)?,
                    "will_create_or_reuse": [
                        "managed_target",
                        "workspace_binding",
                        "skill_projection"
                    ],
                }))
            })
            .collect::<std::result::Result<Vec<_>, CommandFailure>>()?;

        let apply_command = use_apply_command(&self.ctx.root, args, workspace.as_deref());
        if !args.apply {
            return Ok((
                json!({
                    "dry_run": true,
                    "operation": "use",
                    "skill": args.skill,
                    "apply_required": true,
                    "steps": steps,
                    "next_actions": [
                        format!("review this plan, then run `{}`", apply_command)
                    ],
                }),
                Meta::default(),
            ));
        }

        let mut applied = Vec::new();
        let mut warnings = Vec::new();
        for agent in &args.agents {
            let agent_name = agent_kind_as_str(*agent);
            let target_path = target_path_for(&self.ctx, args, agent_name, workspace.as_deref())?;
            ensure_target_can_be_managed(
                &self.ctx,
                args,
                agent_name,
                &target_path,
                workspace.as_deref(),
            )?;
            let target_result = if args.adopt {
                self.cmd_target_adopt_managed(agent_name, &target_path, request_id)?
            } else {
                self.cmd_target(
                    &TargetCommand::Add(TargetAddArgs {
                        agent: *agent,
                        path: target_path.display().to_string(),
                        ownership: TargetOwnership::Managed,
                    }),
                    request_id,
                )?
            };
            let target_data = target_result.0;
            warnings.extend(target_result.1.warnings);
            let target_id = required_string(&target_data, &["target", "target_id"])?;
            let (matcher_kind, matcher_value) = binding_matcher(args.scope, workspace.as_deref())?;

            let binding_result = self.cmd_workspace_binding(
                &WorkspaceBindingCommand::Add(BindingAddArgs {
                    agent: *agent,
                    profile: args.profile.clone(),
                    matcher_kind,
                    matcher_value,
                    target: target_id.clone(),
                    policy_profile: "safe-capture".to_string(),
                }),
                request_id,
            )?;
            let binding_data = binding_result.0;
            warnings.extend(binding_result.1.warnings);
            let binding_id = required_string(&binding_data, &["binding", "binding_id"])?;

            let project_result = self.cmd_project(
                &ProjectArgs {
                    skill: args.skill.clone(),
                    binding: binding_id.clone(),
                    target: Some(target_id.clone()),
                    method: args.method,
                    dry_run: false,
                },
                request_id,
            )?;
            let project_data = project_result.0;
            warnings.extend(project_result.1.warnings);

            applied.push(json!({
                "agent": agent_name,
                "target": target_data["target"],
                "target_noop": target_data["noop"],
                "binding": binding_data["binding"],
                "binding_noop": binding_data["noop"],
                "projection": project_data["projection"],
                "projection_path": project_data["projection"]["materialized_path"],
                "operation_ids": {
                    "target": target_result.1.op_id,
                    "binding": binding_result.1.op_id,
                    "projection": project_result.1.op_id,
                },
                "rollback_commands": [
                    format!(
                        "loom --json --root {} workspace binding remove {} --orphan-projections",
                        shell_arg(&self.ctx.root),
                        shell_arg(&binding_id)
                    ),
                    format!(
                        "loom --json --root {} skill orphan clean --dry-run",
                        shell_arg(&self.ctx.root)
                    )
                ],
            }));
        }

        Ok((
            json!({
                "dry_run": false,
                "operation": "use",
                "skill": args.skill,
                "workspace": workspace.as_ref().map(|path| path.display().to_string()),
                "applied": applied,
                "next_actions": [
                    "restart or refresh the selected agent if it caches skill inventory",
                    "run `loom agent preflight` before follow-up writes",
                    "use the returned rollback_commands if this projection should be removed"
                ],
            }),
            Meta {
                warnings,
                ..Meta::default()
            },
        ))
    }
}

fn validate_use_args(args: &UseArgs) -> std::result::Result<(), CommandFailure> {
    if args.agents.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--agents must include at least one agent",
        ));
    }
    if args.profile.trim().is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--profile must not be empty",
        ));
    }
    Ok(())
}

fn use_workspace(args: &UseArgs) -> std::result::Result<Option<PathBuf>, CommandFailure> {
    if matches!(args.scope, UseScope::User) {
        return Ok(args.workspace.as_ref().map(|path| absolute_path(path)));
    }
    let workspace = match args.workspace.as_ref() {
        Some(path) => path.clone(),
        None => std::env::current_dir().map_err(|err| {
            CommandFailure::new(
                ErrorCode::IoError,
                format!("failed to resolve current workspace: {}", err),
            )
        })?,
    };
    Ok(Some(absolute_path(&workspace)))
}

fn target_path_for(
    ctx: &AppContext,
    args: &UseArgs,
    agent: &str,
    workspace: Option<&Path>,
) -> std::result::Result<PathBuf, CommandFailure> {
    if let Some(target_root) = args.target_root.clone() {
        return Ok(absolute_path(&target_root));
    }
    let scope = use_scope_as_str(args.scope);
    let workspace = workspace.unwrap_or(&ctx.root);
    let adapters = load_agent_adapters(ctx)?;
    if let Some(adapter) = adapters.adapter_for_agent(agent)
        && adapter.has_discovery_root_for_scope(scope)
    {
        if let Some(root) = built_in_projection_root(ctx, adapter, scope, workspace, &args.skill)? {
            return Ok(absolute_path(&root));
        }
        match preferred_discovery_root(adapter, scope, workspace) {
            Ok(root) => return Ok(absolute_path(&root.path)),
            Err(_err) if adapter.source == SOURCE_BUILT_IN => {}
            Err(err) => return Err(err),
        }
    }
    let base = ctx.root.join("targets").join(scope);
    Ok(absolute_path(&base).join(agent).join("skills"))
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn use_scope_as_str(scope: UseScope) -> &'static str {
    match scope {
        UseScope::User => "user",
        UseScope::Project => "project",
    }
}

fn binding_matcher(
    scope: UseScope,
    workspace: Option<&Path>,
) -> std::result::Result<(WorkspaceMatcherKind, String), CommandFailure> {
    match scope {
        UseScope::User => Ok((WorkspaceMatcherKind::Name, "user".to_string())),
        UseScope::Project => {
            let workspace = workspace.ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "--workspace is required when --scope project",
                )
            })?;
            Ok((
                WorkspaceMatcherKind::PathPrefix,
                workspace.display().to_string(),
            ))
        }
    }
}

fn binding_matcher_json(scope: UseScope, workspace: Option<&Path>) -> Value {
    match scope {
        UseScope::User => json!({"kind": "name", "value": "user"}),
        UseScope::Project => json!({
            "kind": "path_prefix",
            "value": workspace.map(|path| path.display().to_string())
        }),
    }
}

fn ensure_target_can_be_managed(
    ctx: &AppContext,
    args: &UseArgs,
    agent: &str,
    target_path: &Path,
    workspace: Option<&Path>,
) -> std::result::Result<(), CommandFailure> {
    if !target_path.exists() || target_is_managed(ctx, agent, target_path)? {
        return Ok(());
    }
    if args.adopt {
        return Ok(());
    }

    let mut failure = CommandFailure::new(
        ErrorCode::TargetNotManaged,
        format!(
            "target path '{}' exists but is not managed by Loom; rerun with --adopt before writing",
            target_path.display()
        ),
    );
    failure.details = json!({
        "agent": agent,
        "scope": use_scope_as_str(args.scope),
        "target_path": target_path.display().to_string(),
        "required_flag": "--adopt",
    });
    failure.next_actions = vec![NextAction {
        cmd: use_apply_command(&ctx.root, args, workspace).replace(" --apply", " --adopt --apply"),
        reason:
            "adopt the existing agent skills directory as a managed Loom target before projection"
                .to_string(),
    }];
    Err(failure)
}

fn target_is_managed(
    ctx: &AppContext,
    agent: &str,
    target_path: &Path,
) -> std::result::Result<bool, CommandFailure> {
    let Some(snapshot) = RegistryStatePaths::from_app_context(ctx)
        .maybe_load_snapshot()
        .map_err(super::helpers::map_registry_state)?
    else {
        return Ok(false);
    };
    let normalized = normalize_existing_or_raw(target_path);
    Ok(snapshot.targets.targets.iter().any(|target| {
        target.agent == agent
            && target.ownership == crate::core::vocab::Ownership::Managed
            && normalize_existing_or_raw(Path::new(&target.path)) == normalized
    }))
}

fn normalize_existing_or_raw(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn required_string(value: &Value, path: &[&str]) -> std::result::Result<String, CommandFailure> {
    let mut current = value;
    for part in path {
        current = &current[*part];
    }
    current.as_str().map(str::to_string).ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("use flow expected string field {}", path.join(".")),
        )
    })
}

fn use_apply_command(root: &Path, args: &UseArgs, workspace: Option<&Path>) -> String {
    let agents = args
        .agents
        .iter()
        .map(|agent| agent_kind_as_str(*agent))
        .collect::<Vec<_>>()
        .join(",");
    let mut command = format!(
        "loom --json --root {} use {} --agents {} --scope {} --profile {} --method {}",
        shell_arg(root),
        shell_arg(&args.skill),
        shell_arg(&agents),
        use_scope_as_str(args.scope),
        shell_arg(&args.profile),
        projection_method_as_str(args.method),
    );
    if let Some(workspace) = workspace {
        command.push_str(&format!(" --workspace {}", shell_arg(workspace)));
    }
    if let Some(target_root) = args.target_root.as_ref() {
        command.push_str(&format!(" --target-root {}", shell_arg(target_root)));
    }
    if args.adopt {
        command.push_str(" --adopt");
    }
    command.push_str(" --apply");
    command
}
