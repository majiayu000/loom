# GH541 Tech Spec - Telemetry ingestion from agent session logs

Issue: https://github.com/majiayu000/loom/issues/541
Product spec: `specs/GH541/product.md`
Route: `write_spec`
Status: implx auto architecture rewrite; independent diff review passed; PR gate still required

## 1. Current Behavior

- 事件唯一写入路径是 `loom skill used` / `loom skill feedback`（`src/commands/skill_usage.rs`）经 `record_skill_invocation_telemetry` 等 emitter（`src/commands/telemetry/emitters.rs`）落到 `state/telemetry/events.jsonl`（`src/commands/telemetry/store.rs:48`）。
- store 已有 malformed-line 容忍（`TelemetryLog.malformed`）与 redaction 校验门（store.rs:336）。
- 没有任何 tracked 代码读取 `~/.claude` / `~/.codex`（全仓 grep 无命中），仓库也没有可复现的
  既有 parser anchor 实现；实现阶段提交的脱敏 Claude/Codex fixture 集是 record shape 与 invocation
  anchor 的唯一真相，不能依赖仓外脚本或本机 skill。

## 2. Proposed Design

1. 新增 CLI：`src/cli/telemetry.rs` 增加 `Ingest(TelemetryIngestArgs)` 子命令（`--agent`、`--since`、`--dry-run`、复用全局 `--json`）。
2. 新增模块 `src/commands/telemetry/ingest/`：
   - `mod.rs` — 命令入口、envelope 组装（scanned/ingested/skipped/unmatched/malformed 计数）。
   - `claude.rs` — 扫描 `~/.claude/projects/*/*.jsonl`，按行解析，识别 fixture 固化的 Skill
     tool_use 块 / `<command-name>` skill command；fixture 必须包含正例、相似自由文本反例与未知 shape。
   - `codex.rs` — 解析 `~/.codex/history.jsonl`（session_id/ts/text）+ 按需关联 `~/.codex/sessions/**`（mtime ≥ since 才打开，规避 13GB 全扫）。
   - `cursor.rs` — 高水位读写 `state/telemetry/ingest_cursor.json`。`LogicalSourceKey =
     hash(agent + canonical_source_identity)`；value 为 `SourceCheckpoint {schema_version,
     generation_token, committed_offset, boundary_hash, covered_since}`，不得落 raw path。
     `committed_offset` 只越过 newline-terminated records。resume 前校验 file length、generation token 与
     committed boundary 前的 bounded hash；truncate、replacement、same-size rewrite 或 continuity
     mismatch 统一 reset 到 0，并用 `min(previous covered_since, requested since)` 重扫。
   - 每个 agent scanner 将不存在的日志根或空 glob 作为 `scanned_files=0`；路径存在但不可读时
     返回 error。`--agent all` 汇总 per-agent 结果，不能因未安装另一 agent 而丢弃可用日志。
3. 根目录解析顺序固定为 Loom test/explicit override → agent-native home → platform home：
   `LOOM_CLAUDE_HOME` → `CLAUDE_HOME` → `~/.claude`，以及
   `LOOM_CODEX_HOME` → `CODEX_HOME` → `~/.codex`。
4. 新增 `src/commands/telemetry/query.rs` 作为唯一 normalized read boundary，输出完整 redacted
   `NormalizedTelemetryDataset {telemetry_enabled, persisted_event_count, malformed_event_count, rows}`。
   `NormalizedTelemetryRow` 保留现有全部 event kind（activation/deactivation/invocation/eval/safety/
   error/feedback）、`skill_ref = Registered(id)|Observed(name)|Unattributed`、`skillset_id`、
   `agent_ref = Known(agent)|Unknown`、workspace/session/task hashes、timestamp，以及完整 redacted metrics
   （tokens、commands、duration、success、baseline delta、feedback、safety/dependency findings、failure
   category）；禁止暴露 prompt、code、raw path 或 unhashed session/workspace。`telemetry report` 直接消费
   full rows，保留 skill/skillset/agent/workspace/since filters 与所有既有指标；GH542 `skill stats` 从 full
   rows 派生仅含 invocation/error 的 `UsageRow` view。这样统一 matched/unmatched/agent filter/malformed
   语义，并把 report 聚合从当前逐 event 重扫 entry list 的 O(events²) 改为 O(events)。匹配：invocation
   名 → registry skill id，经 current
   `RegistrySnapshot` read model；匹配成功写
   `skill_id`。未匹配时写 `skill_id=null` 与新增的受约束 `observed_skill_name`，既通过 redaction
   gate，又让 GH542 可从持久化事件重建 orphan；不能通过名称约束的输入进入 `rejected` 计数。
5. parser 对每个受支持 fixture shape 产出 `ImportedRecord {stable_record_key, record_end_offset,
   invocations}`。deterministic event id 输入包括 `agent`、`session_id_hash`、skill/observed name、UTC
   timestamp、logical source key、stable record key、record 内 invocation ordinal 或 tool-call id；
   generation token 与 byte/line offset 只服务 continuity，禁止进入 event id。无法从 unknown shape
   提取稳定 record key 时显式 rejected，不得以 offset fallback。为
   `TelemetryEventDraft` 增加受控 event-id override，store 仍统一验证 `evt_` prefix 与 privacy。
6. `covered_since` 表示 cursor 已覆盖的最早请求边界。新请求早于它或 continuity reset 时，从 source
   起点重扫；deterministic event id 去重。正常增量从 `committed_offset` 继续；unterminated tail 返回
   `pending_partial` 并把 checkpoint 留在 fragment 之前，补全后只 ingest 一次。
7. scanner 在 workspace lock 外生成带 expected checkpoint 的 immutable plan。commit 获取 workspace
   lock 后重读 cursor；expected/current 不同则释放 lock 并重试扫描。匹配时在同一锁内执行 dedupe、
   append/flush events、原子写 checkpoint，避免调用会再次获取 workspace lock 的 public append API。
   所有 counter/offset 加法使用 checked arithmetic，overflow 返回含字段名的结构化 error，且不得输出
   partial result 或前移 cursor。`--dry-run` 不获取写锁、不写 event/cursor，只返回同构 plan 计数。
8. export 保持 redacted：JSONL 对 unmatched v3 event 输出 validated `observed_skill_name`；CSV 新增
   `observed_skill_name` 列，matched/v2 event 为空。export 不输出 source hash、cursor、record offset
   或 raw rejected value；v2 fixture 保持可导出。
9. `loom telemetry report` 通过 `telemetry/query.rs` 的 full normalized row 在一次遍历中聚合；分组、
   selector filter 与表格/JSON 输出使用同一 key，使 unmatched v3 event 可按受约束名称查询，
   `agent=null` 只进入未指定 agent 的查询且呈现为 agentless/null，不生成会与真实 agent 冲突的
   `"unknown"` key。迁移不得删除或改写既有 activation/deactivation/eval/safety/feedback、cost/value/
   risk/recommendation metrics，也不得破坏 skillset/workspace filters。

## 3. Affected Areas

1. `src/cli/telemetry.rs`（新子命令 args）
2. `src/commands/telemetry/`（新增 `ingest/` 子模块与 `query.rs` normalized read boundary；`mod.rs` 路由）
3. `src/commands/telemetry/model.rs` 与 `store.rs`（event schema v3、
   `observed_skill_name`、deterministic event-id override、批量幂等 append）
4. `src/commands/telemetry/export.rs`（v2/v3 JSONL/CSV redacted export）
5. `src/commands/telemetry/mod.rs`（report 对 matched/unmatched 使用统一分组/filter key）
6. `docs/LOOM_CLI_CONTRACT.md`（命令面 + event/report/export/envelope 字段）
7. `tests/`（fixture 日志 + ingest/report/export 集成测试）

## 4. Output Contract

`loom telemetry ingest --json` envelope data：

```json
{
  "agents": ["claude", "codex"],
  "by_agent": {"claude": {"scanned_files": 0}, "codex": {"scanned_files": 42}},
  "since": "2026-06-01T00:00:00Z",
  "dry_run": false,
  "scanned_files": 42,
  "scanned_events": 5197,
  "ingested": 1893,
  "duplicates_skipped": 3304,
  "malformed": 2,
  "pending_partial": 1,
  "sources_reset": {"count": 1, "reasons": {"boundary_mismatch": 1}},
  "rejected": {"count": 1, "reasons": {"invalid_observed_skill_name": 1}},
  "unmatched": [{"name": "some-unknown-skill", "agent": "codex", "count": 7}],
  "cursor_advanced": true
}
```

## 5. Verification Plan

1. fixture Claude/Codex 日志 → 断言归属、计数、unmatched（集成测试，env 覆盖日志根目录）。
   fixture 是 parser anchor 的唯一真相，并覆盖结构化正例、自由文本反例与未知 shape。
2. `cargo test --test telemetry_ingest deterministic_ids_use_stable_record_identity`：同 timestamp 同 skill
   的两次 invocation 产生两个 event；rotation/copy 后 stable record 不重复，generation/offset 变化不改 id。
3. `cargo test --test telemetry_ingest cursor_hashes_paths_and_backfills_older_since`：cursor 不含 home/
   workspace path，先 recent `--since` 后 older `--since` 不漏历史；同一 source 经 symlink/override/
   默认根访问时 logical source key 保持一致。
4. `cargo test --test telemetry_ingest unmatched_events_remain_queryable`：unmatched 持久化且可被
   GH542 orphan 聚合消费。
5. `cargo test --test telemetry_ingest rejected_names_are_counted_without_raw_echo`：非法 observed name
   只增加 stable reason count，envelope/event/export 均不回显 raw value。
6. `cargo test --test telemetry_ingest native_home_precedence_and_dry_run_is_read_only`：两种 native
   home 与 Loom override 顺序正确，dry-run 前后 state snapshot 相同。
7. `cargo test --test telemetry telemetry_export_v2_v3_observed_name_is_redacted`。
8. `cargo test --test telemetry_ingest telemetry_report_groups_unmatched_by_observed_name`。
9. `cargo test --test telemetry_ingest missing_agent_root_is_empty_but_unreadable_root_fails`。
10. `cargo test --test telemetry_ingest trailing_partial_record_is_not_malformed_or_committed && cargo test --test telemetry_ingest completed_partial_record_is_ingested_once`。
11. `cargo test --test telemetry_ingest truncation_and_same_size_replacement_reset_checkpoint && cargo test --test telemetry_ingest concurrent_cursor_change_retries_before_commit`。
12. `cargo test --test telemetry normalized_query_preserves_report_metrics_and_filters`：覆盖非 usage event、
    cost/value/risk/feedback metrics 与 skillset/workspace filters。
13. `cargo test --test telemetry normalized_query_overflow_fails_without_partial_output`：以两个合法的大值
    persisted metric fixture 触发 checked sum overflow，不返回 partial report。
14. `cargo check --workspace --all-targets --all-features && cargo test`。

## 6. Rollback Plan

新增命令、event schema 字段与 `ingest_cursor.json` 均为 additive；旧 binary 不能读取 v3，故升级
前由发布/运维指引要求备份 v2 event log，回滚时恢复该备份，禁止旧 binary 以 warning 跳过 v3。
本 issue 不承诺 migration tool。cursor 可安全删除后重建；agent 源日志不受影响。

## 7. Product Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | parsers + cursor serialization | `cargo test --test telemetry_ingest source_logs_and_raw_paths_are_read_only` |
| B-002 | event model/store + unmatched/rejected envelope + report normalized key | `cargo test --test telemetry_ingest unmatched_events_remain_queryable && cargo test --test telemetry_ingest telemetry_report_groups_unmatched_by_observed_name && cargo test --test telemetry_ingest rejected_names_are_counted_without_raw_echo` |
| B-003 | command route + dry-run plan | `cargo test --test telemetry_ingest disabled_fails_closed_and_dry_run_is_read_only` |
| B-004 | logical source/stable record identity + deterministic event id builder | `cargo test --test telemetry_ingest deterministic_ids_use_stable_record_identity && cargo test --test telemetry_ingest source_aliases_share_event_identity` |
| B-005, B-006 | continuity-checked cursor + partial/reset/backfill recovery | `cargo test --test telemetry_ingest cursor_hashes_paths_and_backfills_older_since && cargo test --test telemetry_ingest trailing_partial_record_is_not_malformed_or_committed && cargo test --test telemetry_ingest completed_partial_record_is_ingested_once && cargo test --test telemetry_ingest truncation_and_same_size_replacement_reset_checkpoint` |
| B-007 | home resolver | `cargo test --test telemetry_ingest native_home_precedence` |
| B-008 | scan-plan CAS + workspace-lock event/cursor commit order | `cargo test --test telemetry_ingest interrupted_append_does_not_advance_cursor && cargo test --test telemetry_ingest concurrent_cursor_change_retries_before_commit && cargo test ingest_checkpoint_overflow_fails_without_cursor_advance` |
| B-009 | parser/partial/reset/missing-root/error accounting | `cargo test --test telemetry_ingest malformed_records_are_counted_and_io_errors_fail && cargo test --test telemetry_ingest missing_agent_root_is_empty_but_unreadable_root_fails && cargo test --test telemetry_ingest trailing_partial_record_is_not_malformed_or_committed` |
| B-010 | schema/redaction validation + full report/query/export compatibility | `cargo test telemetry_event_v3_is_redacted_and_v2_remains_readable && cargo test --test telemetry normalized_query_preserves_report_metrics_and_filters && cargo test --test telemetry normalized_query_overflow_fails_without_partial_output && cargo test --test telemetry telemetry_export_v2_v3_observed_name_is_redacted` |

## 8. Planned Changes Manifest

```specrail-planned-changes
{"issue":541,"complete":true,"paths":["src/cli/telemetry.rs","src/commands/telemetry/mod.rs","src/commands/telemetry/query.rs","src/commands/telemetry/ingest/mod.rs","src/commands/telemetry/ingest/claude.rs","src/commands/telemetry/ingest/codex.rs","src/commands/telemetry/ingest/cursor.rs","src/commands/telemetry/model.rs","src/commands/telemetry/store.rs","src/commands/telemetry/export.rs","docs/LOOM_CLI_CONTRACT.md","tests/telemetry.rs","tests/telemetry_ingest.rs","tests/fixtures/telemetry_ingest/"],"spec_refs":["specs/GH541/product.md#4-behavior-invariants","specs/GH541/tech.md#5-verification-plan"]}
```
