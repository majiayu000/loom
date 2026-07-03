# GH477 Tech Spec - Projection drift and content digests

Issue: https://github.com/majiayu000/loom/issues/477
Product spec: `specs/GH477/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

`RegistryProjectionInstance` records `last_applied_rev`, `health`, and `observed_drift`, but not the source tree digest or live projection digest. Projection write paths set new records back to healthy. `skill diagnose` computes source drift and reports projection fields, but it does not persist projection drift state.

## 2. Proposed Design

1. Add projection digest metadata to the projection state model. The minimum useful shape is:
   - `source_tree_digest`
   - `materialized_tree_digest`
   - `last_observed_at`
   - `last_observed_error`
2. Reuse the existing tree digest code where possible instead of adding another SHA-256 wrapper.
3. Add an observation path that compares source and live projection content for copy/materialize and records the result.
4. Keep symlink projections path-based: verify the symlink/canonical target and record an observation without copying content.
5. Make `workspace status` count projection drift from persisted `health` and `observed_drift`.

## 3. Affected Areas

1. `src/state_model/mod.rs`: projection fields and serialization compatibility.
2. `src/state_model/persistence.rs`: empty/default projection fixtures and migration tests.
3. `src/commands/projections.rs`: write initial digest metadata when projecting.
4. `src/commands/skill_diagnose.rs`: observation and optional persistence behavior.
5. `src/commands/provenance.rs` or a shared digest helper: source tree digest reuse.
6. `tests/project.rs`, `tests/skill_diagnose.rs`, `tests/status.rs`: behavior coverage.

## 4. Compatibility

1. Existing projection records without digest fields must continue to load.
2. Missing digest means `unknown` or `not_observed`, not `healthy`.
3. JSON readers must tolerate old records while new write paths emit the new fields.

## 5. Verification Plan

1. `cargo test skill_diagnose`
2. `cargo test project`
3. `cargo test status`
4. `cargo check`
5. `cargo test`

## 6. Rollback Plan

The change should be backward-compatible. If the new observation path causes regressions, disable only the write-back behavior behind a clearly named internal guard while leaving read compatibility intact.

## 7. Product Mapping

1. Product invariant 1 maps to digest comparison and persisted projection health.
2. Product invariant 2 maps to structured observation errors.
3. Product invariant 3 maps to `workspace status` count tests.
4. Product invariant 4 maps to distinct status fields and error codes.
