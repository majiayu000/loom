# GH513 Task Plan: Non-overlapping Operation Counts

Issue: https://github.com/majiayu000/loom/issues/513
Product spec: `specs/GH513/product.md`
Tech spec: `specs/GH513/tech.md`
Status: Implementation verified; remote merge gate pending under current `implx auto` authorization

## Scope

以共享四 bucket classifier 修复 local-only false backlog，并让 CLI、doctor、Panel API/UI 与迁移文档使用一致语义。

不删除历史，不新增 persisted schema，不在 read path fetch，不恢复 `pending_ops`，不发布 release。

## Tasks

- [x] `SP513-T1` — Owner: coordinator; Done when: regressions reproduce local-only false backlog and distinguish remote/event-set cases; Verify: focused Rust/Panel tests fail on pre-fix behavior.
- [x] `SP513-T2` — Owner: coordinator; Done when: shared Rust journal/history classifier and CLI/doctor/API projections satisfy P1-P7; Verify: focused Rust tests.
- [x] `SP513-T3` — Owner: coordinator; Done when: Panel types/adapters/view model/pages and docs satisfy P5/P8; Verify: Panel tests/build and contract assertions.
- [x] `SP513-T4` — Owner: verification_owner; Done when: fresh deterministic checks and full suite pass; Verify: commands below.
- [ ] `SP513-T5` — Owner: gh513-merge-reviewer; Done when: independent implementation review, current-head CI, review threads and PR gate pass; Verify: current-head remote evidence.

### SP513-T1: Lock The Classification Contract

Owner: coordinator

Files:

- `tests/reliability.rs`
- `tests/status.rs`
- `tests/doctor.rs`
- `src/panel/tests/handlers.rs`
- relevant `panel/src/**/*.test.tsx`

Done when:

- Healthy local-only fixture proves succeeded rows/history are visible but actionable is zero.
- Remote fixture proves succeeded row becomes actionable and local/remote history uses event-ID set difference.
- Failed, acked, purged, unknown-status, duplicate-ID and malformed-body edges are covered.
- Tests assert compatibility aliases and actionable-only row arrays across CLI, doctor and Panel API.

Verify:

```bash
cargo test --test reliability --test status --test doctor
cargo test panel::tests::handlers
npm --prefix panel test -- --run
```

### SP513-T2: Implement Shared Rust Counts

Owner: coordinator
Depends on: SP513-T1

Files:

- `src/state/registry_ops.rs`
- `src/state/journal.rs`
- `src/gitops/history.rs`
- `src/gitops/history_types.rs`
- `src/commands/projections.rs`
- `src/commands/sync_cmds.rs`
- `src/commands/workspace_cmds/status.rs`
- `src/commands/workspace_cmds/doctor.rs`
- `src/commands/agent_cmds.rs`
- `src/panel/handlers/ops.rs`

Done when:

- One classifier produces mutually exclusive `operation_counts` and actionable rows.
- History uses unique event-ID sets from local and cached remote archives/segments.
- Read/parsing errors fail closed and no status/list/doctor/API read fetches remote refs.
- Compatibility aliases map exactly to the new counters and `pending_ops` remains absent.
- Existing sync/ack behavior transitions succeeded rows when a remote is configured.

Verify:

```bash
cargo test --test reliability --test status --test doctor
cargo test panel::tests::handlers
! rg -n 'pending_ops' src panel/src
```

### SP513-T3: Align Panel And Contracts

Owner: coordinator
Depends on: SP513-T2

Files:

- `panel/src/types.ts`
- `panel/src/lib/api/usePanelData.ts`
- `panel/src/lib/api/adapters.ts`
- `panel/src/lib/panel_view_model.ts`
- `panel/src/pages/PanelApp.tsx`
- `panel/src/pages/panel/OverviewPage.tsx`
- `panel/src/pages/panel/SyncPage.tsx`
- `panel/src/pages/SkillMPanel.tsx`
- related Panel tests
- `docs/LOOM_API_CONTRACT.md`
- `docs/LOOM_CLI_CONTRACT.md`
- `docs/LOOM_STATE_MIGRATION_NOTES.md`
- `CHANGELOG.md`

Done when:

- Panel uses `RegistryOperationRecord` rows and typed four-bucket counts without legacy shape coercion.
- LOCAL_ONLY with zero actionable is informational; each bucket is visibly labeled.
- `SkillMPanel.tsx` remains at or below 800 lines.
- API/CLI/migration docs define aliases, example `0 / 3 / 0 / 400`, and cached-ref limitations.

Verify:

```bash
npm --prefix panel test -- --run
npm --prefix panel run build
wc -l panel/src/pages/SkillMPanel.tsx
rg -n 'actionable_operations|local_journal_events|unpushed_history_events|local_only_history_events' docs panel/src
```

### SP513-T4: Deterministic Verification

Owner: verification_owner
Depends on: SP513-T1, SP513-T2, SP513-T3

Done when:

- All fresh checks pass and large output is stored under `artifacts/logs/2026-07-14-loom-queue-t02`.

Verify:

```bash
git diff --check
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo test
npm --prefix panel test -- --run
npm --prefix panel run build
make check
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/implx-GH513-operation-counts/specs/GH513
```

### SP513-T5: Independent Review And Merge Gate

Owner: gh513-merge-reviewer
Depends on: SP513-T4

Done when:

- Independent reviewer maps P1-P8 and SP513-T1-T4 to final diff and evidence.
- Current-head CI passes, GraphQL review threads are resolved, merge state is clean, and offline PR gate returns `allowed`.
- Merge is remotely confirmed, Issue #513 closes, and remote implementation branch is deleted separately.

## Handoff Notes

- `operation_counts` is the canonical model; aliases are projections, not independent calculations.
- Compare unique event-ID sets, never path counts or total-count subtraction.
- `LOCAL_ONLY` does not hide failures and does not trigger network access from read commands.
- The current user invocation is `implx auto`, so this run carries standing merge authorization after all SpecRail gates pass. It does not authorize publishing a release.
