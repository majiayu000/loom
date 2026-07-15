# GH522 Tech Spec: Registry Sync 与 Runtime Convergence 状态分离

Issue: https://github.com/majiayu000/loom/issues/522
Product spec: `specs/GH522/product.md`
Status: Draft for maintainer review

## Codebase Context

| Area | Current evidence |
| --- | --- |
| Envelope metadata | `src/envelope.rs:15-22` 只有 `sync_state` 与 `op_id`，无法承载三个状态轴 |
| Registry transport enum | `src/types.rs:108-116` 的 `SyncState` 是 remote/backlog 闭集 |
| Registry push | `src/commands/sync_cmds.rs:180-231` 只处理 registry state、operation journal、Git remote 与 tags |
| Projection evidence | `src/commands/projections/observation.rs:126-175` 独立计算 projection check；`:177-224` 以 digest 判断 copy/materialize drift |
| Visibility contract | `docs/LOOM_CLI_CONTRACT.md:430-443` 已禁止从 projection presence 推导 agent visibility |
| Existing public wording | `docs/LOOM_CLI_CONTRACT.md:154-162` 把 `meta.sync_state` 定义为 agent 的权威 sync status |
| Shipped Agent Skill | `skills/loom-registry/SKILL.md:72-79` 当前只提醒 `meta.sync_state` 的 remote 含义，未提供统一三轴对象 |

## 设计

### 1. 统一读模型

新增 typed read model（建议位于 `src/core/convergence_status.rs`）：

```rust
struct ConvergenceStatus {
    registry_transport: RegistryTransportStatus,
    projections: ProjectionConvergenceStatus,
    visibility: VisibilityStatus,
    observed_at: DateTime<Utc>,
}
```

三个子对象都包含 `state`、`evidence`、`errors`，但使用独立枚举。默认公共位置为
`data.convergence`，避免把命令选择器相关的多 projection items 塞进全局 `meta`。
`meta.sync_state` 在兼容期保留，并由同一个 `RegistryTransportStatus` 适配生成，禁止双重
计算。

### 2. 状态闭集

- `RegistryTransportState`: 复用现有序列化值 `SYNCED`、`PENDING_PUSH`、
  `DIVERGED`、`CONFLICTED`、`LOCAL_ONLY`，并允许读取失败时由外围 status 标记
  `ERROR`，不扩写旧 `SyncState`。
- `ProjectionConvergenceState`: `converged`、`drifted`、`missing`、`conflict`、
  `not_applicable`、`unknown`、`error`。
- `VisibilityState`: `visible`、`not_visible`、`restart_required`、`unsupported`、
  `unknown`、`error`。

聚合对象不提供一个会掩盖轴差异的 `overall=success`。如需摘要，只允许
`complete: bool`，其为 true 必须满足本次请求声明的 required axes。

### 3. Evidence join

1. Registry transport 复用 `remote_status_payload` 的 remote/backlog/ahead/behind 结果。
2. Projection 状态复用 `observe_projection`，按 selector 返回 `items[]`，每项包含
   `instance_id`、`method`、`source_digest`、`materialized_digest`、`observed_at`。
3. Visibility 复用 adapter-driven visibility report；adapter 无能力时输出
   `unsupported`，读取失败输出 `error`。
4. Join 开始时捕获 registry HEAD 与 snapshot checkpoint；结束时重新检查，并重新读取
   remote transport、projection digest/链接状态与 adapter 配置/visibility report。任一 live
   evidence 在读取期间变化时，仅将对应轴标记 `stale=true`；registry HEAD/checkpoint 变化时
   三轴均标记 stale。读路径不得保存 observation updates。

### 4. 公共表面

首批接入：

- `workspace status`：registry-wide 摘要；
- `skill inspect` / `skill diagnose`：单 Skill 完整三轴；
- `skill visibility`：保留详细 visibility，同时嵌入同形 `convergence`；
- Panel API adapter：直接消费同一 typed read model，不自行推导状态。

`sync status` 保持 registry transport 专用，但字段命名改为
`data.registry_transport`，兼容期保留 `data.remote` 镜像。写命令只有在已经计算对应证据时
才返回 `data.convergence`，不得填充猜测值。

### 5. 兼容与迁移

1. `meta.sync_state` 保留至下一个 major version；它与
   `data.convergence.registry_transport.state` 由同一值生成。
2. 新旧字段不一致是测试失败和 `STATE_CORRUPT` 级内部错误，不允许 warning 后继续。
3. CLI contract 标记旧字段 deprecated，但不改变现有退出码。
4. Panel 先读新字段，缺失时仅对旧服务器回退到 remote-only 显示，并明确
   `projection=unknown`、`visibility=unknown`。

### 6. 文档与 Agent Skill

更新 `docs/LOOM_CLI_CONTRACT.md`、`docs/LOOM_API_CONTRACT.md`、
`docs/AGENT_USAGE.md`、Panel labels 与 `skills/loom-registry/SKILL.md`。所有示例必须展示
交叉状态，不再把单一 `SYNCED` 作为完成证明。#523 的 contract drift gate 应消费这些示例。

## Product-to-Test Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | `ConvergenceStatus` serializer | `cargo test convergence_status_shape` |
| B-002 | registry transport adapter | `cargo test registry_transport_does_not_read_projection_state` |
| B-003 | projection observation join | `cargo test projection_state_ignores_remote_state` |
| B-004 | adapter visibility mapping | `cargo test visibility_requires_agent_evidence` |
| B-005 | cross-axis fixtures | `cargo test --test convergence_status remote_synced_projection_stale` |
| B-006 | empty projection and unsupported adapter fixtures | `cargo test --test convergence_status empty_and_unsupported` |
| B-007 | per-axis error fixtures | `cargo test --test convergence_status axis_failure_is_not_clean` |
| B-008 | legacy field adapter | `cargo test legacy_sync_state_matches_registry_transport` |
| B-009 | read-only mutation snapshot assertions | `cargo test --test status --test skill_inspect` |
| B-010 | HEAD/checkpoint 与 live projection/visibility evidence race fixtures | `cargo test convergence_status_marks_stale_on_race && cargo test live_recheck_marks_only_changed` |
| B-011 | legacy JSON consumer fixture | `cargo test --test cli_surface legacy_sync_state_contract` |
| B-012 | contract/Skill/Panel terminology check | `cargo test --test cli_surface --test shipped_registry_skill` |
| B-013 | interrupted collector fixture | `cargo test convergence_status_partial_collection` |

## 风险与回滚

1. **Payload growth**：registry-wide status 只返回计数摘要；详细 items 仅在单 Skill 查询中
   返回。
2. **双字段漂移**：旧字段只从新 typed model 适配，禁止两套计算。
3. **Panel 兼容**：先 additive 发布，Panel 保守回退；不在同一版本删除旧字段。
4. **性能**：digest/visibility 检查沿用现有 bounded 查询，并增加现有 perf smoke 预算验证。
5. **回滚**：可移除 additive `data.convergence` 接入而保留旧 `meta.sync_state`；不得回滚为
   将 projection/visibility 推导成 synced。

## 规格门禁

- 本 PR 只写规格，不实现状态模型。
- `Status: Draft for maintainer review`；需要维护者批准后才能进入 implement route。
