# GH541 Tasks: Telemetry ingestion from agent session logs

Issue: https://github.com/majiayu000/loom/issues/541
Product spec: `specs/GH541/product.md`
Tech spec: `specs/GH541/tech.md`
Status: implx auto decisions recorded; implementation begins only after this spec PR merges

## Order

Reviewed anchor/schema decisions -> event schema + cursor -> parsers -> matching/dedup -> CLI/envelope -> contract docs/tests.

## Tasks

- [ ] `SP541-T001` Owner: spec/review | Dependencies: none | Done when: invocation anchors、unmatched/rejected persistence、export、home precedence、cursor privacy/backfill 与 retired-skill policy 均记录且独立 diff review 无 blocker | Verify: independent PR #543 reviewer confirms all eight review findings are mapped | Covers: B-001, B-002, B-004, B-005, B-006, B-007, B-010
- [ ] `SP541-T002` Owner: telemetry model | Dependencies: merged `SP541-T001` spec | Done when: event schema v3 支持 validated `observed_skill_name` 与 deterministic event-id override，同时读取 v2 events | Verify: `cargo test telemetry_event_v3_is_redacted_and_v2_remains_readable` | Covers: B-002, B-004, B-010
- [ ] `SP541-T003` Owner: telemetry ingest | Dependencies: `SP541-T002` | Done when: hashed cursor、`covered_since` older-window rescan、workspace-lock commit ordering 与 failure recovery 落地 | Verify: `cargo test --test telemetry_ingest cursor_hashes_paths_and_backfills_older_since && cargo test --test telemetry_ingest interrupted_append_does_not_advance_cursor` | Covers: B-001, B-005, B-006, B-008
- [ ] `SP541-T004` Owner: parsers | Dependencies: `SP541-T002` | Done when: Claude/Codex 结构化 anchors 与 home precedence 解析 fixture，malformed 与 IO failure 按契约分流 | Verify: `cargo test --test telemetry_ingest parser_fixtures && cargo test --test telemetry_ingest native_home_precedence && cargo test --test telemetry_ingest malformed_records_are_counted_and_io_errors_fail` | Covers: B-001, B-007, B-009
- [ ] `SP541-T005` Owner: telemetry ingest | Dependencies: `SP541-T003`, `SP541-T004` | Done when: read-model matching、unmatched durable event、rejected reason count、record-ordinal collision protection 与幂等批量 append 打通 | Verify: `cargo test --test telemetry_ingest deterministic_ids_include_record_ordinal && cargo test --test telemetry_ingest unmatched_events_remain_queryable && cargo test --test telemetry_ingest rejected_names_are_counted_without_raw_echo && cargo test --test telemetry_ingest repeated_ingest_is_idempotent` | Covers: B-002, B-004, B-005, B-008, B-010
- [ ] `SP541-T006` Owner: cli | Dependencies: `SP541-T005` | Done when: `loom telemetry ingest` args/envelope 可用，disabled fail closed，dry-run snapshot 无副作用 | Verify: `cargo test --test telemetry_ingest disabled_fails_closed_and_dry_run_is_read_only && cargo test --test telemetry_ingest json_envelope_is_stable` | Covers: B-003, B-009
- [ ] `SP541-T007` Owner: docs/export | Dependencies: `SP541-T006` | Done when: `docs/LOOM_CLI_CONTRACT.md` 记录 command、event schema v3、cursor/privacy、rejected 与 envelope 字段，JSONL/CSV export 对 v2/v3 保持 redacted | Verify: `cargo test --test cli_surface && cargo test --test telemetry_ingest contract_surface_matches && cargo test --test telemetry telemetry_export_v2_v3_observed_name_is_redacted` | Covers: B-001, B-002, B-010
- [ ] `SP541-T008` Owner: verification | Dependencies: all prior | Done when: focused、workspace 与 repository checks 均通过 | Verify: `cargo check --workspace --all-targets --all-features && cargo test && make check` | Covers: B-001, B-002, B-003, B-004, B-005, B-006, B-007, B-008, B-009, B-010

## Handoff

- Product invariant set: `B-001..B-010`.
- Task coverage union: `B-001..B-010`.
- Spec approval is not claimed; `implx auto` authorizes drafting/implementation after the corrected spec PR passes review and merges.
