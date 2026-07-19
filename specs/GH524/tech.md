# GH524 Tech Spec: 原子化 Agent-facing Skill Change Convergence Workflow

Issue: https://github.com/majiayu000/loom/issues/524
Product spec: `specs/GH524/product.md`
Status: Maintainer architecture decisions approved; Draft spec amendment

## Codebase Context

| Area | Current evidence |
| --- | --- |
| Direction detection | `src/commands/skill_cmds/commit.rs:9-66` 自动判断 source/projection dirty；`:84-120` capture 单一 selected projection |
| Projection fan-out primitive | `src/commands/projection_executor.rs:84-110` 是 #497 后的单一 executor；`:150-176` 写 projection record；`:178-237` 记录 state/audit/autosync |
| Visibility plan | `src/commands/agent_cmds.rs:196-218` 已提供 adapter-driven read-only reconcile plan |
| Existing idempotency | `src/commands/plan_cmds.rs:230-296` 通过 plan id + idempotency digest replay/阻断 key 冲突 |
| Core facades | `src/core/lifecycle.rs:25-43` 与 `src/core/projection.rs:21-56` 仍回调 `App` 单操作入口，尚无跨域 orchestrator |
| Rollback precedent | #478 的 `rollback_reconciliation` 已定义 source rollback 后 safe projection reapply 与 recovery evidence，但只覆盖 rollback |

## 设计决策

### 1. Public workflow

扩展现有 durable `plan`/`apply` authority，新增 `loom plan converge` leaf；不新增
`loom skill converge`，也不增加 `loom skill` command-budget 例外。workflow 表达“使一次 Skill
change 在已选 active runtime 中收敛”的独立意图，不替代 `commit/project/reconcile/sync` 低层原语。

```text
loom plan converge <skill>
  [--from-source | --from-projection --instance <id>]
  [--agent <agent>] [--workspace <path>] [--profile <id>]
  [--require-runtime] [--accept-restart-required]
  [--push-remote]

loom apply <plan_id> --plan-digest <digest> --idempotency-key <key>
```

`plan converge` 持久化 immutable plan、`plan_id`、`plan_digest` 与 command audit，但不执行任何
domain mutation。convergence plan 标记 `requires_digest_confirmation=true`，因此 `apply` 必须同时
携带匹配的 id 与 digest；现有不要求 digest 的其他 plan kind 保持兼容。`--push-remote` 只在 plan
中声明最后 transport 阶段，apply 不接受新增 effect selector。

### 2. Typed convergence plan

新增 `src/core/convergence.rs` typed service：

```rust
struct SkillConvergencePlan {
    plan_id: String,
    plan_digest: String,
    skill: String,
    source: SourceGuard,
    projections: Vec<ProjectionEffectPlan>,
    visibility: Vec<VisibilityRequirement>,
    accept_restart_required: bool,
    remote: RemotePolicy,
    required_axes: BTreeSet<ConvergenceAxis>,
}
```

plan digest 覆盖 normalized selectors、source HEAD/tree digest、registry checkpoint、每个
projection instance/method/digest、policy/ownership decision、`accept_restart_required` 与 remote
policy。输出采用 #522 的三轴词汇。

### 3. Planning

1. 复用 `source_dirty_paths`/dirty projection detection，但把它们移入 typed core helper。
2. selector 解析只选择 active rules/bindings；顺序按 stable instance id 排序。
3. projection input 必须唯一 instance；多个 dirty projection 即使字节相同也要求显式选择，避免
   身份歧义。
4. 调用 projection executor 的 planning path，收集 backup/safety/policy requirements，但不写。
5. 调用 adapter visibility planner，形成 post-apply check requirements。
6. planning 只追加 immutable plan 与 command audit，不写 registry domain state、operation backlog、
   Git ref/index、live path 或 remote；apply 必须接收 caller 提交的 `plan_id` 与 `--plan-digest`，
   读取 stored plan 并在锁内根据相同输入重新计算。缺少 plan/digest 或比对不一致时必须在任何
   domain 写入前阻断；apply 不得绕过已审阅的 effect plan。

### 4. Local transaction

新增 `ConvergenceTransaction`，在 workspace lock + Skill lock 下执行：

1. 验证 HEAD/checkpoint/digests；
2. 对 projection-input capture 先复制到隔离 staging，不替换 source；
3. 运行 source lint/safety/preflight gates；
4. 为全部 copy/materialize projection 构建 sibling temp directories 并验证 digest；symlink 仅验证
   safe canonical target；
5. snapshot Git index/HEAD、registry state/audit 与所有将替换 live paths；
6. 提交/捕获 source 一次；
7. 按 stable order 原子 rename projection temp paths，upsert records；
8. 追加单一 aggregate `skill.converge` operation，写 observations 与一个 registry state commit；
9. 任一本地步骤失败时逆序恢复 live paths、registry/audit、Git index/HEAD/source。

#### 4.1 Crash-safe ownership ledger

每个 artifact owner 使用 journal-authoritative attempt，而不是可覆盖的固定 reservation token。
Journal 在创建前持久化 `allocated` 的 canonical UUID、精确 candidate/destination path、proof 与
manifest digest；proof、manifest、文件和 parent directory 完成 durable sync 后才持久化 `ready`；
发布仅允许同文件系统 atomic no-replace rename，随后持久化 `activated`。`allocated` 阶段留下的
partial candidate 标记为 `abandoned` 并使用新 generation 重试，不复用或按 prefix/glob 扫描。

恢复与 transaction cleanup 是逻辑终结，不是自动物理删除：精确 artifact 最终进入 `retained`，
未发布 attempt 进入 `abandoned`，journal phase 为 `committed_artifacts_retained` 或
`rolled_back_artifacts_retained`。同文件系统要求意味着 projection/source staging owner 可能保留在
对应 live parent 的隐藏路径；terminal journal 必须逐项列出其 exact path、proof、manifest 与状态。
恢复不得执行“验证 pathname 后删除”的 TOCTOU cleanup，也不得删除或跟随 foreign regular file、
directory 或 symlink。磁盘回收属于未来的显式、人工授权 GC；GC 未实现前 retained evidence 必须保留
并在 recovery evidence 中可见，不能报告成 zero-orphan 或 physical cleanup complete。

为避免每个 projection 独立 commit/autosync，给 #497 executor 增加内部
`ExecutionContext::{Standalone, Convergence}`。Convergence mode 只提供 materialize/validate/state
delta，不自行写 operation、commit 或 remote；所有公开旧入口继续使用 Standalone。

### 5. Visibility 与 remote phase

本地 transaction commit 后：

1. 重新执行 adapter visibility read；不支持时返回 `unsupported`，读取失败按 required axis 决定
   complete/partial。
2. `restart_required` 默认不满足 required runtime axis，返回
   `local_complete_restart_required`/`complete=false`；只有 plan 设置
   `accept_restart_required=true` 时才返回
   `complete_with_restart_required`/`complete=true`。两条路径的 visibility state 都保持
   `restart_required`，并返回 restart/recheck next action。
3. `--push-remote` 最后调用 registry transport service。失败不回滚本地已验证状态，aggregate
   operation 保持 unacked，并返回 `local_complete_remote_pending` 与 exact retry command。
4. completion 使用稳定排序的 `completion_blockers` 集合组合独立轴；remote pending 与未接受的
   restart 同时存在时返回 `local_complete_remote_pending_restart_required`，同时保留 transport retry
   与 restart/recheck next actions。显式接受 restart 只移除 visibility blocker，remote blocker 仍使
   `complete=false`。

### 6. Idempotency 与 recovery

扩展现有 command event lookup，key digest 与 `plan_id + plan_digest` 绑定：

- succeeded/partial terminal record：同 key 重放已记录 result，并对 pending remote 可执行同一
  convergence id 的 transport retry；
- in_progress 且 lease 未过期：返回 `LOCK_BUSY`；
- interrupted/expired：读取 transaction journal 的阶段、ownership attempt ledger 与 backup evidence，
  先 recovery 后重试；terminal retained journal 的同 plan replay 返回原 result，不重复副作用；
- key 用于不同 plan：`DEPENDENCY_CONFLICT`。

idempotency 原文始终 redacted，只持久化 digest。

### 7. Envelope

```json
{
  "convergence_id": "conv_...",
  "plan_id": "plan_...",
  "plan_digest": "sha256:...",
  "local_state": "complete",
  "outcome": "local_complete_remote_pending_restart_required",
  "completion_blockers": ["registry.remote_pending", "visibility.restart_required"],
  "source": {"commit": "...", "direction": "source"},
  "convergence": {
    "registry_transport": {"state": "PENDING_PUSH"},
    "projections": {"state": "converged", "items": []},
    "visibility": {"state": "restart_required"}
  },
  "complete": false,
  "next_actions": [
    {
      "cmd": "loom --json --root \"$ROOT\" apply \"$PLAN_ID\" --plan-digest \"$PLAN_DIGEST\" --idempotency-key \"$KEY\"",
      "reason": "retry the pending registry transport with the same immutable plan and idempotency key"
    },
    {
      "cmd": "loom --json --root \"$ROOT\" skill inspect \"$SKILL\" --agent \"$AGENT\"",
      "reason": "restart the affected agent runtime first, then recheck visibility"
    }
  ]
}
```

`complete` 按 plan required axes 和显式 acceptance policy 计算；warning 不影响计算，也不能覆盖失败
evidence。仅在不存在其他 completion blocker 时，接受 restart 后 `outcome` 为
`complete_with_restart_required`；若仍有 remote pending，则 `outcome` 为
`local_complete_remote_pending`、`complete=false`。visibility state 始终不变。

### 8. Compatibility and rollout

1. Additive `plan converge` leaf；旧 low-level commands 与现有 plan kind 行为保持，并按 #523
   contract policy 递增 agent-facing contract minor、登记 inventory/compatibility。
2. 先实现 durable planning + fault fixtures，再开放对应 plan kind 的 `apply`。
3. `loom-registry` Skill 在 #523 gate 下改用 convergence happy path，但保留低层 recovery 指引。
4. Panel 只在 backend 报告 apply capability 后显示 mutation action。

## Product-to-Test Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | convergence planner/selectors | `cargo test --test skill_convergence exact_effect_plan` |
| B-002 | direction resolver | `cargo test --test skill_convergence projection_input_requires_instance` |
| B-003 | dirty conflict evidence | `cargo test --test skill_convergence dirty_side_conflicts` |
| B-004 | durable plan boundary snapshots | `cargo test --test skill_convergence plan_only_writes_plan_and_audit` |
| B-005 | plan confirmation + idempotency event store | `cargo test --test skill_convergence apply_requires_reviewed_plan_digest && cargo test --test skill_convergence idempotent_replay_and_key_conflict` |
| B-006 | lock/stale guards | `cargo test --test skill_convergence stale_plan_and_lock_contention` |
| B-007 | executor convergence modes | `cargo test --test skill_convergence symlink_copy_materialize` |
| B-008 | fault-injected local transaction | `cargo test --test skill_convergence local_faults_restore_all_surfaces` |
| B-009 | aggregate journal/envelope | `cargo test --test skill_convergence convergence_evidence_is_complete` |
| B-010 | empty active set | `cargo test --test skill_convergence source_only_and_required_runtime` |
| B-011 | post-write adapter visibility | `cargo test --test skill_convergence visibility_and_restart_states` |
| B-012 | remote final phase | `cargo test --test skill_convergence remote_failure_preserves_local_completion && cargo test --test skill_convergence remote_pending_and_restart_blockers_compose` |
| B-013 | policy/ownership/approval fixtures | `cargo test --test skill_convergence gates_do_not_degrade_or_expand` |
| B-014 | interruption journal/recovery | `cargo test --test skill_convergence interrupted_recovery_is_single_commit` |
| B-015 | required-axis completion calculation | `cargo test --test skill_convergence restart_required_acceptance_is_explicit && cargo test --test skill_convergence remote_pending_and_restart_blockers_compose && cargo test --test skill_convergence complete_requires_declared_evidence` |

## 风险与回滚

1. **跨文件系统原子性**：每个 target 只能在其 parent 下创建 temp/backup；跨 target 使用可恢复
   transaction，不宣称 OS 级全局原子 rename。
2. **operation noise**：convergence mode 禁止 child executor autosync/commit，aggregate record 是
   authority。
3. **长锁时间**：所有昂贵构建先 staging；锁内只做 guard、commit 与 swap。
4. **remote irreversible boundary**：remote 永远最后执行，本地成功后 remote failure 进入 pending，
   不做危险 Git history rollback。
5. **回滚**：可移除 `PlanCommand::Converge` registration，保留 typed transaction/recovery evidence；
   旧 low-level commands 与既有 plan kind 未改变。

## 规格门禁

- 本 PR 只修改规格；不修改 CLI、state 或 release。
- 维护者已于 2026-07-16 批准 `plan converge` + `apply` public workflow，以及显式接受
  `restart_required` 的 completion policy。
- implementation-ready 仍依赖 #523 contract gate 和本 spec amendment 合并。
