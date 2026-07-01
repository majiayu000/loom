# GH366 Product Spec: Single-Skill Inspect And Status Read Model

Issue: https://github.com/majiayu000/loom/issues/366
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

让用户可以通过一个只读命令查看单个 skill 的完整生命周期状态：

```bash
loom skill inspect <skill> [--agent <agent>] [--workspace <path>] [--profile <id>] [--json]
```

`skill inspect` 应该把现有分散信息收敛成一个 canonical read model：source、spec/lint、provenance、runtime/projection、quality、safety 和 next actions。它不替代低层 `skill show`、`skill diagnose`、`workspace doctor`、`skill lint`、`skill verify`、`skill policy`、`skill eval`，而是复用这些已经存在或已计划的信号，给用户一个单 skill 状态卡。

## Users

1. 单人用户：想知道某个 skill 是否存在、是否 lint-clean、是否已经投影到目标 agent、下一步该做什么。
2. 维护者：需要一个稳定 JSON read model 给 Panel 和后续 `skillset`、activation、visibility、eval、safety 功能复用。
3. Agent：需要用只读命令判断后续是否应该 lint、activate、doctor、eval、scan 或修复 projection drift。

## Scope For First PR

本 issue 的第一批实现应覆盖 `#366` 的核心 inspect/status 行为：

1. 新增 `loom skill inspect <skill>` CLI。
2. 输出稳定 JSON status model，并在非 JSON 模式打印紧凑状态卡。
3. 复用现有 inventory、lint、verify/provenance、registry snapshot、binding、target、projection 信号。
4. 支持 `--agent`、`--workspace`、`--profile` 过滤当前可判断的 runtime sections。
5. 对不可判断的 agent-specific visibility 显式返回 `unknown` / `not_checked`，不能把 projection 存在静默等同于 visible。
6. 输出 actionable `next_actions`。

## Non-Goals

1. 不做任何 registry、target、agent config 或 filesystem mutation。
2. 不实现 `skill activate`、`skill deactivate`、`skill active list`；这些属于 #367。
3. 不实现 Codex config disable 解析、reconcile apply、`--fix-config`；这些属于 #368。
4. 不升级 adapter schema 到 discovery roots / reload metadata v2；这些属于 #373。
5. 不新增 eval runner、safety scan、trust/quarantine policy；这些分别属于 #369 和 #370。
6. 不把 missing eval 或 missing safety evidence 伪装成 pass；无数据必须显示为空或 `unknown`。

## Behavior Invariants

1. `inspect` 是 read-only；它不能写 registry state、projection target、agent config、pending queue 或 skill source。
2. 缺失 skill 返回 typed `SKILL_NOT_FOUND` style error。
3. `source` 区分 registry source 是否存在、entrypoint 是否存在、working tree drift、last source commit、head tree oid。
4. `spec` 区分 portable lint、agent compatibility lint 和 findings；lint error 不应被吞成 warning。
5. `runtime` 区分 `installed_in_registry`、`active_rule_present`、`projected_to_target`、`materialized_path_exists`、`visible_to_agent`、`enabled_by_agent_config`、`restart_required`。
6. 当前只能通过通用 registry/projection 信号判断的字段必须标记 truth level；agent config 或 reload 语义缺失时返回 `unknown`，并给出 next action。
7. `--agent` 只过滤或聚焦指定 agent，不应隐藏 source/spec/safety/quality 顶层状态。
8. `--workspace` 只影响 workspace binding / project scope 解析，不能改变状态。
9. `--profile` 只影响 profile-specific binding/target 匹配，不能改变状态。
10. `next_actions` 必须从当前状态派生，且命令文本必须是可执行的 Loom CLI 命令或明确的人类动作。

## JSON Output Expectations

Required top-level shape:

```json
{
  "skill": "fixflow",
  "source": {
    "path": "/Users/me/.loom-registry/skills/fixflow",
    "exists": true,
    "entrypoint": "SKILL.md",
    "entrypoint_exists": true,
    "working_tree_drift": false,
    "head_tree_oid": "...",
    "last_source_commit": "..."
  },
  "spec": {
    "portable": "pass",
    "codex": "pass",
    "claude": "warning",
    "findings": []
  },
  "provenance": {
    "source": null,
    "pinned_ref": null,
    "verified": null,
    "drift": null
  },
  "runtime": {
    "codex": {
      "installed_in_registry": true,
      "active_rule_present": true,
      "projected_to_target": true,
      "materialized_path_exists": true,
      "visible_to_agent": "unknown",
      "enabled_by_agent_config": "unknown",
      "restart_required": "unknown",
      "target_path": "/Users/me/.agents/skills",
      "materialized_path": "/Users/me/.agents/skills/fixflow",
      "truth_level": "registry_projection"
    }
  },
  "quality": {
    "last_eval": null,
    "trigger_precision": null,
    "trigger_recall": null,
    "baseline_delta": null
  },
  "safety": {
    "trust": "unknown",
    "policy": "unknown",
    "scripts_present": null,
    "network_requested": null,
    "quarantined": null
  },
  "next_actions": [
    "loom skill doctor fixflow --agent codex",
    "loom skill eval fixflow --agent codex --baseline no-skill"
  ]
}
```

## Human Output

Non-JSON output should be compact and scannable:

```text
fixflow
Source:   present, clean
Spec:     portable pass, codex pass, claude warning
Runtime:  codex projected, visibility unknown
Quality:  no eval evidence
Safety:   unknown
Next:     loom skill doctor fixflow --agent codex
```

## Acceptance Criteria

1. `loom skill inspect <skill> --json` returns the status model above with stable keys.
2. Missing skills return a typed `SKILL_NOT_FOUND` error and do not create files.
3. Existing registry skills with no projections show `installed_in_registry=true` and no active runtime projection.
4. Skills with binding rules and projections show separate `active_rule_present`, `projected_to_target`, and `materialized_path_exists` states.
5. Projected but generically invalid states explain detectable reasons such as missing materialized path, missing source, broken symlink, or target/agent mismatch.
6. Agent-specific visibility that requires #368/#373 metadata is `unknown` with a next action, not silently `true`.
7. `--agent codex` and `--agent claude` focus runtime output for that agent while preserving source/spec top-level status.
8. Output includes actionable `next_actions` for missing lint, missing projection, unknown visibility, eval absence, and safety absence.
9. Focused tests cover clean source, missing source, existing skill with no projections, projected source, stale/missing projection, broken symlink, agent filter behavior, and read-only behavior.
