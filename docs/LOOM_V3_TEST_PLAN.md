# Loom v3 Test Plan

Updated: 2026-04-08
Status: Draft

## 1. Purpose

This document defines how to validate that the Loom v3 design is correct before implementation.

The goal is not to prove that code runs.
The goal is to prove that the design:

1. matches the real problem shape
2. preserves the correct source of truth
3. survives failure and ambiguity
4. is usable by agents without guessing

This test plan validates [LOOM_V3_SPEC.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_V3_SPEC.md) and [LOOM_V3_MIGRATION.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_V3_MIGRATION.md).

## 2. Validation Strategy

Loom v3 is valid only if it passes five test classes:

1. `scenario tests`
2. `invariant tests`
3. `failure tests`
4. `agent contract tests`
5. `migration tests`

Each class answers a different question:

1. Can the model express the real world?
2. Does the model preserve the core rules?
3. Does the system fail safely?
4. Can agents call it without ambiguity?
5. Can v2 move to v3 without unsafe assumptions?

## 3. Exit Criteria

The v3 design is acceptable only if all of the following are true:

1. No core workflow requires Loom to guess a single Claude directory.
2. One skill can be projected into multiple bindings at once.
3. Live edits never become canonical without explicit `capture`.
4. Every destructive projection path has an explicit recovery rule.
5. Migration can stop on ambiguity without touching live directories.

## 4. Test Classes

### 4.1 Scenario Tests

Scenario tests validate that the model can represent the real operating environment.

Required scenarios:

1. One Claude profile with multiple workdirs.
2. One Codex workspace and one Claude workspace using the same skill.
3. One skill projected into two workspaces with different methods.
4. One workspace bound to a managed target and another bound to an observed target.
5. One skill edited live inside a projection directory.
6. One target path that is user-managed and must not be overwritten.
7. One workspace with no default target and an explicit target-only projection call.

Pass condition:

1. Every scenario can be expressed using `target_id`, `binding_id`, `skill_id`, and `ProjectionInstance`.
2. No scenario requires path guessing or path-only identity.

### 4.2 Invariant Tests

Invariant tests validate design rules that must always remain true.

Required invariants:

1. `SkillSource` is canonical.
2. `ProjectionInstance` is derived state.
3. `binding_id` is required for any workspace-scoped projection action.
4. `target_id` is required for any target registration or explicit projection action.
5. A live edit is only canonical after `capture`.
6. Every write produces an `op_id`.
7. Every destructive projection action has a `recovery_ref` or equivalent recovery rule.

Pass condition:

1. No documented workflow violates any invariant.
2. No command contract implies hidden mutation of source from live state.

### 4.3 Failure Tests

Failure tests validate that the design fails safely under ambiguity or conflict.

Required cases:

1. Two bindings point to the same target path.
2. A target is registered as `managed`, but the path contains foreign content.
3. A target is registered as `observed`, and the operator attempts destructive projection.
4. Two live projections of the same skill drift differently before capture.
5. Capture is requested after source changed since `last_applied_rev`.
6. A watch event arrives for a path outside a managed projection.
7. A binding matcher resolves ambiguously to more than one workspace.

Pass condition:

1. The design returns structured errors or blocked states.
2. No failure case relies on silent overwrite or hidden fallback.

### 4.4 Agent Contract Tests

Agent contract tests validate non-interactive operability.

Required checks:

1. Every state-changing command supports `--json`.
2. Every command supports `--root <abs-path>`.
3. Any binding-scoped command requires `binding_id` or an equivalent explicit selector.
4. Any target-scoped command requires `target_id` or an equivalent explicit selector.
5. Every write response can return `op_id`.
6. Every projection or capture response can return `binding_id`, `target_id`, and `instance_id`.
7. Read commands are side-effect free by design.

Pass condition:

1. An agent can perform the documented workflow without reading undocumented state files.
2. An agent never needs to guess the current Claude workdir from path heuristics alone.

### 4.5 Migration Tests

Migration tests validate the v2 to v3 transition.

Required checks:

1. `v2` state can be inspected without mutation.
2. Candidate targets can be enumerated without claiming canonical truth.
3. Ambiguous `claude_path/codex_path` mappings can be reported as unresolved.
4. Migration can generate a plan without applying it.
5. `--apply` can be blocked if unresolved ambiguities remain.
6. Migration does not rewrite live directories.
7. Migration preserves v2 state as historical input.

Pass condition:

1. Migration is explicit, reviewable, and reversible at the state level.
2. No migration step silently promotes a live directory into canonical source.

## 5. Validation Matrix

| Area | Question | Required Evidence |
|---|---|---|
| Model | Can one skill map to many workspaces? | `BindingRule` and `ProjectionInstance` examples |
| Truth | Is canonical source always clear? | `SkillSource` and `capture` semantics |
| Safety | Can Loom refuse unsafe overwrite? | ownership rules and failure states |
| Agent UX | Can an agent call commands without guessing? | explicit selectors and JSON envelope |
| Migration | Can v2 migrate without path assumptions? | dry-run migration report shape |

## 6. Reference Scenarios

### Scenario A: Multi-Workspace Claude

Setup:

1. Claude has workspace A and workspace B.
2. Each workspace resolves to a different target directory.
3. Both use the same `model-onboarding` skill.

Expected:

1. Two bindings exist.
2. One skill maps to two projection instances.
3. A live edit in workspace A does not automatically mutate workspace B or source.

### Scenario B: Mixed Projection Methods

Setup:

1. Workspace A uses `symlink`.
2. Workspace B uses `copy`.

Expected:

1. Projection method is stored per projection instance.
2. Health and drift are tracked independently.

### Scenario C: Observed External Target

Setup:

1. A user-managed target is registered with `ownership=observed`.

Expected:

1. Loom can observe drift.
2. Loom cannot destructively replace that target without explicit adoption.

### Scenario D: Capture Flow

Setup:

1. Agent edits `SKILL.md` inside a live projection.

Expected:

1. Observation records drift.
2. Source remains unchanged until `skill capture`.
3. `capture` produces a source revision and operation record.

### Scenario E: Migration Ambiguity

Setup:

1. v2 state contains one `claude_path`.
2. The operator actually has multiple Claude workdirs.

Expected:

1. Migration plan marks binding resolution as ambiguous.
2. Migration does not auto-assign a single binding.

## 7. Test Method by Stage

### Stage 0: Spec Review

Method:

1. walk every reference scenario against the spec
2. mark missing entities, selectors, or invariants

Output:

1. redline comments against `LOOM_V3_SPEC.md`

### Stage 1: Schema Review

Method:

1. instantiate sample `bindings.json`, `targets.json`, `rules.json`, and `projections.json`
2. verify that all reference scenarios fit without schema hacks

Output:

1. example state fixtures

### Stage 2: CLI Contract Review

Method:

1. write command examples for registration, binding, projection, capture, and status
2. check that each example is explicit and non-ambiguous

Output:

1. CLI example table
2. JSON response fixtures

### Stage 3: Migration Review

Method:

1. run paper migration cases from v2 records to v3 entities
2. classify each case as `safe`, `needs review`, or `blocked`

Output:

1. migration decision table

## 8. Design Smells That Fail Review

The v3 design fails review if any of these reappear:

1. a single default Claude directory becomes execution-critical
2. `binding_id` becomes optional in write flows that affect projections
3. `capture` becomes implicit
4. panel introduces state not present in CLI or schema
5. one skill record stores only one `claude_path` and one `codex_path`
6. ownership is not checked before destructive projection

## 9. Deliverables From Validation

Validation is complete only when these artifacts exist:

1. reviewed v3 spec
2. reviewed migration plan
3. scenario matrix
4. CLI examples
5. JSON envelope examples
6. unresolved issue list

## 10. Recommended Next Step

After this test plan is accepted, the next document to write is:

1. `docs/LOOM_V3_CLI_CONTRACT.md`

That document should turn the spec into concrete command and response examples before any new implementation work starts.
