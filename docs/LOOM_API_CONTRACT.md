# Loom Panel API Contract

Updated: 2026-06-10
Status: Accepted for v1 Panel surface

## 1. Purpose

This document defines the local HTTP API used by the Loom Panel.

The API supports:

1. local Panel rendering
2. machine-readable status inspection
3. CLI-backed Panel mutations

The API is not a second source of truth. Reads project registry state and CLI
read models. Writes execute existing CLI command semantics through the Panel
backend.

## 2. Base Path

All Panel API routes are under:

```text
/api/v1
```

Unversioned routes such as `/api/health`, `/api/info`, `/api/pending`, flat
`/api/ops/*`, flat `/api/sync/*`, `/api/remote/*`, and `/api/registry/*` are not
part of the API contract.

## 3. Envelope

Read and write responses use the CLI envelope shape:

```json
{
  "ok": true,
  "cmd": "registry.status",
  "request_id": "req-id",
  "version": "0.1.3",
  "data": {},
  "error": null,
  "meta": {
    "warnings": []
  }
}
```

Errors use the same top-level shape with `ok: false` and a typed `error.code`.
`error.next_actions[]` is optional and contains runnable `{cmd, reason}` hints
when Loom can guide the caller to a recovery command. The field is omitted
when there is no specific recovery hint.

## 4. Read Routes

```text
GET /api/v1/health
GET /api/v1/overview
GET /api/v1/workspace/status
GET /api/v1/workspace/info
GET /api/v1/workspace/doctor

GET /api/v1/registry/status
GET /api/v1/skills
GET /api/v1/skills/trash
GET /api/v1/skills/{skill_name}/diagnose
GET /api/v1/skills/{skill_name}/history
GET /api/v1/skills/{skill_name}/diff

GET /api/v1/targets
GET /api/v1/targets/{target_id}
GET /api/v1/bindings
GET /api/v1/bindings/{binding_id}
GET /api/v1/projections

GET /api/v1/ops
GET /api/v1/ops/diagnose
GET /api/v1/ops/pending
GET /api/v1/sync/status
```

`/api/v1/workspace/info` exposes Panel bootstrap metadata such as registry root,
state paths, agent directory defaults, and the redacted remote URL.

`/api/v1/ops/pending` is the v1 compatibility route for replayable registry
operation backlog rows. It remains separate from `/api/v1/ops`, which is the
activity/audit read model.

`GET /api/v1/workspace/status`, `GET /api/v1/overview`,
`GET /api/v1/skills/{skill_name}/diagnose`, and the single-Skill inspect API
consume the CLI three-axis `data.convergence` read model unchanged:
`registry_transport`, `projections`, and `visibility`. Panel code must not infer
projection or visibility from a registry transport state. For an older server
that omits `convergence`, Panel may display its legacy remote state only as
registry transport; projection convergence and agent visibility must both be
shown as `unknown`.

`GET /api/v1/sync/status` returns `data.registry_transport` and preserves
`data.remote` as a compatibility mirror. Registry transport covers Git remote
and operation backlog only.

The pending route, `workspace/status`, `workspace/doctor`, and `sync/status`
share this non-overlapping counter model:

```json
{
  "operation_counts": {
    "actionable_operations": 0,
    "local_journal_events": 3,
    "unpushed_history_events": 0,
    "local_only_history_events": 400
  }
}
```

- `actionable_operations` is the number of rows returned by
  `/api/v1/ops/pending.data.ops`.
- `local_journal_events` contains succeeded, unacknowledged rows while no
  origin is configured.
- `unpushed_history_events` is the unique local history event-ID set minus the
  cached `origin/loom-history` set when an origin is configured.
- `local_only_history_events` is the unique local history event-ID count when
  no origin is configured.

Compatibility fields remain additive projections: `count` and
`operation_backlog` equal `actionable_operations`; `journal_events` is the sum
of the two journal buckets; `history_events` is the sum of the two history
buckets. Read routes never fetch. Remote comparisons therefore describe the
cached tracking ref and may be stale relative to the server. Existing malformed
registry or history data produces a structured error instead of zero counters.

## 5. Mutation Routes

The complete v1 mutation surface is:

```text
POST /api/v1/workspace/init
POST /api/v1/workspace/remote

POST /api/v1/targets
POST /api/v1/targets/{target_id}/remove
POST /api/v1/bindings
POST /api/v1/bindings/{binding_id}/remove

POST /api/v1/skills
POST /api/v1/skills/import-observed
POST /api/v1/skills/{skill_name}/save
POST /api/v1/skills/{skill_name}/snapshot
POST /api/v1/skills/{skill_name}/release
POST /api/v1/skills/{skill_name}/rollback
POST /api/v1/skills/{skill_name}/use
POST /api/v1/skills/{skill_name}/trash
POST /api/v1/skills/trash/{trash_id}/restore
POST /api/v1/skills/trash/{trash_id}/purge

POST /api/v1/projections/project
POST /api/v1/projections/capture
POST /api/v1/orphans/clean

POST /api/v1/ops/retry
POST /api/v1/ops/purge
POST /api/v1/ops/history/repair

POST /api/v1/sync/push
POST /api/v1/sync/pull
POST /api/v1/sync/replay
```

Every mutation must pass through `ensure_mutation_authorized` and
`run_panel_command`, preserving CLI locking, audit logging, error mapping, and
envelope semantics.

## 6. Rules

1. Panel routes must not invent semantics absent from CLI or registry state.
2. Reads must not mutate registry state, Git refs/index, target directories, or
   the operation backlog.
3. Mutations must remain CLI-backed and must not define a second write model.
4. Unknown enum-like values in read models must render explicitly instead of
   being silently coerced to a known value.
5. New Panel routes must be v1 routes. Do not add unversioned compatibility
   aliases.
6. Partial or failed convergence collection must preserve each completed axis,
   include structured axis errors and `incomplete_axes`, and never be rendered
   as clean convergence.

## 7. Telemetry Read Model

`GET /api/v1/telemetry/report` returns the same local, redacted report shape as
`loom telemetry report`. It accepts the same read filters: `skill`, `skillset`,
`agent`, `workspace`, and `since`. Missing eval, safety, dependency, and
recommendation evidence remains `missing` rather than zero-valued. Panel
Telemetry renders this route directly and must not invent analytics fields that
are absent from the CLI/API read model.
