# Product Spec

## Linked Issue

GH-511

## 用户问题

`loom skill lint --strict` 会把合法 YAML 与非英文 description 误报为 portable schema 错误。用户无法区分真正的 Agent Skills 规范违规、agent-specific 兼容性差异和仅供改进质量的启发式建议，导致有效 Skill 无法通过严格校验。

## 目标

- 严格 lint 只按解析后的 portable 字段类型与规范约束判定错误。
- 合法 block scalar、嵌套 `metadata` mapping 以及 agent-specific 扩展不再被误报为 YAML 解析失败。
- 非英文 description 不因缺少英文关键词或空格分词而失败。
- portable 校验、agent compatibility 与 quality findings 保持可机器区分。

## 非目标

- 不改变 `SKILL.md` Markdown 正文规则。
- 不承诺所有 agent 都支持同一种 extension 值类型。
- 不在本 issue 中增加新的 lint mode 或自动重写 frontmatter。
- 不发布 crate 或 GitHub Release。

## Behavior Invariants

1. `name` 与 `description` 必须是非空 YAML string；`name` 继续满足目录名、长度和字符约束，`description` 继续满足 1024 字符上限。
2. 合法 YAML block scalar description 作为字符串校验，不因表示形式不同而失败。
3. spec-valid nested `metadata` mapping 继续通过 strict lint；非 string key 或不符合当前 metadata string-map contract 的值仍产生明确 schema finding。
4. agent-specific extension 的非 portable 值形态不产生 `frontmatter_yaml_invalid` 或 portable strict error；指定 `--agent` 时，以对应 `agent_*` warning 报告兼容风险。
5. description 是否包含英文 `use`、`when`、`for`、`trigger` 或空格分隔词数不再是 portable strict 通过条件；相关启发式只能作为 quality warning。
6. 无效 YAML、缺失 frontmatter、缺失必填字段及错误 portable 字段类型继续产生稳定、可机器读取的 error finding。

## 验收标准

- [ ] block scalar description 与 nested metadata 的 strict lint 通过。
- [ ] nested metadata mapping 通过 strict lint，并保持现有 machine-readable metadata 输出。
- [ ] list-valued agent extension 不产生 portable/YAML error，并在 agent lint 中产生 targeted warning。
- [ ] 中文 usage description 的 strict lint 通过，且不出现 `description_missing_usage_context` error。
- [ ] 非 string `name` / `description` 产生明确 schema error，不能被静默转换成字符串。
- [ ] 现有 invalid YAML、name、description 长度与 agent compatibility 回归测试继续通过。

## 边界情况

- frontmatter root 不是 mapping、closing marker 缺失或 YAML 语法错误时仍失败。
- `metadata` 为非 mapping 或包含非 string key 时失败，不静默丢弃。
- `allowed-tools` 的 portable 规范形态仍是 string；agent-specific sequence 仅代表兼容性扩展，不提升为 portable 标准。
- 仅运行 strict lint 时不强制输出 quality finding；`--quality` 继续输出 advisory finding。

## 发布说明

这是向后兼容的 lint 误报修复。现有 string metadata 与 string `allowed-tools` 的 JSON 形态保持不变；list-valued extension 在 JSON 中保留其 sequence。版本发布仍需独立的人类 release 授权。
