# GH537 Tasks: Structured Error Contract Hardening

Issue: https://github.com/majiayu000/loom/issues/537
Product spec: `specs/GH537/product.md`
Tech spec: `specs/GH537/tech.md`
Status: Maintainer decisions approved; ready for implementation

## Order

Exit-code decision -> main.rs envelope emission -> next_actions coverage -> contract docs/tests.

## Tasks

- [x] `SP537-T001` Owner: maintainer | Dependencies: none | Decision: no exit-code re-tiering; `error.code` is sole stable semantic routing key; add `INIT_ERROR` / exit 3 with `cmd: "app.init"` | Verify: decision recorded on 2026-07-16
- [ ] `SP537-T002` Owner: cli | Dependencies: `SP537-T001` | Done when: init/panel/top-level failures emit envelopes under `--json`; init uses `app.init` + `INIT_ERROR` | Verify: new integration test
- [ ] `SP537-T003` Owner: errors | Dependencies: none | Done when: universal remote/lock defaults, contextual conflict/policy call-site actions, and written pure-fault exemptions form a total table; no default action requires missing arguments | Verify: `cargo test error_actions`
- [ ] `SP537-T004` Owner: docs | Dependencies: `SP537-T001`..`T003` | Done when: contract doc lists exit codes, routing-key statement, and next_actions coverage | Verify: `cargo test --test cli_surface`
- [ ] `SP537-T005` Owner: verification | Dependencies: all prior | Done when: full checks pass | Verify: `cargo check && cargo test`
