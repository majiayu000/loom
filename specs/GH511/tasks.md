# GH511 Task Plan: Strict Skill Lint False Positives

Issue: https://github.com/majiayu000/loom/issues/511
Product spec: `specs/GH511/product.md`
Tech spec: `specs/GH511/tech.md`
Status: Draft for implementation under current `implx auto` authorization

## Scope

修复 list-valued agent extension 与非英文 description 的 strict false positive，同时为已经正确工作的 block scalar 和 nested metadata 增加回归保护。

不修改 metadata 数据模型、MCP/dependency consumer、YAML parser dependency 或 release automation。

## Tasks

- [ ] `SP511-T1` — Owner: coordinator; Done when: regression fixtures cover GH511 P1-P6; Verify: `cargo test --test skill_lint`.
- [ ] `SP511-T2` — Owner: coordinator; Done when: extension values and portable field types follow the spec; Verify: `cargo test --test skill_lint`.
- [ ] `SP511-T3` — Owner: verification_owner; Done when: all fresh deterministic checks pass; Verify: commands in SP511-T3.
- [ ] `SP511-T4` — Owner: gh511-merge-reviewer; Done when: independent review and current-head PR gates pass; Verify: commands in SP511-T4.

### SP511-T1: Add Regression Coverage

Owner: coordinator

Files:

- `tests/skill_lint.rs`

Done when:

- 中文 usage description 的旧实现复现 strict failure，新实现通过。
- list-valued `allowed-tools` 的旧实现复现 portable schema failure，新实现通过并保留 sequence。
- `allowed-tools` mapping、boolean、number、null、empty string 与 mixed sequence 产生 stable schema finding。
- block scalar 与 nested scalar metadata 继续通过。
- number、boolean 与 explicit null 的 `name` / `description` 产生 type finding，不被静默转换或仅折叠成 missing。
- repository-contained fixtures 覆盖 official/system 与 representative registry frontmatter shape，不读取用户主目录。

Verify:

```bash
cargo test --test skill_lint
```

### SP511-T2: Preserve Agent Extension Values And Portable Types

Owner: coordinator
Depends on: SP511-T1

Files:

- `src/commands/skill_lint/frontmatter.rs`
- `src/commands/skill_lint.rs`

Done when:

- `allowed_tools` 使用 `serde_json::Value` 保留 non-empty string 或 string sequence，existing string JSON shape 不变；其他 YAML shape 明确失败。
- `name` 与 `description` 只接受 YAML string；其他 optional field contract 保持现状。
- 中文 description 不再因英文关键词或空格词数产生 portable strict error。
- `--quality` 的 advisory heuristic 与 existing agent compatibility warning 保持可用。

Verify:

```bash
cargo test --test skill_lint
```

### SP511-T3: Run Deterministic Verification

Owner: verification_owner
Depends on: SP511-T1, SP511-T2

Done when:

- focused lint tests、format、compile、full test suite 与 SpecRail packet check 都是本 session 的 fresh success。
- 大输出只写入 `artifacts/logs/2026-07-14-loom-gh511-t01`，parent 只读取短 tail 与状态。

Verify:

```bash
git diff --check
cargo fmt --all -- --check
cargo test --test skill_lint
cargo check --workspace --all-targets --all-features
cargo test
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir /Users/apple/Desktop/code/AI/tool/loom-worktrees/implx-GH511-strict-lint-yaml/specs/GH511
```

### SP511-T4: Independent Review And Merge Gate

Owner: gh511-merge-reviewer
Depends on: SP511-T3

Done when:

- independent native reviewer 检查最终 head 与 GH511 product/tech/tasks 覆盖，无 blocking finding。
- 当前 head 的 CI、GraphQL `reviewThreads`、merge state 与 SpecRail PR gate 全部通过。
- auto merge 后远端确认 merge commit，issue 已关闭或 closure audit 明确处理。

Verify:

```bash
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/github_pr_evidence.py --help
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/pr_gate.py --help
```

## Handoff Notes

- 当前 `origin/main` 已使用 `yaml-rust2`；禁止重做 parser 迁移。
- 官方 portable 规范的 `allowed-tools` 是 string；list-valued form 在本 issue 中按 agent extension 处理，不宣称为 portable 标准。
- `implx auto` 是本轮满足全部 gate 后的 standing merge authorization，但不是 release authorization，也不是 reviewer failure 后的 self-review authorization。
