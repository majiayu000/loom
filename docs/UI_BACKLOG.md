# Loom UI Backlog

Status date: 2026-04-24
Scope: `panel/` landing + registry panel

This backlog converts the current UI completion audit into implementation work.
Priority is driven by product leverage, user trust, and closeness to existing code.

## P0

### 1. Make Ops a real control surface

Problem:
`Ops` looks actionable, but the main actions are not wired. This creates false affordance and leaves the operator stuck in the CLI for the very workflow the panel implies it owns.

Current surface:
- [panel/src/pages/panel/OpsPage.tsx](../panel/src/pages/panel/OpsPage.tsx)
- [panel/src/components/panel/OpRow.tsx](../panel/src/components/panel/OpRow.tsx)

Needed backend/API support:
- Add panel endpoints for:
  - `ops retry`
  - `ops purge`
  - `ops history diagnose`
  - `ops history repair`
- Expose enough op metadata to filter by status, skill, target, command kind.

UI work:
- Wire `Retry failed`.
- Wire `Purge completed`.
- Add `Diagnose history` and `Repair history` actions.
- Add filters:
  - status
  - skill
  - target
  - command kind
- Add per-op detail drawer or expandable row.

Acceptance:
- A failed op can be retried from the panel.
- Completed ops can be purged from the panel.
- Diagnose/repair workflows run without leaving the UI.
- All action results show inline success/error feedback.

### 2. Replace fake Overview stats with authoritative state

Problem:
The page mixes real data with hard-coded KPIs and static operational text. That weakens user trust because the screen looks live but parts of it are decorative.

Current surface:
- [panel/src/pages/panel/OverviewPage.tsx](../panel/src/pages/panel/OverviewPage.tsx)
- [panel/src/lib/api/usePanelData.ts](../panel/src/lib/api/usePanelData.ts)

Fake or illustrative areas to remove:
- `+2 · 52 captures · 38 releases`
- `6 agents · 4 profiles`
- static git HEAD text
- static "Last pull from origin/main · 6h ago"

Needed backend/API support:
- Surface:
  - registered agent count
  - profile count
  - release/snapshot/capture counts if available
  - git head / branch / status
  - last successful sync pull / push timestamp
  - write-guard state

UI work:
- Convert KPI cards to only render fields backed by API data.
- If a metric is unavailable, show `—` with a tooltip or helper text.
- Split "write guard" card into:
  - registry root
  - git state
  - write safety
  - remote sync state

Acceptance:
- No decorative metrics remain on Overview.
- Every rendered status comes from a real backend field.
- Missing fields degrade gracefully rather than fabricating numbers.

### 3. Build the real Sync page

Problem:
Sync exists as actions in top-level buttons, but not as a dedicated operational page.

Current surface:
- [panel/src/pages/panel/PlaceholderPage.tsx](../panel/src/pages/panel/PlaceholderPage.tsx)
- [panel/src/components/panel/Sidebar.tsx](../panel/src/components/panel/Sidebar.tsx)
- [panel/src/components/panel/Topbar.tsx](../panel/src/components/panel/Topbar.tsx)

Needed backend/API support:
- Rich remote status payload:
  - remote URL
  - branch
  - sync state
  - ahead/behind if available
  - last push / pull
  - pending queue count
  - divergence/conflict summary

UI work:
- Replace `sync` placeholder page with:
  - remote summary
  - push / pull / replay actions
  - conflict and divergence state
  - recent sync events
  - warnings panel

Acceptance:
- Operators can understand remote sync state from a single page.
- The page explains whether the registry is clean, pending push, local only, or diverged.

## P1

### 4. Build the real History page

Problem:
Audit/history is central to Loom’s value proposition, but the UI still routes this area to a placeholder.

Current surface:
- [panel/src/pages/panel/PlaceholderPage.tsx](../panel/src/pages/panel/PlaceholderPage.tsx)

Needed backend/API support:
- Read-only history timeline endpoint or payload including:
  - op id
  - intent
  - status
  - created/updated time
  - affected skill / binding / target
  - repair/archive events

UI work:
- Timeline view with filters.
- Detail drawer for each history record.
- Jump links from history entry to skill, binding, or target page.

Acceptance:
- History is browseable and filterable.
- Operators can move from history event to affected object in one click.

### 5. Turn Topbar search into command palette

Problem:
The search box visually promises global navigation, but currently has no behavior.

Current surface:
- [panel/src/components/panel/Topbar.tsx](../panel/src/components/panel/Topbar.tsx)

UI work:
- Add `⌘K` / `Ctrl+K` command palette.
- Search across:
  - skills
  - targets
  - bindings
  - pages
  - common actions
- Action entries:
  - sync pull
  - sync push
  - sync replay
  - target add
  - binding add

Acceptance:
- Typing a skill name jumps to its detail.
- Typing a target jumps to the target page and selects it.
- Common commands run from the palette.

### 6. Replace illustrative Skill lifecycle with real history

Problem:
The detail panel is structurally right, but the lifecycle timeline is synthetic.

Current surface:
- [panel/src/pages/panel/SkillsPage.tsx](../panel/src/pages/panel/SkillsPage.tsx)

Needed backend/API support:
- Per-skill lifecycle/history payload including:
  - capture
  - save
  - snapshot
  - release
  - rollback
  - project

UI work:
- Replace `lifecycleFor(skill)` fake events with API-backed events.
- Add projection health and drift summary for each skill.
- Add quick links to related bindings and targets.

Acceptance:
- Skill detail shows only real lifecycle events.
- Users can trace a skill from revision to projected targets.

### 7. Add edit/remove flows for Targets and Bindings

Problem:
The panel currently supports creation but not full management. This forces a fallback to CLI for normal cleanup/edit cases.

Current surface:
- [panel/src/pages/panel/TargetsPage.tsx](../panel/src/pages/panel/TargetsPage.tsx)
- [panel/src/pages/panel/BindingsPage.tsx](../panel/src/pages/panel/BindingsPage.tsx)

Needed backend/API support:
- Update and remove APIs where missing.

UI work:
- Add row/card actions:
  - edit
  - remove
  - copy id/path
- Add confirmation modals for destructive actions.
- Show dependency warnings before deletion.

Acceptance:
- Targets and bindings can be fully managed from the panel.
- Destructive actions require explicit confirmation and show blockers.

## P2

### 8. Improve state honesty across the panel

Problem:
The UI still relies on subtle placeholders in areas where the user expects exact operational truth.

UI work:
- Add explicit badges for:
  - live
  - mock
  - read-only
  - partial data
- Add "source of truth" helper text in pages that mix derived and direct values.
- Standardize success/error/pending banners across all pages.

Acceptance:
- A user can always tell whether the page is showing real registry data or fallback mock data.

### 9. Tighten visual hierarchy in panel pages

Problem:
The visual language is coherent, but too many controls compete equally for attention.

UI work:
- Reduce decorative KPI metadata.
- Keep one primary CTA per page.
- Normalize card header density and spacing.
- Simplify legend copy in the projection graph.
- Reduce inline style sprawl by moving repeated presentation into CSS classes.

Acceptance:
- Primary actions are obvious.
- Secondary actions are visually quieter.
- Repeated layout styles are consolidated.

### 10. Upgrade landing page for conversion, not just explanation

Problem:
The landing page explains Loom well, but it still leans too heavily on feature description instead of proof.

Current surface:
- [panel/src/pages/LandingPage.tsx](../panel/src/pages/LandingPage.tsx)
- [panel/src/components/landing/Hero.tsx](../panel/src/components/landing/Hero.tsx)
- [panel/src/components/landing/Features.tsx](../panel/src/components/landing/Features.tsx)

UI/content work:
- Add a real panel screenshot.
- Add a short "30-second path" section:
  - install
  - init registry
  - add target
  - open panel
- Add before/after examples:
  - unmanaged multi-agent directories
  - Loom-managed projection flow
- Add social proof or "why this exists" with a more concrete pain story.

Acceptance:
- First-time visitors can understand the product from one scroll.
- The page demonstrates proof, not only claims.

## Suggested implementation order

1. Ops page
2. Overview truthfulness cleanup
3. Sync page
4. History page
5. Command palette
6. Real skill lifecycle
7. Edit/remove flows
8. Visual tightening
9. Landing conversion pass

## Notes

- Do not add new decorative analytics unless the backend exposes them.
- Prefer "missing" states over synthetic values.
- When a page is not fully wired, say so explicitly in the UI instead of implying control that does not exist.
