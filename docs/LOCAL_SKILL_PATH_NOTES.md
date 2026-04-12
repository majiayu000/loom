# Local Skill Path Notes

This document stores machine-local troubleshooting notes that are useful during repository maintenance but do not belong in the user-facing `README.md`.

## Snapshot Date

- Captured on `2026-04-12`

## Runtime Shape

- `~/.claude/skills`: `106` top-level entries, `95` are symlinks.
- `~/.claude-work/skills`: only a small set of top-level symlinks (`6`); most entries are local directories.
- Typical pattern: the top-level skill directory is a symlink, while files inside the target directory are regular files.

Example:

- `~/.claude/skills/xiaohongshu` is a symlink.
- `~/.claude/skills/xiaohongshu/references/runtime-rules.md` is a regular file reached through that symlinked directory.

## Sample Target Distribution

Sample distribution for top-level symlinks under `~/.claude/skills`:

- `58` -> `/Users/lifcc/Desktop/code/skill_backup/20260407_154053/claude_skills/*`
- `25` -> `/Users/lifcc/Desktop/code/skill_backup/20260407_154053/codex_skills/*`
- `5` -> `/Users/lifcc/.codex/skills/.system/*`
- `5` -> `/Users/lifcc/Desktop/code/work/life/looper/skills/*`
- `1` -> `/Users/lifcc/Desktop/code/AI/agent/claude-arsenal/skills/*`
- `1` -> `/Users/lifcc/Desktop/code/AI/tools/claude-skill-registry-data/data/*`

## Missing Example

- `~/.claude/skills/x-article-publisher` is currently missing.

Equivalent skill folders exist in registry trees, for example:

- `/Users/lifcc/Desktop/code/AI/tools/claude-skill-registry/skills/data/x-article-publisher`
- `/Users/lifcc/Desktop/code/AI/tools/claude-skill-registry/skills/other/x-article-publisher`
- `/Users/lifcc/Desktop/code/AI/tools/claude-skill-registry/skills/documents/x-article-publisher`
