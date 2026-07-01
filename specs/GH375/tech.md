# GH375 Tech Spec: Panel Single-Skill Detail Page

Issue: https://github.com/majiayu000/loom/issues/375
Product spec: `specs/GH375/product.md`
Status: Draft for implementation

## Current State

Panel has a `skills` page with an inline `SkillDetail` component in
`panel/src/pages/panel/SkillsPage.tsx`. It currently combines inventory summary,
history, diagnose, diff, projection controls, and trash state. It can call:

- `GET /api/v1/skills`
- `GET /api/v1/skills/{skill_name}/diagnose`
- `GET /api/v1/skills/{skill_name}/history`
- `GET /api/v1/skills/{skill_name}/diff`

`src/panel/handlers/skills.rs` wraps `skill.diagnose` through the CLI `App`
handler. `docs/LOOM_API_CONTRACT.md` already requires Panel API routes to be
v1 routes and forbids semantics absent from CLI or registry state.

The gap is that the inline detail is not a stable route, and it does not yet
consume a single inspect read model that covers source, compatibility, runtime
visibility, eval evidence, safety, and next actions.

## Backend Contract

Add or reserve this CLI/shared read model:

```bash
loom --json skill inspect <skill>
```

Recommended envelope data:

```json
{
  "skill": {
    "id": "fixflow",
    "source": {
      "registry_path": "/Users/example/.loom-registry/skills/fixflow",
      "entrypoint": "SKILL.md",
      "source_drift": "clean",
      "current_ref": "abc1234",
      "last_commit": "abc1234",
      "provenance": {"status": "verified"}
    },
    "spec": {
      "portable": {"status": "pass"},
      "agents": [
        {"agent": "codex", "status": "warning"},
        {"agent": "claude", "status": "pass"}
      ],
      "findings": []
    },
    "runtime": {
      "agents": [
        {
          "agent": "codex",
          "active": true,
          "projected": true,
          "visible": true,
          "enabled": false,
          "restart_required": false,
          "verdict": "disabled-by-config",
          "target_path": "/Users/example/.agents/skills",
          "materialized_path": "/Users/example/.agents/skills/fixflow",
          "suggested_commands": [
            "loom --json skill diagnose fixflow"
          ]
        }
      ]
    },
    "eval": {
      "last_run_at": null,
      "offline_fixture_status": "missing",
      "baseline": null,
      "with_skill": null,
      "without_skill": null,
      "trigger_precision": null,
      "trigger_recall": null,
      "baseline_delta": null
    },
    "safety": {
      "trust_level": "unknown",
      "scan_summary": {"status": "not-implemented", "findings": 0},
      "quarantine": false,
      "blocked": false,
      "findings": []
    },
    "next_actions": []
  }
}
```

Panel route:

```text
GET /api/v1/skills/{skill_name}/inspect
```

The handler should call the same Rust function used by `skill inspect`; it must
not duplicate compatibility, visibility, eval, or safety logic inside the Panel
handler.

## Frontend Structure

Recommended new files:

- `panel/src/pages/panel/SkillDetailPage.tsx`
- `panel/src/pages/panel/SkillDetailPage.test.tsx`
- `panel/src/lib/api/skill_inspect.ts` or typed additions to
  `panel/src/lib/api/client.ts`

Refactor `SkillsPage.tsx` so the list can select or navigate to the detail
page without keeping all detail logic inline.

Use a compact operational layout:

- top summary strip with skill id, source state, runtime verdict, and next
  blocking action
- section tabs for source/spec, runtime, eval, safety, and actions
- status chips for pass, warning, error, empty, disabled-by-config, and
  restart-required
- copy buttons for suggested commands

Do not use cards nested inside other cards. Keep each repeated finding or
agent runtime row as one bounded item.

## Navigation

If the app remains state-routed in the first slice:

- extend `PanelPageKey` or route state so `skills` can carry
  `selectedSkill` plus an optional detail tab
- persist selected skill and tab through the URL hash or another refresh-safe
  route state mechanism, not component-local React state alone
- command palette skill entries should open the same detail state

If URL routing is added:

- support `/skills/:skillId`
- support `/skills/:skillId/runtime`
- support `/skills/:skillId/evals`
- support `/skills/:skillId/security`
- keep unknown skill ids as a read-model error/empty state, not a crash

## Safety

Panel may show these affordances:

- copy doctor command
- copy reconcile command
- copy eval command
- copy rollback command
- link to existing diagnose/history/diff views

Panel must not silently run:

- activation
- config repair
- quarantine or trust changes
- eval execution
- rollback

If a mutating action is later added, it must use the existing mutation envelope,
local-host authorization, read-only gate, and explicit confirmation pattern.

## Tests

Backend tests:

1. `/api/v1/skills/{skill_name}/inspect` returns the same data shape as the CLI
   inspect read model.
2. Missing skills return a structured envelope error.
3. Panel security tests include the new read route as a v1 route and do not add
   a mutation route.

Frontend tests:

1. Skill list row opens detail page/state.
2. Source/spec/runtime/eval/safety/next-action sections render from fixture
   data.
3. Codex `disabled-by-config`, `needs-restart`, and missing projection states
   have distinct labels/classes.
4. Empty eval and safety evidence render explicit empty states.
5. Suggested commands are shown and copyable.
6. Read-only/offline mode disables mutating affordances.

## Verification

```bash
git diff --check
cd panel && bun run typecheck
cd panel && bun run test
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #375` until the backend read model, Panel API, routed frontend detail
page, and tests all satisfy the acceptance criteria.
