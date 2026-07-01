# GH368 Product Spec: Codex Visibility Doctor And Active-View Reconcile

Issue: https://github.com/majiayu000/loom/issues/368
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

让 Loom 能解释并修复 Codex skill active view 中最常见的不可见状态。核心原则是：

```text
projection exists != Codex can see this skill
```

`#368` 为 Codex 增加一个 visibility planner，并把它接入单 skill 诊断和 Codex active-view reconcile：

```bash
loom skill diagnose <skill> --agent codex
loom skill visibility <skill> --agent codex [--workspace <path>] [--profile <id>] [--json]
loom codex reconcile --dry-run [--binding <binding-id>] [--target <target-id>] [--allowlist <path>]
loom codex reconcile --apply [--binding <binding-id>] [--target <target-id>]
loom codex reconcile --apply --fix-config [--binding <binding-id>] [--target <target-id>]
```

If a future command names this surface `skill doctor`, it should be an alias or wrapper over the same visibility planner rather than a separate implementation.

## Users

1. Codex user: projected a skill but Codex does not show or trigger it.
2. Maintainer: needs a deterministic dry-run plan before removing stale active-view projections or touching Codex config.
3. Agent: needs machine-readable checks that separate source, active rule, projection, symlink, config disable, runtime entries, and restart requirement.

## Scope For First Implementation

This issue should deliver the Codex visibility foundation:

1. A pure planner that joins Loom registry state, Codex target filesystem entries, symlink canonicalization, and Codex config `skills.config`.
2. `loom skill diagnose <skill> --agent codex` or `skill visibility` output for a single skill.
3. `loom codex reconcile --dry-run` plan output with no mutation.
4. `loom codex reconcile --apply` for safe projection drift repair and stale Loom-owned symlink cleanup.
5. `loom codex reconcile --apply --fix-config` for narrowly-scoped config repair.
6. Restart/new-session recommendation after config or active-view changes.

## Non-Goals

1. 不实现 general adapter v2 discovery roots；#373 owns adapter schema expansion.
2. 不实现 `skill activate` / `deactivate` high-level lifecycle commands；#367 owns them.
3. 不扫描或修复 Claude、Cursor、Windsurf 等非 Codex agent config。
4. 不删除 non-Loom filesystem entries。
5. 不默认信任 legacy full-mirror Codex rules；migration cleanup must be dry-run/allowlist driven.
6. 不绕过 policy、安全、ownership 或 symlink safety gates。
7. 不声称当前 Codex session 已热更新；修改后只能建议 restart/new session，除非有实际 runtime proof.

## Behavior Invariants

1. `--dry-run` is the default planning mode for reconcile and must be fully read-only.
2. `--apply` may repair only Loom-owned projection drift and stale Loom-owned symlinks.
3. `--apply` without `--fix-config` must not edit Codex config.
4. `--apply --fix-config` may edit only safe active-skill config disables that the planner can prove are Loom-owned or explicitly selected.
5. User-authored disables are preserved unless a future command adds explicit per-entry confirmation.
6. Config writes must be atomic: read, parse, patch, write temp file, parse temp file, rename.
7. Malformed Codex config must produce an error finding and block `--fix-config`; it must not be silently ignored.
8. Runtime entries such as `.system` and `codex-primary-runtime` are preserved.
9. External entries under Codex skill roots are reported and preserved.
10. Symlink canonicalization must compare canonical source skill directories and canonical `SKILL.md` paths.
11. Multiple active bindings sharing one Codex target must be reconciled as a union; one binding must not delete another binding's active projection.
12. Every mutating action must be represented in the dry-run plan before apply.

## Visibility Checks

For one skill, the planner should emit checks like:

```json
{
  "skill": "fixflow",
  "agent": "codex",
  "visible": false,
  "checks": [
    {
      "id": "codex_source_exists",
      "ok": true,
      "severity": "info",
      "message": "source skill exists"
    },
    {
      "id": "codex_projection_points_to_source",
      "ok": true,
      "severity": "info",
      "message": "projection symlink resolves to source skill"
    },
    {
      "id": "codex_config_not_disabled_by_path",
      "ok": false,
      "severity": "error",
      "message": "canonical SKILL.md is disabled in Codex config",
      "details": {
        "config_path": "/Users/me/.codex/config.toml",
        "disabled_path": "/Users/me/.loom-registry/skills/fixflow/SKILL.md"
      },
      "next_action": "loom codex reconcile --apply --fix-config, then restart Codex"
    }
  ],
  "next_actions": [
    "loom codex reconcile --apply --fix-config",
    "restart Codex or open a new session"
  ]
}
```

## Reconcile Plan

Plan categories:

```text
create_projection
repair_projection
remove_stale_projection
remove_stale_record
preserve_runtime_entry
preserve_external_entry
fix_config_disable
legacy_rule_remove
manual_review
```

Required plan shape:

```json
{
  "agent": "codex",
  "binding_id": "bind_codex_default",
  "target_id": "target_codex_default",
  "target_path": "/Users/me/.agents/skills",
  "dry_run": true,
  "safe_to_apply": false,
  "actions": [
    {
      "category": "fix_config_disable",
      "skill": "fixflow",
      "safe": true,
      "requires_fix_config": true,
      "reason": "active skill disabled by Codex config"
    }
  ],
  "warnings": [
    "restart_required_after_apply"
  ]
}
```

## Config Repair Policy

Codex config repair is allowed only when all are true:

1. `--apply --fix-config` is provided.
2. The config parses successfully.
3. The disabled entry matches an active Loom-managed Codex skill by canonical path or exact skill name.
4. The entry is safe to remove or flip because it is Loom-generated or explicitly targeted by the selected active skill.
5. The patched TOML parses before replace.

If these conditions are not met, the planner reports `manual_review`.

## Acceptance Criteria

1. `loom skill diagnose <skill> --agent codex` or `loom skill visibility <skill> --agent codex --json` explains wrong target path, missing projection, broken symlink, path disable, name disable, missing `SKILL.md`, runtime entry, external entry, and restart requirement when applicable.
2. `loom codex reconcile --dry-run` reports all proposed projection/config actions and mutates nothing.
3. `loom codex reconcile --apply` repairs safe projection drift and stale Loom-owned symlinks but does not edit config.
4. `loom codex reconcile --apply --fix-config` repairs only safe active skill disables and reports `restart_required=true`.
5. Runtime entries and non-Loom entries are preserved.
6. Multiple bindings sharing the same Codex target are reconciled as a union of desired active skills.
7. Malformed config blocks config repair and returns a typed error finding.
8. Tests cover user root, project root, legacy root when supported, symlink canonicalization, path disable, name disable, malformed TOML, runtime entries, external entries, dry-run read-only behavior, apply projection repair, and fix-config atomic write.
