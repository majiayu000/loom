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
   agent 的 skill 误报为所选 agent 的 zombie。跨 runtime 的 `single_runtime` 始终从未过滤的
   binding/usage 集计算，且仅在当前至少绑定两个 agent、实际只在一个 agent 使用时为 true；
   envelope 标记 `single_runtime_scope="all_agents"`。
4. **B-004** `--since` 只决定 window count/ranking；zombie 独立比较全量 eligible history 的
   `last_used` 与 injectable `now - zombie_days`。`skill.invocation` 与 `skill.error` 都表示一次实际
   尝试并参与 lifecycle `last_used`/cutoff；较早窗口中的旧调用不能让 30 天未使用的 skill 变 active。
5. **B-005** 每个 registry skill 恰好一个 category：`active`、`zombie`、`unbound_unused` 或
   `unbound_but_used`；`single_runtime` 是 flag，不是第五种 category。
6. **B-006** `skill_id=null` 且含合法 `observed_skill_name` 的 invocation 聚合到独立 orphan 列表；
   malformed/invalid unmatched records 不得伪装成 registry skill。
7. **B-007** error rate 仅在所选 window 中 invocation + error 样本数至少 5 时输出数值；不足时为
   `null` 并给出 `error_sample_size`，failure categories 仍报告原始计数。
8. **B-008** 排序确定：active/unbound_but_used 先按 window invocation count 降序再 skill id；
   zombie 后置并按 last_used（null 最后）再 skill id；unbound_unused 最后按 skill id。相同输入重跑顺序稳定。
9. **B-009** 空 registry、空 events、全部 malformed 或 filter 无匹配均返回结构完整的空/零数组与
   显式计数，不虚构字段、不用 warning 代替读取错误；registry/event IO failure 必须返回 error。

## 5. Acceptance Criteria

1. fixture registry + fixture 事件产出预期的 zombie/orphan/single-runtime/unbound-unused 分类。
2. 无绑定且无调用的 skill 报 unbound-unused，与 zombie（有绑定无调用）可区分。
3. `--json` envelope 字段写入 CLI contract 并有 surface 测试。
4. `--since`/`--agent`/`--zombie-days` 过滤生效且有测试。

## 6. Edge Cases

1. skill 在窗口内被解绑：按当前绑定状态分类（不考古历史绑定）。
2. 同名 skill 多 target 绑定同一 agent：调用数按 agent 聚合，不按 target 拆。
3. `--since` 晚于全部事件：window count 归零并返回 `window_events=0`，但 category 仍由独立
   zombie cutoff view 决定；近期用过的 bound skill 保持 active。
4. orphan 名与已退役 skill 重名：报 orphan（与 GH541 Open Question 2 的保守策略一致）。

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
| Concurrency / race / ordering | read-only snapshot; covered: B-001, B-008 |
| Retry / repetition / idempotency | covered: B-001, B-008 |
| Illegal state transitions | N/A: command owns no state transition |
| Compatibility / migration | covered: B-002, B-006 |
| Degradation / fallback | covered: B-002, B-007, B-009 |
| Evidence and audit integrity | covered: B-003, B-004, B-005, B-006 |
| Cancellation / interruption | N/A: read-only aggregation can be rerun from the immutable snapshot |
