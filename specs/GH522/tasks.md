# GH522 Tasks: Registry Sync 与 Runtime Convergence 状态分离

Issue: https://github.com/majiayu000/loom/issues/522
Product spec: `specs/GH522/product.md`
Tech spec: `specs/GH522/tech.md`
Status: Draft for maintainer review

## 顺序

typed read model → evidence join → CLI/API/Panel integration → compatibility → docs/Agent Skill →
verification and review。

## Implementation Tasks

- [ ] `SP522-T001` Owner: core status model | Dependencies: approved product/tech specs | Done when: 三个独立状态枚举与 `ConvergenceStatus` typed model 落地，缺失字段不能序列化成 healthy | Verify: `cargo test convergence_status_shape` | Covers: B-001, B-002, B-003, B-004, B-006
- [ ] `SP522-T002` Owner: evidence join | Dependencies: SP522-T001 | Done when: remote/backlog、projection observation、adapter visibility 通过一个 read-only join 聚合，并携带 revision/digest/timestamp | Verify: `cargo test convergence_status` | Covers: B-002, B-003, B-004, B-005, B-007, B-010, B-013
- [ ] `SP522-T003` Owner: CLI/API integration | Dependencies: SP522-T002 | Done when: `workspace status`、`skill inspect`、`skill diagnose`、`skill visibility` 和对应 API 返回同形对象，空/unsupported 显式表达 | Verify: `cargo test --test status --test skill_inspect --test skill_diagnose` | Covers: B-001, B-006, B-007, B-009
- [ ] `SP522-T004` Owner: compatibility | Dependencies: SP522-T002 | Done when: `meta.sync_state` 与新 registry transport 字段由同一值生成，旧消费者 fixture 和退出码不变 | Verify: `cargo test legacy_sync_state_matches_registry_transport && cargo test --test cli_surface` | Covers: B-008, B-011
- [ ] `SP522-T005` Owner: Panel | Dependencies: SP522-T003 | Done when: Panel 使用新 typed payload，不从单一 sync 字段推导 projection/visibility；旧服务器回退显示 unknown | Verify: `cd panel && bun test` | Covers: B-001, B-004, B-005, B-007, B-012
- [ ] `SP522-T006` Owner: docs and Agent Skill | Dependencies: SP522-T003, SP522-T004 | Done when: CLI/API contract、runbook、Panel labels、`loom-registry` Skill 使用一致三轴词汇并包含交叉状态示例 | Verify: `cargo test --test cli_surface --test shipped_registry_skill && git diff --check` | Covers: B-008, B-011, B-012

## Verification Tasks

- [ ] `SP522-T007` Owner: test | Dependencies: SP522-T001..T006 | Done when: 正负 fixture 覆盖 remote synced + projection stale、projection converged + restart required、local only + projection converged、axis read failure、race、interruption | Verify: `cargo test --test convergence_status` | Covers: B-005, B-006, B-007, B-009, B-010, B-013
- [ ] `SP522-T008` Owner: integration verification | Dependencies: SP522-T007 | Done when: Rust、Panel、E2E 与格式检查 fresh pass，且无新 mutation 出现在 read paths | Verify: `cargo check --workspace --all-targets --all-features && cargo test && cd panel && bun test && cd .. && ./scripts/e2e-agent-flow.sh && cargo fmt --all -- --check` | Covers: B-009, B-011, B-012

## Handoff

- Product invariant set: `B-001..B-013`。
- Task coverage union: `B-001..B-013`。
- Human gate: 维护者批准 product/tech spec 后才能把任务状态改为 implementation-ready。
