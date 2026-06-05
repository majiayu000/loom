# Panel Design Repair Spec

Date: 2026-06-03
Source audit: [panel-design-audit-2026-06-03/report.md](panel-design-audit-2026-06-03/report.md)

## Linked Issues

| Issue | Title | Priority | PR Plan |
|---|---|---:|---|
| [#203](https://github.com/majiayu000/loom/issues/203) | Style search and filter inputs in the dark UI | P1 | PR 1 |
| [#204](https://github.com/majiayu000/loom/issues/204) | Replace passive empty tables with action-oriented empty states | P1 | PR 1 |
| [#205](https://github.com/majiayu000/loom/issues/205) | Add mobile card layouts for table-heavy pages | P0 | PR 2 |
| [#206](https://github.com/majiayu000/loom/issues/206) | Normalize pending and activity count terminology | P0 | PR 2 |
| [#207](https://github.com/majiayu000/loom/issues/207) | Humanize Activity and Audit log row hierarchy | P1 | PR 3 |
| [#208](https://github.com/majiayu000/loom/issues/208) | Surface observed-skill import from first-run states | P1 | PR 1 |
| [#209](https://github.com/majiayu000/loom/issues/209) | Make Doctor and Settings operational data scannable | P1 | PR 4 |
| [#210](https://github.com/majiayu000/loom/issues/210) | Rebalance typography and sparse-page whitespace | P1 | PR 4 |

## Product Goal

Make the panel understandable when the registry is sparse: users should immediately know what Loom already sees, what Loom is managing, and what action to take next. The first repair should remove the strongest "unfinished UI" signals without changing backend contracts.

## PR 1 Scope

PR 1 addresses the visible first-run polish problems:

- Dark-system search/filter inputs.
- Next-step action buttons with clear click affordance.
- Action-oriented empty states for Skills, Bindings, Projections, and the Overview graph.
- Observed-skill import entry points as explicit navigation/intent, without silently importing anything.

PR 1 deliberately does not change:

- Backend registry schema.
- Pending queue semantics.
- Mobile table-to-card architecture.
- Activity/Audit row data model.
- Doctor/Settings data contracts.

## UX Contract

### Empty State Contract

Every empty state must answer three questions:

1. What is empty?
2. Why is that expected in this state?
3. What is the next safe action?

The empty state may include a CLI command, but the primary action should be a panel action whenever one exists.

### Input Contract

Panel search/filter fields must:

- Use dark background, panel border, and existing typography.
- Show a visible focus state.
- Keep icons and keyboard hints inside the field without overlapping typed text.
- Work at 390px mobile width.

### Action Affordance Contract

Actions inside workflow rows must look clickable before hover:

- Primary next action: filled accent button.
- Secondary actions: bordered button with subtle accent background.
- Disabled action: visible but clearly unavailable, with title explaining why.

### Observed Import Contract

When the registry has observed targets but no managed skills:

- The UI may tell the user Loom sees external skill directories.
- The UI must not claim external skills are already managed.
- The UI must not import observed skills without explicit user action.
- The primary copy should use "Import observed skills" only if the action path exists; otherwise use "Open Skills" plus CLI guidance.

## PR 1 Acceptance Criteria

- Skills and Audit log no longer show native white inputs.
- Skills page shows one useful empty state, not duplicated table/detail empty messages.
- Bindings page empty state asks the user to add a binding, not select a non-existent one.
- Projections page empty state points to creating bindings/projections, not only "No projections found".
- Overview graph empty state includes the next action.
- Overview workflow actions are visually clickable at desktop and mobile widths.
- No existing tests fail:
  - `cd panel && bun run typecheck`
  - `cd panel && bun run test`
  - `cargo test`

## PR 1 Verification

Fresh verification from this branch:

- `cd panel && bun run typecheck` passed.
- `cd panel && bun run test` passed: 7 files, 49 tests.
- `cargo check` passed.
- `cargo test` passed.
- `cd panel && bun run build` passed.
- `cargo build` passed.
- Screenshot pass: [pr1-verification/metadata.json](panel-design-audit-2026-06-03/pr1-verification/metadata.json) covers Overview, Skills, Bindings, Projections, and Audit log at desktop and mobile widths.
- Screenshot DOM checks found 0 light native controls across the PR 1 pages.

## PR 2 Scope Preview

PR 2 should handle the P0 mobile and terminology issues:

- Mobile card layouts for Skills, Bindings, Projections, Activity, Audit log, Settings.
- Exact count labels for pending queue, replayable writes, loaded audit rows, and failed operations.

## PR 3 Scope Preview

PR 3 should improve operation row hierarchy:

- Human labels for intents.
- Secondary placement for operation IDs.
- Grouping or visual differentiation for duplicate-looking target operations.

## PR 4 Scope Preview

PR 4 should handle operational data presentation:

- Doctor check human labels and issue/check count chips.
- Settings path copy/wrap behavior.
- Typography and sparse-state whitespace cleanup.
