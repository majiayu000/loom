# GH524 Tasks: 原子化 Agent-facing Skill Change Convergence Workflow

Issue: https://github.com/majiayu000/loom/issues/524
Product spec: `specs/GH524/product.md`
Tech spec: `specs/GH524/tech.md`
Status: Draft for maintainer review

## 顺序

#522 状态契约批准 → typed planner/dry-run → convergence executor mode/local transaction →
idempotency/recovery → visibility/remote → Agent Skill/Panel → full verification。

## Implementation Tasks

- [ ] `SP524-T001` Owner: CLI/planner | Dependencies: approved GH524 specs, approved #522 contract | Done when: `skill converge` command/ADR 例外与 typed plan 落地，selectors、required axes、plan digest 稳定且 dry-run 只读 | Verify: `cargo test --test skill_convergence exact_effect_plan dry_run_is_read_only` | Covers: B-001, B-004, B-010, B-015
- [ ] `SP524-T002` Owner: direction/source | Dependencies: SP524-T001 | Done when: canonical source 默认路径、projection instance 显式输入、双侧/多 projection dirty conflict 与 source preflight gates 完整 | Verify: `cargo test --test skill_convergence projection_input_requires_instance dirty_side_conflicts` | Covers: B-002, B-003, B-013
- [ ] `SP524-T003` Owner: projection executor | Dependencies: SP524-T001 | Done when: #497 executor 支持 Standalone/Convergence 内部模式，symlink 验证、copy 原子替换、materialize 重建均不产生 child commit/autosync | Verify: `cargo test --test skill_convergence symlink_copy_materialize` | Covers: B-007, B-008, B-009
- [ ] `SP524-T004` Owner: transaction/recovery | Dependencies: SP524-T002, SP524-T003 | Done when: workspace/Skill locks、HEAD/checkpoint/digest guards、staging、snapshot、逆序恢复与 interruption journal 落地 | Verify: `cargo test --test skill_convergence stale_plan_and_lock_contention local_faults_restore_all_surfaces interrupted_recovery_is_single_commit` | Covers: B-006, B-008, B-014
- [ ] `SP524-T005` Owner: idempotency/audit | Dependencies: SP524-T004 | Done when: key digest 绑定 plan digest，single `convergence_id`/aggregate operation/evidence 落地，replay 不重复副作用 | Verify: `cargo test --test skill_convergence idempotent_replay_and_key_conflict convergence_evidence_is_complete` | Covers: B-005, B-009, B-014
- [ ] `SP524-T006` Owner: visibility/transport | Dependencies: SP524-T005, #522 implementation | Done when: post-write adapter visibility 与 remote-last phase 落地，restart policy、not_requested、remote pending 精确返回 | Verify: `cargo test --test skill_convergence visibility_and_restart_states remote_failure_preserves_local_completion complete_requires_declared_evidence` | Covers: B-011, B-012, B-015
- [ ] `SP524-T007` Owner: policy/scope | Dependencies: SP524-T001..T006 | Done when: ownership、policy、approval、filesystem gates fail closed，apply 不扩大 plan selectors 或降级 method | Verify: `cargo test --test skill_convergence gates_do_not_degrade_or_expand` | Covers: B-001, B-006, B-013
- [ ] `SP524-T008` Owner: Agent Skill/Panel/docs | Dependencies: SP524-T006, #523 gate | Done when: shipped Skill 使用 convergence happy path，Panel capability-gated，CLI/API docs 与 recovery 指引同步 | Verify: `cargo test --test shipped_registry_skill --test cli_surface && cd panel && bun test` | Covers: B-001, B-009, B-011, B-012, B-015

## Verification Tasks

- [ ] `SP524-T009` Owner: fault/E2E | Dependencies: SP524-T001..T008 | Done when: 三种 method、双 dirty、lock/stale、每个本地 fault point、remote unavailable、restart required、interrupt/retry 全部由 fresh fixtures 覆盖 | Verify: `cargo test --test skill_convergence --test reliability && ./scripts/e2e-agent-flow.sh` | Covers: B-003, B-005, B-006, B-007, B-008, B-010, B-011, B-012, B-014
- [ ] `SP524-T010` Owner: final verification | Dependencies: SP524-T009 | Done when: Rust/Panel/full test/format/perf fresh pass，PR gate 记录 command-budget exception 与 invariant coverage | Verify: `cargo check --workspace --all-targets --all-features && cargo test && cd panel && bun test && cd .. && cargo fmt --all -- --check && ./scripts/perf-smoke.sh` | Covers: B-004, B-009, B-013, B-015

## Handoff

- Product invariant set: `B-001..B-015`。
- Task coverage union: `B-001..B-015`。
- Human gates: #522 contract、new leaf ADR exception、`restart_required` completion policy、product/tech
  spec approval。
