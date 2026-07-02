use std::{fmt, ops::Deref};

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(
    rename_all = "kebab-case",
    export,
    export_to = "../panel/src/generated/"
)]
pub enum AgentKind {
    Claude,
    Codex,
    Cursor,
    Windsurf,
    Cline,
    Copilot,
    Aider,
    Opencode,
    GeminiCli,
    Goose,
}

impl AgentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::Cline => "cline",
            Self::Copilot => "copilot",
            Self::Aider => "aider",
            Self::Opencode => "opencode",
            Self::GeminiCli => "gemini-cli",
            Self::Goose => "goose",
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<AgentKind> for AgentId {
    fn from(value: AgentKind) -> Self {
        Self(value.as_str().to_string())
    }
}

impl From<String> for AgentId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for AgentId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl Deref for AgentId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for AgentId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for AgentId {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    rename_all = "lowercase",
    export,
    export_to = "../panel/src/generated/"
)]
pub enum Ownership {
    Managed,
    Observed,
    External,
}

impl Ownership {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Observed => "observed",
            Self::External => "external",
        }
    }
}

impl fmt::Display for Ownership {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for Ownership {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    rename_all = "lowercase",
    export,
    export_to = "../panel/src/generated/"
)]
pub enum ProjectionMethod {
    Symlink,
    Copy,
    Materialize,
}

impl ProjectionMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Symlink => "symlink",
            Self::Copy => "copy",
            Self::Materialize => "materialize",
        }
    }
}

impl fmt::Display for ProjectionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for ProjectionMethod {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    rename_all = "lowercase",
    export,
    export_to = "../panel/src/generated/"
)]
pub enum Health {
    Healthy,
    Drifted,
    Missing,
    Conflict,
    Orphaned,
}

impl Health {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Drifted => "drifted",
            Self::Missing => "missing",
            Self::Conflict => "conflict",
            Self::Orphaned => "orphaned",
        }
    }
}

impl fmt::Display for Health {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for Health {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(
    rename_all = "snake_case",
    export,
    export_to = "../panel/src/generated/"
)]
pub enum MatcherKind {
    #[serde(alias = "path-prefix")]
    PathPrefix,
    #[serde(alias = "exact-path")]
    ExactPath,
    Name,
}

impl MatcherKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PathPrefix => "path_prefix",
            Self::ExactPath => "exact_path",
            Self::Name => "name",
        }
    }
}

impl fmt::Display for MatcherKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for MatcherKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentKind, MatcherKind};

    #[test]
    fn agent_kind_serde_round_trip_uses_kebab_case() {
        for (variant, wire) in [
            (AgentKind::Claude, "\"claude\""),
            (AgentKind::Codex, "\"codex\""),
            (AgentKind::Cursor, "\"cursor\""),
            (AgentKind::Windsurf, "\"windsurf\""),
            (AgentKind::Cline, "\"cline\""),
            (AgentKind::Copilot, "\"copilot\""),
            (AgentKind::Aider, "\"aider\""),
            (AgentKind::Opencode, "\"opencode\""),
            (AgentKind::Goose, "\"goose\""),
            (AgentKind::GeminiCli, "\"gemini-cli\""),
        ] {
            let serialized = serde_json::to_string(&variant).expect("serialize AgentKind");
            assert_eq!(serialized, wire, "serialize {:?}", variant);

            let deserialized: AgentKind =
                serde_json::from_str(wire).expect("deserialize AgentKind");
            assert_eq!(deserialized, variant, "deserialize {}", wire);
        }
    }

    #[test]
    fn matcher_kind_deserializes_cli_and_api_spellings() {
        let kebab: MatcherKind =
            serde_json::from_str("\"path-prefix\"").expect("deserialize kebab-case matcher");
        let snake: MatcherKind =
            serde_json::from_str("\"path_prefix\"").expect("deserialize snake_case matcher");

        assert_eq!(kebab, MatcherKind::PathPrefix);
        assert_eq!(snake, MatcherKind::PathPrefix);
    }
}
