# Loom Registry API Contract

Updated: 2026-04-09
Status: Draft

## 1. Purpose

This document defines the read-oriented API contract for Loom Registry.

The API contract exists to support:

1. local panel rendering
2. machine-readable status inspection
3. consistent read models shared with CLI output

This API is not a second source of truth.
It is a projection of registry state and source metadata.

## 2. API Principles

1. API is read-first.
2. API must not invent semantics not present in CLI or state schema.
3. API responses must be stable enough for panel consumption.
4. API must expose bindings, targets, projections, and history explicitly.
5. API must never require guessing a single default Claude directory.

## 3. Transport Assumptions

Initial assumptions:

1. local HTTP server
2. loopback bind only
3. JSON responses only

Base path:

```text
/api/registry
```

## 4. Common Response Shape

Read responses should use a stable top-level envelope:

```json
{
  "ok": true,
  "version": "3.0.0",
  "data": {},
  "error": null,
  "meta": {
    "warnings": []
  }
}
```

Rules:

1. read APIs do not require `op_id`
2. write APIs are intentionally out of scope for this document
3. every list endpoint should be deterministic and explicitly typed

## 5. Error Shape

```json
{
  "ok": false,
  "version": "3.0.0",
  "data": {},
  "error": {
    "code": "BINDING_NOT_FOUND",
    "message": "binding 'bind_x' does not exist",
    "details": {}
  },
  "meta": {
    "warnings": []
  }
}
```

## 6. Resource Model

The API exposes six primary resources:

1. `workspace summary`
2. `bindings`
3. `targets`
4. `projections`
5. `history`
6. `migration plan`

## 7. Endpoints

### 7.1 `GET /api/registry/health`

Purpose:

1. basic service liveness

Response:

```json
{
  "ok": true,
  "version": "3.0.0",
  "data": {
    "service": "loom-panel",
    "healthy": true
  },
  "error": null,
  "meta": {
    "warnings": []
  }
}
```

### 7.2 `GET /api/registry/workspace`

Purpose:

1. top-level summary for the panel overview

Query params:

1. `binding_id` optional
2. `all_bindings=true` optional

Response:

```json
{
  "ok": true,
  "version": "3.0.0",
  "data": {
    "git": {
      "branch": "main",
      "head": "abc123"
    },
    "remote": {
      "configured": false,
      "sync_state": "LOCAL_ONLY"
    },
    "counts": {
      "skills": 91,
      "bindings": 3,
      "targets": 4,
      "projections": 3,
      "pending_ops": 1,
      "drifted_projections": 1
    }
  },
  "error": null,
  "meta": {
    "warnings": []
  }
}
```

Field-level tier classification for all fields returned by this endpoint: see `docs/STATUS_FIELD_CLASSIFICATION.md`.

### 7.3 `GET /api/registry/bindings`

Purpose:

1. list all bindings
2. support bindings page and selectors

Response item shape:

```json
{
  "binding_id": "bind_claude_project_a",
  "agent": "claude",
  "profile_id": "default",
  "workspace_matcher": {
    "kind": "path_prefix",
    "value": "/Users/foo/code/project-a"
  },
  "default_target_id": "target_claude_proj_a",
  "policy_profile": "safe-capture",
  "active": true
}
```

### 7.4 `GET /api/registry/bindings/{binding_id}`

Purpose:

1. show one binding in detail
2. include resolved rules and projection summary

Response:

```json
{
  "ok": true,
  "version": "3.0.0",
  "data": {
    "binding": {
      "binding_id": "bind_claude_project_a",
      "agent": "claude",
      "profile_id": "default"
    },
    "rules": [],
    "projections": []
  },
  "error": null,
  "meta": {
    "warnings": []
  }
}
```

### 7.5 `GET /api/registry/targets`

Purpose:

1. list all targets
2. show ownership and capabilities

Response item shape:

```json
{
  "target_id": "target_claude_proj_a",
  "agent": "claude",
  "path": "/Users/foo/.claude-profiles/project-a/skills",
  "ownership": "managed",
  "capabilities": {
    "symlink": true,
    "copy": true,
    "watch": true
  }
}
```

### 7.6 `GET /api/registry/targets/{target_id}`

Purpose:

1. show one target
2. include dependent bindings and projections

### 7.7 `GET /api/registry/projections`

Purpose:

1. list projections across all bindings
2. filter by health, skill, binding, or target

Query params:

1. `binding_id`
2. `target_id`
3. `skill_id`
4. `health`

Response item shape:

```json
{
  "instance_id": "inst_model-onboarding_claude_b",
  "skill_id": "model-onboarding",
  "binding_id": "bind_claude_project_b",
  "target_id": "target_claude_proj_b",
  "materialized_path": "/Users/foo/.claude-profiles/project-b/skills/model-onboarding",
  "method": "copy",
  "last_applied_rev": "abc123",
  "health": "drifted",
  "observed_drift": true,
  "updated_at": "2026-04-09T10:06:00Z"
}
```

### 7.8 `GET /api/registry/projections/{instance_id}`

Purpose:

1. show one projection instance
2. support detail panel and capture review

Response:

```json
{
  "ok": true,
  "version": "3.0.0",
  "data": {
    "projection": {
      "instance_id": "inst_model-onboarding_claude_b",
      "skill_id": "model-onboarding",
      "binding_id": "bind_claude_project_b",
      "target_id": "target_claude_proj_b",
      "health": "drifted"
    },
    "recent_observations": [],
    "source": {
      "skill_id": "model-onboarding",
      "head": "abc123"
    }
  },
  "error": null,
  "meta": {
    "warnings": []
  }
}
```

### 7.9 `GET /api/registry/history`

Purpose:

1. list operation history for audit and recovery views

Query params:

1. `skill_id`
2. `binding_id`
3. `intent`
4. `status`
5. `limit`

Response item shape:

```json
{
  "op_id": "op_004",
  "intent": "skill.capture",
  "status": "blocked",
  "payload": {
    "skill_id": "model-onboarding",
    "binding_id": "bind_claude_project_b"
  },
  "effects": {
    "instance_id": "inst_model-onboarding_claude_b"
  },
  "last_error": {
    "code": "CAPTURE_CONFLICT",
    "message": "projection drift differs from source head"
  },
  "created_at": "2026-04-09T10:07:00Z",
  "updated_at": "2026-04-09T10:07:00Z"
}
```

### 7.10 `GET /api/registry/skills`

Purpose:

1. list source registry skills
2. support registry page

Response item shape:

```json
{
  "skill_id": "model-onboarding",
  "source_path": "skills/model-onboarding",
  "head": "abc123",
  "projection_count": 3
}
```

### 7.11 `GET /api/registry/skills/{skill_id}`

Purpose:

1. show one source skill
2. include revision summary and projection summary

### 7.12 `GET /api/registry/migration/plan`

Purpose:

1. support migration review UI
2. show unresolved legacy-to-registry ambiguities without writing

Response:

```json
{
  "ok": true,
  "version": "3.0.0",
  "data": {
    "candidate_targets": [],
    "candidate_bindings": [],
    "unresolved": [
      {
        "kind": "binding_ambiguity",
        "source": "state/targets.json",
        "message": "one claude_path cannot be mapped to a single registry binding safely"
      }
    ]
  },
  "error": null,
  "meta": {
    "warnings": []
  }
}
```

## 8. Query and Filtering Rules

Rules:

1. list endpoints must support stable ordering
2. filters must be additive
3. unknown filters must return `ARG_INVALID`
4. ambiguous selectors must return typed errors

## 9. Read Model Rules

The API may aggregate state for convenience, but must not create new truth.

Allowed:

1. counts
2. health summaries
3. grouped history views
4. projection summaries per skill

Not allowed:

1. guessed default binding assignment
2. implicit promotion of observed drift into source change
3. hidden target resolution not explainable via returned fields

## 10. Panel Mapping

Recommended panel pages:

1. `Overview`
   backed by `/api/registry/workspace`
2. `Bindings`
   backed by `/api/registry/bindings`
3. `Targets`
   backed by `/api/registry/targets`
4. `Projections`
   backed by `/api/registry/projections`
5. `History`
   backed by `/api/registry/history`
6. `Migration`
   backed by `/api/registry/migration/plan`

## 11. Compatibility Rules

1. The API must reflect registry state only.
2. If legacy state still exists, API should expose migration review instead of pretending registry resolution already exists.
3. Panel should never call write endpoints that have no CLI equivalent.

## 12. API Acceptance Criteria

The API contract is acceptable only if:

1. panel can render multi-workspace state without path guessing
2. bindings, targets, and projections are all visible as separate resources
3. drift and capture conflicts are queryable
4. migration ambiguity is visible before any apply step
5. the API does not depend on one default Claude directory assumption
