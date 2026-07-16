# GH537 Product Spec - Structured error contract hardening

Issue: https://github.com/majiayu000/loom/issues/537
Route: `write_spec`
State: `ready_to_implement`
Locale: `zh-CN`

## 1. Problem

三处削弱 agent 错误契约：启动/顶层失败绕过 envelope（裸 `eprintln!+exit(3)`，`--json` 下也无结构化输出）；仅 5/~27 个错误码有默认 `next_actions`；exit code 3 被 ~18 个语义不同的错误码复用，exit code 几乎无路由信号。

## 2. Goals

1. `--json` 模式下所有失败路径（含初始化失败）都产出合法 envelope。
2. 每个错误码要么有普适默认 `next_action`、由调用点提供上下文动作，要么有文档化的"无动作"理由。
3. 保持现有 exit code 分层，并由契约文档声明 `error.code` 是唯一稳定的语义路由键。

## 3. Non-Goals

1. 除初始化故障专用的 `INIT_ERROR` 外，不新增或重排其他错误码。
2. 不改变现有成功路径的 envelope 结构。
3. 不承诺 exit code 向后兼容（本仓无兼容层约定）。

## 4. Behavior Invariants

1. `--json` 下 stdout 上出现的失败输出必须是可解析的 envelope。
2. 人类模式（非 json）stderr 文案行为可保持。
3. next_actions 中的命令必须是可直接运行的 `loom … --json` 形式；只有不依赖调用参数的动作可作为 code-level default，需要 skill/target/ref 等上下文的动作必须由调用点提供或进入书面豁免。
4. 契约文档、错误码表、测试三者一致。

## 5. Acceptance Criteria

1. `loom --json <cmd>` 在 `App::new` 失败时 stdout 输出含错误码的 envelope，退出码非 0。
2. PROJECTION_CONFLICT / POLICY_BLOCKED / REMOTE_DIVERGED 等冲突/策略/远程类错误码有普适默认 next_actions、调用点上下文动作或书面豁免，且三者归属由 totality 表锁定。
3. `docs/LOOM_CLI_CONTRACT.md` 错误码表含 exit code 与 next_actions 覆盖情况。
4. 测试锁定初始化失败的 envelope 输出与 next_actions 覆盖表。

## 6. Edge Cases

1. CLI 解析失败早于 `--json` 标志可得（clap error 时机）。
2. panel 长驻进程失败与一次性命令失败的差异。
3. envelope 序列化自身失败的最终兜底。

## 7. Maintainer Decisions（2026-07-16）

1. 不重排 exit code；`error.code` 是唯一稳定的语义路由键，exit code 只保留粗粒度失败类别。
2. `App::new` 失败新增 `INIT_ERROR`（exit 3），JSON envelope 使用稳定 `cmd: "app.init"`；不得误用可恢复的 `STATE_NOT_INITIALIZED`。
3. code-level defaults 只放无参数、普适的动作：远程类使用 `loom sync status --json`，`LOCK_BUSY` 使用 `loom ops list --json`。
4. `POLICY_BLOCKED`、`PROJECTION_CONFLICT`、`REPLAY_CONFLICT`、`CAPTURE_CONFLICT` 等依赖调用上下文的错误由调用点提供具体 next_action，无法给出安全动作时进入文档豁免；不得拼造缺参数命令。
5. `IO_ERROR`、`GIT_ERROR`、`INTERNAL_ERROR`、`STATE_CORRUPT`、`SCHEMA_MISMATCH` 等纯故障类进入书面豁免清单。
