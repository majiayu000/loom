# GH482 Tasks: Provider Outdated And Re-Pin Workflow

Issue: https://github.com/majiayu000/loom/issues/482
Product spec: `specs/GH482/product.md`
Tech spec: `specs/GH482/tech.md`
Status: Implemented

## Order

Command placement decision -> read-only outdated report -> re-pin plan -> docs/tests.

## Tasks

- [x] `SP482-T001` Owner: maintainer | Dependencies: none | Done when: command placement is chosen: `skill outdated`, `provider outdated`, or `skill provenance outdated` | Verify: selected `skill provenance outdated`
- [x] `SP482-T002` Owner: provenance/provider | Dependencies: `SP482-T001` | Done when: read-only outdated command lists provider-backed skills with current ref/digest, candidate ref/digest, provider, status, and next actions | Verify: `cargo test skill_provenance`
- [x] `SP482-T003` Owner: provider resolver | Dependencies: `SP482-T002` | Done when: unreachable provider, unpinned candidate, invalid source, and up-to-date cases are distinct machine-readable statuses | Verify: provider command tests
- [x] `SP482-T004` Owner: re-pin plan | Dependencies: `SP482-T002`, `SP482-T003` | Done when: re-pin/update flow emits a plan without mutating skill content until explicit apply/review gates | Verify: provider/provenance tests
- [x] `SP482-T005` Owner: docs | Dependencies: `SP482-T004` | Done when: docs explain immutable pin policy, advisory heads, and safe re-pin workflow | Verify: `git diff --check`
- [x] `SP482-T006` Owner: verification | Dependencies: all prior tasks | Done when: full Rust checks pass | Verify: `cargo check && cargo test`
