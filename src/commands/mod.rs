mod agent_cmds;
mod backup_cmds;
mod codex_cmds;
mod codex_config;
mod codex_reconcile_plan;
mod codex_visibility;
mod event_store;
mod file_ops;
mod fs_probe;
mod helpers;
mod history_cmds;
mod instruction;
mod mcp;
#[cfg(test)]
mod observed_tests;
mod org_policy;
mod package_export;
mod plan_cmds;
mod projections;
mod provenance;
#[path = "provider_cmds/model.rs"]
mod provider_cmds;
mod provision;
mod skill_activation;
mod skill_cmds;
mod skill_deps;
mod skill_diagnose;
#[cfg(test)]
mod skill_diagnose_tests;
mod skill_eval;
mod skill_eval_harness;
mod skill_inspect;
mod skill_inventory;
mod skill_lint;
mod skill_new;
mod skill_policy;
mod skill_preflight;
mod skill_recommend;
mod skill_recommend_active;
mod skill_safety;
mod skill_safety_findings;
mod skill_verify;
mod skillset_cmds;
mod sync_cmds;
mod target_cmds;
mod trash_cmds;
mod use_cmds;
mod version_cmds;
mod watch_cmds;
mod workflow_cmds;
mod workspace_cmds;

use anyhow::Result;
use serde_json::json;
use uuid::Uuid;

use crate::cli::{
    AgentCommand, ApprovalCommand, Cli, CodexCommand, Command, McpCommand, OpsCommand,
    OpsHistoryCommand, OrgPolicyCommand, PackageCommand, PolicyCommand, ProvisionCommand,
    RemoteCommand, RolesCommand, SkillActiveCommand, SkillCommand, SkillOrphanCommand,
    SkillProvenanceCommand, SkillTrashCommand, SkillsetCommand, SyncCommand, TargetCommand,
    WorkflowCommand, WorkspaceBindingCommand, WorkspaceCommand, WorkspaceInitArgs,
};
use crate::envelope::{Envelope, Meta};
use crate::state::{AppContext, home_dir};
use crate::types::ErrorCode;

pub(crate) use event_store::redact_sensitive_string;
pub use projections::collect_skill_inventory;
pub(crate) use skill_inventory::build_skill_read_model;
pub(crate) use skill_lint::{
    SkillLintMode, SkillLintReport, lint_skill_source, lint_skill_source_for_agent,
};

use event_store::{
    append_command_audit_failure, append_command_finished, append_command_started,
    command_event_input, prepare_command_event_store,
};
use helpers::{command_name, ensure_initial_commit, map_git, map_io};

use crate::gitops;
use crate::state_model::RegistryStatePaths;

#[derive(Debug)]
pub struct CommandFailure {
    pub code: ErrorCode,
    pub message: String,
    pub details: serde_json::Value,
}

impl CommandFailure {
    pub(crate) fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: json!({}),
        }
    }

    pub(crate) fn with_rollback_errors(mut self, rollback_errors: Vec<serde_json::Value>) -> Self {
        if rollback_errors.is_empty() {
            return self;
        }
        let original_details = std::mem::replace(&mut self.details, json!({}));
        self.details = json!({
            "original_error": {
                "code": self.code.as_str(),
                "message": self.message.clone(),
            },
            "original_details": original_details,
            "rollback_errors": rollback_errors,
        });
        self
    }
}

pub struct App {
    pub ctx: AppContext,
}

impl App {
    pub fn new(root: Option<std::path::PathBuf>) -> Result<Self> {
        let ctx = AppContext::new(root)?;
        Ok(Self { ctx })
    }

    pub(crate) fn ensure_write_layout(&self) -> std::result::Result<(), CommandFailure> {
        self.ctx.ensure_not_loom_tool_repo_root().map_err(map_io)?;
        self.ctx.ensure_state_layout().map_err(map_io)?;
        Ok(())
    }

    pub(crate) fn ensure_write_repo_ready(&self) -> std::result::Result<(), CommandFailure> {
        self.ensure_write_layout()?;
        gitops::ensure_repo_initialized(&self.ctx).map_err(map_git)?;
        self.ctx.ensure_gitignore_entries().map_err(map_io)?;
        ensure_initial_commit(&self.ctx).map_err(map_git)?;
        Ok(())
    }

    pub fn execute(&self, cli: Cli) -> Result<(Envelope, i32)> {
        let cmd = command_name(&cli.command);
        let request_id = cli
            .request_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let audit_required = command_requires_durable_audit(&cli.command);
        let audit_enabled = command_records_audit(&cli.command)
            && (audit_required || self.ctx.ensure_not_loom_tool_repo_root().is_ok());
        if audit_required && let Err(err) = self.ctx.ensure_not_loom_tool_repo_root() {
            let message = err.to_string();
            let message = message
                .strip_prefix("ARG_INVALID:")
                .map(str::trim)
                .unwrap_or(&message);
            let env = Envelope::err(cmd, request_id, ErrorCode::ArgInvalid, message, json!({}));
            return Ok((env, ErrorCode::ArgInvalid.exit_code()));
        }
        let mut audit_event_id = None;
        let mut audit_warnings = Vec::new();
        if audit_enabled {
            let input = command_event_input(&cli, &request_id);
            match prepare_command_event_store(&self.ctx) {
                Ok(()) => match append_command_started(&self.ctx, cmd, input, &request_id) {
                    Ok(event_id) => audit_event_id = Some(event_id),
                    Err(err) => {
                        let warning = format!("failed to append command event: {}", err);
                        if audit_required {
                            let env = Envelope::err(
                                cmd,
                                request_id,
                                ErrorCode::AuditError,
                                warning,
                                json!({}),
                            );
                            return Ok((env, ErrorCode::AuditError.exit_code()));
                        }
                        audit_warnings.push(warning);
                    }
                },
                Err(err) => {
                    let warning = format!("failed to prepare command event log: {}", err);
                    if audit_required {
                        let env = Envelope::err(
                            cmd,
                            request_id,
                            ErrorCode::AuditError,
                            warning,
                            json!({}),
                        );
                        return Ok((env, ErrorCode::AuditError.exit_code()));
                    }
                    audit_warnings.push(warning);
                }
            }
        }

        let result = match &cli.command {
            Command::Init => {
                let args = WorkspaceInitArgs {
                    scan_existing: home_dir().is_some(),
                };
                self.cmd_workspace_init(&args, &request_id)
            }
            Command::Backup { command } => self.cmd_backup(command),
            Command::Monitor(args) => self.cmd_monitor_observed(args, &request_id),
            Command::Use(args) => self.cmd_use(args, &request_id),
            Command::Plan { command } => self.cmd_plan(command),
            Command::Apply(args) => self.cmd_apply(args, &request_id),
            Command::Doctor => self.cmd_doctor(),
            Command::Workspace { command } => match command {
                WorkspaceCommand::Status => self.cmd_status(),
                WorkspaceCommand::Doctor => self.cmd_doctor(),
                WorkspaceCommand::Init(args) => self.cmd_workspace_init(args, &request_id),
                WorkspaceCommand::Binding { command } => {
                    self.cmd_workspace_binding(command, &request_id)
                }
                WorkspaceCommand::Remote { command } => self.cmd_remote(command),
            },
            Command::Target { command } => self.cmd_target(command, &request_id),
            Command::Skill { command } => match command {
                SkillCommand::Add(args) => self.cmd_add(args, &request_id),
                SkillCommand::Install(args) => self.cmd_skill_install(args, &request_id),
                SkillCommand::ImportObserved(args) => self.cmd_import_observed(args, &request_id),
                SkillCommand::MonitorObserved(args) => self.cmd_monitor_observed(args, &request_id),
                SkillCommand::Project(args) if args.dry_run => self.cmd_project_plan(args),
                SkillCommand::Project(args) => self.cmd_project(args, &request_id),
                SkillCommand::Capture(args) if args.dry_run => self.cmd_capture_plan(args),
                SkillCommand::Capture(args) => self.cmd_capture(args, &request_id),
                SkillCommand::Improve(args) => self.cmd_skill_improve(args),
                SkillCommand::Regression(args) => self.cmd_skill_regression(args),
                SkillCommand::Save(args) => self.cmd_save(args, &request_id),
                SkillCommand::Watch(args) => self.cmd_watch(args, &request_id),
                SkillCommand::Snapshot(args) => self.cmd_snapshot(args, &request_id),
                SkillCommand::Release(args) => self.cmd_release(args, &request_id),
                SkillCommand::Rollback(args) if args.dry_run => self.cmd_rollback_plan(args),
                SkillCommand::Rollback(args) => self.cmd_rollback(args, &request_id),
                SkillCommand::Diff(args) => self.cmd_diff(args),
                SkillCommand::History(args) => self.cmd_history(args),
                SkillCommand::Trash {
                    command: SkillTrashCommand::Add(args),
                } if args.dry_run => self.cmd_skill_trash_add_plan(args),
                SkillCommand::Trash {
                    command: SkillTrashCommand::Add(args),
                } => self.cmd_skill_trash_add(args, &request_id),
                SkillCommand::Trash {
                    command: SkillTrashCommand::List,
                } => self.cmd_skill_trash_list(),
                SkillCommand::Trash {
                    command: SkillTrashCommand::Restore(args),
                } => self.cmd_skill_trash_restore(args, &request_id),
                SkillCommand::Trash {
                    command: SkillTrashCommand::Purge(args),
                } if args.dry_run => self.cmd_skill_trash_purge_plan(args),
                SkillCommand::Trash {
                    command: SkillTrashCommand::Purge(args),
                } => self.cmd_skill_trash_purge(args, &request_id),
                SkillCommand::List => self.cmd_skill_list(),
                SkillCommand::Show(args) => self.cmd_skill_show(args),
                SkillCommand::Inspect(args) => self.cmd_skill_inspect(args),
                SkillCommand::Deps(args) => self.cmd_skill_deps(args),
                SkillCommand::Activate(args) => self.cmd_skill_activate(args, &request_id),
                SkillCommand::Deactivate(args) => self.cmd_skill_deactivate(args, &request_id),
                SkillCommand::Active {
                    command: SkillActiveCommand::List(args),
                } => self.cmd_skill_active_list(args),
                SkillCommand::Search(args) => self.cmd_skill_search(args),
                SkillCommand::Recommend(args) => self.cmd_skill_recommend(args),
                SkillCommand::Resolve(args) if args.semantic => {
                    self.cmd_skill_resolve_semantic(args)
                }
                SkillCommand::Resolve(args) => self.cmd_skill_resolve(args),
                SkillCommand::New(args) => self.cmd_skill_new(args, &request_id),
                SkillCommand::Provenance { command } => {
                    self.cmd_skill_provenance(command, &request_id)
                }
                SkillCommand::Verify(args) => self.cmd_verify(args),
                SkillCommand::Lint(args) => self.cmd_skill_lint(args),
                SkillCommand::Policy(args) => self.cmd_skill_policy(args),
                SkillCommand::Scan(args) => self.cmd_skill_scan(args),
                SkillCommand::Trust(args) => self.cmd_skill_trust(args, &request_id),
                SkillCommand::Quarantine(args) => self.cmd_skill_quarantine(args, &request_id),
                SkillCommand::Unquarantine(args) => {
                    self.cmd_skill_unquarantine(&args.skill, &request_id)
                }
                SkillCommand::Visibility(args) => self.cmd_skill_visibility(args),
                SkillCommand::Diagnose(args) => self.cmd_skill_diagnose(args),
                SkillCommand::Eval(args) => self.cmd_skill_eval(args),
                SkillCommand::Orphan {
                    command: SkillOrphanCommand::List,
                } => self.cmd_skill_orphan_list(),
                SkillCommand::Orphan {
                    command: SkillOrphanCommand::Clean(args),
                } if args.dry_run => self.cmd_skill_orphan_clean_plan(args),
                SkillCommand::Orphan {
                    command: SkillOrphanCommand::Clean(args),
                } => self.cmd_skill_orphan_clean(args, &request_id),
            },
            Command::Skillset { command } => match command {
                SkillsetCommand::Create(args) => self.cmd_skillset_create(args),
                SkillsetCommand::Add(args) => self.cmd_skillset_add(args),
                SkillsetCommand::Remove(args) => self.cmd_skillset_remove(args),
                SkillsetCommand::Show(args) => self.cmd_skillset_show(args),
                SkillsetCommand::Lint(args) => self.cmd_skillset_lint(args),
            },
            Command::Provider { command } => self.cmd_provider(command, &request_id),
            Command::Catalog { command } => self.cmd_catalog(command),
            Command::Package { command } => self.cmd_package(command),
            Command::Mcp { command } => self.cmd_mcp(command),
            Command::Provision { command } => self.cmd_provision(command),
            Command::Policy { command } => match command {
                PolicyCommand::Org { command } => self.cmd_policy_org(command, &request_id),
            },
            Command::Approval { command } => self.cmd_approval(command, &request_id),
            Command::Roles { command } => self.cmd_roles(command, &request_id),
            Command::Instruction { command } => self.cmd_instruction(command),
            Command::Workflow { command } => self.cmd_workflow(command),
            Command::Index(args) if args.action == "build" => self.cmd_index_build(args),
            Command::Index(args) if args.action == "status" => self.cmd_index_status(),
            Command::Index(args) => Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "unknown index action '{}'; expected build or status",
                    args.action
                ),
            )),
            Command::Active(args) if args.action == "recommend" => self.cmd_active_recommend(args),
            Command::Active(args) => Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "unknown active action '{}'; expected recommend",
                    args.action
                ),
            )),
            Command::Sync { command } => self.cmd_sync(command),
            Command::Ops { command } => self.cmd_ops(command),
            Command::Agent { command } => match command {
                AgentCommand::Preflight(args) => self.cmd_agent_preflight(args),
            },
            Command::Codex { command } => match command {
                CodexCommand::Reconcile(args) => self.cmd_codex_reconcile(args, &request_id),
            },
            Command::Panel(_) => Ok((json!({"message": "panel handled in main"}), Meta::default())),
        };

        match result {
            Ok((data, meta)) => {
                let mut env = Envelope::ok(cmd, request_id, data, meta);
                env.meta.warnings.extend(audit_warnings);
                Ok(
                    self.finish_command_audit(
                        cmd,
                        env,
                        0,
                        audit_event_id.is_some(),
                        audit_required,
                    ),
                )
            }
            Err(f) => {
                let exit_code = f.code.exit_code();
                let mut env = Envelope::err(cmd, request_id, f.code, f.message, f.details);
                env.meta.warnings.extend(audit_warnings);
                Ok(self.finish_command_audit(
                    cmd,
                    env,
                    exit_code,
                    audit_event_id.is_some(),
                    audit_required,
                ))
            }
        }
    }

    fn finish_command_audit(
        &self,
        cmd: &str,
        mut env: Envelope,
        exit_code: i32,
        audit_started: bool,
        audit_required: bool,
    ) -> (Envelope, i32) {
        if !audit_started {
            return (env, exit_code);
        }

        if let Err(err) = append_command_finished(&self.ctx, cmd, &env, exit_code) {
            let warning = format!("failed to append command event: {}", err);
            if !audit_required {
                env.meta.warnings.push(warning);
                return (env, exit_code);
            }

            let failure_exit = ErrorCode::AuditError.exit_code();
            let mut failure_env = Envelope::err(
                cmd,
                env.request_id.clone(),
                ErrorCode::AuditError,
                warning,
                json!({
                    "audit_stage": "finish",
                    "original_ok": env.ok,
                    "original_exit_code": exit_code,
                    "original_error": env.error.as_ref().map(|error| {
                        json!({
                            "code": error.code,
                            "message": error.message,
                        })
                    }),
                }),
            );
            failure_env.meta.warnings = env.meta.warnings;
            if let Err(recovery_err) =
                append_command_audit_failure(&self.ctx, cmd, &failure_env, failure_exit)
            {
                failure_env.meta.warnings.push(format!(
                    "failed to append audit failure event after terminal append failure: {}",
                    recovery_err
                ));
            }
            return (failure_env, failure_exit);
        }

        (env, exit_code)
    }

    pub(crate) fn require_registry_snapshot(
        &self,
    ) -> std::result::Result<crate::state_model::RegistrySnapshot, CommandFailure> {
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        match paths
            .maybe_load_snapshot()
            .map_err(helpers::map_registry_state)?
        {
            Some(snapshot) => Ok(snapshot),
            None => Err(CommandFailure::new(
                ErrorCode::StateNotInitialized,
                format!(
                    "registry state not initialized under {}",
                    paths.registry_dir.display()
                ),
            )),
        }
    }

    pub(crate) fn ensure_registry_layout(
        &self,
    ) -> std::result::Result<RegistryStatePaths, CommandFailure> {
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        paths.ensure_layout().map_err(helpers::map_registry_state)?;
        Ok(paths)
    }
}

fn command_records_audit(command: &Command) -> bool {
    if let Command::Skill {
        command: SkillCommand::New(args),
    } = command
    {
        return !args.dry_run;
    }
    if let Command::Skill {
        command: SkillCommand::Activate(args),
    } = command
    {
        return !args.dry_run;
    }
    if let Command::Skill {
        command: SkillCommand::Deactivate(args),
    } = command
    {
        return !args.dry_run;
    }
    if let Command::Codex {
        command: CodexCommand::Reconcile(args),
    } = command
    {
        return args.apply;
    }

    !matches!(
        command,
        Command::Panel(_)
            | Command::Backup { .. }
            | Command::Index(_)
            | Command::Active(_)
            | Command::Catalog { .. }
            | Command::Mcp { .. }
            | Command::Provision {
                command: ProvisionCommand::Plan(_)
                    | ProvisionCommand::Doctor(_)
                    | ProvisionCommand::Export(_)
                    | ProvisionCommand::Import(_),
            }
            | Command::Package {
                command: PackageCommand::Plan(_) | PackageCommand::Verify(_),
            }
            | Command::Policy {
                command: PolicyCommand::Org {
                    command: OrgPolicyCommand::Show | OrgPolicyCommand::Check(_),
                },
            }
            | Command::Approval {
                command: ApprovalCommand::List(_),
            }
            | Command::Roles {
                command: RolesCommand::List,
            }
            | Command::Instruction { .. }
            | Command::Provider {
                command: crate::cli::ProviderCommand::List,
            }
            | Command::Skill {
                command: SkillCommand::History(_)
                    | SkillCommand::List
                    | SkillCommand::Show(_)
                    | SkillCommand::Inspect(_)
                    | SkillCommand::Deps(_)
                    | SkillCommand::Improve(_)
                    | SkillCommand::Regression(_)
                    | SkillCommand::Active { .. }
                    | SkillCommand::Search(_)
                    | SkillCommand::Recommend(_)
                    | SkillCommand::Resolve(_)
                    | SkillCommand::Lint(_)
                    | SkillCommand::Visibility(_)
                    | SkillCommand::Diagnose(_)
                    | SkillCommand::Eval(_)
                    | SkillCommand::Trash {
                        command: SkillTrashCommand::List,
                    }
            }
            | Command::Skillset {
                command: SkillsetCommand::Show(_) | SkillsetCommand::Lint(_),
            }
            | Command::Workflow {
                command: WorkflowCommand::Show(_)
                    | WorkflowCommand::Preflight(_)
                    | WorkflowCommand::Run(_),
            }
    ) && !is_rollback_preview(command)
}

fn command_requires_durable_audit(command: &Command) -> bool {
    match command {
        Command::Init | Command::Monitor(_) => true,
        Command::Use(args) => args.apply,
        Command::Plan { .. } | Command::Apply(_) => true,
        Command::Backup { .. } => false,
        Command::Doctor => false,
        Command::Workspace { command } => match command {
            WorkspaceCommand::Status | WorkspaceCommand::Doctor => false,
            WorkspaceCommand::Init(_) => true,
            WorkspaceCommand::Binding { command } => !matches!(
                command,
                WorkspaceBindingCommand::List | WorkspaceBindingCommand::Show(_)
            ),
            WorkspaceCommand::Remote { command } => !matches!(command, RemoteCommand::Status),
        },
        Command::Target { command } => {
            !matches!(command, TargetCommand::List | TargetCommand::Show(_))
        }
        Command::Skill { command } => match command {
            SkillCommand::Add(_)
            | SkillCommand::ImportObserved(_)
            | SkillCommand::MonitorObserved(_)
            | SkillCommand::Project(_)
            | SkillCommand::Capture(_)
            | SkillCommand::Save(_)
            | SkillCommand::Watch(_)
            | SkillCommand::Snapshot(_)
            | SkillCommand::Release(_)
            | SkillCommand::Trust(_)
            | SkillCommand::Quarantine(_)
            | SkillCommand::Unquarantine(_)
            | SkillCommand::Provenance {
                command: SkillProvenanceCommand::Refresh(_),
            }
            | SkillCommand::Trash {
                command:
                    SkillTrashCommand::Add(_)
                    | SkillTrashCommand::Restore(_)
                    | SkillTrashCommand::Purge(_),
            }
            | SkillCommand::Orphan {
                command: SkillOrphanCommand::Clean(_),
            } => true,
            SkillCommand::Install(args) => !args.dry_run,
            SkillCommand::Rollback(args) => !args.dry_run,
            SkillCommand::New(args) => !args.dry_run,
            SkillCommand::Activate(args) => !args.dry_run,
            SkillCommand::Deactivate(args) => !args.dry_run,
            SkillCommand::Diff(_)
            | SkillCommand::History(_)
            | SkillCommand::List
            | SkillCommand::Show(_)
            | SkillCommand::Inspect(_)
            | SkillCommand::Deps(_)
            | SkillCommand::Improve(_)
            | SkillCommand::Regression(_)
            | SkillCommand::Active { .. }
            | SkillCommand::Search(_)
            | SkillCommand::Recommend(_)
            | SkillCommand::Resolve(_)
            | SkillCommand::Lint(_)
            | SkillCommand::Policy(_)
            | SkillCommand::Scan(_)
            | SkillCommand::Verify(_)
            | SkillCommand::Visibility(_)
            | SkillCommand::Diagnose(_)
            | SkillCommand::Eval(_)
            | SkillCommand::Provenance {
                command: SkillProvenanceCommand::Inspect(_) | SkillProvenanceCommand::Verify(_),
            }
            | SkillCommand::Trash {
                command: SkillTrashCommand::List,
            }
            | SkillCommand::Orphan {
                command: SkillOrphanCommand::List,
            } => false,
        },
        Command::Skillset { command } => match command {
            SkillsetCommand::Create(_) | SkillsetCommand::Add(_) | SkillsetCommand::Remove(_) => {
                true
            }
            SkillsetCommand::Show(_) | SkillsetCommand::Lint(_) => false,
        },
        Command::Provider { command } => !matches!(command, crate::cli::ProviderCommand::List),
        Command::Catalog { .. } => false,
        Command::Package { command } => match command {
            PackageCommand::Build(_) => false,
            PackageCommand::Plan(_) | PackageCommand::Verify(_) => false,
        },
        Command::Mcp { command } => match command {
            McpCommand::Requirement { .. }
            | McpCommand::Plan(_)
            | McpCommand::Doctor(_)
            | McpCommand::Catalog { .. } => false,
        },
        Command::Provision { command } => match command {
            ProvisionCommand::Plan(_)
            | ProvisionCommand::Doctor(_)
            | ProvisionCommand::Export(_)
            | ProvisionCommand::Import(_) => false,
            ProvisionCommand::Apply(_) => true,
        },
        Command::Policy { command } => match command {
            PolicyCommand::Org { command } => matches!(command, OrgPolicyCommand::Init(_)),
        },
        Command::Approval { command } => match command {
            ApprovalCommand::Request(_)
            | ApprovalCommand::Approve(_)
            | ApprovalCommand::Reject(_) => true,
            ApprovalCommand::List(_) => false,
        },
        Command::Roles { command } => match command {
            RolesCommand::Grant(_) | RolesCommand::Revoke(_) => true,
            RolesCommand::List => false,
        },
        Command::Instruction { .. } => false,
        Command::Workflow { command } => match command {
            WorkflowCommand::Create(args) => !args.dry_run,
            WorkflowCommand::Plan(_) => true,
            WorkflowCommand::Show(_) | WorkflowCommand::Preflight(_) | WorkflowCommand::Run(_) => {
                false
            }
        },
        Command::Index(_) | Command::Active(_) => false,
        Command::Sync { command } => !matches!(command, SyncCommand::Status),
        Command::Ops { command } => match command {
            OpsCommand::List => false,
            OpsCommand::Retry | OpsCommand::Purge => true,
            OpsCommand::History { command } => !matches!(command, OpsHistoryCommand::Diagnose),
        },
        Command::Agent { .. } => false,
        Command::Codex { command } => match command {
            CodexCommand::Reconcile(args) => args.apply,
        },
        Command::Panel(_) => false,
    }
}

fn is_rollback_preview(command: &Command) -> bool {
    matches!(
        command,
        Command::Skill {
            command: SkillCommand::Rollback(args),
        } if args.dry_run
    )
}
