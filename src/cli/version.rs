use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillCommitArgs {
    /// Registry skill name.
    pub skill: String,

    /// Git commit message for the source revision.
    #[arg(long)]
    pub message: Option<String>,

    /// Force capture semantics from one live projection.
    #[arg(long, conflicts_with = "from_source")]
    pub from_projection: bool,

    /// Force save semantics from the registry source tree.
    #[arg(long, conflicts_with = "from_projection")]
    pub from_source: bool,

    /// Binding id used to disambiguate projection capture.
    #[arg(long)]
    pub binding: Option<String>,

    /// Projection instance id used to disambiguate projection capture.
    #[arg(long)]
    pub instance: Option<String>,

    /// Run the skill improvement preflight before committing source-side edits.
    #[arg(long)]
    pub preflight: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ReleaseArgs {
    /// Registry skill name.
    pub skill: String,
    /// Release tag or version label to create. Omit with --anchor for an unnamed anchor.
    pub version: Option<String>,

    /// Create an unnamed anchor tag instead of a versioned release tag.
    #[arg(long, conflicts_with = "version")]
    pub anchor: bool,

    /// Run the skill improvement preflight before creating the release tag.
    #[arg(long)]
    pub preflight: bool,

    /// Baseline ref used by release preflight, usually the previous release.
    #[arg(long)]
    pub baseline: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct RollbackArgs {
    /// Registry skill name.
    pub skill: String,

    /// Git revision or snapshot reference to restore from.
    #[arg(long)]
    pub to: Option<String>,

    /// Number of source commits to roll back when --to is not provided.
    #[arg(long)]
    pub steps: Option<u32>,

    /// Preview rollback impact without changing Git refs, source files, or registry state.
    #[arg(long, visible_alias = "preview")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct DiffArgs {
    /// Registry skill name.
    pub skill: String,

    /// Return a structured security-relevant diff instead of the raw patch.
    #[arg(long)]
    pub security: bool,

    /// Older revision, snapshot, or release reference.
    pub from: String,

    /// Newer revision, snapshot, or release reference.
    pub to: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct HistoryArgs {
    /// Registry skill name.
    pub skill: String,

    /// Maximum number of history entries to return.
    #[arg(long, default_value_t = 30)]
    pub limit: usize,

    /// Older revision boundary. When set, history uses <from>..<to>.
    #[arg(long)]
    pub from: Option<String>,

    /// Newer revision boundary.
    #[arg(long, default_value = "HEAD")]
    pub to: String,

    /// Include per-commit short diff statistics.
    #[arg(long)]
    pub include_diff_stat: bool,

    /// Include registry operations added by each history commit.
    #[arg(long, default_value_t = true)]
    pub include_ops: bool,
}
