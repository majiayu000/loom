# GH534 Tech Spec - Module size ceiling guard automation

Issue: https://github.com/majiayu000/loom/issues/534
Product spec: `specs/GH534/product.md`
Route: `write_spec`
Human gate: maintainer decisions approved on 2026-07-16

## 1. Current Behavior

`docs/module-ceiling-signal-report.md` manually tracks a 600-line ceiling for 3 files fixed in PR #59. No line-count check exists in `build.rs`, `Makefile`, `scripts/`, or `.github/workflows/`. With `tests/` paths and `*_tests.rs` excluded, 21 production files exceed 700 lines at `bb9b738`: 3 above 800 and 18 in the warning band.

## 2. Proposed Design

1. Add `scripts/module-ceiling.sh`: enumerate non-test `src/**/*.rs`, use `awk` record count so an unterminated final line is included, compare the complete physical-file size against the 800 hard ceiling, and warn for files above 700. Exclude test-only paths/files, but do not subtract inline `#[cfg(test)]` blocks from production files. Reject Rust-file and production source-directory symlinks rather than silently omitting them.
2. Allowlist file (e.g. `scripts/module-ceiling-allowlist.txt`): `path<TAB>baseline_lines<TAB>issue-ref` entries. The initial entries are `src/commands/mcp/apply.rs` → #544, `src/commands/mcp.rs` → #545, and `src/commands/skillset_activation.rs` → #546. While a file remains above 800, its current size must exactly equal the reviewed baseline; both increases and decreases fail until the baseline is explicitly reviewed and updated. Entries at 800 or below are stale and must be removed.
3. Wire into `Makefile` (own target) and CI `verify` job after lint.
4. Refresh `docs/module-ceiling-signal-report.md` to describe the guard, current allowlist, and split queue.

## 3. Affected Areas

1. `scripts/` (new guard script + allowlist)
2. `Makefile`
3. `.github/workflows/ci.yml`
4. `docs/module-ceiling-signal-report.md`

## 4. Output Contract

Guard output prints one line per warning/violation: `level path current_lines ceiling [baseline issue-ref]`; it exits non-zero on any non-allowlisted file above 800, allowlisted baseline mismatch, malformed or stale entry, missing file, or unsupported source symlink.

## 5. Verification Plan

1. Run guard locally on current tree: passes with 3 files allowlisted and reports the 700–800 warning band.
2. Negative test: temporary oversized file causes non-zero exit.
3. Stale allowlist entry (missing file) causes non-zero exit.
4. Baseline decrease/growth, an unterminated 801st line, and a Rust symlink each cause non-zero exit.
5. CI green on the wired pipeline.

## 6. Rollback Plan

Remove the Makefile/CI hook; script and allowlist are inert without wiring.

## 7. Product Mapping

1. Invariant 1-2 map to the script's file filter and allowlist format checks.
2. Invariant 3 maps to the output contract.
3. Invariant 4 maps to the shared Makefile target used by CI and local runs.
