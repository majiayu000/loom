# Signal Report: PR #20 / Issue #19

## Scope

Review the existing `feat/panel-state-workflow-honesty` branch against issue #19 and the open PR #20 review comments, then close any remaining correctness and honesty gaps without widening scope.

## Evidence Reviewed

- `gh issue view 19`
- `gh pr diff 20`
- `gh api repos/majiayu000/loom/pulls/20/comments`
- `gh api repos/majiayu000/loom/pulls/20/reviews`
- Current branch tip: `d2e1c7b`

## Confirmed Root Causes

1. `HistoryPage` only refreshes on `live` and local `mutationVersion`.
   Root cause: the page is disconnected from the panel-wide polling signal, so CLI-created or external-session operations do not appear until navigation or a local mutation triggers a fetch.

2. `/api/registry/ops` returns the entire `operations.jsonl` journal with full row payloads.
   Root cause: the handler serializes `snapshot.operations` directly, which makes response cost grow with registry age and with each row's `payload` / `effects` size.

3. `BindingsPage` clears selection with an unconditional stale callback after delete.
   Root cause: the remove-success path uses `setSelectedId(null)` rather than clearing only if the deleted binding is still the active selection, so a later user selection can be wiped out when the earlier request resolves.

## Already Fixed On Branch

- `"succeeded"` now classifies as success in `HistoryPage.bucket`.
- `LiveDataBanner` no longer renders a false offline banner during `mode === "live"` refreshes.
- `TargetsPage` detail fetches already track `mutationVersion`.
- `TargetsPage` delete flow already clears selection only for the removed target id.

## Planned Fixes

1. Bind `HistoryPage` to the existing panel-wide refresh stamp and keep mutation-triggered refreshes.
2. Paginate and bound `/api/registry/ops`, returning compact row summaries instead of full operation payload blobs.
3. Add minimal pager UI in History so the full journal remains reachable.
4. Guard binding delete completion the same way targets already guard delete completion.
5. Extend tests around history refresh, ops pagination, and stale-selection races.
