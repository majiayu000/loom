# GH536 Product Spec - Adapter differentiation or fidelity tiers

Issue: https://github.com/majiayu000/loom/issues/536
Route: `write_spec`
State: `triaged`
Locale: `zh-CN`

## 1. Problem

10 个内置 agent adapter 中只有 codex/claude 有实质差异化（专属 discovery roots、config 可见性、运行时探测）；其余 8 个全部回落 `legacy-default` 通用根与默认 visibility，reload 维度即使 claude/codex 也只有文案差异。通用数据被呈现得像已验证的 agent 专属行为。

## 2. Goals

1. 每个内置 agent 的 adapter 数据保真度（fidelity）可观测：`verified` 或 `generic` 显式分层。
2. 有专属研究结论的 agent（逐个补齐）获得真实的 discovery/visibility/reload 差异化。
3. `loom agent` 输出与 doctor/diagnose 判定不再把 generic 数据当 verified 呈现。

## 3. Non-Goals

1. 不要求一次性补齐全部 8 个 agent 的专属逻辑。
2. 不改变外部 adapter（v1/v2 schema 文件）的校验行为。
3. 不新增 agent。

## 4. Behavior Invariants

1. adapter metadata 输出必须含 fidelity 字段，取值受控枚举。
2. `legacy-default` 根只能出现在 `generic` 层，不能与 `verified` 混用。
3. 补齐某 agent 的专属逻辑时必须附带该 agent 的针对性测试。
4. 文档（SUPPORTED_AGENTS / AGENT_ADAPTERS）逐 agent 标注层级。

## 5. Acceptance Criteria

1. `loom agent list/inspect --json` 输出每个内置 agent 的 fidelity 层级。
2. 8 个回落 agent 标注为 `generic`，codex/claude 标注为 `verified`。
3. reload 语义要么按 agent 差异化，要么归入 generic 层不再伪装差异。
4. 测试断言层级字段与 codex/claude 的差异化维持。

## 6. Edge Cases

1. 外部 adapter 文件覆盖内置 agent 时的层级归属。
2. agent 目录存在但 env 探测不可用（available=false）时层级不变。
3. 未来某 agent 从 generic 升级到 verified 的迁移路径。

## 7. Open Questions

1. fidelity 字段命名与枚举值（`verified`/`generic` 还是三层）？
2. 优先补齐哪几个 agent（按用户基数：cursor? gemini-cli?）？
