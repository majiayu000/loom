# Panel Trash UI Spec

Date: 2026-06-04
Status: Draft
GitHub issue: https://github.com/majiayu000/loom/issues/232

## Goal

Expose the existing Git-tracked skill trash workflow in Panel so users can recoverably remove skills, inspect trashed entries, restore them, or purge one entry without switching to the CLI.

The CLI trash MVP already provides the source-of-truth operations:

- `loom skill trash add <skill>`
- `loom skill trash list`
- `loom skill trash restore <skill> [--trash-id <id>]`
- `loom skill trash purge <trash-id>`

Panel should reuse those commands through the existing mutation envelope path rather than creating a second deletion implementation.

## User Problem

The Skills page currently shows live skills and lifecycle actions, but the recoverable deletion path is hidden behind terminal commands. Users need to answer these questions from the UI:

- Can I remove this skill without losing it permanently?
- What is currently in the trash?
- Which trash entry is newest for a skill?
- Can I restore the deleted skill?
- Can I permanently purge a specific trash entry when I am sure?

## Non-Goals

- Bulk trash, bulk restore, or bulk purge.
- Full Git history browsing for trash entries.
- Projection cleanup or target directory deletion. Trash only moves the source skill under the registry root.
- A separate top-level Trash page in the MVP. Trash belongs inside the Skills management workflow.
- Editing trash metadata.

## API Contract

Add v1 Panel routes that wrap the existing CLI commands and return the same envelope shape used by current Panel APIs.

### List

```text
GET /api/v1/skills/trash
```

Response data:

```json
{
  "items": [
    {
      "trash_id": "demo-20260604T010203Z-a1b2c3d4",
      "skill": "demo",
      "original_path": "skills/demo",
      "trashed_at": "2026-06-04T01:02:03Z",
      "source_commit": "abcdef...",
      "trash_path": "trash/demo-20260604T010203Z-a1b2c3d4"
    }
  ]
}
```

Rules:

- Read-only.
- Must not initialize registry state or append command audit beyond the normal read envelope behavior.
- Malformed trash entries should surface as envelope warnings when the CLI command emits warnings.

### Move To Trash

```text
POST /api/v1/skills/{skill_name}/trash
```

Body: `{}`.

Maps to `SkillCommand::Trash { Add(SkillOnlyArgs { skill }) }`.

Expected success status: `200 OK`.

Rules:

- Uses `ensure_mutation_authorized`.
- Respects live/read-only Panel mode through the existing UI gate.
- Returns CLI data including `trash_id`, `trash_path`, and `commit`.
- On success, the Skills page refreshes live data and clears the current selection if the removed skill was selected.

### Restore

```text
POST /api/v1/skills/trash/{trash_id}/restore
```

Body:

```json
{
  "skill": "demo"
}
```

Maps to `SkillCommand::Trash { Restore(TrashRestoreArgs { skill, trash_id: Some(trash_id) }) }`.

Rules:

- Restore must be by explicit `trash_id`; the UI should not rely on newest-entry resolution.
- If `skills/<skill>` already exists, return the CLI `ARG_INVALID` error unchanged.
- On success, refresh skills and trash list.

### Purge

```text
POST /api/v1/skills/trash/{trash_id}/purge
```

Body: `{}`.

Maps to `SkillCommand::Trash { Purge(TrashPurgeArgs { trash_id }) }`.

Rules:

- Requires a confirm step in the UI because the operation permanently deletes a trash entry.
- On success, refresh the trash list.

## UI Design

Add a compact mode switch in the Skills page header:

```text
Skills | Trash
```

Design constraints:

- Keep the existing Skills page layout: header, left table, right detail pane.
- Use a segmented control instead of a new sidebar page so trash stays close to skill management.
- Keep action buttons small and explicit; avoid a large destructive banner.
- Disable all mutation buttons when `readOnly` is true.
- Preserve the existing search input in each mode, scoped to the visible list.

### Skills Mode

The current skill table remains the default. Add a `Trash` action to the selected skill detail near lifecycle actions.

Behavior:

- Button label: `Trash`.
- Disabled when `readOnly` or `sourceStatus !== "present"`.
- First click opens an inline confirmation panel in the detail pane.
- Confirmation copy states that the skill will move to Git-tracked trash and can be restored later.
- Confirm calls `api.skillTrashAdd(skill.name)`.
- Success clears the confirmation, refreshes Panel data, and leaves the user on Skills mode unless the skill list becomes empty.

### Trash Mode

The left table lists trash entries newest-first:

Columns:

- Skill
- Trashed
- Source commit
- Trash id

The right pane shows the selected trash entry:

- skill name
- original path
- trash path
- trashed timestamp
- source commit
- restore button
- purge button with confirmation

Empty state:

```text
Trash is empty.
```

Error state:

- Show fetch errors inline below the header.
- Do not hide stale visible entries unless the fetch returned a definite empty list.

## State Model

Frontend state additions:

- `mode: "skills" | "trash"` inside `SkillsPage`.
- `trashEntries: SkillTrashEntry[]`.
- `trashLoading`, `trashError`.
- `selectedTrashId`.
- mutation state via existing `useMutation`.

Refresh rules:

- Fetch trash entries when entering Trash mode.
- Refetch trash entries after trash, restore, or purge.
- Refetch skills after trash or restore.
- Keep selected trash entry if it still exists; otherwise select newest entry.

## Accessibility And Safety

- Trash and purge controls must be reachable by button role and clear accessible names.
- Confirmation buttons must be explicit: `Move to trash`, `Purge forever`, `Cancel`.
- `Purge forever` is never shown without first selecting an entry and opening confirmation.
- API errors are rendered as text, not swallowed.

## Implementation Notes

Likely files:

- `src/panel/mod.rs`: route registration and restore request type.
- `src/panel/handlers/mutations.rs`: trash add, restore, purge handlers.
- `src/panel/handlers/skills.rs` or a small new handler: trash list read route.
- `panel/src/lib/api/client.ts`: trash payload types and API methods.
- `panel/src/pages/panel/SkillsPage.tsx`: segmented Skills/Trash UI, trash detail, confirmations.
- `panel/src/pages/panel/SkillsPage.test.tsx`: user-flow tests.
- `src/panel/tests/handlers.rs`: API route/handler tests.

## Acceptance Criteria

- Panel can list trash entries through `GET /api/v1/skills/trash`.
- From Skills mode, a present source skill can be moved to trash after confirmation.
- From Trash mode, a selected trash entry can be restored by explicit `trash_id`.
- From Trash mode, a selected trash entry can be purged only after confirmation.
- Read-only mode disables trash, restore, and purge actions.
- API errors are visible in the UI.
- Backend tests cover list, restore route, and purge route.
- Frontend tests cover trash mode loading, add confirmation, restore, purge confirmation, and read-only gating.
- `cargo check`, focused Rust tests, `npm run typecheck`, and focused Panel tests pass.

## Follow-Up

- Top-level sidebar count badge for trash entries if users want faster access.
- Bulk purge for old entries after the single-entry workflow proves safe.
- Include trash events in skill history timelines.
