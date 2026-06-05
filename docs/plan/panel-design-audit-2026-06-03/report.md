# Loom Panel Design Screenshot Audit

Date: 2026-06-03
Target: http://127.0.0.1:43117/
State audited: current local registry, 0 managed skills, 2 observed targets, pending activity present.

## Screenshot Index

| Page | Desktop | Mobile |
|---|---|---|
| Overview | [desktop-overview.png](screenshots/desktop-overview.png) | [mobile-overview.png](screenshots/mobile-overview.png) |
| Skills | [desktop-skills.png](screenshots/desktop-skills.png) | [mobile-skills.png](screenshots/mobile-skills.png) |
| Targets | [desktop-targets.png](screenshots/desktop-targets.png) | [mobile-targets.png](screenshots/mobile-targets.png) |
| Bindings | [desktop-bindings.png](screenshots/desktop-bindings.png) | [mobile-bindings.png](screenshots/mobile-bindings.png) |
| Projections | [desktop-projections.png](screenshots/desktop-projections.png) | [mobile-projections.png](screenshots/mobile-projections.png) |
| Activity | [desktop-ops.png](screenshots/desktop-ops.png) | [mobile-ops.png](screenshots/mobile-ops.png) |
| Audit log | [desktop-history.png](screenshots/desktop-history.png) | [mobile-history.png](screenshots/mobile-history.png) |
| Git sync | [desktop-sync.png](screenshots/desktop-sync.png) | [mobile-sync.png](screenshots/mobile-sync.png) |
| Doctor | [desktop-doctor.png](screenshots/desktop-doctor.png) | [mobile-doctor.png](screenshots/mobile-doctor.png) |
| Settings | [desktop-settings.png](screenshots/desktop-settings.png) | [mobile-settings.png](screenshots/mobile-settings.png) |

Raw capture metadata: [screenshot-metadata.json](screenshot-metadata.json)

## Overall Verdict

Design score: C on desktop, D on mobile.

The panel has a coherent dark operational shell and the navigation is understandable, but many pages still feel like developer tables placed in a UI shell rather than finished operator screens. The largest issues are weak empty states, inconsistent action affordances, unstyled native inputs, confusing pending counts, and mobile layouts that compress desktop tables instead of changing information shape.

## Highest Priority Findings

1. Inputs are not styled in the panel system.
   Evidence: Skills and Audit log show default white browser inputs on a dark interface.
   Impact: High. It immediately makes the product feel unfinished and breaks the visual system.
   Fix: Add a shared `.field` / `.search-field` style and apply it to filter inputs, including shortcut hints.

2. Empty states are mostly passive text, not workflow guidance.
   Evidence: Skills, Bindings, Projections, and Overview projection graph.
   Impact: High. In the current 0-skill state, users need "what do I do next", but several pages say only "none found" or "select one".
   Fix: Replace empty tables with one empty-state panel per page: short reason, primary action, secondary CLI command.

3. Mobile layouts are not designed; they are clipped desktop layouts.
   Evidence: Skills, Bindings, Projections, Activity, Audit log, Settings mobile screenshots.
   Impact: High. Tables lose columns, long IDs wrap badly, paths are clipped, and the horizontal nav hides later sections.
   Fix: Use mobile card rows for table pages, collapse side/detail panes, and provide an explicit horizontal nav affordance or mobile menu.

4. Pending counts mean different things in different places.
   Evidence: topbar shows `2 pending`, nav Activity shows `4`, Overview shows `4 pending`, Activity shows `6 tracked changes / 4 pending`, History shows `4 loaded / 2 pending`.
   Impact: High. Operators cannot tell whether "pending" means queue items, loaded audit rows, replayable changes, or UI filter count.
   Fix: Split labels into exact terms: `Pending queue`, `Loaded changes`, `Replayable writes`, and use the same source everywhere.

5. Several pages show duplicate empty messages.
   Evidence: Skills and Projections display an empty message in the table and another empty message in the side/detail region.
   Impact: Medium. It looks accidental and wastes attention.
   Fix: If no row exists, hide the detail pane and use one centered empty state.

6. Some click targets still read as text or low-emphasis controls.
   Evidence: older Overview action buttons were ambiguous; Projections filter buttons and icon/text action clusters vary in visual strength.
   Impact: Medium. Users scan for the next action and miss low-contrast controls.
   Fix: Keep one primary action per page in orange; secondary actions should have consistent bordered button treatment.

7. The typography choice is too editorial for dense operational tables.
   Evidence: large serif numerals/headings in KPI cards and state tiles.
   Impact: Medium. It gives personality, but in ops-heavy pages it competes with data readability.
   Fix: Keep the serif display for page titles only; use a more utilitarian numeric style for KPIs and status cards.

8. Large dead zones make sparse pages feel broken.
   Evidence: Targets, Bindings, Projections, Skills desktop screenshots.
   Impact: Medium. Empty space is not intentionally used; it reads like missing content.
   Fix: In sparse states, pull next-step cards, import suggestions, or recent activity into the empty area.

## Page-by-Page Findings

### Overview

- The Next steps section is the strongest workflow surface, but it competes with topbar actions and page-header actions that repeat the same commands.
- The empty projection graph takes a large area and only says "No projections yet"; it should point to "Add binding" or "Replay / sync".
- The pending count is inconsistent with the topbar and Activity page.
- On mobile, the row labels wrap awkwardly (`Add a binding`, `Apply projections`) and the action column squeezes the text column.

Recommended changes:
- Keep one primary CTA in the page header; move repeated actions into contextual rows.
- Add a projection empty state inside the graph area with the next command.
- For mobile, stack next-step status, title, detail, and action vertically.

### Skills

- The search input is a default white browser input and visually breaks the dark panel.
- The `⌘K` hint collides with the input area on both desktop and mobile.
- The empty state is duplicated: once in the table and once in the detail pane.
- The page says "No skills in this registry", but the app knows there are many observed source skills elsewhere. That is technically true but operationally unhelpful.
- On mobile, the table header and empty copy are clipped; this is the weakest page visually.

Recommended changes:
- Style search as a dark field with integrated icon and shortcut chip.
- Replace the table/detail empty layout with a single onboarding empty state: "Import observed skills" and "Add one skill".
- On mobile, use cards instead of table columns.

### Targets

- The two target cards are readable and the observed badge works.
- The page leaves a very large unused area below the cards.
- `0 skills present` can be misleading because the overview reports many source skills; clarify whether this means managed skills, live skills, or imported skills.
- The primary action is clear, but there is no obvious "import from these observed targets" action.

Recommended changes:
- Add a second panel under target cards: "Observed skills found" with import action.
- Rename `0 skills present` to the exact data source, such as `0 managed projections`.

### Bindings

- Empty state tells users to "Select a binding" even when no binding exists.
- The `New binding` button is clear, but the main body does not reinforce it.
- Table headers occupy space even when the table has no rows.
- On mobile, the nav and table columns are clipped; the screen mostly shows an empty table.

Recommended changes:
- Hide empty table headers when no bindings exist.
- Show an empty-state card with "Add a binding" and prerequisites.
- Mobile should show a single card explaining the binding concept with a primary CTA.

### Projections

- Filter pills are visible, but the page has no useful next step when all counts are zero.
- Empty text is duplicated between the table body and the detail pane.
- The label "Materialized skill instances" may be too narrow because projections can be symlink/copy/materialize elsewhere in the app.
- On mobile, only part of the table header is visible and the rest of the page is blank.

Recommended changes:
- Change subtitle to "Live skill instances across targets".
- Replace duplicate empty messages with one action-oriented empty state.
- Convert projection rows to cards on mobile.

### Activity

- The Activity page is the most operationally useful page, but the counts are hard to reconcile.
- Rows repeat `target add` multiple times with tiny IDs; the duplicate-looking events make it unclear what is actually pending.
- The summary card `Pending 4` says `target add target.add -> —`, which reads like raw debug text.
- On mobile, operation IDs wrap into multiple lines and the row hierarchy becomes noisy.

Recommended changes:
- Group duplicate operations by target and status.
- Use human labels: `Claude target registration pending`, not raw `target.add ->`.
- Show technical IDs in a secondary disclosure or tooltip, not as primary row text.

### Audit Log

- The filter input is an unstyled white browser input.
- Full timestamps are too long and low-contrast, making the table hard to scan.
- The table prioritizes raw change IDs over user-meaningful events.
- On mobile, most columns disappear; users see mainly change IDs and `target.add`.

Recommended changes:
- Style filter input.
- Use relative time in the main table and put exact timestamps in detail.
- Make intent/status/time primary; move IDs to secondary text.
- Use mobile event cards.

### Git Sync

- The page has a clear structure: state, remote, repair, actions.
- `LOCAL_ONLY` is too visually dominant on desktop and is clipped to `LOC` on mobile.
- Remote setup and replay pending are competing tasks; replay is primary now, but remote setup may be the real next step.
- The actions row uses mixed icon-only and filled buttons without a clear order of operations.

Recommended changes:
- Treat sync state as a badge or compact status, not a huge display word.
- On mobile, avoid all-caps state words in KPI cards.
- Separate "Set remote" and "Replay pending" into two explicit task cards.

### Doctor

- Health summary is clear and reassuring.
- Check rows expose internal IDs (`git_fsck`, `schema_file`, `target_path_exists:...`) as visible primary labels.
- The category score chips (`0 / 1`, `0 / 2`) are ambiguous; users need to know whether this means failures or warnings.
- Mobile rows split labels and descriptions into narrow columns, making normal sentences wrap badly.

Recommended changes:
- Use human labels first, internal check IDs second.
- Rename category chips to `0 issues` / `2 checks`.
- On mobile, stack each check row vertically.

### Settings

- Registry paths are useful, but the page is mostly a raw key/value dump.
- Long paths are clipped on mobile.
- There are no copy buttons for paths.
- UI preferences are below the fold and represented by explanatory text rather than controls.

Recommended changes:
- Add copy buttons for important paths.
- Wrap or horizontally scroll path values deliberately.
- Convert UI preferences into actual controls, or move the explanatory text out of the main settings card.

## Quick Wins

1. Style all inputs and shortcut hints.
2. Replace duplicated empty table/detail panes with one action-oriented empty state.
3. Convert table pages to mobile cards under 700px.
4. Normalize pending/count labels across topbar, nav, Overview, Activity, and Audit log.
5. Add "Import observed skills" as a visible path from Overview, Skills, and Targets.
6. Hide technical IDs by default on Activity and Audit log; show them as secondary details.

## Suggested Next Fix Order

1. Skills page empty state and input styling.
2. Global mobile table/card pattern.
3. Pending terminology cleanup.
4. Overview projection empty state and mobile next-step stacking.
5. Activity/Audit log row hierarchy cleanup.
6. Settings path copy/wrap behavior.
