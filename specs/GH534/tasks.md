# GH534 Tasks: Module Size Ceiling Guard Automation

Issue: https://github.com/majiayu000/loom/issues/534
Product spec: `specs/GH534/product.md`
Tech spec: `specs/GH534/tech.md`
Status: Maintainer decisions approved; ready for implementation

## Order

Ceiling decision -> guard script + allowlist -> Makefile/CI wiring -> report refresh.

## Tasks

- [x] `SP534-T001` Owner: maintainer | Dependencies: none | Decision: 800 hard-fail, 700 warning; `scripts/module-ceiling.sh` + own Makefile target + CI verify after lint; full production-file line count; split-on-touch with tracking issues created up front | Verify: decision recorded in product/tech spec on 2026-07-16
- [x] `SP534-T002` Owner: tooling | Dependencies: `SP534-T001` | Done when: `scripts/module-ceiling.sh` + 3-entry allowlist (`#544`, `#545`, `#546`) exist and pass on current tree; format is `path<TAB>baseline_lines<TAB>issue-ref` | Verify: `make module-ceiling module-ceiling-test`
- [x] `SP534-T003` Owner: tooling | Dependencies: `SP534-T002` | Done when: guard wired into Makefile and CI verify job | Verify: `make module-ceiling module-ceiling-test`; CI verification passed on PR #547
- [x] `SP534-T004` Owner: docs | Dependencies: `SP534-T003` | Done when: `docs/module-ceiling-signal-report.md` reflects guard, allowlist, and split queue | Verify: `git diff --check`
- [x] `SP534-T005` Owner: verification | Dependencies: all prior | Done when: full checks pass | Verify: `cargo check --workspace --all-targets --all-features && make check`; CI verification passed on PR #547
