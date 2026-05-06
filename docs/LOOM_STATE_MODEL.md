# Loom registry model Spec

Updated: 2026-04-08
Status: Draft

## 1. Summary

Loom registry model stops modeling the world as "one Loom repo plus one Claude path plus one Codex path".

Loom registry model is a local Skill Control Plane with four layers:

1. `Source Registry`
2. `Workspace Binding`
3. `Projection`
4. `History`

The main design correction is simple:

1. Skill content has one canonical source of truth.
2. Agent live directories are projections, not truth.
3. Claude/Codex workspace selection must be explicit.
4. Versioning, projection, and observation must be separate concerns.

## 2. Problem Statement

Loom legacy design still assumes that:

1. one agent kind maps to one target directory
2. a skill can be represented by one `claude_path` and one `codex_path`
3. `init/import/link` can be chained as one coarse-grained bootstrap action
4. a live agent directory can safely act as the operational target without a workspace model

These assumptions fail when:

1. Claude Code uses multiple workdirs
2. one skill must be projected into multiple workspaces at once
3. live edits happen outside Loom
4. different targets use different projection methods or ownership modes

## 3. Goals

1. Make `SkillSource` the only canonical content source.
2. Support multiple workspaces, profiles, and targets per agent.
3. Allow one skill to be projected into many targets safely.
4. Track live edits without treating live directories as canonical.
5. Give agents a stable, explicit, machine-friendly command contract.
6. Keep Git as the revision backend for skill content.

## 4. Non-Goals

1. registry state does not make panel the primary control surface.
2. registry state does not auto-rewrite arbitrary agent directories by default.
3. registry state does not implement implicit bidirectional sync between live directories and source.
4. registry state does not require a hosted registry or database.

## 5. Design Principles

1. Read paths must have zero side effects.
2. Live directories are derived state.
3. Destructive projection changes require explicit intent and recovery points.
4. Default discovery is advisory only. Execution requires explicit target or binding identity.
5. Every write returns an `op_id` and records an operation event.
6. Git owns revisions. Loom owns bindings, projections, and operation history.

## 6. Core Concepts

### 6.1 Source Registry

The source registry stores canonical skills under the Loom root, for example:

```text
skills/<skill>/
```

Git commits, tags, snapshots, and releases apply only to source registry content.

### 6.2 Projection Target

A projection target is a concrete location where a skill can be materialized for an agent.

Examples:

1. a Claude profile skill directory
2. a Codex-specific skill directory
3. a manually managed external directory observed by Loom

### 6.3 Workspace Binding

A workspace binding answers:

1. which agent session or profile is this
2. which workdir or matcher identifies it
3. which targets should it use by default
4. which policy profile applies

This is the missing layer in legacy.

### 6.4 Projection

A projection is the applied result of taking a source skill and materializing it into a target for a binding.

Projection methods:

1. `symlink`
2. `copy`
3. `materialize`

### 6.5 History

History is the durable event trail for:

1. source changes
2. binding changes
3. projection changes
4. capture and reconcile operations
5. sync and replay operations

## 7. Canonical Invariants

1. `SkillSource` is always canonical.
2. `ProjectionInstance` is always derived.
3. A live edit does not become canonical until `capture` succeeds.
4. `binding_id` and `target_id` are stable identities. Absolute paths are data, not identity.
5. A destructive projection command must not run against an unmanaged target without explicit adoption.

## 8. Entity Model

### 8.1 SkillSource

```json
{
  "skill_id": "loom",
  "source_path": "skills/loom",
  "repo_ref": "main",
  "fingerprint": "sha256:...",
  "owner": "loom"
}
```

### 8.2 Revision

Git-native object summary stored as Loom metadata:

```json
{
  "skill_id": "loom",
  "kind": "commit|snapshot|release",
  "ref": "release/loom/legacy.2.0",
  "commit": "abc123",
  "created_at": "2026-04-08T12:00:00Z"
}
```

### 8.3 ProjectionTarget

```json
{
  "target_id": "target_claude_default",
  "agent": "claude",
  "path": "/Users/foo/.../skills",
  "ownership": "managed",
  "capabilities": {
    "symlink": true,
    "copy": true,
    "watch": true
  }
}
```

`ownership` values:

1. `managed`
2. `observed`
3. `external`

### 8.4 WorkspaceBinding

```json
{
  "binding_id": "bind_claude_project_a",
  "agent": "claude",
  "profile_id": "default",
  "workspace_matcher": {
    "kind": "path_prefix",
    "value": "/Users/foo/code/project-a"
  },
  "default_target_id": "target_claude_default",
  "policy_profile": "safe-capture",
  "active": true
}
```

### 8.5 BindingRule

```json
{
  "binding_id": "bind_claude_project_a",
  "skill_id": "loom",
  "target_id": "target_claude_default",
  "method": "symlink",
  "watch_policy": "observe_only"
}
```

### 8.6 ProjectionInstance

```json
{
  "instance_id": "inst_loom_bind_claude_project_a",
  "skill_id": "loom",
  "binding_id": "bind_claude_project_a",
  "target_id": "target_claude_default",
  "materialized_path": "/Users/foo/.../skills/loom",
  "method": "symlink",
  "last_applied_rev": "abc123",
  "health": "healthy"
}
```

### 8.7 ObservationEvent

```json
{
  "event_id": "obs_01",
  "instance_id": "inst_loom_bind_claude_project_a",
  "kind": "file_changed",
  "path": "SKILL.md",
  "observed_at": "2026-04-08T12:01:00Z"
}
```

### 8.8 OperationJournal

```json
{
  "op_id": "op_01",
  "intent": "skill.capture",
  "status": "succeeded",
  "ack": false,
  "payload": {
    "skill_id": "loom",
    "binding_id": "bind_claude_project_a"
  },
  "effects": {
    "commit": "abc123"
  }
}
```

## 9. State Layout

```text
state/
  registry/
    schema.json
    bindings.json
    targets.json
    rules.json
    projections.json
    observations/
      events-YYYYMMDD.jsonl
    ops/
      operations.jsonl
      checkpoint.json
```

### 9.1 `bindings.json`

Stores `WorkspaceBinding`.

### 9.2 `targets.json`

Stores `ProjectionTarget`.

### 9.3 `rules.json`

Stores `BindingRule`.

### 9.4 `projections.json`

Stores current `ProjectionInstance` state and health.

## 10. Layer Responsibilities

### 10.1 Versioning Layer

Owns:

1. `save`
2. `snapshot`
3. `release`
4. `rollback`
5. `diff`

Acts only on source registry content.

### 10.2 Projection Layer

Owns:

1. target registration
2. workspace binding
3. projection apply
4. projection repair
5. ownership checks

Does not define canonical content.

### 10.3 Observation Layer

Owns:

1. drift detection
2. file watching
3. change notifications
4. capture preparation

Does not mutate source automatically.

### 10.4 History Layer

Owns:

1. operation journal
2. replay bookkeeping
3. audit trail
4. recovery references

## 11. Command Surface

### 11.1 Workspace

```bash
loom workspace status [--binding <id>|--all-bindings]
loom workspace doctor [--binding <id>|--all-bindings]
loom workspace binding add --agent <agent> --profile <id> --matcher-kind <kind> --matcher-value <value> --target <target-id>
loom workspace binding list
loom workspace binding remove <binding-id>
```

### 11.2 Target

```bash
loom target add --agent <agent> --path <dir> [--ownership managed|observed|external]
loom target list
loom target show <target-id>
loom target remove <target-id>
```

### 11.3 Skill

```bash
loom skill import --source <dir>
loom skill import --from-binding <binding-id> [--skill <name>]
loom skill project <skill> --binding <binding-id> [--method symlink|copy|materialize]
loom skill capture <skill> --binding <binding-id>
loom skill save <skill>
loom skill snapshot <skill>
loom skill release <skill> <version>
loom skill rollback <skill> --to <ref>
loom skill diff <skill> <from> <to>
```

### 11.4 Sync and Ops

```bash
loom sync status
loom sync push
loom sync pull
loom sync replay
loom ops list
loom ops show <op-id>
```

## 12. Command Semantics

1. `workspace binding add` declares binding metadata only. It does not project anything.
2. `target add` registers a concrete directory and ownership mode only. It does not infer workspace semantics.
3. `skill project` materializes one skill into one binding and produces or updates one projection instance.
4. `skill capture` is the explicit bridge from live projection changes back into canonical source.
5. `workspace status` must report resolved bindings, targets, and projection health, not just Git status.

## 13. Safety Model

1. No implicit overwrite of unmanaged targets.
2. No implicit conversion of live changes into source commits.
3. `capture` requires explicit skill and binding identity.
4. Destructive projection changes require a recovery point.
5. Ownership mismatch returns a structured error instead of silently replacing content.

## 14. Observation and Capture

Observation policy values:

1. `off`
2. `observe_only`
3. `observe_and_warn`

registry state intentionally does not define `auto_capture`.

Reason:

1. live edits are facts
2. auto-promotion of live facts into canonical truth is unsafe

Recommended flow:

1. observe drift
2. show diff
3. run `capture`
4. create Git commit in source registry
5. update projection instances as needed

## 15. JSON Contract

Every command must support:

1. `--json`
2. `--root <abs-path>`
3. explicit binding or target selectors where relevant

Required envelope:

```json
{
  "ok": true,
  "cmd": "skill.project",
  "request_id": "req_01",
  "version": "<loom-version>",
  "data": {},
  "error": null,
  "meta": {
    "op_id": "op_01"
  }
}
```

Recommended data additions:

1. `binding_id`
2. `target_id`
3. `instance_id`
4. `recovery_ref`
5. `sync_state`

## 16. Panel Information Architecture

Panel is optional and secondary.

The panel should expose:

1. `Registry`
2. `Bindings`
3. `Projections`
4. `History`
5. `Conflicts`

The panel must not own any operation that cannot also be expressed via CLI.

## 17. Explicit Rejections

registry state rejects these designs:

1. one global `claude` directory assumption
2. one `targets.json` record per skill with only `claude_path/codex_path`
3. implicit two-way sync between live target and source registry
4. panel-first workflows
5. path-only identity without `binding_id` or `target_id`

## 18. Acceptance Criteria

1. One skill can be projected into multiple bindings at once.
2. One agent can have multiple bindings without path collision in Loom state.
3. Live edits can be observed and captured without redefining canonical truth.
4. `workspace status` can explain which binding resolved to which target and projection instance.
5. No command requires Loom to guess a single default Claude directory.
