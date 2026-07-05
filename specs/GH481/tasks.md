# GH481 Tasks: Workflow Run And Rollback Token Contract

Issue: https://github.com/majiayu000/loom/issues/481
Product spec: `specs/GH481/product.md`
Tech spec: `specs/GH481/tech.md`
Status: Route B implemented

## Order

Route B selected -> hide/deprecate incomplete workflow run contract -> remove public rollback token -> docs/help parity -> tests.

## Tasks

- [x] `SP481-T001` Owner: maintainer | Dependencies: none | Done when: Route B is selected in `specs/GH481/tech.md` | Verify: spec diff
- [x] `SP481-T002` Owner: workflow | Dependencies: `SP481-T001` | Done when: `workflow run` is hidden/deferred and no longer presents permanent `PolicyBlocked` as a normal executable command path | Verify: `cargo test --test workflow_cli`
- [x] `SP481-T003` Owner: recovery | Dependencies: `SP481-T001` | Done when: `rollback_token` is removed from public JSON output in favor of explicit rollback commands | Verify: `cargo test --test agent_plan_apply`
- [x] `SP481-T004` Owner: docs/help | Dependencies: `SP481-T002`, `SP481-T003` | Done when: CLI help and `docs/LOOM_CLI_CONTRACT.md` match the implemented public contract | Verify: `cargo test --test cli_surface && git diff --check`
- [x] `SP481-T005` Owner: verification | Dependencies: all prior tasks | Done when: dry-run/no-write behavior and full Rust checks pass | Verify: `cargo check --workspace --all-targets --all-features && cargo test`
