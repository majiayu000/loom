# Loom V1 Core Spec

Status: Implemented
Date: 2026-05-18
Target branch baseline: `origin/main@bea78fc`
Tracking issue: https://github.com/majiayu000/loom/issues/98
Acceptance matrix: [LOOM_V1_ACCEPTANCE.md](LOOM_V1_ACCEPTANCE.md)

## 1. Decision

Loom V1 is a Git-backed skill registry and projection control plane for AI coding
agents.

V1 may break existing CLI, API, state, and Panel behavior. Backward compatibility
is not a goal. Correctness, explicitness, auditability, and product coherence are
higher priority than preserving phase-1 contracts.

## 2. Product Thesis

Agent Skills have become a portable filesystem standard: a skill is a directory
with a required `SKILL.md` file, YAML frontmatter with at least `name` and
`description`, and optional scripts, references, and assets. The ecosystem
problem is no longer only "where do I download skills from"; it is "how do I
operate the same skills safely across many agents, projects, directories, and
machines."

Loom should not primarily compete as a skill marketplace. Loom should own the
operator layer:

1. Git-backed canonical registry.
2. Explicit target, binding, and projection state.
3. Safe projection into agent directories.
4. Capture, release, rollback, diff, sync, and replay with audit trail.
5. A Panel that is a real control room, not a decorative dashboard.
6. A machine-facing CLI that agents can call without guessing.

## 3. Non-Goals

1. No backward compatibility with phase-1 CLI/API/state contracts.
2. No hidden migration during read commands.
3. No implicit fallback from one projection method to another.
4. No guessing a single Claude, Codex, or other agent directory as execution
   identity.
5. No decorative Panel metrics or synthetic lifecycle events.
6. No marketplace/catalog scope in V1 beyond import from local path or Git URL.

## 4. Core Concepts

### 4.1 Skill

A Skill is a canonical source directory stored under the registry:

```text
skills/<skill_id>/
  SKILL.md
  scripts/
  references/
  assets/
```

Rules:

1. `SKILL.md` is required for a valid portable skill.
2. `name` and `description` frontmatter are required for spec-compliant skills.
3. V1 may import non-compliant legacy skills only with an explicit warning in
   JSON and Panel.
4. `skill_id` is the registry identity. Frontmatter `name` may be shown but must
   not replace `skill_id`.

### 4.2 Target

A Target is one concrete agent skills directory.

Required fields:

```json
{
  "target_id": "target_claude_work",
  "agent": "claude",
  "path": "/Users/me/.claude/skills",
  "ownership": "managed",
  "capabilities": {
    "symlink": true,
    "copy": true,
    "materialize": true,
    "watch": true
  },
  "created_at": "..."
}
```

Supported agents for V1:

1. `claude`
2. `codex`
3. `cursor`
4. `windsurf`
5. `cline`
6. `copilot`
7. `aider`
8. `opencode`
9. `gemini-cli`
10. `goose`

Ownership semantics:

1. `managed`: Loom may create, replace, and remove projection directories.
2. `observed`: Loom may read and import, but must never write.
3. `external`: Loom may reference for inventory only, but must not import or
   write unless the user explicitly changes ownership.

### 4.3 Binding

A Binding maps a workspace matcher to a default target and policy.

Required fields:

```json
{
  "binding_id": "bind_claude_repo",
  "agent": "claude",
  "profile_id": "work",
  "workspace_matcher": {
    "kind": "path-prefix",
    "value": "/Users/me/work"
  },
  "default_target_id": "target_claude_work",
  "policy_profile": "safe-capture",
  "active": true
}
```

Matcher kinds:

1. `path-prefix`
2. `exact-path`
3. `name`

### 4.4 Projection

A Projection is a realized instance of a canonical skill inside a target.

Required fields:

```json
{
  "instance_id": "inst_review_bind_claude_repo_target_claude_work",
  "skill_id": "review",
  "binding_id": "bind_claude_repo",
  "target_id": "target_claude_work",
  "materialized_path": "/Users/me/.claude/skills/review",
  "method": "symlink",
  "last_applied_rev": "abc123",
  "health": "healthy",
  "observed_drift": false,
  "updated_at": "..."
}
```

Projection methods:

1. `symlink`
2. `copy`
3. `materialize`

Rules:

1. Projection requires a binding.
2. Projection may override the target only with an explicit `target_id`.
3. The chosen target must be `managed`.
4. The chosen target must be compatible with the binding's agent.
5. If `symlink` is requested and unavailable, Loom must fail fast and suggest
   `--method copy`; it must not silently downgrade.
6. Replacing an existing projection requires a recoverable backup or typed
   conflict.

### 4.5 Operation

Every successful write creates a registry operation and returns `meta.op_id`.

Noop writes also return a stable result. They may either:

1. return an existing relevant `op_id`; or
2. return `noop: true` and omit `op_id`.

The rule must be explicit per command and covered by tests.

## 5. CLI Contract

### 5.1 Global Rules

1. `--root <path>` is supported by every command.
2. `--json` returns a stable envelope.
3. `--pretty` formats the envelope for humans.
4. Read commands have no control-plane side effects: they must not change Git
   refs, Git index, registry state, live target directories, or pending queue.
   They do append durable command-audit events under `state/events/commands.jsonl`.
5. Write commands acquire locks, perform preflight checks, mutate state, append
   audit, commit state where applicable, and return typed errors on failure.

### 5.2 Envelope

Success:

```json
{
  "ok": true,
  "cmd": "skill.project",
  "request_id": "req_01",
  "version": "1.0.0",
  "data": {},
  "error": null,
  "meta": {
    "op_id": "op_01",
    "warnings": [],
    "sync_state": "LOCAL_ONLY"
  }
}
```

Failure:

```json
{
  "ok": false,
  "cmd": "skill.project",
  "request_id": "req_01",
  "version": "1.0.0",
  "data": {},
  "error": {
    "code": "TARGET_NOT_MANAGED",
    "message": "target 'target_codex_home' has ownership 'observed'",
    "details": {
      "target_id": "target_codex_home",
      "ownership": "observed"
    }
  },
  "meta": {
    "warnings": []
  }
}
```

### 5.3 Error Codes

V1 error codes:

1. `ARG_INVALID`
2. `SCHEMA_MISMATCH`
3. `STATE_CORRUPT`
4. `SKILL_NOT_FOUND`
5. `BINDING_NOT_FOUND`
6. `TARGET_NOT_FOUND`
7. `TARGET_NOT_MANAGED`
8. `TARGET_AGENT_MISMATCH`
9. `PROJECTION_CONFLICT`
10. `PROJECTION_METHOD_UNSUPPORTED`
11. `CAPTURE_CONFLICT`
12. `DEPENDENCY_CONFLICT`
13. `LOCK_BUSY`
14. `REMOTE_UNREACHABLE`
15. `REMOTE_DIVERGED`
16. `PUSH_REJECTED`
17. `REPLAY_CONFLICT`
18. `QUEUE_BLOCKED`
19. `GIT_ERROR`
20. `IO_ERROR`
21. `AUDIT_ERROR`
22. `INTERNAL_ERROR`

## 6. Command Surface

### 6.1 Workspace

```bash
loom workspace init [--scan-existing]
loom workspace status
loom workspace doctor
loom workspace binding add --agent <agent> --profile <id> --matcher-kind <kind> --matcher-value <value> --target <target-id> [--policy-profile <id>]
loom workspace binding list
loom workspace binding show <binding-id>
loom workspace binding remove <binding-id>
loom workspace remote set <git-url>
loom workspace remote status
```

Rules:

1. `workspace status` and `workspace doctor` are read-only.
2. Legacy state detection in read commands returns warnings, not migration.
3. `workspace init --scan-existing` scans all ten supported agents.
4. `workspace doctor` reports Git health, schema health, target existence,
   projection health, operation queue health, and history branch health.

### 6.2 Target

```bash
loom target add --agent <agent> --path <abs-path> [--ownership <managed|observed|external>]
loom target list
loom target show <target-id>
loom target remove <target-id>
```

Rules:

1. `target add` defaults to `observed`.
2. `managed` may create the path.
3. `observed` and `external` require the path to exist.
4. `target remove` fails with `DEPENDENCY_CONFLICT` when active bindings,
   rules, or non-orphaned projections reference the target.

### 6.3 Skill

```bash
loom skill add <path|git-url> --name <skill>
loom skill project <skill> --binding <binding-id> [--target <target-id>] [--method <symlink|copy|materialize>]
loom skill capture [<skill>] [--binding <binding-id>] [--instance <instance-id>] [--message <msg>]
loom skill save <skill> [--message <msg>]
loom skill snapshot <skill>
loom skill release <skill> <version>
loom skill rollback <skill> [--to <ref> | --steps <n>]
loom skill diff <skill> <from> <to>
loom skill import-observed [--target <target-id>]
loom skill monitor-observed [--target <target-id>] [--once] [--interval-seconds <seconds>]
loom skill orphan list
loom skill orphan clean [--delete-live-paths]
```

Rules:

1. `skill add` validates the source and records compliance warnings.
2. `skill project` is managed-target only.
3. `skill capture` refuses ambiguous selectors.
4. `skill capture` detects source drift since `last_applied_rev` and returns
   `CAPTURE_CONFLICT` unless the selected policy explicitly allows it.
5. `skill save`, `snapshot`, `release`, and `rollback` all write operation
   history.
6. `skill rollback` creates `recovery_ref` before changing source.
7. `skill diff` accepts commit SHA, branch, snapshot tag, or release tag.
8. `import-observed` and `monitor-observed` never delete canonical source when
   a live observed skill disappears.
9. `orphan clean` is explicit cleanup after binding removal.

### 6.4 Sync and Ops

```bash
loom sync status
loom sync push
loom sync pull
loom sync replay

loom ops list
loom ops retry
loom ops purge
loom ops history diagnose
loom ops history repair --strategy <local|remote>
```

Rules:

1. `meta.sync_state` is the authoritative field for agent decisions.
2. Push and pull cover main state, tags, pending queue, and `loom-history`.
3. Replay is idempotent.
4. Repair requires explicit strategy.

## 7. Panel Product Spec

The Panel is a control room for the same contract as the CLI. It may have a new
layout and does not need to preserve phase-1 routes or components.

### 7.1 Navigation

V1 Panel pages:

1. Overview
2. Skills
3. Targets
4. Bindings
5. Projections
6. Activity
7. Sync
8. Doctor
9. Settings

### 7.2 Overview

Purpose: show whether the registry is operable.

Must show real data only:

1. registry root
2. Git branch/head/status
3. sync state
4. skill count
5. target count by ownership
6. binding count
7. projection count by method and health
8. pending operation count
9. last operation timestamp
10. write guard state

No decorative KPIs are allowed.

### 7.3 Skills

Skills page must use a union data source:

1. source inventory under `skills/`
2. registry rules
3. registry projections
4. observed imported records

Each skill row must show:

1. source status: `present`, `missing`, `non-compliant`
2. latest rev
3. release/snapshot tags
4. bindings count
5. projections count
6. drift status

Skill detail actions:

1. add
2. save
3. snapshot
4. release
5. rollback
6. diff
7. project
8. capture
9. import observed
10. monitor observed once

### 7.4 Targets

Target page must emphasize concrete identity:

1. target id
2. agent
3. ownership
4. path
5. capabilities
6. related bindings
7. related projections
8. filesystem health

Derived profile labels may be shown only as derived labels.

### 7.5 Bindings

Binding list must show:

1. binding id
2. matcher
3. default target
4. policy profile
5. rule count
6. projection count
7. active state

If a binding has multiple rules, the list must show `multi`; it must not pretend
there is only one skill.

### 7.6 Projections

Projection page must show:

1. instance id
2. skill
3. binding
4. target
5. method
6. live path
7. last applied rev
8. health
9. drift reason
10. available actions

Actions:

1. re-project
2. capture
3. show diff
4. mark orphaned
5. clean orphan

### 7.7 Activity

Activity is the operation history, not a synthetic projection feed.

Must support:

1. pagination
2. filters by status, command, skill, target, binding, request id
3. detail drawer
4. retry failed or pending operation
5. purge completed local queue records

### 7.8 Sync

Sync page must show:

1. remote URL
2. branch
3. ahead/behind
4. sync state
5. pending count
6. history branch status
7. conflict summary

Actions:

1. set/update remote
2. pull
3. push
4. replay
5. diagnose history
6. repair history with local or remote strategy

### 7.9 Doctor

Doctor page must expose `workspace doctor`.

Sections:

1. Registry schema
2. Git repository
3. State files
4. Targets
5. Bindings
6. Projections
7. Operations
8. Sync and history
9. Performance summary

Each failed check must include a concrete next action.

### 7.10 First-Run Flow

When registry state is missing, Panel starts in first-run mode.

Steps:

1. choose registry root
2. initialize registry
3. scan existing agent dirs
4. review observed targets
5. import observed skills
6. create first managed target
7. create first binding
8. project first skill

The user can stop after any step and return later.

## 8. API Spec

The Panel API may break previous routes.

Route families:

```text
GET  /api/v1/health
GET  /api/v1/overview
GET  /api/v1/workspace/status
POST /api/v1/workspace/init
GET  /api/v1/workspace/doctor

GET  /api/v1/skills
POST /api/v1/skills
GET  /api/v1/skills/{skill_id}
POST /api/v1/skills/{skill_id}/save
POST /api/v1/skills/{skill_id}/snapshot
POST /api/v1/skills/{skill_id}/release
POST /api/v1/skills/{skill_id}/rollback
GET  /api/v1/skills/{skill_id}/diff
GET  /api/v1/skills/{skill_id}/history

GET  /api/v1/targets
POST /api/v1/targets
GET  /api/v1/targets/{target_id}
DELETE /api/v1/targets/{target_id}

GET  /api/v1/bindings
POST /api/v1/bindings
GET  /api/v1/bindings/{binding_id}
DELETE /api/v1/bindings/{binding_id}

GET  /api/v1/projections
POST /api/v1/projections
GET  /api/v1/projections/{instance_id}
POST /api/v1/projections/{instance_id}/capture
POST /api/v1/projections/{instance_id}/reproject

GET  /api/v1/ops
POST /api/v1/ops/retry
POST /api/v1/ops/purge

GET  /api/v1/sync/status
POST /api/v1/sync/pull
POST /api/v1/sync/push
POST /api/v1/sync/replay
POST /api/v1/sync/history/repair
```

API rules:

1. Mutations must call core command handlers, not duplicate write logic.
2. Mutations require loopback and Origin/Referer validation.
3. Mutation route coverage must be exhaustive in tests.
4. API responses use the same envelope shape as CLI.
5. GET routes must not write.

## 9. State Migration

Since backward compatibility is not required, V1 may remove implicit legacy
migration.

Rules:

1. If legacy state is present, read commands report `SCHEMA_MISMATCH` with
   migration instructions.
2. V1 provides an explicit migration command only if needed:

```bash
loom migrate inspect
loom migrate apply --from legacy-v3
```

3. Migration must be reviewable before apply.
4. Migration must not write live agent directories.

## 10. Security and Safety

1. No hardcoded credentials.
2. All Git commands use array arguments.
3. URL imports validate Git source syntax.
4. Symlink traversal is rejected during imports and Panel asset serving.
5. Writes refuse `--root` pointing at the Loom tool repository.
6. Observed and external targets are read-only.
7. Panel POST routes are local-only and origin-checked.
8. Destructive UI actions require confirmation and show dependency blockers.

## 11. Verification Gates

### 11.1 Required Commands

Before completion:

```bash
cargo check
npx tsc --noEmit
```

Before submission:

```bash
cargo test
cd panel && bun run test
./scripts/e2e-agent-flow.sh
```

Release gate:

```bash
make ci
cargo build --release --locked
make perf-smoke
```

CI and release workflows must run the same logical gate, including
`panel-typecheck`.

### 11.2 Coverage

1. Critical paths are covered by dedicated Rust and Panel regression suites for
   projection, capture, release, rollback, sync, audit failure, root guard, and
   Panel mutation guard.
2. Every write command has tests for success, noop where applicable, typed
   failure, and audit failure.
3. Line-coverage reporting is a release engineering metric; V1's enforced gate
   is the CI suite plus targeted critical-path regressions.

### 11.3 Performance

Targets:

1. release binary size: <= 3 MB
2. `loom --help` p95: < 300 ms
3. `loom --version` p95: < 300 ms
4. `workspace status` with 1000 skills: p95 < 500 ms
5. `/api/v1/ops?limit=100`: p95 < 150 ms
6. `/api/v1/ops?limit=100` response: < 200 KB
7. Panel production bundle gzip total: < 100 KB unless justified by a measured
   feature need

The enforced `make perf-smoke` gate currently checks items 1, 2, 3, and 7.
Status/API load gates remain benchmark targets for larger fixture runs.

### 11.4 Release

1. `[profile.release]` enables strip, LTO, single codegen unit, and panic abort
   unless measured regressions justify otherwise.
2. Every release artifact is smoke-tested after packaging:
   - `loom --version`
   - `loom --help`
   - `loom --json workspace status`
   - Panel asset readiness
3. `cargo install skillloom` must either produce a working Panel or fail fast
   with clear instructions. A no-panel build must be explicit opt-in.

## 12. Implementation Plan

### P0: Contract and Trust

1. Remove read-command control-plane side effects while preserving durable
   command audit.
2. Add operation records and `meta.op_id` coverage for all writes.
3. Add rollback `recovery_ref`.
4. Add typed projection/capture errors.
5. Add release profile.
6. Add `panel-typecheck` to CI/release.
7. Add `skill.add` Panel mutation guard coverage.
8. Replace Panel skill source with union data source.

### P1: Complete Control Plane

1. Add Panel first-run flow.
2. Add Panel doctor.
3. Add Panel lifecycle actions for save, snapshot, release, rollback.
4. Add Panel observed import and monitor-once.
5. Add orphan list/clean in CLI and Panel.
6. Make diff accept tags and refs in CLI and Panel.
7. Align all 10 agent paths across scan, status, docs, and UI.

### P2: Product Quality

1. Refactor Panel into domain pages and shared mutation/status components.
2. Rename Activity to operation history or back it with operation history.
3. Add command palette.
4. Add coverage thresholds.
5. Split oversized Rust files under U-16.
6. Add release artifact smoke tests.
7. Add performance benchmark jobs.

## 13. Acceptance Criteria

V1 is complete when:

1. A new user can initialize a registry, scan observed targets, import skills,
   create a managed target, bind a workspace, project a skill, capture a live
   edit, release it, rollback it, and sync it without leaving Panel.
2. The same workflow can be run entirely from CLI using `--json`.
3. No read command changes Git refs, Git index, registry state, live target
   directories, or pending queue; command audit events are the only expected
   read-path write.
4. Every registry write is auditable by `op_id`; lifecycle events that do not
   mutate registry state, such as snapshots, are queryable through V1 ops
   activity without fabricating registry `op_id` values.
5. Every visible Panel fact comes from API data.
6. No projection writes to observed or external targets.
7. No projection method silently falls back.
8. CI, tests, coverage, release smoke, and performance gates pass.
9. No new file violates U-16, and existing oversized files have a tracked split
   plan.

## 14. Open Questions

1. Should `noop` writes create operations for complete audit trails, or should
   they return `noop: true` without `op_id`?
2. Should V1 keep any explicit legacy migration command, or should it require a
   fresh registry?
3. Should Panel be embedded-only, or should a standalone static build be
   supported for advanced deployment?
4. Should `policy_profile` become a fixed enum in V1, or remain extensible with
   unknown values rendered as labels?
