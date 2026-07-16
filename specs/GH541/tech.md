# GH541 Tech Spec - Telemetry ingestion from agent session logs

Issue: https://github.com/majiayu000/loom/issues/541
Product spec: `specs/GH541/product.md`
Route: `write_spec`
Status: implx auto draft; independent diff review pending; PR gate still required

## 1. Current Behavior

- 事件唯一写入路径是 `loom skill used` / `loom skill feedback`（`src/commands/skill_usage.rs`）经 `record_skill_invocation_telemetry` 等 emitter（`src/commands/telemetry/emitters.rs`）落到 `state/telemetry/events.jsonl`（`src/commands/telemetry/store.rs:48`）。
- store 已有 malformed-line 容忍（`TelemetryLog.malformed`）与 redaction 校验门（store.rs:336）。
- 没有任何代码读取 `~/.claude` / `~/.codex`（全仓 grep 无命中），日志解析目前只存在于 registry 内的 Python skill `skill-usage-stats`（`scripts/skill_usage_report.py`），其锚点逻辑可作为解析参考实现。

## 2. Proposed Design

1. 新增 CLI：`src/cli/telemetry.rs` 增加 `Ingest(TelemetryIngestArgs)` 子命令（`--agent`、`--since`、`--dry-run`、复用全局 `--json`）。
2. 新增模块 `src/commands/telemetry/ingest/`：
   - `mod.rs` — 命令入口、envelope 组装（scanned/ingested/skipped/unmatched/malformed 计数）。
   - `claude.rs` — 扫描 `~/.claude/projects/*/*.jsonl`，按行解析，识别 skill 调用锚点（Skill tool_use 块 / `<command-name>` 块；最终锚点集合在 T001 用真实样本核定，参考 `skill_usage_report.py` 已验证的锚点）。
   - `codex.rs` — 解析 `~/.codex/history.jsonl`（session_id/ts/text）+ 按需关联 `~/.codex/sessions/**`（mtime ≥ since 才打开，规避 13GB 全扫）。
   - `cursor.rs` — 高水位读写 `state/telemetry/ingest_cursor.json`；key 是 source path 的
     `sha256`，value 为 `{mtime, byte_offset, covered_since}`，不得落 raw path。
3. 根目录解析顺序固定为 Loom test/explicit override → agent-native home → platform home：
   `LOOM_CLAUDE_HOME` → `CLAUDE_HOME` → `~/.claude`，以及
   `LOOM_CODEX_HOME` → `CODEX_HOME` → `~/.codex`。
4. 匹配：invocation 名 → registry skill id，经现有 read model 与投影绑定观测；匹配成功写
   `skill_id`。未匹配时写 `skill_id=null` 与新增的受约束 `observed_skill_name`，既通过 redaction
   gate，又让 GH542 可从持久化事件重建 orphan；不能通过名称约束的输入进入 `rejected` 计数。
5. 幂等：deterministic event id 输入包括 `agent`、`session_id_hash`、skill/observed name、UTC
   timestamp、source hash、byte/line offset、record 内 invocation ordinal 或 tool-call id。为
   `TelemetryEventDraft` 增加受控 event-id override，store 仍统一验证 `evt_` prefix 与 privacy。
6. `covered_since` 表示 cursor 已覆盖的最早请求边界。新请求早于它时，从 source 起点重扫；
   deterministic event id 去重，避免既漏数又重复。正常增量从 byte offset 继续。
7. 在现有 workspace lock 内先 append/flush events，再原子写 cursor；任何 event 写失败都不前移
   cursor。`--dry-run` 不获取写锁、不写 event/cursor，只返回同构 plan 计数。
8. export 保持 redacted：JSONL 对 unmatched v3 event 输出 validated `observed_skill_name`；CSV 新增
   `observed_skill_name` 列，matched/v2 event 为空。export 不输出 source hash、cursor、record offset
   或 raw rejected value；v2 fixture 保持可导出。

## 3. Affected Areas

1. `src/cli/telemetry.rs`（新子命令 args）
2. `src/commands/telemetry/`（新增 `ingest/` 子模块；`mod.rs` 路由）
3. `src/commands/telemetry/model.rs` 与 `store.rs`（event schema v3、
   `observed_skill_name`、deterministic event-id override、批量幂等 append）
4. `src/commands/telemetry/export.rs`（v2/v3 JSONL/CSV redacted export）
5. `docs/LOOM_CLI_CONTRACT.md`（命令面 + event/export/envelope 字段）
6. `tests/`（fixture 日志 + ingest/export 集成测试）

## 4. Output Contract

`loom telemetry ingest --json` envelope data：

```json
{
  "agents": ["claude", "codex"],
  "since": "2026-06-01T00:00:00Z",
  "dry_run": false,
  "scanned_files": 42,
  "scanned_events": 5197,
  "ingested": 1893,
  "duplicates_skipped": 3304,
  "malformed": 2,
  "rejected": {"count": 1, "reasons": {"invalid_observed_skill_name": 1}},
  "unmatched": [{"name": "some-unknown-skill", "agent": "codex", "count": 7}],
  "cursor_advanced": true
}
```

## 5. Verification Plan

1. fixture Claude/Codex 日志 → 断言归属、计数、unmatched（集成测试，env 覆盖日志根目录）。
2. `cargo test --test telemetry_ingest deterministic_ids_include_record_ordinal`：同 timestamp 同 skill
   的两次 invocation 产生两个 event，重跑仍不增加计数。
3. `cargo test --test telemetry_ingest cursor_hashes_paths_and_backfills_older_since`：cursor 不含 home/
   workspace path，先 recent `--since` 后 older `--since` 不漏历史。
4. `cargo test --test telemetry_ingest unmatched_events_remain_queryable`：unmatched 持久化且可被
   GH542 orphan 聚合消费。
5. `cargo test --test telemetry_ingest rejected_names_are_counted_without_raw_echo`：非法 observed name
   只增加 stable reason count，envelope/event/export 均不回显 raw value。
6. `cargo test --test telemetry_ingest native_home_precedence_and_dry_run_is_read_only`：两种 native
   home 与 Loom override 顺序正确，dry-run 前后 state snapshot 相同。
7. `cargo test --test telemetry telemetry_export_v2_v3_observed_name_is_redacted`。
8. `cargo check --workspace --all-targets --all-features && cargo test`。

## 6. Rollback Plan

新增命令、event schema 字段与 `ingest_cursor.json` 均为 additive；旧 binary 不能读取 v3，故升级
前由发布/运维指引要求备份 v2 event log，回滚时恢复该备份，禁止旧 binary 以 warning 跳过 v3。
本 issue 不承诺 migration tool。cursor 可安全删除后重建；agent 源日志不受影响。

## 7. Product Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | parsers + cursor serialization | `cargo test --test telemetry_ingest source_logs_and_raw_paths_are_read_only` |
| B-002 | event model/store + unmatched/rejected envelope | `cargo test --test telemetry_ingest unmatched_events_remain_queryable && cargo test --test telemetry_ingest rejected_names_are_counted_without_raw_echo` |
| B-003 | command route + dry-run plan | `cargo test --test telemetry_ingest disabled_fails_closed_and_dry_run_is_read_only` |
| B-004 | deterministic event id builder | `cargo test --test telemetry_ingest deterministic_ids_include_record_ordinal` |
| B-005, B-006 | hashed cursor + covered_since recovery | `cargo test --test telemetry_ingest cursor_hashes_paths_and_backfills_older_since` |
| B-007 | home resolver | `cargo test --test telemetry_ingest native_home_precedence` |
| B-008 | workspace lock + event/cursor commit order | `cargo test --test telemetry_ingest interrupted_append_does_not_advance_cursor` |
| B-009 | parser/error accounting | `cargo test --test telemetry_ingest malformed_records_are_counted_and_io_errors_fail` |
| B-010 | schema/redaction validation + export | `cargo test telemetry_event_v3_is_redacted_and_v2_remains_readable && cargo test --test telemetry telemetry_export_v2_v3_observed_name_is_redacted` |

## 8. Planned Changes Manifest

```specrail-planned-changes
{"issue":541,"complete":true,"paths":["src/cli/telemetry.rs","src/commands/telemetry/mod.rs","src/commands/telemetry/ingest/mod.rs","src/commands/telemetry/ingest/claude.rs","src/commands/telemetry/ingest/codex.rs","src/commands/telemetry/ingest/cursor.rs","src/commands/telemetry/model.rs","src/commands/telemetry/store.rs","src/commands/telemetry/export.rs","docs/LOOM_CLI_CONTRACT.md","tests/telemetry.rs","tests/telemetry_ingest.rs","tests/fixtures/telemetry_ingest/"],"spec_refs":["specs/GH541/product.md#4-behavior-invariants","specs/GH541/tech.md#5-verification-plan"]}
```
