use crate::cli::{
    AgentCommand, ApprovalCommand, BackupCommand, CatalogCommand, CodexCommand, Command,
    InstructionCommand, McpCatalogCommand, McpCommand, McpRequirementCommand, OpsCommand,
    OpsHistoryCommand, OrgPolicyCommand, PackageCommand, PlanCommand, PolicyCommand,
    ProviderCommand, ProvisionCommand, RemoteCommand, RolesCommand, SkillActiveCommand,
    SkillAuthorCommand, SkillCommand, SkillCompileCommand, SkillOrphanCommand,
    SkillProvenanceCommand, SkillTrashCommand, SkillsetCommand, SyncCommand, TargetCommand,
    TelemetryCommand, WorkflowCommand, WorkspaceBindingCommand, WorkspaceCommand,
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

    /// No audit trail: read-only queries that never touch registry state.
    const NONE: Self = Self::new(false, false, false);
    /// Best-effort audit trail without a durability requirement.
    const RECORDED: Self = Self::new(true, false, false);
    /// Mutations that must land in the durable audit log.
    const DURABLE: Self = Self::new(true, true, false);
    /// Previews that skip the audit trail entirely.
    const PREVIEW_ONLY: Self = Self::new(false, false, true);
    /// Read-only lookups recorded on a best-effort basis.
    const RECORDED_PREVIEW: Self = Self::new(true, false, true);
    /// Plans that preview changes but still require durable audit.
    const DURABLE_PREVIEW: Self = Self::new(true, true, true);

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

    /// `DURABLE` for real runs, `PREVIEW_ONLY` when `dry_run` is set.
    const fn durable_unless_dry_run(dry_run: bool) -> Self {
        Self::new(!dry_run, !dry_run, dry_run)
    }

    pub(crate) const fn records_audit(self) -> bool {
        self.bits & Self::RECORDS_AUDIT != 0
    }

    pub(crate) const fn durable_audit(self) -> bool {
        self.bits & Self::DURABLE_AUDIT != 0
    }
}

/// Single source of truth for the per-command topology: the canonical audit
/// event name plus the audit requirements. New commands only need one match
/// arm here; `command_name` and `command_meta` are thin projections.
#[derive(Clone, Copy)]
pub(crate) struct CommandDescriptor {
    pub(crate) name: &'static str,
    pub(crate) meta: CommandMeta,
}

const fn desc(name: &'static str, meta: CommandMeta) -> CommandDescriptor {
    CommandDescriptor { name, meta }
}

pub(crate) fn command_meta(command: &Command) -> CommandMeta {
    command_descriptor(command).meta
}

pub(crate) fn command_descriptor(command: &Command) -> CommandDescriptor {
    match command {
        Command::Init => desc("init", CommandMeta::DURABLE),
        Command::Backup { command } => match command {
            BackupCommand::Export(_) => desc("backup.export", CommandMeta::NONE),
            BackupCommand::Inspect(_) => desc("backup.inspect", CommandMeta::NONE),
            BackupCommand::Restore(_) => desc("backup.restore", CommandMeta::NONE),
        },
        Command::Monitor(_) => desc("monitor", CommandMeta::DURABLE),
        Command::Use(args) => desc("use", CommandMeta::new(true, args.apply, !args.apply)),
        Command::Plan { command } => match command {
            PlanCommand::Converge(_) => desc("plan.converge", CommandMeta::DURABLE),
            PlanCommand::Use(_) => desc("plan.use", CommandMeta::DURABLE),
        },
        Command::Apply(_) => desc("apply", CommandMeta::DURABLE),
        Command::Workspace { command } => match command {
            WorkspaceCommand::Status => desc("workspace.status", CommandMeta::RECORDED),
            WorkspaceCommand::Doctor => desc("workspace.doctor", CommandMeta::RECORDED),
            WorkspaceCommand::Init(_) => desc("workspace.init", CommandMeta::DURABLE),
            WorkspaceCommand::Binding { command } => match command {
                WorkspaceBindingCommand::Add(_) => {
                    desc("workspace.binding.add", CommandMeta::DURABLE)
                }
                WorkspaceBindingCommand::List => {
                    desc("workspace.binding.list", CommandMeta::RECORDED_PREVIEW)
                }
                WorkspaceBindingCommand::Show(_) => {
                    desc("workspace.binding.show", CommandMeta::RECORDED_PREVIEW)
                }
                WorkspaceBindingCommand::Remove(_) => {
                    desc("workspace.binding.remove", CommandMeta::DURABLE)
                }
            },
            WorkspaceCommand::Remote { command } => match command {
                RemoteCommand::Set { .. } => desc("workspace.remote", CommandMeta::DURABLE),
                RemoteCommand::Status => desc("workspace.remote", CommandMeta::RECORDED_PREVIEW),
            },
        },
        Command::Target { command } => match command {
            TargetCommand::Add(_) => desc("target.add", CommandMeta::DURABLE),
            TargetCommand::List => desc("target.list", CommandMeta::RECORDED_PREVIEW),
            TargetCommand::Show(_) => desc("target.show", CommandMeta::RECORDED_PREVIEW),
            TargetCommand::Remove(_) => desc("target.remove", CommandMeta::DURABLE),
        },
        Command::Skill { command } => match command {
            SkillCommand::List => desc("skill.list", CommandMeta::NONE),
            SkillCommand::Stats(_) => desc("skill.stats", CommandMeta::NONE),
            SkillCommand::Inspect(_) => desc("skill.inspect", CommandMeta::NONE),
            SkillCommand::Deps(_) => desc("skill.deps", CommandMeta::NONE),
            SkillCommand::Compile(args) => match &args.command {
                Some(SkillCompileCommand::List(_)) => desc("skill.compile.list", CommandMeta::NONE),
                Some(SkillCompileCommand::Verify(_)) => {
                    desc("skill.compile.verify", CommandMeta::NONE)
                }
                None if args.dry_run => desc("skill.compile.dry_run", CommandMeta::NONE),
                None => desc("skill.compile", CommandMeta::NONE),
            },
            SkillCommand::Activate(args) => desc(
                "skill.activate",
                CommandMeta::durable_unless_dry_run(args.dry_run),
            ),
            SkillCommand::Deactivate(args) => desc(
                "skill.deactivate",
                CommandMeta::durable_unless_dry_run(args.dry_run),
            ),
            SkillCommand::Active { command } => match command {
                SkillActiveCommand::List(_) => desc("skill.active.list", CommandMeta::NONE),
            },
            SkillCommand::Search(_) => desc("skill.search", CommandMeta::NONE),
            SkillCommand::Recommend(_) => desc("skill.recommend", CommandMeta::NONE),
            SkillCommand::Resolve(_) => desc("skill.resolve", CommandMeta::NONE),
            SkillCommand::Used(_) => desc("skill.used", CommandMeta::NONE),
            SkillCommand::Feedback(_) => desc("skill.feedback", CommandMeta::NONE),
            SkillCommand::Author { command } => match command {
                SkillAuthorCommand::Draft(args) => desc(
                    "skill.author.draft",
                    CommandMeta::durable_unless_dry_run(args.dry_run),
                ),
                SkillAuthorCommand::Extract(args) => desc(
                    "skill.author.extract",
                    CommandMeta::durable_unless_dry_run(args.dry_run),
                ),
                SkillAuthorCommand::Rewrite(args) => desc(
                    "skill.author.rewrite",
                    CommandMeta::durable_unless_dry_run(args.dry_run),
                ),
                SkillAuthorCommand::TuneDescription(args) => desc(
                    "skill.author.tune_description",
                    CommandMeta::durable_unless_dry_run(args.dry_run),
                ),
                SkillAuthorCommand::GenerateEvals(args) => desc(
                    "skill.author.generate_evals",
                    CommandMeta::durable_unless_dry_run(args.dry_run),
                ),
                SkillAuthorCommand::ApplyPatch(_) => {
                    desc("skill.author.apply_patch", CommandMeta::DURABLE)
                }
                SkillAuthorCommand::New(args) => desc(
                    "skill.author.new",
                    CommandMeta::durable_unless_dry_run(args.dry_run),
                ),
            },
            SkillCommand::Add(_) => desc("skill.add", CommandMeta::DURABLE),
            SkillCommand::Install(args) => desc(
                "skill.install",
                CommandMeta::durable_unless_dry_run(args.dry_run),
            ),
            SkillCommand::ImportObserved(_) => desc("skill.import_observed", CommandMeta::DURABLE),
            SkillCommand::MonitorObserved(_) => {
                desc("skill.monitor_observed", CommandMeta::DURABLE)
            }
            SkillCommand::Project(_) => desc("skill.project", CommandMeta::DURABLE),
            SkillCommand::Commit(_) => desc("skill.commit", CommandMeta::DURABLE),
            SkillCommand::Improve(_) => desc("skill.improve", CommandMeta::NONE),
            SkillCommand::Regression(_) => desc("skill.regression", CommandMeta::NONE),
            SkillCommand::Watch(_) => desc("skill.watch", CommandMeta::DURABLE),
            SkillCommand::Release(_) => desc("skill.release", CommandMeta::DURABLE),
            SkillCommand::Rollback(args) => desc(
                "skill.rollback",
                CommandMeta::durable_unless_dry_run(args.dry_run),
            ),
            SkillCommand::Diff(_) => desc("skill.diff", CommandMeta::RECORDED),
            SkillCommand::History(_) => desc("skill.history", CommandMeta::NONE),
            SkillCommand::Lint(_) => desc("skill.lint", CommandMeta::NONE),
            SkillCommand::Policy(_) => desc("skill.policy", CommandMeta::RECORDED),
            SkillCommand::Scan(_) => desc("skill.scan", CommandMeta::RECORDED),
            SkillCommand::Trust(_) => desc("skill.trust", CommandMeta::DURABLE),
            SkillCommand::Quarantine(_) => desc("skill.quarantine", CommandMeta::DURABLE),
            SkillCommand::Unquarantine(_) => desc("skill.unquarantine", CommandMeta::DURABLE),
            SkillCommand::Visibility(_) => desc("skill.visibility", CommandMeta::NONE),
            SkillCommand::Diagnose(_) => desc("skill.diagnose", CommandMeta::NONE),
            SkillCommand::Eval(_) => desc("skill.eval", CommandMeta::NONE),
            SkillCommand::Provenance { command } => match command {
                SkillProvenanceCommand::Inspect(_) => {
                    desc("skill.provenance.inspect", CommandMeta::RECORDED)
                }
                SkillProvenanceCommand::Verify(_) => {
                    desc("skill.provenance.verify", CommandMeta::RECORDED)
                }
                SkillProvenanceCommand::Outdated(_) => {
                    desc("skill.provenance.outdated", CommandMeta::RECORDED)
                }
                SkillProvenanceCommand::Refresh(_) => {
                    desc("skill.provenance.refresh", CommandMeta::DURABLE)
                }
            },
            SkillCommand::Trash { command } => match command {
                SkillTrashCommand::Add(args) => desc(
                    "skill.trash.add",
                    CommandMeta::durable_unless_dry_run(args.dry_run),
                ),
                SkillTrashCommand::List => desc("skill.trash.list", CommandMeta::NONE),
                SkillTrashCommand::Restore(_) => desc("skill.trash.restore", CommandMeta::DURABLE),
                SkillTrashCommand::Purge(args) => desc(
                    "skill.trash.purge",
                    CommandMeta::durable_unless_dry_run(args.dry_run),
                ),
            },
            SkillCommand::Orphan { command } => match command {
                SkillOrphanCommand::List => desc("skill.orphan.list", CommandMeta::RECORDED),
                SkillOrphanCommand::Clean(_) => desc("skill.orphan.clean", CommandMeta::DURABLE),
            },
        },
        Command::Skillset { command } => match command {
            SkillsetCommand::Create(_) => desc("skillset.create", CommandMeta::DURABLE),
            SkillsetCommand::Add(_) => desc("skillset.add", CommandMeta::DURABLE),
            SkillsetCommand::Remove(_) => desc("skillset.remove", CommandMeta::DURABLE),
            SkillsetCommand::Show(_) => desc("skillset.show", CommandMeta::NONE),
            SkillsetCommand::Lint(_) => desc("skillset.lint", CommandMeta::NONE),
            SkillsetCommand::Activate(args) => desc(
                "skillset.activate",
                CommandMeta::durable_unless_dry_run(args.dry_run),
            ),
            SkillsetCommand::Deactivate(args) => desc(
                "skillset.deactivate",
                CommandMeta::durable_unless_dry_run(args.dry_run),
            ),
            SkillsetCommand::Eval(_) => desc("skillset.eval", CommandMeta::NONE),
            SkillsetCommand::Release(_) => desc("skillset.release", CommandMeta::DURABLE),
            SkillsetCommand::Rollback(_) => desc("skillset.rollback", CommandMeta::DURABLE),
        },
        Command::Telemetry { command } => match command {
            TelemetryCommand::Status => desc("telemetry.status", CommandMeta::NONE),
            TelemetryCommand::Enable(_) => desc("telemetry.enable", CommandMeta::DURABLE),
            TelemetryCommand::Disable => desc("telemetry.disable", CommandMeta::DURABLE),
            TelemetryCommand::Ingest(args) => desc(
                "telemetry.ingest",
                CommandMeta::durable_unless_dry_run(args.dry_run),
            ),
            TelemetryCommand::Report(_) => desc("telemetry.report", CommandMeta::NONE),
            TelemetryCommand::Export(_) => desc("telemetry.export", CommandMeta::NONE),
            TelemetryCommand::Purge(args) => desc(
                "telemetry.purge",
                CommandMeta::new(
                    args.confirm.is_some(),
                    args.confirm.is_some(),
                    args.confirm.is_none(),
                ),
            ),
        },
        Command::Provider { command } => match command {
            ProviderCommand::Add(_) => desc("provider.add", CommandMeta::DURABLE),
            ProviderCommand::List => desc("provider.list", CommandMeta::PREVIEW_ONLY),
            ProviderCommand::Remove(_) => desc("provider.remove", CommandMeta::DURABLE),
        },
        Command::Catalog { command } => match command {
            CatalogCommand::Search(_) => desc("catalog.search", CommandMeta::NONE),
            CatalogCommand::Show(_) => desc("catalog.show", CommandMeta::NONE),
            CatalogCommand::Preview(_) => desc("catalog.preview", CommandMeta::NONE),
        },
        Command::Package { command } => match command {
            PackageCommand::Plan(_) => desc("package.plan", CommandMeta::PREVIEW_ONLY),
            PackageCommand::Build(_) => desc("package.build", CommandMeta::RECORDED),
            PackageCommand::Verify(_) => desc("package.verify", CommandMeta::PREVIEW_ONLY),
        },
        Command::Mcp { command } => match command {
            McpCommand::Requirement { command } => match command {
                McpRequirementCommand::List(_) => desc("mcp.requirement.list", CommandMeta::NONE),
            },
            McpCommand::Plan(_) => desc("mcp.plan", CommandMeta::DURABLE),
            McpCommand::Apply(_) => desc("mcp.apply", CommandMeta::DURABLE),
            McpCommand::Doctor(_) => desc("mcp.doctor", CommandMeta::NONE),
            McpCommand::Catalog { command } => match command {
                McpCatalogCommand::Search(_) => desc("mcp.catalog.search", CommandMeta::NONE),
                McpCatalogCommand::Show(_) => desc("mcp.catalog.show", CommandMeta::NONE),
            },
        },
        Command::Provision { command } => match command {
            ProvisionCommand::Plan(_) => desc("provision.plan", CommandMeta::PREVIEW_ONLY),
            ProvisionCommand::Apply(_) => desc("provision.apply", CommandMeta::DURABLE),
            ProvisionCommand::Doctor(_) => desc("provision.doctor", CommandMeta::PREVIEW_ONLY),
            ProvisionCommand::Export(_) => desc("provision.export", CommandMeta::PREVIEW_ONLY),
            ProvisionCommand::Import(_) => desc("provision.import", CommandMeta::PREVIEW_ONLY),
        },
        Command::Policy { command } => match command {
            PolicyCommand::Org { command } => match command {
                OrgPolicyCommand::Init(_) => desc("policy.org.init", CommandMeta::DURABLE),
                OrgPolicyCommand::Show => desc("policy.org.show", CommandMeta::NONE),
                OrgPolicyCommand::Check(_) => desc("policy.org.check", CommandMeta::NONE),
            },
        },
        Command::Approval { command } => match command {
            ApprovalCommand::Request(_) => desc("approval.request", CommandMeta::DURABLE),
            ApprovalCommand::List(_) => desc("approval.list", CommandMeta::NONE),
            ApprovalCommand::Approve(_) => desc("approval.approve", CommandMeta::DURABLE),
            ApprovalCommand::Reject(_) => desc("approval.reject", CommandMeta::DURABLE),
        },
        Command::Roles { command } => match command {
            RolesCommand::List => desc("roles.list", CommandMeta::NONE),
            RolesCommand::Grant(_) => desc("roles.grant", CommandMeta::DURABLE),
            RolesCommand::Revoke(_) => desc("roles.revoke", CommandMeta::DURABLE),
        },
        Command::Instruction { command } => match command {
            InstructionCommand::Scan(_) => desc("instruction.scan", CommandMeta::NONE),
            InstructionCommand::Show(_) => desc("instruction.show", CommandMeta::NONE),
            InstructionCommand::Classify(_) => desc("instruction.classify", CommandMeta::NONE),
            InstructionCommand::Doctor(_) => desc("instruction.doctor", CommandMeta::NONE),
            InstructionCommand::MigratePlan(_) => {
                desc("instruction.migrate_plan", CommandMeta::NONE)
            }
        },
        Command::Workflow { command } => match command {
            WorkflowCommand::Create(args) => desc(
                "workflow.create",
                CommandMeta::durable_unless_dry_run(args.dry_run),
            ),
            WorkflowCommand::Show(_) => desc("workflow.show", CommandMeta::NONE),
            WorkflowCommand::Plan(_) => desc("workflow.plan", CommandMeta::DURABLE_PREVIEW),
            WorkflowCommand::Preflight(_) => desc("workflow.preflight", CommandMeta::NONE),
            WorkflowCommand::Run(_) => desc("workflow.run", CommandMeta::NONE),
        },
        Command::Index(args) if args.action == "build" => desc("index.build", CommandMeta::NONE),
        Command::Index(args) if args.action == "status" => desc("index.status", CommandMeta::NONE),
        Command::Index(_) => desc("index", CommandMeta::NONE),
        Command::Active(args) if args.action == "recommend" => {
            desc("active.recommend", CommandMeta::NONE)
        }
        Command::Active(_) => desc("active", CommandMeta::NONE),
        Command::Sync { command } => match command {
            SyncCommand::Status => desc("sync.status", CommandMeta::RECORDED_PREVIEW),
            SyncCommand::Push(_) => desc("sync.push", CommandMeta::DURABLE),
            SyncCommand::Pull => desc("sync.pull", CommandMeta::DURABLE),
            SyncCommand::Replay => desc("sync.replay", CommandMeta::DURABLE),
        },
        Command::Ops { command } => match command {
            OpsCommand::List(_) => desc("ops.list", CommandMeta::RECORDED),
            OpsCommand::Retry => desc("ops.retry", CommandMeta::DURABLE),
            OpsCommand::Purge => desc("ops.purge", CommandMeta::DURABLE),
            OpsCommand::History { command } => match command {
                OpsHistoryCommand::Diagnose => {
                    desc("ops.history.diagnose", CommandMeta::RECORDED_PREVIEW)
                }
                OpsHistoryCommand::Repair(_) => desc("ops.history.repair", CommandMeta::DURABLE),
            },
        },
        Command::Agent { command } => match command {
            AgentCommand::Preflight(_) => desc("agent.preflight", CommandMeta::RECORDED),
            AgentCommand::Reconcile(_) => desc("agent.reconcile", CommandMeta::RECORDED),
        },
        Command::Codex { command } => match command {
            CodexCommand::Reconcile(args) => desc(
                "codex.reconcile",
                CommandMeta::new(args.apply, args.apply, !args.apply),
            ),
        },
        Command::Panel(_) => desc("panel", CommandMeta::NONE),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::Cli;

    fn parse(args: &[&str]) -> Command {
        Cli::try_parse_from(args)
            .unwrap_or_else(|err| panic!("test command should parse: {args:?}: {err}"))
            .command
    }

    fn meta(args: &[&str]) -> CommandMeta {
        command_meta(&parse(args))
    }

    fn descriptor(args: &[&str]) -> CommandDescriptor {
        command_descriptor(&parse(args))
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
            meta(&["loom", "skill", "trash", "add", "demo", "--dry-run"]),
            CommandMeta::new(false, false, true)
        );
        assert_eq!(
            meta(&["loom", "skill", "trash", "purge", "entry", "--dry-run"]),
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
            meta(&["loom", "telemetry", "ingest", "--agent", "all", "--dry-run",]),
            CommandMeta::new(false, false, true)
        );
        assert_eq!(
            meta(&["loom", "telemetry", "ingest", "--agent", "all"]),
            CommandMeta::new(true, true, false)
        );
        assert_eq!(
            meta(&["loom", "mcp", "plan", "--skill", "demo", "--agent", "codex"]),
            CommandMeta::new(true, true, false)
        );
    }

    #[test]
    fn command_descriptor_pins_name_and_audit_pairs() {
        // Read-only skill queries: no audit trail at all.
        let d = descriptor(&["loom", "skill", "list"]);
        assert_eq!(d.name, "skill.list");
        assert_eq!(d.meta, CommandMeta::new(false, false, false));

        // skill.policy and skill.scan record audit on a best-effort basis
        // while skill.lint does not; this asymmetry is pre-existing behavior.
        let d = descriptor(&["loom", "skill", "policy", "demo"]);
        assert_eq!(d.name, "skill.policy");
        assert_eq!(d.meta, CommandMeta::new(true, false, false));
        let d = descriptor(&["loom", "skill", "lint", "demo"]);
        assert_eq!(d.name, "skill.lint");
        assert_eq!(d.meta, CommandMeta::new(false, false, false));

        // Provenance verify records audit; refresh requires durable audit.
        let d = descriptor(&["loom", "skill", "provenance", "verify", "demo"]);
        assert_eq!(d.name, "skill.provenance.verify");
        assert_eq!(d.meta, CommandMeta::new(true, false, false));
        let d = descriptor(&["loom", "skill", "provenance", "refresh", "demo"]);
        assert_eq!(d.name, "skill.provenance.refresh");
        assert_eq!(d.meta, CommandMeta::new(true, true, false));

        // Compile verify keeps its dedicated name but stays audit-free.
        let d = descriptor(&["loom", "skill", "compile", "verify", "demo"]);
        assert_eq!(d.name, "skill.compile.verify");
        assert_eq!(d.meta, CommandMeta::new(false, false, false));

        // Orphan clean is durable even with --dry-run; pre-existing behavior.
        let d = descriptor(&["loom", "skill", "orphan", "clean", "--dry-run"]);
        assert_eq!(d.name, "skill.orphan.clean");
        assert_eq!(d.meta, CommandMeta::new(true, true, false));

        // Read-only listings that still record best-effort audit previews.
        let d = descriptor(&["loom", "workspace", "binding", "list"]);
        assert_eq!(d.name, "workspace.binding.list");
        assert_eq!(d.meta, CommandMeta::new(true, false, true));
        let d = descriptor(&["loom", "sync", "status"]);
        assert_eq!(d.name, "sync.status");
        assert_eq!(d.meta, CommandMeta::new(true, false, true));

        // provider.list previews without recording; pre-existing asymmetry
        // with target.list which records best-effort.
        let d = descriptor(&["loom", "provider", "list"]);
        assert_eq!(d.name, "provider.list");
        assert_eq!(d.meta, CommandMeta::new(false, false, true));
        let d = descriptor(&["loom", "target", "list"]);
        assert_eq!(d.name, "target.list");
        assert_eq!(d.meta, CommandMeta::new(true, false, true));

        // workflow.plan is the only durable preview.
        let d = descriptor(&[
            "loom",
            "workflow",
            "plan",
            "wf-1",
            "--agent",
            "codex",
            "--workspace",
            "/tmp/ws",
        ]);
        assert_eq!(d.name, "workflow.plan");
        assert_eq!(d.meta, CommandMeta::new(true, true, true));

        // Arg-dependent metas keep a stable name.
        let d = descriptor(&["loom", "use", "profile"]);
        assert_eq!(d.name, "use");
        assert_eq!(d.meta, CommandMeta::new(true, false, true));
        let d = descriptor(&["loom", "use", "profile", "--apply"]);
        assert_eq!(d.name, "use");
        assert_eq!(d.meta, CommandMeta::new(true, true, false));
        let d = descriptor(&["loom", "codex", "reconcile"]);
        assert_eq!(d.name, "codex.reconcile");
        assert_eq!(d.meta, CommandMeta::new(false, false, true));
        let d = descriptor(&["loom", "codex", "reconcile", "--apply"]);
        assert_eq!(d.name, "codex.reconcile");
        assert_eq!(d.meta, CommandMeta::new(true, true, false));

        // Remote subcommands share one name with split audit requirements.
        let d = descriptor(&["loom", "workspace", "remote", "status"]);
        assert_eq!(d.name, "workspace.remote");
        assert_eq!(d.meta, CommandMeta::new(true, false, true));
        let d = descriptor(&[
            "loom",
            "workspace",
            "remote",
            "set",
            "git@example.com:r.git",
        ]);
        assert_eq!(d.name, "workspace.remote");
        assert_eq!(d.meta, CommandMeta::new(true, true, false));

        // Index/active fall back to bare names on unknown actions.
        assert_eq!(descriptor(&["loom", "index", "build"]).name, "index.build");
        assert_eq!(descriptor(&["loom", "index", "bogus"]).name, "index");
        assert_eq!(
            descriptor(&["loom", "active", "recommend", "task", "--agent", "codex"]).name,
            "active.recommend"
        );
        assert_eq!(
            descriptor(&["loom", "active", "bogus", "task", "--agent", "codex"]).name,
            "active"
        );
    }
}
