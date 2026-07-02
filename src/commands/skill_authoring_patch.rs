use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::state::AppContext;
use crate::types::ErrorCode;

use super::CommandFailure;
use super::helpers::map_io;
use super::skill_authoring::split_lines;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PatchChangeKind {
    Add,
    Modify,
}

#[derive(Debug)]
pub(super) struct ReviewedPatchFile {
    pub path: String,
    pub change: String,
}

#[derive(Debug)]
pub(super) struct ParsedPatchChange {
    pub rel: PathBuf,
    pub body: String,
}

struct ParsedPatchFile {
    rel: PathBuf,
    kind: PatchChangeKind,
    hunks: Vec<PatchHunk>,
}

struct PatchHunk {
    old_start: usize,
    old_count: usize,
    new_count: usize,
    lines: Vec<HunkLine>,
}

enum HunkLine {
    Context(String),
    Remove(String),
    Add(String),
}

pub(super) fn parse_patch_changes(
    ctx: &AppContext,
    skill: &str,
    patch_body: &str,
    files: &[ReviewedPatchFile],
) -> std::result::Result<Vec<ParsedPatchChange>, CommandFailure> {
    let expected = expected_changes(skill, files)?;
    let mut parsed = BTreeMap::<String, ParsedPatchFile>::new();
    let mut current: Option<ParsedPatchFile> = None;
    let mut current_hunk: Option<PatchHunk> = None;

    for line in patch_body.lines() {
        if line.starts_with("diff --git ") {
            finish_hunk(&mut current, &mut current_hunk)?;
            finish_file(&mut parsed, current.take())?;
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ b/") {
            finish_hunk(&mut current, &mut current_hunk)?;
            let rel = validate_patch_rel(skill, path)?;
            let key = rel.to_string_lossy().to_string();
            let Some(kind) = expected.get(&key).copied() else {
                return Err(CommandFailure::new(
                    ErrorCode::SchemaMismatch,
                    "patch target is not listed in artifact files",
                ));
            };
            current = Some(ParsedPatchFile {
                rel,
                kind,
                hunks: Vec::new(),
            });
            continue;
        }
        if line.starts_with("@@ ") {
            finish_hunk(&mut current, &mut current_hunk)?;
            if current.is_none() {
                return Err(CommandFailure::new(
                    ErrorCode::SchemaMismatch,
                    "patch hunk is missing target path",
                ));
            }
            current_hunk = Some(parse_hunk_header(line)?);
            continue;
        }
        if let Some(hunk) = current_hunk.as_mut() {
            if let Some(added) = line.strip_prefix('+') {
                hunk.lines.push(HunkLine::Add(added.to_string()));
            } else if let Some(removed) = line.strip_prefix('-') {
                hunk.lines.push(HunkLine::Remove(removed.to_string()));
            } else if let Some(context) = line.strip_prefix(' ') {
                hunk.lines.push(HunkLine::Context(context.to_string()));
            } else if line.starts_with("\\ No newline") {
                continue;
            } else {
                return Err(CommandFailure::new(
                    ErrorCode::SchemaMismatch,
                    "unsupported patch hunk line",
                ));
            }
        }
    }
    finish_hunk(&mut current, &mut current_hunk)?;
    finish_file(&mut parsed, current.take())?;

    if parsed.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "patch file contains no supported file changes",
        ));
    }
    if parsed.keys().collect::<Vec<_>>() != expected.keys().collect::<Vec<_>>() {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "patch paths do not match artifact files",
        ));
    }

    parsed
        .into_values()
        .map(|file| apply_file_patch(ctx, file))
        .collect()
}

pub(super) fn validate_patch_rel(
    skill: &str,
    raw: &str,
) -> std::result::Result<PathBuf, CommandFailure> {
    let path = Path::new(raw);
    let parts = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_string_lossy().to_string(),
            _ => String::new(),
        })
        .collect::<Vec<_>>();
    if parts.len() < 3 || parts[0] != "skills" || parts[1] != skill {
        return Err(CommandFailure::new(
            ErrorCode::PolicyBlocked,
            format!("patch path '{raw}' is outside skills/{skill}"),
        ));
    }
    if parts.iter().any(|part| part.is_empty() || part == ".git") {
        return Err(CommandFailure::new(
            ErrorCode::PolicyBlocked,
            format!("patch path '{raw}' is not allowed"),
        ));
    }
    Ok(parts.iter().collect::<PathBuf>())
}

fn expected_changes(
    skill: &str,
    files: &[ReviewedPatchFile],
) -> std::result::Result<BTreeMap<String, PatchChangeKind>, CommandFailure> {
    let mut paths = BTreeMap::new();
    for file in files {
        let kind = match file.change.as_str() {
            "add" => PatchChangeKind::Add,
            "modify" => PatchChangeKind::Modify,
            other => {
                return Err(CommandFailure::new(
                    ErrorCode::SchemaMismatch,
                    format!("unsupported patch file change '{other}'"),
                ));
            }
        };
        let rel = validate_patch_rel(skill, &file.path)?;
        if paths
            .insert(rel.to_string_lossy().to_string(), kind)
            .is_some()
        {
            return Err(CommandFailure::new(
                ErrorCode::SchemaMismatch,
                "patch artifact contains duplicate file paths",
            ));
        }
    }
    Ok(paths)
}

fn finish_hunk(
    current: &mut Option<ParsedPatchFile>,
    hunk: &mut Option<PatchHunk>,
) -> std::result::Result<(), CommandFailure> {
    let Some(hunk) = hunk.take() else {
        return Ok(());
    };
    let Some(file) = current.as_mut() else {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "patch hunk is missing target path",
        ));
    };
    file.hunks.push(hunk);
    Ok(())
}

fn finish_file(
    parsed: &mut BTreeMap<String, ParsedPatchFile>,
    current: Option<ParsedPatchFile>,
) -> std::result::Result<(), CommandFailure> {
    let Some(file) = current else {
        return Ok(());
    };
    if file.hunks.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "patch file change contains no hunks",
        ));
    }
    let key = file.rel.to_string_lossy().to_string();
    if parsed.insert(key, file).is_some() {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "patch contains duplicate file changes",
        ));
    }
    Ok(())
}

fn parse_hunk_header(line: &str) -> std::result::Result<PatchHunk, CommandFailure> {
    let Some(rest) = line.strip_prefix("@@ ") else {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "unsupported patch hunk header",
        ));
    };
    let Some(end) = rest.find(" @@") else {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "unsupported patch hunk header",
        ));
    };
    let mut parts = rest[..end].split_whitespace();
    let old = parts.next().ok_or_else(|| {
        CommandFailure::new(ErrorCode::SchemaMismatch, "patch hunk missing old range")
    })?;
    let new = parts.next().ok_or_else(|| {
        CommandFailure::new(ErrorCode::SchemaMismatch, "patch hunk missing new range")
    })?;
    let (old_start, old_count) = parse_range(old, '-')?;
    let (_, new_count) = parse_range(new, '+')?;
    Ok(PatchHunk {
        old_start,
        old_count,
        new_count,
        lines: Vec::new(),
    })
}

fn parse_range(raw: &str, prefix: char) -> std::result::Result<(usize, usize), CommandFailure> {
    let Some(range) = raw.strip_prefix(prefix) else {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "unsupported patch hunk range",
        ));
    };
    let (start, count) = match range.split_once(',') {
        Some((start, count)) => (start, count),
        None => (range, "1"),
    };
    let start = start
        .parse::<usize>()
        .map_err(|_| CommandFailure::new(ErrorCode::SchemaMismatch, "invalid patch hunk start"))?;
    let count = count
        .parse::<usize>()
        .map_err(|_| CommandFailure::new(ErrorCode::SchemaMismatch, "invalid patch hunk count"))?;
    Ok((start, count))
}

fn apply_file_patch(
    ctx: &AppContext,
    file: ParsedPatchFile,
) -> std::result::Result<ParsedPatchChange, CommandFailure> {
    let target = ctx.root.join(&file.rel);
    let old = match file.kind {
        PatchChangeKind::Add if target.exists() => {
            return Err(CommandFailure::new(
                ErrorCode::SchemaMismatch,
                format!("add patch target '{}' already exists", file.rel.display()),
            ));
        }
        PatchChangeKind::Add => String::new(),
        PatchChangeKind::Modify if !target.is_file() => {
            return Err(CommandFailure::new(
                ErrorCode::SchemaMismatch,
                format!(
                    "modify patch target '{}' does not exist",
                    file.rel.display()
                ),
            ));
        }
        PatchChangeKind::Modify => fs::read_to_string(&target).map_err(map_io)?,
    };
    let body = apply_hunks(&split_lines(&old), &file.hunks)?;
    Ok(ParsedPatchChange {
        rel: file.rel,
        body,
    })
}

fn apply_hunks(
    old_lines: &[String],
    hunks: &[PatchHunk],
) -> std::result::Result<String, CommandFailure> {
    let mut output = Vec::new();
    let mut cursor = 0usize;

    for hunk in hunks {
        let start = hunk.old_start.saturating_sub(1);
        if start < cursor || start > old_lines.len() {
            return Err(CommandFailure::new(
                ErrorCode::SchemaMismatch,
                "patch hunk old range does not match source",
            ));
        }
        output.extend(old_lines[cursor..start].iter().cloned());
        let mut old_index = start;
        let mut old_seen = 0usize;
        let mut new_seen = 0usize;
        for line in &hunk.lines {
            match line {
                HunkLine::Context(expected) => {
                    require_old_line(old_lines, old_index, expected)?;
                    output.push(expected.clone());
                    old_index += 1;
                    old_seen += 1;
                    new_seen += 1;
                }
                HunkLine::Remove(expected) => {
                    require_old_line(old_lines, old_index, expected)?;
                    old_index += 1;
                    old_seen += 1;
                }
                HunkLine::Add(value) => {
                    output.push(value.clone());
                    new_seen += 1;
                }
            }
        }
        if old_seen != hunk.old_count || new_seen != hunk.new_count {
            return Err(CommandFailure::new(
                ErrorCode::SchemaMismatch,
                "patch hunk line counts do not match header",
            ));
        }
        cursor = old_index;
    }

    output.extend(old_lines[cursor..].iter().cloned());
    Ok(join_lines(&output))
}

fn require_old_line(
    old_lines: &[String],
    index: usize,
    expected: &str,
) -> std::result::Result<(), CommandFailure> {
    if old_lines.get(index).is_some_and(|line| line == expected) {
        return Ok(());
    }
    Err(CommandFailure::new(
        ErrorCode::CaptureConflict,
        "patch hunk context does not match source",
    ))
}

fn join_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}
