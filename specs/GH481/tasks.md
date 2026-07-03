# GH481 Tasks: Workflow Run And Rollback Token Contract

Issue: https://github.com/majiayu000/loom/issues/481
Product spec: `specs/GH481/product.md`
Tech spec: `specs/GH481/tech.md`
Status: Draft for review

## Order

Maintainer route decision -> implement selected contract -> docs/help parity -> tests.

## Tasks

- [ ] `SP481-T001` Owner: maintainer | Dependencies: none | Done when: maintainers choose Route A (minimal executable contract) or Route B (hide/deprecate incomplete contract) in `specs/GH481/tech.md` or issue comment | Verify: human review
- [ ] `SP481-T002` Owner: workflow | Dependencies: `SP481-T001` | Done when: `workflow run` behavior matches the chosen route and no longer presents permanent `PolicyBlocked` as a normal executable command path | Verify: workflow tests plus `cargo test cli_surface`
- [ ] `SP481-T003` Owner: recovery | Dependencies: `SP481-T001` | Done when: `rollback_token` is either consumed by a validated command or removed from public JSON output in favor of explicit rollback commands | Verify: `cargo test agent_plan_apply`
- [ ] `SP481-T004` Owner: docs/help | Dependencies: `SP481-T002`, `SP481-T003` | Done when: CLI help and `docs/LOOM_CLI_CONTRACT.md` match the implemented public contract | Verify: `cargo test cli_surface && git diff --check`
- [ ] `SP481-T005` Owner: verification | Dependencies: all prior tasks | Done when: dry-run/no-write behavior and full Rust checks pass | Verify: `cargo check && cargo test`
