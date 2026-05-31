# Skill History Timeline Spec

Date: 2026-05-29
Status: Draft
GitHub issue: https://github.com/majiayu000/loom/issues/184

## Goal

Make version history visible and understandable for each skill.

Users should not need to know Git commands to answer:

- What changed in this skill?
- When did it change?
- Which Loom operation caused it?
- Which snapshots and releases are relevant?
- What revision should I diff or roll back to?

## Problem

Loom already records skill changes in Git and registry operations, but that value is hidden behind raw Git commands and internal JSONL files. Without a first-class history surface, version control feels theoretical.

This blocks the next layer of management features: rollback preview, blame, backup audits, and Panel timelines.

## Non-Goals

- Full Git blame UI.
- Branch graph visualization.
- Editing commits, rebasing, or squashing.
- Team author identity management beyond what Git already records.

## CLI

```text
loom skill history <skill> [--limit <n>] [--from <rev>] [--to <rev>] [--include-diff-stat] [--include-ops]
```

Defaults:

- `--limit`: 30.
- `--to`: `HEAD`.
- `--include-ops`: true.
- `--include-diff-stat`: false.

## JSON Output

```json
{
  "skill": "idea-team",
  "range": {
    "from": null,
    "to": "HEAD"
  },
  "items": [
    {
      "commit": "abc123...",
      "short_commit": "abc1234",
      "author_name": "loom",
      "author_email": "loom@local",
      "committed_at": "2026-05-29T10:00:00Z",
      "message": "save(idea-team): event",
      "refs": ["snapshot/idea-team/20260529T100000Z-abc1234"],
      "operations": [
        {
          "op_id": "op_...",
          "intent": "skill.save",
          "created_at": "2026-05-29T10:00:01Z"
        }
      ],
      "diff_stat": {
        "files_changed": 2,
        "insertions": 12,
        "deletions": 3
      }
    }
  ]
}
```

## Behavior

### Read-Only Contract

`skill history` must not:

- Initialize a Git repository.
- Create registry state.
- Append command audit.
- Modify `.gitignore`.
- Write any state files.

If the registry is not a Git repository, return `ARG_INVALID` with an actionable message.

### Skill Filtering

The command only includes commits that touched:

- `skills/<skill>/`
- optionally `trash/*/metadata.json` and `trash/*/skill/` when `--include-trash` is added later.

MVP should only include live skill path history.

### Operation Enrichment

When registry operation logs exist, enrich commits by matching:

- payload fields such as `skill` or `skill_id`
- effects fields such as `skill` or `skill_id`

If operation logs are missing or malformed, return history with warnings instead of failing the whole command.

### Refs

For each commit, include matching refs:

- `snapshot/<skill>/...`
- `release/<skill>/...`
- `recovery/<skill>/...`

Refs should be local only in MVP. Remote ref enrichment is out of scope.

## Implementation Notes

Likely files:

- `src/cli.rs`: add `SkillCommand::History`.
- `src/commands/history_cmds.rs`: implement command.
- existing `gitops::run_git`: call Git with argument arrays for `git log`, `git for-each-ref`, and optional shortstat.
- `src/panel/skill_history.rs`: reuse parsing concepts in later Panel follow-up.
- `tests/skill_history_cli.rs`: integration tests.

Use Git command arrays, never shell string concatenation.

## Acceptance Criteria

- `loom skill history demo` returns newest-first commits touching `skills/demo`.
- The command is read-only in an empty directory.
- `--limit` caps results.
- Snapshot and release refs are attached to matching commits.
- Registry operation enrichment works when operations exist.
- Malformed operation records produce warnings, not hidden drops.
- `cargo check` and `cargo test --test skill_history_cli` pass.

## Follow-Up

- Panel Skill detail timeline.
- `--include-trash`.
- Blame summary per file.
- History search by message/ref/date.
