# GH524 Tasks: 原子化 Agent-facing Skill Change Convergence Workflow

Issue: https://github.com/majiayu000/loom/issues/524
Product spec: `specs/GH524/product.md`
Tech spec: `specs/GH524/tech.md`
Status: Maintainer architecture decisions approved; Draft spec amendment

## 顺序

#522 状态契约批准 → typed durable planner → convergence executor mode/local transaction →
idempotency/recovery → visibility/remote → Agent Skill/Panel → full verification。

## Implementation Tasks

- [x] `SP524-T001` Owner: CLI/planner | Dependencies: approved GH524 specs, implemented #522 contract, #523 contract gate | Done when: `PlanCommand::Converge` 与 typed durable plan 落地，不新增 `skill converge`；selectors、required axes、plan id/digest 稳定，planning 只写 immutable plan/audit，且 apply 强制携带并验证该 plan id/digest | Verify: `cargo test --test skill_convergence exact_effect_plan plan_only_writes_plan_and_audit apply_requires_reviewed_plan_digest` | Covers: B-001, B-004, B-006, B-010, B-015
- [x] `SP524-T002` Owner: direction/source | Dependencies: SP524-T001 | Done when: canonical source 默认路径、projection instance 显式输入、双侧/多 projection dirty conflict 与 source preflight gates 完整 | Verify: `cargo test --test skill_convergence projection_input_requires_instance dirty_side_conflicts` | Covers: B-002, B-003, B-013
- [x] `SP524-T003` Owner: projection executor | Dependencies: SP524-T001 | Done when: #497 executor 支持 Standalone/Convergence 内部模式，symlink 验证、copy 原子替换、materialize 重建均不产生 child commit/autosync | Verify: `cargo test --test skill_convergence symlink_copy_materialize` | Covers: B-007, B-008, B-009
- [x] `SP524-T004` Owner: transaction/recovery | Dependencies: SP524-T002, SP524-T003 | Done when: workspace/Skill locks、HEAD/checkpoint/digest guards、staging、snapshot、逆序恢复与 interruption journal 落地；ownership attempts 按 allocated/ready/activated/abandoned/retained 持久化 exact path + manifest digest，terminal cleanup 保留可审计 evidence 且不执行 pathname-racy 自动删除；未来磁盘回收明确留给显式/manual GC | Verify: `cargo test --test skill_convergence stale_plan_and_lock_contention interrupted_recovery_is_single_commit owner_attempt_interruptions_recover_with_exact_retained_ledger crash_after_ready_proof_is_retryable_and_retains_exact_attempt nonexact_ready_attempts_block_activation_until_exact_retry local_faults_restore_all_surfaces preparation_failure_retains_exact_artifact_ledger` | Covers: B-006, B-008, B-014
- [ ] `SP524-T005` Owner: idempotency/audit | Dependencies: SP524-T004 | Done when: key digest 绑定 plan id + digest，single `convergence_id`/aggregate operation/evidence 落地，replay 不重复副作用 | Verify: `cargo test --test skill_convergence -- idempotent_replay_and_key_conflict convergence_evidence_is_complete` | Covers: B-005, B-009, B-014
  - [x] idempotency binding（key digest 绑定 plan id + plan digest，binding mismatch fail closed）与 derived `convergence_id`、replay 零副作用已落地。
  - [ ] **BLOCKED — 需要架构决策**：aggregate operation record。证据见下方 “B-009 aggregate record 阻塞点”。
- [ ] `SP524-T006` Owner: visibility/transport | Dependencies: SP524-T005, #522 implementation | Done when: post-write adapter visibility 与 remote-last phase 落地；未接受 restart 返回 `local_complete_restart_required`/`complete=false`，显式接受返回 `complete_with_restart_required`/`complete=true`，两者 visibility 均保持 `restart_required`；not_requested、remote pending 与 remote+restart 组合 blocker 精确返回 | Verify: `cargo test --test skill_convergence visibility_and_restart_states restart_required_acceptance_is_explicit remote_failure_preserves_local_completion remote_pending_and_restart_blockers_compose complete_requires_declared_evidence` | Covers: B-011, B-012, B-015
- [ ] `SP524-T007` Owner: policy/scope | Dependencies: SP524-T001..T006 | Done when: ownership、policy、approval、filesystem gates fail closed，apply 不扩大 plan selectors 或降级 method | Verify: `cargo test --test skill_convergence gates_do_not_degrade_or_expand` | Covers: B-001, B-006, B-013
- [ ] `SP524-T008` Owner: Agent Skill/Panel/docs | Dependencies: SP524-T006, #523 gate | Done when: shipped Skill 使用 convergence happy path，Panel capability-gated，CLI/API docs 与 recovery 指引同步 | Verify: `cargo test --test shipped_registry_skill --test cli_surface && cd panel && bun test` | Covers: B-001, B-009, B-011, B-012, B-015

## Verification Tasks

- [ ] `SP524-T009` Owner: fault/E2E | Dependencies: SP524-T001..T008 | Done when: 三种 method、双 dirty、lock/stale、每个本地 fault point、remote unavailable、restart required、interrupt/retry 全部由 fresh fixtures 覆盖 | Verify: `cargo test --test skill_convergence --test reliability && ./scripts/e2e-agent-flow.sh` | Covers: B-003, B-005, B-006, B-007, B-008, B-010, B-011, B-012, B-014
- [ ] `SP524-T010` Owner: final verification | Dependencies: SP524-T009 | Done when: Rust/Panel/full test/format/perf fresh pass，PR gate 证明未新增 `skill` leaf、`plan converge` 已登记 #523 inventory/contract minor 与 invariant coverage | Verify: `cargo check --workspace --all-targets --all-features && cargo test && cd panel && bun test && cd .. && cargo fmt --all -- --check && ./scripts/perf-smoke.sh` | Covers: B-004, B-009, B-013, B-015

## Handoff

- Product invariant set: `B-001..B-015`。
- Task coverage union: `B-001..B-015`。
- Maintainer architecture gates resolved on 2026-07-16: `plan converge` + `apply` public workflow，
  explicit `restart_required` acceptance policy。
- Completed implementation tranches: SP524-T001 typed planning/digest-confirmation boundary,
  SP524-T002 direction/input-preflight evidence, SP524-T003 projection executor mode,
  SP524-T004 transaction/recovery, and the SP524-T005 idempotency binding half.
- Remaining gates: the SP524-T005 aggregate record（阻塞于下方架构决策）与
  SP524-T006..T010 implementation and implementation PR review/merge.

## B-009 aggregate record 阻塞点

B-009 要求每次 apply 用单一 `convergence_id` 记录聚合 evidence。把它实现为 registry
operations ledger 中的一行，与 T003/T004 已确立的提交语义冲突。

**实测证据**（本 worktree，projected fixture，apply 后 `git status --porcelain`）：

- 不写聚合记录：registry 干净，只有未跟踪的 `state/events/`、`state/transactions/`。
- 写聚合记录：`M state/registry/ops/checkpoint.json`、`M state/registry/ops/operations.jsonl`。

原因链：

1. `record_registry_operation`（`projections.rs:238`）同时追加 `operations.jsonl` 并更新
   `checkpoint.json` 的 `last_scanned_op_id`。
2. convergence 提交只暂存 `state/registry/projections.json`
   （`registry_commit.rs:5` `REGISTRY_PATH`）与 `skills/<name>`
   （`source_commit.rs:15`）。`state/registry/ops/` 不在任何 convergence 提交里。
3. `validate_routing_paths_clean`（`guards.rs:154`）把
   `state/registry/ops/checkpoint.json` 列为必须干净的 routing path，因此下一个 plan 报
   `PLAN_CHECKPOINT_DRIFT`。
4. convergence 模式本来就不写 ops ledger：`projection_executor.rs:267-288` 在
   `M::CONVERGENCE` 分支提前 return，永远到不了 `:313` 的 `record_registry_operation`。
   这正是 SP524-T003 “不产生 child commit/autosync” 的既定属性。

三种写入位置均已实测（after registry commit / before registry commit / 与 projection
同批次），分别为 122、59、57 通过（共 125）。位置调整不能解决，问题在提交范围本身。

**待决策（二选一）**：

- **A. 扩大 convergence registry 提交范围**，把 `state/registry/ops/` 纳入
  `commit_convergence_registry` 的暂存路径，聚合记录在提交前写入。改动 T004 的提交契约与
  recovery/rollback 的 mutated-surface 校验范围，需要重新审视 T004 的 fault fixtures。
- **B. 改用其他持久面满足 B-009**，不新增 ledger 行——`convergence_id` 已在 apply envelope
  与 transaction journal 中，可由读侧聚合。不触碰提交路径，但需要修改 B-009 中
  “aggregate operation” 的措辞。

建议 B：ops ledger 在 convergence 下是刻意留空的（第 4 点），往里写一行会重新引入 T003 明确
移除的耦合；而 envelope + journal 已经承载了 B-009 要求的全部 evidence。
