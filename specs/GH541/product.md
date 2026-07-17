# GH541 Product Spec - Telemetry ingestion from agent session logs

Issue: https://github.com/majiayu000/loom/issues/541
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`
Complexity: medium

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

1. **B-001** ingest 只读 agent 日志；不得修改、移动或删除源文件，也不得保存 prompt、代码、
   raw transcript path、workspace path 或未哈希 session identity。
2. **B-002** matched invocation 持久化 registry `skill_id`；unmatched invocation 也必须持久化为
   `skill.invocation`，使用 `skill_id=null` 与受长度/字符约束的 `observed_skill_name`，并同时进入
   envelope 的 `unmatched` 聚合，供后续 orphan 统计；`loom telemetry report` 必须按
   `observed_skill_name` 分组/过滤这些事件而不是只计入总数或标成无法选择的 `unknown`；非法名称
   必须计入显式 rejected 计数而非静默丢弃。
3. **B-003** telemetry disabled 时 ingest fail closed 并返回 `loom telemetry enable` next action；
   `--dry-run` 仍可读取并报告候选项，但不得写 event 或 cursor。
4. **B-004** source continuity、record identity 与 event identity 必须分离。event id 必须由 agent、
   session hash、skill/observed name、timestamp、canonical logical-source identity hash、parser 产出的
   stable record key 与 invocation ordinal/tool-call id 共同确定；generation token、byte offset 与
   raw path 不得进入 event identity。已知 fixture shape 必须定义 stable record key；无法提供稳定身份的
   unknown shape 必须显式 rejected，禁止回退到 offset。源文件经 symlink/override/default root、rotation
   或 copy-truncate 后，同一逻辑 record 仍产生同一 event id；同 timestamp 同 skill 的多次调用不得碰撞。
5. **B-005** cursor 只保存 logical source key 与 `{schema_version, generation_token,
   committed_offset, boundary_hash, covered_since}`，不保存 raw path。`committed_offset` 永远指向最后一条
   newline-terminated record 之后；unterminated tail 是 `pending_partial`，既不是 malformed，也不得前移
   cursor。恢复扫描前必须校验长度、generation token 与 committed boundary 前的 bounded hash；truncate、
   replacement、same-size rewrite 或 continuity mismatch 必须从 0 reset/rescan，并依赖 B-004 去重。
6. **B-006** `--since` 只限制本次候选窗口。cursor 必须记录 `covered_since`；后续请求更早窗口时
   从 source 起点重扫并依赖 deterministic event id 去重，不得因旧 cursor 永久跳过历史。
7. **B-007** 日志根优先级固定为 `LOOM_CLAUDE_HOME`/`LOOM_CODEX_HOME` 显式 override →
   agent-native `CLAUDE_HOME`/`CODEX_HOME` → platform home 下的默认目录。
8. **B-008** 文件发现与扫描在 workspace lock 外执行；提交时获取 workspace lock，重读 cursor 并与扫描
   使用的 expected checkpoint 比较，并在 append 前复验 source snapshot；发生 cursor 或 source 并发变化
   则丢弃该 plan 并重试。compare-and-commit 内按
   dedupe → append/flush events → 原子写 cursor 的顺序执行，且不得通过现有会再次加锁的 public append
   路径造成 nested lock。取消、崩溃或部分写失败不得让 cursor 越过未持久化 invocation。
9. **B-009** newline-terminated malformed/oversized record、非 session JSONL 与未知 record shape 必须
   以有界内存逐条计数并继续扫描其他记录；unterminated tail 只计 `pending_partial`，下一次补全后恰好
   ingest 一次。发生
   continuity reset 时必须按 stable reason 计 `sources_reset`，不得回显 raw path。某个已选 agent 的日志根
   不存在或没有匹配文件时按该 agent `scanned_files=0` 处理，以便 `--agent all`
   仍可采集其他 agent；已存在的源目录不可读、cursor schema 不兼容或 event persistence 失败则整次
   命令返回 error，不得伪装完整成功。
10. **B-010** 事件写入必须通过现有 telemetry redaction/validation gate；新增字段与 deterministic
    event id 属于显式 schema 版本变更，旧 event 仍可读取。

## 5. Acceptance Criteria

1. fixture 的 Claude + Codex 日志 ingest 后，事件归属到正确的 registry skill；再次 ingest 事件数不变（幂等测试）。
2. `loom telemetry report --since <date>` 反映已 ingest 的事件。
3. `--dry-run --json` 输出将要 ingest 的统计（per-agent、per-skill、unmatched），且状态目录无变化。
4. 高水位生效：追加新日志行后再 ingest 只处理新增部分（envelope 中报告 scanned/skipped/ingested 计数）。
5. 日志中存在损坏行时不中断，计入 `malformed` 计数继续处理（与现有 TelemetryLog.malformed 行为一致）。
6. trailing partial record 不计 malformed 且不前移 cursor；补全换行后只 ingest 一次。
7. truncate/regrow、rotation/replacement 与 same-size rewrite 均触发 continuity reset，从 0 重扫且不重复 event。
8. 两个并发 ingest 使用同一 expected checkpoint 时只有一个提交；另一个检测变化并重试，不丢失或重复事件。

## 6. Edge Cases

1. Claude 项目目录含非会话 jsonl（如 memory/工具产物）——按解析失败跳过而非报错。
2. Codex `sessions/` 体量大（观测值 13GB）——必须支持只扫 `history.jsonl` 或按 mtime/since 剪枝，避免全量扫描。
3. 同一 skill 名在多个 agent 目录都有投影绑定——按事件的 agent 字段区分，不合并。
4. skill 在 registry 中已退役但日志中有历史调用——按 ingest 时的当前读模型归为 unmatched，
   并持久化 `observed_skill_name`，不做历史 registry 考古。
5. 时钟/时区：日志时间戳统一转 UTC 存储。
6. source 在 cursor 后被截断、原位替换或同尺寸改写——以 generation/boundary continuity 失败处理，
   reset 后把旧 generation 的 coverage 作废，以本次 `requested since` 作为新 coverage 并从头重扫。
7. 进程在末尾 record 尚未写完时运行——保留该 fragment 到下一轮，不计 malformed/rejected。

## 7. Resolved Decisions

1. cursor 使用独立的 `state/telemetry/ingest_cursor.json`，避免 telemetry enablement config 与采集进度耦合。
2. 已退役/已删除 skill 按 ingest 时当前读模型归为 unmatched。
3. Claude 接受原生 `sessionId` 记录中的结构化 Skill tool call 与明确 `<command-name>` skill command；
   Codex 接受 rollout `session_meta`/`turn_context` 上下文及 `response_item` 中 Codex 注入的结构化
   `<skill><name>...</name>...</skill>` message，且该注入必须与同一 turn 中此前的显式 `$skill-name`
   marker 对应。Claude 内置 slash command 与孤立/用户粘贴的 Codex `<skill>` XML 均不算 invocation；
   自由文本提及不算 invocation；确切 record fixtures 在实现中固化。

## 8. Boundary Checklist

| Boundary | Verdict |
| --- | --- |
| Empty / missing input | covered: B-002, B-009 |
| Error and failure paths | covered: B-003, B-009 |
| Authorization / permission | covered: B-001, B-003 |
| Concurrency / race / ordering | covered: B-005, B-008 |
| Retry / repetition / idempotency | covered: B-004, B-005, B-006, B-008 |
| Illegal state transitions | covered: B-006, B-008 |
| Compatibility / migration | covered: B-010 |
| Degradation / fallback | covered: B-002, B-009 |
| Evidence and audit integrity | covered: B-001, B-002, B-004, B-005 |
| Cancellation / interruption | covered: B-008 |
