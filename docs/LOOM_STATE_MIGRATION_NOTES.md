# Loom registry model Migration Plan

Updated: 2026-04-08
Status: Draft

## 1. Summary

This plan migrates Loom from the legacy single-target mental model to a registry binding-based model.

For current operator-facing active-view migration, see
`MIGRATING_TO_ACTIVE_VIEW.md`. For Codex-specific root, config, and restart
semantics, see `CODEX_SKILL_VISIBILITY.md`.

The migration priority is:

1. correct the model
2. preserve local safety
3. postpone UI and remote complexity

## 2. Migration Goals

1. Replace single-directory assumptions with explicit bindings and targets.
2. Preserve Git-native Git-backed source history.
3. Stop treating live directories as implicit truth.
4. Keep migration additive at the design level before any destructive runtime behavior is introduced.

## 3. What Must Not Happen

1. Do not auto-rewrite arbitrary Claude or Codex directories during migration.
2. Do not infer canonical truth from live directories.
3. Do not combine state migration with panel or remote redesign in one step.
4. Do not silently map old `claude_path/codex_path` records into a fake single-binding world.

## 4. Current Legacy Gaps

Legacy currently has these structural limits:

1. target selection is modeled as `claude/codex/both`
2. one skill stores at most one Claude path and one Codex path
3. `init` mixes bootstrap, import, and projection
4. observation and capture are not first-class flows
5. documentation and code are already drifting on state layout expectations

## 5. Migration Strategy

### Phase 0: Freeze Terminology

Deliverables:

1. `LOOM_STATE_MODEL.md`
2. this migration document

Acceptance:

1. source registry, binding, target, projection, and history are defined as separate concepts
2. "live directory is not canonical" is documented as a hard rule

### Phase 1: Introduce Registry State Schema

Deliverables:

1. `state/registry/schema.json`
2. `state/registry/bindings.json`
3. `state/registry/targets.json`
4. `state/registry/rules.json`
5. `state/registry/projections.json`
6. `state/registry/ops/*`

Acceptance:

1. registry state readers can load an empty state layout without touching agent directories
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

1. sync built on registry state operation journal
2. replay aware of source revisions and projection events

Acceptance:

1. remote features do not change binding resolution semantics
2. local correctness does not depend on remote configuration

### Phase 8: Panel Convergence

Deliverables:

1. one panel implementation
2. panel views based on registry state

Acceptance:

1. panel does not invent state that CLI cannot express
2. panel is optional for core workflows

## 6. legacy to registry state Mapping

### 6.1 Keep

1. `skills/<skill>` as source registry layout
2. Git-native revisions
3. operation journal as a durable concept

### 6.2 Replace

1. `state/targets.json` -> `state/registry/targets.json` + `bindings.json` + `rules.json` + `projections.json`
2. `Target::Claude|Codex|Both` execution model -> explicit `target_id` and `binding_id`
3. coarse `init/import/link` chaining -> explicit state, registration, projection, and capture steps

### 6.3 Reinterpret

1. existing live directories become candidate targets, not canonical sources
2. `link/use` become projection operations, not identity-defining operations

## 7. Proposed Compatibility Policy

1. registry state may read legacy state only through an explicit migration command.
2. registry state must not silently mutate legacy files in place.
3. If migration support is implemented, it should output a migration report before writing any registry state.

## 8. Proposed Migration Command

Historical note:

```bash
# removed from runtime CLI
# use explicit registry state bootstrap:
loom target add --agent claude --path /Users/foo/.claude/skills --ownership observed
loom target add --agent claude --path /Users/foo/.claude-work/skills --ownership observed
loom target add --agent codex --path /Users/foo/.agents/skills --ownership observed
# Legacy Codex roots may also be registered as observed migration input:
loom target add --agent codex --path /Users/foo/.codex/skills --ownership observed
loom workspace binding add --agent claude --profile <profile> --matcher-kind path-prefix --matcher-value <workspace> --target <target-id>
```

Runtime policy:

1. no in-tool migration subcommand
2. no implicit read of `state/targets.json`
3. multi-directory support is explicit via repeated `target add`

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

## 11. Operation Log Authority Migration

Status: plan for #459 implementation review. Runtime behavior is not migrated
by this document.

### 11.1 Target State

The registry journal becomes the only operation-log write authority:

- durable log: `state/registry/ops/operations.jsonl`
- replay cursor: `state/registry/ops/checkpoint.json`
- history branch mirror: `loom-history`

The legacy pending queue files are removed after sync and ops command families
read and write through the registry journal:

- `state/pending_ops.jsonl`
- `state/pending_ops_snapshot.json`
- `state/pending_ops_history/`

No command may dual-write both models after the migration lands. During the
code migration, a PR may include a temporary adapter only inside the command
family being migrated, but that PR must delete the replaced legacy writer before
merge.

### 11.2 Registry Journal Semantics

Each durable operation record must carry enough information to replace the
pending queue row without reading command audit events:

1. `op_id`: stable idempotency key for retry, purge, replay, and ack.
2. `intent`: command family and action, such as `sync.push` or
   `skill.project`.
3. `status`: `pending`, `running`, `succeeded`, `failed`, or `blocked`.
4. `ack`: whether the operation has been mirrored to the remote authority.
5. `payload`: sanitized command input needed to replay the operation.
6. `effects`: source commits, registry commits, history refs, projection ids,
   and other observed outputs.
7. `last_error`: typed retry guidance for failed or blocked operations.
8. timestamps: append time and last status update time.

`status` is the local execution state. `ack` is the remote synchronization
state. A succeeded operation can still have `ack=false` when local work
completed but the remote push has not.

### 11.3 `loom-history` Interplay

The `loom-history` branch remains the remote reconciliation surface, but it
mirrors registry journal segments instead of pending snapshots.

1. `sync push` reads registry operations with `ack=false`, writes or updates a
   deterministic history segment for those operations, pushes `main` and
   `loom-history`, then marks the successfully pushed operation ids `ack=true`.
2. `sync pull` fetches `main` and `loom-history`, imports remote journal
   segments that are not present locally, fast-forwards local state when
   possible, and leaves conflicting operations as `blocked` with a typed
   conflict error.
3. `sync replay` selects unacked or blocked registry operations by `op_id` and
   replays from `payload` only after validating the current registry head and
   idempotency preimage.
4. `ops history repair` rebuilds missing or corrupt history segments from the
   registry journal when local registry state is trusted, or rebuilds the local
   registry journal from `loom-history` when the history branch is the only
   intact copy.

History repair must never silently drop an operation. If the two sources
disagree and neither side can be proven newer by commit ancestry plus operation
timestamps, the operation becomes `blocked` and requires explicit retry or purge.

### 11.4 Command Family Migration Order

Migrate one command family per PR after this plan is reviewed:

1. Read-only ops views: make `ops list/history diagnose` derive from the
   registry journal while leaving pending writers untouched.
2. `ops retry` and `ops purge`: operate on registry operation ids and update
   `status`, `ack`, and `last_error` in place.
3. `sync push` and `sync pull`: replace pending snapshot reads with registry
   journal queries and history segment import/export.
4. `sync replay`: replay registry payloads with stale-head and idempotency
   checks.
5. Deletion PR: remove `pending_ops.*` files, writers, and repair code once the
   migrated tests cover the old recorded fixtures.

Each code PR must include equivalence tests against recorded pending-queue
fixtures for the command family it migrates.

### 11.5 Failure And Rollback Rules

1. Append failures in the registry journal are terminal for durable mutation
   commands.
2. A failed remote push does not roll back completed local registry operations;
   it leaves `ack=false` and records `last_error`.
3. A failed replay must keep the operation row and update `status=failed` with
   typed guidance.
4. Purge must tombstone or archive the registry operation before deleting any
   replay input needed for audit.
5. Rollback of a migration PR is allowed only before its deletion PR lands. Once
   legacy writers are deleted, rollback means restoring from registry journal
   plus `loom-history`, not reviving pending queue writes.

### 11.6 Verification Matrix

The implementation PRs must keep these checks green:

- `cargo test --test sync --test ops`
- `cargo test reliability`
- `./scripts/e2e-agent-flow.sh`
- fixtures that compare legacy pending queue inputs to registry journal
  outcomes for push, pull, replay, retry, purge, and history repair

The final deletion PR must also prove:

- `rg pending_ops src/` returns only migration notes or test fixtures, or
  returns nothing.
- `docs/LOOM_ARCHITECTURE_DECISIONS.md` section 1 records the registry journal
  as the single operation history authority.

## 12. Risks and Countermeasures

1. Risk: users expect one global Claude directory.
Countermeasure: require explicit bindings and show clear status output.

2. Risk: migration conflates observed directories with managed directories.
Countermeasure: force ownership declaration on registered targets.

3. Risk: legacy state and registry state schema drift further apart.
Countermeasure: stop editing legacy schema docs once registry state spec is adopted.

4. Risk: panel work outruns core model work.
Countermeasure: make panel consume registry state only after CLI semantics stabilize.

## 13. Release Gate

registry state should not be considered ready until:

1. bindings and targets are first-class state objects
2. projection is binding-scoped
3. capture is explicit
4. status can explain binding resolution
5. no core command requires a guessed single agent directory
