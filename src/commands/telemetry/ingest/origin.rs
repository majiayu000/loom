use super::super::model::SessionOrigin;

/// Claude Code stamps every transcript record with the client that started the session.
/// Programmatic hosts (`claude -p`, the Agent SDKs) run scheduled jobs and harnesses, so their
/// skill use is real but is not a person asking for the skill.
pub(super) fn claude_entrypoint(entrypoint: &str) -> Option<SessionOrigin> {
    match entrypoint {
        "sdk-cli" | "sdk-py" | "sdk-ts" => Some(SessionOrigin::Automated),
        "cli" | "claude-desktop" | "claude-vscode" | "claude-jetbrains" => {
            Some(SessionOrigin::Interactive)
        }
        _ => None,
    }
}

/// Codex records the client in `session_meta.payload.originator`.
pub(super) fn codex_originator(originator: &str) -> Option<SessionOrigin> {
    match originator {
        "codex_exec" => Some(SessionOrigin::Automated),
        "codex-tui" | "codex_cli_rs" | "codex_vscode" | "codex_work_desktop" => {
            Some(SessionOrigin::Interactive)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionOrigin, claude_entrypoint, codex_originator};

    #[test]
    fn programmatic_clients_are_automated() {
        assert_eq!(claude_entrypoint("sdk-cli"), Some(SessionOrigin::Automated));
        assert_eq!(
            codex_originator("codex_exec"),
            Some(SessionOrigin::Automated)
        );
    }

    #[test]
    fn people_facing_clients_are_interactive() {
        assert_eq!(claude_entrypoint("cli"), Some(SessionOrigin::Interactive));
        assert_eq!(
            claude_entrypoint("claude-desktop"),
            Some(SessionOrigin::Interactive)
        );
        assert_eq!(
            codex_originator("codex-tui"),
            Some(SessionOrigin::Interactive)
        );
    }

    #[test]
    fn unknown_clients_stay_unclassified() {
        assert_eq!(claude_entrypoint("some-new-host"), None);
        assert_eq!(codex_originator(""), None);
    }
}
