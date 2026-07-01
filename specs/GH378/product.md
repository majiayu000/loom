# GH378 Product Spec: Capability Graph And Recommendations

Issue: https://github.com/majiayu000/loom/issues/378
Parent: https://github.com/majiayu000/loom/issues/376
Status: Blocked design packet
Locale: zh-CN

## Goal

Add a local capability graph and recommendation layer that helps operators pick
skills or skillsets for a task, workspace, agent, and policy context.

The recommendation system must remain explainable, deterministic in lexical-only
mode, and non-mutating by default. It should rank candidates, show evidence,
surface risks, and provide next actions such as activation commands or dry-run
plans, but it must never silently activate a skill.

## Blocking Dependencies

Production implementation is blocked by:

- #366 for the single-skill status and `skill inspect` read model.
- #369 for real eval evidence.
- #370 for blocked/quarantined/trust state.
- #371 for dependency and MCP readiness.
- #377 for skillsets and grouped lifecycle data.

The first implementation may extend existing deterministic `skill search` and
`skill resolve` behavior only when it preserves read-only semantics.

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

Each candidate must include explanations for positive signals and risks.

## Output Contract

Each ranked result should include:

```json
{
  "skill": "fixflow",
  "score": 0.87,
  "mode": "lexical",
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
    "loom --json skill activate fixflow --agent codex --scope user --dry-run"
  ]
}
```

## Behavior Invariants

1. `index build` can run without network access.
2. Lexical-only output is deterministic across runs for identical registry
   state.
3. Semantic mode is optional and disabled unless a local provider is configured.
4. Blocked or quarantined skills are filtered out of activation
   recommendations.
5. Unevaluated skills may appear only with warnings.
6. Missing dependencies reduce ranking and appear in risks or warnings.
7. Recommendation output is read-only and does not write registry state, active
   views, agent config, or MCP config.
8. `active recommend` returns a dry-run plan, not mutations.

## Acceptance Criteria

1. Index can be built without network access.
2. Recommendation output is deterministic in lexical-only mode.
3. Ranked candidates include explanations and risk/dependency status.
4. Blocked/quarantined skills are never recommended for activation.
5. Unevaluated skills can be recommended only with a warning.
6. `active recommend` returns a dry-run plan, not mutations.
7. Tests cover lexical ranking, semantic-disabled mode, blocked skill filtering,
   dependency penalty, eval boost, workspace filter, and skillset
   recommendation.
