# GH377 Product Spec: Skillsets And Bundles

Issue: https://github.com/majiayu000/loom/issues/377
Parent: https://github.com/majiayu000/loom/issues/376
Status: Draft for implementation
Locale: zh-CN

## Goal

让用户可以把多个已存在的 registry skills 组织成一个 `skillset`，并先完成可审计、可查询、可校验的基础生命周期：

```text
create -> add/remove members -> show -> lint
```

第一阶段不承诺 activation、eval、release、rollback 的完整行为，因为 #377 明确被 #367、#369、#370 阻塞。

## Users

1. 单人用户：希望把常用的一组 skills 保存成一个命名集合。
2. 维护者：希望先验证一个 skillset 的成员是否存在、是否重复、required 成员是否完整。
3. 后续高级功能：#378 recommendation、#379 workflow、#385 telemetry 需要一个稳定的 skillset 数据基础。

## Non-Goals

1. 不实现 marketplace 或 catalog 依赖。
2. 不实现 DAG workflow 编排。
3. 不实现 semantic recommendation。
4. 不在本 slice 中实现 `skillset activate` 的真实写入。
5. 不在本 slice 中实现 `skillset eval`、`release`、`rollback`。
6. 不复制 `skill inspect` / activation / trust / eval 的状态逻辑；这些必须等 #366/#367/#369/#370 后复用。

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
13. 本 slice 中任何 activation/eval/release/rollback 命令如暴露，也必须清楚返回 not implemented / blocked，而不是静默执行部分行为。

## User-Facing CLI

Required for this slice:

```bash
loom skillset create <name> [--description <text>]
loom skillset add <name> <skill> [--role <role>] [--required|--optional]
loom skillset remove <name> <skill>
loom skillset show <name> [--json]
loom skillset lint <name> [--json]
```

Deferred:

```bash
loom skillset activate ...
loom skillset deactivate ...
loom skillset eval ...
loom skillset release ...
loom skillset rollback ...
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
10. Focused tests cover create/add/remove/show/lint, duplicate create, duplicate member, missing member, empty lint, and missing skill drift.

## Open Questions

1. Whether `skillset` should be a top-level CLI command forever or later alias into `loom skill set`.
2. Whether activation defaults belong in this first state file or should wait for #367.
3. Whether role vocabulary should remain free-form in v1 or become constrained after real workflow use.
