# GH478 Product Spec - Rollback projection reconciliation

Issue: https://github.com/majiayu000/loom/issues/478
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

`loom skill rollback` 可以恢复源内容和 registry 历史，但 copy/materialize live projection 不会同步恢复。用户可能看到 rollback 成功，而 agent 仍读取旧投影内容。

## 2. Goals

1. rollback 后的 live projection 状态必须明确、可机器解析。
2. 对 stale copy/materialize projection，Loom 必须执行安全回投或返回完整 recovery plan。
3. registry snapshot 读取失败不能吞掉 stale projection 警告。
4. rollback 成功信息不能暗示 agent 已看到新内容，除非 Loom 已验证或执行 projection reconciliation。

## 3. Non-Goals

1. 不要求支持删除非 Loom-owned 目录。
2. 不在本 issue 内解决 projection digest 总体模型；该部分由 GH477 处理。
3. 不改变 release/snapshot 的语义。

## 4. Behavior Invariants

1. rollback 必须区分 source restored、registry restored、live projection reconciled 三种状态。
2. copy/materialize projection 未回投时，JSON 输出必须包含 `requires_projection_reapply=true` 或等价字段。
3. snapshot 读取错误必须进入 `error` 或 `meta.warnings`，不能被忽略。
4. symlink projection 的处理必须保持只检查路径，不复制内容。
5. recovery plan 必须包含可执行命令或明确的人类操作步骤。

## 5. Acceptance Criteria

1. rollback 后存在 stale copy projection 时，命令返回结构化 stale projection 列表。
2. rollback 后存在 stale materialize projection 时，命令返回结构化 stale projection 列表。
3. snapshot 损坏时，命令不会静默成功且无 projection warning。
4. 如果实现自动回投，必须只作用于 Loom-owned projection，并在失败时回滚或报告 partial failure。

## 6. Edge Cases

1. projection target 已不存在。
2. projection record 存在但 binding 已 orphaned。
3. source rollback 成功但 registry operation audit 写入失败。
4. 用户执行 `--dry-run` 或 rollback preview。

## 7. Open Questions

1. 默认行为应为自动回投，还是默认 recovery plan、显式 `--reproject-live` 才写 live path？
2. 是否需要把 rollback 后 visibility verification 纳入本 issue，还是留给 agent visibility issue？
