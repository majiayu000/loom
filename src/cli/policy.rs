use clap::{Args, Subcommand};
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillPolicyArgs {
    /// Registry skill name.
    pub skill: String,

    /// Policy profile to evaluate. Built-ins: safe-capture, audit-only, deny-risky, strict.
    #[arg(long)]
    pub policy_profile: Option<String>,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum PolicyCommand {
    #[command(about = "Manage Git-backed org policy")]
    Org {
        #[command(subcommand)]
        command: OrgPolicyCommand,
    },
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum OrgPolicyCommand {
    #[command(about = "Initialize org policy with an explicit first admin")]
    Init(OrgPolicyInitArgs),
    #[command(about = "Show org policy, roles, and approval summary")]
    Show,
    #[command(about = "Evaluate one lifecycle action against org policy")]
    Check(OrgPolicyCheckArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct OrgPolicyInitArgs {
    /// First admin subject. Required for a fresh policy bootstrap.
    #[arg(long)]
    pub bootstrap_admin: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct OrgPolicyCheckArgs {
    /// Canonical action id, for example skill.activate or provider.add.
    pub action: String,

    /// Skill subject for skill lifecycle actions.
    #[arg(long)]
    pub skill: Option<String>,

    /// Provider subject for provider lifecycle actions.
    #[arg(long)]
    pub provider: Option<String>,

    /// Sync remote subject for sync lifecycle actions.
    #[arg(long)]
    pub sync_remote: Option<String>,

    /// Agent subject for agent-specific actions.
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum ApprovalCommand {
    #[command(about = "Create an auditable approval request")]
    Request(ApprovalRequestArgs),
    #[command(about = "List approval request state derived from append-only events")]
    List(ApprovalListArgs),
    #[command(about = "Approve one request if the current actor has a required role")]
    Approve(ApprovalDecisionArgs),
    #[command(about = "Reject one request if the current actor has a required role")]
    Reject(ApprovalDecisionArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ApprovalRequestArgs {
    /// Canonical action id, for example skill.activate or provider.add.
    pub action: String,

    /// Skill subject for skill lifecycle actions.
    #[arg(long)]
    pub skill: Option<String>,

    /// Provider subject for provider lifecycle actions.
    #[arg(long)]
    pub provider: Option<String>,

    /// Sync remote subject for sync lifecycle actions.
    #[arg(long)]
    pub sync_remote: Option<String>,

    /// Agent subject for agent-specific actions.
    #[arg(long)]
    pub agent: Option<String>,

    /// Free-form reason. Redacted before persistence.
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ApprovalListArgs {
    /// Show only pending requests.
    #[arg(long)]
    pub pending: bool,

    /// Show only approved requests.
    #[arg(long)]
    pub approved: bool,

    /// Show only rejected requests.
    #[arg(long)]
    pub rejected: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ApprovalDecisionArgs {
    /// Request id returned by approval request.
    pub request_id: String,

    /// Free-form comment. Redacted before persistence.
    #[arg(long)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum RolesCommand {
    #[command(about = "List resolved org role grants")]
    List,
    #[command(about = "Grant a role; requires current admin role")]
    Grant(RoleGrantArgs),
    #[command(about = "Revoke a role; requires current admin role")]
    Revoke(RoleGrantArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct RoleGrantArgs {
    /// User or team subject, for example alice or team:platform.
    pub subject: String,

    /// Role to grant or revoke.
    pub role: String,
}
