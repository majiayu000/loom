# GH542 Tasks: skill stats lifecycle governance report

Issue: https://github.com/majiayu000/loom/issues/542
Product spec: `specs/GH542/product.md`
Tech spec: `specs/GH542/tech.md`
Status: implx auto decisions recorded; implementation begins only after this spec PR and GH541 implementation merge
Depends on: GH541

## Order

Reviewed mount/filter decisions -> normalized telemetry query + one locked snapshot -> three aggregator projections + classifier -> CLI/envelope -> contract docs/tests.

## Tasks

- [ ] `SP542-T001` Owner: spec/review | Dependencies: none | Done when: `loom skill stats` 挂载点、error threshold=5、attempt denominator、all/scoped/window 三投影、agentless policy、exact-window orphan、multi-binding guarded global single-runtime、independent zombie cutoff、one locked snapshot 与 checked overflow behavior 均记录且独立 diff review 无 blocker | Verify: independent PR #543 reviewer confirms mapped review findings | Covers: B-002, B-003, B-004, B-006, B-007, B-009
- [ ] `SP542-T002` Owner: telemetry query/inventory | Dependencies: merged GH541 implementation, `SP542-T001` | Done when: `telemetry/query.rs` 单遍 normalize v2/v3 events 为保留全部 report event/metric/filter 的 full dataset，并派生 usage-only view；inventory 可从 caller-supplied `RegistrySnapshot` 构造；stats 持有现有 exclusive workspace lock 做只读 snapshot/config/log capture 后释放锁聚合，不新增 RW lock | Verify: `cargo test --test telemetry normalized_query_preserves_report_metrics_and_filters && cargo test --test skill_stats command_is_read_only_and_linear && cargo test --test skill_stats current_snapshot_ignores_stale_binding_observations && cargo test --test skill_stats stats_reads_one_locked_snapshot` | Covers: B-001, B-002, B-003, B-004, B-006, B-009
- [ ] `SP542-T003` Owner: skill stats | Dependencies: `SP542-T002` | Done when: 单遍建立 all_lifetime/scoped_lifetime/window 三投影；四类仅由 scoped_lifetime + bindings/cutoff 决定且独立于 since；agentless unfiltered-only、multi-binding guarded global single-runtime、exact-window orphan/reconciliation、正交 agentless subtotal、checked invocation/error/attempt counters，以及 skill/per-agent/orphan/agentless 各层显式 failure-category maps（空时 `{}` 且 values sum 等于 error_count）、error threshold 与 stable ordering落地 | Verify: `cargo test --test skill_stats lifecycle_categories_are_exhaustive && cargo test --test skill_stats unbound_category_is_independent_from_since && cargo test --test skill_stats agent_filter_scopes_bindings_but_single_runtime_is_global && cargo test --test skill_stats agentless_events_are_unfiltered_only && cargo test --test skill_stats error_events_count_as_recent_lifecycle_usage && cargo test --test skill_stats durable_unmatched_events_become_orphans && cargo test --test skill_stats orphans_share_since_and_agent_window && cargo test --test skill_stats window_totals_reconcile_with_skill_and_orphan_attempts && cargo test --test skill_stats error_threshold_is_five && cargo test aggregation_overflow_fails_without_partial_output && cargo test --test skill_stats ordering_is_stable` | Covers: B-003, B-004, B-005, B-006, B-007, B-008, B-009
- [ ] `SP542-T004` Owner: cli | Dependencies: `SP542-T003` | Done when: args、filters 与 envelope（`telemetry_enabled`、`telemetry_empty`、`single_runtime_scope`、`window_events`、各层 invocation/error/attempt counters 与 failure-category maps、agentless/unattributed counts）稳定，empty/error paths 显式 | Verify: `cargo test --test skill_stats disabled_with_history_is_not_empty && cargo test --test skill_stats empty_and_error_contracts_are_explicit && cargo test --test skill_stats agent_filter_scopes_bindings_but_single_runtime_is_global && cargo test --test skill_stats orphan_and_error_threshold_contract && cargo test --test skill_stats unbound_category_is_independent_from_since` | Covers: B-002, B-003, B-004, B-007, B-009
- [ ] `SP542-T005` Owner: docs | Dependencies: `SP542-T004` | Done when: `docs/LOOM_CLI_CONTRACT.md` 增补命令与 envelope 字段 | Verify: `cargo test --test cli_surface && cargo test --test skill_stats contract_surface_matches` | Covers: B-002, B-003, B-005, B-006, B-007, B-008, B-009
- [ ] `SP542-T006` Owner: verification | Dependencies: all prior | Done when: focused、workspace 与 repository checks 均通过，command 只读，one-snapshot/seeded-overflow/window reconciliation fixtures 全绿 | Verify: `cargo test --test skill_stats stats_reads_one_locked_snapshot && cargo test aggregation_overflow_fails_without_partial_output && cargo test --test skill_stats window_totals_reconcile_with_skill_and_orphan_attempts && cargo check --workspace --all-targets --all-features && cargo test && make check` | Covers: B-001, B-002, B-003, B-004, B-005, B-006, B-007, B-008, B-009

## Handoff

- Product invariant set: `B-001..B-009`.
- Task coverage union: `B-001..B-009`.
- GH542 implementation is serial after GH541 because its orphan contract consumes GH541 event schema v3.
- Spec approval is not claimed; `implx auto` authorizes drafting/implementation after the corrected spec PR passes review and merges.
