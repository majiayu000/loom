# GH497 Technical Spec

Issue: https://github.com/majiayu000/loom/issues/497
Product spec: `specs/GH497/product.md`
Status: Draft for implementation

## Design Summary

Introduce a shared projection executor under `src/commands/projection_executor.rs`.
The executor accepts an already resolved target/binding/skill/method selection and owns the mutating projection transaction.

Existing command-specific code keeps responsibility for:

- CLI parsing and JSON envelope shape;
- resolving or creating target and binding candidates;
- command-specific operation intent (`skill.project` vs `skill.activate`);
- telemetry emitted after successful activation.

The executor owns:

- validating target ownership, target-agent match, method capability, skill existence, and safety policy;
- probing symlink support before destructive writes;
- materializing or no-oping the projection;
- building/upserting `RegistryBindingRule` and `RegistryProjectionInstance`;
- applying `observe_projection` / `apply_projection_observation`;
- saving target/binding/rule/projection state atomically enough to rollback on command failures;
- recording registry operation and projection observation;
- committing registry state and returning commit/meta/projection evidence.

## API Shape

```rust
pub(crate) struct ProjectionExecutionInput {
    pub skill: String,
    pub binding: RegistryWorkspaceBinding,
    pub binding_is_new: bool,
    pub target: RegistryProjectionTarget,
    pub target_is_new: bool,
    pub method: ProjectionMethod,
    pub operation_intent: &'static str,
    pub observation_kind: &'static str,
    pub request_id: String,
    pub commit_message: String,
    pub replace_existing: bool,
    pub safe_existing_noop: bool,
}
```

The exact Rust shape can change during implementation, but the fields above must remain represented by typed data rather than ad hoc JSON.

## Command Routing

1. `skill project`
   - resolves the existing binding and target from the snapshot;
   - calls the shared executor with `target_is_new=false`, `binding_is_new=false`;
   - keeps `replace_existing=true` because existing `skill project` intentionally replaces an owned projection after taking a backup.
2. `skill activate`
   - uses existing activation resolution for default target/binding selection;
   - calls the shared executor with activation-created target/binding flags;
   - keeps `safe_existing_noop=true` so an already safe projection is a no-op.
3. `use --apply`
   - continues to compose target add/adopt, binding add, and `cmd_project`;
   - inherits executor behavior through `cmd_project`.
4. `plan apply`
   - continues to replay durable `use_args`;
   - inherits executor behavior through `cmd_use` -> `cmd_project`.

## Rollback Rules

1. If materialization fails after removing/replacing a path, restore any projection backup and original registry state.
2. If state save, operation record, observation record, commit, or autosync fails after materialization, restore registry state and the live projection path when possible.
3. Rollback failures must be attached to the typed error details; no silent cleanup failure.
4. Existing activation target save rollback must remain covered when activation creates a new target.

## Compatibility

- Keep operation intents `skill.project` and `skill.activate`.
- Keep observation kinds `projected` and `activated`.
- Keep projection instance id construction unchanged.
- Keep `safe-capture` policy profile for activation-created bindings.
- Keep `use --apply` operation id output shape.
- Preserve current no-op semantics for repeated safe activation.

## Risks

1. `skill project` currently replaces existing projection paths; activation currently refuses unknown existing paths. The executor must model both policies explicitly.
2. Activation creates target/binding records in memory before materialization; rollback must include targets when state persistence later fails.
3. Digests for symlink and copy/materialize differ by design; parity tests should compare semantic fields rather than raw timestamps.

## Test Plan

Focused tests:

1. `use --apply` and `skill activate` project the same skill to the same agent/workspace with equivalent registry state.
2. `skill project --method copy` and `skill activate --method copy` both record source/materialized digests and healthy observation state.
3. Existing activation no-op test still passes for repeated symlink activation.
4. Existing project rollback tests still pass.

Suggested commands:

```bash
git diff --check
cargo test --test use_flow_cli
cargo test --test skill_activation
cargo test --test project
cargo test --test agent_plan_apply
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
```

## Rollback

Rollback is limited to removing the shared executor and returning `cmd_project` and `cmd_skill_activate` to their previous separate materialization paths. No state migration is introduced.
