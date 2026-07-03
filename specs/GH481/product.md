# GH481 Product Spec - Workflow run and rollback_token contract

Issue: https://github.com/majiayu000/loom/issues/481
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

Loom 暴露 `workflow run` 和 `rollback_token`，但 execution path 与 token consumer 没有接线。用户和 agent 会看到似乎可执行的恢复/工作流能力，但实际无法完成闭环。

## 2. Goals

1. `workflow run` 的 CLI surface 必须与真实能力一致。
2. `rollback_token` 必须可消费，或从公共输出移除。
3. 不支持的能力必须以明确状态呈现，不能像半成品 contract。
4. 文档、help、JSON output 必须一致。

## 3. Non-Goals

1. 不实现完整 DAG workflow orchestration；该方向属于 GH379。
2. 不改变已有 `plan use` 和 `apply` 的安全门禁。
3. 不绕过 required approvals。

## 4. Behavior Invariants

1. 如果 `workflow run` 保留，它必须执行一个可验证的最小安全路径，或只作为 hidden/deprecated command。
2. 如果 `rollback_token` 保留，必须有命令能验证 token、执行恢复或报告 token stale/invalid。
3. unsupported feature 必须返回明确 `unsupported` / `deferred` 状态，不应混同 policy failure。
4. `--dry-run` 必须保持无副作用。

## 5. Acceptance Criteria

1. `workflow run` 的行为与 docs/help 完全一致。
2. apply recovery output 不再包含无消费方的 token，或 token consumer 已实现并测试。
3. stale/invalid token 有明确错误码。
4. 对 unsupported/deferred behavior 的测试不再依赖“永久 PolicyBlocked”作为正常路径。

## 6. Edge Cases

1. workflow plan 引用被删除的 skill。
2. token 对应的 plan 已被执行或回滚。
3. required approvals 缺失。
4. dry-run 与真实 run 输出字段差异。

## 7. Open Questions

1. 最小 `workflow run` 是否只支持单节点 workflow？
2. 是否应把 `rollback_token` 替换为显式 `rollback_commands`，直到 token store 成熟？
