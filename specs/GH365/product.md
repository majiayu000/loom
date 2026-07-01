# GH365 Product Spec: Expanded Skill Lint

Issue: https://github.com/majiayu000/loom/issues/365
Parent: https://github.com/majiayu000/loom/issues/363
Status: Draft for implementation
Locale: zh-CN

## Goal

让 `loom skill lint` 从浅层 `SKILL.md` 检查升级为可回答三类问题的只读检查：

1. 这个 skill 是否符合 portable Agent Skills 基础规范。
2. 这个 skill 是否对目标 agent（本 slice 先覆盖 `codex` / `claude`）存在明显兼容风险。
3. 这个 skill 是否有容易导致触发不准、上下文膨胀或脚本维护困难的质量风险。

## Scope For First PR

本 PR 实现最小可合并 slice：

- 保留 `--strict`、`--compat`、`--fix` 现有语义。
- 新增 `--portable` 作为 strict portable alias。
- 新增 `--agent <agent>`，先覆盖 `codex` / `claude` 的结构化 compatibility section。
- 新增 `--quality`，只产生 non-fatal warning。
- 用真实 YAML parser 解析 frontmatter，支持 nested YAML，不再因为 `metadata` / `compatibility` 形态直接拒绝。
- 报告 `sections.portable_spec`、`sections.agent_compatibility`、`sections.quality`、`sections.resources`、`sections.progressive_disclosure`。

## Non-Goals

1. 不在本 slice 自动修改 skill；`--fix` 仍然只返回 read-only plan。
2. 不实现完整 Agent Skills 官方 spec 的所有字段验证。
3. 不做 active Codex/Claude skill directory collision 扫描。
4. 不引入外部网络请求或远端 spec 抓取。
5. 不改变 `skill new` 的生成模板策略。

## Behavior Invariants

1. `loom skill lint <skill>` 默认等价于 strict portable lint。
2. `--portable` 与 `--strict` 互为同类模式，不能与 `--compat` / `--fix` 同时使用。
3. `SKILL.md` 缺失、legacy lowercase `skill.md`、missing name、missing description、name mismatch 继续按现有 error/warning 规则返回。
4. Frontmatter 必须由 YAML parser 解析；parse error 继续返回 `frontmatter_yaml_invalid`。
5. `metadata` string map、`compatibility` nested object、`license` scalar、`allowed-tools` scalar 不应因为 nested YAML parser 限制失败。
6. `--agent codex` 对 Claude-only fields 返回 warning；`--agent claude` 识别这些字段但不把它们当作 portable failure。
7. `--quality` 只增加 warning，不让原本 valid 的 skill 变为 invalid。
8. 资源和 progressive disclosure 计数必须在 JSON report 中稳定输出。

## Acceptance Criteria

1. Rich YAML frontmatter with `metadata`, `license`, `compatibility`, and `allowed-tools` passes `loom skill lint --portable`.
2. Existing strict failures still return `SCHEMA_MISMATCH` with `error.details.report`.
3. `--agent codex` returns an agent compatibility warning for Claude-only frontmatter fields.
4. `--quality` warns for missing eval fixtures and unclear script entrypoints.
5. Report JSON includes `sections.portable_spec`, `sections.agent_compatibility`, `sections.quality`, `sections.resources`, and `sections.progressive_disclosure`.
6. Tests cover portable rich YAML, agent-specific field handling, quality warnings, and existing strict validation behavior.
