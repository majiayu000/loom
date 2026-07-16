# GH538 Tech Spec - ops list silent zero on git history failure

Issue: https://github.com/majiayu000/loom/issues/538
Product spec: `specs/GH538/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

`src/commands/sync_cmds.rs:88`: `let history = gitops::history_status(&self.ctx).unwrap_or_default();` inside `OpsCommand::List`. A `history_status` error yields default (zero) history data with no signal. Precedent: workspace status surfaces unavailable audit paths as warnings (`tests/reliability.rs:233,292`).

## 2. Proposed Design

1. Replace `unwrap_or_default()` with a match: on error, push a warning into the envelope `meta.warnings` (e.g. `history unavailable: <err>`) and set a `history_degraded: true` field in the payload; keep listing ops.
2. Alternative (if maintainer prefers fail closed): map the error via `map_git` and fail the command. Default recommendation: warning + degraded flag, consistent with read-only listing precedent.
3. Add fault-injection or broken-repo fixture test asserting the warning and flag; add empty-history test asserting no warning.

## 3. Affected Areas

1. `src/commands/sync_cmds.rs`
2. `tests/reliability.rs` (or sibling ops tests)
3. `docs/LOOM_CLI_CONTRACT.md` if the payload gains `history_degraded`

## 4. Output Contract

`ops list --json` gains optional `history_degraded: bool` (default false) and uses existing `meta.warnings` for the failure description.

## 5. Verification Plan

1. New test: forced `history_status` failure -> warning present, `history_degraded == true`.
2. New test: empty registry -> no warning, count 0, flag false/absent.
3. `cargo test --test reliability`
4. `cargo check && cargo test`

## 6. Rollback Plan

Single-file revert; additive JSON field, no state migration.

## 7. Product Mapping

1. Invariant 1 maps to the match-arm warning emission test.
2. Invariant 2-3 map to the success-path and empty-history tests.
