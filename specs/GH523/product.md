# GH523 Product Spec: 防止 CLI、Agent Skill 与生命周期文档契约漂移

Issue: https://github.com/majiayu000/loom/issues/523
Status: Implementation complete; independent follow-up review PASS; repository verification PASS; exact-head CI pending
Locale: zh-CN
Complexity: medium

## 问题

#454 删除 `capture/save/snapshot/verify` 后，部分 agent runbook、生命周期文档和运行时提示
仍引用旧命令，直到 #512 才再次修正。现有测试能证明若干固定字符串存在或不存在，却不能
证明所有公开命令示例能被当前 CLI 解析，也不能证明 release 中的 `loom-registry` Skill 与
同包 CLI 属于同一契约版本。

## 目标

1. 给 machine-facing CLI contract 一个稳定、可比较的版本身份。
2. 让 CI 以只读方式验证 Agent Skill、runbook、lifecycle、README、`next_actions` 和 release
   package 中的命令与当前 CLI 一致。
3. 让 CLI/Skill 版本不兼容时 fail closed，并返回明确升级路径。

## 非目标

1. 不自动重写 `AGENTS.md`、`SKILL.md` 或其他高上下文文档。
2. 不要求所有解释性历史/legacy 文档中的示例都可执行；它们必须显式分类为 legacy。
3. 不以网络调用验证命令，也不在 contract check 中执行 mutation。
4. 不取代现有 Skill lint、trigger eval 或 release archive smoke tests。

## 行为不变量

1. **B-001** 每个发行版必须暴露独立于 crate/package semver 的 SemVer
   `cli_contract_version`，并在 JSON envelope 与 shipped `loom-registry` Skill metadata 中可比较；
   缺失、为空或不是合法 SemVer 时视为不兼容。
2. **B-002** shipped `loom-registry` Skill 必须声明支持的 contract version 范围；运行时 CLI
   不在范围内时，Skill 指令必须要求 agent 停止 mutation，并给出安装匹配版本的建议。
3. **B-003** 仓库必须维护一个明确的 agent-facing surface inventory，至少覆盖 README、
   `AGENT_USAGE.md`、`SINGLE_SKILL_LIFECYCLE.md`、`loom-registry/SKILL.md`、Panel mutation
   labels、公开 `next_actions`、release package smoke 与 Homebrew share；新增公开表面或
   `next_actions` producer 但未登记时 CI 必须失败。每个 producer 的 fixture trace 必须携带可观测的
   stable emitter id；仅输出相同 command text 不能证明对应 producer 已被覆盖。
4. **B-004** inventory 中标记 executable 的每个命令示例必须由当前 CLI parser 验证 command
   与 flags，并确认解析路径及实际使用的每个 flag/option 都属于公开可见 surface；公开 command
   上的 hidden/deferred flag 同样不得进入公开契约。parser 接受 hidden/deferred command 或 flag
   不等于公开契约有效。placeholder 可以替换为 fixture 值，但不得通过删除参数来使示例通过。
5. **B-005** 被删除的 command/flag 出现在任一 active agent-facing surface 时 CI 必须失败；
   只检查少量 denylist 字符串不满足此不变量。
6. **B-006** 分类粒度必须达到单个 example/行区间，而不是整文件；解释性、输出展示或 legacy
   示例必须显式标记类别。未分类的 fenced shell 或 inline code 中的 `loom` command 默认按
   executable 检查，不能静默跳过。
7. **B-007** contract check 必须是只读且确定性的：不得修改文档、Git index/refs、registry、
   live targets 或用户 home；相同 tree 与 binary 重跑得到相同结果。
8. **B-008** parser 或文档读取失败必须导致 gate 失败，并报告文件、示例标识和解析错误；
   不得 warning 后继续发布。
9. **B-009** release archive 中 binary、Skill metadata 和 contract inventory 必须来自同一
   release version；manifest 必须绑定 binary、shipped Skill 内容和 inventory 的 digest。混合旧
   Skill + 新 CLI 或新 Skill + 旧 CLI 的负例必须被拒绝。
10. **B-010** 兼容范围或 contract capability 变更必须是显式 reviewable diff；新增可被 shipped
    Skill 依赖的 agent-facing command/flag/field 时至少递增 contract minor，breaking shape/semantics
    变更递增 major，非能力修正才可递增 patch。range-policy gate 必须接收并校验明确的 base
    tree/SHA，缺失或不可读取时 fail closed；任何兼容范围缩小都必须携带 migration note。
11. **B-011** 高上下文文件只允许人类/评审过的补丁修改；检查工具发现漂移时只输出建议和失败
    证据，不得自动修复。
12. **B-012** 并发构建或取消检查不得留下被部分更新的 generated contract artifact；发布只能
    消费已完成且校验通过的 artifact。
13. **B-013** 缺少 CLI binary、Skill metadata、surface/emitter inventory、manifest 或 fixture 时，
    check 必须 fail closed；空 inventory 与“没有需要检查的命令”不等价。

## 边界清单

| 边界 | 判定 |
| --- | --- |
| Empty / missing input | covered: B-001, B-013 |
| Error and failure paths | covered: B-008, B-013 |
| Authorization / permission | covered: B-011 |
| Concurrency / race / ordering | covered: B-012 |
| Retry / repetition / idempotency | covered: B-007 |
| Illegal state transitions | covered: B-002, B-009 |
| Compatibility / migration | covered: B-001, B-002, B-010 |
| Degradation / fallback | covered: B-006, B-008 |
| Evidence and audit integrity | covered: B-003, B-004, B-009 |
| Cancellation / interruption | covered: B-012 |

## 验收标准

1. 将 active 文档命令改成不存在的 `loom skill save` 时，CI 精确指出文件与示例并失败。
2. 将 release Skill 的 contract range 改为不包含 binary version 时，package smoke 失败。
3. contract check 前后 `git status --short`、用户 home 与 registry fixture 没有变化。
4. #524 新增公开 workflow 时，若未登记 inventory 与 compatibility，相关 PR 无法通过 gate。
5. 新增一个未登记的 `next_actions` producer 时，CI 在 parser fixture 运行前即因 coverage 失败。
6. 并发生成或在 publish 前注入取消时，最终 artifact 只能是完整旧版本或完整新版本。
7. 在公开 command 示例中加入 hidden flag（例如 `--max-cycles`）时，public visibility gate 失败。
8. 两个 producer 发出相同 command text 时，fixture 只运行其中一个不能覆盖另一个 emitter id。
9. range-policy check 未获得可读取的显式 diff base 时失败，不得只验证最终 tree 后通过。
10. 新 shipped Skill 开始使用 additive command、但 CLI contract minor 未递增时，compatibility gate
    失败；递增后“新 Skill + 旧 CLI”必须 fail closed，而同 major 的旧 Skill + 新 CLI 仍可按范围兼容。

## Maintainer 架构决策（2026-07-16）

1. `cli_contract_version` 使用独立 SemVer，不与 crate/package semver 同步。
2. parser/checker 使用 package 内最小共享 library facade；不新增 public checker command/leaf。
3. `docs/agent-command-surfaces.toml` 是 review-owned source of truth；scanner 与 release manifest
   只能消费和验证，不得自动改写它。
