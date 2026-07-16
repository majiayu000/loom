# GH538 Tasks: Ops List Silent Zero On Git History Failure

Issue: https://github.com/majiayu000/loom/issues/538
Product spec: `specs/GH538/product.md`
Tech spec: `specs/GH538/tech.md`
Status: Pending maintainer approval

## Order

Fail-mode decision -> fix -> tests -> contract doc.

## Tasks

- [ ] `SP538-T001` Owner: maintainer | Dependencies: none | Done when: warning+degraded vs fail-closed decided | Verify: decision recorded here
- [ ] `SP538-T002` Owner: sync | Dependencies: `SP538-T001` | Done when: `sync_cmds.rs` surfaces the failure per decision | Verify: `cargo check`
- [ ] `SP538-T003` Owner: tests | Dependencies: `SP538-T002` | Done when: injected-failure and empty-history tests pass | Verify: `cargo test --test reliability`
- [ ] `SP538-T004` Owner: docs | Dependencies: `SP538-T002` | Done when: contract doc reflects any new payload field | Verify: `cargo test --test cli_surface`
- [ ] `SP538-T005` Owner: verification | Dependencies: all prior | Done when: full checks pass | Verify: `cargo test`
