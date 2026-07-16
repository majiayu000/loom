# GH537 Tasks: Structured Error Contract Hardening

Issue: https://github.com/majiayu000/loom/issues/537
Product spec: `specs/GH537/product.md`
Tech spec: `specs/GH537/tech.md`
Status: Pending maintainer approval

## Order

Exit-code decision -> main.rs envelope emission -> next_actions coverage -> contract docs/tests.

## Tasks

- [ ] `SP537-T001` Owner: maintainer | Dependencies: none | Done when: decided between exit-code re-tiering vs documenting `error.code` as sole routing key; init-failure error code chosen | Verify: decision recorded here
- [ ] `SP537-T002` Owner: cli | Dependencies: `SP537-T001` | Done when: init/panel/top-level failures emit envelopes under `--json` | Verify: new integration test
- [ ] `SP537-T003` Owner: errors | Dependencies: none | Done when: conflict/policy/remote error codes carry default next_actions or exemption entries; table-driven test enforces totality | Verify: `cargo test error_actions`
- [ ] `SP537-T004` Owner: docs | Dependencies: `SP537-T001`..`T003` | Done when: contract doc lists exit codes, routing-key statement, and next_actions coverage | Verify: `cargo test --test cli_surface`
- [ ] `SP537-T005` Owner: verification | Dependencies: all prior | Done when: full checks pass | Verify: `cargo check && cargo test`
