# GH537 Tech Spec - Structured error contract hardening

Issue: https://github.com/majiayu000/loom/issues/537
Product spec: `specs/GH537/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

`src/main.rs:60-64/68-71/82-85` print bare `eprintln!` and `exit(3)` for app-init, panel, and top-level errors — no envelope even with `--json`. `src/error_actions.rs:18-42` covers 5 codes, wildcard returns empty. `src/types.rs:73-105` maps ~18 codes to exit 3.

## 2. Proposed Design

1. In `main.rs`, when `cli.json` is set, build `Envelope::err_with_next_actions` for init/panel/top-level failures and print via `print_envelope` before exiting; keep stderr text for human mode.
2. Extend `default_next_actions` to cover conflict/policy/remote classes; add a table-driven test asserting every contract error code has either an action or a documented exemption list entry.
3. Document exit-code tiers and declare `error.code` the routing key in `docs/LOOM_CLI_CONTRACT.md` (preferred over re-tiering, pending maintainer decision).

## 3. Affected Areas

1. `src/main.rs`
2. `src/error_actions.rs`
3. `src/types.rs` (only if re-tiering chosen)
4. `docs/LOOM_CLI_CONTRACT.md`
5. `tests/` (new coverage test; `tests/cli_surface.rs` error-code table)

## 4. Output Contract

Init failure with `--json`: `{ok:false, cmd:"init-failure"|actual, error:{code,…}, …}` on stdout, non-zero exit. Human mode unchanged.

## 5. Verification Plan

1. Integration test: run binary with unwritable/broken root + `--json`, assert stdout parses as envelope with expected code.
2. `cargo test error_actions`
3. Contract doc test in `cli_surface`
4. `cargo check && cargo test`

## 6. Rollback Plan

Changes are additive at the output boundary; revert restores bare stderr behavior without state impact.

## 7. Product Mapping

1. Invariant 1 maps to the main.rs envelope emission test.
2. Goal 2 maps to the table-driven coverage test.
3. Goal 3 maps to the contract doc update and its existing drift gate.
