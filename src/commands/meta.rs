use crate::cli::{
    ApprovalCommand, CodexCommand, Command, McpCommand, OpsCommand, OpsHistoryCommand,
    OrgPolicyCommand, PackageCommand, PolicyCommand, ProvisionCommand, RemoteCommand, RolesCommand,
    SkillCommand, SkillOrphanCommand, SkillProvenanceCommand, SkillTrashCommand, SkillsetCommand,
    SyncCommand, TargetCommand, TelemetryCommand, WorkflowCommand, WorkspaceBindingCommand,
    WorkspaceCommand,
};

#[derive(Clone, Copy, Eq, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct CommandMeta {
    bits: u8,
}

impl CommandMeta {
    const RECORDS_AUDIT: u8 = 0b001;
    const DURABLE_AUDIT: u8 = 0b010;
    const PREVIEW: u8 = 0b100;

    const fn new(records_audit: bool, durable_audit: bool, is_preview: bool) -> Self {
        let mut bits = 0;
        if records_audit {
            bits |= Self::RECORDS_AUDIT;
        }
        if durable_audit {
            bits |= Self::DURABLE_AUDIT;
        }
        if is_preview {
            bits |= Self::PREVIEW;
        }
        Self { bits }
    }

    pub(crate) const fn records_audit(self) -> bool {
        self.bits & Self::RECORDS_AUDIT != 0
    }

    pub(crate) const fn durable_audit(self) -> bool {
        self.bits & Self::DURABLE_AUDIT != 0
    }
}

pub(crate) fn command_meta(command: &Command) -> CommandMeta {
    match command {
        Command::Init | Command::Monitor(_) => CommandMeta::new(true, true, false),
        Command::Use(args) => CommandMeta::new(true, args.apply, !args.apply),
        Command::Plan { .. } | Command::Apply(_) => CommandMeta::new(true, true, false),
        Command::Backup { .. } => CommandMeta::new(false, false, false),
        Command::Workspace { command } => match command {
            WorkspaceCommand::Status | WorkspaceCommand::Doctor => {
                CommandMeta::new(true, false, false)
            }
            WorkspaceCommand::Init(_) => CommandMeta::new(true, true, false),
            WorkspaceCommand::Binding { command } => {
                let durable = !matches!(
                    command,
                    WorkspaceBindingCommand::List | WorkspaceBindingCommand::Show(_)
                );
                CommandMeta::new(true, durable, !durable)
            }
            WorkspaceCommand::Remote { command } => {
                let durable = !matches!(command, RemoteCommand::Status);
                CommandMeta::new(true, durable, !durable)
            }
        },
        Command::Target { command } => {
            let durable = !matches!(command, TargetCommand::List | TargetCommand::Show(_));
            CommandMeta::new(true, durable, !durable)
        }
        Command::Skill { command } => match command {
            SkillCommand::Add(_)
            | SkillCommand::ImportObserved(_)
            | SkillCommand::MonitorObserved(_)
            | SkillCommand::Project(_)
            | SkillCommand::Commit(_)
            | SkillCommand::Watch(_)
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
            } => CommandMeta::new(true, true, false),
            SkillCommand::Install(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillCommand::New(args) => CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run),
            SkillCommand::Draft(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillCommand::Extract(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillCommand::Rewrite(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillCommand::TuneDescription(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillCommand::GenerateEvals(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillCommand::Activate(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillCommand::Deactivate(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillCommand::Rollback(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillCommand::ApplyPatch(_) => CommandMeta::new(true, true, false),
            SkillCommand::Diff(_)
            | SkillCommand::Policy(_)
            | SkillCommand::Scan(_)
            | SkillCommand::Provenance {
                command:
                    SkillProvenanceCommand::Inspect(_)
                    | SkillProvenanceCommand::Verify(_)
                    | SkillProvenanceCommand::Outdated(_),
            }
            | SkillCommand::Orphan {
                command: SkillOrphanCommand::List,
            } => CommandMeta::new(true, false, false),
            SkillCommand::History(_)
            | SkillCommand::List
            | SkillCommand::Inspect(_)
            | SkillCommand::Deps(_)
            | SkillCommand::Compile(_)
            | SkillCommand::Improve(_)
            | SkillCommand::Regression(_)
            | SkillCommand::Active { .. }
            | SkillCommand::Search(_)
            | SkillCommand::Recommend(_)
            | SkillCommand::Resolve(_)
            | SkillCommand::Used(_)
            | SkillCommand::Feedback(_)
            | SkillCommand::Lint(_)
            | SkillCommand::Visibility(_)
            | SkillCommand::Diagnose(_)
            | SkillCommand::Eval(_)
            | SkillCommand::Trash {
                command: SkillTrashCommand::List,
            } => CommandMeta::new(false, false, false),
        },
        Command::Skillset { command } => match command {
            SkillsetCommand::Create(_) | SkillsetCommand::Add(_) | SkillsetCommand::Remove(_) => {
                CommandMeta::new(true, true, false)
            }
            SkillsetCommand::Activate(args) | SkillsetCommand::Deactivate(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            SkillsetCommand::Release(_) | SkillsetCommand::Rollback(_) => {
                CommandMeta::new(true, true, false)
            }
            SkillsetCommand::Show(_) | SkillsetCommand::Lint(_) | SkillsetCommand::Eval(_) => {
                CommandMeta::new(false, false, false)
            }
        },
        Command::Telemetry { command } => match command {
            TelemetryCommand::Enable(_) | TelemetryCommand::Disable => {
                CommandMeta::new(true, true, false)
            }
            TelemetryCommand::Purge(args) => CommandMeta::new(
                args.confirm.is_some(),
                args.confirm.is_some(),
                args.confirm.is_none(),
            ),
            TelemetryCommand::Status
            | TelemetryCommand::Report(_)
            | TelemetryCommand::Export(_) => CommandMeta::new(false, false, false),
        },
        Command::Provider { command } => {
            let durable = !matches!(command, crate::cli::ProviderCommand::List);
            CommandMeta::new(durable, durable, !durable)
        }
        Command::Catalog { .. } => CommandMeta::new(false, false, false),
        Command::Package { command } => match command {
            PackageCommand::Build(_) => CommandMeta::new(true, false, false),
            PackageCommand::Plan(_) | PackageCommand::Verify(_) => {
                CommandMeta::new(false, false, true)
            }
        },
        Command::Mcp { command } => match command {
            McpCommand::Requirement { .. } | McpCommand::Doctor(_) | McpCommand::Catalog { .. } => {
                CommandMeta::new(false, false, false)
            }
            McpCommand::Plan(_) => CommandMeta::new(true, true, false),
            McpCommand::Apply(_) => CommandMeta::new(true, true, false),
        },
        Command::Provision { command } => match command {
            ProvisionCommand::Plan(_)
            | ProvisionCommand::Doctor(_)
            | ProvisionCommand::Export(_)
            | ProvisionCommand::Import(_) => CommandMeta::new(false, false, true),
            ProvisionCommand::Apply(_) => CommandMeta::new(true, true, false),
        },
        Command::Policy { command } => match command {
            PolicyCommand::Org { command } => match command {
                OrgPolicyCommand::Init(_) => CommandMeta::new(true, true, false),
                OrgPolicyCommand::Show | OrgPolicyCommand::Check(_) => {
                    CommandMeta::new(false, false, false)
                }
            },
        },
        Command::Approval { command } => match command {
            ApprovalCommand::Request(_)
            | ApprovalCommand::Approve(_)
            | ApprovalCommand::Reject(_) => CommandMeta::new(true, true, false),
            ApprovalCommand::List(_) => CommandMeta::new(false, false, false),
        },
        Command::Roles { command } => match command {
            RolesCommand::Grant(_) | RolesCommand::Revoke(_) => CommandMeta::new(true, true, false),
            RolesCommand::List => CommandMeta::new(false, false, false),
        },
        Command::Instruction { .. } => CommandMeta::new(false, false, false),
        Command::Workflow { command } => match command {
            WorkflowCommand::Create(args) => {
                CommandMeta::new(!args.dry_run, !args.dry_run, args.dry_run)
            }
            WorkflowCommand::Plan(_) => CommandMeta::new(true, true, true),
            WorkflowCommand::Show(_) | WorkflowCommand::Preflight(_) | WorkflowCommand::Run(_) => {
                CommandMeta::new(false, false, false)
            }
        },
        Command::Index(_) | Command::Active(_) => CommandMeta::new(false, false, false),
        Command::Sync { command } => {
            let durable = !matches!(command, SyncCommand::Status);
            CommandMeta::new(true, durable, !durable)
        }
        Command::Ops { command } => match command {
            OpsCommand::List => CommandMeta::new(true, false, false),
            OpsCommand::Retry | OpsCommand::Purge => CommandMeta::new(true, true, false),
            OpsCommand::History { command } => {
                let durable = !matches!(command, OpsHistoryCommand::Diagnose);
                CommandMeta::new(true, durable, !durable)
            }
        },
        Command::Agent { .. } => CommandMeta::new(true, false, false),
        Command::Codex { command } => match command {
            CodexCommand::Reconcile(args) => CommandMeta::new(args.apply, args.apply, !args.apply),
        },
        Command::Panel(_) => CommandMeta::new(false, false, false),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::Cli;

    fn meta(args: &[&str]) -> CommandMeta {
        let cli = Cli::try_parse_from(args).expect("test command should parse");
        command_meta(&cli.command)
    }

    #[test]
    fn command_meta_classifies_durable_mutations_previews_and_soft_audit() {
        assert_eq!(meta(&["loom", "init"]), CommandMeta::new(true, true, false));
        assert_eq!(
            meta(&["loom", "workspace", "status"]),
            CommandMeta::new(true, false, false)
        );
        assert_eq!(
            meta(&["loom", "skill", "commit", "demo", "--from-source"]),
            CommandMeta::new(true, true, false)
        );
        assert_eq!(
            meta(&["loom", "skill", "rollback", "demo", "--dry-run"]),
            CommandMeta::new(false, false, true)
        );
        assert_eq!(
            meta(&[
                "loom",
                "package",
                "build",
                "plan.json",
                "--output",
                "package.tgz",
                "--idempotency-key",
                "key-1",
            ]),
            CommandMeta::new(true, false, false)
        );
        assert_eq!(
            meta(&["loom", "telemetry", "purge", "--dry-run"]),
            CommandMeta::new(false, false, true)
        );
        assert_eq!(
            meta(&["loom", "mcp", "plan", "--skill", "demo", "--agent", "codex"]),
            CommandMeta::new(true, true, false)
        );
    }
}
