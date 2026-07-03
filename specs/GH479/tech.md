# GH479 Tech Spec - Remove silent degradation in safety and recovery

Issue: https://github.com/majiayu000/loom/issues/479
Product spec: `specs/GH479/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

Several code paths intentionally or accidentally drop errors:

1. `active_projection_cleanup` maps snapshot load errors to an empty list.
2. `restore_preimages` ignores write/remove failures.
3. `write_sources` and `write_lock` use non-atomic `fs::write`.
4. `digest_json` hashes empty bytes when serialization fails.

## 2. Proposed Design

1. Change cleanup helpers to return `Result<CleanupReport>` and propagate snapshot load errors.
2. Change `restore_preimages` to return `Result<Vec<RestoreFailure>>` or fail on the first restore error with path details.
3. Replace provenance `fs::write` calls with `write_atomic`.
4. Change JSON digest helpers to return `Result<String>` and thread errors to callers.
5. Preserve benign best-effort cleanup only for temporary files where user-visible state is unaffected.

## 3. Affected Areas

1. `src/commands/skill_safety.rs`
2. `src/commands/skill_authoring_apply.rs`
3. `src/commands/provenance.rs`
4. `src/commands/skill_recommend.rs`
5. `src/fs_util.rs` if atomic helper behavior needs extension.
6. `tests/skill_safety.rs`, `tests/skill_authoring.rs`, `tests/skill_provenance.rs`, `tests/skill_inventory_cli.rs`

## 4. Error Contract

1. Corrupt registry state should use existing state corruption error codes where available.
2. Restore failures should include path, action, and source operation.
3. Atomic write failures should preserve the old file and return `IO_ERROR`.
4. Digest serialization failure should be impossible to confuse with a valid digest.

## 5. Verification Plan

1. `cargo test skill_safety`
2. `cargo test skill_authoring`
3. `cargo test skill_provenance`
4. `cargo test skill_inventory_cli`
5. `cargo check`
6. `cargo test`

## 6. Rollback Plan

Each fix is isolated by module. If one area regresses, revert that module's error-propagation change while keeping atomic provenance writes and digest failure behavior.

## 7. Product Mapping

1. Product invariant 1 maps to cleanup `Result` behavior.
2. Product invariant 2 maps to restore failure details.
3. Product invariant 3 maps to atomic writes.
4. Product invariant 4 maps to fallible digest generation.
