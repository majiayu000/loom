# GH537 Tech Spec - Structured error contract hardening

Issue: https://github.com/majiayu000/loom/issues/537
Product spec: `specs/GH537/product.md`
Route: `write_spec`
Human gate: maintainer decisions approved on 2026-07-16

## 1. Current Behavior

`src/main.rs:60-64/68-71/82-85` print bare `eprintln!` and `exit(3)` for app-init, panel, and top-level errors — no envelope even with `--json`. `src/error_actions.rs:18-42` covers 5 of 29 codes, wildcard returns empty. `src/types.rs:73-105` maps 20 codes to exit 3; adding `INIT_ERROR` makes the totals 30 codes / 21 at exit 3 without re-tiering existing codes.

## 2. Proposed Design

1. Add `ErrorCode::InitError` mapped to `INIT_ERROR` / exit 3. In `main.rs`, when `cli.json` is set, emit `cmd: "app.init"` + `INIT_ERROR` for `App::new` failure; panel/top-level failures emit structured envelopes with their actual/stable command identity and appropriate existing error code. Keep stderr text for human mode.
2. Extend `default_next_actions` only for universal no-argument actions (`REMOTE_*` → `loom sync status --json`, `LOCK_BUSY` → `loom ops list --json`). Conflict/policy call sites provide contextual actions when possible; every remaining code appears in a documented exemption table. Add a table-driven totality test.
3. Document existing exit-code tiers without reordering and declare `error.code` the sole stable semantic routing key in `docs/LOOM_CLI_CONTRACT.md`.

## 3. Affected Areas

1. `src/main.rs`
2. `src/error_actions.rs`
3. `src/types.rs` (`INIT_ERROR` only; no exit-code re-tiering)
4. `docs/LOOM_CLI_CONTRACT.md`
5. `tests/` (new coverage test; `tests/cli_surface.rs` error-code table)

## 4. Output Contract

Init failure with `--json`: `{ok:false, cmd:"app.init", error:{code:"INIT_ERROR",…}, …}` on stdout, exit 3. Human mode unchanged.

## 5. Verification Plan

1. Integration test: launch the binary with `HOME` and `USERPROFILE` removed, omit `--root`, pass `--json`, and assert stdout parses as an `app.init` / `INIT_ERROR` envelope. An unwritable explicit root does not trigger `App::new` and is not a valid fixture for this path.
2. `cargo test error_actions`
3. Contract doc test in `cli_surface`
4. `cargo check && cargo test`

## 6. Rollback Plan

Changes are additive at the output boundary; revert restores bare stderr behavior without state impact.

## 7. Product Mapping

1. Invariant 1 maps to the main.rs envelope emission test.
2. Goal 2 maps to the table-driven coverage test.
3. Goal 3 maps to the contract doc update and its existing drift gate.
