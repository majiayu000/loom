# GH367 Product Spec: Single-Skill Activate, Deactivate, And Active List

Issue: https://github.com/majiayu000/loom/issues/367
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

让用户可以用生命周期语言管理一个 skill 的 active state，而不必手动理解 target、binding、rule、projection 的低层组合：

```bash
loom skill activate <skill> --agent <agent> [--scope user|project] [--workspace <path>] [--profile <id>] [--target <target-id>] [--method symlink|copy|materialize] [--dry-run]
loom skill deactivate <skill> --agent <agent> [--scope user|project] [--workspace <path>] [--profile <id>] [--target <target-id>] [--dry-run]
loom skill active list --agent <agent> [--scope user|project] [--workspace <path>] [--profile <id>] [--json]
```

`activate` / `deactivate` 是高层 UX；底层仍应复用 Loom 现有 registry state、managed target、binding rule、projection、policy、audit、rollback 机制。

## Users

1. 单人用户：希望一条命令把某个 skill 激活给 Codex 或 Claude，而不是先创建 target 和 binding。
2. 维护者：需要 active desired state 和 actual filesystem projection 可以被 `skill inspect`、`workspace doctor`、Panel 复用。
3. Agent：需要先 dry-run，再 apply，且可以安全重试、解释失败和恢复。

## Scope For First PR

本 issue 的第一批实现应覆盖：

1. `loom skill activate` 生成并可执行单 skill activation plan。
2. `loom skill deactivate` 移除 desired active rule，并安全移除 Loom-owned projection。
3. `loom skill active list` 显示 desired active rules、actual projections 和 health。
4. `--dry-run` 对 activate/deactivate 完全只读。
5. user/project scope 使用当前可判断的 agent 默认 skill root；更完整的 discovery roots 和 reload metadata 由 #373 扩展。
6. Codex config disable、reconcile apply、`--fix-config` 由 #368 负责；本 issue 只能报告 restart/new-session recommendation 或 visibility unknown。

## Non-Goals

1. 不编辑 Codex、Claude 或其他 agent config。
2. 不实现 Codex visibility doctor 或 active-view reconcile；这些属于 #368。
3. 不升级 adapter schema；discovery root v2 属于 #373。
4. 不实现 trust/quarantine/safety scan；这些属于 #370，但 activation 必须保留现有 policy gate。
5. 不实现 eval gating；#369 只会在后续把 eval evidence 接入 inspect/activation policy。
6. 不默认删除 copy/materialize projection 内容，除非实现了明确的 safe backup/capture path。

## Behavior Invariants

1. `activate --dry-run` 和 `deactivate --dry-run` 不能修改 registry state、target files、skill source、pending queue 或 agent config。
2. `activate` 必须验证 skill source 存在，并至少通过兼容 lint / policy gate。
3. Mutating activate 只能写 managed targets；observed/external targets fail closed。
4. Target path 必须是 agent 会扫描的 known root 或用户显式给定的 managed target；不能把 arbitrary path 当作 visible。
5. Re-running activate is idempotent: healthy active state returns noop or repair-only plan.
6. Missing projection with active rule should be repaired when safe.
7. `deactivate` removes desired active rule first and removes only Loom-owned projection for the selected activation.
8. Symlink deactivation may delete the symlink only when it resolves to the canonical registry source.
9. Copy/materialize deactivation must fail closed unless a safe capture/backup recovery path is implemented in the same PR.
10. Canonical registry skill source must never be deleted.
11. Runtime/system entries such as `.system` and external non-Loom target entries must be preserved.
12. Activation output must not claim `visible=true` without agent-specific proof from #368/#373.

## Activation Plan

`activate --dry-run` should return the same plan shape that `activate` applies:

```json
{
  "skill": "fixflow",
  "agent": "codex",
  "scope": "user",
  "dry_run": true,
  "safe_to_apply": true,
  "actions": [
    {
      "id": "ensure_target",
      "action": "create_or_reuse_managed_target",
      "target_path": "/Users/me/.agents/skills"
    },
    {
      "id": "ensure_active_rule",
      "action": "upsert_active_rule",
      "method": "symlink"
    },
    {
      "id": "ensure_projection",
      "action": "create_or_repair_projection",
      "materialized_path": "/Users/me/.agents/skills/fixflow"
    }
  ],
  "warnings": [
    "restart_or_new_session_may_be_required"
  ]
}
```

## Output Expectations

Successful activate:

```json
{
  "skill": "fixflow",
  "agent": "codex",
  "scope": "user",
  "active": true,
  "noop": false,
  "target_id": "codex_user_agents_skills",
  "binding_id": "active_codex_user_default",
  "projection": {
    "method": "symlink",
    "materialized_path": "/Users/me/.agents/skills/fixflow",
    "health": "healthy"
  },
  "visibility": {
    "visible": "unknown",
    "enabled": "unknown",
    "restart_required": true
  },
  "next_actions": [
    "restart Codex or open a new session",
    "loom skill inspect fixflow --agent codex"
  ]
}
```

`active list` should distinguish desired state from actual state:

```json
{
  "agent": "codex",
  "scope": "user",
  "skills": [
    {
      "skill": "fixflow",
      "desired_active": true,
      "projection_present": true,
      "status": "healthy",
      "target_path": "/Users/me/.agents/skills",
      "materialized_path": "/Users/me/.agents/skills/fixflow"
    }
  ]
}
```

## Acceptance Criteria

1. `activate --dry-run` returns a complete plan without mutating registry state or target files.
2. `activate` makes one existing skill active in a managed target without requiring manual target/binding IDs.
3. Re-running `activate` is idempotent and repairs a missing safe projection when the active rule already exists.
4. `deactivate --dry-run` returns the removal plan without mutation.
5. `deactivate` removes the Loom-owned active rule and symlink projection while preserving the registry source.
6. `deactivate` fails closed for copy/materialize projections unless safe recovery is implemented.
7. `active list` distinguishes desired active rules from actual filesystem projection state.
8. Observed/external targets and non-Loom filesystem entries are preserved.
9. Output does not claim true agent visibility unless backed by agent-specific proof.
10. Tests cover symlink activation, idempotency, missing projection repair, deactivation, observed target failure, copy/materialize fail-closed behavior, dry-run read-only behavior, and active list status.
