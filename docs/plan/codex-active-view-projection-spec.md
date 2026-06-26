# Codex Active View Projection Spec

Date: 2026-06-26
Status: Draft
Scope: Make Loom-managed Codex skills visible through an explicit active view instead of projecting the full registry into `~/.codex/skills`.

## Product Thesis

Loom is the Git-backed registry and projection control plane for agent skills. For
Codex, that means Loom must manage not only the canonical source under
`~/.loom-registry/skills`, but also which skills are visible to the current Codex
runtime.

The current full-mirror pattern is wrong for Codex:

1. `~/.loom-registry/skills` contains the canonical inventory.
2. `~/.codex/skills` currently mirrors most or all registry skills.
3. `~/.codex/config.toml` may then disable registry skill paths to reduce prompt
   noise.

That makes the filesystem projection say "active" while Codex configuration says
"disabled". Operators then see a symlinked skill that still does not appear in
Codex. Loom should remove that ambiguity.

Decision: for Codex, `~/.codex/skills` is an active view, not a registry mirror.
Only active skills should be projected there.

## Goals

1. Keep `~/.loom-registry/skills` as the complete canonical inventory.
2. Make `~/.codex/skills` contain only the active Codex skill set plus
   Codex-owned runtime directories.
3. Use existing registry rules and projections as the source of desired active
   state.
4. Add explicit activate, deactivate, list, and reconcile workflows.
5. Detect and repair Codex config entries that disable active Loom projections.
6. Make diagnostics explain the full chain: source, active rule, projection,
   symlink target, Codex config visibility, and restart requirement.
7. Avoid bulk `enabled=false` config generation as the normal noise-control
   mechanism.

## Non-Goals

1. No marketplace or catalog scope.
2. No automatic activation of all registry skills.
3. No hidden edits to user-authored `~/.codex/config.toml` entries.
4. No background daemon or scheduled automation in the MVP.
5. No best-effort deletion of non-symlink projections in the first release.
6. No change to Claude, Cursor, Windsurf, or other agent projection behavior
   unless a later spec extends active views to them.
7. No weakening of existing target ownership rules. Loom may write only to
   managed targets.

## Current Baseline

Relevant existing model:

- `RegistryProjectionTarget` represents an agent skills directory.
- `RegistryWorkspaceBinding` maps a workspace/profile matcher to a target.
- `RegistryBindingRule` records that a skill should be projected for a binding
  and target.
- `RegistryProjectionInstance` records the realized target path.
- `loom skill project` writes a projection into a managed target and upserts the
  corresponding rule and projection.
- `loom skill diagnose` already joins source, bindings, targets, projections,
  recent operations, and pending operations.

Relevant historical state:

- The June 16 unification projected normal registry skills into both Claude and
  Codex endpoints.
- That made the Codex endpoint large enough to trigger context budget pressure.
- A later local Codex config cleanup disabled many
  `/Users/lifcc/.loom-registry/skills/*/SKILL.md` paths.
- Because Codex follows symlink targets when scanning skill folders, a symlink in
  `~/.codex/skills/<skill>` can still be disabled by a config entry for the
  canonical `~/.loom-registry/skills/<skill>/SKILL.md` path.

## Core Decision

For Codex targets, `RegistryBindingRule` is the desired active set.

```text
rule exists for codex binding + target + skill
=> skill should be present in ~/.codex/skills/<skill>
=> canonical SKILL.md must not be disabled by Loom-managed Codex config

rule absent
=> skill remains in ~/.loom-registry/skills
=> skill should not be present in ~/.codex/skills as a Loom-owned projection
```

`RegistryProjectionInstance` remains the actual state record. Reconcile compares
rules, projections, filesystem entries, and Codex config.

## Terminology

### Inventory

The full canonical source tree:

```text
~/.loom-registry/skills/<skill>/
```

Inventory skills may be active, inactive, broken, trashed, or never projected.

### Active View

The small set of skills visible to one Codex runtime endpoint:

```text
~/.codex/skills/<skill> -> ~/.loom-registry/skills/<skill>
```

The active view is a runtime projection, not a backup and not the canonical
source.

### Runtime Directory

A directory under `~/.codex/skills` that Codex owns and Loom must preserve, such
as:

```text
.system
codex-primary-runtime
```

Runtime directories are not Loom projections.

### External Entry

An entry under `~/.codex/skills` that is not owned by Loom. Reconcile must report
it and leave it alone unless the user explicitly imports or adopts it.

## State Model

### No New MVP State File

MVP should not add `active_skills.json`.

The desired active set is already expressible as registry rules:

```json
{
  "binding_id": "bind_codex_default",
  "skill_id": "threads",
  "target_id": "target_codex_default",
  "method": "symlink",
  "watch_policy": "observe_only",
  "created_at": "..."
}
```

This keeps one source of truth and avoids a second active-set schema drifting
from rules and projections.

### Target Requirements

Codex active view commands require a managed Codex target:

```json
{
  "target_id": "target_codex_default",
  "agent": "codex",
  "path": "/Users/lifcc/.codex/skills",
  "ownership": "managed",
  "capabilities": {
    "symlink": true,
    "copy": true,
    "watch": true
  }
}
```

If the existing `~/.codex/skills` target is `observed`, activation must fail with
a typed error and explain how to register a managed target. It must not silently
write to observed targets.

### Binding Requirements

MVP uses a binding to scope the active set:

```json
{
  "binding_id": "bind_codex_default",
  "agent": "codex",
  "profile_id": "default",
  "workspace_matcher": {
    "kind": "path-prefix",
    "value": "/"
  },
  "default_target_id": "target_codex_default",
  "policy_profile": "active-view",
  "active": true
}
```

Implementation may create this binding explicitly via existing
`workspace binding add`, or provide a convenience setup command later. The MVP
must not assume a global default binding unless one exists.

### Projection Requirements

MVP supports `symlink` for Codex active view writes.

`copy` and `materialize` are allowed in the broader Loom projection model, but
deactivation is risky for non-symlink paths because live edits may exist inside
the target directory. MVP behavior:

- `activate` defaults to `symlink`.
- `deactivate` removes only Loom-owned symlink projections.
- `deactivate` fails closed for `copy` or `materialize` projections unless a
  future command adds capture/backup semantics.

## CLI Contract

### Activate

```bash
loom skill activate <skill> --agent codex --binding <binding-id> [--method symlink] [--dry-run]
```

Behavior:

1. Validate skill name.
2. Require source directory and `SKILL.md`.
3. Load registry snapshot.
4. Resolve binding.
5. Require binding agent `codex`.
6. Resolve target from binding or explicit future target flag.
7. Require target ownership `managed`.
8. Require target capability for requested method.
9. For `symlink`, probe target filesystem support before mutation.
10. Add or update the active rule.
11. Create or replace the Loom-owned projection at `<target.path>/<skill>`.
12. Upsert projection instance with `health = "healthy"`.
13. Record operation `skill.activate`.
14. Commit registry state.
15. Return restart advice when Codex config changed or visibility cannot be
    reloaded in the current session.

Suggested JSON:

```json
{
  "skill": "threads",
  "agent": "codex",
  "binding_id": "bind_codex_default",
  "target_id": "target_codex_default",
  "active": true,
  "projection": {
    "instance_id": "inst_threads_bind_codex_default_target_codex_default",
    "materialized_path": "/Users/lifcc/.codex/skills/threads",
    "method": "symlink",
    "health": "healthy"
  },
  "config_visibility": {
    "status": "ok",
    "disabled_by_config": false,
    "restart_required": false
  },
  "commit": "..."
}
```

No-op behavior:

- If the rule exists, projection exists, symlink points to the canonical source,
  and config visibility is OK, return `noop: true`.
- If the rule exists but projection or config visibility is wrong, repair the
  inconsistent part unless `--dry-run`.

### Deactivate

```bash
loom skill deactivate <skill> --agent codex --binding <binding-id> [--dry-run]
```

Behavior:

1. Validate binding and target as Codex managed target.
2. Find matching rule and projection.
3. Verify live path is Loom-owned before deletion.
4. Remove active rule.
5. Remove symlink projection from `~/.codex/skills`.
6. Remove projection instance, or mark it `orphaned` only if preserving audit
   history is required by existing projection cleanup semantics.
7. Record operation `skill.deactivate`.
8. Commit registry state.
9. Leave source under `~/.loom-registry/skills/<skill>` untouched.

Deletion safety:

- If live path does not exist, still remove rule and stale projection record.
- If live path is a symlink to the canonical source, remove it.
- If live path is a symlink elsewhere, fail with `PROJECTION_CONFLICT`.
- If live path is a real directory, fail with `PROJECTION_CONFLICT` and suggest
  `loom skill capture` or manual backup before cleanup.

### Active List

```bash
loom skill active list --agent codex [--binding <binding-id>] [--json]
```

Behavior:

1. Read registry state.
2. Return desired active rules for Codex.
3. Join projection and filesystem status when available.
4. Do not mutate.

### Reconcile

```bash
loom codex reconcile --binding <binding-id> --dry-run
loom codex reconcile --binding <binding-id> --apply
loom codex reconcile --binding <binding-id> --apply --fix-config
```

Reconcile computes:

```text
desired = registry rules for binding + codex target
recorded = projection instances for binding + codex target
actual = filesystem entries under target.path
config = ~/.codex/config.toml skills.config visibility
```

Plan categories:

1. `create_projection`: desired rule exists, path missing.
2. `repair_projection`: desired rule exists, path exists but points elsewhere.
3. `remove_stale_projection`: Loom-owned projection exists without desired rule.
4. `remove_stale_record`: projection record exists but path is missing and rule
   is absent.
5. `preserve_runtime_entry`: Codex-owned runtime entry exists.
6. `preserve_external_entry`: non-Loom entry exists.
7. `fix_config_disable`: active canonical `SKILL.md` is disabled by Codex
   config.
8. `manual_review`: path conflict, non-symlink projection, unreadable path, or
   user-authored config conflict.

Dry-run JSON:

```json
{
  "agent": "codex",
  "binding_id": "bind_codex_default",
  "target_id": "target_codex_default",
  "target_path": "/Users/lifcc/.codex/skills",
  "summary": {
    "desired": 4,
    "actual_entries": 10,
    "create": 1,
    "remove": 2,
    "fix_config": 1,
    "manual_review": 0
  },
  "actions": [
    {
      "kind": "fix_config_disable",
      "skill": "threads",
      "path": "/Users/lifcc/.loom-registry/skills/threads/SKILL.md",
      "safe_to_apply": true,
      "reason": "active skill disabled by Loom-generated Codex config block"
    }
  ],
  "restart_required": true
}
```

Apply behavior:

- Apply only actions with `safe_to_apply == true`.
- If any `manual_review` action exists, return non-zero unless
  `--allow-partial` is introduced later.
- Record operation `codex.reconcile`.
- Commit registry state when registry files change.
- Do not commit user home config changes to the registry Git repo.

## Codex Config Visibility

### Detection

Loom must detect whether an active canonical skill path is disabled by:

```toml
[[skills.config]]
path = "/Users/lifcc/.loom-registry/skills/threads/SKILL.md"
enabled = false
```

For each active skill:

1. Resolve the projection target path.
2. Resolve symlink target to canonical source.
3. Build canonical `SKILL.md` path.
4. Parse `~/.codex/config.toml`.
5. Find matching `[[skills.config]]` entries by exact path and, where possible,
   canonicalized path.

### Repair Policy

Default `reconcile --apply` must not edit config.

`--fix-config` may edit config only when the disabled entry is safe:

1. The path belongs to the Loom registry root.
2. The path belongs to an active Codex rule.
3. The entry is inside a Loom-generated block, or the entry exactly matches the
   active skill and changing it is the requested operation.

Safe repairs:

- Change `enabled = false` to `enabled = true` for active skills.
- Or remove the matching generated disable entry.

Unsafe repairs:

- Editing unrelated skills.
- Editing non-Loom skill paths.
- Removing a user-authored disable entry without an explicit targeted command.

Config edits must be atomic: write to a temp file, validate TOML shape if a
parser is available, then rename.

### Restart Semantics

After `~/.codex/config.toml` changes, output must say:

```text
restart_required: true
reason: Codex loads skills at session startup; current sessions may not reload config changes.
```

Do not claim the current Codex session can see the skill unless verified by the
current runtime.

## Diagnosis Additions

Extend `loom skill diagnose <skill>` with a Codex visibility section when the
skill has Codex rules, projections, or target entries.

New checks:

1. `codex_active_rule_exists:<binding_id>`
   - Info when active; warning when source exists but no Codex active rule.
2. `codex_projection_path_exists:<instance_id>`
   - Error when active rule exists but path is missing.
3. `codex_projection_is_symlink:<instance_id>`
   - Error when active view projection is not a symlink in MVP.
4. `codex_projection_points_to_source:<instance_id>`
   - Error when symlink target does not resolve to canonical source.
5. `codex_config_not_disabled:<skill>`
   - Error when active skill is disabled by `skills.config`.
6. `codex_runtime_entry_classification:<path>`
   - Info for runtime or external entries related to the skill name.
7. `codex_restart_required`
   - Warning when config was changed by the last operation and a restart has not
     been observed or acknowledged.

Example failed check:

```json
{
  "section": "codex_visibility",
  "id": "codex_config_not_disabled:threads",
  "ok": false,
  "severity": "error",
  "message": "active Codex skill is disabled by Codex config",
  "next_action": "run loom codex reconcile --binding bind_codex_default --apply --fix-config, then restart Codex",
  "details": {
    "skill": "threads",
    "config_path": "/Users/lifcc/.codex/config.toml",
    "disabled_path": "/Users/lifcc/.loom-registry/skills/threads/SKILL.md"
  }
}
```

## Panel UX

Panel should expose active view semantics only after CLI behavior is stable.

Minimum UI:

1. Skill detail page shows Codex active status:
   - inactive
   - active and healthy
   - active but missing projection
   - active but disabled by config
   - active but restart required
2. Skill row includes `active_targets` count separate from inventory count.
3. Codex target detail page shows:
   - desired active skills
   - actual filesystem entries
   - preserved runtime entries
   - external entries
   - stale Loom projections
4. Reconcile preview appears before apply.
5. No card or label should imply inactive registry skills are broken merely
   because they are absent from `~/.codex/skills`.

No Panel mutation should bypass the CLI safety rules.

## Migration Plan

### Phase 0: Read-Only Audit

Add `loom codex reconcile --dry-run`.

It must report:

1. Active desired rules, if any.
2. All `~/.codex/skills` entries.
3. Entries symlinked to `~/.loom-registry/skills`.
4. Entries disabled by Codex config.
5. Which entries are safe to remove from the Codex active view.
6. Which entries require manual review.

No mutation in this phase.

### Phase 1: Seed Active Allowlist

Create active rules for a small operator-approved allowlist.

Example allowlist:

```text
threads
vibeguard
fixflow
plan-flow
agent-workflow
```

The allowlist must be explicit input. Loom must not infer it from all existing
symlinks because the existing endpoint is polluted by the full mirror.

Possible command:

```bash
loom skill activate threads --agent codex --binding bind_codex_default
```

Batch activation may be added later:

```bash
loom skill activate --from-file codex-active-skills.txt --agent codex --binding bind_codex_default
```

### Phase 2: Remove Inactive Loom Projections

Run:

```bash
loom codex reconcile --binding bind_codex_default --apply
```

This removes only safe stale projections:

- path is under Codex target
- path is a symlink
- symlink resolves under Loom registry root
- skill is not in desired active rules

It must preserve:

- `.system`
- `codex-primary-runtime`
- non-Loom entries
- directories with uncommitted local edits or non-symlink materialization

### Phase 3: Fix Active Config Disables

Run:

```bash
loom codex reconcile --binding bind_codex_default --apply --fix-config
```

This repairs active skills disabled by Codex config and reports
`restart_required: true`.

### Phase 4: Diagnose and Verify

Run:

```bash
loom skill active list --agent codex --binding bind_codex_default
loom skill diagnose threads
loom codex reconcile --binding bind_codex_default --dry-run
```

Expected state:

- active allowlist projected
- inactive registry skills absent from `~/.codex/skills`
- no active skill disabled by config
- runtime directories preserved
- no manual review actions

## Failure Behavior

All failures are fail-closed.

1. If registry state is unavailable, activation/deactivation fails.
2. If target is observed or external, writes fail.
3. If filesystem path is unreadable, reconcile marks manual review.
4. If a stale projection is not a symlink, reconcile refuses deletion.
5. If symlink target is outside Loom registry root, reconcile refuses deletion.
6. If config parse fails, `--fix-config` refuses to write.
7. If operation recording fails after filesystem mutation, rollback filesystem and
   registry state where possible and report rollback errors.
8. If the same repair fails repeatedly, stop and surface the conflict instead of
   retrying silently.

## Security and Data Safety

1. Do not follow user-controlled symlinks for deletion unless the symlink itself
   is the object being removed.
2. Never recursively delete a real directory in MVP deactivation.
3. Use array arguments for shell commands in tests and scripts.
4. Do not store secrets in registry state or operation payloads.
5. Redact home-local config snippets in logs unless the path itself is required
   for diagnosis.
6. Config edits must preserve unrelated TOML content.
7. No force push or remote sync side effect from activation unless the existing
   autosync policy already queues it explicitly.

## Implementation Plan

### Step 1: Read-Only Reconcile Engine

Likely files:

- `src/cli.rs`: add `codex reconcile` command shape.
- `src/commands/codex_cmds.rs`: new command module.
- `src/commands/projections.rs`: reuse projection helpers where possible.
- `src/state_model/mod.rs`: no schema change expected.

Build a pure planner:

```rust
fn plan_codex_reconcile(snapshot: &RegistrySnapshot, target: &RegistryProjectionTarget, binding_id: &str, codex_config: Option<&CodexConfigView>) -> ReconcilePlan
```

The planner should be unit-testable without touching the real home directory.

### Step 2: Activate and Deactivate Commands

Likely files:

- `src/cli.rs`
- `src/commands/skill_cmds.rs` or new `src/commands/skill_active_cmds.rs`
- `src/commands/projections.rs`

Prefer extracting shared projection mutation helpers from `cmd_project` instead
of duplicating remove/project/rollback logic.

### Step 3: Config Parser and Safe Editor

Likely files:

- `src/codex_config.rs` or `src/commands/codex_config.rs`
- tests using temp config files

MVP parser can be structured TOML if the project already has a TOML crate.
Otherwise, add one deliberately and keep string editing out of the command path.

### Step 4: Apply Reconcile

Apply only safe planned actions.

Do not mix planning and mutation logic. The apply step consumes a plan generated
from current state and should revalidate each path before mutation.

### Step 5: Diagnose Integration

Extend `skill_diagnose` to call the Codex visibility planner for related Codex
targets.

Keep diagnosis read-only.

### Step 6: Panel Follow-Up

After CLI tests pass, expose active status and reconcile preview in Panel.

## Test Plan

### Unit Tests

1. Planner marks desired rule + missing path as `create_projection`.
2. Planner marks inactive Loom symlink as `remove_stale_projection`.
3. Planner preserves `.system`.
4. Planner preserves `codex-primary-runtime`.
5. Planner preserves non-Loom symlink.
6. Planner refuses real directory deletion.
7. Planner detects active skill disabled by config.
8. Planner does not request config edits without `--fix-config`.
9. Config editor changes only the targeted active skill entry.
10. Config editor refuses malformed TOML.

### Integration Tests

Use temp directories for registry root, Codex home, and target path.

1. `skill activate` creates rule, projection record, and symlink.
2. `skill activate` is idempotent.
3. `skill deactivate` removes symlink and active rule while preserving source.
4. `codex reconcile --dry-run` makes no filesystem or registry changes.
5. `codex reconcile --apply` removes only inactive Loom-owned symlinks.
6. `codex reconcile --apply --fix-config` repairs active config disables and
   reports restart required.
7. `skill diagnose` reports active-but-disabled config as an error.
8. Observed target activation fails with a typed error.
9. Non-symlink stale projection fails closed.
10. Existing unrelated dirty worktree files do not affect read-only reconcile.

### Manual Verification

On a real Codex setup:

```bash
loom codex reconcile --binding bind_codex_default --dry-run
loom skill activate threads --agent codex --binding bind_codex_default
loom codex reconcile --binding bind_codex_default --apply --fix-config
loom skill active list --agent codex --binding bind_codex_default
loom skill diagnose threads
```

Then restart Codex and verify `threads` appears in the available skills list.

## Acceptance Criteria

1. Inactive registry skills are absent from `~/.codex/skills`.
2. Active Codex skills are present as Loom-owned symlinks.
3. Active Codex skills are not disabled by Loom-managed Codex config.
4. Runtime directories are preserved.
5. External entries are preserved and reported.
6. `reconcile --dry-run` is fully read-only.
7. `reconcile --apply` refuses unsafe deletes.
8. `deactivate` never removes canonical registry source.
9. `skill diagnose` explains why an active skill is not visible to Codex.
10. Fresh `cargo check` and relevant Rust tests pass.

## Open Questions

1. Should `profile_id` map to Codex config profiles or remain Loom-only?
2. Should Loom add a convenience `loom codex setup-active-view` command that
   creates the managed target and default binding?
3. Should active set selection be workspace-scoped for Codex, or is one global
   Codex active view sufficient for MVP?
4. Should `deactivate` remove projection records or mark them orphaned for
   stronger audit continuity?
5. Should batch activation read from a plain text allowlist or a registry-managed
   named profile?
6. How should Loom detect that Codex has been restarted after a config repair?
7. Should the same active view model later apply to Claude when skill counts
   become large there?

## Rollout Strategy

1. Ship read-only `codex reconcile --dry-run`.
2. Validate on the current local polluted endpoint and record the action plan.
3. Ship `skill activate`, `skill deactivate`, and `skill active list`.
4. Migrate one local Codex endpoint using an explicit allowlist.
5. Add `--apply` after dry-run output matches expected filesystem changes.
6. Add `--fix-config` only after config parser tests exist.
7. Add Panel UI after CLI behavior is stable.

Do not remove the old full mirror in a migration command until dry-run identifies
every entry as active, runtime, external, or safe stale Loom projection.
