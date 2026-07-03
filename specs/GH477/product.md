# GH477 Product Spec - Projection drift and content digests

Issue: https://github.com/majiayu000/loom/issues/477
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

Loom 可以把 skill 投影到 agent 目录，但 copy/materialize 投影缺少可读回的内容证据。当前状态可能显示 `healthy`，即使 agent 实际读取的是旧内容。

## 2. Goals

1. 投影状态必须能表达 live projection 与源 skill 的内容是否一致。
2. `diagnose`、watch 或等价观察路径必须能把发现的 projection drift 写回为持久状态。
3. `workspace status` 的 `drifted_projections` 必须来自真实观察，而不是永远依赖默认 `observed_drift=false`。
4. copy/materialize 投影必须有内容级校验信号；symlink 投影必须保持路径级校验信号。

## 3. Non-Goals

1. 不改变 skill 作者编辑内容的流程。
2. 不把 eval 成功等同于 projection healthy。
3. 不要求 rollback 在本 issue 内完成 live projection 回投；该行为由 GH478 处理。

## 4. Behavior Invariants

1. 当 live projection 内容与最近一次投影记录不一致时，Loom 必须报告 drift。
2. 当 drift 观察失败且结果会影响用户看到的健康状态时，Loom 必须返回结构化错误或显式 warning。
3. `healthy` 只能表示最近一次可验证观察通过。
4. 缺少 live path、缺少源 skill、内容 digest 不匹配和无法读取 live path 必须是不同的可机器解析状态。
5. JSON 输出必须保留 `instance_id`、`skill_id`、`target_id`、`materialized_path` 和 `method`。

## 5. Acceptance Criteria

1. 对 symlink、copy、materialize 三种投影分别提供 healthy、drifted、missing、unreadable 的验收场景。
2. `loom skill diagnose <skill>` 能让 projection drift 进入后续 `workspace status` 可见的持久状态，或明确声明只读模式并提供对应写入命令。
3. `workspace status` 中 drifted projection 计数与最新持久 projection health 一致。
4. 所有新增错误都进入 JSON envelope，不依赖纯文本 stderr。

## 6. Edge Cases

1. 源 skill 未被 Git 跟踪。
2. live projection 被手工删除。
3. copy projection 中存在内部 symlink。
4. materialize projection 解引用后内容变化。
5. registry snapshot 损坏或无法读取。

## 7. Open Questions

1. drift 写回是否由 `diagnose --apply`、watch 周期，还是专门的 `skill observe` 命令负责？
2. digest 字段是记录源树 digest、live projection digest，还是两者都记录？
