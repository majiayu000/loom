# GH522 Product Spec: Registry Sync 与 Runtime Convergence 状态分离

Issue: https://github.com/majiayu000/loom/issues/522
Status: Draft for maintainer review
Locale: zh-CN
Complexity: medium

## 问题

Loom 当前使用 `meta.sync_state` 表示 registry Git remote 的状态，但 Skill
是否已经投影到 agent 目录、agent 配置是否启用该 Skill、当前 session 是否需要
reload，属于另外两个状态轴。三个状态分散在 `sync status`、projection observation、
`skill visibility` 和 reconcile 输出中，agent 容易把 `SYNCED` 错读成“运行时已加载
最新 Skill”。

## 目标

1. 用稳定、互不冒充的字段表达 registry transport、projection convergence、agent
   visibility/reload 三个状态轴。
2. 让人类和 agent 在单次读取中区分“远端已同步”“文件已收敛”“当前 agent 是否可用”。
3. 为现有 `meta.sync_state` 和自动化消费者提供保守、可测试的迁移路径。

## 非目标

1. 本 issue 不实现一次修改后的原子 convergence workflow；该能力由 #524 负责。
2. 本 issue 不改变 projection materialization、Git push 或 adapter reconcile 的写语义。
3. 本 issue 不把 filesystem presence 当成当前 session 已加载的证明。
4. 本 issue 不移除 `sync push/pull/replay`，除非后续独立 breaking-change 规格批准。

## 行为不变量

1. **B-001** 所有声明 convergence 状态的公共 JSON 必须分别提供
   `registry_transport`、`projections`、`visibility` 三个命名状态轴；字段缺失不得解释为
   healthy 或 synced。
2. **B-002** `registry_transport.state` 只描述 registry Git remote 与 registry operation
   backlog，取值必须来自闭集；它不得由 projection 或 visibility 证据推导。
3. **B-003** `projections.state` 必须由被选择 projection 的实时存在性、method 与内容/链接
   证据推导；remote ahead/behind 不得改变该状态。
4. **B-004** `visibility.state` 必须区分 `visible`、`not_visible`、`restart_required`、
   `unsupported`、`unknown`；仅有 projection 证据时最多报告 `unknown`，不得报告
   `visible`。
5. **B-005** `registry_transport=SYNCED` 与 `projections!=converged` 的组合是合法且必须原样
   返回的状态，不能被折叠为整体成功。
6. **B-006** 没有 active projection 时，`projections` 必须返回显式的 `not_applicable` 与
   空 `items`，不能通过字段缺失表达；没有 adapter visibility 能力时必须返回
   `unsupported` 而不是 error 或 visible。
7. **B-007** 任一状态轴的底层证据读取失败时，该轴必须返回 `error` 或 `unknown` 及
   结构化原因；其他轴仍可返回其独立证据，但整体不得宣称 clean convergence。
8. **B-008** `meta.sync_state` 在兼容期继续保持原有 registry transport 语义，并与
   `registry_transport.state` 一致；新消费者必须以新字段为准，兼容期和移除条件必须在
   CLI contract 中明确。
9. **B-009** read-only status、inspect、diagnose、visibility 请求不得修改 registry state、
   Git refs/index、live targets 或 operation backlog；重复读取在外部状态不变时返回等价
   分类。
10. **B-010** 每个状态轴必须携带足够的新鲜度证据（至少 observed revision/digest 或
    timestamp）；并发修改使证据失效时，输出必须标记 stale/unknown，不能复用旧 healthy
    结论。
11. **B-011** 旧客户端只读取 `meta.sync_state` 时仍获得与升级前相同的 registry transport
    决策；新字段不得改变旧字段取值或退出码。
12. **B-012** 文档、Panel label 和 shipped `loom-registry` Skill 必须将 `sync` 描述为
    registry transport，并明确 projection/visibility 是独立检查。
13. **B-013** 若读取在中断或部分完成时只收集到部分状态轴，响应必须记录哪些轴未完成；
    不得用已完成轴替代未完成轴，也不得留下持久化的“已收敛”证据。

## 边界清单

| 边界 | 判定 |
| --- | --- |
| Empty / missing input | covered: B-001, B-006 |
| Error and failure paths | covered: B-007 |
| Authorization / permission | N/A：本 issue 只定义 read/status 语义，不新增写权限 |
| Concurrency / race / ordering | covered: B-010 |
| Retry / repetition / idempotency | covered: B-009 |
| Illegal state transitions | covered: B-002, B-003, B-004 |
| Compatibility / migration | covered: B-008, B-011 |
| Degradation / fallback | covered: B-004, B-007 |
| Evidence and audit integrity | covered: B-005, B-010 |
| Cancellation / interruption | covered: B-013 |

## 验收标准

1. fixture 可以稳定表达并验证 `remote synced + projection stale`、`projection converged +
   restart required`、`local only + projection converged` 三种交叉状态。
2. CLI contract、Agent Skill 和 Panel 对三类状态使用相同词汇。
3. 兼容测试证明旧 `meta.sync_state` 消费者不发生行为变化。
4. 任一轴证据缺失或读取失败时，测试证明系统不会产生完整 convergence 成功声明。

## 开放问题

1. 新状态聚合对象应放在 `meta.convergence`，还是放在命令数据的稳定顶层；技术规格提出
   默认选择，维护者评审后冻结。
2. `meta.sync_state` 的移除是否需要等到下一个 major version。
