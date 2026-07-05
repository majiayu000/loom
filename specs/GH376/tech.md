# GH376 Tech Spec: Advanced Skill Ecosystem Umbrella

Issue: https://github.com/majiayu000/loom/issues/376
Product spec: `specs/GH376/product.md`
Status: Implemented; closeout evidence recorded

## Current State

`docs/plan/advanced-skill-ecosystem-todolist.md` already records the local
planning bridge for #376 through #386. It says the advanced ecosystem should not
implement runtime behavior until the single-skill lifecycle primitives are
stable.

The repo currently has a partial #377 implementation for
`skillset create/add/remove/show/lint`, but activation, eval aggregation,
release, rollback, recommendations, orchestration, provisioning, and policy
enforcement are intentionally not complete.

## Shared Technical Principles

### Consume Single-Skill Read Models

Advanced features must consume shared read models from the single-skill
foundation:

- `skill inspect` for source, compatibility, runtime visibility, eval, safety,
  and next actions.
- activation status from #367.
- Codex visibility and reconcile state from #368.
- eval reports from #369.
- safety/trust/quarantine state from #370.
- dependency and MCP readiness from #371.
- adapter discovery, visibility, and reload metadata from #373.

No child issue should reimplement these joins independently.

### Plan Before Apply

Every mutating advanced workflow must have a plan or dry-run command before any
apply command:

```text
recommend -> explain only
workflow plan -> workflow apply
catalog preview/install --dry-run -> install --apply
provision plan -> provision apply
mcp plan -> mcp apply
skillset activate --dry-run -> skillset activate --apply
```

Apply commands must revalidate plan inputs, current registry head, policy
state, idempotency key, and required approvals.

### Policy And Audit

Advanced features must write through existing command audit and policy patterns.
They should reuse:

- command envelopes
- pending operation queue
- approval token semantics
- operation history
- read-only Panel gates
- local-host Panel authorization for mutations

### Data Compatibility

New data files under `state/registry` must be deterministic and migration-safe:

- stable schema version
- sorted keys/records where practical
- explicit unknown-field handling
- no implicit destructive migration
- malformed files fail without overwrite

## Child Spec Requirements

Each child issue packet (#377 through #386) must include:

1. Blocking dependencies from #363.
2. Read model dependencies.
3. State files or schema additions.
4. CLI/API surface.
5. Dry-run or plan contract.
6. Apply contract only when safe to implement.
7. Safety, trust, approval, and audit rules.
8. Panel behavior if applicable.
9. Focused tests and full verification commands.

## Phase Gate Table

| Phase | Child Issues | Gate Before Runtime Apply |
|---|---|---|
| A Data foundation | #377, #378, #381, #385 | schemas and read models are deterministic |
| B Safe operations | #377, #380, #382, #386 | dry-run plans exist and policy gates are wired |
| C Intelligence | #378, #379, #383, #384 | outputs are explainable and non-mutating by default |
| D Org/product | #381, #385, Panel/docs | backend read models exist before UI claims |

## Verification

Umbrella spec verification:

```bash
git diff --check
SPEC_RAIL_REPO=/path/to/specrail
python3 "$SPEC_RAIL_REPO/checks/check_workflow.py" --repo . --spec-dir specs/GH376
cargo check --workspace --all-targets --all-features
cargo test
```

Child implementation PRs must also run focused tests for their changed modules.

## Handoff Notes

Use `Refs #376` for this umbrella packet. Do not use a closing keyword for #376
until all child issues are either implemented, explicitly closed as out of
scope, or replaced by newer tracked issues.
