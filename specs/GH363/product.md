# GH363 Product Spec: Single-Skill Lifecycle Workbench

Issue: https://github.com/majiayu000/loom/issues/363
Status: Draft epic coordination spec
Locale: en-US

## Goal

Make Loom a best-in-class workbench for one skill before expanding into
multi-skill ecosystem features. A user should be able to create or import a
single skill, validate it, scan it, activate it, verify that the target agent
can see it, evaluate it against a baseline, improve it, release it, roll it
back, deactivate it, or quarantine it without manually inspecting hidden
registry state.

## Product Thesis

The user-facing object is a lifecycle status card, not just a directory,
projection record, or target binding:

```text
new/import -> validate -> scan -> activate -> verify visibility -> eval -> improve -> release -> rollback/deactivate/deprecate
```

`loom skill inspect <skill>` should become the compact source of truth for:

1. source/provenance/drift;
2. portable and agent-specific spec status;
3. active/installed/visible runtime status;
4. eval evidence versus baseline;
5. safety, trust, quarantine, script, dependency, and MCP readiness state.

## Users

1. Individual users who need to understand whether one skill is valid and
   visible to an agent.
2. Maintainers who need a repeatable quality, safety, and release workflow for
   one skill.
3. Future advanced features that must consume the single-skill status model
   instead of duplicating lifecycle logic.

## Child Issue Map

| Issue | Scope |
|---|---|
| #364 | `skill new` scaffolding and Loom-local skill manifest |
| #365 | portable, agent-specific, and quality lint |
| #366 | first-class status model and `skill inspect` |
| #367 | activation/deactivation/list semantics |
| #368 | Codex visibility doctor and active-view reconcile |
| #369 | real agent eval harness with with-skill/no-skill baselines |
| #370 | safety scan, trust levels, quarantine, and security diff gates |
| #371 | runtime dependency and MCP readiness checks |
| #372 | edit/improve/regression workflow |
| #373 | agent adapters, discovery roots, config visibility, reload semantics |
| #374 | docs and migration guide |
| #375 | Panel single-skill detail page |

## Non-Goals

1. Marketplace scope.
2. Multi-skill DAG orchestration.
3. Team/cloud sync changes beyond current registry support.
4. Replacing native agent skill systems.
5. Advanced ecosystem features before required single-skill foundations are
   stable.

## Behavior Invariants

1. Child features must consume the shared single-skill status model.
2. Status must distinguish missing data, failed checks, blocked checks, and
   passing checks.
3. Activation and visibility must be separate from projection internals.
4. Eval claims require real baseline evidence.
5. Safety/trust state must be visible before activation.
6. Dependency and MCP readiness must not silently degrade.
7. Docs and Panel must consume backend/CLI read models and not invent separate
   lifecycle semantics.
8. Rollback/deactivate/quarantine paths must be explicit and auditable.

## Target UX

```bash
loom skill new fixflow --template coding-workflow
loom skill inspect fixflow
loom skill lint fixflow --portable
loom skill lint fixflow --agent codex
loom skill scan fixflow
loom skill activate fixflow --agent codex --scope user
loom skill doctor fixflow --agent codex
loom skill eval run fixflow --agent codex --baseline no-skill
loom skill release fixflow v1.0.0
```

## Epic Acceptance Criteria

1. A user can create or import one skill and see source/provenance status.
2. A user can validate portable and agent-specific compatibility.
3. A user can activate and deactivate the skill for Codex without confusing
   projection state for active state.
4. A user can verify visibility and diagnose common Codex failure modes.
5. A user can run an eval against a no-skill baseline and see the result in
   inspect.
6. A user can see safety/trust/dependency/MCP readiness before activation.
7. A user can improve the skill and run regression checks before release.
8. A user can release and roll back a skill with auditable state changes.
9. Docs explain the full lifecycle and migration from earlier projection-only
   workflows.
10. Panel renders the same lifecycle status from backend read models.

## Completion Policy

This epic should close only after the child issue implementations that satisfy
the acceptance criteria are merged and verified. A spec or planning PR for this
epic must use `Refs #363`, not a closing keyword.

Closeout evidence:

- Child issues #364 through #375 are closed.
- `tests/single_skill_lifecycle_e2e.rs` covers the end-to-end single-skill
  lifecycle without manual registry inspection.
- Current verification for the closeout PR includes Rust checks, the lifecycle
  E2E test, `scripts/e2e-agent-flow.sh`, and Panel typecheck/test.
