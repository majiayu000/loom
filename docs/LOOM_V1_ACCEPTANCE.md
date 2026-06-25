# Loom V1 Acceptance Matrix

Status: Accepted
Date: 2026-05-18
Tracking issue: https://github.com/majiayu000/loom/issues/98

This matrix is the closeout checklist for the V1 core spec. It records the
implemented contract and the verification gates that keep it from regressing.

## Product Scope

| Area | Accepted behavior | Evidence |
| --- | --- | --- |
| Registry | Skills, targets, bindings, rules, projections, operations, observations, and checkpoints are stored in the Git-backed registry state. | `docs/LOOM_STATE_MODEL.md`, `tests/status.rs`, `tests/write.rs` |
| Agent targets | All ten V1 agents are represented across CLI args, default scan paths, status payloads, docs, and Panel settings. | `tests/cli_surface.rs`, `tests/status.rs`, `panel/src/pages/panel/SettingsPage.tsx` |
| Projection | Projection requires an explicit binding and managed target, fails typed when target ownership/agent/method is invalid, and never downgrades methods silently. | `tests/project.rs`, `src/commands/projections.rs` |
| Capture/save | Capture uses explicit selectors, detects drift, restores state on late failures, and reports rollback errors. Save records real registry operations. | `tests/capture.rs`, `tests/write.rs` |
| Snapshot/release/rollback/diff | Snapshot is queryable via V1 ops history without registry `op_id` fabrication; release/rollback write registry ops; rollback creates `recovery_ref`; diff accepts tags and refs. | `tests/reliability.rs`, `tests/project.rs`, `src/panel/skill_diff/tests.rs` |
| Observed workflows | Import and monitor observed targets are explicit and non-destructive; disappearing observed live skills do not delete canonical source. | `tests/import_observed.rs`, `tests/monitor_observed.rs` |
| Orphans | Binding removal marks projections orphaned; cleanup is explicit and optionally deletes live paths only when requested. | `tests/remove.rs`, `src/commands/workspace_cmds/orphan.rs` |
| Sync/history | Push, pull, replay, pending compaction, and `loom-history` reconciliation cover offline and divergent workflows. | `tests/reliability.rs`, `scripts/e2e-agent-flow.sh` |
| Panel | Panel uses live API data for Overview, Skills, Targets, Bindings, Projections, Activity, Sync, Doctor, Settings, and first-run mode. | `panel/src/pages`, `panel/src/lib/api/usePanelData.ts`, `panel/src/pages/PanelApp.test.tsx` |

## Command Contract

Read commands do not mutate registry state, Git refs, Git index, live target
directories, or pending queue. They do append durable command audit events under
`state/events/commands.jsonl`; this is the only accepted read-path write.

Registry writes return `meta.op_id` when a registry operation exists. Noop
writes and non-registry lifecycle actions may omit `meta.op_id` only when the
response explicitly reports noop or the lifecycle event is exposed through the
V1 ops activity model.

## Quality Gates

Required local/CI gate:

```bash
make ci
```

`make ci` runs formatting, clippy, Rust tests, Panel typecheck, Panel tests,
Panel production build, the agent E2E flow, and `make perf-smoke`.

Performance smoke currently enforces:

1. release binary size <= 3200 KiB
2. `loom --version` p95 < 300 ms
3. `loom --help` p95 < 300 ms
4. Panel `.html`/`.css`/`.js` gzip payload <= 100 KiB

Release workflow additionally smoke-tests packaged binaries with `--version`,
`--help`, JSON `workspace status`, and Panel startup.

## U-16 Status

No new oversized Rust file is introduced by V1 acceptance. Existing oversized
modules are tracked as structural split work in
`docs/module-ceiling-signal-report.md`; the report names the files, root cause,
and staged split plan.
