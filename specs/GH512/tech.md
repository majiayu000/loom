# Tech Spec

## Linked Issue

GH-512

## Product Spec

见 `product.md`。

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Agent docs | `docs/AGENT_USAGE.md`, `docs/SINGLE_SKILL_LIFECYCLE.md` | runbook 引用不存在且会冲突的 `loom` Skill；两处仍使用旧生命周期命令 | 直接复现入口和 Skill 链接目标 |
| First-party assets | `skills/`, `.gitignore` | mutable registry content 默认忽略、Cargo package 排除该目录，仓库没有 scoped shipped Agent Skill exception | 需要单一受测 source tree，同时保持 tool checkout 与 registry 分离 |
| Release archive | `.github/workflows/release.yml` | 只复制 binary、README、LICENSE；Homebrew 只安装 binary | clean install 无 Skill 可发现 |
| Install docs | `README.md` | 只安装 CLI，没有 Skill copy/discovery 说明 | acceptance 要求 Claude/Codex 可安装 |
| Skill validation | Loom lint/eval commands and `tests/common` | 已有 portable/agent lint 与 offline fixtures，但没有第一方 shipped asset regression | 可复用现有 parser/contract，禁止另造 evaluator |

## 设计方案

1. 使用 repository-wide collision search 后，由 `skill-creator` 的 `init_skill.py` 在 `skills/loom-registry/` 创建标准 Agent Skill skeleton；在 `.gitignore` 中只为该 first-party 目录增加 scoped exception，其他 mutable registry content 仍忽略；保留唯一 canonical name `loom-registry`，不创建 `loom` alias。
2. `SKILL.md` frontmatter 只使用 portable `name` / `description`，正文以 imperative workflow 描述 local registry 边界、JSON envelope、显式 `--root`、read-first、dry-run、approval、warnings、sync/history 与 lifecycle。详细 CLI 契约链接到 `docs/AGENT_USAGE.md` 和 `docs/SINGLE_SKILL_LIFECYCLE.md`，避免复制全部文档。
3. `agents/openai.yaml` 提供 display name、短描述与显式 `$loom-registry` default prompt；`loom.skill.toml` 使用现有 `loom.skill.v1` schema 和 `requires_tools = ["loom"]`。
4. `evals/triggers.jsonl` 使用显式 `expected_trigger` / `observed_trigger` fixtures：至少四个 local-registry positives 和三个 Loom.com/video negatives。负例文本可包含 Loom 品牌词，但 `observed_trigger=false`，防止词面命中被误当作通过。
5. 新增 `tests/shipped_registry_skill.rs`，把 tracked Skill 复制到隔离的临时 registry，再调用真实 Loom binary 验证 strict portable lint、Claude/Codex compatibility 与 offline trigger eval；另直接断言 canonical name、正负样本数量、precision/recall 和 exclusion 文案。
6. release archive staging 复制完整 `skills/loom-registry` 到 `skills/loom-registry`；解包 smoke 显式检查 `SKILL.md`、`agents/openai.yaml`、`loom.skill.toml` 与 trigger fixtures，避免只测试 source checkout。
7. Homebrew formula 在 archive 根目录安装 binary 后，以 `pkgshare.install "skills"` 保存同一份 Skill。它不触碰用户 home；README 从 archive 路径或 `$(brew --prefix loom)/share/loom/skills/loom-registry` 执行 fail-closed copy。
8. README 对 Claude Code 使用 `$HOME/.claude/skills/loom-registry`，对 Codex 使用 `$HOME/.agents/skills/loom-registry`；先检查目标不存在，再 `cp -R`，并明确新 session discovery。更新 agent runbook 和 single-Skill lifecycle 到当前 lifecycle verbs。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1/P2/P3 | `SKILL.md`, manifest, metadata | skill-creator validation；strict/agent lint integration test |
| P4 | `evals/triggers.jsonl` | isolated offline eval for Claude/Codex；fixture assertions |
| P5 | release archive + Homebrew formula | workflow source regression + archive smoke checks |
| P6/P7 | README + agent runbook | documentation assertions and searched command audit |

## 数据流

tracked `skills/loom-registry` → release staging `.../skills/loom-registry` → tar archive → direct fail-closed copy，或 Homebrew `pkgshare/skills/loom-registry` → fail-closed copy → new Claude/Codex session discovery。

测试路径为 tracked Skill → isolated temp registry → real `loom skill lint` / `skill eval offline` → JSON envelope assertions；不会把 repository checkout 用作 mutable registry。

## 备选方案

- 名称继续使用 `loom`：已知与 Loom.com Skill 冲突，违反 issue 的核心目标。
- 自动 installer 写入两个 agent directories：扩大 CLI 权限面并可能覆盖既有 Skill；改为文档化 fail-closed copy。
- 只在 README 内嵌 Skill 文本：无法被 agent discovery，也不能被 lint/eval/release smoke 作为单一 source 测试。
- 把 Skill 放入 crate package：当前 acceptance 由 release archive / documented installer 满足；改变 crates.io package 内容不帮助 binary-only install。

## 风险

- Compatibility: Claude/Codex metadata 差异通过 portable frontmatter、agent-specific lint 与 `agents/openai.yaml` 覆盖。
- Security: installer 示例不使用 force/overwrite，不执行远端脚本，不写 source checkout；Skill 禁止 ad-hoc registry mutation。
- Release: workflow 必须复制目录而不是 symlink，并验证 archive 内真实文件；本 PR 不触发 tag workflow。
- Maintenance: docs 与 Skill 的命令可能漂移，因此 integration test 锁定关键安全 contract，CLI 细节继续以 runbook 为准。

## 测试计划

- [ ] Skill-creator: `python3 /Users/apple/.codex/skills/.system/skill-creator/scripts/quick_validate.py skills/loom-registry`
- [ ] Focused: `cargo test --test shipped_registry_skill`
- [ ] Static/build: `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --all-features`, `git diff --check`
- [ ] Full repository: `make check`
- [ ] Spec packet: `python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir <worktree>/specs/GH512`
- [ ] Release source audit: assert archive copy, unpack smoke, and Homebrew `pkgshare` statements exist in `.github/workflows/release.yml`.

## 回滚方案

回滚本 PR 会移除 first-party Skill 与 archive/package-share copy，并恢复旧文档；不涉及 registry schema、用户 home migration 或已发布 artifact。任何真实 release 仍需独立授权。
