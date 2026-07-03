# GH480 Product Spec - Skill inspect quality and safety evidence

Issue: https://github.com/majiayu000/loom/issues/480
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

`loom skill inspect` 面向用户展示 lifecycle status card，但 `quality` 仍是 `null`，`safety.policy` 仍是 `unknown`。这让 inspect 无法回答“这个 skill 最近是否有质量证据、是否被策略允许”。

## 2. Goals

1. `skill inspect` 必须展示真实可用的 eval quality evidence。
2. `skill inspect` 必须展示真实可用的 safety/policy decision。
3. 缺少证据时必须区分 `not_run`、`missing`、`stale`、`unavailable`。
4. JSON 和 human output 必须一致表达证据状态。

## 3. Non-Goals

1. 不在 inspect 内运行昂贵 eval。
2. 不把 eval 成功当作 safety guarantee。
3. 不改变 trust/quarantine 的存储模型。

## 4. Behavior Invariants

1. `quality.last_eval` 只能来自真实 eval report 或明确为无证据状态。
2. `trigger_precision`、`trigger_recall` 缺失时必须说明原因。
3. `safety.policy` 必须来自 policy/safety evaluation 或明确为 `not_run` / `unavailable`。
4. malformed eval/safety evidence 不得被渲染成“no evidence”而丢失错误细节。
5. inspect 是 read-only；不得修改 registry state、events、skills 或 live projection。

## 5. Acceptance Criteria

1. 无 eval report 时，inspect 输出 `quality.status=not_run` 或等价字段。
2. 有有效 eval report 时，inspect 输出 last eval status、timestamp、precision/recall。
3. eval report 损坏时，inspect 输出 evidence error，而不是 null。
4. trust blocked、quarantined、policy blocked 三类 safety 状态可区分。
5. 文本输出不再把未知 policy 当作正常 safety summary。

## 6. Edge Cases

1. eval report 是旧 schema。
2. eval report skill id 与查询 skill 不一致。
3. safety scan 可读但 policy profile 未知。
4. registry state 未初始化。

## 7. Open Questions

1. inspect 是否读取最新 eval report，还是读取 registry-indexed eval summary？
2. stale 的阈值是否需要配置？
