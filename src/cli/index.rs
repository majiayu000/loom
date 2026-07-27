use clap::{
    Arg, Args, Command, ValueEnum,
    builder::{PossibleValue, StringValueParser, TypedValueParser},
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IndexAction {
    Build,
    Status,
}

#[derive(Clone)]
struct IndexActionParser;

impl TypedValueParser for IndexActionParser {
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
            IndexAction::value_variants()
                .iter()
                .filter_map(ValueEnum::to_possible_value),
        ))
    }
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct IndexArgs {
    /// Action to run: build or status.
    #[arg(value_parser = IndexActionParser, hide_possible_values = true)]
    pub action: String,

    /// Skip embedding records even when a local provider is configured.
    #[arg(long)]
    pub no_embeddings: bool,

    /// Embedding provider. `local` falls back to no embeddings until configured.
    #[arg(long, default_value = "none")]
    pub provider: String,
}

impl IndexArgs {
    pub(crate) fn is_build_action(&self) -> bool {
        self.matches_action(IndexAction::Build)
    }

    pub(crate) fn is_status_action(&self) -> bool {
        self.matches_action(IndexAction::Status)
    }

    pub(crate) fn expected_actions(&self) -> String {
        IndexAction::value_variants()
            .iter()
            .filter_map(ValueEnum::to_possible_value)
            .map(|value| value.get_name().to_string())
            .collect::<Vec<_>>()
            .join(" or ")
    }

    fn matches_action(&self, expected: IndexAction) -> bool {
        expected
            .to_possible_value()
            .is_some_and(|value| value.matches(&self.action, false))
    }
}
