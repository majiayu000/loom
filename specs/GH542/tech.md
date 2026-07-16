# GH542 Tech Spec - skill stats lifecycle governance report

Issue: https://github.com/majiayu000/loom/issues/542
Product spec: `specs/GH542/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation
Depends on: GH541

## 1. Current Behavior

- `loom telemetry report`（`src/commands/telemetry/`）按事件流聚合，不 join registry；`loom doctor` 检查完整性/投影，不看用量。
- skill 读模型来自 `build_skill_read_model`（`src/commands/mod.rs` 系列 helper），绑定/投影观测在 `state/registry/observations/*.jsonl`。
- 事件存储 `state/telemetry/events.jsonl`，已有解析容错（`TelemetryLog`）。

## 2. Proposed Design

1. 新增 CLI：`src/cli/` skill 命令组挂 `Stats(SkillStatsArgs)`（`--since`、`--agent`、`--zombie-days` 默认 30、全局 `--json`）。
2. 新增 `src/commands/skill_stats.rs`：
   - 载入 skill 读模型 + 绑定观测 → 每 skill 的 bound agents 集合。
   - 单遍扫 `TelemetryLog.events`，过滤 `--since`/`--agent`，按 (skill, agent) 聚合 count/last_used/error/failure_category。
   - 分类器（互斥完备）：
     - registry skill：has_bindings × has_usage(window) → active / zombie / unbound-unused / unbound-but-used（绑定缺失但有事件，提示绑定漂移）。
     - single-runtime flag：bound_agents ≥ 2 且 used_agents == 1（叠加在 active 上的标记，不是独立类别）。
     - orphan：事件 skill 名不在读模型 → 单独列表。
   - 错误率：invocation+error 事件数 ≥ 5 才输出（门槛常量，Open Question 2 定案后调整）。
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
  "telemetry_empty": false,
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
2. `--since` 晚于全部事件 → `window_events=0` 且全部退化分类正确。
3. 过滤参数测试（`--agent`、`--zombie-days`）。
4. contract surface 测试 + `cargo check && cargo test`。

## 6. Rollback Plan

纯只读新增命令，revert 无状态影响。

## 7. Product Mapping

1. Goal 3 分类完备互斥 → 分类器表驱动测试。
2. Invariant 3（telemetry 空不报错）→ 空事件 fixture 测试。
3. Goal 4（稳定 envelope）→ contract doc + surface 测试。
4. Invariant 1/2（只读、O(events)）→ 实现约束，代码评审确认无写路径。
