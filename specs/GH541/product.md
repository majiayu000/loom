# GH541 Product Spec - Telemetry ingestion from agent session logs

Issue: https://github.com/majiayu000/loom/issues/541
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

telemetry 事件存储（#385/#496）只在调用方显式执行 `loom skill used` 时才有数据，实际没有任何 agent 集成在写入，`loom telemetry report` 面对空数据集。而真实使用证据早已存在于 agent 自己的会话日志（`~/.claude/projects/**/*.jsonl`、`~/.codex/history.jsonl` + sessions），目前靠 `skill-usage-stats` Python skill 每次用 LLM 重新解析回答同样的确定性问题（45 天内 ~489 条治理类提问）。

## 2. Goals

1. 新增 `loom telemetry ingest --agent claude|codex|all [--since <date>] [--dry-run] [--json]`，从 agent 会话日志回填 `skill.invocation` 事件。
2. 幂等：同一批日志重复 ingest 不产生重复事件。
3. 增量：记录 per-source 高水位，调度器（looper cron）低成本重复执行。
4. 隐私立场不变：只读日志，不存 prompt/transcript 原文，workspace/session 按现有 schema 哈希。

## 3. Non-Goals

1. 不做 agent 实时 hook 集成（后续 issue）。
2. 不新增报表 UI；`loom telemetry report` 自然变为非空。
3. 不新增事件类型。
4. 不解析 Cursor/Windsurf 等其他 agent（本期只做 claude/codex 两个有据可查的日志格式）。

## 4. Behavior Invariants

1. ingest 是只读采集：绝不修改、移动、删除 agent 侧日志文件。
2. 无法匹配到 registry skill 的调用名出现在命令 envelope 的 `unmatched` 列表中，不静默丢弃。
3. telemetry 关闭（disabled）时 ingest 拒绝执行并给出 next_action（`loom telemetry enable`）。
4. `--dry-run` 不写任何状态（不写事件、不推高水位）。
5. 事件写入沿用现有 redaction 校验路径（store.rs 的 "must be redacted before persistence" 门）。

## 5. Acceptance Criteria

1. fixture 的 Claude + Codex 日志 ingest 后，事件归属到正确的 registry skill；再次 ingest 事件数不变（幂等测试）。
2. `loom telemetry report --since <date>` 反映已 ingest 的事件。
3. `--dry-run --json` 输出将要 ingest 的统计（per-agent、per-skill、unmatched），且状态目录无变化。
4. 高水位生效：追加新日志行后再 ingest 只处理新增部分（envelope 中报告 scanned/skipped/ingested 计数）。
5. 日志中存在损坏行时不中断，计入 `malformed` 计数继续处理（与现有 TelemetryLog.malformed 行为一致）。

## 6. Edge Cases

1. Claude 项目目录含非会话 jsonl（如 memory/工具产物）——按解析失败跳过而非报错。
2. Codex `sessions/` 体量大（观测值 13GB）——必须支持只扫 `history.jsonl` 或按 mtime/since 剪枝，避免全量扫描。
3. 同一 skill 名在多个 agent 目录都有投影绑定——按事件的 agent 字段区分，不合并。
4. skill 在 registry 中已退役但日志中有历史调用——归为 unmatched 还是历史归属？（Open Question 2）
5. 时钟/时区：日志时间戳统一转 UTC 存储。

## 7. Open Questions

1. 高水位存储位置：`state/telemetry/ingest_cursor.json` 还是并入 TelemetryConfig？（倾向独立文件，避免 config schema 演进）
2. 已退役/已删除 skill 的历史调用如何归属：建议按 ingest 时点的 registry 读模型匹配，匹配不到即 unmatched（保守，不做历史考古）。
3. Claude transcript 中 skill 调用的识别锚点（Skill tool call vs command-name block）需要在 tech spec 中用真实样本确认一种最稳定的锚点集合。
