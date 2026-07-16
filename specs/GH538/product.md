# GH538 Product Spec - ops list silent zero on git history failure

Issue: https://github.com/majiayu000/loom/issues/538
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

`loom ops list` 在 git history 查询失败时用 `unwrap_or_default()` 吞掉错误，输出貌似合理的 0 条历史事件，与真实空历史不可区分，且无任何 warning。违反本仓防静默降级原则（GH479 同类先例已修）。

## 2. Goals

1. git history 查询失败对用户/agent 可见。
2. 降级输出与真实空历史在 JSON 中可区分。
3. 与 workspace status 的 unavailable-path warning 先例保持一致的表达方式。

## 3. Non-Goals

1. 不改变 `ops list` 其余字段的语义。
2. 不修复 git 失败本身的根因（那是环境问题）。

## 4. Behavior Invariants

1. 失败时要么命令报错，要么 `meta.warnings` 挂显式降级说明并附 degraded 标记。
2. 成功路径输出不变。
3. 空历史（真实 0 条）不产生 warning。

## 5. Acceptance Criteria

1. 注入 git 失败后 `ops list --json` 输出含 warning/错误，且 history 部分带 degraded 标记或命令失败。
2. 真实空 registry 下 `ops list` 无 warning、history 计数为 0。
3. 新增测试覆盖注入失败与真实空两种情形。

## 6. Edge Cases

1. registry 非 git 目录（init 前）。
2. git 存在但 history 部分可读（局部失败）。
3. panel 端消费同一数据时的展示。

## 7. Open Questions

1. 选报错（fail closed）还是 warning + degraded 标记（只读列表倾向后者）？
