# GH542 Tech Spec - skill stats lifecycle governance report

Issue: https://github.com/majiayu000/loom/issues/542
Product spec: `specs/GH542/product.md`
Route: `write_spec`
Status: implx auto draft; independent diff review passed; PR gate still required
Depends on: GH541

## 1. Current Behavior

- `loom telemetry report`（`src/commands/telemetry/`）按事件流聚合，不 join registry；`loom doctor` 检查完整性/投影，不看用量。
- skill 读模型来自 `build_skill_read_model`（`src/commands/mod.rs` 系列 helper），绑定/投影观测在 `state/registry/observations/*.jsonl`。
- 事件存储 `state/telemetry/events.jsonl`，已有解析容错（`TelemetryLog`）。

## 2. Proposed Design

1. 新增 CLI：`src/cli/` skill 命令组挂 `Stats(SkillStatsArgs)`（`--since`、`--agent`、`--zombie-days` 默认 30、全局 `--json`）。
2. 新增 `src/commands/skill_stats.rs`：
   - 载入 skill 读模型 + 绑定观测 → 每 skill 的 bound agents 集合。
   - 单遍扫 `TelemetryLog.events`，同时维护 unfiltered lifetime/global-flag view、按 `--agent`
     过滤但不受 `--since` 影响的 lifetime/cutoff view，以及 `--since` + `--agent` window view，
     按 (skill, agent) 聚合 count/last_used/error/failure_category。
   - 分类器（互斥完备）：
     - `--agent` 同时过滤当前 binding set 与 window usage；has_bindings 不得引用其他 agent。
     - registry skill：filtered bindings、window usage 与独立 zombie cutoff view → active / zombie /
       unbound-unused / unbound-but-used。
     - zombie 使用 agent-scoped lifetime/cutoff view 的 last_used 与 injectable clock 的
       `now-zombie_days` 比较，不复用 `--since` window，也不读取其他 agent usage；invocation 与
       error event 都更新 lifecycle `last_used`，因为当前 `loom skill used --error` 只发 error event。
     - single-runtime flag 从 unfiltered bound_agents/used_agents 计算（invocation/error 均计入
       used_agents），且要求
       `bound_agents.len() >= 2 && used_agents.len() == 1`；envelope 声明
       `single_runtime_scope: "all_agents"`。
     - orphan：GH541 v3 event 中 `skill_id=null` 的 `observed_skill_name`，以及现有 event 的
       `skill_id` 已不在当前读模型者，聚合到独立列表。
   - 错误率：window 中 invocation+error 样本数 ≥ 5 才输出，否则 `error_rate=null`；总是返回
     `error_sample_size` 与 failure-category 原始计数。
3. 排序：active 按窗口调用数降序 → zombie → unbound-unused；orphan 独立数组。
4. 复用 GH537 方向的 envelope 约定，全部字段进 `docs/LOOM_CLI_CONTRACT.md`。

## 3. Affected Areas

1. `src/cli/`（skill 命令组新增 args；若 GH535 命令面收敛先落地，则按其归组结论挂载）
2. `src/commands/skill_stats.rs`（新文件）
3. `src/commands/mod.rs`（路由）
4. `docs/LOOM_CLI_CONTRACT.md`
5. `tests/`（fixture registry + events 集成测试）

## 4. Output Contract

```json
{
  "since": "2026-06-16T00:00:00Z",
  "zombie_days": 30,
  "telemetry_enabled": false,
  "telemetry_empty": false,
  "single_runtime_scope": "all_agents",
  "window_events": 1893,
  "skills": [
    {"skill": "x-reply", "category": "active", "single_runtime": false,
     "by_agent": {"claude": {"count": 812, "last_used": "…"}, "codex": {"count": 40, "last_used": "…"}},
     "error_rate": 0.02, "failure_categories": {"timeout": 3}}
  ],
  "zombies": ["…"],
  "unbound_unused": ["…"],
  "orphans": [{"name": "old-deleted-skill", "agent": "codex", "count": 7}]
}
```

（`skills` 数组含全量分类明细，`zombies`/`unbound_unused` 是便于 diff 的名字摘要；确切字段以 contract doc 为准。）

## 5. Verification Plan

1. fixture registry（含多绑定/无绑定 skill）+ fixture events → 断言四类分类与 single-runtime 标记。
2. `cargo test --test skill_stats agent_filter_scopes_bindings_but_single_runtime_is_global`：Claude-only
   binding 不成为 Codex zombie，单 agent binding 不标 single-runtime，多 agent flag 不受 window filter 误导。
3. `cargo test --test skill_stats zombie_cutoff_is_independent_from_since`：60 天前调用在宽 `--since`
   window 有 count，但 `zombie_days=30` 仍为 zombie；测试 clock 可注入。
4. `cargo test --test skill_stats disabled_with_history_is_not_empty`：disabled config 仍读取 persisted
   events，分别返回 enablement 与 emptiness。
5. `cargo test --test skill_stats orphan_and_error_threshold_contract`：durable unmatched 进入 orphan，
   样本 4 返回 null、样本 5 返回 rate。
6. `cargo test --test skill_stats error_events_count_as_recent_lifecycle_usage`：仅有近期 error event 的
   bound skill 保持 active，且 failure category 仍计数。
7. contract surface test + `cargo check --workspace --all-targets --all-features && cargo test`。

## 6. Rollback Plan

纯只读新增命令，revert 无状态影响。

## 7. Product Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | read-only stats command + snapshot | `cargo test --test skill_stats command_is_read_only_and_linear` |
| B-002 | telemetry config/log loading | `cargo test --test skill_stats disabled_with_history_is_not_empty` |
| B-003 | filtered binding/window views + guarded global flag view | `cargo test --test skill_stats agent_filter_scopes_bindings_but_single_runtime_is_global` |
| B-004 | independent cutoff clock/view + error-as-usage semantics | `cargo test --test skill_stats zombie_cutoff_is_independent_from_since && cargo test --test skill_stats error_events_count_as_recent_lifecycle_usage` |
| B-005 | table-driven classifier | `cargo test --test skill_stats lifecycle_categories_are_exhaustive` |
| B-006 | v2/v3 event normalization + orphan aggregation | `cargo test --test skill_stats durable_unmatched_events_become_orphans` |
| B-007 | error aggregation | `cargo test --test skill_stats error_threshold_is_five` |
| B-008 | stable comparator | `cargo test --test skill_stats ordering_is_stable` |
| B-009 | empty/malformed/error paths | `cargo test --test skill_stats empty_and_error_contracts_are_explicit` |

## 8. Planned Changes Manifest

```specrail-planned-changes
{"issue":542,"complete":true,"paths":["src/cli.rs","src/commands/mod.rs","src/commands/skill_stats.rs","docs/LOOM_CLI_CONTRACT.md","tests/skill_stats.rs"],"spec_refs":["specs/GH542/product.md#4-behavior-invariants","specs/GH542/tech.md#5-verification-plan","specs/GH541/tech.md#8-planned-changes-manifest"]}
```
