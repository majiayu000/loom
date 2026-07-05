# GH379 Product Spec: Workflow DAG Orchestration

Issue: https://github.com/majiayu000/loom/issues/379
Parent: https://github.com/majiayu000/loom/issues/376
Status: Blocked design packet
Locale: zh-CN

## Goal

Add guarded workflow planning for multi-skill tasks without letting an LLM
freely chain arbitrary skills.

Loom should model workflows as explicit, auditable DAG plans. The first version
should create and preflight plans. Execution may remain external or
agent-driven until plan/preflight semantics are stable.

Example workflow:

```text
repo-orientation -> plan-flow -> fixflow -> regression-check -> pr-writer
```

## Blocking Dependencies

Production implementation is blocked by:

- #366 single-skill status and `skill inspect`.
- #367 activate/deactivate/list semantics.
- #369 real eval harness.
- #370 safety/trust/quarantine.
- #371 dependency and MCP readiness.
- #377 skillsets and grouped lifecycle data.
- #378 capability graph and recommendations.

## User-Facing Commands

Target command surface:

```bash
loom workflow create <name> --file <workflow.json>
loom workflow create <name> --from-skillset <skillset> --dry-run
loom workflow plan <name|task-description> --agent <agent> --workspace <path> [--json]
loom workflow preflight <plan-id>
loom workflow run <name> --agent <agent> --workspace <path> [--dry-run]
```

Deferred command:

```bash
loom workflow apply <plan-id> --idempotency-key <key> [--approve <token[,token]>]
```

The first implementation should prioritize `workflow plan` and `workflow
preflight`. `workflow apply` is deferred until those semantics are stable; when
added, it must reuse durable plan/apply safety semantics.

## Non-Goals

1. No autonomous background daemon.
2. No unbounded recursive skill invocation.
3. No bypass of native agent approval or sandbox controls.
4. No automatic execution until planning/preflight semantics are proven.
5. No workflow nodes that silently activate blocked, quarantined, or unsafe
   skills.
6. No hidden network, MCP, config, or filesystem writes during planning.

## Workflow Model

Workflow definitions should be explicit:

```json
{
  "workflow_id": "coding-fix-flow",
  "description": "Diagnose and fix a failing test or CI run.",
  "nodes": [
    {
      "id": "orient",
      "skill_id": "repo-orientation",
      "kind": "skill",
      "requires": [],
      "outputs": ["repo_summary", "implementation_plan"]
    },
    {
      "id": "fix",
      "skill_id": "fixflow",
      "kind": "skill",
      "requires": ["implementation_plan"],
      "outputs": ["patch"]
    }
  ],
  "edges": [
    {"from": "orient", "to": "fix"}
  ],
  "policy": {
    "max_nodes": 8,
    "requires_human_approval_before": ["fix"],
    "rollback_strategy": "checkpoint-before-mutating-node"
  }
}
```

## Plan Behavior

A workflow plan must:

1. Resolve candidate skills from a named workflow, skillset, or task
   description. For skillset or task-description input, materialize a canonical
   workflow snapshot before planning.
2. Validate every node's skill status with `skill inspect`.
3. Check safety, trust, quarantine, and dependency readiness.
4. Verify required skills are active or emit activation steps.
5. Emit approval tokens for risky nodes.
6. Create checkpoint requirements before mutating phases.
7. Return a fully auditable JSON plan.

## Safety Rules

1. Cycles are invalid.
2. Node count and depth are bounded.
3. Missing skills fail planning.
4. Blocked or quarantined skills fail planning.
5. Mutating nodes require explicit approval tokens.
6. Network/MCP-dependent nodes require readiness checks.
7. Failed apply must report recovery commands and avoid silent retries.

## Acceptance Criteria

1. Users can define a named workflow with DAG nodes and edges.
2. `workflow plan` returns an auditable plan and no mutation.
3. Planner rejects cycles, missing skills, blocked skills, unmet dependencies,
   and unsafe policies.
4. Planner can include activation steps but does not apply them without explicit
   apply.
5. Apply uses idempotency keys and preserves existing plan/apply safety
   semantics.
6. Tests cover cycle detection, missing node skill, blocked skill,
   approval-required node, activation step generation, idempotency, and dry-run
   plan stability.

Closeout evidence:

- PR #430 implements the planning-first workflow foundation and tests the
  workflow model, validation, planning, preflight, and deferred run behavior.
- GH481 Route B removes the misleading public run/token affordances so the
  first shipped workflow surface remains planning-first and non-autonomous.
