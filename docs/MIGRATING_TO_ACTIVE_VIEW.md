# Migrating To An Active View

Use this guide when an older setup projected many or all registry skills into an
agent directory and you want an explicit, reviewable active set.

References:

- Agent Skills specification: <https://agentskills.io/specification>
- Agent Skills client implementation guide: <https://agentskills.io/client-implementation/adding-skills-support>
- Claude Code skills docs: <https://code.claude.com/docs/en/skills>
- Codex visibility guide: [CODEX_SKILL_VISIBILITY.md](CODEX_SKILL_VISIBILITY.md)
- Local Codex active-view plan: [plan/codex-active-view-projection-spec.md](plan/codex-active-view-projection-spec.md)

Existing full-mirror projections are migration input. They are not trusted
desired state.

## 1. Preflight And Backup

Start read-only. Keep the registry root explicit:

```bash
ROOT="$HOME/.loom-registry"
loom --json --root "$ROOT" workspace status
loom --json --root "$ROOT" workspace doctor
loom --json --root "$ROOT" skill list
loom --json --root "$ROOT" target list
```

If the registry has local changes, snapshot or commit them before applying any
migration plan. If a target directory contains manual edits, use `skill capture`
or an out-of-band backup before projection cleanup.

## 2. Read-Only Audit

Collect the current source, target, projection, and Codex config picture:

```bash
loom --json --root "$ROOT" skill active list --agent codex --scope user
loom --json --root "$ROOT" skill visibility fixflow --agent codex --workspace "$PWD"
loom --json --root "$ROOT" codex reconcile --dry-run
```

The audit should classify entries as active, stale Loom projection,
Loom-managed config disable, user-authored config disable, external/manual
entry, system/bundled entry, or unknown/manual-review.

## 3. Build An Explicit Allowlist

Choose the skills that should remain active. The allowlist is operator input,
not something Loom infers from the old full-mirror directory.

Example file:

```text
fixflow
review-pr
release-checklist
```

Do not activate every registry skill just because it exists. Source inventory is
larger than the active view by design.

## 4. Dry-Run Activation

For each allowlisted skill, plan activation first:

```bash
loom --json --root "$ROOT" skill activate fixflow --agent codex --scope user --dry-run
loom --json --root "$ROOT" skill visibility fixflow --agent codex --workspace "$PWD"
```

Review the target path, projection method, binding/profile, config checks, and
restart guidance. Apply only after the plan matches the allowlist:

```bash
loom --json --root "$ROOT" skill activate fixflow --agent codex --scope user
```

## 5. Reconcile Stale Entries

Use reconcile to plan cleanup and config repair:

```bash
loom --json --root "$ROOT" codex reconcile --dry-run
```

Safe cleanup rules:

- Remove or repair only Loom-owned stale symlink projections.
- Preserve non-symlink paths unless the operator reviews them manually.
- Repair Loom-managed config disable entries only when they block an active
  Loom projection.
- Preserve user-authored Codex disables and report them for manual review.
- Never delete system, bundled, or external entries as part of active-view
  migration.

Apply after reviewing the JSON plan:

```bash
loom --json --root "$ROOT" codex reconcile --apply --fix-config
```

## 6. Verify

After activation and reconcile:

```bash
loom --json --root "$ROOT" workspace doctor
loom --json --root "$ROOT" skill active list --agent codex --scope user
loom --json --root "$ROOT" skill diagnose fixflow --agent codex
loom --json --root "$ROOT" skill visibility fixflow --agent codex --workspace "$PWD"
```

Expected result:

- allowlisted skills are active, visible, and enabled;
- stale full-mirror projections are removed or explicitly reported;
- user-authored disables are preserved or manually resolved;
- restart-required/new-session guidance is clear.

Start a new Codex session when visibility output says the runtime must reload.

## 7. Recovery

If a migration step produces the wrong active set:

```bash
loom --json --root "$ROOT" skill deactivate fixflow --agent codex --scope user --dry-run
loom --json --root "$ROOT" skill rollback fixflow --to <ref> --dry-run
loom --json --root "$ROOT" ops list
```

Use `deactivate` for active-view state, `rollback` for source history, and
`ops retry` only for queued operations whose original plan is still valid.

When in doubt, stop after the dry-run output and review the target files and
Codex config manually. The migration goal is a small, explicit active view, not
bulk deletion.
