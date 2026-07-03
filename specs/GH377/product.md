# GH377 Product Spec: Skillsets And Bundles

Issue: https://github.com/majiayu000/loom/issues/377
Parent: https://github.com/majiayu000/loom/issues/376
Status: Implementation update for lifecycle completion PR
Locale: zh-CN

## Goal

让用户可以把多个已存在的 registry skills 组织成一个 `skillset`，并完成可审计、可查询、可校验的基础生命周期与收尾生命周期：

```text
create -> add/remove members -> show -> lint -> activate/deactivate -> eval aggregation -> release -> rollback
```

本 PR 补齐 `activate`/`deactivate`、成员 eval 聚合、skillset definition release/rollback，以及 partial activation failure 的 best-effort rollback/recovery reporting。

## Users

1. 单人用户：希望把常用的一组 skills 保存成一个命名集合。
2. 维护者：希望先验证一个 skillset 的成员是否存在、是否重复、required 成员是否完整。
3. 后续高级功能：#378 recommendation、#379 workflow、#385 telemetry 需要一个稳定的 skillset 数据基础。

## Non-Goals

1. 不实现 marketplace 或 catalog 依赖。
2. 不实现 DAG workflow 编排。
3. 不实现 semantic recommendation。
4. 不实现 `skillsets/<name>/evals/` 的端到端 runner；当前 `skillset eval` 聚合成员 skill eval 结果，并在检测到端到端 fixtures 时返回 deferred 状态。
5. 不复制单 skill activation / trust / eval 的状态逻辑；skillset lifecycle 必须复用现有单 skill 路径。

## Behavior Invariants

1. 用户可以创建一个空 skillset，并提供可选 description。
2. skillset id 必须使用 Loom 已有 skill name 风格：lowercase alphanumeric plus hyphen。
3. 创建已存在的 skillset 必须失败，不能覆盖。
4. 用户可以把已存在的 registry/source skill 加入 skillset。
5. `skillset add` 必须拒绝不存在的 skill，避免产生不可解析成员。
6. 同一个 skill 在同一个 skillset 中只能出现一次。
7. member role 是可选文本；空 role 在输出中表现为 `null`。
8. member required 默认是 `true`；用户可以显式标记 optional。
9. 用户可以从 skillset 删除 member；删除不存在的 member 必须返回 typed error。
10. `skillset show` 必须返回成员列表，并包含每个成员的基础 skill read-model summary。
11. `skillset lint` 必须验证 member existence、重复成员、空集合 warning、required/optional 计数。
12. 所有写入必须进入 registry state，并保持 Git/audit 习惯：可追踪、可回滚、不会写 agent target。
13. `skillset activate --dry-run` 必须先调用单 skill activation dry-run 形成逐成员计划，且不写 registry 或 target。
14. `skillset activate` 必须复用单 skill activation 路径逐成员执行；required member 缺失、blocked、quarantined、lint/safety invalid 必须 typed error fail closed。
15. partial activation failure 必须 best-effort 回滚已激活成员；如果回滚不完整，必须返回精确 recovery commands，不能静默降级。
16. `skillset deactivate` 必须复用单 skill deactivation 路径逐成员执行。
17. `skillset eval` 必须聚合 member eval pass/fail/case counts，并保留 baseline 标签 `no-skill` 或 `single-skills`。
18. `skillset release` 必须创建 skillset definition tag `release/skillset/<name>/<version>`，且 invalid skillset 不可 release。
19. `skillset rollback --to <version|ref>` 必须只恢复目标 skillset definition，不回滚 member skill source。

## User-Facing CLI

```bash
loom skillset create <name> [--description <text>]
loom skillset add <name> <skill> [--role <role>] [--required|--optional]
loom skillset remove <name> <skill>
loom skillset show <name> [--json]
loom skillset lint <name> [--json]
loom skillset activate <name> --agent <agent> [--scope user|project] [--workspace <path>] [--profile <id>] [--dry-run]
loom skillset deactivate <name> --agent <agent> [--scope user|project] [--workspace <path>] [--profile <id>] [--dry-run]
loom skillset eval <name> --agent <agent> [--baseline no-skill|single-skills]
loom skillset release <name> <version>
loom skillset rollback <name> --to <version|ref>
```

## JSON Output Expectations

`skillset show` should return:

```json
{
  "skillset": {
    "id": "coding-flow",
    "description": "Skills for coding tasks.",
    "members": [
      {
        "skill_id": "fixflow",
        "role": "execution",
        "required": true,
        "skill": {}
      }
    ],
    "summary": {
      "members": 1,
      "required": 1,
      "optional": 0,
      "missing": 0
    }
  }
}
```

`skillset lint` should return:

```json
{
  "skillset": "coding-flow",
  "valid": true,
  "summary": {
    "members": 1,
    "required": 1,
    "optional": 0,
    "missing": 0,
    "duplicates": 0
  },
  "findings": []
}
```

## Acceptance Criteria

1. `loom skillset create coding-flow --description ...` creates a persisted skillset.
2. Creating the same skillset twice returns a typed error and does not mutate state.
3. `loom skillset add coding-flow fixflow --role execution` adds one required member.
4. `loom skillset add` rejects missing skills.
5. `loom skillset remove coding-flow fixflow` removes the member and preserves the skill source.
6. `loom skillset show coding-flow --json` includes member summary from the current skill read model.
7. `loom skillset lint coding-flow --json` reports valid status, member counts, and findings.
8. Empty skillsets lint with warning but not fatal invalidity.
9. Required missing members lint invalid if state drift is detected.
10. `skillset activate --dry-run` returns a full per-member plan without projecting target files.
11. `skillset activate` projects ready members through the single skill activation path.
12. Required member activation failures fail closed with typed errors; partial activation failures rollback or report recovery commands.
13. `skillset eval` aggregates member eval cases, pass/fail counts, and aggregate score.
14. `skillset release` creates a `release/skillset/<name>/<version>` tag for the current skillset definition.
15. `skillset rollback --to <version|ref>` restores that skillset definition without changing member skill source.
16. Focused tests cover create/add/remove/show/lint, duplicate create, duplicate member, missing member, empty lint, missing skill drift, dry-run activation, successful activation, partial failure rollback, eval aggregation, and release/rollback.

## Open Questions

1. Whether `skillset` should be a top-level CLI command forever or later alias into `loom skill set`.
2. Whether role vocabulary should remain free-form in v1 or become constrained after real workflow use.
3. Whether `skillsets/<name>/evals/` should use a dedicated end-to-end runner or be represented as a generated workflow/eval harness in a follow-up issue.
