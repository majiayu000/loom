# GH524 Product Spec: 原子化 Agent-facing Skill Change Convergence Workflow

Issue: https://github.com/majiayu000/loom/issues/524
Depends on: #522 contract decision
Related: #454, #478, #497, #498, #512, #523
Status: Maintainer architecture decisions approved; Draft spec amendment
Locale: zh-CN
Complexity: large

## 问题

一次 Skill 修改后，agent 需要自行组合 direction detection、source commit、active projection
枚举与刷新、visibility/reload 检查、可选 registry remote sync。任一步失败都可能留下部分完成
状态，而现有成功 envelope 无法一次证明所有请求轴已经收敛。

## 目标

1. 提供一个 durable-plan-first、幂等、scope-bounded 的 Skill convergence workflow。
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

1. **B-001** durable plan 必须解析一个 Skill、输入方向、精确 target/binding/projection 集合、每项
   method、visibility/reload 检查和 remote policy；任何未解析 selector 必须阻断 apply。
2. **B-002** 默认 source 方向只读取 canonical registry source；projection 内容只有在调用者
   显式选择 projection input 且唯一定位 instance 时才能成为 capture 输入。
3. **B-003** source 与任一 projection 同时 dirty，或多个 projection 提供不一致 dirty 内容时，
   workflow 必须返回 conflict 与逐项 evidence；不得自动选择“最新”一方。
4. **B-004** `plan converge` 只允许持久化 immutable durable plan 与 command audit；不得修改
   source、registry domain state、operation backlog、Git ref/index、live path 或 remote。plan 输出
   必须与 apply 使用同形 effect plan。
5. **B-005** apply 必须要求 `plan_id`、匹配的非空 `plan_digest` 与非空 idempotency key；同一 key +
   同一 plan 重试返回原结果且不重复 commit、projection swap、operation record 或 remote push；
   digest 不匹配或同一 key 用于不同 plan 必须阻断。
6. **B-006** apply 开始后必须持有 workspace/Skill 写锁，并验证 durable plan evidence 中的 source
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
    `local_complete_remote_pending` 与可幂等重试的 next action。若 required runtime 同时处于未接受的
    `restart_required`，必须保留两个 blocker，并返回
    `local_complete_remote_pending_restart_required`；不得用单一状态覆盖另一轴。
13. **B-013** target ownership、policy、approval 或 filesystem safety 阻断时，apply 不得绕过
    gate、降级 method 或扩大 scope；错误必须指出被阻断的 effect。
14. **B-014** 中断发生在本地 commit 前时必须回滚 staging；发生在本地 commit 后时必须能通过
    `convergence_id` 恢复/重试，且不得产生第二个 source commit。
15. **B-015** workflow complete 只表示本次声明的 required axes 均有成功 evidence；默认情况下
    `restart_required` 不满足 required runtime axis，并返回 `local_complete_restart_required` 与
    `complete=false`。只有 durable plan 显式记录 `accept_restart_required=true` 时才可返回
    `complete_with_restart_required` 与 `complete=true`；visibility state 始终保持
    `restart_required`，不得改写为 `visible`。

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

维护者批准的公共表面：

```bash
loom --json --root "$ROOT" plan converge "$SKILL"
loom --json --root "$ROOT" apply "$PLAN_ID" --plan-digest "$PLAN_DIGEST" --idempotency-key "$KEY"
```

selectors、runtime requirement、`--accept-restart-required` 与 `--push-remote` 都在 plan 阶段声明并
进入 digest；apply 不接受扩大或改变这些 effect 的参数。

## 验收标准

1. 一个 workflow 能把 source 修改收敛到全部选定 active projections，并返回 #522 三轴状态。
2. symlink、copy、materialize 的正负路径都由 E2E fixture 覆盖。
3. source/projection 双 dirty、projection 间分歧、lock contention、stale plan、filesystem failure、
   remote unavailable、restart required 均有确定输出。
4. fault injection 证明本地阶段不会留下无法解释的半完成成功。
5. `plan converge` 只能新增 durable plan/audit evidence；前后 source、registry domain state、Git、
   live target 与 remote snapshot 完全一致。
6. 未接受与显式接受 `restart_required` 的 fixture 分别返回
   `local_complete_restart_required`/`complete=false` 和
   `complete_with_restart_required`/`complete=true`，两者 visibility state 都保持
   `restart_required`。
7. remote pending 与未接受 restart 同时发生时，结果包含两个稳定排序的 completion blocker、两个
   next action 和 `local_complete_remote_pending_restart_required`；接受 restart 只移除 visibility
   blocker，不得把 remote pending 变成 complete。

## Maintainer 架构决策（2026-07-16）

1. 使用现有 durable `plan`/`apply` authority：新增 `plan converge`，不新增 `skill converge` leaf。
2. `restart_required` 默认不满足 required runtime axis；只有 plan 显式
   `accept_restart_required=true` 时可作为明确接受的完成状态，且绝不伪装成 `visible`。
