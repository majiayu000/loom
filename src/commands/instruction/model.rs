use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use walkdir::{DirEntry, WalkDir};

use crate::cli::AgentKind;
use crate::commands::helpers::{agent_kind_as_str, map_io};
use crate::commands::{CommandFailure, instruction::analysis::read_text_lossy};
use crate::sha256::{Sha256, to_hex};
use crate::types::ErrorCode;

#[derive(Debug, Clone, Serialize)]
pub(super) struct InstructionSurface {
    pub(super) instruction_id: String,
    pub(super) agent: String,
    pub(super) kind: String,
    pub(super) scope: String,
    pub(super) path: String,
    pub(super) applies_to: Option<String>,
    pub(super) path_patterns: Vec<String>,
    pub(super) precedence: String,
    pub(super) always_on: bool,
    pub(super) contains_skill_like_workflow: bool,
    pub(super) suggested_action: String,
    pub(super) signals: Vec<String>,
    pub(super) warnings: Vec<String>,
    pub(super) size_bytes: u64,
    pub(super) line_count: usize,
    pub(super) adapter_source: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct UnsupportedSurface {
    pub(super) agent: String,
    pub(super) workspace: String,
    pub(super) reason: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct InstructionAdapterMetadata {
    agent: &'static str,
    kind: &'static str,
    path_pattern: &'static str,
    precedence: &'static str,
    always_on: bool,
    adapter_source: &'static str,
}

pub(super) struct ScanResult {
    pub(super) surfaces: Vec<InstructionSurface>,
    pub(super) unsupported_surfaces: Vec<UnsupportedSurface>,
    pub(super) warnings: Vec<String>,
}

pub(super) fn scan_workspace(
    workspace: &Path,
    agent_filter: Option<AgentKind>,
) -> std::result::Result<ScanResult, CommandFailure> {
    let mut surfaces = Vec::new();
    let mut warnings = Vec::new();
    for entry in WalkDir::new(workspace)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_descend)
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                warnings.push(format!("failed to inspect path while scanning: {err}"));
                continue;
            }
        };
        if !entry.file_type().is_file() || is_inside_skill_root(workspace, entry.path()) {
            continue;
        }
        for metadata in classify_metadatas(workspace, entry.path(), agent_filter) {
            surfaces.push(build_surface(workspace, entry.path(), metadata)?);
        }
    }
    surfaces.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(ScanResult {
        surfaces,
        unsupported_surfaces: unsupported_surfaces(workspace, agent_filter),
        warnings,
    })
}

pub(super) fn classify_path(
    workspace: &Path,
    path: &Path,
) -> std::result::Result<InstructionSurface, CommandFailure> {
    if is_inside_skill_root(workspace, path) {
        return build_unknown_surface(workspace, path, "skill");
    }
    match classify_metadatas(workspace, path, None).into_iter().next() {
        Some(metadata) => build_surface(workspace, path, metadata),
        None => build_unknown_surface(workspace, path, "unknown"),
    }
}

pub(super) fn adapter_metadata_json(agent_filter: Option<AgentKind>) -> Vec<Value> {
    instruction_metadata(agent_filter)
        .into_iter()
        .map(|metadata| json!(metadata))
        .collect()
}

pub(super) fn resolve_workspace(
    path: Option<&Path>,
) -> std::result::Result<PathBuf, CommandFailure> {
    let raw = match path {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().map_err(map_io)?,
    };
    if !raw.is_dir() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("workspace '{}' is not a directory", raw.display()),
        ));
    }
    fs::canonicalize(&raw).map_err(map_io)
}

pub(super) fn resolve_file(path: &Path) -> std::result::Result<PathBuf, CommandFailure> {
    if !path.is_file() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("path '{}' is not a file", path.display()),
        ));
    }
    fs::canonicalize(path).map_err(map_io)
}

fn build_surface(
    workspace: &Path,
    path: &Path,
    metadata: InstructionAdapterMetadata,
) -> std::result::Result<InstructionSurface, CommandFailure> {
    let signals = read_text_lossy(path)?;
    let scope = scope_for(workspace, path, metadata);
    let path_patterns = if is_path_specific_copilot(metadata) {
        signals.path_patterns.clone()
    } else {
        Vec::new()
    };
    let applies_to = applies_to(workspace, path, metadata, &path_patterns);
    let path_string = path.display().to_string();
    let instruction_id = instruction_id(metadata.agent, metadata.kind, &path_string);

    Ok(InstructionSurface {
        instruction_id,
        agent: metadata.agent.to_string(),
        kind: metadata.kind.to_string(),
        scope,
        path: path_string,
        applies_to,
        path_patterns,
        precedence: metadata.precedence.to_string(),
        always_on: metadata.always_on,
        contains_skill_like_workflow: signals.contains_skill_like_workflow,
        suggested_action: signals.suggested_action,
        signals: signals.signals,
        warnings: signals.warnings,
        size_bytes: signals.size_bytes,
        line_count: signals.line_count,
        adapter_source: metadata.adapter_source.to_string(),
    })
}

fn build_unknown_surface(
    workspace: &Path,
    path: &Path,
    kind: &str,
) -> std::result::Result<InstructionSurface, CommandFailure> {
    let signals = read_text_lossy(path)?;
    let path_string = path.display().to_string();
    Ok(InstructionSurface {
        instruction_id: instruction_id("unknown", kind, &path_string),
        agent: "unknown".to_string(),
        kind: kind.to_string(),
        scope: scope_for_unknown(workspace, path, kind),
        path: path_string,
        applies_to: applies_to_unknown(workspace, path, kind)
            .map(|path| path.display().to_string()),
        path_patterns: Vec::new(),
        precedence: "unknown".to_string(),
        always_on: false,
        contains_skill_like_workflow: signals.contains_skill_like_workflow,
        suggested_action: "unsupported".to_string(),
        signals: signals.signals,
        warnings: vec!["adapter metadata is not available for this surface".to_string()],
        size_bytes: signals.size_bytes,
        line_count: signals.line_count,
        adapter_source: "unknown".to_string(),
    })
}

fn classify_metadatas(
    workspace: &Path,
    path: &Path,
    agent_filter: Option<AgentKind>,
) -> Vec<InstructionAdapterMetadata> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let Ok(rel) = path.strip_prefix(workspace) else {
        return Vec::new();
    };
    let rel_parts = rel
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();

    if file_name == "AGENTS.md" {
        candidates.push(metadata_for("codex", "agents_md", "**/AGENTS.md"));
        candidates.push(metadata_for("copilot", "agents_md", "**/AGENTS.md"));
    } else if file_name == "CLAUDE.md" {
        candidates.push(metadata_for("claude", "claude_md", "**/CLAUDE.md"));
    } else if rel_parts.len() >= 3
        && rel_parts[0] == ".cursor"
        && rel_parts[1] == "rules"
        && file_name.ends_with(".mdc")
    {
        candidates.push(metadata_for("cursor", "cursor_rule", ".cursor/rules/*.mdc"));
    } else if rel_parts == [".github", "copilot-instructions.md"] {
        candidates.push(metadata_for(
            "copilot",
            "copilot_instruction",
            ".github/copilot-instructions.md",
        ));
    } else if rel_parts.len() >= 3
        && rel_parts[0] == ".github"
        && rel_parts[1] == "instructions"
        && file_name.ends_with(".instructions.md")
    {
        candidates.push(metadata_for(
            "copilot",
            "copilot_instruction",
            ".github/instructions/*.instructions.md",
        ));
    } else if rel_parts.len() >= 3 && rel_parts[0] == ".windsurf" && file_name.ends_with(".md") {
        match rel_parts[1] {
            "rules" => candidates.push(metadata_for(
                "windsurf",
                "windsurf_rule",
                ".windsurf/rules/*.md",
            )),
            "workflows" => candidates.push(metadata_for(
                "windsurf",
                "windsurf_workflow",
                ".windsurf/workflows/*.md",
            )),
            "memories" => candidates.push(metadata_for(
                "windsurf",
                "windsurf_memory",
                ".windsurf/memories/*.md",
            )),
            _ => {}
        }
    }

    candidates
        .into_iter()
        .flatten()
        .filter(|metadata| match agent_filter {
            Some(agent) => metadata.agent == agent_kind_as_str(agent),
            None => metadata.agent != "copilot" || metadata.kind != "agents_md",
        })
        .collect()
}

fn metadata_for(agent: &str, kind: &str, path_pattern: &str) -> Option<InstructionAdapterMetadata> {
    instruction_metadata(None).into_iter().find(|metadata| {
        metadata.agent == agent && metadata.kind == kind && metadata.path_pattern == path_pattern
    })
}

fn instruction_metadata(agent_filter: Option<AgentKind>) -> Vec<InstructionAdapterMetadata> {
    let metadata = vec![
        InstructionAdapterMetadata {
            agent: "codex",
            kind: "agents_md",
            path_pattern: "**/AGENTS.md",
            precedence: "agent-defined",
            always_on: true,
            adapter_source: "built-in",
        },
        InstructionAdapterMetadata {
            agent: "copilot",
            kind: "agents_md",
            path_pattern: "**/AGENTS.md",
            precedence: "agent-defined",
            always_on: true,
            adapter_source: "built-in",
        },
        InstructionAdapterMetadata {
            agent: "claude",
            kind: "claude_md",
            path_pattern: "**/CLAUDE.md",
            precedence: "agent-defined",
            always_on: true,
            adapter_source: "built-in",
        },
        InstructionAdapterMetadata {
            agent: "cursor",
            kind: "cursor_rule",
            path_pattern: ".cursor/rules/*.mdc",
            precedence: "adapter-defined",
            always_on: true,
            adapter_source: "built-in",
        },
        InstructionAdapterMetadata {
            agent: "copilot",
            kind: "copilot_instruction",
            path_pattern: ".github/copilot-instructions.md",
            precedence: "adapter-defined",
            always_on: true,
            adapter_source: "built-in",
        },
        InstructionAdapterMetadata {
            agent: "copilot",
            kind: "copilot_instruction",
            path_pattern: ".github/instructions/*.instructions.md",
            precedence: "adapter-defined",
            always_on: false,
            adapter_source: "built-in",
        },
        InstructionAdapterMetadata {
            agent: "windsurf",
            kind: "windsurf_rule",
            path_pattern: ".windsurf/rules/*.md",
            precedence: "adapter-defined",
            always_on: true,
            adapter_source: "built-in",
        },
        InstructionAdapterMetadata {
            agent: "windsurf",
            kind: "windsurf_workflow",
            path_pattern: ".windsurf/workflows/*.md",
            precedence: "adapter-defined",
            always_on: false,
            adapter_source: "built-in",
        },
        InstructionAdapterMetadata {
            agent: "windsurf",
            kind: "windsurf_memory",
            path_pattern: ".windsurf/memories/*.md",
            precedence: "adapter-defined",
            always_on: true,
            adapter_source: "built-in",
        },
    ];
    match agent_filter {
        Some(agent) => metadata
            .into_iter()
            .filter(|metadata| metadata.agent == agent_kind_as_str(agent))
            .collect(),
        None => metadata,
    }
}

fn unsupported_surfaces(
    workspace: &Path,
    agent_filter: Option<AgentKind>,
) -> Vec<UnsupportedSurface> {
    match agent_filter {
        Some(agent) if instruction_metadata(Some(agent)).is_empty() => vec![UnsupportedSurface {
            agent: agent_kind_as_str(agent).to_string(),
            workspace: workspace.display().to_string(),
            reason: "adapter has no instruction-surface metadata for read-only discovery"
                .to_string(),
        }],
        _ => Vec::new(),
    }
}

fn should_descend(entry: &DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    !matches!(
        name.as_ref(),
        ".git"
            | "target"
            | "node_modules"
            | ".loom-registry"
            | ".specrail"
            | ".cargo"
            | "dist"
            | "build"
            | "vendor"
    )
}

fn is_inside_skill_root(workspace: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(workspace) else {
        return false;
    };
    let parts = rel
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect::<Vec<_>>();
    parts.first() == Some(&"skills")
        || parts.windows(2).any(|window| {
            matches!(
                window,
                [".agents", "skills"]
                    | [".claude", "skills"]
                    | [".codex", "skills"]
                    | [".cursor", "skills"]
                    | [".windsurf", "skills"]
            )
        })
}

fn scope_for(workspace: &Path, path: &Path, metadata: InstructionAdapterMetadata) -> String {
    if is_path_specific_copilot(metadata) {
        return "path-specific".to_string();
    }
    if matches!(
        metadata.kind,
        "cursor_rule"
            | "copilot_instruction"
            | "windsurf_rule"
            | "windsurf_workflow"
            | "windsurf_memory"
    ) {
        return "workspace".to_string();
    }
    match path.parent() {
        Some(parent) if parent == workspace => "workspace".to_string(),
        Some(_) => "nested".to_string(),
        None => "unknown".to_string(),
    }
}

fn applies_to(
    workspace: &Path,
    path: &Path,
    metadata: InstructionAdapterMetadata,
    path_patterns: &[String],
) -> Option<String> {
    if !path_patterns.is_empty() {
        return Some(path_patterns.join(","));
    }
    if matches!(
        metadata.kind,
        "cursor_rule"
            | "copilot_instruction"
            | "windsurf_rule"
            | "windsurf_workflow"
            | "windsurf_memory"
    ) {
        return Some(workspace.display().to_string());
    }
    path.parent().map(|path| path.display().to_string())
}

fn scope_for_unknown(workspace: &Path, path: &Path, kind: &str) -> String {
    if matches!(
        kind,
        "cursor_rule"
            | "copilot_instruction"
            | "windsurf_rule"
            | "windsurf_workflow"
            | "windsurf_memory"
    ) {
        return "workspace".to_string();
    }
    match path.parent() {
        Some(parent) if parent == workspace => "workspace".to_string(),
        Some(_) => "nested".to_string(),
        None => "unknown".to_string(),
    }
}

fn applies_to_unknown(workspace: &Path, path: &Path, kind: &str) -> Option<PathBuf> {
    if matches!(
        kind,
        "cursor_rule"
            | "copilot_instruction"
            | "windsurf_rule"
            | "windsurf_workflow"
            | "windsurf_memory"
    ) {
        return Some(workspace.to_path_buf());
    }
    path.parent().map(Path::to_path_buf)
}

fn is_path_specific_copilot(metadata: InstructionAdapterMetadata) -> bool {
    metadata.agent == "copilot"
        && metadata.kind == "copilot_instruction"
        && metadata.path_pattern == ".github/instructions/*.instructions.md"
}

fn instruction_id(agent: &str, kind: &str, path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(agent.as_bytes());
    hasher.update(b"\0");
    hasher.update(kind.as_bytes());
    hasher.update(b"\0");
    hasher.update(path.as_bytes());
    let digest = to_hex(&hasher.finalize());
    format!(
        "instr_{}_{}_{}",
        agent.replace('-', "_"),
        kind,
        &digest[..12]
    )
}
