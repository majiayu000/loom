# GH379 Tech Spec: Workflow DAG Orchestration

Issue: https://github.com/majiayu000/loom/issues/379
Product spec: `specs/GH379/product.md`
Status: Implemented; closeout evidence recorded

## Current State

Loom already has durable `plan use` and top-level `apply` semantics in
`src/commands/plan_cmds.rs`. Plans record registry head, source digest, risks,
required approvals, idempotency key digest, and recovery commands.

GH379 should reuse that plan/apply contract. It should not introduce a separate
execution safety model.

Relevant files:

- `src/commands/plan_cmds.rs`
- `docs/schemas/agent-plan-v1.schema.json`
- `docs/LOOM_CLI_CONTRACT.md`
- `src/commands/skill_policy.rs`
- future read models from #366/#370/#371/#378

## State Model

Recommended registry state:

```text
state/registry/workflows.json
state/registry/workflow_runs.json
```

`workflows.json` stores named workflow definitions. `workflow_runs.json` should
not be required for planning; it is for future apply/run audit summaries if the
existing command event log is not enough.

Workflow definition file:

```json
{
  "schema_version": 1,
  "workflows": [
    {
      "workflow_id": "coding-fix-flow",
      "description": "Diagnose and fix a failing test or CI run.",
      "nodes": [],
      "edges": [],
      "external_inputs": [],
      "policy": {
        "max_nodes": 8,
        "max_depth": 6,
        "requires_human_approval_before": [],
        "rollback_strategy": "checkpoint-before-mutating-node"
      },
      "created_at": "2026-07-01T00:00:00Z",
      "updated_at": "2026-07-01T00:00:00Z"
    }
  ]
}
```

Records must be sorted and deterministic before write. Malformed workflow state
must fail without overwrite.

## DAG Validation

Validation must run before plan persistence:

1. Unique workflow id.
2. Unique node ids.
3. Every edge references existing nodes.
4. No cycles.
5. Bounded node count and depth.
6. Every required output is produced by a predecessor or declared external
   input.
7. Every skill node references an existing skill.
8. Every skill node passes inspect, safety, and dependency gates.
9. Every mutating node has an approval token or approval requirement.

Use deterministic topological ordering for output and tests.

## CLI Surface

Add a `workflow` command group:

```bash
loom workflow create <name> --file <workflow.json>
loom workflow create <name> --from-skillset <skillset> --dry-run
loom workflow show <name>
loom workflow plan <name|task-description> --agent <agent> --workspace <path>
loom workflow preflight <plan-id>
```

`workflow run` should be deferred or implemented as `workflow plan` plus a
dry-run summary until apply safety is proven. `workflow apply` should be absent
or return typed not-implemented until plan/preflight behavior is stable.

## Plan Contract

Workflow plans should use the existing durable plan event store, with a new
operation value:

```json
{
  "protocol_version": "1.0",
  "schema_version": "workflow-plan-v1",
  "plan_id": "plan_...",
  "operation": "workflow",
  "workflow": "coding-fix-flow",
  "workflow_snapshot": {},
  "agent": "codex",
  "workspace": "/repo",
  "ready": false,
  "nodes": [
    {"id": "orient", "skill": "repo-orientation", "status": "ready"},
    {"id": "fix", "skill": "fixflow", "status": "approval_required"}
  ],
  "activation_steps": [],
  "risk_summary": {"high": 1, "medium": 2},
  "required_approvals": ["write-code"],
  "guards": {
    "root": "/registry",
    "registry_head": "abc123",
    "workflow_digest": "sha256:...",
    "workflow_snapshot_digest": "sha256:...",
    "skill_digests": {}
  },
  "next_actions": []
}
```

For named workflows, `workflow_digest` identifies the persisted definition. For
skillset or task-description planning, the planner must materialize a canonical
workflow snapshot and use `workflow_snapshot_digest` as the guard.

Apply must validate:

- root
- registry head
- workflow digest
- all skill source digests
- required approvals
- idempotency key

## Safety And Recovery

Planning must not mutate registry state except durable plan audit events.

Apply must:

- re-run preflight
- preserve idempotent replay
- checkpoint before mutating nodes
- stop at first failed node
- report recovery commands
- never silently retry failed nodes

## Tests

Focused tests:

1. workflow create stores deterministic DAG definitions.
2. cycle detection fails.
3. missing node skill fails.
4. blocked/quarantined skill fails.
5. dependency readiness failure fails or marks not-ready.
6. approval-required node emits approval token.
7. activation step generation does not mutate active view.
8. canonical plan body is stable for the same registry state, excluding
   volatile fields such as `plan_id`, timestamps, and audit/event metadata.
9. apply validates idempotency key and stale guards.

## Verification

```bash
git diff --check
cargo test --test workflow_cli
cargo test --test agent_plan_apply
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #379` until workflow definition, planning, preflight, and apply gates
are all implemented and verified.
