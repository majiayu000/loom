# GH477 Tasks: Projection Drift And Content Digests

Issue: https://github.com/majiayu000/loom/issues/477
Product spec: `specs/GH477/product.md`
Tech spec: `specs/GH477/tech.md`
Status: Draft for review

## Order

State model compatibility -> projection digest writing -> observation/write-back path -> status/diagnose tests -> docs.

## Tasks

- [ ] `SP477-T001` Owner: state model | Dependencies: none | Done when: projection records support optional digest/observation fields while existing fixtures load unchanged | Verify: `cargo test state_model`
- [ ] `SP477-T002` Owner: projection write path | Dependencies: `SP477-T001` | Done when: symlink/copy/materialize projection apply records initial source/live observation metadata without adding duplicate SHA wrappers | Verify: `cargo test project`
- [ ] `SP477-T003` Owner: diagnose/watch | Dependencies: `SP477-T001`, `SP477-T002` | Done when: a live projection mismatch can persist `health=drifted` or equivalent durable drift state with structured errors on observation failure | Verify: `cargo test skill_diagnose`
- [ ] `SP477-T004` Owner: status/read model | Dependencies: `SP477-T003` | Done when: `workspace status` drift counts reflect persisted projection health and old records render as `not_observed`/unknown, not false healthy | Verify: `cargo test status`
- [ ] `SP477-T005` Owner: docs | Dependencies: `SP477-T004` | Done when: CLI contract documents digest fields, observation semantics, and read-only vs write-back behavior | Verify: `git diff --check`
- [ ] `SP477-T006` Owner: verification | Dependencies: all prior tasks | Done when: full Rust checks pass after the behavior change | Verify: `cargo check && cargo test`
