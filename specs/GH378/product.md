# GH378 Product Spec: Capability Graph And Recommendations

Issue: https://github.com/majiayu000/loom/issues/378
Parent: https://github.com/majiayu000/loom/issues/376
Status: Complete implementation
Locale: zh-CN

## Goal

Add a local capability graph and recommendation layer that helps operators pick
skills or skillsets for a task, workspace, agent, and policy context.

The recommendation system must remain explainable, deterministic in lexical-only
mode, and non-mutating by default. It should rank candidates, show evidence,
surface risks, and provide next actions such as activation commands or dry-run
plans, but it must never silently activate a skill.

## Dependency History

The original design packet depended on these upstream read models and gates:

- #366 for the single-skill status and `skill inspect` read model.
- #367 for authoritative activation/deactivation/list state.
- #368 for active-view visibility and reconcile diagnostics.
- #369 for real eval evidence.
- #370 for blocked/quarantined/trust state.
- #371 for dependency and MCP readiness.
- #377 for skillsets and grouped lifecycle data.

Those dependencies now provide enough local state for the read-only command
surface, deterministic lexical foundation, dependency/eval scoring, and
negative-trigger ranking joins required for GH378 completion.

## Current Implementation Status

The read-only local recommendation foundation is implemented. `loom index
build/status`, `loom skill recommend`, `loom skill resolve`, and `loom active
recommend` are all non-mutating command surfaces.
`skill recommend` and `skill resolve` reuse the deterministic discovery engine
also available through `skill search`; semantic mode remains local-provider-only
and returns a `semantic-disabled` warning with lexical fallback when no local
provider is configured.
Recommendation scoring now joins dependency readiness, persisted eval evidence,
trigger fixtures, trust/quarantine state, safety policy, and skillset member
readiness with focused regression coverage.

## User-Facing Commands

Target command surface:

```bash
loom index build
loom index status
loom skill recommend "fix failing CI" --agent codex --workspace .
loom skill resolve "fix failing CI" --agent codex --semantic
loom active recommend --agent codex --workspace .
```

`loom skill recommend` returns ranked skills and skillsets. `loom active
recommend` returns a dry-run add/remove/keep plan for an active view.

## Non-Goals

1. No automatic activation without explicit apply.
2. No dependency on network-only embedding services in v1.
3. No DAG execution in this issue.
4. No hidden writes to active views, agent config, or registry state from
   recommendation commands.
5. No recommendation of blocked or quarantined skills for activation.
6. No opaque scoring that cannot be explained in output.

## Index Sources

The local index may consume:

- `SKILL.md` frontmatter name and description.
- `SKILL.md` body headings.
- `loom.skill.toml` metadata.
- `evals/triggers.jsonl` positive and negative trigger cases.
- Eval results from #369.
- Skillset membership from #377.
- Dependency and MCP readiness from #371.
- Safety, trust, and quarantine state from #370.
- Runtime and compatibility status from #366/#373.

## Ranking Signals

Candidate score should combine:

1. Lexical match.
2. Semantic match if a local embedding provider is configured.
3. Workspace compatibility.
4. Agent compatibility.
5. Active status.
6. Trust level.
7. Safety risk.
8. Dependency readiness.
9. Eval evidence.
10. Recency or staleness.
11. Skillset coherence.
12. Negative trigger matches from `evals/triggers.jsonl`, which must reduce the
    score or suppress activation recommendations when the task resembles a known
    non-trigger case.

Each candidate must include explanations for positive signals and risks.

## Output Contract

Each ranked result should include:

```json
{
  "kind": "skill",
  "id": "fixflow",
  "score": 0.87,
  "mode": "lexical",
  "score_inputs": {
    "matched_fields": ["description", "trigger_eval"],
    "skillsets": ["ci-maintenance"]
  },
  "reasons": [
    "description matches 'failing CI'",
    "trigger eval recall 0.77",
    "compatible with codex",
    "dependencies ready",
    "trust reviewed"
  ],
  "risks": ["medium risk: can run test commands"],
  "warnings": ["no real-agent eval evidence yet"],
  "recommended_action": "activate",
  "suggested_commands": [
    "loom --json skill activate fixflow --agent codex --binding <binding-id> --dry-run"
  ]
}
```

Skillset recommendations use the same result shape with
`"kind": "skillset"` and an id from the skillset read model. Until a dedicated
`skillset activate` lifecycle exists, skillset results must not suggest
`skill activate <skillset-id>` or any other invalid activation command. They may
suggest read-only inspection such as `loom --json skillset show <skillset-id>`,
or per-member activation dry-runs only when every required member passes safety,
policy, dependency, and readiness filters.

## Behavior Invariants

1. `index build` can run without network access.
2. Lexical-only output is deterministic across runs for identical registry
   state.
3. Semantic mode is optional and disabled unless a local provider is configured.
4. Blocked or quarantined skills are filtered out of activation
   recommendations. Skillsets containing blocked, quarantined, or policy-blocked
   required members are either excluded from activation recommendations or
   degraded to read-only inspection with the unsafe members listed as risks.
5. Unevaluated skills may appear only with warnings.
6. Missing dependencies reduce ranking and appear in risks or warnings.
7. Recommendation output is read-only and does not write registry state, active
   views, agent config, or MCP config. `index build` may write rebuildable
   derived index files under `state/index`, but those files are not source of
   truth.
8. `active recommend` returns a dry-run plan, not mutations.

## Acceptance Criteria

1. Index can be built without network access.
2. Recommendation output is deterministic in lexical-only mode.
3. Ranked candidates include explanations and risk/dependency status.
4. Blocked/quarantined skills are never recommended for activation.
5. Unevaluated skills can be recommended only with a warning.
6. `active recommend` returns a dry-run plan, not mutations.
7. Tests cover lexical ranking, semantic-disabled mode for both `recommend` and
   `resolve`, blocked skill filtering, unsafe skillset-member filtering,
   dependency penalty, positive eval boost, negative-trigger penalty, workspace
   filter, deterministic tie-breaking across result kinds, and skillset
   recommendation.
