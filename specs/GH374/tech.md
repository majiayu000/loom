# GH374 Tech Spec: Single-Skill Lifecycle Docs

Issue: https://github.com/majiayu000/loom/issues/374
Product spec: `specs/GH374/product.md`
Status: Draft for implementation

## Current State

The README currently introduces Loom as a registry/projection control plane and
then shows install, `loom init`, monitor, `loom use`, target registration,
workspace binding, and projection examples. That is accurate for advanced
operators but too low-level for the single-skill lifecycle epic.

Existing docs with relevant material:

- `README.md`: current quick start and lifecycle verb table.
- `docs/LOOM_CLI_CONTRACT.md`: command contract and migration policy.
- `docs/LOOM_STATE_MIGRATION_NOTES.md`: older registry migration notes.
- `docs/plan/codex-active-view-projection-spec.md`: active-view terminology,
  Codex config visibility, and migration plan.
- `docs/plan/advanced-skill-ecosystem-todolist.md`: dependency map showing
  single-skill primitives before advanced ecosystem blocks.

## Documentation Structure

Add three new docs:

```text
docs/SINGLE_SKILL_LIFECYCLE.md
docs/CODEX_SKILL_VISIBILITY.md
docs/MIGRATING_TO_ACTIVE_VIEW.md
```

Update:

```text
README.md
docs/LOOM_COMPLETE_GUIDE_ZH.md
docs/LOOM_STATE_MIGRATION_NOTES.md
docs/plan/codex-active-view-projection-spec.md
```

Only update the last three files when they contain text that conflicts with the
new active-view story. Prefer small corrections and cross-links over broad
rewrites.

## README Changes

Add a single-skill quick start before the managed projection flow:

```bash
loom init
loom skill new fixflow --template coding-workflow
loom skill lint fixflow --portable
loom skill activate fixflow --agent codex --scope user
loom skill doctor fixflow --agent codex
```

If commands are not implemented at implementation time, show them under a
"Target lifecycle" or "Planned single-skill lifecycle" label and provide the
currently implemented equivalent. Do not present planned commands as generally
available.

Keep the advanced target, binding, and projection flow later in README under an
advanced section.

## `SINGLE_SKILL_LIFECYCLE.md`

Recommended sections:

1. What is a skill?
2. Source versus active view.
3. Installed, active, visible, enabled, disabled-by-config, restart-required.
4. Create or import: `skill new`, `skill add`, `skill save`, `skill capture`.
5. Validate: `skill lint --portable`, agent compatibility, quality checks.
6. Activate and diagnose: planned target workflow plus current implemented
   alternatives when needed.
7. Evaluate: offline eval fixtures and future real-agent baseline flow.
8. Release and rollback: semver release, snapshot, diff, rollback.
9. Automation notes: use `--json`, branch on `ok` and `error.code`.

## `CODEX_SKILL_VISIBILITY.md`

Recommended sections:

1. Codex active-view roots.
2. Why a symlink is not enough.
3. Canonical `SKILL.md` identity.
4. `skills.config` disable rules by path and name.
5. New session or restart guidance.
6. Diagnosis command examples with `--json`.
7. Planned reconcile flow and dry-run-first constraints.

The doc should avoid claiming that Loom can safely edit all user-authored Codex
config entries. It should distinguish Loom-managed rules from manual user
rules.

## `MIGRATING_TO_ACTIVE_VIEW.md`

Recommended sections:

1. Who needs migration.
2. Preflight and backup.
3. Read-only audit.
4. Explicit allowlist.
5. Dry-run activation or reconcile.
6. Apply only reviewed changes.
7. Verify with doctor and JSON.
8. Rollback or recovery notes.

The migration guide must say that existing full-mirror projections are
migration input, not trusted desired active state.

## Link Policy

Each new doc should link to the relevant upstream or local source:

- Agent Skills spec: `https://agentskills.io/specification`
- Agent Skills client implementation guide:
  `https://agentskills.io/client-implementation/adding-skills-support`
- Claude Code skills docs: `https://code.claude.com/docs/en/skills`
- Local Codex active-view plan:
  `docs/plan/codex-active-view-projection-spec.md`

## Verification

Docs-only verification:

```bash
git diff --check
rg -n "full[- ]mirror|\\.codex/skills|\\.agents/skills|skill activate|skill doctor|--json" README.md docs
```

Repository verification before submission:

```bash
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #374` unless the PR fully updates README and all three documentation
files. A spec-only PR must not use `Fixes #374`.
