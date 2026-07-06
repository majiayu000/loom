# GH495 Product Spec: Real Codex CLI Eval Runner

Issue: https://github.com/majiayu000/loom/issues/495
Status: Draft for implementation
Locale: zh-CN

## Goal

让 `skill eval run` 的 `--runner codex-cli` 真实执行 Codex CLI，而不是在通过 executable/env gate 后返回 `runner_unsupported`。真实 runner 必须在隔离 workspace 中运行 task/trigger cases，解析 Codex JSONL trace，并把报告和 compiled promotion evidence 标记为真实 Codex CLI 证据。

## Users

1. Skill 维护者：需要知道 skill 是否改善真实 agent 行为，而不是只通过 mock fixture。
2. Release / promotion 维护者：需要选择是否要求真实 agent evidence 后才把 compiled artifact 视为可推广。
3. 自动化调用方：需要 runner 缺失、未授权、timeout、trace 损坏等问题以 typed JSON failure 暴露，不能静默回退到 mock。

## Non-Goals

1. 不改变默认 runner；未显式选择时仍使用 mock/offline fixture。
2. 不绕过 `LOOM_EVAL_ALLOW_CODEX_CLI=1` 授权。
3. 不承诺 eval success 是安全证明。
4. 不新增网络凭据、模型配置或 Codex 账号管理。
5. 不把 `skill eval compare --runner codex-cli` 扩展为跨 ref 检出执行；本次只防止它静默使用 mock evidence。

## Behavior Invariants

1. `skill eval run --runner mock` 的报告、评分和默认行为保持兼容。
2. `skill eval run --runner codex-cli` 必须要求 `codex` executable 和 `LOOM_EVAL_ALLOW_CODEX_CLI=1`。
3. Codex CLI 每个 case/variant 使用独立 workspace。
4. Codex JSONL trace 不能解析时命令失败；不能降级到 mock。
5. Runner timeout 必须 typed fail，并清理临时 workspace。
6. Eval 报告必须区分 `real_codex_cli` 与 fixture/mock evidence。
7. Compile 默认继续使用 offline fixture evidence；只有显式 flag 才要求 real Codex CLI evidence。

## Acceptance Criteria

1. `CodexCliRunner` 实现 existing `SkillEvalRunner` trait，负责 workspace 准备、`codex exec --json` 调用、JSONL trace 解析、task scoring 和 cleanup。
2. `skill eval trigger --runner codex-cli` 使用 Codex CLI 输出判断 trigger，而不是 lexical fallback。
3. `LOOM_EVAL_ALLOW_CODEX_CLI` gate 保留；未授权时返回现有 structured error。
4. Missing executable、timeout、unparseable trace 均返回 `EVAL_FAILED` typed envelope，且不写成功报告。
5. `skill eval run --runner codex-cli` 报告 `mode: real_codex_cli`，并保留 runner/provenance 信息。
6. `skill compile --require-real-eval` 使用 real Codex CLI eval evidence；默认 compile 行为仍使用 `offline_fixture`。
7. Tests 用 fake `codex` binary 覆盖 real runner success、authorization/missing executable gate、unparseable trace failure 和 compile real evidence。

## Done When

- `cargo test --test skill_eval`
- `cargo test --test skill_compile`
- `cargo check --workspace --all-targets --all-features`
- `cargo test --workspace --all-features`
