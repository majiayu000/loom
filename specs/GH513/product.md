# Product Spec

## Linked Issue

GH-513

## 用户问题

Loom 当前把未确认的 registry journal rows 和 `loom-history` 中的本地审计事件都显示为 pending operations。健康的 `LOCAL_ONLY` registry 因此可能报告数百个 backlog，尽管没有任何失败或可立即执行的同步工作；CLI、doctor 与 Panel 还使用不同的近似计数，用户无法区分本地事实、待推送历史与真正需要处理的操作。

## 目标

- 用同一组互斥计数区分 actionable operations、本地 journal 事实、待推送 history 与 local-only history。
- 让 `sync status`、`ops list`、`workspace doctor`、Panel API 与 Panel UI 使用同一分类语义。
- 让健康且无失败的 `LOCAL_ONLY` registry 显示零 actionable operations，同时保留本地 journal/history 可见性。
- 保留既有 machine-readable 字段的兼容 alias，并文档化新计数与迁移方式。
- 仅依赖本地 refs 计算读状态，不在 status/list/doctor/Panel read path 中 fetch 或修改 registry。

## 非目标

- 不删除 registry operation rows 或 `loom-history` 审计事件。
- 不新增 persisted registry schema、远端协议或后台同步器。
- 不恢复已移除的 `pending_ops` runtime 字段。
- 不把 `LOCAL_ONLY` 本身视为故障，也不隐藏真实 failed operation。
- 不发布 crate、Homebrew formula、GitHub Release 或版本 tag。

## Behavior Invariants

1. 所有 surfaces 输出同一 `operation_counts` 对象，包含 `actionable_operations`、`local_journal_events`、`unpushed_history_events` 与 `local_only_history_events` 四个非负整数；每个 active journal row 和每个唯一 history event 只进入一个 bucket。
2. `ack=true` 或 `status=purged` 的 journal row 不进入任何 active bucket。其余 row 中，failed 或非终态 row 始终 actionable；`status=succeeded` 且未确认的 row 在配置 origin 时 actionable，在没有 origin 时计入 `local_journal_events`。
3. history 以 archives 与 segments 内去重后的 `event_id` 为单位。配置 origin 时，本地集合减去 cached `origin/loom-history` 集合计入 `unpushed_history_events`；没有 origin 时，全部本地唯一事件计入 `local_only_history_events`。不得用 segment/archive 文件数或两个总数相减代替集合差。
4. 健康且没有 failed/non-terminal row 的 `LOCAL_ONLY` registry 必须报告 `actionable_operations=0`。例如 3 个成功未确认 journal rows 与 400 个本地 history events 报告 `0 / 3 / 0 / 400`，不得显示 403 pending operations。
5. `ops list` 与 `/api/v1/ops/pending.data.ops` 只返回 actionable rows。兼容字段 `count`、`operation_backlog` 与 doctor 的 `operation_journal.count` 均为 `actionable_operations`；`journal_events` 等于前两个 journal buckets 之和，`history_events` 等于后两个 history buckets 之和。
6. 配置 origin 后，成功未确认 journal rows 转为 actionable，并继续由既有 sync push/ack 流程处理；history 差异单独显示，不混入操作 rows。failed local-only row 保持 actionable，并明确受“未配置 remote”阻塞。
7. status/list/doctor/Panel 只读取当前 registry 与 cached refs。cached remote 可能落后于服务器，输出与迁移文档必须说明这是本地视图；读取或解析失败必须返回结构化错误，不得静默降级为零。
8. Panel 将无 actionable operations 的 `LOCAL_ONLY` 呈现为信息状态，并分别展示四个 bucket；真实 actionable 或 failed rows 仍保持可见且可操作。

## 验收标准

- [x] local-only regression 覆盖成功 journal rows 与大量 history events，并在 CLI、doctor、Panel API/UI 中一致显示零 actionable。
- [x] configured-remote regression 覆盖成功未确认 row、failed row、远端共有 history 与仅本地 history，证明集合差和 bucket 互斥。
- [x] `sync status`、`ops list`、`workspace doctor` 与 Panel API JSON 暴露完整 `operation_counts`，兼容 aliases 保持预期值。
- [x] `ops list`、retry/replay 与 `/api/v1/ops/pending` 不再把 local-only succeeded rows 当作 actionable；显式 `ops purge` 仍可清除全部未确认 journal rows。
- [x] malformed registry/history 或 Git read failure fail closed，不返回伪造的零计数。
- [x] CLI/Panel focused tests、Rust build/test、Panel type/test/build、完整仓库检查与 SpecRail gate 通过。

## 边界情况

- local 与 cached remote 事件总数相同但 `event_id` 完全不同，所有本地 IDs 仍是 unpushed。
- 同一 `event_id` 同时存在于 archive 和 segment 时只计一次。
- remote 已配置但尚无 cached `origin/loom-history` 时，全部本地 history events 是 unpushed。
- registry 尚未初始化或没有 history branch 时四个 counter 合法为零；损坏的已存在数据不能当作“无数据”。
- 未知非终态 status 按 actionable 处理，避免新状态被静默隐藏。
- 配置 origin 不代表已执行 fetch；read surface 不能声称计数反映服务器最新状态。

## 发布说明

这是 additive JSON counter 与状态语义修复。兼容 aliases 保留，但其含义明确收敛为 actionable operations；迁移文档说明新字段和 cached-remote 限制。本 PR 不执行发布动作。
