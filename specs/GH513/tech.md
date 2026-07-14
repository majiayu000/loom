# Tech Spec

## Linked Issue

GH-513

## Product Spec

见 `product.md`。

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Registry operation reads | `src/state/registry_ops.rs` | 所有 `!ack && status != purged` rows 都进入 `RegistryOpsReport.ops` | 根因之一：local-only succeeded rows 被当作 backlog |
| History status | `src/gitops/history.rs`, `history_types.rs`, `src/state/journal.rs` | 只报告 archive/segment 文件数；已有 fail-closed journal parser 与 event ID 去重逻辑 | 需要按唯一 event ID 比较 local/cached remote 集合 |
| CLI projections | `src/commands/projections.rs`, `sync_cmds.rs`, `workspace_cmds/status.rs`, `doctor.rs`, `agent_cmds.rs` | 多处独立读取 `ops.len()`，history 文件数被塞入 `history_events` | 必须收敛为共享计数模型并保留 aliases |
| Panel API | `src/panel/handlers/ops.rs` | pending endpoint 返回旧 report rows/count | 需要与 CLI 使用同一 classifier |
| Panel client/UI | `panel/src/types.ts`, `lib/api/*`, `lib/panel_view_model.ts`, `pages/panel/*`, `pages/SkillMPanel.tsx` | pending adapter 假设旧 row shape；LOCAL_ONLY 固定 warning；只展示 queued count | 需要 typed counters、正确 row adapter 与信息态 |
| Contracts/migration | `docs/LOOM_API_CONTRACT.md`, `docs/LOOM_CLI_CONTRACT.md`, `docs/LOOM_STATE_MIGRATION_NOTES.md`, `CHANGELOG.md` | 没有四 bucket 定义和 cached-ref 限制 | machine consumers 需要可迁移契约 |

## 设计方案

1. 在 `src/state/registry_ops.rs` 新增 serializable `OperationCounts` 与分类后的 `RegistryOpsReport`。读取函数接收/推导 `remote_configured`，先排除 ack/purged，再将 succeeded-unacked 按 remote presence 分到 actionable 或 local journal；failed、pending、running 和未知 active status 全部 actionable。`ops` 只包含 actionable rows。
2. 在 journal module 暴露复用现有 `parse_journal_line` 的 fail-closed event-ID collector。history module 分别读取 local 与 cached `origin/loom-history` archives/segments，合并去重集合；没有 remote 时填 `local_only_history_events`，配置 remote 时计算 local set difference 填 `unpushed_history_events`。read path 不 fetch、不更新 ref。
3. 由一个共享 operation report 组合 journal 与 history buckets，避免 commands 自行相加。没有数据返回真实零；已存在 blob/JSON 解析失败向上传播错误。
4. `remote_status_payload`、workspace status、doctor、ops list 与 agent sync dry-run 直接序列化共享 `operation_counts`。`operation_backlog`、`count`、doctor `operation_journal.count` 取 `actionable_operations`；`journal_events`/`history_events` 是兼容 bucket sums。禁止重新引入 `pending_ops`。
5. retry/replay/purge/sync push 继续只使用 `report.ops`，因此 local-only succeeded rows 不会被列为可操作；配置 remote 后相同 rows 自动进入 actionable 并沿既有 ack 流程处理。failed local-only rows 会被列出，实际 replay 继续返回 remote-not-configured blocker。
6. `/api/v1/ops/pending` 返回真实 `RegistryOperationRecord` rows 和 `operation_counts`。Panel 移除错误的 legacy pending-row adaptation，使用 registry operation adapter，并把 typed counts 放入 live data/view model。
7. Overview、Sync 与 Ops surfaces 分别显示四个 bucket；`LOCAL_ONLY && actionable_operations == 0` 使用 info/neutral copy，actionable/failed 保持 warning/error。`SkillMPanel.tsx` 当前正好 800 行，修改时必须通过抽取或净减行数保持不超过硬上限。
8. 更新 API/CLI/migration docs 和 changelog，说明 aliases、四 bucket 示例、cached remote 语义与 consumer migration。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1/P2/P4 | registry classifier | unit/integration fixtures for local-only, remote, failed, acked, purged, unknown status |
| P3/P7 | history event-set reader | duplicate ID, divergent equal-size sets, no cached ref, malformed body tests |
| P5/P6 | CLI/doctor/Panel handlers | JSON compatibility assertions, actionable-only row assertions, sync transition tests |
| P8 | Panel types/view model/pages | Vitest render/model tests for info LOCAL_ONLY and four visible counters |

## 数据流

registry `operations.json` + local `loom-history` + cached `origin/loom-history` + origin presence → shared classifier → `OperationCounts` + actionable operation rows → CLI status/list/doctor、Panel pending API、agent dry-run → typed Panel live data → Overview/Sync/Ops presentation。

所有 read-only surfaces 使用同一 classifier；只有既有显式 sync command 可以 fetch/push/ack。

## 备选方案

- 仅把 `pending_ops` 改名：仍混合不同生命周期，也违反已移除字段的 contract regression。
- 用 history segment/archive 数量：compaction 会改变文件数但不改变事件事实，且无法计算准确的 remote set difference。
- 用 local count 减 remote count：两个集合大小相同也可能完全分叉，会静默漏报。
- LOCAL_ONLY 时忽略全部 journal rows：会隐藏 failed/non-terminal operations，违反 fail-visible 要求。
- status 时自动 fetch：把只读命令变成网络与 ref mutation，破坏 deterministic/offline contract。

## 风险

- Compatibility: aliases 保留但语义从“所有未 ack 事实”收敛为 actionable；migration docs 必须给出字段映射。
- Performance: event IDs 需读取 history blobs，复杂度与 cached history 大小线性；现有 retention 限制 blob 数，使用 `BTreeSet` 去重。
- Correctness: snapshot 是派生汇总且不含完整 event IDs，不能用于集合差；必须解析 archives/segments。
- Security/reliability: Git 与 JSON 错误向上传播；不新增 shell 拼接或 remote network side effect。
- UI: Panel 类型和 adapters 必须一次更新，避免 Rust JSON 与 TS 假定再次漂移。

## 测试计划

- [ ] Regression-first: `cargo test --test reliability --test status --test doctor`
- [ ] Panel handler: `cargo test panel::tests::handlers`
- [ ] Panel: `npm --prefix panel test -- --run` and `npm --prefix panel run build`
- [ ] Static/build: `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --all-features`, `git diff --check`
- [ ] Full repository: `make check`
- [ ] Spec packet: `python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/implx-GH513-operation-counts/specs/GH513`
- [ ] Removed-field guard: `! rg -n 'pending_ops' src panel/src`

## 回滚方案

回滚本 PR 会恢复旧 backlog 语义；没有 persisted schema migration 或数据删除。新字段是 additive，回滚后 consumers 仍可使用保留的旧 aliases，但会再次看到混合计数。
