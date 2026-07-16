# GH535 Product Spec - Skill command surface convergence

Issue: https://github.com/majiayu000/loom/issues/535
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

`skill` 分组下 44 个子命令（`bb9b738`；审计时为 42，仍在增长）混杂运维（list/inspect/activate/diagnose）与 authoring（draft/extract/rewrite/tune-description/generate-evals/apply-patch）两种人格。对 agent 是最大的剩余人体工学成本：单次选择空间过宽且异质，预算 gate 只冻结增长不降规模。

## 2. Goals

1. 运维与 authoring 命令按人格分组，agent 单次选择空间显著收窄。
2. 分组后 `skill` 子命令预算明显低于 44。
3. envelope `cmd` 值与错误 `next_actions` 跟随新路径。
4. 契约文档、README、预算测试在同一变更内更新。

## 3. Non-Goals

1. 不保留旧路径兼容别名（直接删除，同 release 完成迁移）。
2. 不改变各命令自身的行为语义。
3. 不在本 issue 内新增或删除命令能力。

## 4. Behavior Invariants

1. 每个现存子命令在新分组中有且只有一个归属。
2. 被移除的旧路径进入 `tests/cli_surface.rs` 僵尸命令黑名单。
3. `--json` envelope 的 `cmd` 字段与新 CLI 路径一致。
4. 分组决策（顶层 `author` vs `skill author` 二级）由 maintainer 显式选定。

## 5. Acceptance Criteria

1. `loom skill --help` 只含运维命令，authoring 命令在新分组下可用。
2. `tests/cli_surface.rs` 预算断言更新为新数值并通过。
3. `docs/LOOM_CLI_CONTRACT.md` 与 README 命令表与实际 help 输出一致。
4. 旧路径调用返回标准 clap unknown-command 错误。

## 6. Edge Cases

1. 同名子命令在两组间语义歧义（如 `inspect`）。
2. 脚本/skills 内部互相调用旧路径（需全库 grep 收敛）。
3. panel 端引用的命令路径同步更新。

## 7. Open Questions

1. 顶层 `loom author …` 还是二级 `loom skill author …`？
2. `compile`/`eval` 类介于两种人格之间的命令归属哪组？
