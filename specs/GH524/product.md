# GH524 Product Spec: 原子化 Agent-facing Skill Change Convergence Workflow

Issue: https://github.com/majiayu000/loom/issues/524
Depends on: #522 contract decision
Related: #454, #478, #497, #498, #512, #523
Status: Draft for maintainer review
Locale: zh-CN
Complexity: large

## 问题

一次 Skill 修改后，agent 需要自行组合 direction detection、source commit、active projection
枚举与刷新、visibility/reload 检查、可选 registry remote sync。任一步失败都可能留下部分完成
状态，而现有成功 envelope 无法一次证明所有请求轴已经收敛。

## 目标

1. 提供一个 dry-run-first、幂等、scope-bounded 的 Skill convergence workflow。
2. 正常编辑路径以 canonical registry source 为唯一写入源，live projection edit 作为显式恢复
   输入。
3. 在一个 convergence identity 下协调 source history、全部选定 active projections、registry
   state/audit、visibility evidence 与可选 remote transport。
4. 任何部分失败都返回结构化恢复状态，不用 warning 冒充完整成功。

## 非目标

1. 不保证正在运行的第三方 agent process 支持 hot reload；只能报告真实 evidence 或
   `restart_required`。
2. 不自动采用 observed/external target，不扩大 binding、agent、workspace 或 remote scope。
3. 不删除低层 `skill commit`、`skill project`、reconcile 或 `sync` 原语。
4. 不在没有明确授权时自动 push remote。

## 行为不变量

1. **B-001** dry-run 必须解析一个 Skill、输入方向、精确 target/binding/projection 集合、每项
   method、visibility/reload 检查和 remote policy；任何未解析 selector 必须阻断 apply。
2. **B-002** 默认 source 方向只读取 canonical registry source；projection 内容只有在调用者
   显式选择 projection input 且唯一定位 instance 时才能成为 capture 输入。
3. **B-003** source 与任一 projection 同时 dirty，或多个 projection 提供不一致 dirty 内容时，
   workflow 必须返回 conflict 与逐项 evidence；不得自动选择“最新”一方。
4. **B-004** dry-run 必须完全只读，不创建 plan/state/audit/Git ref/index/live path；输出必须与
   apply 使用同形 effect plan。
5. **B-005** apply 必须要求非空 idempotency key；同一 key + 同一 plan 重试返回原结果且不重复
   commit、projection swap、operation record 或 remote push；同一 key + 不同 plan 必须阻断。
6. **B-006** apply 开始后必须持有 workspace/Skill 写锁，并验证 dry-run evidence 中的 source
   HEAD、registry checkpoint 与 projection digests；任一 stale guard 使 apply 在写前失败。
7. **B-007** symlink projection 在安全指向 canonical source 时不得无意义重建；copy projection
   必须原子替换为 source 字节；materialize projection 必须按其变换规则重建并验证 digest。
8. **B-008** 所有本地 projection 必须先完成 staging/验证，再进入 commit/swap；任一本地写失败
   时，已修改的 source、registry metadata、Git index/refs 与 live paths 必须恢复，或以
   terminal recovery-required 状态列出无法恢复项。
9. **B-009** 每个 apply 使用单一 `convergence_id`，并在结果中逐项记录 source commit、
   projection effect、registry operation、visibility evidence、remote effect 与 rollback/recovery
   evidence；缺少 required evidence 时不得报告 complete。
10. **B-010** 没有 active projection 是合法的 source-only plan，必须显式返回空 projection 集与
    `not_applicable`；若调用者要求 runtime convergence，则同一情况必须阻断。
11. **B-011** visibility 必须在 projection 写后重新读取；结果区分 `visible`、
    `restart_required`、`unsupported`、`unknown`、`error`，不得从文件存在推导当前 session 已
    加载。
12. **B-012** remote transport 是本地事务后的显式可选阶段。未请求 push 时返回
    `not_requested`；请求后 remote 失败不得回滚已经验证的本地 commit，而必须返回
    `local_complete_remote_pending` 与可幂等重试的 next action。
13. **B-013** target ownership、policy、approval 或 filesystem safety 阻断时，apply 不得绕过
    gate、降级 method 或扩大 scope；错误必须指出被阻断的 effect。
14. **B-014** 中断发生在本地 commit 前时必须回滚 staging；发生在本地 commit 后时必须能通过
    `convergence_id` 恢复/重试，且不得产生第二个 source commit。
15. **B-015** workflow complete 只表示本次声明的 required axes 均有成功 evidence；
    `restart_required` 可以是可接受完成状态，但必须由 plan 明确声明，不得默认视为 visible。

## 边界清单

| 边界 | 判定 |
| --- | --- |
| Empty / missing input | covered: B-001, B-005, B-010 |
| Error and failure paths | covered: B-003, B-008, B-012, B-013 |
| Authorization / permission | covered: B-013 |
| Concurrency / race / ordering | covered: B-006, B-008 |
| Retry / repetition / idempotency | covered: B-005, B-014 |
| Illegal state transitions | covered: B-002, B-003, B-015 |
| Compatibility / migration | 本 workflow 为 additive；旧低层命令保留，covered: B-001 |
| Degradation / fallback | covered: B-011, B-012, B-013 |
| Evidence and audit integrity | covered: B-006, B-009, B-015 |
| Cancellation / interruption | covered: B-014 |

## 用户流程

默认候选表面：

```bash
loom --json --root "$ROOT" skill converge "$SKILL" --dry-run
loom --json --root "$ROOT" skill converge "$SKILL" --apply --plan-digest "$PLAN_DIGEST" --idempotency-key "$KEY"
```

命令名需要维护者批准；无论最终命令形态如何，B-001..B-015 保持不变。

## 验收标准

1. 一个 workflow 能把 source 修改收敛到全部选定 active projections，并返回 #522 三轴状态。
2. symlink、copy、materialize 的正负路径都由 E2E fixture 覆盖。
3. source/projection 双 dirty、projection 间分歧、lock contention、stale plan、filesystem failure、
   remote unavailable、restart required 均有确定输出。
4. fault injection 证明本地阶段不会留下无法解释的半完成成功。

## 开放问题

1. 是否接受新增 `skill converge` leaf 的 ADR 例外，或将其表达为 `plan converge`；技术规格默认
   选择前者，因为这是新的用户意图而不是低层 projection 同义词。
2. `restart_required` 是否默认满足 runtime required axis；默认否，plan 必须显式选择。
