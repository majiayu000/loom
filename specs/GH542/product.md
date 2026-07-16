# GH542 Product Spec - skill stats lifecycle governance report

Issue: https://github.com/majiayu000/loom/issues/542
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`
Depends on: GH541 (telemetry ingestion)

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

1. 只读命令：不改 registry、绑定、telemetry 状态。
2. O(events) 单遍聚合，无网络、无 LLM。
3. telemetry 为空/关闭时不报错：输出全量 skill 的 unbound/zero 分类，envelope 标注 `telemetry_empty=true`。
4. 分类互斥且完备：每个 registry skill 恰好落一个生命周期类别；orphan 单列（不属于 registry skill 集合）。

## 5. Acceptance Criteria

1. fixture registry + fixture 事件产出预期的 zombie/orphan/single-runtime/unbound-unused 分类。
2. 无绑定且无调用的 skill 报 unbound-unused，与 zombie（有绑定无调用）可区分。
3. `--json` envelope 字段写入 CLI contract 并有 surface 测试。
4. `--since`/`--agent`/`--zombie-days` 过滤生效且有测试。

## 6. Edge Cases

1. skill 在窗口内被解绑：按当前绑定状态分类（不考古历史绑定）。
2. 同名 skill 多 target 绑定同一 agent：调用数按 agent 聚合，不按 target 拆。
3. `--since` 晚于全部事件：全部归零调用，分类退化为 zombie/unbound——envelope 需带 `window_events=0` 提示避免误读。
4. orphan 名与已退役 skill 重名：报 orphan（与 GH541 Open Question 2 的保守策略一致）。

## 7. Open Questions

1. 命令挂载点：`loom skill stats` 还是 `loom telemetry stats`？（倾向 `skill stats`：主体是 skill 生命周期而非事件流；且与 GH535 命令面收敛方向一致，需 maintainer 确认归组）
2. 错误率的最小样本门槛（调用数 < N 时不显示错误率避免噪声）？建议 N=5。
