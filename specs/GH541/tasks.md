# GH541 Tasks: Telemetry ingestion from agent session logs

Issue: https://github.com/majiayu000/loom/issues/541
Product spec: `specs/GH541/product.md`
Tech spec: `specs/GH541/tech.md`
Status: Follow-up review remediation complete; independent review PASS; repository verification PASS; exact-head CI pending

## Order

Reviewed anchor/schema decisions -> stable record/event identity + continuity cursor -> parsers -> normalized query + matching/dedup -> CAS commit -> CLI/envelope -> contract docs/tests.

## Tasks

- [x] `SP541-T001` Owner: spec/review | Dependencies: none | Done when: fixture-owned invocation anchors、unmatched/rejected persistence、export、home precedence、logical source/stable record identity、continuity cursor privacy/backfill/reset、missing-root behavior 与 retired-skill policy 均记录且独立 diff review 无 blocker | Verify: independent PR #543 reviewer confirms all validated review findings are mapped | Covers: B-001, B-002, B-004, B-005, B-006, B-007, B-009, B-010
- [x] `SP541-T002` Owner: telemetry model | Dependencies: merged `SP541-T001` spec | Done when: event schema v3 支持 validated `observed_skill_name` 与 deterministic event-id override，同时读取 v2 events | Verify: `cargo test telemetry_event_v3_is_redacted_and_v2_remains_readable` | Covers: B-002, B-004, B-010
- [x] `SP541-T003` Owner: telemetry ingest | Dependencies: `SP541-T002` | Done when: `SourceCheckpoint` 使用 generation token、committed newline boundary、boundary hash 与 covered_since；partial tail 保留，truncate/replacement/same-size rewrite reset；scan-plan 在锁外生成并以 expected checkpoint 在 workspace lock 内 compare-and-commit | Verify: `cargo test --test telemetry_ingest cursor_hashes_paths_and_backfills_older_since && cargo test --test telemetry_ingest trailing_partial_record_is_not_malformed_or_committed && cargo test --test telemetry_ingest completed_partial_record_is_ingested_once && cargo test --test telemetry_ingest truncation_and_same_size_replacement_reset_checkpoint && cargo test --test telemetry_ingest concurrent_cursor_change_retries_before_commit && cargo test --test telemetry_ingest interrupted_append_does_not_advance_cursor` | Covers: B-001, B-005, B-006, B-008, B-009
- [x] `SP541-T004` Owner: parsers | Dependencies: `SP541-T002` | Done when: Claude/Codex 结构化 anchors 由 tracked fixtures 独占定义，home precedence、missing root、malformed 与 IO failure 按契约分流 | Verify: `cargo test --test telemetry_ingest parser_fixtures && cargo test --test telemetry_ingest native_home_precedence && cargo test --test telemetry_ingest missing_agent_root_is_empty_but_unreadable_root_fails && cargo test --test telemetry_ingest malformed_records_are_counted_and_io_errors_fail` | Covers: B-001, B-007, B-009
- [x] `SP541-T005` Owner: telemetry ingest | Dependencies: `SP541-T003`, `SP541-T004` | Done when: fixture parser 产出 stable record key；event id 与 generation/offset 解耦；read-model matching、unmatched durable event、rejected reason count、logical source identity 与幂等批量 append 打通；unsupported shape 不使用 offset fallback | Verify: `cargo test --test telemetry_ingest deterministic_ids_use_stable_record_identity && cargo test --test telemetry_ingest source_aliases_share_event_identity && cargo test --test telemetry_ingest unmatched_events_remain_queryable && cargo test --test telemetry_ingest rejected_names_are_counted_without_raw_echo && cargo test --test telemetry_ingest repeated_ingest_is_idempotent` | Covers: B-002, B-004, B-005, B-008, B-010
- [x] `SP541-T006` Owner: cli/query | Dependencies: `SP541-T005` | Done when: `telemetry/query.rs` 提供保留全部 event kinds、hashed filters 与 redacted metrics 的 `NormalizedTelemetryDataset`，并为 stats 派生 usage-only view；report 保持所有既有 metrics 与 skillset/workspace filters 且线性聚合；ingest envelope 含 pending/reset 计数，disabled fail closed，dry-run snapshot 无副作用，所有聚合 checked overflow fail closed | Verify: `cargo test --test telemetry_ingest disabled_fails_closed_and_dry_run_is_read_only && cargo test --test telemetry_ingest json_envelope_is_stable && cargo test ingest_checkpoint_overflow_fails_without_cursor_advance && cargo test --test telemetry normalized_query_preserves_report_metrics_and_filters && cargo test --test telemetry normalized_query_overflow_fails_without_partial_output` | Covers: B-002, B-003, B-008, B-009, B-010
- [x] `SP541-T007` Owner: docs/report/export | Dependencies: `SP541-T006` | Done when: `docs/LOOM_CLI_CONTRACT.md` 记录 command、event schema v3、cursor/privacy、rejected 与 envelope 字段，telemetry report 用 normalized skill/observed-name key 分组过滤，JSONL/CSV export 对 v2/v3 保持 redacted | Verify: `cargo test --test cli_surface && cargo test --test telemetry_ingest contract_surface_matches && cargo test --test telemetry_ingest telemetry_report_groups_unmatched_by_observed_name && cargo test --test telemetry telemetry_export_v2_v3_observed_name_is_redacted` | Covers: B-001, B-002, B-010
- [x] `SP541-T008` Owner: verification | Dependencies: all prior | Done when: focused、workspace 与 repository checks 均通过 | Verify: `cargo check --workspace --all-targets --all-features && cargo test && make check` | Covers: B-001, B-002, B-003, B-004, B-005, B-006, B-007, B-008, B-009, B-010

## Handoff

- Product invariant set: `B-001..B-010`.
- Task coverage union: `B-001..B-010`.
- Spec approval is not claimed; `implx auto` authorizes drafting/implementation after the corrected spec PR passes review and merges.
