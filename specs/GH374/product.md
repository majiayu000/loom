# GH374 Product Spec: Single-Skill Lifecycle Docs

Issue: https://github.com/majiayu000/loom/issues/374
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

Reframe the user-facing documentation around the simpler single-skill
lifecycle before exposing lower-level registry, target, binding, and projection
internals.

The documentation should teach users to manage one skill through creation,
import, linting, activation, visibility diagnosis, evaluation, release, and
rollback. It should also make the Codex active-view model explicit so users do
not confuse a projected filesystem entry with a skill that Codex can actually
see and use.

## Scope For First PR

Update documentation only:

- Add `docs/SINGLE_SKILL_LIFECYCLE.md`.
- Add `docs/CODEX_SKILL_VISIBILITY.md`.
- Add `docs/MIGRATING_TO_ACTIVE_VIEW.md`.
- Update README quick start so single-skill commands appear before advanced
  registry/projection internals.
- Correct or mark legacy any docs that imply Codex should receive a full mirror
  of every registry skill.

## Non-Goals

1. Do not implement new commands in this docs slice.
2. Do not claim `skill activate`, `skill inspect`, `skill diagnose`, `codex
   reconcile`, or `skill eval run` behavior exists unless the command is
   already implemented; mark target workflows as planned when needed.
3. Do not change runtime migration behavior.
4. Do not remove advanced target, binding, projection, or sync docs.
5. Do not document unsafe bulk activation as the recommended migration path.

## Terminology To Define

The new docs must define these terms consistently:

- Source: the canonical registry-owned skill files.
- Target: an agent skill directory known to Loom.
- Active view: the small runtime directory set the agent scans for active
  skills.
- Active: Loom intends the skill to be in a target active view.
- Installed: skill files exist in a source or target location.
- Visible: the target agent can discover the skill in its active view.
- Enabled: agent config does not disable the visible skill by canonical
  `SKILL.md` path.
- Disabled-by-config: files exist but agent configuration suppresses the skill.
- Restart-required: files/config changed and the agent may need a new session.

## Documentation Requirements

### `docs/SINGLE_SKILL_LIFECYCLE.md`

Explain the target workflow:

```bash
loom skill new fixflow
loom skill lint fixflow --portable
loom skill lint fixflow --quality
loom skill activate fixflow --agent codex --binding <binding-id>
loom skill diagnose fixflow
loom skill eval run fixflow --agent codex --baseline no-skill
loom skill release fixflow v1.0.0
```

The document must say which commands are implemented now and which are planned
by the single-skill lifecycle epic if that distinction is still true when the
PR is implemented.

### `docs/CODEX_SKILL_VISIBILITY.md`

Explain Codex-specific visibility:

- Preferred user active view: `~/.agents/skills`.
- Legacy user root: `${CODEX_HOME:-~/.codex}/skills`.
- Project roots: `.agents/skills` discovered from the current working directory
  up through the repository root.
- Symlink target canonicalization.
- `skills.config` disables by canonical `SKILL.md` path; skill names are used
  only for collision/display diagnostics.
- New session or restart guidance.
- How to use JSON output for automation when diagnosing visibility.

### `docs/MIGRATING_TO_ACTIVE_VIEW.md`

Explain a safe dry-run-first migration:

1. Audit current registry, targets, projections, and Codex config.
2. Build an explicit allowlist.
3. Activate only selected skills.
4. Reconcile stale projections.
5. Repair or report config disables.
6. Verify with `diagnose` and JSON output.

## Behavior Invariants

1. The README quick start must not imply that full registry mirroring is the
   default recommended Codex path.
2. Advanced registry/projection examples remain available for power users.
3. Automation examples use `--json` where output is meant to be parsed.
4. Dry-run or read-only commands appear before apply commands in migration
   guidance.
5. Docs link to the Agent Skills spec, the client implementation guide, Claude
   Code skills docs, and the local Codex active-view plan.
6. Legacy behavior is labeled as legacy instead of silently removed from
   historical architecture docs.

## Acceptance Criteria

1. README has a single-skill quick start before advanced registry/projection
   internals.
2. New docs define source, target, active view, active, visible, enabled,
   disabled-by-config, and restart-required.
3. Codex docs explicitly mention `~/.agents/skills`, legacy
   `${CODEX_HOME:-~/.codex}/skills`, and the current-working-directory to
   repository-root `.agents/skills` search chain.
4. Migration guide includes a safe dry-run-first active-view migration.
5. Command examples use `--json` where automation output matters.
6. Docs link back to Agent Skills spec and relevant upstream docs.
7. Existing docs that imply full-mirror Codex behavior are corrected or marked
   legacy.
