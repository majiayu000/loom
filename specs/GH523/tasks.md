# GH523 Tasks: 防止 CLI、Agent Skill 与生命周期文档契约漂移

Issue: https://github.com/majiayu000/loom/issues/523
Product spec: `specs/GH523/product.md`
Tech spec: `specs/GH523/tech.md`
Status: Draft for maintainer review

## 顺序

contract identity → surface inventory/parser checker → Skill compatibility → release pairing → CI gate。

## Implementation Tasks

- [ ] `SP523-T001` Owner: CLI contract | Dependencies: approved specs | Done when: 单一 `CLI_CONTRACT_VERSION` 以 `cli_contract_version` 字段进入 JSON envelope，breaking/additive 规则写入 CLI contract | Verify: `cargo test cli_contract_version_is_exposed_and_declared` | Covers: B-001, B-010
- [ ] `SP523-T002` Owner: Agent Skill | Dependencies: SP523-T001 | Done when: `loom.skill.toml` 声明 contract range，Skill 对 missing/out-of-range CLI 仅允许 read-only diagnosis 并阻止 mutation | Verify: `cargo test --test shipped_registry_skill --test agent_contract_surfaces incompatible_cli_blocks_mutation` | Covers: B-001, B-002, B-009, B-013
- [ ] `SP523-T003` Owner: surface inventory | Dependencies: SP523-T001 | Done when: review-owned inventory 覆盖所有 active agent-facing surfaces，classification 闭集且空 inventory 失败 | Verify: `cargo test --test agent_contract_surfaces inventory_covers_public_surfaces` | Covers: B-003, B-006, B-013
- [ ] `SP523-T004` Owner: parser checker | Dependencies: SP523-T003 | Done when: executable 示例与实际 `next_actions` 使用 Clap parser 验证，错误定位到 stable id/file/line/argv | Verify: `cargo test --test agent_contract_surfaces executable_examples_parse removed_commands_fail parse_failure_is_terminal` | Covers: B-004, B-005, B-006, B-008
- [ ] `SP523-T005` Owner: checker safety | Dependencies: SP523-T004 | Done when: checker 使用临时 HOME/root，重复运行确定且不改 source/index/refs/home | Verify: `cargo test --test agent_contract_surfaces checker_is_read_only_and_repeatable checker_never_rewrites_sources` | Covers: B-007, B-011, B-012
- [ ] `SP523-T006` Owner: release | Dependencies: SP523-T001..T005 | Done when: archive 包含匹配的 binary/Skill/contract manifest，smoke 校验 range、digest 与 mismatch 负例 | Verify: local release archive smoke + `cargo test packaged_contract_mismatch_fails release_manifest_is_atomic_and_untracked` | Covers: B-001, B-002, B-009, B-010, B-012, B-013

## Verification Tasks

- [ ] `SP523-T007` Owner: CI | Dependencies: SP523-T006 | Done when: contract checks 接入 `make check` 和 release workflow，故意加入 `skill save`、错误 flag、空 inventory、旧新版本错配均阻断 | Verify: `make check` and release workflow fixture job | Covers: B-003, B-004, B-005, B-008, B-009, B-013
- [ ] `SP523-T008` Owner: docs/review | Dependencies: SP523-T007 | Done when: ADR/CLI contract/CHANGELOG 说明 version policy，所有高上下文文件修改均为显式 patch | Verify: `git diff --check && cargo fmt --all -- --check` | Covers: B-010, B-011

## Handoff

- Product invariant set: `B-001..B-013`。
- Task coverage union: `B-001..B-013`。
- Human gate: 维护者批准 contract version 与 inventory ownership 后才能实现。
