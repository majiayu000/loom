use crate::cli::{
    Command, OpsCommand, OpsHistoryCommand, ProviderCommand, RemoteCommand, SkillAuthorCommand,
    SkillCommand, SkillOrphanCommand, SkillProvenanceCommand, SkillTrashCommand, SkillsetCommand,
    SyncCommand, TargetCommand, WorkspaceBindingCommand, WorkspaceCommand,
};
use crate::gitops;
use crate::state::AppContext;

use super::super::CommandFailure;
use super::require_policy_checks;
use super::state::PolicyCheck;

pub(crate) fn require_command_policy(
    ctx: &AppContext,
    command: &Command,
) -> std::result::Result<(), CommandFailure> {
    let checks = governed_checks(ctx, command)?;
    require_policy_checks(ctx, &checks)
}

pub(crate) fn require_action_policy(
    ctx: &AppContext,
    check: PolicyCheck,
) -> std::result::Result<(), CommandFailure> {
    require_policy_checks(ctx, std::slice::from_ref(&check))
}

fn governed_checks(
    ctx: &AppContext,
    command: &Command,
) -> std::result::Result<Vec<PolicyCheck>, CommandFailure> {
    Ok(match command {
        Command::Monitor(_) => vec![PolicyCheck::new("skill.monitor_observed")],
        Command::Use(args) if args.apply => vec![
            PolicyCheck::new("skill.activate").skill(&args.skill),
            PolicyCheck::new("skill.project").skill(&args.skill),
            PolicyCheck::new("target.add"),
            PolicyCheck::new("workspace.binding.add"),
        ],
        Command::Workspace { command } => match command {
            WorkspaceCommand::Binding { command } => match command {
                WorkspaceBindingCommand::Add(args) => {
                    vec![PolicyCheck::new("workspace.binding.add").agent(args.agent.as_str())]
                }
                WorkspaceBindingCommand::Remove(args) => {
                    vec![PolicyCheck::new("workspace.binding.remove").binding_id(&args.binding_id)]
                }
                _ => Vec::new(),
            },
            WorkspaceCommand::Remote {
                command: RemoteCommand::Set { .. },
            } => vec![PolicyCheck::new("workspace.remote.set")],
            _ => Vec::new(),
        },
        Command::Target { command } => match command {
            TargetCommand::Add(args) => {
                vec![PolicyCheck::new("target.add").agent(args.agent.as_str())]
            }
            TargetCommand::Remove(args) => {
                vec![PolicyCheck::new("target.remove").target_id(&args.target_id)]
            }
            _ => Vec::new(),
        },
        Command::Skill { command } => skill_checks(command)?,
        Command::Skillset { command } => skillset_checks(command),
        Command::Provider { command } => match command {
            ProviderCommand::Add(args) => {
                vec![PolicyCheck::new("provider.add").provider(&args.id)]
            }
            ProviderCommand::Remove(args) => {
                vec![PolicyCheck::new("provider.remove").provider(&args.id)]
            }
            _ => Vec::new(),
        },
        Command::Sync { command } => sync_checks(ctx, command)?,
        Command::Ops { command } => match command {
            OpsCommand::Retry => vec![PolicyCheck::new("ops.retry")],
            OpsCommand::Purge => vec![PolicyCheck::new("ops.purge")],
            OpsCommand::History {
                command: OpsHistoryCommand::Repair(_),
            } => vec![PolicyCheck::new("ops.history.repair")],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    })
}

fn skill_checks(command: &SkillCommand) -> std::result::Result<Vec<PolicyCheck>, CommandFailure> {
    Ok(match command {
        SkillCommand::Author {
            command: SkillAuthorCommand::New(args),
        } if !args.dry_run => vec![PolicyCheck::new("skill.author.new").skill(&args.name)],
        SkillCommand::Add(args) => vec![PolicyCheck::new("skill.add").skill(&args.name)],
        SkillCommand::Install(args) if !args.dry_run => {
            vec![PolicyCheck::new("skill.install").skill(&args.name)]
        }
        SkillCommand::ImportObserved(_) => vec![PolicyCheck::new("skill.import_observed")],
        SkillCommand::MonitorObserved(_) => vec![PolicyCheck::new("skill.monitor_observed")],
        SkillCommand::Project(args) if !args.dry_run => {
            vec![PolicyCheck::new("skill.project").skill(&args.skill)]
        }
        SkillCommand::Commit(args) => {
            let action = if args.from_projection {
                "skill.capture"
            } else {
                "skill.save"
            };
            vec![PolicyCheck::new(action).skill(&args.skill)]
        }
        SkillCommand::Watch(args) if !args.dry_run => {
            let mut check = PolicyCheck::new("skill.watch");
            if let Some(skill) = args.skill.as_deref() {
                check = check.skill(skill);
            }
            vec![check]
        }
        SkillCommand::Activate(args) if !args.dry_run => {
            vec![
                PolicyCheck::new("skill.activate")
                    .skill(&args.skill)
                    .agent(&args.agent),
            ]
        }
        SkillCommand::Deactivate(args) if !args.dry_run => {
            vec![
                PolicyCheck::new("skill.deactivate")
                    .skill(&args.skill)
                    .agent(&args.agent),
            ]
        }
        SkillCommand::Release(args) => {
            let action = if args.anchor || args.version.is_none() {
                "skill.snapshot"
            } else {
                "skill.release"
            };
            vec![PolicyCheck::new(action).skill(&args.skill)]
        }
        SkillCommand::Rollback(args) if !args.dry_run => {
            vec![PolicyCheck::new("skill.rollback").skill(&args.skill)]
        }
        SkillCommand::Trust(args) => {
            vec![PolicyCheck::new("skill.trust.update").skill(&args.skill)]
        }
        SkillCommand::Quarantine(args) => {
            vec![PolicyCheck::new("skill.quarantine").skill(&args.skill)]
        }
        SkillCommand::Unquarantine(args) => {
            vec![PolicyCheck::new("skill.quarantine").skill(&args.skill)]
        }
        SkillCommand::Provenance {
            command: SkillProvenanceCommand::Refresh(args),
        } => vec![PolicyCheck::new("skill.provenance.refresh").skill(&args.skill)],
        SkillCommand::Trash { command } => match command {
            SkillTrashCommand::Add(args) if !args.dry_run => {
                vec![PolicyCheck::new("skill.trash.add").skill(&args.skill)]
            }
            SkillTrashCommand::Restore(args) => {
                vec![PolicyCheck::new("skill.trash.restore").skill(&args.skill)]
            }
            SkillTrashCommand::Purge(args) if !args.dry_run => {
                vec![PolicyCheck::new("skill.trash.purge").trash_id(&args.trash_id)]
            }
            _ => Vec::new(),
        },
        SkillCommand::Orphan {
            command: SkillOrphanCommand::Clean(args),
        } if !args.dry_run => vec![PolicyCheck::new("skill.orphan.clean")],
        _ => Vec::new(),
    })
}

fn skillset_checks(command: &SkillsetCommand) -> Vec<PolicyCheck> {
    match command {
        SkillsetCommand::Activate(args) if !args.dry_run => vec![
            PolicyCheck::new("skillset.activate")
                .skillset(&args.name)
                .agent(&args.agent),
        ],
        SkillsetCommand::Deactivate(args) if !args.dry_run => vec![
            PolicyCheck::new("skillset.deactivate")
                .skillset(&args.name)
                .agent(&args.agent),
        ],
        SkillsetCommand::Release(args) => {
            vec![PolicyCheck::new("skillset.release").skillset(&args.name)]
        }
        SkillsetCommand::Rollback(args) => {
            vec![PolicyCheck::new("skillset.rollback").skillset(&args.name)]
        }
        _ => Vec::new(),
    }
}

fn sync_checks(
    ctx: &AppContext,
    command: &SyncCommand,
) -> std::result::Result<Vec<PolicyCheck>, CommandFailure> {
    let action = match command {
        SyncCommand::Push(args) if !args.dry_run => "sync.push",
        SyncCommand::Pull => "sync.pull",
        SyncCommand::Replay => "sync.replay",
        _ => return Ok(Vec::new()),
    };
    let remote = gitops::remote_url(ctx)
        .ok()
        .flatten()
        .map(|url| super::sync_remote_identity(&url))
        .unwrap_or_else(|| "origin".to_string());
    Ok(vec![PolicyCheck::new(action).sync_remote(remote)])
}
