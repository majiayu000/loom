use std::path::{Path, PathBuf};

use crate::cli::AgentKind;

pub(super) const DEFAULT_SCAN_AGENTS: [AgentKind; 10] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Cursor,
    AgentKind::Windsurf,
    AgentKind::Cline,
    AgentKind::Copilot,
    AgentKind::Aider,
    AgentKind::Opencode,
    AgentKind::GeminiCli,
    AgentKind::Goose,
];

pub(super) fn default_skill_dir(agent: AgentKind, home: &Path) -> PathBuf {
    match agent {
        AgentKind::Claude => home.join(".claude/skills"),
        AgentKind::Codex => home.join(".codex/skills"),
        AgentKind::Cursor => home.join(".cursor/skills"),
        AgentKind::Windsurf => home.join(".windsurf/skills"),
        AgentKind::Cline => home.join(".cline/skills"),
        AgentKind::Copilot => home.join(".github/copilot/skills"),
        AgentKind::Aider => home.join(".aider/skills"),
        AgentKind::Opencode => home.join(".opencode/skills"),
        AgentKind::GeminiCli => home.join(".gemini/skills"),
        AgentKind::Goose => home.join(".config/goose/skills"),
    }
}
