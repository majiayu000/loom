# GH497 Product Spec: Projection Executor Convergence

Issue: https://github.com/majiayu000/loom/issues/497
Status: Draft for implementation
Locale: zh-CN

## Goal

让“把 skill 投射到 agent 目录”只有一个生产执行器负责目标、binding、projection、safety gate、digest/observation seeding 和 rollback。`use --apply`、`skill project`、`skill activate` 和 `plan apply` 必须共享同一套投射语义，避免同一意图因为入口不同而产生不同保障。

## Users

1. Agent wrapper 作者：需要选择 `use --apply` 或 `skill activate` 时得到等价的 registry state 和 live projection。
2. Skill 维护者：需要投射路径的 safety、digest、observation 和 rollback 行为保持一致。
3. Panel / workflow 调用方：需要 `plan apply` 和直接命令写入同样的状态模型，便于审计和恢复。

## Non-Goals

1. 不改变现有 CLI 参数或 JSON envelope 顶层契约。
2. 不重写 compiled activation 的 artifact 选择和验证。
3. 不新增 agent adapter 行为。
4. 不更改 projection storage schema。
5. 不把 `skill deactivate` 扩展到 copy/materialize 删除。

## Behavior Invariants

1. `skill project` 仍然要求已有 binding/target，并返回 `projection`、`backup`、`commit`、`noop`。
2. `skill activate` 仍然可以创建缺失 target 和 binding，并返回 `plan`、`projection`、`target`、`binding`、`commit`、`noop`。
3. `use --apply` 和 `plan apply` 继续通过 target add/adopt、binding add、project 的组合入口输出现有 `applied` 数组。
4. safety policy 只允许在投射前通过；失败时 live target、registry state、operation log、observation log 不应留下半写状态。
5. symlink physical probe 在 destructive replace 前执行。
6. copy/materialize projection 必须记录 source/materialized digest 和 observation state。
7. repeated activation of an already safe projection must be a no-op.
8. projection conflict 必须 fail loudly；不能用 warning 或 fallback 覆盖已有未知目录。

## Acceptance Criteria

1. 单一公共执行器负责 target/rule/projection state construction、materialization、observation seeding、state save、operation record、observation record、commit 和 rollback。
2. `skill project` 和 `skill activate` 均调用该执行器；`use --apply` 与 `plan apply` 通过 `cmd_project` 复用它。
3. Tests 证明同一 skill/agent/workspace/method 通过 `use --apply` 与 `skill activate` 得到等价 target、binding、rule、projection、digest 和 live file state。
4. Tests 证明 `skill project --method copy` 与 `skill activate --method copy` 记录等价 digest/observation evidence。
5. Existing CLI JSON contracts 保持兼容；任何新增字段只能是 additive。
6. Existing project/use/activate/plan tests 继续通过。

## Done When

- `cargo test --test use_flow_cli`
- `cargo test --test skill_activation`
- `cargo test --test project`
- `cargo test --test agent_plan_apply`
- `cargo test --workspace --all-features`
