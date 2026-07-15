# GH524 Tech Spec: 原子化 Agent-facing Skill Change Convergence Workflow

Issue: https://github.com/majiayu000/loom/issues/524
Product spec: `specs/GH524/product.md`
Status: Draft for maintainer review

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

### 1. Public command

新增 `loom skill converge` leaf，并在 `docs/LOOM_ARCHITECTURE_DECISIONS.md` 记录 command-surface
budget 例外与 sunset 条件。它表达“使一次 Skill change 在已选 active runtime 中收敛”的独立
意图，不替代 `commit/project/reconcile/sync` 低层原语。

```text
loom skill converge <skill>
  [--from-source | --from-projection --instance <id>]
  [--agent <agent>] [--workspace <path>] [--profile <id>]
  [--require-runtime] [--accept-restart-required]
  [--push-remote]
  --dry-run

loom skill converge <skill> ... --apply --idempotency-key <key>
```

`--dry-run` 与 `--apply` 必须二选一。`--push-remote` 只控制最后 transport 阶段。

### 2. Typed convergence plan

新增 `src/core/convergence.rs` typed service：

```rust
struct SkillConvergencePlan {
    plan_digest: String,
    skill: String,
    source: SourceGuard,
    projections: Vec<ProjectionEffectPlan>,
    visibility: Vec<VisibilityRequirement>,
    remote: RemotePolicy,
    required_axes: BTreeSet<ConvergenceAxis>,
}
```

plan digest 覆盖 normalized selectors、source HEAD/tree digest、registry checkpoint、每个
projection instance/method/digest、policy/ownership decision 与 remote policy。输出采用 #522 的
三轴词汇。

### 3. Planning

1. 复用 `source_dirty_paths`/dirty projection detection，但把它们移入 typed core helper。
2. selector 解析只选择 active rules/bindings；顺序按 stable instance id 排序。
3. projection input 必须唯一 instance；多个 dirty projection 即使字节相同也要求显式选择，避免
   身份歧义。
4. 调用 projection executor 的 planning path，收集 backup/safety/policy requirements，但不写。
5. 调用 adapter visibility planner，形成 post-apply check requirements。
6. dry-run 不持久化 plan；apply 根据相同输入重新计算并比对 caller 提交的 plan digest。CLI 可将
   dry-run 返回的 digest 作为 `--plan-digest` 可选强 guard；缺失时 apply 仍在锁内重算。

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

为避免每个 projection 独立 commit/autosync，给 #497 executor 增加内部
`ExecutionContext::{Standalone, Convergence}`。Convergence mode 只提供 materialize/validate/state
delta，不自行写 operation、commit 或 remote；所有公开旧入口继续使用 Standalone。

### 5. Visibility 与 remote phase

本地 transaction commit 后：

1. 重新执行 adapter visibility read；不支持时返回 `unsupported`，读取失败按 required axis 决定
   complete/partial。
2. `restart_required` 只有 plan 设置 `accept_restart_required=true` 时满足 required runtime axis。
3. `--push-remote` 最后调用 registry transport service。失败不回滚本地已验证状态，aggregate
   operation 保持 unacked，并返回 `local_complete_remote_pending` 与 exact retry command。

### 6. Idempotency 与 recovery

扩展现有 command event lookup，key digest 与 `plan_digest` 绑定：

- succeeded/partial terminal record：同 key 重放已记录 result，并对 pending remote 可执行同一
  convergence id 的 transport retry；
- in_progress 且 lease 未过期：返回 `LOCK_BUSY`；
- interrupted/expired：读取 transaction journal 的阶段与 backup evidence，先 recovery 后重试；
- key 用于不同 plan：`DEPENDENCY_CONFLICT`。

idempotency 原文始终 redacted，只持久化 digest。

### 7. Envelope

```json
{
  "convergence_id": "conv_...",
  "plan_digest": "sha256:...",
  "local_state": "complete",
  "source": {"commit": "...", "direction": "source"},
  "convergence": {
    "registry_transport": {"state": "PENDING_PUSH"},
    "projections": {"state": "converged", "items": []},
    "visibility": {"state": "restart_required"}
  },
  "complete": false,
  "next_actions": []
}
```

`complete` 按 plan required axes 计算；warning 不影响计算，也不能覆盖失败 evidence。

### 8. Compatibility and rollout

1. Additive command；旧 low-level commands 行为保持。
2. 先实现 dry-run + fault fixtures，再开放 `--apply`。
3. `loom-registry` Skill 在 #523 gate 下改用 convergence happy path，但保留低层 recovery 指引。
4. Panel 只在 backend 报告 apply capability 后显示 mutation action。

## Product-to-Test Mapping

| Behavior invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | convergence planner/selectors | `cargo test --test skill_convergence exact_effect_plan` |
| B-002 | direction resolver | `cargo test --test skill_convergence projection_input_requires_instance` |
| B-003 | dirty conflict evidence | `cargo test --test skill_convergence dirty_side_conflicts` |
| B-004 | dry-run write snapshots | `cargo test --test skill_convergence dry_run_is_read_only` |
| B-005 | idempotency event store | `cargo test --test skill_convergence idempotent_replay_and_key_conflict` |
| B-006 | lock/stale guards | `cargo test --test skill_convergence stale_plan_and_lock_contention` |
| B-007 | executor convergence modes | `cargo test --test skill_convergence symlink_copy_materialize` |
| B-008 | fault-injected local transaction | `cargo test --test skill_convergence local_faults_restore_all_surfaces` |
| B-009 | aggregate journal/envelope | `cargo test --test skill_convergence convergence_evidence_is_complete` |
| B-010 | empty active set | `cargo test --test skill_convergence source_only_and_required_runtime` |
| B-011 | post-write adapter visibility | `cargo test --test skill_convergence visibility_and_restart_states` |
| B-012 | remote final phase | `cargo test --test skill_convergence remote_failure_preserves_local_completion` |
| B-013 | policy/ownership/approval fixtures | `cargo test --test skill_convergence gates_do_not_degrade_or_expand` |
| B-014 | interruption journal/recovery | `cargo test --test skill_convergence interrupted_recovery_is_single_commit` |
| B-015 | required-axis completion calculation | `cargo test --test skill_convergence complete_requires_declared_evidence` |

## 风险与回滚

1. **跨文件系统原子性**：每个 target 只能在其 parent 下创建 temp/backup；跨 target 使用可恢复
   transaction，不宣称 OS 级全局原子 rename。
2. **operation noise**：convergence mode 禁止 child executor autosync/commit，aggregate record 是
   authority。
3. **长锁时间**：所有昂贵构建先 staging；锁内只做 guard、commit 与 swap。
4. **remote irreversible boundary**：remote 永远最后执行，本地成功后 remote failure 进入 pending，
   不做危险 Git history rollback。
5. **回滚**：可隐藏/移除新 command registration，保留 typed transaction/recovery evidence；旧
   low-level commands 未改变。

## 规格门禁

- 本 PR 只新增规格；不修改 CLI、state 或 release。
- 新 command leaf 与 `restart_required` completion policy 需要维护者明确批准。
