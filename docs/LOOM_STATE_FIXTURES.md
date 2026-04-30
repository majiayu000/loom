# Loom registry model State Fixtures

Updated: 2026-04-09
Status: Draft

## 1. Purpose

This document provides example registry state files.

The goal is not to freeze implementation details yet.
The goal is to prove that the registry model can represent:

1. multiple targets per agent
2. multiple bindings per workspace family
3. multiple projections per skill
4. explicit observation and capture state

This document is a schema exercise companion to:

1. [LOOM_STATE_MODEL.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_STATE_MODEL.md)
2. [LOOM_TEST_PLAN.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_TEST_PLAN.md)
3. [LOOM_CLI_CONTRACT.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_CLI_CONTRACT.md)

## 2. Fixture Layout

```text
state/
  registry/
    schema.json
    targets.json
    bindings.json
    rules.json
    projections.json
    ops/
      operations.jsonl
      checkpoint.json
    observations/
      events-20260409.jsonl
```

## 3. Example Scenario

This fixture models:

1. one Loom source registry
2. two Claude workspaces
3. one Codex workspace
4. one shared skill projected into all three
5. mixed projection methods
6. one observed external target

## 4. `schema.json`

```json
{
  "schema_version": 1,
  "created_at": "2026-04-09T10:00:00Z",
  "writer": "loom/registry-draft"
}
```

## 5. `targets.json`

```json
{
  "schema_version": 1,
  "targets": [
    {
      "target_id": "target_claude_proj_a",
      "agent": "claude",
      "path": "/Users/foo/.claude-profiles/project-a/skills",
      "ownership": "managed",
      "capabilities": {
        "symlink": true,
        "copy": true,
        "watch": true
      },
      "created_at": "2026-04-09T10:00:00Z"
    },
    {
      "target_id": "target_claude_proj_b",
      "agent": "claude",
      "path": "/Users/foo/.claude-profiles/project-b/skills",
      "ownership": "managed",
      "capabilities": {
        "symlink": true,
        "copy": true,
        "watch": true
      },
      "created_at": "2026-04-09T10:00:00Z"
    },
    {
      "target_id": "target_codex_default",
      "agent": "codex",
      "path": "/Users/foo/.codex/skills",
      "ownership": "managed",
      "capabilities": {
        "symlink": true,
        "copy": true,
        "watch": true
      },
      "created_at": "2026-04-09T10:00:00Z"
    },
    {
      "target_id": "target_external_archive",
      "agent": "claude",
      "path": "/Users/foo/archive/legacy-skills",
      "ownership": "observed",
      "capabilities": {
        "symlink": false,
        "copy": false,
        "watch": true
      },
      "created_at": "2026-04-09T10:00:00Z"
    }
  ]
}
```

## 6. `bindings.json`

```json
{
  "schema_version": 1,
  "bindings": [
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
      "active": true,
      "created_at": "2026-04-09T10:00:00Z"
    },
    {
      "binding_id": "bind_claude_project_b",
      "agent": "claude",
      "profile_id": "default",
      "workspace_matcher": {
        "kind": "path_prefix",
        "value": "/Users/foo/code/project-b"
      },
      "default_target_id": "target_claude_proj_b",
      "policy_profile": "safe-capture",
      "active": true,
      "created_at": "2026-04-09T10:00:00Z"
    },
    {
      "binding_id": "bind_codex_workbench",
      "agent": "codex",
      "profile_id": "default",
      "workspace_matcher": {
        "kind": "path_prefix",
        "value": "/Users/foo/code/workbench"
      },
      "default_target_id": "target_codex_default",
      "policy_profile": "safe-capture",
      "active": true,
      "created_at": "2026-04-09T10:00:00Z"
    }
  ]
}
```

## 7. `rules.json`

```json
{
  "schema_version": 1,
  "rules": [
    {
      "binding_id": "bind_claude_project_a",
      "skill_id": "model-onboarding",
      "target_id": "target_claude_proj_a",
      "method": "symlink",
      "watch_policy": "observe_only",
      "created_at": "2026-04-09T10:00:00Z"
    },
    {
      "binding_id": "bind_claude_project_b",
      "skill_id": "model-onboarding",
      "target_id": "target_claude_proj_b",
      "method": "copy",
      "watch_policy": "observe_only",
      "created_at": "2026-04-09T10:00:00Z"
    },
    {
      "binding_id": "bind_codex_workbench",
      "skill_id": "model-onboarding",
      "target_id": "target_codex_default",
      "method": "symlink",
      "watch_policy": "observe_only",
      "created_at": "2026-04-09T10:00:00Z"
    }
  ]
}
```

## 8. `projections.json`

```json
{
  "schema_version": 1,
  "projections": [
    {
      "instance_id": "inst_model-onboarding_claude_a",
      "skill_id": "model-onboarding",
      "binding_id": "bind_claude_project_a",
      "target_id": "target_claude_proj_a",
      "materialized_path": "/Users/foo/.claude-profiles/project-a/skills/model-onboarding",
      "method": "symlink",
      "last_applied_rev": "abc123",
      "health": "healthy",
      "observed_drift": false,
      "updated_at": "2026-04-09T10:05:00Z"
    },
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
    },
    {
      "instance_id": "inst_model-onboarding_codex",
      "skill_id": "model-onboarding",
      "binding_id": "bind_codex_workbench",
      "target_id": "target_codex_default",
      "materialized_path": "/Users/foo/.codex/skills/model-onboarding",
      "method": "symlink",
      "last_applied_rev": "abc123",
      "health": "healthy",
      "observed_drift": false,
      "updated_at": "2026-04-09T10:05:00Z"
    }
  ]
}
```

## 9. `operations.jsonl`

Example lines:

```json
{"op_id":"op_001","intent":"target.add","status":"succeeded","ack":false,"payload":{"target_id":"target_claude_proj_a"},"effects":{},"created_at":"2026-04-09T10:00:00Z","updated_at":"2026-04-09T10:00:00Z"}
{"op_id":"op_002","intent":"workspace.binding.add","status":"succeeded","ack":false,"payload":{"binding_id":"bind_claude_project_a"},"effects":{},"created_at":"2026-04-09T10:01:00Z","updated_at":"2026-04-09T10:01:00Z"}
{"op_id":"op_003","intent":"skill.project","status":"succeeded","ack":false,"payload":{"skill_id":"model-onboarding","binding_id":"bind_claude_project_a"},"effects":{"instance_id":"inst_model-onboarding_claude_a"},"created_at":"2026-04-09T10:05:00Z","updated_at":"2026-04-09T10:05:00Z"}
{"op_id":"op_004","intent":"skill.capture","status":"blocked","ack":false,"payload":{"skill_id":"model-onboarding","binding_id":"bind_claude_project_b"},"effects":{"instance_id":"inst_model-onboarding_claude_b"},"last_error":{"code":"CAPTURE_CONFLICT","message":"projection drift differs from source head"},"created_at":"2026-04-09T10:07:00Z","updated_at":"2026-04-09T10:07:00Z"}
```

## 10. `checkpoint.json`

```json
{
  "schema_version": 1,
  "last_scanned_op_id": "op_004",
  "last_acked_op_id": null,
  "updated_at": "2026-04-09T10:07:00Z"
}
```

## 11. `events-20260409.jsonl`

Example lines:

```json
{"event_id":"obs_001","instance_id":"inst_model-onboarding_claude_b","kind":"file_changed","path":"SKILL.md","observed_at":"2026-04-09T10:06:30Z"}
{"event_id":"obs_002","instance_id":"inst_model-onboarding_claude_b","kind":"health_changed","from":"healthy","to":"drifted","observed_at":"2026-04-09T10:06:31Z"}
```

## 12. Fixture Assertions

This fixture proves:

1. one skill can map to three projection instances
2. one agent kind can have multiple targets and bindings
3. projection method is target-scoped, not skill-scoped
4. drift is recorded per projection instance
5. capture conflict is representable without promoting live content into source

## 13. Invalid Fixture Patterns

These patterns should fail schema or contract review:

1. one skill record containing only `claude_path` and `codex_path`
2. path-only identities without `binding_id` or `target_id`
3. projection records without `last_applied_rev`
4. capture records without `instance_id` or `binding_id`

## 14. Recommended Next Use

These fixtures should be used in:

1. schema review
2. CLI response examples
3. migration dry-run examples
4. panel read-model examples
