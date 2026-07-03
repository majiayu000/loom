# GH482 Tech Spec - Provider outdated and re-pin workflow

Issue: https://github.com/majiayu000/loom/issues/482
Product spec: `specs/GH482/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

Provider-backed install requires immutable refs and records provenance in `sources.json` / `loom.lock`. `provenance verify/refresh` exists, but there is no command that reports installed skills as outdated against provider heads or generates a reviewed re-pin plan.

## 2. Proposed Design

1. Add a read-only outdated command under the provider/provenance surface selected by maintainers.
2. Load provider-backed source records from `sources.json` and lock metadata.
3. Resolve candidate upstream refs without treating unpinned heads as trusted final refs.
4. Compute candidate digest when a candidate immutable ref is available.
5. Add a plan-first re-pin flow that writes a plan artifact or JSON plan only; applying the update remains explicit.

## 3. Affected Areas

1. `src/commands/provenance.rs`
2. `src/commands/provider_cmds/locator.rs`
3. `src/commands/provider_cmds/install.rs`
4. `src/cli/provider.rs` or provenance CLI args
5. `docs/LOOM_CLI_CONTRACT.md`
6. `tests/skill_provenance.rs`, provider command tests

## 4. Output Contract

Outdated result rows should include:

1. `skill_id`
2. `provider`
3. `current_ref`
4. `current_digest`
5. `candidate_ref`
6. `candidate_digest`
7. `status`: `up_to_date`, `outdated`, `unreachable`, `unpinned_candidate`, `invalid_source`
8. `next_actions`

## 5. Verification Plan

1. `cargo test skill_provenance`
2. Provider command tests for reachable/unreachable sources.
3. `cargo check`
4. `cargo test`

## 6. Rollback Plan

Because outdated is read-only by default, rollback is removing the new command surface or hiding the apply/re-pin subcommand while preserving provenance verify behavior.

## 7. Product Mapping

1. Product invariant 1 maps to read-only outdated command tests.
2. Product invariant 2 maps to provider unreachable status.
3. Product invariant 3 maps to unpinned candidate handling.
4. Product invariant 4 maps to plan/apply separation.
