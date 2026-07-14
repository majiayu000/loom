# Tech Spec

## Linked Issue

GH-511

## Product Spec

见 `product.md`。

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Frontmatter parse | `src/commands/skill_lint/frontmatter.rs` | 已使用 `yaml-rust2`，但把多种 scalar 静默转成 string，并把 metadata sequence 与 list-valued `allowed-tools` 变成 strict schema issue | 真实根因在 parse 后的类型收窄，而不是 issue 描述中的旧手写 parser |
| Portable validation | `src/commands/skill_lint.rs` | 用英文关键词和空格词数决定 strict error | 造成中文等非英文 description 误报 |
| Agent/quality sections | `src/commands/skill_lint/sections.rs` | quality 已是 warning；agent findings 可与 portable section 分离 | 可承载降级后的启发式与 extension compatibility |
| CLI regression tests | `tests/skill_lint.rs` | 覆盖基础 YAML 与 string-only rich frontmatter，缺少本 issue fixtures | 需要锁定中文、block scalar、nested metadata 与 list-valued extension 行为 |

## 设计方案

1. 保留 `yaml-rust2`，不引入第二个 YAML parser。
2. 将 `name` / `description` 解析收紧为“YAML string 或明确 schema issue”，禁止 number/bool 到 string 的隐式转换。block scalar 由 parser 产生 string，因此自然通过。
3. 保留当前 nested metadata flattening/string-map contract；用 regression fixture 证明 spec-valid nested scalar mapping 继续通过，不扩大 metadata API。
4. 将 `allowed_tools` 表示为 `Option<Value>`：trimmed non-empty string 的 JSON 响应不变，仅含 non-empty string 的 sequence 作为 agent extension 保真。其他 YAML shape 产生 `frontmatter_allowed_tools_invalid`；`--agent codex` 复用现有 `agent_codex_unsupported_field` targeted warning，Claude list 保持 pass。
5. 从 `validate_frontmatter` 移除空格词数和英文关键词 strict errors。`--quality` 下的 `quality_description_vague` 保持 advisory warning，不影响 `valid`。
6. `compatibility` 与其他未涉及 optional fields 保持现状，避免超出 GH-511。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1/P2 | `frontmatter.rs`, `skill_lint.rs` | block scalar 与 non-string portable field tests |
| P3 | `frontmatter.rs` existing path | nested scalar mapping strict-pass assertion |
| P4 | `frontmatter.rs`, `sections.rs` | strict portable pass + `--agent codex` targeted warning；Claude-supported list 不产生假 warning |
| P5 | `skill_lint.rs`, `sections.rs` | 中文 strict pass；`--quality` 仅 warning |
| P6 | existing error paths and corpus fixtures | invalid YAML/name/missing description/null tests；official/system 与 registry-shaped frontmatter snapshots |

## 数据流

`SKILL.md` bytes → frontmatter delimiter extraction → `yaml-rust2::Yaml` mapping → typed `SkillLintFrontmatter`（仅 `allowed_tools` 用 `Value` 保留 extension sequence）→ portable validation / agent checks / optional quality checks → stable JSON envelope。

## 备选方案

- 仅忽略 `allowed-tools` sequence schema error但不保留值：会造成 lint JSON 静默丢失 extension 数据。
- 再引入 `serde_yaml`：增加重复 parser 与依赖体积，且当前 parser 已能正确读取 YAML。
- 为中文硬编码关键词：无法覆盖所有语言，仍把语言启发式错误地当 portable schema。

## 风险

- Security: 不执行 metadata 或 extension；只保留结构化值，避免把复合值拼成命令。
- Compatibility: existing scalar JSON shape 与 metadata contract 保持；仅 list-valued `allowed_tools` 从 `null` 变为原 sequence。
- Performance: frontmatter 规模很小，递归 YAML-to-JSON 的成本可忽略。
- Maintenance: portable、agent 和 quality finding 必须继续用稳定前缀分区。

## 测试计划

- [ ] Unit/integration: `cargo test --test skill_lint`
- [ ] Static/build: `cargo check --workspace --all-targets --all-features`
- [ ] Full suite: `cargo test`
- [ ] Spec packet: `python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/implx-GH511-strict-lint-yaml/specs/GH511`

## 回滚方案

回滚本 PR 即恢复旧 lint 行为；无持久化 schema 或数据迁移。`allowed_tools` 是内部 report model，string 输入的 JSON 形态不变。
