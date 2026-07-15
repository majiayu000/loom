# GH523 Product Spec: 防止 CLI、Agent Skill 与生命周期文档契约漂移

Issue: https://github.com/majiayu000/loom/issues/523
Status: Draft for maintainer review
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

1. **B-001** 每个发行版必须暴露稳定的 `cli_contract_version`，并在 JSON envelope 与 shipped
   `loom-registry` Skill metadata 中可比较；缺失或空值视为不兼容。
2. **B-002** shipped `loom-registry` Skill 必须声明支持的 contract version 范围；运行时 CLI
   不在范围内时，Skill 指令必须要求 agent 停止 mutation，并给出安装匹配版本的建议。
3. **B-003** 仓库必须维护一个明确的 agent-facing surface inventory，至少覆盖 README、
   `AGENT_USAGE.md`、`SINGLE_SKILL_LIFECYCLE.md`、`loom-registry/SKILL.md`、公开
   `next_actions` 与 release package smoke；新增公开表面但未登记时 CI 必须失败。
4. **B-004** inventory 中标记 executable 的每个命令示例必须由当前 CLI parser 验证 command
   与 flags；placeholder 可以替换为 fixture 值，但不得通过删除参数来使示例通过。
5. **B-005** 被删除的 command/flag 出现在任一 active agent-facing surface 时 CI 必须失败；
   只检查少量 denylist 字符串不满足此不变量。
6. **B-006** 解释性、输出展示或 legacy 示例必须显式标记类别；未分类的 shell command 默认
   按 executable 检查，不能静默跳过。
7. **B-007** contract check 必须是只读且确定性的：不得修改文档、Git index/refs、registry、
   live targets 或用户 home；相同 tree 与 binary 重跑得到相同结果。
8. **B-008** parser 或文档读取失败必须导致 gate 失败，并报告文件、示例标识和解析错误；
   不得 warning 后继续发布。
9. **B-009** release archive 中 binary、Skill metadata 和 contract inventory 必须来自同一
   release version；混合旧 Skill + 新 CLI 或新 Skill + 旧 CLI 的负例必须被拒绝。
10. **B-010** 兼容范围变更必须是显式 reviewable diff；patch release 不得在没有 migration
    note 的情况下缩小兼容范围。
11. **B-011** 高上下文文件只允许人类/评审过的补丁修改；检查工具发现漂移时只输出建议和失败
    证据，不得自动修复。
12. **B-012** 并发构建或取消检查不得留下被部分更新的 generated contract artifact；发布只能
    消费已完成且校验通过的 artifact。
13. **B-013** 缺少 CLI binary、Skill metadata、inventory 或 fixture 时，check 必须 fail closed；
    空 inventory 与“没有需要检查的命令”不等价。

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

## 开放问题

1. `cli_contract_version` 是否与 crate semver 同步，还是独立递增；技术规格选择独立整数，等待
   维护者确认。
