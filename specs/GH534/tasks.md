# GH534 Tasks: Module Size Ceiling Guard Automation

Issue: https://github.com/majiayu000/loom/issues/534
Product spec: `specs/GH534/product.md`
Tech spec: `specs/GH534/tech.md`
Status: Pending maintainer approval

## Order

Ceiling decision -> guard script + allowlist -> Makefile/CI wiring -> report refresh.

## Tasks

- [ ] `SP534-T001` Owner: maintainer | Dependencies: none | Done when: ceiling value (700 vs 800) and guard placement are decided | Verify: decision recorded in this file
- [ ] `SP534-T002` Owner: tooling | Dependencies: `SP534-T001` | Done when: `scripts/module-ceiling.sh` + allowlist (22 files at `bb9b738`) exist and pass on current tree | Verify: `bash scripts/module-ceiling.sh`
- [ ] `SP534-T003` Owner: tooling | Dependencies: `SP534-T002` | Done when: guard wired into Makefile and CI verify job | Verify: CI run green; negative test fails locally
- [ ] `SP534-T004` Owner: docs | Dependencies: `SP534-T003` | Done when: `docs/module-ceiling-signal-report.md` reflects guard, allowlist, and split queue | Verify: `git diff --check`
- [ ] `SP534-T005` Owner: verification | Dependencies: all prior | Done when: full checks pass | Verify: `cargo check && make perf-smoke` unaffected
