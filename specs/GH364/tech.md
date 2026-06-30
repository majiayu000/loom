# GH364 Tech Spec: Skill New Scaffolding

Product spec: `specs/GH364/product.md`

## Design

Add a `New` variant to `SkillCommand` and keep the argument structs in `src/cli/skill_new_args.rs` so `src/cli.rs` stays below the hard file-size ceiling.

Implement the command in `src/commands/skill_new.rs`:

1. Validate `<name>` as a portable skill name: lowercase letters, digits, hyphens, no leading/trailing hyphen, no repeated hyphen, length 1-64.
2. Build file bodies for the selected template.
3. For `--dry-run`, return paths and previews before locking or initializing registry state.
4. For real writes, lock the workspace and initialize registry Git state through existing write helpers.
5. Write generated files into a staging directory under `state/`.
6. Run current strict `lint_skill_source` against the staged skill.
7. Parse generated `loom.skill.toml` with a minimal Loom-local parser.
8. Atomically rename the staged skill into `skills/<name>`.
9. Stage and commit `skills/<name>` as a source change.

## Current Lint Compatibility

Current strict lint rejects nested or continued YAML. The generated `SKILL.md` must therefore avoid nested `metadata:` in this slice. Loom-local values such as template, trust, risk, and requirements live in `loom.skill.toml`.

## JSON Contract

Success returns:

```json
{
  "skill": "fixflow",
  "path": "/tmp/root/skills/fixflow",
  "template": "coding-workflow",
  "description": "Use when ...",
  "agent": "codex",
  "created": true,
  "dry_run": false,
  "files": ["SKILL.md"],
  "manifest": {
    "schema": "loom.skill.v1",
    "name": "fixflow",
    "trust": "local-draft"
  },
  "lint": {
    "valid": true,
    "error_count": 0,
    "warning_count": 0
  },
  "commit": "abc123",
  "next_actions": []
}
```

`--dry-run` returns the same identity fields with `created=false`, `dry_run=true`, and `previews`.

## Error Handling

1. Invalid names: `ARG_INVALID`.
2. Existing skill directories: `ARG_INVALID`.
3. Generated lint failure: `SCHEMA_MISMATCH`.
4. Filesystem failures: `IO_ERROR`.
5. Git failures: `GIT_ERROR`.

No errors may be silently downgraded to warnings when they affect generated files or committed state.

## Tests

Add `tests/skill_new_cli.rs`:

1. create with `--template coding-workflow --description ... --agent codex`;
2. generated skill passes `loom skill lint --strict`;
3. `--dry-run` writes no skill or registry state;
4. invalid names fail before source skill files are created;
5. existing skills are not overwritten and no partial files remain.
