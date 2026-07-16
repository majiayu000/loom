# GH537 Product Spec - Structured error contract hardening

Issue: https://github.com/majiayu000/loom/issues/537
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

三处削弱 agent 错误契约：启动/顶层失败绕过 envelope（裸 `eprintln!+exit(3)`，`--json` 下也无结构化输出）；仅 5/~27 个错误码有默认 `next_actions`；exit code 3 被 ~18 个语义不同的错误码复用，exit code 几乎无路由信号。

## 2. Goals

1. `--json` 模式下所有失败路径（含初始化失败）都产出合法 envelope。
2. 每个错误码要么有默认 `next_action`，要么有文档化的"无动作"理由。
3. exit code 语义要么重新分层，要么契约文档明确声明 `error.code` 是唯一路由键。

## 3. Non-Goals

1. 不新增错误码。
2. 不改变现有成功路径的 envelope 结构。
3. 不承诺 exit code 向后兼容（本仓无兼容层约定）。

## 4. Behavior Invariants

1. `--json` 下 stdout 上出现的失败输出必须是可解析的 envelope。
2. 人类模式（非 json）stderr 文案行为可保持。
3. next_actions 中的命令必须是可直接运行的 `loom … --json` 形式（沿用现有测试约束）。
4. 契约文档、错误码表、测试三者一致。

## 5. Acceptance Criteria

1. `loom --json <cmd>` 在 `App::new` 失败时 stdout 输出含错误码的 envelope，退出码非 0。
2. PROJECTION_CONFLICT / POLICY_BLOCKED / REMOTE_DIVERGED 等冲突/策略/远程类错误码有默认 next_actions 或书面豁免。
3. `docs/LOOM_CLI_CONTRACT.md` 错误码表含 exit code 与 next_actions 覆盖情况。
4. 测试锁定初始化失败的 envelope 输出与 next_actions 覆盖表。

## 6. Edge Cases

1. CLI 解析失败早于 `--json` 标志可得（clap error 时机）。
2. panel 长驻进程失败与一次性命令失败的差异。
3. envelope 序列化自身失败的最终兜底。

## 7. Open Questions

1. 初始化失败用现有错误码（如 STATE_NOT_INITIALIZED）还是新增 INIT_ERROR？
2. exit code 重新分层是否值得破坏现有脚本（倾向：文档声明 error.code 为唯一路由键，成本最低）？
