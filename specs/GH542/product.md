# GH542 Product Spec - skill stats lifecycle governance report

Issue: https://github.com/majiayu000/loom/issues/542
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`
Depends on: GH541 (telemetry ingestion)
Complexity: medium

## 1. Problem

registry 无法回答生命周期问题："哪些 skill 是僵尸（有绑定但没人用）"、"哪些只在单一 runtime 被用"、"用量排行"。这些问题需要 join registry 读模型 × 投影绑定 × usage 事件，目前 `loom doctor`（完整性/投影）和 `loom telemetry report`（裸聚合）各管一段，都不做 join。运营者 45 天内为此发了 ~489 条 LLM 治理提问，由 `skill-usage-stats` / `skill-ecosystem-doctor` Python skill 每次重新推导——确定性查询应该是一条 CLI 命令。

## 2. Goals

1. 新增 `loom skill stats [--since <date>] [--agent <agent>] [--zombie-days <n>] [--json]`。
2. 每个 registry skill 输出：per-agent 调用数、last-used、错误率/失败类别（有 `skill.error` 时）。
3. 生命周期分类：zombie（有绑定、`--zombie-days` 内零调用，默认 30 天）、orphan（有事件但无对应 registry skill）、single-runtime（多 agent 投影但只在一个被用）、unbound-unused（无绑定且无调用，与 zombie 区分）。
4. 排序输出：高频在前，zombie 归组在后；`--json` envelope 稳定，供 looper cron 周报 diff。

## 3. Non-Goals

1. 不做自动退役/自动解绑——只报告，行动留给人或独立的 reviewed flow。
2. 不改 Panel。
3. 不新增事件类型，只消费 GH541 与现有 emitter 产出。
4. 不做跨周趋势存储（diff 由外部调度方对两次 JSON 输出做）。

## 4. Behavior Invariants

1. **B-001** `loom skill stats` 是只读命令，不得修改 registry、binding、projection、telemetry event
   或 config；聚合 O(events)，不访问网络、不调用 LLM。
2. **B-002** `telemetry_enabled` 反映写入开关，`telemetry_empty` 只由持久化 event 是否为空决定；
   disabled 但有历史 event 时仍必须聚合，不能生成假 zombie。
3. **B-003** `--agent` 同时限制分类使用的 binding set 与 window usage set；分类不得把仅绑定其他
   agent 的 skill 误报为所选 agent 的 zombie。current binding set 必须直接来自同一
   `RegistrySnapshot` 的 active bindings/rules/targets 关系推导；rule/binding 已生效但 projection
   尚未 materialize 时仍是 bound，projection 仅提供健康信息，不得用历史 `observations/*.jsonl`
   推断绑定。跨 runtime 的 `single_runtime` 始终从未过滤的 binding/usage 集计算，且仅在当前至少绑定
   两个 agent、实际只在一个 agent 使用时为 true；envelope 标记 `single_runtime_scope="all_agents"`。
   `agent=null`/agentless telemetry 只参与未指定 `--agent` 的全局 usage/category 证据；指定
   `--agent X` 时只匹配 `Known(X)`，agentless 不能匹配任意 agent。agentless usage 可阻止未过滤报告
   产生假 zombie，但不得进入 `used_agents` 或 `single_runtime`，输出以 JSON `agent:null` 或明确
   agentless aggregate 表示，禁止占用可能与真实 agent 冲突的 `"unknown"` key。
4. **B-004** `--since` 只决定 window count/ranking；zombie 独立比较全量 eligible history 的
   `last_used` 与 injectable `now - zombie_days`。`skill.invocation` 与 `skill.error` 都表示一次实际
   尝试并参与 lifecycle `last_used`/cutoff；较早窗口中的旧调用不能让 30 天未使用的 skill 变 active。
5. **B-005** 每个 registry skill 恰好一个 category：`active`、`zombie`、`unbound_unused` 或
   `unbound_but_used`；`single_runtime` 是 flag，不是第五种 category。
6. **B-006** `skill_id=null` 且含合法 `observed_skill_name` 的 event，以及 `skill_id` 已不在 current
   registry 的 event，必须从与 skills 完全相同的 `--since` + `--agent` window 派生为 orphan；
   `window_events`、per-skill/per-agent counts、orphan counts、error samples 与 failure categories 必须
   对同一 filtered row set 可核对。malformed/invalid unmatched records 不得伪装成 registry skill。
7. **B-007** 明确定义 `attempt_count = invocation_count + error_count`，`error_rate =
   error_count / attempt_count`。仅当所选 window 的 attempt_count ≥ 5 时输出 rate，否则为 `null` 并
   给出 `error_sample_size=attempt_count`；invocation/error/attempt 三个 counter 与 failure-category
   原始计数在 skill total、per-agent、orphan 与 agentless aggregate 上始终显式返回。lifecycle 与
   ranking 必须一致使用 attempt_count，禁止一处按 invocation、另一处按 attempts 而未声明。
8. **B-008** 排序确定：active/unbound_but_used 先按 window attempt_count 降序再 skill id；
   zombie 后置并按 last_used（null 最后）再 skill id；unbound_unused 最后按 skill id。相同输入重跑顺序稳定。
9. **B-009** 命令必须持有现有 exclusive workspace lock 完成只读 snapshot capture：读取 registry
   snapshot、telemetry config 与 event log，并从传入 snapshot 构造 inventory；不得由 helper 隐式二次
   reload 形成撕裂视图。释放锁后才聚合，不新增 RW-lock 机制。所有 counter/sum 使用 checked arithmetic；overflow 返回带 aggregate field name 的
   `STATE_CORRUPT` 或 `INTERNAL_ERROR`，禁止 wrap、saturate 或 partial output。空 registry、空 events、
   全部 malformed 或 filter 无匹配均返回结构完整的空/零数组与显式计数；registry/event IO failure
   必须返回 error。

## 5. Acceptance Criteria

1. fixture registry + fixture 事件产出预期的 zombie/orphan/single-runtime/unbound-unused 分类。
2. 无绑定且无调用的 skill 报 unbound-unused，与 zombie（有绑定无调用）可区分。
3. `--json` envelope 字段写入 CLI contract 并有 surface 测试。
4. `--since`/`--agent`/`--zombie-days` 过滤生效且有测试。
5. agentless 只进入未过滤视图，orphan 与 skills 使用相同 window，所有 window totals 可核对。
6. stats 从一个 locked snapshot 读取；人工构造 overflow 时 fail closed 且不返回 partial output。

## 6. Edge Cases

1. skill 在窗口内被解绑：按当前绑定状态分类（不考古历史绑定）。
2. 同名 skill 多 target 绑定同一 agent：调用数按 agent 聚合，不按 target 拆。
3. `--since` 晚于全部事件：window count 归零并返回 `window_events=0`，但 category 仍由独立
   zombie cutoff view 决定；近期用过的 bound skill 保持 active。
4. orphan 名与已退役 skill 重名：报 orphan（与 GH541 Open Question 2 的保守策略一致）。
5. `agent=null` event 与命名为 `unknown` 的真实 adapter 同时存在：分别输出，绝不合并。
6. counter 接近整数上限：下一次累加立即结构化失败，不 wrap/saturate。

## 7. Resolved Decisions

1. 命令挂载为 `loom skill stats`：主体是 registry skill lifecycle，保持 issue 请求的公开路径；
   GH535 后续若移动 authoring commands，不移动本 operational read command。
2. error rate 最小样本门槛固定为 5，并在 envelope 返回样本数。

## 8. Boundary Checklist

| Boundary | Verdict |
| --- | --- |
| Empty / missing input | covered: B-009 |
| Error and failure paths | covered: B-006, B-009 |
| Authorization / permission | covered: B-001 |
| Concurrency / race / ordering | one locked snapshot; covered: B-001, B-009 |
| Retry / repetition / idempotency | covered: B-001, B-008 |
| Illegal state transitions | N/A: command owns no state transition |
| Compatibility / migration | covered: B-002, B-006 |
| Degradation / fallback | covered: B-002, B-007, B-009 |
| Evidence and audit integrity | covered: B-003, B-004, B-005, B-006 |
| Cancellation / interruption | N/A: read-only aggregation can be rerun from the immutable snapshot |
