# GH365 Tech Spec: Expanded Skill Lint

Issue: https://github.com/majiayu000/loom/issues/365
Product spec: `specs/GH365/product.md`
Status: Draft for implementation

## Design Summary

Implement the `skill lint` expansion without changing registry state:

1. Move `SkillLintArgs` out of `src/cli.rs` to keep the CLI file under the hard line ceiling.
2. Add `yaml_serde` and replace the hand-rolled top-level-only frontmatter parser.
3. Keep existing report fields and add a `sections` object.
4. Add optional agent and quality checks after portable parsing.
5. Keep all lint/fix behavior read-only.

## Affected Areas

| Area | Files |
|---|---|
| CLI args | `src/cli.rs`, `src/cli/skill_lint_args.rs` |
| lint implementation | `src/commands/skill_lint.rs`, `src/commands/skill_lint/frontmatter.rs`, `src/commands/skill_lint/sections.rs` |
| tests | `tests/skill_lint.rs` |
| docs/specs | `docs/LOOM_CLI_CONTRACT.md`, `specs/GH365/*` |

## Data Model

No registry state migration.

Report addition:

```json
{
  "sections": {
    "portable_spec": { "status": "pass", "findings": [] },
    "agent_compatibility": {
      "codex": { "status": "warning", "findings": ["agent_codex_unsupported_field"] }
    },
    "quality": { "status": "warning", "findings": ["quality_evals_missing"] },
    "resources": { "scripts": 1, "references": 0, "assets": 0 },
    "progressive_disclosure": { "main_line_count": 12, "main_token_estimate": 90 }
  }
}
```

## Validation

Portable parser:

1. Read only the YAML frontmatter between opening and closing `---`.
2. Parse with `yaml_serde`.
3. Require the YAML root to be a mapping.
4. Preserve existing name/description validation and reject descriptions above
   1024 characters in strict portable mode.
5. Accept optional `license`, `compatibility`, `metadata`, and `allowed-tools`.
6. Treat schema-shape issues as strict errors and compat/fix warnings through the existing mode rules.

Agent checks:

1. `--agent codex` warns for Claude-only fields such as `allowed-tools`.
2. `--agent claude` recognizes Claude-only fields without failing portable lint.
3. Codex and Claude checks scan configured agent skill directories and warn when
   another same-name active copy exists outside the source skill path.
4. Unknown agents produce a warning but do not fail portable lint.

Quality checks:

1. Warn on vague or over-broad descriptions.
2. Warn when `SKILL.md` exceeds the recommended line threshold.
3. Warn when eval fixtures are missing.
4. Warn when script files lack shebangs and no nearby usage doc exists.
5. Warn when reference files are nested deeply enough to obscure progressive
   disclosure.

## Test Plan

```bash
cargo test --test skill_lint
cargo check --workspace --all-targets --all-features
git diff --check
```

Run SpecRail workflow validation for this packet when available.

## Rollback

Revert the new CLI args module, YAML dependency, lint parser modules, tests, and contract/spec docs. No registry state needs cleanup.
