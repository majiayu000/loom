# GH379 Tasks: Workflow DAG Orchestration

Issue: https://github.com/majiayu000/loom/issues/379
Product spec: `specs/GH379/product.md`
Tech spec: `specs/GH379/tech.md`
Status: Implemented; closeout evidence recorded

## Scope For First PR

Implement the planning-first workflow foundation:

```text
workflow definition model + DAG validation + workflow plan/preflight + no autonomous run
```

Do not implement:

```text
background daemon, recursive skill invocation, automatic workflow execution, workflow apply, native agent sandbox bypass
```

## Tasks

- [x] `SP379-T001` Owner: workflow-model | Done when: deterministic workflow definition state supports named DAG nodes, edges, policy, and timestamps | Verify: `cargo test --test workflow_cli`
- [x] `SP379-T002` Owner: dag-validation | Done when: planner rejects duplicate nodes, missing edge targets, cycles, excessive depth, missing skills, blocked skills, and unmet dependencies | Verify: `cargo test --test workflow_cli`
- [x] `SP379-T003` Owner: workflow-plan | Done when: `workflow plan` creates an auditable read-only plan with ordered nodes, activation steps, risks, approvals, guards, and next actions | Verify: `cargo test --test workflow_cli`
- [x] `SP379-T004` Owner: workflow-preflight | Done when: `workflow preflight <plan-id>` revalidates current registry, workflow digest, skill digests, policy, dependency readiness, and active state | Verify: `cargo test --test workflow_cli`
- [x] `SP379-T005` Owner: workflow-apply-deferred | Done when: autonomous workflow execution remains hidden/deferred until plan/preflight semantics are stable; future apply requirements are documented | Verify: `cargo test --test cli_surface`
- [x] `SP379-T006` Owner: safety | Done when: no command silently executes workflow nodes, activates skills, writes configs, or retries failed nodes without explicit apply and approvals | Verify: `cargo test --test workflow_cli && cargo test --test agent_plan_apply`
- [x] `SP379-T007` Owner: regression | Done when: focused and full repository checks pass | Verify: `cargo check --workspace --all-targets --all-features && cargo test`

### SP379-T1: Add Workflow Definition Model

Owner: backend

Files:

- `src/cli.rs`
- new workflow CLI module
- new workflow command module
- workflow state tests

Done when:

- `workflow create` stores deterministic workflow definitions.
- Definitions include schema version, nodes, edges, policy, timestamps.
- `workflow show` returns a stable read model.
- Malformed state fails without overwrite.

Verify:

```bash
cargo test --test workflow_cli
```

### SP379-T2: Add DAG Validation

Owner: backend
Depends on: SP379-T1

Done when:

- Duplicate node ids fail.
- Edges to missing nodes fail.
- Cycles fail.
- Max node count and max depth are enforced.
- Missing skills fail.
- Blocked/quarantined skills fail once #370 state is available.
- Dependency readiness failures are surfaced once #371 state is available.

Verify:

```bash
cargo test --test workflow_cli
```

### SP379-T3: Add Workflow Planning

Owner: backend
Depends on: SP379-T1, SP379-T2

Done when:

- `workflow plan` returns a durable plan event.
- Plan output includes ordered nodes, activation steps, risks, approvals,
  guards, recovery policy, and next actions.
- Plan creation does not mutate registry state, target dirs, active views, or
  agent config.
- Plans generated from task descriptions use #378 recommendations when
  available.

Verify:

```bash
cargo test --test workflow_cli
```

### SP379-T4: Add Workflow Preflight

Owner: backend
Depends on: SP379-T3

Done when:

- Preflight reloads the stored plan.
- Preflight revalidates root, registry head, workflow digest, skill digests,
  policy, dependency readiness, and active status.
- Stale or unsafe plans fail with structured errors and recovery suggestions.

Verify:

```bash
cargo test --test workflow_cli
```

### SP379-T5: Add Workflow Apply Gate

Owner: backend
Depends on: SP379-T4

Done when:

- Apply requires idempotency key.
- Apply validates approval tokens.
- Apply returns idempotent replay for the same plan/key digest.
- Apply stops at first failed node and returns recovery commands.
- Apply does not silently retry.

Verify:

```bash
cargo test --test agent_plan_apply
```

### SP379-T6: Defer Run Until Safety Is Proven

Owner: safety
Depends on: SP379-T3

Done when:

- `workflow run --dry-run` is equivalent to plan/preflight or is deferred.
- Non-dry-run autonomous execution is not implemented in the first slice.
- Docs and command help do not imply background execution exists.

Verify:

```bash
cargo test --test workflow_cli
```

### SP379-T7: Full Verification

Owner: testing
Depends on: SP379-T1, SP379-T2, SP379-T3, SP379-T4, SP379-T5, SP379-T6

Done when:

- Focused tests cover every acceptance criterion.
- Full check and test suites pass.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Closeout evidence:

- PR #430 added workflow definition, DAG validation, plan, preflight, and
  deferred execution tests.
- GH481 Route B hides the incomplete `workflow run` public surface, preserves a
  dry-run compatibility response, and removes public `rollback_token` output
  until a consumer exists.
- Current verification includes `cargo test --test workflow_cli`,
  `cargo test --test agent_plan_apply`,
  `cargo check --workspace --all-targets --all-features`, and `cargo test`.

Use `Fixes #379` only from a closeout PR that includes the GH481 Route B
contract correction or is based on it.
