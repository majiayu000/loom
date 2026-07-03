# GH378 Tech Spec: Capability Graph And Recommendations

Issue: https://github.com/majiayu000/loom/issues/378
Product spec: `specs/GH378/product.md`
Status: Complete implementation

## Current State

`loom skill search`, `loom skill recommend`, and `loom skill resolve` provide
deterministic lexical matching over the skill inventory read model. They return
`score_inputs` and do not invoke an LLM. This is the correct base for GH378.

Recommendation scoring joins dependency readiness, persisted eval evidence,
negative-trigger fixtures, safety/trust state, and skillset member readiness
without mutating registry or active-view state.

Current relevant files:

- `src/cli/discovery.rs`
- `src/commands/skill_inventory.rs`
- `tests/skill_inventory_cli.rs`
- `src/commands/skill_policy.rs`
- `src/commands/skill_eval.rs`
- `src/commands/skillset_cmds.rs`

GH378 should extend this read-only discovery path with an indexed capability
model rather than replacing it.

## State Layout

Add local index state under:

```text
state/index/
  skills.lexical.json
  skills.capabilities.json
  skills.embeddings.jsonl
  workspaces.json
```

`state/index/` is rebuildable cache data. It must be ignored by registry
commit/sync/backup flows unless a later spec explicitly promotes index artifacts
to reviewed registry state.

`skills.embeddings.jsonl` is optional and may exist only when a local provider
is configured. It must not be required for `index build` or `skill recommend`.

Recommended lexical record:

```json
{
  "schema_version": 1,
  "skill_id": "fixflow",
  "source_digest": "sha256:...",
  "tokens": ["fixflow", "ci", "failure", "tests"],
  "fields": {
    "name": ["fixflow"],
    "description": ["fix", "failing", "ci"],
    "headings": ["workflow", "verification"],
    "positive_triggers": ["failing tests"],
    "negative_triggers": ["write product copy"]
  },
  "source_timestamp": null
}
```

`skills.lexical.json` stores records sorted by `skill_id`; every record includes
the source digest used to derive tokens. Timestamp fields are omitted unless they
come from committed registry metadata covered by the source digest. Lexical
tokenization must be deterministic and rebuildable from registry source,
`SKILL.md`, and trigger eval files.

Recommended capability record:

```json
{
  "schema_version": 1,
  "skill_id": "fixflow",
  "source_digest": "sha256:...",
  "input_digests": {
    "eval": "sha256:...",
    "trust": "sha256:...",
    "dependency_readiness": "sha256:...",
    "skillsets": "sha256:..."
  },
  "capabilities": ["test-diagnosis", "ci-failure", "code-fix"],
  "triggers": ["failing tests", "CI failure"],
  "domains": ["software-engineering"],
  "tools": ["git", "test-runner"],
  "risk": "medium",
  "trust": "reviewed",
  "dependency_status": "ready",
  "eval": {
    "trigger_precision": 0.91,
    "trigger_recall": 0.77,
    "baseline_delta": 0.14
  },
  "skillsets": ["ci-maintenance"]
}
```

Recommended workspace index record:

```json
{
  "schema_version": 1,
  "workspace": "/repo",
  "agent": "codex",
  "binding_id": "bind_codex_project",
  "policy_profile": "deny-risky",
  "active_view_digest": "sha256:...",
  "source_digest": "sha256:...",
  "compatibility": {
    "supports_symlink": true,
    "supports_copy": true,
    "visible_roots_digest": "sha256:..."
  },
  "input_digests": {
    "bindings": "sha256:...",
    "targets": "sha256:...",
    "adapter": "sha256:..."
  }
}
```

`workspaces.json` stores records sorted by `workspace`, `agent`, then
`binding_id`. Every compatibility or policy signal must be derived from the
listed input digests so stale workspace context can be detected before ranking.

The index must be derived data. It can be rebuilt from registry source and
read models. Do not make it the source of truth for skill metadata.

## CLI Surface

Add an `index` command group:

```bash
loom index build [--no-embeddings] [--provider local|none]
loom index status
```

Extend discovery:

```bash
loom skill recommend <task> [--agent <agent>] [--workspace <path>] [--binding <binding-id>|--policy-profile <profile>] [--semantic]
loom skill resolve <task> [--agent <agent>] [--workspace <path>] [--semantic]
loom active recommend <task> --agent <agent> [--workspace <path>] [--binding <binding-id>] [--desired-skill <skill>...]
```

If `active recommend` is introduced later under a different command group, keep
the JSON plan contract equivalent.

## Ranking Pipeline

Recommended pipeline:

1. Load current skill inventory.
2. Load existing capability records or build transient in-memory records.
3. Join `skill inspect` status once #366 exists.
4. Join safety/trust/quarantine from #370.
5. Join dependency readiness from #371.
6. Join eval evidence from #369.
7. Join skillset membership from #377.
8. Resolve policy context from `--binding`, an explicit `--policy-profile`, or a
   single unambiguous workspace binding; otherwise fail closed before returning
   activation recommendations.
9. Score candidates, applying positive trigger boosts and negative trigger
   penalties or filters.
10. Filter out blocked, quarantined, or policy-blocked activation candidates,
   including standalone skills and skillset candidates whose required members are
   blocked, quarantined, policy-blocked, or dependency-unready for activation.
11. Return ranked results with explanations and suggested commands.

Recommendation commands must not persist rebuilt index data. If `state/index`
is missing or stale, they may use an in-memory rebuild for the current response
with a warning, or refuse with a suggested `loom index build`; only
`loom index build` may write `state/index`.

Lexical-only scoring must be stable. Sort by score descending, then `kind`, then
`id`, then source path when needed for a final deterministic tie-break.

## Scoring Contract

Keep scoring transparent:

- `score_inputs`: raw fields that matched.
- `reasons`: human-readable positive evidence.
- `risks`: policy, safety, dependency, or eval concerns.
- `warnings`: non-blocking missing evidence.
- `mode`: `lexical`, `semantic-local`, or `semantic-disabled`.
- `recommended_action`: `none`, `inspect`, `activate`, `evaluate`,
  `fix-dependencies`, or `review-policy`.

Scores may be floats, but tests must avoid brittle exact values unless the
weights are intentionally part of the public contract.

## Semantic Mode

Semantic retrieval is optional:

- default mode remains lexical
- `--semantic` without a configured local provider returns
  `semantic-disabled` with a warning and falls back to lexical ranking
- the same fallback behavior and argument parsing apply to both `skill
  recommend --semantic` and `skill resolve --semantic`
- no network embedding service is called by default
- local provider configuration must never expose secrets in output

## Active Recommend Plan

`active recommend` requires `--binding <binding-id>` when the agent/workspace
cannot resolve to exactly one active binding. Ambiguous or missing binding
resolution fails closed with a structured ambiguity result rather than comparing
against an arbitrary active view.
It also requires an explicit task, desired skill list, or prior recommendation
artifact as the desired-state input. Without that input, the command is limited
to workspace hygiene findings and must not produce add/remove activation plans.

`active recommend` should return:

```json
{
  "agent": "codex",
  "workspace": "/repo",
  "task": "fix failing CI",
  "binding_id": "bind_codex_project",
  "dry_run": true,
  "plan": {
    "add": [],
    "keep": [],
    "remove": []
  },
  "policy": {"allowed": true},
  "suggested_commands": []
}
```

The canonical command is `loom active recommend`; if implementation ownership
lands under another command group, `loom active recommend` must remain available
as a compatibility alias with the same JSON contract. The command is read-only.
It may suggest activation/deactivation commands but must not apply them.

## Tests

Focused tests should cover:

1. `index build` succeeds without network access.
2. Lexical-only recommendations are deterministic.
3. `--semantic` falls back with a warning when no local provider exists.
4. Blocked/quarantined skills are not activation recommendations.
5. Missing dependencies reduce score and appear as risks.
6. Eval evidence boosts candidates and missing eval emits warnings.
7. Workspace filter/boost is explained.
8. Skillset recommendation joins member skills.
9. Commands are read-only and do not mutate registry state or target dirs.

## Verification

```bash
git diff --check
cargo test --test skill_inventory_cli
cargo test --test skill_eval
cargo test --test skill_policy
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #378` for design-only or partial slices. Do not use `Fixes #378`
until index build, recommendation output, active dry-run plan, and all filtering
rules are implemented and verified.
