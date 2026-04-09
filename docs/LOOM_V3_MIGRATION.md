# Loom v3 Migration Plan

Updated: 2026-04-08
Status: Draft

## 1. Summary

This plan migrates Loom from the v2 single-target mental model to a v3 binding-based model.

The migration priority is:

1. correct the model
2. preserve local safety
3. postpone UI and remote complexity

## 2. Migration Goals

1. Replace single-directory assumptions with explicit bindings and targets.
2. Preserve Git-native source versioning.
3. Stop treating live directories as implicit truth.
4. Keep migration additive at the design level before any destructive runtime behavior is introduced.

## 3. What Must Not Happen

1. Do not auto-rewrite arbitrary Claude or Codex directories during migration.
2. Do not infer canonical truth from live directories.
3. Do not combine state migration with panel or remote redesign in one step.
4. Do not silently map old `claude_path/codex_path` records into a fake single-binding world.

## 4. Current v2 Gaps

v2 currently has these structural limits:

1. target selection is modeled as `claude/codex/both`
2. one skill stores at most one Claude path and one Codex path
3. `init` mixes bootstrap, import, and projection
4. observation and capture are not first-class flows
5. documentation and code are already drifting on state layout expectations

## 5. Migration Strategy

### Phase 0: Freeze Terminology

Deliverables:

1. `LOOM_V3_SPEC.md`
2. this migration document

Acceptance:

1. source registry, binding, target, projection, and history are defined as separate concepts
2. "live directory is not canonical" is documented as a hard rule

### Phase 1: Introduce v3 State Schema

Deliverables:

1. `state/v3/schema.json`
2. `state/v3/bindings.json`
3. `state/v3/targets.json`
4. `state/v3/rules.json`
5. `state/v3/projections.json`
6. `state/v3/ops/*`

Acceptance:

1. v3 readers can load an empty state layout without touching agent directories
2. read commands stay side-effect free

### Phase 2: Target Registration

Deliverables:

1. `target add/list/show/remove`
2. ownership modes: `managed`, `observed`, `external`

Acceptance:

1. Loom can register multiple target directories for the same agent kind
2. no projection occurs during registration

### Phase 3: Workspace Binding

Deliverables:

1. `workspace binding add/list/remove`
2. `binding_id` resolution in CLI and JSON output

Acceptance:

1. multiple Claude workspaces can coexist in Loom state
2. one binding can resolve to one default target without affecting others

### Phase 4: Projection Refactor

Deliverables:

1. `skill project`
2. `ProjectionInstance` persistence
3. target-scoped method recording

Acceptance:

1. one skill can be projected into many bindings
2. projection state is independent per binding
3. method fallback is recorded per projection instance, not per skill

### Phase 5: Observation and Capture

Deliverables:

1. drift detection state
2. `skill capture`
3. explicit capture audit records

Acceptance:

1. live edits can be detected without being auto-promoted to source
2. capture always records `binding_id`, `instance_id`, and resulting revision

### Phase 6: Lifecycle Command Remap

Deliverables:

1. `skill import --from-binding <id>`
2. `skill project`
3. `workspace status --binding <id|all>`

Acceptance:

1. no write command depends on a guessed default Claude directory
2. `init` is no longer the only bootstrap abstraction

### Phase 7: Sync and Remote

Deliverables:

1. sync built on v3 operation journal
2. replay aware of source revisions and projection events

Acceptance:

1. remote features do not change binding resolution semantics
2. local correctness does not depend on remote configuration

### Phase 8: Panel Convergence

Deliverables:

1. one panel implementation
2. panel views based on v3 state

Acceptance:

1. panel does not invent state that CLI cannot express
2. panel is optional for core workflows

## 6. v2 to v3 Mapping

### 6.1 Keep

1. `skills/<skill>` as source registry layout
2. Git-native revisions
3. operation journal as a durable concept

### 6.2 Replace

1. `state/targets.json` -> `state/v3/targets.json` + `bindings.json` + `rules.json` + `projections.json`
2. `Target::Claude|Codex|Both` execution model -> explicit `target_id` and `binding_id`
3. coarse `init/import/link` chaining -> explicit state, registration, projection, and capture steps

### 6.3 Reinterpret

1. existing live directories become candidate targets, not canonical sources
2. `link/use` become projection operations, not identity-defining operations

## 7. Proposed Compatibility Policy

1. v3 may read v2 state only through an explicit migration command.
2. v3 must not silently mutate v2 files in place.
3. If migration support is implemented, it should output a migration report before writing any v3 state.

## 8. Proposed Migration Command

Recommended shape:

```bash
loom migrate v2-to-v3 --plan
loom migrate v2-to-v3 --apply
```

`--plan` should:

1. inspect existing v2 state
2. enumerate candidate targets
3. enumerate unresolved ambiguities
4. produce a migration report without writing

`--apply` should:

1. write v3 state
2. preserve v2 state untouched
3. never rewrite live agent directories as part of migration

## 9. Operator Workflow During Migration

1. Register known targets.
2. Create workspace bindings.
3. Review binding rules.
4. Project selected skills.
5. Enable observation if desired.
6. Use `capture` to absorb live edits intentionally.

## 10. Deferred Work

These are intentionally after the core migration:

1. remote registry workflows
2. aggressive auto-discovery
3. panel-heavy flows
4. automatic watch-based capture
5. cross-machine optimization

## 11. Risks and Countermeasures

1. Risk: users expect one global Claude directory.
Countermeasure: require explicit bindings and show clear status output.

2. Risk: migration conflates observed directories with managed directories.
Countermeasure: force ownership declaration on registered targets.

3. Risk: v2 state and v3 schema drift further apart.
Countermeasure: stop editing v2 schema docs once v3 spec is adopted.

4. Risk: panel work outruns core model work.
Countermeasure: make panel consume v3 state only after CLI semantics stabilize.

## 12. Release Gate

v3 should not be considered ready until:

1. bindings and targets are first-class state objects
2. projection is binding-scoped
3. capture is explicit
4. status can explain binding resolution
5. no core command requires a guessed single agent directory
