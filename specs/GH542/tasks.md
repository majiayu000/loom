# GH542 Tasks: skill stats lifecycle governance report

Issue: https://github.com/majiayu000/loom/issues/542
Product spec: `specs/GH542/product.md`
Tech spec: `specs/GH542/tech.md`
Status: implx auto decisions recorded; implementation begins only after this spec PR and GH541 implementation merge
Depends on: GH541

## Order

Reviewed mount/filter decisions -> aggregator views + classifier -> CLI/envelope -> contract docs/tests.

## Tasks

- [ ] `SP542-T001` Owner: spec/review | Dependencies: none | Done when: `loom skill stats` 挂载点、error threshold=5、agent-scoped lifetime/window views、multi-binding guarded global single-runtime、error-as-lifecycle-usage、independent zombie cutoff 与 disabled/history behavior 均记录且独立 diff review 无 blocker | Verify: independent PR #543 reviewer confirms mapped review findings | Covers: B-002, B-003, B-004, B-007
- [ ] `SP542-T002` Owner: skill stats | Dependencies: merged GH541 implementation, `SP542-T001` | Done when: 单遍建立 lifetime/window/global views，支持 v2/v3 telemetry，并只从 current RegistrySnapshot 构造 binding truth、忽略 stale observations | Verify: `cargo test --test skill_stats command_is_read_only_and_linear && cargo test --test skill_stats current_snapshot_ignores_stale_binding_observations` | Covers: B-001, B-002, B-003, B-004, B-006
- [ ] `SP542-T003` Owner: skill stats | Dependencies: `SP542-T002` | Done when: 四类互斥分类、multi-binding guarded global single-runtime、error event lifecycle usage、orphan、error threshold 与包含 unbound-but-used 的 stable ordering 落地 | Verify: `cargo test --test skill_stats lifecycle_categories_are_exhaustive && cargo test --test skill_stats agent_filter_scopes_bindings_but_single_runtime_is_global && cargo test --test skill_stats error_events_count_as_recent_lifecycle_usage && cargo test --test skill_stats durable_unmatched_events_become_orphans && cargo test --test skill_stats error_threshold_is_five && cargo test --test skill_stats ordering_is_stable && cargo test --test skill_stats unbound_but_used_sort_is_stable` | Covers: B-003, B-004, B-005, B-006, B-007, B-008
- [ ] `SP542-T004` Owner: cli | Dependencies: `SP542-T003` | Done when: args、filters 与 envelope（`telemetry_enabled`、`telemetry_empty`、`single_runtime_scope`、`window_events`）稳定，empty/error paths 显式 | Verify: `cargo test --test skill_stats disabled_with_history_is_not_empty && cargo test --test skill_stats empty_and_error_contracts_are_explicit && cargo test --test skill_stats agent_filter_scopes_bindings_but_single_runtime_is_global` | Covers: B-002, B-003, B-009
- [ ] `SP542-T005` Owner: docs | Dependencies: `SP542-T004` | Done when: `docs/LOOM_CLI_CONTRACT.md` 增补命令与 envelope 字段 | Verify: `cargo test --test cli_surface && cargo test --test skill_stats contract_surface_matches` | Covers: B-002, B-003, B-005, B-006, B-007, B-008, B-009
- [ ] `SP542-T006` Owner: verification | Dependencies: all prior | Done when: focused、workspace 与 repository checks 均通过且 command snapshot 不写状态 | Verify: `cargo check --workspace --all-targets --all-features && cargo test && make check` | Covers: B-001, B-002, B-003, B-004, B-005, B-006, B-007, B-008, B-009

## Handoff

- Product invariant set: `B-001..B-009`.
- Task coverage union: `B-001..B-009`.
- GH542 implementation is serial after GH541 because its orphan contract consumes GH541 event schema v3.
- Spec approval is not claimed; `implx auto` authorizes drafting/implementation after the corrected spec PR passes review and merges.
