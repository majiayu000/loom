# GH534 Tech Spec - Module size ceiling guard automation

Issue: https://github.com/majiayu000/loom/issues/534
Product spec: `specs/GH534/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

`docs/module-ceiling-signal-report.md` manually tracks a 600-line ceiling for 3 files fixed in PR #59. No line-count check exists in `build.rs`, `Makefile`, `scripts/`, or `.github/workflows/`. 23 non-test files exceed 700 lines at `7ff1b34`.

## 2. Proposed Design

1. Add `scripts/module-ceiling.sh`: enumerate non-test `src/**/*.rs`, compare `wc -l` against the ceiling.
2. Allowlist file (e.g. `scripts/module-ceiling-allowlist.txt`): `path<TAB>issue-ref` entries; allowlisted files report but do not fail; a ratchet check fails if an allowlisted file grows beyond its recorded baseline.
3. Wire into `Makefile` (own target) and CI `verify` job after lint.
4. Refresh `docs/module-ceiling-signal-report.md` to describe the guard, current allowlist, and split queue.

## 3. Affected Areas

1. `scripts/` (new guard script + allowlist)
2. `Makefile`
3. `.github/workflows/ci.yml`
4. `docs/module-ceiling-signal-report.md`

## 4. Output Contract

Guard failure prints one line per violation: `path current_lines ceiling [allowlist-issue]`, exit non-zero on any non-allowlisted violation or stale allowlist entry.

## 5. Verification Plan

1. Run guard locally on current tree: passes with the 23 files allowlisted.
2. Negative test: temporary oversized file causes non-zero exit.
3. Stale allowlist entry (missing file) causes non-zero exit.
4. CI green on the wired pipeline.

## 6. Rollback Plan

Remove the Makefile/CI hook; script and allowlist are inert without wiring.

## 7. Product Mapping

1. Invariant 1-2 map to the script's file filter and allowlist format checks.
2. Invariant 3 maps to the output contract.
3. Invariant 4 maps to the shared Makefile target used by CI and local runs.
