# Loom v3 CLI Contract

Updated: 2026-04-08
Status: Draft

## 1. Purpose

This document defines the command contract for Loom v3.

It exists to make three things explicit:

1. what commands exist
2. what selectors must be explicit
3. what JSON shape agents can rely on

This document turns [LOOM_V3_SPEC.md](/Users/lifcc/Desktop/code/work/infra/loom/docs/LOOM_V3_SPEC.md) into a concrete machine-facing interface.

## 2. Contract Principles

1. Every state-changing command must support `--json`.
2. Every command must support `--root <abs-path>`.
3. Projection-related writes must never rely on a guessed default Claude directory.
4. Workspace-scoped writes must identify a `binding_id`.
5. Target-scoped writes must identify a `target_id`.
6. Every successful write returns an `op_id`.
7. Read commands must have zero side effects.

## 3. Naming Rules

Top-level command groups:

1. `workspace`
2. `target`
3. `skill`
4. `sync`
5. `ops`
6. `panel`

Removed from runtime surface:

1. `skill import`
2. `migrate v2-to-v3`

The legacy v2 mental model is rejected:

1. no `Target::Claude|Codex|Both` execution shortcut
2. no hidden path resolution as execution identity
3. no write command keyed only by `agent=claude`

## 4. Global Flags

Required global flags:

```bash
--root <abs-path>
```

Optional global flags:

```bash
--json
--request-id <id>
--strict
--dry-run
```

Rules:

1. Agents should always use `--json`.
2. `--root` is mandatory for automation and examples in this spec.
3. `--dry-run` must not mutate source, targets, bindings, or ops journal.

## 5. Selector Rules

### 5.1 `skill_id`

Represents a canonical source skill under `skills/<skill>`.

### 5.2 `binding_id`

Represents a workspace binding.

Required when:

1. projecting a skill into a workspace context
2. capturing live changes from a workspace context
3. reading workspace-scoped projection health

### 5.3 `target_id`

Represents a concrete registered target directory.

Required when:

1. registering or removing a target
2. explicitly overriding target choice during projection

### 5.4 `instance_id`

Represents one materialized projection instance.

Required when:

1. inspecting one projection instance
2. repairing one projection instance
3. capturing from one specific instance when `skill_id + binding_id` is not unique

## 6. JSON Envelope

All `--json` commands return the same top-level shape:

```json
{
  "ok": true,
  "cmd": "skill.project",
  "request_id": "req_01",
  "version": "3.0.0",
  "data": {},
  "error": null,
  "meta": {
    "op_id": "op_01",
    "warnings": [],
    "sync_state": "LOCAL_ONLY"
  }
}
```

Rules:

1. `ok=true` means the command succeeded.
2. `ok=false` means the command failed and `error` must be populated.
3. `cmd` is the canonical command name, not raw argv text.
4. `request_id` is echoed back if supplied, otherwise generated.
5. `meta.op_id` is required for successful writes and omitted for pure reads.

## 7. Error Object

Failure envelope shape:

```json
{
  "ok": false,
  "cmd": "skill.project",
  "request_id": "req_01",
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

## 8. Error Codes

Base error codes:

1. `ARG_INVALID`
2. `DEPENDENCY_CONFLICT`
3. `BINDING_NOT_FOUND`
4. `TARGET_NOT_FOUND`
5. `SKILL_NOT_FOUND`
6. `INSTANCE_NOT_FOUND`
7. `OWNERSHIP_CONFLICT`
8. `PROJECTION_CONFLICT`
9. `CAPTURE_CONFLICT`
10. `LOCK_BUSY`
11. `STATE_CORRUPT`
12. `SCHEMA_MISMATCH`
13. `REMOTE_UNREACHABLE`
14. `REMOTE_DIVERGED`
15. `PUSH_REJECTED`
16. `REPLAY_CONFLICT`
17. `INTERNAL_ERROR`

Semantics:

1. selector-related failures must be explicit
2. ownership and projection conflicts must not collapse into generic IO errors
3. migration ambiguity must return structured details, not only free-form strings

## 9. Workspace Commands

### 9.1 `workspace status`

```bash
loom --json --root <root> workspace status [--binding <binding-id>|--all-bindings]
```

Read-only.

Response shape:

```json
{
  "bindings": [],
  "targets": [],
  "projections": [],
  "git": {
    "branch": "main",
    "head": "abc123"
  },
  "remote": {
    "configured": false,
    "sync_state": "LOCAL_ONLY"
  }
}
```

Requirements:

1. must explain resolved bindings
2. must explain projection health
3. must not write state

### 9.2 `workspace doctor`

```bash
loom --json --root <root> workspace doctor [--binding <binding-id>|--all-bindings]
```

Read-only unless a future explicit repair subcommand is introduced.

### 9.3 `workspace binding add`

```bash
loom --json --root <root> workspace binding add \
  --agent <claude|codex> \
  --profile <profile-id> \
  --matcher-kind <path_prefix|exact_path|name> \
  --matcher-value <value> \
  --target <target-id>
```

Write command.

Success response:

```json
{
  "binding": {
    "binding_id": "bind_claude_project_a",
    "agent": "claude",
    "profile_id": "default",
    "default_target_id": "target_claude_default"
  }
}
```

Meta requirements:

1. include `op_id`

### 9.4 `workspace binding list`

```bash
loom --json --root <root> workspace binding list
```

Read-only.

### 9.5 `workspace binding remove`

```bash
loom --json --root <root> workspace binding remove <binding-id>
```

Write command.

Rules:

1. must fail if live projections still depend on the binding unless `--force` is explicitly supported

## 10. Target Commands

### 10.1 `target add`

```bash
loom --json --root <root> target add \
  --agent <claude|codex> \
  --path <dir> \
  --ownership <managed|observed|external>
```

Write command.

Rules:

1. registration does not project anything
2. `ownership` must be explicit

### 10.2 `target list`

```bash
loom --json --root <root> target list
```

Read-only.

### 10.3 `target show`

```bash
loom --json --root <root> target show <target-id>
```

Read-only.

### 10.4 `target remove`

```bash
loom --json --root <root> target remove <target-id>
```

Write command.

Rules:

1. removing a target does not delete the underlying directory
2. must fail if active projections or bindings still depend on it unless force semantics are explicitly defined

## 11. Skill Commands

### 11.1 `skill add`

```bash
loom --json --root <root> skill add <path|git-url> --name <skill-id>
```

Write command.

Rules:

1. adds canonical source under `skills/<skill-id>`
2. must fail when target skill already exists

### 11.2 `skill project`

```bash
loom --json --root <root> skill project <skill-id> --binding <binding-id> [--target <target-id>] [--method <symlink|copy|materialize>]
```

Write command.

Success response:

```json
{
  "projection": {
    "instance_id": "inst_loom_bind_claude_project_a",
    "skill_id": "loom",
    "binding_id": "bind_claude_project_a",
    "target_id": "target_claude_default",
    "method": "symlink",
    "materialized_path": "/Users/foo/.../skills/loom",
    "health": "healthy"
  }
}
```

Rules:

1. `binding_id` is mandatory
2. if `--target` is absent, Loom may use `default_target_id` from binding metadata
3. if multiple targets are possible and no default exists, the command must fail explicitly

### 11.3 `skill capture`

```bash
loom --json --root <root> skill capture <skill-id> --binding <binding-id>
```

Optional disambiguating form:

```bash
loom --json --root <root> skill capture --instance <instance-id>
```

Write command.

Success response:

```json
{
  "capture": {
    "skill_id": "loom",
    "binding_id": "bind_claude_project_a",
    "instance_id": "inst_loom_bind_claude_project_a",
    "commit": "abc123"
  }
}
```

Rules:

1. capture is always explicit
2. capture must fail if drift cannot be reconciled safely

### 11.4 `skill save`

```bash
loom --json --root <root> skill save <skill-id>
```

Acts on canonical source only.

### 11.5 `skill snapshot`

```bash
loom --json --root <root> skill snapshot <skill-id>
```

Acts on canonical source only.

### 11.6 `skill release`

```bash
loom --json --root <root> skill release <skill-id> <version>
```

Acts on canonical source only.

### 11.7 `skill rollback`

```bash
loom --json --root <root> skill rollback <skill-id> --to <ref>
```

Acts on canonical source only.

Success response should include:

1. `recovery_ref`
2. resulting source revision

### 11.8 `skill diff`

```bash
loom --json --root <root> skill diff <skill-id> <from> <to>
```

Read-only.

## 12. Sync Commands

### 12.1 `sync status`

```bash
loom --json --root <root> sync status
```

Read-only.

### 12.2 `sync push`

```bash
loom --json --root <root> sync push
```

Write command.

Acts on source and operation history, not on live target directories.

### 12.3 `sync pull`

```bash
loom --json --root <root> sync pull
```

Write command.

### 12.4 `sync replay`

```bash
loom --json --root <root> sync replay
```

Write command.

## 13. Ops Commands

### 13.1 `ops list`

```bash
loom --json --root <root> ops list [--status <queued|running|succeeded|failed>]
```

Read-only.

### 13.2 `ops show`

```bash
loom --json --root <root> ops show <op-id>
```

Read-only.

## 14. Migration Policy

Migration commands are intentionally removed from the runtime CLI surface.

Rules:

1. no in-tool `v2-to-v3` migration entrypoint
2. operators must register targets explicitly with `target add`
3. binding resolution must be explicit with `workspace binding add`

## 15. Response Requirements by Command Type

### 15.1 Pure Reads

Examples:

1. `workspace status`
2. `workspace doctor`
3. `target list`
4. `skill diff`
5. `sync status`
6. `ops list`

Requirements:

1. no `op_id`
2. no write side effects

### 15.2 Writes

Examples:

1. `workspace binding add`
2. `target add`
3. `skill project`
4. `skill capture`
5. `skill save`
6. `sync push`

Requirements:

1. `meta.op_id` is mandatory
2. selector identities must be echoed in `data`

## 16. Minimal Agent Workflow

Recommended agent-safe sequence:

```bash
loom --json --root "$ROOT" workspace binding list
loom --json --root "$ROOT" target list
loom --json --root "$ROOT" skill project model-onboarding --binding bind_claude_project_a
loom --json --root "$ROOT" skill capture model-onboarding --binding bind_claude_project_a
loom --json --root "$ROOT" skill snapshot model-onboarding
```

Why this is safe:

1. binding is explicit
2. projection is explicit
3. capture is explicit
4. versioning stays on source

## 17. Rejected CLI Shapes

These command shapes are explicitly rejected for v3:

1. `loom skill link <skill> --target claude`
2. `loom init --from-agent both --target both`
3. any command that treats `claude` as an execution identity without binding resolution
4. any command that mutates live directories based only on a guessed default path

## 18. Acceptance Criteria

The CLI contract is acceptable only if:

1. every write can be called non-interactively
2. every projection write is binding-scoped
3. every response needed by agents is available in `--json`
4. no core workflow depends on path guessing
5. projection and capture errors are structured and typed
