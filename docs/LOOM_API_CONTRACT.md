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

`/api/v1/ops/pending` exposes the replayable pending queue read model. It remains
separate from `/api/v1/ops`, which is the activity/audit read model.

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
   the pending queue.
3. Mutations must remain CLI-backed and must not define a second write model.
4. Unknown enum-like values in read models must render explicitly instead of
   being silently coerced to a known value.
5. New Panel routes must be v1 routes. Do not add unversioned compatibility
   aliases.

## 7. Deferred Telemetry Read Model

Telemetry analytics are implemented first through `loom telemetry report`. A
future `GET /api/v1/telemetry/report` route must return the same local,
redacted report shape and must keep missing eval, safety, dependency, and
recommendation evidence as `missing` rather than inventing zero-valued fields.
Panel dashboard rendering remains deferred until that route is implemented and
tested.
