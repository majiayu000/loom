# GH481 Tech Spec - Workflow run and rollback_token contract

Issue: https://github.com/majiayu000/loom/issues/481
Product spec: `specs/GH481/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

`workflow run` returns `PolicyBlocked` for non-dry-run calls. `plan apply` emits a `rollback_token`, but no command consumes it. Documentation presents the recovery token as part of the public contract.

## 2. Proposed Design

There are two acceptable implementation routes. Maintainers should choose one before implementation.

### Route A: Implement minimal executable contract

1. Limit `workflow run` to single-node workflows or already-supported plan/apply paths.
2. Enforce existing preflight, policy, and approval gates.
3. Persist rollback token metadata with enough context to validate and consume it.
4. Add a token consumer command or subcommand that validates token freshness and executes documented rollback commands.

### Route B: Hide/deprecate incomplete contract

1. Hide or clearly mark `workflow run` as unsupported/deferred.
2. Remove `rollback_token` from public JSON output.
3. Keep explicit `rollback_commands` as the recovery mechanism.
4. Update docs and tests to assert the reduced contract.

## 3. Affected Areas

1. `src/commands/workflow_cmds/mod.rs`
2. `src/commands/plan_cmds.rs`
3. `src/cli/workflow.rs`
4. `src/cli.rs`
5. `docs/LOOM_CLI_CONTRACT.md`
6. `tests/agent_plan_apply.rs`, `tests/cli_surface.rs`, workflow tests

## 4. Compatibility

1. Route A is additive but must avoid unsafe automatic execution.
2. Route B is a public contract reduction and should be documented in changelog/release notes.
3. In both routes, `--dry-run` remains no-write.

## 5. Verification Plan

1. `cargo test agent_plan_apply`
2. `cargo test cli_surface`
3. Workflow-specific tests after implementation route is chosen.
4. `cargo check`
5. `cargo test`

## 6. Rollback Plan

If Route A proves too broad, revert to Route B and keep only explicit rollback commands. If Route B breaks users, restore token output only with `deprecated=true` and clear unsupported status.

## 7. Product Mapping

1. Product invariant 1 maps to either executable workflow run tests or hidden/deprecated CLI tests.
2. Product invariant 2 maps to token consumer or token removal.
3. Product invariant 3 maps to explicit unsupported/deferred output.
4. Product invariant 4 maps to dry-run tests.
