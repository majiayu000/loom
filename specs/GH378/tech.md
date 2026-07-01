# GH378 Tech Spec: Capability Graph And Recommendations

Issue: https://github.com/majiayu000/loom/issues/378
Product spec: `specs/GH378/product.md`
Status: Blocked design packet

## Current State

`loom skill search` and `loom skill resolve` already provide deterministic
lexical matching over the skill inventory read model. They return `score_inputs`
and do not invoke an LLM. This is the correct base for GH378.

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

`skills.embeddings.jsonl` is optional and may exist only when a local provider
is configured. It must not be required for `index build` or `skill recommend`.

Recommended capability record:

```json
{
  "schema_version": 1,
  "skill_id": "fixflow",
  "source_digest": "sha256:...",
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
loom skill recommend <task> [--agent <agent>] [--workspace <path>] [--semantic]
loom skill resolve <task> [--agent <agent>] [--workspace <path>] [--semantic]
loom active recommend --agent <agent> [--workspace <path>]
```

If `active recommend` is introduced later under a different command group, keep
the JSON plan contract equivalent.

## Ranking Pipeline

Recommended pipeline:

1. Load current skill inventory.
2. Load or build capability records.
3. Join `skill inspect` status once #366 exists.
4. Join safety/trust/quarantine from #370.
5. Join dependency readiness from #371.
6. Join eval evidence from #369.
7. Join skillset membership from #377.
8. Score candidates.
9. Filter out blocked/quarantined activation candidates.
10. Return ranked results with explanations and suggested commands.

Lexical-only scoring must be stable. Sort ties by skill id.

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
- no network embedding service is called by default
- local provider configuration must never expose secrets in output

## Active Recommend Plan

`active recommend` should return:

```json
{
  "agent": "codex",
  "workspace": "/repo",
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

The command is read-only. It may suggest activation/deactivation commands but
must not apply them.

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
