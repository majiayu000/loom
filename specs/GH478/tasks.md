# GH478 Tasks: Rollback Projection Reconciliation

Issue: https://github.com/majiayu000/loom/issues/478
Product spec: `specs/GH478/product.md`
Tech spec: `specs/GH478/tech.md`
Status: Implemented

## Order

Snapshot error surfacing -> structured recovery output -> optional live reconciliation decision -> tests/docs.

## Tasks

- [x] `SP478-T001` Owner: rollback | Dependencies: none | Done when: rollback handles `Ok(Some)`, `Ok(None)`, and snapshot load `Err` explicitly and never drops projection-warning evidence | Verify: `cargo test project`
- [x] `SP478-T002` Owner: rollback output | Dependencies: `SP478-T001` | Done when: rollback JSON includes `projection_reconciliation` with stale projection `instance_id`, `target_id`, explicit `materialized_path`, method, status, `requires_projection_reapply`, and exact executable next commands or an explicit manual-review action | Verify: `cargo test project`
- [x] `SP478-T003` Owner: safety | Dependencies: `SP478-T002` | Done when: maintainers have chosen recovery-plan-only or automatic Loom-owned live reproject behavior, and code follows that choice | Verify: recovery-plan-only selected per `specs/GH478/tech.md` default
- [x] `SP478-T004` Owner: tests | Dependencies: `SP478-T003` | Done when: tests cover stale copy, stale materialize, symlink no-op, missing target, and corrupt snapshot | Verify: `cargo test rollback_preview && cargo test project`
- [x] `SP478-T005` Owner: docs | Dependencies: `SP478-T004` | Done when: rollback docs do not imply live agent content is updated unless reconciliation is verified | Verify: `git diff --check`
- [x] `SP478-T006` Owner: verification | Dependencies: all prior tasks | Done when: full Rust checks pass | Verify: `cargo check && cargo test`
