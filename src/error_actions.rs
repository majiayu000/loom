use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NextAction {
    pub cmd: String,
    pub reason: String,
}

impl NextAction {
    fn new(cmd: &str, reason: &str) -> Self {
        Self {
            cmd: cmd.to_string(),
            reason: reason.to_string(),
        }
    }
}

pub(crate) fn default_next_actions(code: &str) -> Vec<NextAction> {
    match code {
        "BINDING_NOT_FOUND" => vec![NextAction::new(
            "loom workspace binding list --json",
            "list existing bindings to find a valid binding_id",
        )],
        "TARGET_NOT_FOUND" => vec![NextAction::new(
            "loom target list --json",
            "list registered targets to find a valid target_id",
        )],
        "SKILL_NOT_FOUND" => vec![NextAction::new(
            "loom skill list --json",
            "list available skills to find a valid skill name",
        )],
        "STATE_NOT_INITIALIZED" => vec![NextAction::new(
            "loom workspace init --json",
            "initialize registry state before running registry commands",
        )],
        "TARGET_NOT_MANAGED" => vec![NextAction::new(
            "loom target list --json",
            "inspect target ownership before writing projections",
        )],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::default_next_actions;

    #[test]
    fn default_next_actions_cover_top_guidance_errors() {
        for code in [
            "BINDING_NOT_FOUND",
            "TARGET_NOT_FOUND",
            "SKILL_NOT_FOUND",
            "STATE_NOT_INITIALIZED",
            "TARGET_NOT_MANAGED",
        ] {
            let actions = default_next_actions(code);
            assert!(!actions.is_empty(), "missing next action for {code}");
            assert!(
                actions.iter().all(|action| action.cmd.starts_with("loom ")),
                "next action commands must be runnable loom commands: {actions:?}"
            );
        }
    }

    #[test]
    fn default_next_actions_omit_unmapped_errors() {
        assert!(default_next_actions("ARG_INVALID").is_empty());
    }
}
