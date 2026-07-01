# GH375 Product Spec: Panel Single-Skill Detail Page

Issue: https://github.com/majiayu000/loom/issues/375
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

Add a Panel detail page for one skill that answers the same questions as the
CLI single-skill read model:

1. What is this skill source?
2. Is it portable and compatible with the target agents?
3. Is it active, projected, visible, enabled, disabled by config, or waiting
   for restart?
4. What quality or eval evidence exists?
5. Is it trusted, blocked, quarantined, or carrying security findings?
6. What should the operator do next?

The CLI and shared backend read models remain the source of truth. The Panel
must not infer safety, visibility, or eval status from partial frontend data.

## Scope For First PR

Implement the first read-only detail-page slice:

- Skill list rows navigate to a stable single-skill detail view.
- Detail view renders sections for source, spec/compatibility, runtime
  visibility, quality/eval evidence, safety/trust, and next actions.
- Panel consumes the same backend read model as `loom --json skill inspect
  <skill>` once that model exists.
- If `skill inspect` is not implemented yet, the first UI slice may render a
  clearly partial detail view from existing `skill show`, `skill diagnose`,
  `skill history`, and registry projection data, but the spec must reserve the
  final inspect contract.
- Dangerous actions are copyable CLI commands or existing confirmed mutation
  flows, never silent Panel-only mutations.

## Non-Goals

1. Do not implement direct Panel mutation paths for activation, quarantine,
   trust changes, config repair, eval execution, or rollback without the CLI
   safety gate.
2. Do not fabricate eval or safety data when no backend evidence exists.
3. Do not make the Panel own visibility logic separately from the CLI/shared
   backend.
4. Do not replace the existing skills list, history, diff, diagnose, trash, or
   projection controls unless the replacement is fully covered by tests.
5. Do not add marketing or explanatory onboarding content to the operational
   detail page.

## User Experience

Routes should support these stable views:

```text
/skills/:skillId
/skills/:skillId/runtime
/skills/:skillId/evals
/skills/:skillId/security
```

If the current Panel router is state-based instead of URL-based, the first PR
may implement equivalent internal navigation and persist the active route in
state. A later route cleanup should not change the read model contract.

For one skill, render these sections:

### Source

- Registry path.
- Entrypoint.
- Source drift.
- Current ref or last commit.
- Provenance status.

### Spec And Compatibility

- Portable Agent Skills lint status.
- Codex compatibility status.
- Claude compatibility status.
- Findings with severity and suggested action.

### Runtime Visibility

- Per-agent state: inactive, active, projected, visible,
  disabled-by-config, needs-restart, conflict.
- Target path.
- Materialized path.
- Suggested doctor or reconcile commands.

### Quality And Eval Evidence

- Last eval date.
- Offline fixture status.
- Real with-skill versus no-skill summary when available.
- Trigger precision, recall, and baseline delta when available.
- Explicit empty state when no eval evidence exists.

### Safety And Trust

- Trust level.
- Scan summary.
- Quarantine or blocked state.
- Security findings.
- Explicit empty state when safety scanning is not implemented yet.

### Next Actions

- Actions derived from `skill inspect.next_actions`.
- Dangerous actions require a CLI command copy or existing CLI-backed
  confirmation workflow.

## Behavior Invariants

1. Panel read data comes from CLI/shared backend read models.
2. Panel route state and selected skill stay in sync when a user clicks a skill
   row, opens a command-palette result, or refreshes the page.
3. Missing eval or safety data renders as empty evidence, not success.
4. Codex disabled-by-config and restart-required states are visually distinct
   from missing projection.
5. Findings include suggested commands when the backend provides them.
6. Mutating buttons remain disabled in read-only/offline mode.
7. Panel tests cover pass, warning, error, and empty evidence states.

## Acceptance Criteria

1. Skill list rows link to a skill detail page.
2. Detail page renders source, spec, runtime, quality, safety, and next actions.
3. Codex disabled-by-config and restart-required states are visually distinct
   from missing projection.
4. Findings include suggested commands.
5. Panel uses read models from CLI/shared backend logic.
6. No Panel mutation bypasses CLI safety checks.
7. Tests cover rendering pass/warning/error states and empty eval/safety data.
