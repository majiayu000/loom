# Product Spec

## Linked Issue

GH-510

## 用户问题

`loom skill trash add <skill>` 当前只把 source 移入 `trash/`，仍保留该 Skill 的 active rules、projection records 与 live runtime links。source 消失后，原有链接变成 dangling symlink，registry 进入 drift/unhealthy 状态，用户必须手工清理内部状态。

## 目标

- trash 成为 source、active desired state 与 Loom-owned runtime links 的一致生命周期操作。
- 只删除能够证明属于 Loom 且安全指向待 trash source 的 symlink。
- dry-run 准确报告 rule、projection 与 live-link 影响，且保持完全只读。
- 任一 pre-commit 步骤失败时恢复 trash 前的一致状态，并完整暴露 rollback errors。

## 非目标

- 不删除 `copy` / `materialize` projection 内容或其他非 symlink live path。
- 不删除可能被其他 Skill 复用的 targets 或 bindings。
- `trash restore` 不自动恢复历史 activation；恢复 source 后仍需显式 `skill activate`。
- 不修改 trash metadata schema，不发布 crate 或 GitHub Release。

## Behavior Invariants

1. trash 成功后，所有 `skill_id` 等于目标 Skill 的 active rules 与 projection records 均不存在；无关 Skill、bindings 与 targets 保持不变。
2. live path 仅在同时满足 registry projection ownership、registered target 下的预期路径、实际为 symlink、且最终指向当前 registry source 时才允许删除。
3. 已缺失的安全 live path 视为幂等状态；错误 symlink、普通文件、目录以及 `copy` / `materialize` projection 必须保留，并在计划与结果中给出稳定 retained reason。
4. source move、active-state cleanup、安全 symlink cleanup 与 registry audit mutation 属于同一 pre-commit transaction；任一步失败时恢复 source、rules、projections、live links、audit state 与 Git index，并在恢复失败时返回结构化 rollback errors。
5. `--dry-run` 不创建 registry layout、trash entry、audit event、Git commit 或 live-path mutation，并报告确定性的 removed rule count、projection IDs、deleted-link candidates 与 retained paths。
6. `trash restore` 只恢复 source 和 trash entry 状态，不恢复已移除的 rules、projections 或 live links。
7. 标准 Loom-managed symlink activation 被 trash 后，`workspace doctor` 不因该 Skill 留下 dangling projection 或 source drift。

## 验收标准

- [ ] active Skill trash 成功后 rules、projections 与安全 runtime symlinks 全部移除。
- [ ] 多 target / 多 agent symlink projection 被完整且去重清理，无关 activation 不受影响。
- [ ] 普通文件、目录、错误目标 symlink 与非 symlink projection 保留并明确报告，不发生误删。
- [ ] dry-run 返回完整 impact，同时 source、registry、live paths、audit 与 Git head 均不变。
- [ ] 故障注入证明 source、registry state、live symlink 与 audit state 恢复到 trash 前状态。
- [ ] restore 后 source 存在但 activation 保持为空。
- [ ] success path 后 doctor 为 healthy，相关 focused tests 通过。

## 边界情况

- 同一规范化 live path 被多个 projection record 引用时只备份、删除和恢复一次。
- 相对 symlink 必须按其父目录解析，rollback 恢复原始 link target 文本，不猜测绝对路径。
- malformed registry state 必须 fail closed，不允许先移动 source 再静默跳过 cleanup。
- live path 已丢失不阻止 trash；不安全或无法证明 ownership 的现存 path 必须保留。
- commit 或 operation recording 失败时不得留下 staged trash/state 变更。

## 发布说明

这是 lifecycle consistency 修复。返回 JSON 会增加 additive impact 字段；`trash restore` 的 source-only 行为会在文档中明确。版本发布仍需独立 release 授权。
