# Product Spec

## Linked Issue

GH-512

## 用户问题

Loom 的 agent runbook 要求先加载一个仓库并未提供的 `loom` Skill；该名称还会与面向 Loom.com 视频的第三方 Skill 冲突。结果是 Claude Code / Codex 无法自动发现本地 Loom registry 的安全调用契约，agent 可能加载错误 Skill，或绕过 CLI 直接修改 registry 文件。

## 目标

- 随 Loom 发行物提供 canonical name 为 `loom-registry` 的第一方 Agent Skill。
- 让 Claude Code 与 Codex 能通过明确、不会静默覆盖既有 Skill 的安装步骤发现它。
- 让触发评测区分本地 Skill registry / projection / binding / sync / history 请求与 Loom.com 视频请求。
- 让 Skill 固化 Loom 的 agent-safe JSON、显式 registry root、dry-run 与 approval 契约。
- 修正文档中缺失 Skill 与过期 `skill save` / `skill snapshot` 生命周期命令。

## 非目标

- 不提供 `loom` 名称或 alias，不接管 Loom.com 视频录制、分享或转写请求。
- 不新增自动写入用户 `$HOME` 的 installer 或 CLI subcommand。
- 不覆盖或删除任意已存在的 Claude Code / Codex Skill。
- 不改变通用 trigger evaluator、agent discovery 规则或 registry schema。
- 不发布 crate、Homebrew formula、GitHub Release 或版本 tag。

## Behavior Invariants

1. 第一方 Skill 的唯一名称是 `loom-registry`；描述与正文同时包含本地 registry 正向触发边界和 Loom.com/video 负向边界，不存在 `loom` alias。
2. Skill 将 machine-facing 调用固定为 `loom --json --root <registry_root> ...`，要求只以 `ok=true` 为成功、按 `error.code` 分支，并记录 warnings / request ID；不得把 Loom 源码 checkout 当作可写 registry。
3. 支持 dry-run 的变更命令必须先预演，再检查 `data.safe_to_run`、`required_approvals` 或等价响应字段；不得默认使用 `--force`，不得吞掉 warnings 或错误。
4. 正向 trigger fixtures 覆盖 local registry、projections / bindings、sync、operation history 与 single-Skill lifecycle；负向 fixtures 覆盖 Loom.com 视频录制、分享与转写。两个 agent surface 的离线评测都必须保持满 precision / recall。
5. 每个 release archive 都包含完整 `skills/loom-registry/`；archive smoke test 校验 `SKILL.md`、Agent metadata、manifest 和 trigger fixtures。Homebrew formula 把该目录安装到 package share，而不是用户的 Skill 目录。
6. Claude Code / Codex 安装说明从 release archive 或 Homebrew package share 复制 Skill；目标存在时 fail closed，并要求重新启动或新开 session 完成 discovery。
7. `docs/AGENT_USAGE.md` 不再引用缺失的 `loom` Skill，生命周期示例使用当前 `skill commit` 与 `skill release --anchor` 契约。

## 验收标准

- [ ] tracked `skills/loom-registry/SKILL.md` 通过 skill-creator validation 与 Loom strict portable lint。
- [ ] Claude Code / Codex compatibility 和 trigger fixtures 的离线验证通过，Loom.com/video negatives 不触发。
- [ ] release workflow 将完整 Skill 打入 archive，并在解包 smoke 中校验；Homebrew formula 安装 package-share copy。
- [ ] README 提供 release archive 与 Homebrew 的 fail-closed 安装步骤，且不自动写 `$HOME`。
- [ ] agent runbook 不再引用缺失 Skill 或旧的 save/snapshot lifecycle。
- [ ] focused tests、Rust build/test、完整仓库检查与 SpecRail gate 通过。

## 边界情况

- 安装目标已存在时，说明必须停止并要求用户选择备份、重命名或移除；不得给出覆盖命令。
- release archive 的目标平台不同，但 Skill 文件集与路径必须一致。
- `HOMEBREW_PREFIX` 可能为 Intel 或 Apple Silicon 路径；文档使用 `brew --prefix loom` 推导 package share。
- Skill 被复制进当前 agent 的 discoverable directory 后，不能声称当前 session 已热加载。
- 提示同时出现 “Loom” 和 “video” 时按视频负向边界处理，除非明确要求本地 registry/CLI。

## 发布说明

这是发行物内容、安装文档和 agent contract 的改进；本 PR 只准备后续 release，不执行任何发布动作。
