# GH497 Tasks: Projection Executor Convergence

Issue: https://github.com/majiayu000/loom/issues/497
Product spec: `specs/GH497/product.md`
Tech spec: `specs/GH497/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement one complete convergence slice:

```text
shared projection executor + skill project/activate routing + parity tests
```

Do not implement:

```text
compiled activation executor rewrite
CLI contract changes
copy/materialize deactivation
agent adapter changes
state schema migration
```

## Tasks

### SP497-T1: Add Shared Projection Executor

Owner: implementation

Files:

- `src/commands/projection_executor.rs`
- `src/commands/mod.rs`
- existing projection helper modules as needed

Done when:

- executor materializes symlink/copy/materialize projections;
- executor writes rules and projection instances;
- executor records operation and observation logs;
- executor supports replace-existing and safe-existing-noop policies;
- rollback errors are propagated through `CommandFailure`.

Verify:

```bash
cargo test --test project
cargo test --test skill_activation
```

### SP497-T2: Route `skill project` Through Executor

Owner: implementation
Depends on: SP497-T1

Files:

- `src/commands/skill_cmds.rs`

Done when:

- existing `skill project` tests pass;
- copy/materialize digest behavior is unchanged;
- operation intent remains `skill.project`;
- JSON output remains compatible.

Verify:

```bash
cargo test --test project
```

### SP497-T3: Route `skill activate` Through Executor

Owner: implementation
Depends on: SP497-T1

Files:

- `src/commands/skill_activation/mod.rs`
- `src/commands/skill_activation/apply.rs`

Done when:

- non-compiled activation uses the shared executor;
- repeated safe activation remains no-op;
- activation-created targets and bindings are saved through the executor;
- operation intent remains `skill.activate`;
- activation telemetry still records after successful activation.

Verify:

```bash
cargo test --test skill_activation
```

### SP497-T4: Add Parity Tests

Owner: implementation
Depends on: SP497-T2, SP497-T3

Files:

- `tests/use_flow_cli.rs`
- `tests/skill_activation.rs`
- `tests/project.rs` if copy/materialize digest parity fits better there

Done when:

- tests assert `use --apply` and `skill activate` produce equivalent registry state and live projection for the same project scope;
- tests assert copy projection digests/observation health exist for both `skill project` and `skill activate`.

Verify:

```bash
cargo test --test use_flow_cli
cargo test --test skill_activation
cargo test --test project
```

### SP497-T5: Final Verification

Owner: implementation
Depends on: SP497-T1, SP497-T2, SP497-T3, SP497-T4

Done when:

- focused tests pass;
- formatting passes;
- full workspace test passes;
- PR body maps acceptance criteria to evidence.

Verify:

```bash
git diff --check
cargo fmt --check
cargo test --test use_flow_cli
cargo test --test skill_activation
cargo test --test project
cargo test --test agent_plan_apply
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
```
