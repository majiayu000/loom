use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NextAction {
    pub cmd: String,
    pub reason: String,
}

impl NextAction {
    pub(crate) fn new(cmd: &str, reason: &str) -> Self {
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
        "LOCK_BUSY" => vec![NextAction::new(
            "loom ops list --json",
            "inspect active or queued operations before retrying",
        )],
        "REMOTE_UNREACHABLE" | "REMOTE_DIVERGED" | "PUSH_REJECTED" => vec![NextAction::new(
            "loom sync status --json",
            "inspect remote synchronization state before retrying",
        )],
        _ => Vec::new(),
    }
}

pub(crate) fn contextual_skill_action(skill: &str, reason: &str) -> NextAction {
    NextAction::new(
        &format!("loom skill inspect {} --json", shell_arg(skill)),
        reason,
    )
}

fn shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionCoverage {
    Default,
    Contextual,
    Exempt,
}

#[cfg(test)]
fn action_coverage(code: crate::types::ErrorCode) -> ActionCoverage {
    use crate::types::ErrorCode;

    match code {
        ErrorCode::BindingNotFound
        | ErrorCode::TargetNotFound
        | ErrorCode::SkillNotFound
        | ErrorCode::StateNotInitialized
        | ErrorCode::TargetNotManaged
        | ErrorCode::LockBusy
        | ErrorCode::RemoteUnreachable
        | ErrorCode::RemoteDiverged
        | ErrorCode::PushRejected => ActionCoverage::Default,
        ErrorCode::DependencyConflict
        | ErrorCode::ProviderNotFound
        | ErrorCode::TrashEntryNotFound
        | ErrorCode::TargetAgentMismatch
        | ErrorCode::ProjectionConflict
        | ErrorCode::ProjectionMethodUnsupported
        | ErrorCode::PolicyBlocked
        | ErrorCode::EvalFailed
        | ErrorCode::CaptureConflict
        | ErrorCode::CommitDirectionAmbiguous
        | ErrorCode::ReplayConflict
        | ErrorCode::QueueBlocked
        | ErrorCode::AdapterInvalid => ActionCoverage::Contextual,
        ErrorCode::ArgInvalid
        | ErrorCode::InitError
        | ErrorCode::SchemaMismatch
        | ErrorCode::StateCorrupt
        | ErrorCode::AuditError
        | ErrorCode::GitError
        | ErrorCode::IoError
        | ErrorCode::InternalError => ActionCoverage::Exempt,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{ActionCoverage, action_coverage, contextual_skill_action, default_next_actions};
    use crate::types::ErrorCode;

    #[test]
    fn default_next_actions_cover_top_guidance_errors() {
        for code in [
            "BINDING_NOT_FOUND",
            "TARGET_NOT_FOUND",
            "SKILL_NOT_FOUND",
            "STATE_NOT_INITIALIZED",
            "TARGET_NOT_MANAGED",
            "LOCK_BUSY",
            "REMOTE_UNREACHABLE",
            "REMOTE_DIVERGED",
            "PUSH_REJECTED",
        ] {
            let actions = default_next_actions(code);
            assert!(!actions.is_empty(), "missing next action for {code}");
            assert!(
                actions.iter().all(|action| action.cmd.starts_with("loom ")),
                "next action commands must be runnable loom commands: {actions:?}"
            );
            assert!(
                actions.iter().all(|action| action.cmd.contains("--json")),
                "next action commands must request JSON output: {actions:?}"
            );
        }
    }

    #[test]
    fn default_next_actions_omit_unmapped_errors() {
        assert!(default_next_actions("ARG_INVALID").is_empty());
    }

    #[test]
    fn error_actions_totality_matches_contract() {
        let contract = include_str!("../docs/LOOM_CLI_CONTRACT.md");
        let mut rows = BTreeMap::new();
        let mut in_table = false;
        for line in contract.lines() {
            if line == "<!-- error-code-table-start -->" {
                in_table = true;
                continue;
            }
            if line == "<!-- error-code-table-end -->" {
                break;
            }
            if !in_table || !line.starts_with("| `") {
                continue;
            }
            let code = line
                .split('|')
                .nth(1)
                .expect("error code table first column")
                .trim()
                .trim_matches('`');
            assert!(
                rows.insert(code, line).is_none(),
                "duplicate contract error code {code}"
            );
        }

        assert_eq!(ErrorCode::ALL.len(), 30);
        assert_eq!(
            rows.len(),
            ErrorCode::ALL.len(),
            "contract code count drift"
        );
        assert_eq!(
            ErrorCode::ALL
                .iter()
                .filter(|code| code.exit_code() == 3)
                .count(),
            21,
            "exit-3 tier count drift"
        );
        for code in ErrorCode::ALL {
            let row = rows
                .get(code.as_str())
                .unwrap_or_else(|| panic!("contract missing error code {}", code.as_str()));
            let coverage = action_coverage(code);
            let label = match coverage {
                ActionCoverage::Default => "default",
                ActionCoverage::Contextual => "contextual",
                ActionCoverage::Exempt => "exempt",
            };
            assert!(
                row.contains(&format!("| `{label}` |")),
                "contract coverage mismatch for {}: {row}",
                code.as_str()
            );
            assert!(
                row.contains(&format!("| {} |", code.exit_code())),
                "contract exit-code mismatch for {}: {row}",
                code.as_str()
            );
            match coverage {
                ActionCoverage::Default => assert!(
                    !default_next_actions(code.as_str()).is_empty(),
                    "default coverage has no action for {}",
                    code.as_str()
                ),
                ActionCoverage::Contextual | ActionCoverage::Exempt => assert!(
                    default_next_actions(code.as_str()).is_empty(),
                    "non-default coverage unexpectedly has a default for {}",
                    code.as_str()
                ),
            }
        }
    }

    #[test]
    fn contextual_actions_are_concrete_json_commands() {
        let action = contextual_skill_action("demo skill", "inspect the affected skill");
        assert_eq!(action.cmd, "loom skill inspect 'demo skill' --json");
        assert!(!action.cmd.contains('<'));
        assert!(!action.cmd.contains('>'));
    }
}
