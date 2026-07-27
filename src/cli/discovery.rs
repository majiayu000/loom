use std::path::PathBuf;

use clap::{
    Arg, Args, Command, ValueEnum,
    builder::{PossibleValue, StringValueParser, TypedValueParser},
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ActiveAction {
    Recommend,
}

#[derive(Clone)]
struct ActiveActionParser;

impl TypedValueParser for ActiveActionParser {
    type Value = String;

    fn parse_ref(
        &self,
        command: &Command,
        argument: Option<&Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, clap::Error> {
        StringValueParser::new().parse_ref(command, argument, value)
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(
            ActiveAction::value_variants()
                .iter()
                .filter_map(ValueEnum::to_possible_value),
        ))
    }
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillSearchArgs {
    /// Lexical query matched against skill id, description, tags, and warnings.
    pub query: String,

    /// Treat the query as a task and include deterministic selection metadata.
    #[arg(long)]
    pub for_task: bool,

    /// Restrict results to skills compatible with this agent.
    #[arg(long)]
    pub agent: Option<String>,

    /// Restrict results to skills connected to this profile id.
    #[arg(long)]
    pub profile: Option<String>,

    /// Restrict results by source status such as present, missing, or non-compliant.
    #[arg(long)]
    pub status: Option<String>,

    /// Restrict results by trust metadata. Only unknown is available until policy metadata lands.
    #[arg(long)]
    pub trust: Option<String>,

    /// Boost skills whose binding matcher covers this workspace path.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Policy binding context used to disambiguate recommendation output.
    #[arg(long)]
    pub binding: Option<String>,

    /// Policy profile context used when no binding is selected.
    #[arg(long = "policy-profile")]
    pub policy_profile: Option<String>,

    /// Restrict results to skills with an active projection record.
    #[arg(long)]
    pub active: bool,

    /// Request local semantic retrieval. Falls back to lexical mode when no local provider exists.
    #[arg(long)]
    pub semantic: bool,

    /// Include recommendation explanations, skillset candidates, and safety/risk inputs.
    #[arg(long)]
    pub explain: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ActiveRecommendArgs {
    /// Action to run: recommend.
    #[arg(value_parser = ActiveActionParser, hide_possible_values = true)]
    pub action: String,

    /// Task description for the desired active state.
    pub task_description: String,

    /// Agent whose active view should be compared.
    #[arg(long)]
    pub agent: String,

    /// Workspace path for project-scoped recommendations.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Disambiguate the active binding when multiple bindings match.
    #[arg(long)]
    pub binding: Option<String>,

    /// Explicit desired skills to compare against the active view.
    #[arg(long = "desired-skill")]
    pub desired_skills: Vec<String>,
}

impl ActiveRecommendArgs {
    pub(crate) fn is_recommend_action(&self) -> bool {
        ActiveAction::Recommend
            .to_possible_value()
            .is_some_and(|value| value.matches(&self.action, false))
    }

    pub(crate) fn expected_actions(&self) -> String {
        ActiveAction::value_variants()
            .iter()
            .filter_map(ValueEnum::to_possible_value)
            .map(|value| value.get_name().to_string())
            .collect::<Vec<_>>()
            .join(" or ")
    }
}
