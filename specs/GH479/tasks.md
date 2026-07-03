# GH479 Tasks: Remove Silent Degradation In Safety And Recovery

Issue: https://github.com/majiayu000/loom/issues/479
Product spec: `specs/GH479/product.md`
Tech spec: `specs/GH479/tech.md`
Status: Draft for review

## Order

Fix one silent degradation lane at a time: safety cleanup -> patch recovery -> provenance atomic writes -> digest failure -> regression suite.

## Tasks

- [ ] `SP479-T001` Owner: safety | Dependencies: none | Done when: quarantine/active projection cleanup returns structured snapshot read errors instead of empty cleanup evidence | Verify: `cargo test skill_safety`
- [ ] `SP479-T002` Owner: patch recovery | Dependencies: none | Done when: `restore_preimages` reports per-path write/remove failures and callers propagate them in envelope details | Verify: `cargo test skill_authoring`
- [ ] `SP479-T003` Owner: provenance | Dependencies: none | Done when: `sources.json` and `loom.lock` writes use atomic write semantics and preserve old files on failure | Verify: `cargo test skill_provenance`
- [ ] `SP479-T004` Owner: recommendations | Dependencies: none | Done when: JSON digest generation is fallible and no longer hashes empty bytes on serialization failure | Verify: `cargo test skill_inventory_cli`
- [ ] `SP479-T005` Owner: docs/review | Dependencies: `SP479-T001`, `SP479-T002`, `SP479-T003`, `SP479-T004` | Done when: error behavior is documented where command output changes | Verify: `git diff --check`
- [ ] `SP479-T006` Owner: verification | Dependencies: all prior tasks | Done when: focused and full Rust checks pass | Verify: `cargo check && cargo test`
