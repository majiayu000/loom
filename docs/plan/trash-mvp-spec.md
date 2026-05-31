# Trash MVP Spec

Issue: https://github.com/majiayu000/loom/issues/181

## Goal

Make skill deletion recoverable by default. A user who removes a skill through Loom should get a Git-tracked trash entry, operation history, and a direct restore path instead of an irreversible filesystem delete.

## User Problem

Loom is a skill management tool. Its core trust surface is not whether an agent can repair a broken skill at runtime, but whether the user can manage skill assets safely:

- Recover a deleted skill without remembering Git commands.
- See when and why a skill was removed.
- Keep the deleted content in Git history until the user explicitly purges it.
- Refuse destructive restores that would overwrite a live skill.

## MVP Scope

Add source-skill trash support under the registry root:

- `loom skill trash add <skill>`
- `loom skill trash list`
- `loom skill trash restore <skill> [--trash-id <id>]`
- `loom skill trash purge <trash-id>`

The CLI is the first shipped surface. Panel UI, projection cleanup, and live agent directory workflows are follow-up work.

## Data Model

Trash entries live under:

```text
trash/<trash_id>/
  metadata.json
  skill/
    SKILL.md
    ...
```

`metadata.json` fields:

- `schema_version`: currently `1`.
- `trash_id`: stable entry id.
- `skill`: source skill name.
- `original_path`: original registry-relative path, for example `skills/idea-team`.
- `trashed_at`: UTC timestamp.
- `source_commit`: registry `HEAD` before the skill was trashed.

`trash_id` format:

```text
<skill>-<utc timestamp>-<uuid suffix>
```

All IDs are filesystem-safe and limited to ASCII alphanumeric, dot, underscore, and hyphen.

## Behavior

### Trash

`loom skill trash add <skill>`:

- Validates the skill name.
- Requires a healthy write registry and Git repository.
- Acquires workspace and skill locks.
- Moves `skills/<skill>` to `trash/<trash_id>/skill`.
- Writes `trash/<trash_id>/metadata.json`.
- Commits the source move with `trash(<skill>): move to trash`.
- Records a `skill.trash.add` registry operation.
- Returns `skill`, `trash_id`, `trash_path`, `commit`, and optional `state_commit`.

### List

`loom skill trash list`:

- Reads all valid `trash/*/metadata.json` entries.
- Sorts newest first by `trashed_at`, then `trash_id`.
- Returns an `items` array.
- Does not mutate state and does not require durable audit.

Malformed trash metadata should be reported as warnings, not hidden. A bad entry must not break valid entries.

### Restore

`loom skill trash restore <skill> [--trash-id <id>]`:

- Resolves `--trash-id` exactly, or chooses the newest trashed entry for the skill.
- Refuses to restore if `skills/<skill>` already exists.
- Moves `trash/<trash_id>/skill` back to `skills/<skill>`.
- Removes the trash entry directory.
- Commits with `restore(<skill>): restore from trash`.
- Records a `skill.trash.restore` registry operation.

### Purge

`loom skill trash purge <trash-id>`:

- Removes the selected trash entry permanently.
- Commits with `purge(<trash_id>): remove trash entry`.
- Records a `skill.trash.purge` registry operation.

## Acceptance Criteria

- Trash removes `skills/<skill>` and creates a tracked trash entry.
- Restore recreates `skills/<skill>` and removes the trash entry.
- Restore refuses to overwrite an existing live skill.
- Purge removes exactly one trash entry.
- Mutations create Git commits and registry operation records.
- Integration tests cover trash/list/restore/conflict/purge.
- `cargo check` and `cargo test` pass from this branch.
