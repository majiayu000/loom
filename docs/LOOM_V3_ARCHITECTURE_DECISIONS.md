# Loom v3 Architecture Decisions

Updated: 2026-04-27
Status: Accepted for phase 1

This document closes the current design-debt split from issue #6. It freezes the phase-1 boundaries for operation history, v3 vocabulary rules, projection removal, panel mutations, and environment-based discovery.

These decisions describe the contract Loom should preserve while implementation continues. They do not imply that every future migration or cleanup is already implemented.

## 1. Operation History Authority

Decision: phase 1 keeps the legacy pending queue and history branch as the operational authority for sync, replay, pending queue maintenance, and history repair. The v3 operation journal is the activity/audit read model.

Authoritative for sync and replay:

- `state/pending_ops.jsonl`
- `state/pending_ops_snapshot.json`
- `state/pending_ops_history/`
- `loom-history`

Authoritative for v3 panel activity and audit display:

- `state/v3/ops/operations.jsonl`
- `state/v3/ops/checkpoint.json`

Rules:

1. `sync push`, `sync pull`, `sync replay`, `ops retry`, `ops purge`, and `ops history repair` continue to operate on the pending/history model.
2. `/api/v3/ops` exposes bounded summaries from the v3 operation journal for activity history.
3. `/api/ops/retry` and `/api/ops/purge` are flat pending-queue maintenance endpoints, not v3 op-id endpoints.
4. A future migration may make v3 operations authoritative, but that requires a separate migration plan and compatibility story.

Rationale:

The current implementation already has working sync/replay semantics around pending ops and the `loom-history` branch. Treating v3 ops as authoritative before migration would create two write authorities. Keeping v3 ops as the read model avoids that split while still giving the panel a stable activity surface.

## 2. V3 Vocabularies And Cardinality

Decision: phase 1 freezes writer vocabularies where the CLI already owns the field, keeps `policy_profile` as a constrained slug namespace, and enforces one active projection per `skill_id + binding_id`.

### 2.1 Writer-owned vocabularies

Writers must emit only these values:

- `ownership`: `managed`, `observed`, `external`
- `method`: `symlink`, `copy`, `materialize`
- `watch_policy`: `off`, `observe_only`, `observe_and_warn`
- `health`: `healthy`, `drifted`, `missing`, `conflict`

`agent` is owned by the CLI `AgentKind` enum. JSON readers should preserve unknown agent strings for forward compatibility, but CLI and panel write paths must only write known `AgentKind` values.

### 2.2 Policy profiles

`policy_profile` is not a closed enum in phase 1. It is a constrained slug:

```text
[a-z0-9_-]{1,64}
```

Built-in profiles currently reserved by convention:

- `safe-capture`
- `read-only`
- `manual-review`

Unknown but syntactically valid profiles may be stored so local operators can extend policy names without a schema migration. UI surfaces should not invent behavior for an unknown profile; they should render it as a label unless runtime policy handling exists.

### 2.3 Cardinality

Phase 1 allows one active projection per:

```text
skill_id + binding_id
```

That projection has one `target_id` and one `method`. Updating the target or method for the same `skill_id + binding_id` replaces the active projection metadata instead of adding a second active projection.

Fan-out remains possible by creating multiple bindings. Multi-target fan-out inside a single binding is a future extension.

## 3. Projection Lifecycle On Binding Removal

Decision: removing a binding removes control-plane metadata but does not delete live projected files automatically. Projection records are preserved in an orphaned state so the control plane retains visibility.

Rules:

1. `workspace binding remove <binding_id>` removes the binding and its rules from v3 state; projection records for that binding are marked `health = "orphaned"` and `binding_id = null` rather than deleted.
2. If live projection paths still exist, Loom reports them as orphaned paths in warnings/effects.
3. Loom must not silently delete live bytes during binding removal.
4. Orphaned projections (health = "orphaned", binding_id = null) remain in the control plane until `loom skill orphan clean` is run.
5. `loom skill orphan clean` removes orphaned projection metadata by default and preserves live paths.
6. Live path deletion requires `loom skill orphan clean --delete-live-paths` and is limited to validated directories under the projection's registered target.
7. `GET /api/v3/projections?health=orphaned` lists all orphaned projections for panel display.

Rationale:

Automatic deletion is too destructive for a command that primarily removes control-plane metadata. Preserving projection records (as orphaned) avoids data loss and keeps the operation visible in the control plane. The explicit `orphan clean` command gives operators a deliberate, audited path to final metadata removal; live byte deletion requires a separate flag and path validation.

See section 6 for the full orphan lifecycle decision and alternative analysis.

## 4. Panel Mutation Contract

Decision: panel mutations are allowed in phase 1 only when they execute existing CLI command semantics through the panel backend. The panel must not define an independent write model.

Rules:

1. Every panel mutation route must pass through `ensure_mutation_authorized`.
2. Every panel mutation route must use `run_panel_command` or an equivalent wrapper that preserves the CLI envelope, lock acquisition, audit logging, and error mapping.
3. The panel must hide or disable mutation actions when the backend is not live.
4. Offline, stale, mock, and read-only modes must not expose a second path to write APIs through shortcuts or command palette actions.
5. `/api/v3/*` remains read-oriented unless a future decision explicitly adds v3 write semantics.
6. Flat write routes such as `/api/ops/retry`, `/api/ops/purge`, and `/api/sync/replay` are compatibility/control routes backed by CLI command behavior.

Non-goal:

This decision does not make the panel the primary control plane. The CLI remains the authoritative write contract.

## 5. Environment-Based Discovery

Decision: environment-based discovery is advisory. Registered v3 state is authoritative.

Authoritative status fields (sourced from `state/v3/` JSON files or derived exclusively
from registered v3 entities):

- `schema_version`
- `counts.skills`, `counts.targets`, `counts.bindings`, `counts.active_bindings`
- `counts.rules`, `counts.projections`, `counts.operations`
- `targets[]`, `bindings[]`, `rules[]`
- `projections[].health` and all other stored projection sub-fields
- `checkpoint`

Advisory status fields (computed from comparisons or heuristics; useful for UX but must
not drive control-plane decisions):

- `counts.drifted_projections`
- `projections[].observed_drift`
- `projections[].last_applied_rev`
- default Claude/Codex skill directory guesses
- `CLAUDE_SKILLS_DIR`, `CODEX_SKILLS_DIR`
- scanned source or backup skill directories
- local inventory hints not backed by registered v3 entities

For the field-level tier table, source citations, and env-discovery variable reference,
see `docs/STATUS_FIELD_CLASSIFICATION.md`.

Rules:

1. Advisory discovery may help users understand their local filesystem.
2. Advisory discovery must not create targets, bindings, rules, or projections by itself.
3. Mutation commands must use explicit target, binding, skill, and projection identities.
4. API and panel labels should make the distinction between registered state and discovered hints visible.

Rationale:

Loom v3 is an explicit control plane. Ambient environment discovery is useful for onboarding and diagnostics, but it must not silently change what the control plane believes is managed.

## 6. Orphan Lifecycle: Three Alternatives And Decision

Decision: adopt **preserve-as-orphan** when a binding is removed. Projections are kept as orphaned records until an explicit cleanup command removes them.

### Alternatives considered

**Option A — Auto-delete on removal**

On `workspace binding remove`, automatically delete both projection metadata and live filesystem paths.

Rejected because: binding removal is a control-plane operation; silently destroying live bytes is irreversible and destructive for an operation whose primary purpose is removing a registration. A user removing a stale binding should not lose files without an explicit choice.

**Option B — Preserve-as-orphan (chosen)**

On `workspace binding remove`, remove the binding record and its rules. For each projection that belonged to the binding, set `health = "orphaned"` and `binding_id = null`. The projection record and its live filesystem path remain intact. A separate `loom skill orphan clean` command explicitly removes orphaned metadata. Operators may also pass `--delete-live-paths` to delete validated live projection directories under registered targets.

Chosen because: projection records stay visible in the control plane (discoverable via `GET /api/v3/projections?health=orphaned`); operators retain full control over when files are actually deleted; the audit journal records both the orphaning event and the eventual cleanup event; the operation is recoverable (re-project the skill to a binding to restore managed status).

**Option C — Require operator choice at removal time**

Require the user to pass `--orphan` or `--delete` at removal time.

Rejected because: adding a required decision to every binding removal creates friction without improving safety beyond option B; operators can always run `orphan clean --delete-live-paths` immediately if they want explicit delete semantics.

### State model

`V3ProjectionInstance.binding_id` becomes `Option<String>`:

- `Some(id)`: projection is owned by the named binding.
- `None`: projection is orphaned (its binding was removed).

`#[serde(default)]` ensures existing `projections.json` files with non-null `binding_id` continue to deserialize without a schema migration.

### Cleanup command

`loom skill orphan clean`:

1. Lists all projections where `binding_id = null` and `health = "orphaned"`.
2. Removes the projection record from `projections.json`.
3. Preserves live filesystem paths by default.
4. With `--delete-live-paths`, removes only non-symlink directories whose canonical path is inside the projection's registered target and is not the target root.
5. Records `skill.orphan.clean` in the audit journal with cleaned projection ids, deleted paths, skipped paths, and the delete flag.

### Panel surface

`GET /api/v3/projections?health=orphaned` returns orphaned projection records so the panel can surface an orphaned-count badge and a "Clean up" action on the Bindings page.

## Issue Mapping

- #38 is closed by section 1.
- #39 is closed by section 2.
- #40 is closed by sections 3 and 6.
- #41 is closed by section 4.
- #42 is closed by section 5.
