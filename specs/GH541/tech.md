# GH541 Tech Spec - Telemetry ingestion from agent session logs

Issue: https://github.com/majiayu000/loom/issues/541
Product spec: `specs/GH541/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

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
   - `cursor.rs` — 高水位读写 `state/telemetry/ingest_cursor.json`（per-source: 文件路径 → {mtime, line_count/byte_offset}）。
3. 匹配：invocation 名 → registry skill id，经现有 read model（`build_skill_read_model`）与投影绑定观测；不区分大小写连字符归一后仍无匹配 → unmatched。
4. 幂等：event_id 取 `sha256(agent, session_hash, skill, timestamp)` 前缀派生（determinstic id），写前查重（events.jsonl 读入现成 `TelemetryLog`）；配合高水位双保险。
5. 事件构造走现有 `TelemetryEventDraft::new(TelemetryEventType::SkillInvocation)` + emitter 持久化路径，天然过 redaction 门。
6. agent 日志根目录可用环境变量覆盖（`LOOM_CLAUDE_HOME`/`LOOM_CODEX_HOME`），fixture 测试即指向 `tests/fixtures/ingest/`。

## 3. Affected Areas

1. `src/cli/telemetry.rs`（新子命令 args）
2. `src/commands/telemetry/`（新增 `ingest/` 子模块；`mod.rs` 路由）
3. `src/commands/telemetry/model.rs`（若 deterministic event_id 需要新构造函数）
4. `docs/LOOM_CLI_CONTRACT.md`（命令面 + envelope 字段）
5. `tests/`（fixture 日志 + 集成测试）

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
  "unmatched": [{"name": "some-unknown-skill", "agent": "codex", "count": 7}],
  "cursor_advanced": true
}
```

## 5. Verification Plan

1. fixture Claude/Codex 日志 → 断言归属、计数、unmatched（集成测试，env 覆盖日志根目录）。
2. 幂等测试：连续两次 ingest，第二次 `ingested=0`、`duplicates_skipped>0` 或 cursor 短路。
3. `--dry-run` 前后对 `state/telemetry/` 目录做快照比对，断言无变化。
4. 追加日志行后增量 ingest 只处理新行。
5. `cargo check && cargo test`。

## 6. Rollback Plan

新增命令与新增状态文件（`ingest_cursor.json`）均为增量面；revert 后 events.jsonl 中已 ingest 的事件可用现有 `loom telemetry purge --before/--dry-run` 清除（deterministic event_id 让按来源清除可识别）。

## 7. Product Mapping

1. Invariant 1/4（只读、dry-run 无副作用）→ 快照比对测试。
2. Invariant 2（unmatched 不静默丢弃）→ fixture 含未注册 skill 名的断言。
3. Goal 2（幂等）→ 幂等集成测试。
4. Goal 3（增量）→ 高水位测试。
5. Invariant 5（redaction 门）→ 事件走现有 emitter 路径，由 store.rs:336 现有测试覆盖。
