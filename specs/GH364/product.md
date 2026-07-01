# GH364 Product Spec: Skill New Scaffolding

## Goal

Add `loom skill new` so a user can start a new registry-owned Agent Skill without manually remembering the directory layout, frontmatter fields, eval stub names, or Loom-local manifest shape.

## Users

1. Skill authors creating a new local skill from scratch.
2. Maintainers who want generated skills to pass current strict portable lint before projection.
3. Agents that need a non-interactive way to create a safe starter skill under an explicit registry root.

## Non-Goals

1. No marketplace or remote package installation.
2. No LLM-based skill generation.
3. No multi-skill skillset behavior.
4. No lint parser expansion for nested Agent Skills metadata; richer lint belongs to #365.
5. No direct writes to live agent host skill directories.

## Command

```bash
loom skill new <name> [--template basic|coding-workflow|scripted|reference-heavy] [--description <text>] [--agent <agent>] [--dry-run]
```

## Generated Layout

```text
skills/<name>/
  SKILL.md
  references/
    README.md
  scripts/
    README.md
  assets/
    README.md
  evals/
    triggers.jsonl
    tasks.jsonl
  loom.skill.toml
```

`SKILL.md` is portable agent-facing metadata and workflow text. `loom.skill.toml` is Loom-local management metadata and must not affect portable lint.

## Acceptance Criteria

1. `loom skill new foo` creates a valid portable skill under `skills/foo`.
2. Generated `SKILL.md` includes `name`, `description`, and workflow sections.
3. Default descriptions are useful enough to pass current strict lint.
4. User-supplied descriptions are preserved.
5. Generated directory includes eval fixture stubs.
6. Generated `loom.skill.toml` is parsed by Loom and ignored by agent-facing portable lint.
7. `--dry-run` returns planned paths and previews without filesystem, Git, registry, pending queue, or command-audit writes.
8. Existing skill directories return a typed error and are not overwritten.
9. Invalid portable names fail before source skill files are created.
10. Tests cover success, invalid name, existing skill, dry-run, and strict lint compatibility.
