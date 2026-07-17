# GH542 Tech Spec - skill stats lifecycle governance report

Issue: https://github.com/majiayu000/loom/issues/542
Product spec: `specs/GH542/product.md`
Route: `write_spec`
Status: implx auto architecture rewrite; independent diff review passed; PR gate still required
Depends on: GH541

## 1. Current Behavior

- `loom telemetry report`（`src/commands/telemetry/`）按事件流聚合，不 join registry；`loom doctor` 检查完整性/投影，不看用量。
- skill 读模型来自 `build_skill_read_model`；当前绑定真相是同一 `RegistrySnapshot` 中 active
  binding/rule/target 的关联。projection 可缺失且只提供 materialization/health 信息，不改变已生效
  rule 的 bound 状态；`state/registry/observations/*.jsonl` 是历史投影观测，不能用于 zombie、
  unbound 或 single-runtime 的 current-binding 分类。
- 事件存储 `state/telemetry/events.jsonl`，已有解析容错（`TelemetryLog`）。
- `TelemetryEvent`、`TelemetryLog` 与 `read_event_log` 当前是 telemetry 私有实现，新的 sibling
  `skill_stats.rs` 不能直接消费；report 还会在每个 event 上重扫 entry list，存在 O(events²) 分组路径。
- `build_skill_read_model()` 会自行 reload registry，而 `RegistryStatePaths::load_snapshot()` 又逐文件读取；
  stats 若分别调用会得到撕裂视图，不能满足同一 snapshot 契约。

## 2. Proposed Design

1. 新增 CLI：`src/cli/` skill 命令组挂 `Stats(SkillStatsArgs)`（`--since`、`--agent`、`--zombie-days` 默认 30、全局 `--json`）。按现有 command-surface budget 记录 GH542 例外：owner 为 GH542 telemetry governance workstream；当 operational skill inventory reads 收敛为一个不破坏 1.x contract 的 grouped read surface 时 sunset。
2. 新增 `src/commands/telemetry/query.rs`，暴露完整 redacted `NormalizedTelemetryDataset`（保留
   activation/deactivation/invocation/eval/safety/error/feedback、hashed identifiers 与全部现有 metrics）
   供 `telemetry report` 使用；同时从 full rows 派生最小 `UsageRow {skill_ref:
   Registered(id)|Observed(name)|Unattributed, agent_ref: Known(agent)|Unknown, kind:
   Invocation|Error, timestamp, failure_category}` usage-only view 供 stats 使用。两者共享 matched/
   unmatched、agent filter、malformed 与 O(events) 语义，但 stats 不丢弃或改变 report 的非 usage 数据。
3. 新增 `src/commands/skill_stats.rs`：
   - 获取现有 exclusive workspace lock，仅在同一临界区做只读 snapshot capture：读取 current
     `RegistrySnapshot`、telemetry config 与 event log，并通过新增的
     `build_skill_read_model_from_snapshot` 风格入口从传入 snapshot 构造 inventory；释放 lock 后再聚合，
     不新增 RW-lock 实现。从 active binding/rule/target 关联直接
     构造每 skill 的 bound agents 集合，即使 projection 尚未 materialize 也保持 bound；不读取历史
     observation 作为绑定真相。
   - 单遍扫从 full dataset 派生的 usage-only `UsageRow` view，同时维护 `all_lifetime`（全局 known-agent single-runtime）、
     `scoped_lifetime`（只受 `--agent` 影响，用于 lifecycle）与 `window`（`--agent` + `--since`，用于
     counts/ranking/errors/orphans/window_events）三份投影，
     按 (skill, agent) 聚合 count/last_used/error/failure_category。
   - 分类器（互斥完备）：
     - `--agent` 同时过滤当前 binding set 与 window usage；has_bindings 不得引用其他 agent。
     - registry skill：bound skill 仅用 `scoped_lifetime.last_used` 与 injectable
       `now-zombie_days` cutoff 判 active/zombie；unbound skill 仅按 `scoped_lifetime` 是否存在任意 attempt
       判 unbound-but-used/unbound-unused。`window` 绝不参与 category，只控制 counts、ranking、errors、
       orphans 与 `window_events`，因此 category 独立于 `--since`。
     - invocation 与 error event 都更新 lifecycle `last_used`，因为当前 `loom skill used --error` 只发
       error event；命名 `--agent` 时不读取其他 agent usage。
     - single-runtime flag 从 unfiltered bound_agents/used_agents 计算（invocation/error 均计入
       used_agents），且要求
       `bound_agents.len() >= 2 && used_agents.len() == 1`；envelope 声明
       `single_runtime_scope: "all_agents"`。
     - `agent_ref=Unknown` 仅进入未指定 `--agent` 的 scoped/window view，可阻止 unfiltered fake
       zombie；它不进入 used_agents/single-runtime。命名 filter 只匹配 `Known(X)`。
     - orphan：GH541 v3 event 中 `skill_id=null` 的 `observed_skill_name`，以及现有 event 的
       `skill_id` 已不在当前读模型者，只从同一 `window` rows 聚合；agentless 以 null/独立 aggregate
       表示，不使用 `"unknown"` agent key。
   - 计数：所有累加使用 checked arithmetic；overflow 返回包含 aggregate field name 的结构化
     `STATE_CORRUPT`/`INTERNAL_ERROR`，不产生 partial result。定义 `attempt_count = invocation_count +
     error_count` 与 `error_rate = error_count / attempt_count`；attempt_count ≥ 5 才输出 rate，始终返回
     invocation/error/attempt counts、`error_sample_size` 与 failure-category raw counts。
   - reconciliation invariant：`window_events` 等于同一 filtered row set 中 registry-skill attempts、
     orphan attempts 与 unattributed rows 的可解释总和；per-agent/error/failure-category 也从该 set 派生。
     `agentless` 是已包含在 registry/orphan/unattributed partitions 中的正交 attribution subtotal，不能
     作为额外一项再次加入 reconciliation total。
4. 排序：active 与 unbound-but-used 同组，按窗口 attempt_count 降序再 skill id；随后 zombie
   按 last_used（null 最后）再 skill id，最后 unbound-unused 按 skill id；orphan 独立数组。
5. 复用 GH537 方向的 envelope 约定，全部字段进 `docs/LOOM_CLI_CONTRACT.md`。

## 3. Affected Areas

1. `src/cli/`（skill 命令组新增 args；若 GH535 命令面收敛先落地，则按其归组结论挂载）
2. `src/commands/telemetry/query.rs` 与 `src/commands/telemetry/mod.rs`（normalized read boundary；report 复用）
3. `src/commands/skill_inventory.rs`（从 caller-supplied snapshot 构造 read model）
4. `src/commands/skill_stats.rs`（新文件）
5. `src/commands/mod.rs`（路由）
6. `docs/LOOM_CLI_CONTRACT.md`
7. `tests/`（fixture registry + events 集成测试）

## 4. Output Contract

```json
{
  "since": "2026-06-16T00:00:00Z",
  "zombie_days": 30,
  "telemetry_enabled": false,
  "telemetry_empty": false,
  "single_runtime_scope": "all_agents",
  "window_events": 1893,
  "unattributed_window_events": 0,
  "skills": [
    {"skill": "x-reply", "category": "active", "single_runtime": false,
     "invocation_count": 847, "error_count": 5, "attempt_count": 852,
     "last_used": "…",
     "by_agent": {"claude": {"invocation_count": 809, "error_count": 3, "attempt_count": 812,
                                "window_last_used": "…", "failure_categories": {"timeout": 3}},
                  "codex": {"invocation_count": 38, "error_count": 2, "attempt_count": 40,
                              "window_last_used": "…", "failure_categories": {"tool_error": 2}}},
     "error_rate": 0.006, "error_sample_size": 852,
     "failure_categories": {"timeout": 3, "tool_error": 2}}
  ],
  "zombies": ["…"],
  "unbound_unused": ["…"],
  "orphans": [{"name": "old-deleted-skill", "agent": "codex", "invocation_count": 7,
                "error_count": 0, "attempt_count": 7, "failure_categories": {}}],
  "agentless": {"invocation_count": 0, "error_count": 0, "attempt_count": 0,
                  "failure_categories": {}}
}
```

（`skills[].last_used` 是 scoped-lifetime lifecycle 时间；`by_agent[].window_last_used` 只描述当前 window。
`agentless` 是已包含在 skills/orphans/unattributed 中的 attribution subtotal，不额外增加 `window_events`。
`skills` 数组含全量分类明细，`zombies`/`unbound_unused` 是便于 diff 的名字摘要；确切字段以 contract doc 为准。）

## 5. Verification Plan

1. fixture current snapshot（含多绑定、无绑定、binding/rule 无 projection、stale historical
   observation skill）+ fixture events → 断言四类分类与 single-runtime 标记只跟随 current snapshot。
2. `cargo test --test skill_stats agent_filter_scopes_bindings_but_single_runtime_is_global`：Claude-only
   binding 不成为 Codex zombie，单 agent binding 不标 single-runtime，多 agent flag 不受 window filter 误导。
3. `cargo test --test skill_stats zombie_cutoff_is_independent_from_since`：60 天前调用在宽 `--since`
   window 有 count，但 `zombie_days=30` 仍为 zombie；测试 clock 可注入。
4. `cargo test --test skill_stats disabled_with_history_is_not_empty`：disabled config 仍读取 persisted
   events，分别返回 enablement 与 emptiness。
5. `cargo test --test skill_stats orphan_and_error_threshold_contract`：durable unmatched 进入 orphan，
   样本 4 返回 null、样本 5 返回 rate；skill total、per-agent、orphan、agentless 均显式返回
   `failure_categories`，无数据时为 `{}`，且每层 `sum(failure_categories.values()) == error_count`。
6. `cargo test --test skill_stats error_events_count_as_recent_lifecycle_usage`：仅有近期 error event 的
   bound skill 保持 active，且 failure category 仍计数。
7. `cargo test --test skill_stats unbound_but_used_sort_is_stable`。
8. `cargo test --test skill_stats agentless_events_are_unfiltered_only && cargo test --test skill_stats orphans_share_since_and_agent_window`。
9. `cargo test --test skill_stats window_totals_reconcile_with_skill_and_orphan_attempts && cargo test --test skill_stats unbound_category_is_independent_from_since`。
10. `cargo test --test skill_stats stats_reads_one_locked_snapshot && cargo test aggregation_overflow_fails_without_partial_output`；后者通过 cfg(test) seeded accumulator unit seam 注入 near-`u64::MAX` 初值。
11. contract surface test + `cargo check --workspace --all-targets --all-features && cargo test`。

## 6. Rollback Plan

纯只读新增命令，revert 无状态影响。

## 7. Product Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | read-only stats command + normalized O(events) query | `cargo test --test skill_stats command_is_read_only_and_linear` |
| B-002 | telemetry config/log loading | `cargo test --test skill_stats disabled_with_history_is_not_empty` |
| B-003 | current snapshot binding/three usage projections + agentless policy | `cargo test --test skill_stats current_snapshot_ignores_stale_binding_observations && cargo test --test skill_stats agent_filter_scopes_bindings_but_single_runtime_is_global && cargo test --test skill_stats agentless_events_are_unfiltered_only` |
| B-004 | independent cutoff clock/view + error-as-usage semantics | `cargo test --test skill_stats zombie_cutoff_is_independent_from_since && cargo test --test skill_stats error_events_count_as_recent_lifecycle_usage` |
| B-005 | table-driven classifier | `cargo test --test skill_stats lifecycle_categories_are_exhaustive` |
| B-006 | normalized exact-window orphan aggregation + reconciliation | `cargo test --test skill_stats durable_unmatched_events_become_orphans && cargo test --test skill_stats orphans_share_since_and_agent_window && cargo test --test skill_stats window_totals_reconcile_with_skill_and_orphan_attempts` |
| B-007 | checked invocation/error/attempt aggregation | `cargo test --test skill_stats error_threshold_is_five && cargo test aggregation_overflow_fails_without_partial_output` |
| B-008 | four-category stable comparator | `cargo test --test skill_stats ordering_is_stable && cargo test --test skill_stats unbound_but_used_sort_is_stable` |
| B-009 | one locked snapshot + checked empty/malformed/error paths | `cargo test --test skill_stats stats_reads_one_locked_snapshot && cargo test aggregation_overflow_fails_without_partial_output && cargo test --test skill_stats empty_and_error_contracts_are_explicit` |

## 8. Planned Changes Manifest

```specrail-planned-changes
{"issue":542,"complete":true,"paths":["src/cli.rs","src/commands/mod.rs","src/commands/telemetry/mod.rs","src/commands/telemetry/query.rs","src/commands/skill_inventory.rs","src/commands/skill_stats.rs","docs/LOOM_CLI_CONTRACT.md","tests/skill_stats.rs"],"spec_refs":["specs/GH542/product.md#4-behavior-invariants","specs/GH542/tech.md#5-verification-plan","specs/GH541/tech.md#8-planned-changes-manifest"]}
```
