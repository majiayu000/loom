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

Decision: removing a binding removes control-plane metadata but does not delete live projected files automatically.

Rules:

1. `workspace binding remove <binding_id>` removes the binding, its rules, and its projection metadata from v3 state.
2. If live projection paths still exist, Loom reports them as orphaned paths in warnings/effects.
3. Loom must not silently delete live bytes during binding removal.
4. Orphaned paths are outside active control-plane ownership until a future explicit cleanup or adoption command handles them.

Rationale:

Automatic deletion is too destructive for a command that primarily removes control-plane metadata. Preserving live bytes avoids data loss and keeps the operation reversible by the operator. The cost is that the system must clearly report orphaned paths and must not pretend they are still managed projections.

Follow-up implementation work, if desired:

- add an explicit projection cleanup command
- add an explicit projection adopt/orphan inspection command
- add panel affordances for orphaned paths

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

Authoritative status fields:

- registered v3 targets
- registered v3 bindings
- binding rules
- projection instances
- Git head/branch/remote state
- pending queue count
- v3 operation summaries

Advisory status fields:

- default Claude/Codex skill directory guesses
- `CLAUDE_SKILLS_DIR`
- `CODEX_SKILLS_DIR`
- scanned source or backup skill directories
- local inventory hints not backed by registered v3 entities

Rules:

1. Advisory discovery may help users understand their local filesystem.
2. Advisory discovery must not create targets, bindings, rules, or projections by itself.
3. Mutation commands must use explicit target, binding, skill, and projection identities.
4. API and panel labels should make the distinction between registered state and discovered hints visible.

Rationale:

Loom v3 is an explicit control plane. Ambient environment discovery is useful for onboarding and diagnostics, but it must not silently change what the control plane believes is managed.

## Issue Mapping

- #38 is closed by section 1.
- #39 is closed by section 2.
- #40 is closed by section 3.
- #41 is closed by section 4.
- #42 is closed by section 5.

