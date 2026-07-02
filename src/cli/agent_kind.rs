use clap::ValueEnum;

#[derive(
    Debug,
    Clone,
    Copy,
    ValueEnum,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
#[serde(rename_all = "kebab-case")]
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

#[cfg(test)]
mod tests {
    use super::AgentKind;

    #[test]
    fn agent_kind_serde_round_trip_uses_kebab_case() {
        // Existing single-word variants must keep their legacy lowercase spelling
        // (kebab-case == lowercase for single words, so persisted data is unaffected).
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
            // Multi-word variant uses kebab-case, matching the CLI flag value.
            (AgentKind::GeminiCli, "\"gemini-cli\""),
        ] {
            let serialized = serde_json::to_string(&variant).expect("serialize AgentKind");
            assert_eq!(serialized, wire, "serialize {:?}", variant);

            let deserialized: AgentKind =
                serde_json::from_str(wire).expect("deserialize AgentKind");
            assert_eq!(deserialized, variant, "deserialize {}", wire);
        }
    }
}
