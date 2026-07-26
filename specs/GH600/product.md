# GH600 Product Spec - 安全识别当前 Codex Skill 入口读取

Issue: https://github.com/majiayu000/loom/issues/600
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`
Complexity: medium

## 1. 问题

Codex 当前把 Skill 入口读取记录为 `response_item.function_call`。现有 importer
仅在整段文本中分别搜索 read verb 与 `SKILL.md` 路径，导致 `echo` 自由文本、
shell chain 中的删除 operand、伪造 trusted-root 前缀和损坏的 tool schema
可能被误记或静默忽略。结果既会制造错误 usage，也会让损坏输入看起来像正常的
“无调用”。

## 2. 目标

1. 准确识别 `exec_command` 与 `exec` 中真实读取受信任 Skill 入口的调用。
2. 把 read verb、shell segment 与该 segment 的 path operands 绑定，消除跨
   segment/free-text 误归因。
3. 对 trusted roots 和已识别 tool schema 采用 fail-closed 语义。
4. 保持 telemetry 的本地 opt-in、去重、隐私与 legacy Codex 兼容契约。

## 3. 非目标

1. 不执行或模拟 session 中的命令。
2. 不实现通用 POSIX shell、PowerShell 或 JavaScript parser。
3. 不接受任意 workspace 内的 `.codex`、自定义 Skill root 或不存在的新 tool
   schema。
4. 不删除 legacy `$skill` marker + structured `<skill>` injection 识别路径。
5. 不扩大 telemetry 默认启用范围，不持久化新的 raw source 字段。

## 4. 行为不变量

1. **B-001** read verb 只能授权同一 simple command segment 中的 path
   operands。quoted token 与 `;`、换行、`&&`、`||`、pipeline 等 control
   operator 必须按 segment 处理；`echo`/free-text 中出现 read verb、另一个
   segment 的 `rm` operand 或无法保守分词的 command 均不得产生调用。token
   必须保留 quote/escape provenance：single-quoted 或 escaped `$HOME` 是
   literal，不得展开；double-quoted `$HOME` 只按 shell variable 语义展开。
2. **B-002** 支持的 read verb 是闭集
   `{cat, sed, head, tail, less, more, bat, get-content}`，匹配忽略 ASCII
   大小写但要求完整 command token。`exec_command` 读取其 JSON `cmd`；
   `exec` 只读取可保守提取的 nested `exec_command` 的 `cmd`，其他函数或
   仅含相似文本的源码不授权路径。每个 verb 必须使用自身的闭集 option
   grammar 识别真实 file operands；sed program、option value、未知/歧义
   option 和 help/version short-circuit 均不得成为 Skill path，`--` 之后才
   按 literal operand 处理。
3. **B-003** trusted roots 必须从进程实际 home 构造，并以 normalized、
   component-aware `Path` 校验：
   `.codex/skills`、`.agents/skills`、`.claude/skills`、
   `.loom-registry/skills`、`.vibeguard/installed/skills`，以及
   `.codex/plugins/cache/**/skills/<skill>/SKILL.md`。伪造字符串前缀、
   `..`、home 外路径、无关目录中嵌套的 `.codex` 和非直接 Skill
   entrypoint 必须拒绝。
4. **B-004** `exec_command`/`exec` 一旦被识别，缺失或错误类型的
   `arguments`、损坏 JSON、缺失或错误类型的 `cmd`、以及无法解析的 nested
   command schema 必须进入稳定 `rejected.reasons`，不得通过 `.ok()` 或
   fallback 静默变为 `Ignored`；reason 不得包含 raw 输入。
5. **B-005** 有稳定 function-call identity、session identity 和 timestamp
   的合法读取产生既有 `skill.invocation`；同一 turn 同名 Skill 至多一次，
   新 turn 可再次计数，event identity 不依赖 raw command/path。
6. **B-006** durable telemetry 只持久化 validated Skill name、既有 hashed
   workspace/session identity 与 redacted event 字段；raw command、raw
   path、prompt、tool arguments、source content 及 rejected raw value 均不得
   出现在 event、cursor 或 JSON envelope。
7. **B-007** legacy `$skill` marker + structured `<skill>` injection 继续按
   GH541 契约工作；本变更不得改变 telemetry opt-in、cursor continuity、
   unmatched name validation 或 existing report 行为。
8. **B-008** schema 合法但不含受支持 read、路径不受信任或与 Skill 读取
   无关的 function call 仍是 `Ignored`，不得伪造 rejection 或 invocation；
   `HOME`/`USERPROFILE` 均缺失时所有候选路径保持 untrusted/`Ignored`，不得
   产生 `missing_home` rejection；正负例必须同时由 unit 与 end-to-end
   fixture 固化。

## 5. 验收标准

1. `exec_command` 与 `exec` 对实际 home 下受信任 roots 的读取均产生调用，
   同 turn 去重。
2. chain/pipeline/quoting fixture 证明只有 read segment 自身的真实 file
   operand 被计数，single-quoted/escaped home 与 option/program values 不计数。
3. prefix spoof、`..`、无关 nested `.codex` 与 plugin-cache 伪路径均不计数。
4. malformed JSON、arguments 类型漂移、缺失/错误 `cmd` 均按稳定 reason
   进入 `rejected`。
5. legacy structured injection 现有测试保持通过。
6. end-to-end events/cursor/envelope 不包含 fixture 的 raw command/path/prompt。
7. focused tests、`cargo check --locked` 与指定 telemetry ingest 集成测试通过。
8. 八个 read verb 均有正例；缺失 home 的 E2E 返回成功且不计 tool read/rejection。

## 6. 边界清单

| Boundary | Verdict |
| --- | --- |
| Empty / missing input | covered: B-004, B-008 |
| Error and failure paths | covered: B-001, B-003, B-004 |
| Authorization / permission | covered: B-002, B-003 |
| Concurrency / race / ordering | N/A：parser 对单条顺序 session stream 纯计算；跨记录 ordering/turn reset 由 B-005 与既有 GH541 cursor 契约覆盖 |
| Retry / repetition / idempotency | covered: B-005, B-007 |
| Illegal state transitions | covered: B-004, B-005 |
| Compatibility / migration | covered: B-007 |
| Degradation / fallback | covered: B-001, B-004, B-008 |
| Evidence and audit integrity | covered: B-003, B-004, B-006 |
| Cancellation / interruption | N/A：单 record parser 无长运行或 partial mutation；durable commit 仍由 GH541 compare-and-commit 负责 |
