# GH595 Tasks - 统一 workspace binding 匹配语义

Issue: https://github.com/majiayu000/loom/issues/595
Product spec: `specs/GH595/product.md`
Tech spec: `specs/GH595/tech.md`
Status: `implx auto`

## Order

契约与调用点清单 -> matcher normalization/error API -> 全 consumer 迁移 -> 边界测试 -> 验证与审计。

## Tasks

- [x] `SP595-T1` Owner: spec | Dependencies: none | Done when: product/tech/tasks 记录 `B-001..B-009`、完整 call-site inventory、完整 planned-changes manifest 与 NotFound-only/error 契约 | Verify: product ID set 与 task Covers union 都是 `B-001..B-009`；manifest `issue=595,complete=true` 且路径非空 | Covers: B-001, B-003, B-004, B-006, B-007, B-009
- [x] `SP595-T2` Owner: state model | Dependencies: SP595-T1 | Done when: `RegistryWorkspaceMatcher` 是唯一 project matcher，实现 relative anchoring、symlink resolution、trailing slash 与 deepest-existing-ancestor suffix，且非 NotFound 错误保持 typed | Verify: `cargo test --locked state_model::matches_workspace_tests -- --nocapture` | Covers: B-001, B-002, B-003, B-004, B-005
- [x] `SP595-T3` Owner: command consumers | Dependencies: SP595-T2 | Done when: inventory 中全部 project Registry consumers 使用权威 API；所有 owning command/result boundary 传播 `IO_ERROR`；optional workspace 的 `None` 保持不筛选 | Verify: focused command tests + matcher call-site `rg` audit | Covers: B-001, B-004, B-008, B-009
- [x] `SP595-T4` Owner: scope compatibility | Dependencies: SP595-T2 | Done when: activation、workflow、visibility 的 `name=user` scope marker 显式保留且与 project basename 语义有注释/测试；inventory serialized matchers serde decode 并复用 API | Verify: user/project scope tests + inventory malformed/symlink tests | Covers: B-005, B-006, B-007, B-008
- [x] `SP595-T5` Owner: verification | Dependencies: SP595-T2, SP595-T3, SP595-T4 | Done when: symlink、relative、trailing slash、missing suffix、permission denied、symlink loop 和至少一个命令级遗漏 consumer 都有回归；fresh fmt/check/focused tests 与 diff audit 通过 | Verify: `cargo fmt --all -- --check && cargo check --locked && git diff --check`，并执行 tech spec 的 focused tests 与 `rg` audit | Covers: B-001, B-002, B-003, B-004, B-005, B-006, B-007, B-008, B-009

## Handoff

- Product invariant set: `B-001..B-009`.
- Task coverage union: `B-001..B-009`.
- `implx auto` 授权本轮规范起草与实现；merge 仍由队列 coordinator 在当前 CI、review-thread、
  independent review 与 PR gate 全绿后执行。
