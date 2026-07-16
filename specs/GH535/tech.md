# GH535 Tech Spec - Skill command surface convergence

Issue: https://github.com/majiayu000/loom/issues/535
Product spec: `specs/GH535/product.md`
Route: `write_spec`
Human gate: maintainer breaking-change decisions approved on 2026-07-16

## 1. Current Behavior

`src/cli.rs` `SkillCommand` holds 44 subcommands mixing operational and authoring flows; `tests/cli_surface.rs:299` pins the budget at 44 (42 at audit time — the surface is still growing). Authoring handlers live in `src/commands/skill_authoring*.rs`, operational handlers across `src/commands/skill_*.rs`.

## 2. Proposed Design

1. Add nested `SkillCommand::Author { command: SkillAuthorCommand }`; do not add a top-level command group.
2. Move exactly 7 variants (`Draft`, `Extract`, `Rewrite`, `TuneDescription`, `GenerateEvals`, `ApplyPatch`, `New`) into `SkillAuthorCommand`; keep `Improve`, `Regression`, `Eval`, and `Compile` operational. Dispatch in `src/commands/mod.rs` follows.
3. Update moved envelope `cmd` strings to the `skill.author.*` namespace using snake_case identifiers.
4. Update `default_next_actions` / call-site `err_with_next_actions` strings referencing moved paths.
5. Update `tests/cli_surface.rs` budgets, add removed old paths to the stale-command blacklist.
6. Update `docs/LOOM_CLI_CONTRACT.md`, `README.md`, and panel references.

## 3. Affected Areas

1. `src/cli.rs`
2. `src/commands/mod.rs` (dispatch)
3. `src/commands/skill_authoring*.rs` (arg types only)
4. `src/error_actions.rs` and call sites embedding `loom skill …` strings
5. `tests/cli_surface.rs`
6. `docs/LOOM_CLI_CONTRACT.md`, `README.md`
7. `src/panel/` references if any

## 4. Output Contract

Moved commands keep their envelope `data` schema unchanged; only `cmd` and CLI path change. Old paths produce standard clap errors (exit 2). `skill` budget becomes 38 and `skill author` budget becomes 7.

## 5. Verification Plan

1. `cargo test --test cli_surface`
2. Full `cargo test`
3. Repo-wide grep for old command strings returns only the explicit stale-command blacklist tests and migration/CHANGELOG/spec history
4. `docs/LOOM_CLI_CONTRACT.md` contract test passes

## 6. Rollback Plan

Single revert of the taxonomy commit restores the flat surface; no state or data migration involved.

## 7. Product Mapping

1. Invariant 1 maps to the enum move with exhaustive match (compiler-enforced).
2. Invariant 2 maps to the stale blacklist additions.
3. Invariant 3 maps to dispatcher `cmd` string updates + contract test.
4. Invariant 4 maps to the human gate above.
