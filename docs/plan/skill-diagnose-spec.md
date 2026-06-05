# Skill Diagnose Spec

Date: 2026-06-04
Status: Draft
Scope: Add a read-only, per-skill diagnosis workflow across CLI, Panel API, and the Skills detail UI.
Tracking issue: https://github.com/majiayu000/loom/issues/227

## Product Thesis

Loom already has global workspace health (`workspace doctor`) and narrow source drift verification (`skill verify`). Operators still need a skill-scoped explanation that answers:

1. Is this skill source valid?
2. Which bindings and targets can use it?
3. Are its projections healthy, drifted, missing, conflicted, or orphaned?
4. Why would `project`, `capture`, `save`, or `sync` fail for this skill?
5. What exact next action should the operator take?

`skill diagnose` should be the per-skill control-plane truth. It must not repair, initialize, project, capture, save, or silently fabricate unavailable data.

## Current Baseline

Existing pieces to reuse:

- `loom skill verify <skill>` checks whether `skills/<skill>` has uncommitted drift relative to the last source commit.
- `loom workspace doctor` checks Git health, schema availability, pending queue warnings, history conflicts, target existence, binding-target consistency, projection paths, and source existence.
- `GET /api/v1/workspace/doctor` exposes global doctor checks to the Panel.
- `GET /api/v1/skills` provides per-skill summaries including source status, tags, binding counts, projection counts, target ids, and observed import state.
- Skills detail UI already has `Lifecycle`, `Diff`, and `Targets` tabs.

Gap:

- There is no single read-only command or API endpoint that joins source, Git drift, bindings, targets, projections, recent operations, and concrete remediation for one skill.

## Non-Goals

1. No repair command in the MVP.
2. No automatic projection, capture, save, import, or orphan cleanup.
3. No agent runtime activation tests or marketplace/catalog validation.
4. No synthetic Panel data, fake lifecycle events, or inferred health when registry state is unavailable.
5. No new persistence schema for diagnosis results.
6. No hidden fallback from one projection method to another.
7. No mutation audit operation for diagnosis. CLI invocation may follow the existing read-command audit policy, but the Panel diagnose endpoint should not append command-audit events on every tab refresh.

## Command Contract

Add:

```bash
loom skill diagnose <skill>
loom --json skill diagnose <skill>
```

Properties:

- Read-only.
- Skill-scoped.
- Stable JSON envelope under `--json`.
- Human output can remain the current envelope pretty-printing style for MVP.
- Exit code should follow the existing CLI envelope pattern: command success when diagnosis ran successfully, even when `data.healthy == false`.
- Missing skill source should be a successful diagnosis when the skill is referenced by rules, projections, tags, or operation history; it should return a failed check instead of `SKILL_NOT_FOUND`.
- A completely unknown skill should return `SKILL_NOT_FOUND`.

Suggested command summary:

```json
{
  "skill": "model-onboarding",
  "healthy": false,
  "status": "blocked",
  "summary": {
    "source_status": "present",
    "binding_count": 2,
    "target_count": 2,
    "projection_count": 2,
    "failed_check_count": 1,
    "warning_check_count": 1,
    "drifted_path_count": 1,
    "recent_failed_op_count": 0
  },
  "checks": [],
  "related": {}
}
```

`status` values:

- `healthy`: all error and warning checks pass.
- `attention`: one or more warning checks fail, but no error checks fail.
- `blocked`: one or more error checks fail.

`healthy` is `status == "healthy"`.

## Check Row Contract

Reuse the Panel doctor row shape:

```json
{
  "section": "projection",
  "id": "projection_path_exists:inst_model_onboarding_claude",
  "ok": false,
  "severity": "error",
  "message": "projection path is missing",
  "next_action": "rerun loom skill project model-onboarding --binding bind_claude_repo",
  "details": {
    "instance_id": "inst_model_onboarding_claude",
    "target_id": "target_claude_default",
    "path": "/Users/me/.claude/skills/model-onboarding"
  }
}
```

Rules:

1. `section`, `id`, `ok`, `severity`, `message`, `next_action`, and `details` are always present.
2. `severity` is one of `ok`, `info`, `warning`, or `error`.
3. `next_action` is `null` when `ok == true`.
4. `details` is a JSON object, never omitted.
5. Check IDs must be stable and deterministic for the same registry state.

## Required Checks

### Source

1. `source_directory_exists`
   - Error when `skills/<skill>` does not exist and the skill is referenced by registry state.
   - Next action: restore from trash/history, re-add the skill, or clean orphaned references depending on related data.
2. `skill_file_exists`
   - Error when neither `SKILL.md` nor `skill.md` exists.
   - Details include which entrypoint filename was found.
   - Next action: add `skills/<skill>/SKILL.md` or remove the non-compliant source.
3. `skill_frontmatter_description`
   - Warning when description cannot be read from the detected entrypoint.
   - Next action: add `description:` frontmatter.

### Git Source Drift

1. `source_tracked_at_head`
   - Error when the skill has no tracked tree at `HEAD` and source exists.
   - Next action: run `loom skill save <skill>`.
2. `source_drift`
   - Warning when tracked, staged, or untracked paths under `skills/<skill>` differ from the last source commit.
   - Next action: run `loom skill save <skill>` or inspect `loom skill diff`.
   - Details include `head_tree_oid`, `last_source_commit`, and `drifted_paths`.

### Registry Relations

1. `binding_target_exists:<binding_id>`
   - Error when a binding/rule for this skill points to a missing target.
2. `binding_target_agent_match:<binding_id>`
   - Error when binding agent and target agent differ.
3. `rule_target_exists:<binding_id>:<target_id>`
   - Error when a rule references a missing target.
4. `binding_active:<binding_id>`
   - Warning when a related binding is inactive.

### Targets

1. `target_path_exists:<target_id>`
   - Error when a related target path is missing.
2. `target_ownership_writeable:<target_id>`
   - Warning when a rule/projection expects write behavior but target ownership is not `managed`.
3. `target_capability:<target_id>:<method>`
   - Error when a related rule/projection method is unsupported by target capabilities.
   - Current schema has `symlink`, `copy`, and `watch`; `materialize` should be diagnosed against `copy` capability unless a future schema adds an explicit `materialize` capability.

### Projections

1. `projection_path_exists:<instance_id>`
   - Error when materialized path is missing.
2. `projection_source_exists:<instance_id>`
   - Error when source skill is missing.
3. `projection_health:<instance_id>`
   - Error for `missing`, `conflict`, or other non-healthy health values.
   - Warning for `drifted` and `orphaned`.
4. `projection_observed_drift:<instance_id>`
   - Warning when `observed_drift == true`.
5. `projection_symlink_target:<instance_id>`
   - Error when a symlink projection is not a symlink, is dangling, or points away from the canonical source.
   - Relative symlink targets must resolve against the symlink parent directory, not process CWD.
6. `projection_binding_exists:<instance_id>`
   - Warning when `binding_id == null` and health is `orphaned`.
   - Error when `binding_id` is set but no matching binding exists.

### Recent Operations

1. `recent_failed_ops`
   - Warning when recent operations for the skill have `last_error`.
   - Details include bounded newest-first records without large payload/effects.
2. `recent_pending_ops`
   - Warning when pending queue contains skill-related operations.

## Related Data Contract

`related` should include bounded, normalized arrays:

```json
{
  "source": {
    "path": "/Users/me/.loom-registry/skills/model-onboarding",
    "skill_file": "/Users/me/.loom-registry/skills/model-onboarding/SKILL.md",
    "description": "..."
  },
  "bindings": [],
  "rules": [],
  "targets": [],
  "projections": [],
  "recent_operations": [],
  "pending_operations": []
}
```

Bounds:

- Recent operations: newest 10 matching records.
- Pending operations: newest 10 matching records.
- Drifted paths: first 100 paths, plus `drifted_paths_truncated: true` when clipped.

Do not include full operation payloads when they are large. Surface only identifiers, intent, status, timestamps, and last error.

## Panel API Contract

Add:

```text
GET /api/v1/skills/{skill_id}/diagnose
```

Response:

- CLI-style envelope via a read-only wrapper or direct command call.
- `data` equals the `skill diagnose` payload.
- Non-2xx only when the command cannot run or the skill is completely unknown.
- The endpoint should avoid durable command-audit writes because the UI fetches diagnosis interactively.

Back-compat route:

- No legacy `/api/registry/...` route is required for MVP.

## Panel UX

Extend the existing Skills detail panel:

1. Add a `Diagnose` tab beside `Lifecycle`, `Diff`, and `Targets`.
2. Fetch diagnosis only when the tab is active.
3. Abort in-flight fetch on skill/tab change.
4. Render a compact verdict row:
   - `healthy`
   - `attention`
   - `blocked`
   - failed/warning counts
5. Group checks by `section`.
6. Show each failed check message and next action.
7. Show related projections/targets as compact identifiers, not large cards.
8. Offline/read-only mode:
   - diagnosis fetch is unavailable when live API is offline.
   - existing stale skill summaries can remain visible, but the Diagnose tab must state that live diagnosis requires the API.

No repair buttons in MVP. Repair actions can be follow-up once diagnosis has been manually validated.

## Issue and PR Plan

### Issue 1: Add per-skill diagnose contract and docs

Deliverables:

- This spec.
- GitHub issue that links the spec and defines acceptance.

### PR 1: CLI and API

Deliverables:

- `SkillCommand::Diagnose(SkillOnlyArgs)`.
- `src/commands/skill_diagnose.rs`.
- Reuse or share helper logic from `skill_verify` and `workspace_cmds/doctor.rs` where practical.
- `GET /api/v1/skills/{skill_id}/diagnose`.
- Rust tests for:
  - healthy skill with source, binding, target, projection.
  - skill with source drift.
  - missing `SKILL.md`.
  - missing projection path.
  - relative symlink resolution.
  - skill referenced by projection but missing source.
  - completely unknown skill returns `SKILL_NOT_FOUND`.

### PR 2: Panel Diagnose tab

Deliverables:

- TypeScript client method and payload types.
- Skills detail `Diagnose` tab.
- Grouped check rendering with existing chip/table language.
- Tests for:
  - fetch only on Diagnose tab.
  - blocked/attention/healthy verdict rendering.
  - failed check next action rendering.
  - live API offline message.
  - abort/refetch on skill change.

### PR 3: Optional polish after manual validation

Only after PR 1 and PR 2 are manually validated:

- Add quick links from failed projection checks to the Projections page.
- Add a top-level Doctor link filtered by skill if a filter route exists.
- Add repair affordances only if each repair action maps to an existing explicit CLI mutation and read-only gating is preserved.

## Acceptance Criteria

1. `loom --json skill diagnose <skill>` returns stable JSON with `healthy`, `status`, `summary`, `checks`, and `related`.
2. The CLI command has no control-plane side effects beyond existing read-command audit behavior.
3. All failed checks include concrete `next_action`.
4. Projection symlink checks correctly resolve relative symlink targets from the symlink parent.
5. Panel Skills detail can diagnose a selected skill without fabricating data.
6. Offline Panel state never renders fake diagnosis.
7. Backend and frontend tests cover healthy, warning, and blocked states.
8. Follow-up repair UI is explicitly deferred.

## Verification

PR 1:

```bash
cargo check
cargo test skill_diagnose
cargo test
```

PR 2:

```bash
cd panel && bun run typecheck
cd panel && bun run test -- SkillsPage
cd panel && bun run test
```

If a full command is too slow or blocked by environment, the PR must report the exact command, failure, and narrower command that passed.
