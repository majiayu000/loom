# GH478 Tech Spec - Rollback projection reconciliation

Issue: https://github.com/majiayu000/loom/issues/478
Product spec: `specs/GH478/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

`cmd_rollback` restores source and registry state, then attempts to load a registry snapshot. If loading succeeds, it warns for non-symlink projections. If loading fails, the warning path is skipped. Live copy/materialize projections are not re-applied.

## 2. Proposed Design

1. Replace `if let Ok(Some(snapshot))` with explicit handling for `Ok(Some)`, `Ok(None)`, and `Err`.
2. Return a structured `projection_reconciliation` object in rollback output.
3. Default behavior should be recovery-plan-only unless maintainers approve automatic live writes.
4. If automatic reconciliation is added, restrict it to Loom-owned projection paths under registered targets and reuse existing projection write helpers.
5. Keep dry-run/preview read-only and include projected reconciliation impact.

## 3. Affected Areas

1. `src/commands/version_cmds.rs`: rollback post-processing and output envelope.
2. `src/commands/projections.rs`: safe re-project helper reuse if auto reconciliation is chosen.
3. `src/commands/file_ops.rs`: live path safety checks if auto writes are introduced.
4. `tests/project.rs`, `tests/rollback_preview.rs`: rollback coverage.
5. `docs/LOOM_CLI_CONTRACT.md`: rollback semantics.

## 4. Output Contract

Rollback output should include:

1. `source_restored`
2. `registry_restored`
3. `projection_reconciliation.status`
4. `projection_reconciliation.items[]` with `instance_id`, `skill_id`,
   `target_id`, `materialized_path`, `method`, `status`, and
   `requires_projection_reapply`
5. `projection_reconciliation.next_actions[]` with exact executable commands
   when a CLI recovery command exists, or an explicit `manual_review_required`
   action when no safe command exists

Snapshot read failure must appear in `error.details` or `meta.warnings`.

## 5. Verification Plan

1. `cargo test rollback_preview`
2. `cargo test project`
3. `cargo check`
4. `cargo test`

## 6. Rollback Plan

If automatic reconciliation is implemented and fails in production, fall back to recovery-plan-only while preserving explicit stale projection reporting.

## 7. Product Mapping

1. Product invariant 1 maps to separate restored/reconciled output fields.
2. Product invariant 2 maps to `requires_projection_reapply` or equivalent.
3. Product invariant 3 maps to snapshot error tests.
4. Product invariant 4 maps to symlink-specific tests.
