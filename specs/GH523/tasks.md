# GH523 Tasks: 防止 CLI、Agent Skill 与生命周期文档契约漂移

Issue: https://github.com/majiayu000/loom/issues/523
Product spec: `specs/GH523/product.md`
Tech spec: `specs/GH523/tech.md`
Status: Draft for maintainer review

## 顺序

contract identity → per-example surface inventory → parser exposure decision/checker → Skill compatibility →
release pairing → CI gate。

## Implementation Tasks

- [ ] `SP523-T001` Owner: CLI contract | Dependencies: approved specs | Done when: 单一 `CLI_CONTRACT_VERSION` 以 `cli_contract_version` 字段进入 JSON envelope，breaking/additive 规则写入 CLI contract | Verify: `cargo test cli_contract_version_is_exposed_and_declared` | Covers: B-001, B-010
- [ ] `SP523-T002` Owner: Agent Skill | Dependencies: SP523-T001, SP523-T003, SP523-T004 | Done when: `loom.skill.toml` 声明 contract range，Skill 对 missing/out-of-range CLI 仅允许 read-only diagnosis 并阻止 mutation，且这些 next actions 已由 checker 验证 | Verify: `cargo test --test shipped_registry_skill && cargo test --test agent_contract_surfaces incompatible_cli_blocks_mutation` | Covers: B-001, B-002, B-009, B-013
- [ ] `SP523-T003` Owner: surface/emitter inventory | Dependencies: SP523-T001 | Done when: review-owned inventory 以单 example/行区间覆盖 README、runbook、lifecycle、Skill、release/Homebrew 与所有 active commands；静态 scan 枚举所有 `next_actions` producer 并要求稳定 emitter id/fixture，test-only trace 在 producer 被调用时记录 `{emitter_id, payload}`，相同 command text 不得跨 producer 计覆盖；每个 Panel mutation action/label 使用真实稳定 action id（例如 `skill.trash.add`）绑定实际后端 route 与公开 CLI argv，或以 review-owned rationale 标记无 CLI 等价项；classification 闭集、marker 唯一且空 inventory 失败 | Verify: `cargo test --test agent_contract_surfaces inventory_covers_public_surfaces && cargo test --test agent_contract_surfaces emitter_inventory_is_complete && cargo test --test agent_contract_surfaces emitter_fixture_identity_is_observable && cargo test --test agent_contract_surfaces panel_mutations_are_mapped && cargo test --test agent_contract_surfaces unclassified_command_fails` | Covers: B-003, B-006, B-013
- [ ] `SP523-T004` Owner: parser checker | Dependencies: SP523-T003, approved parser exposure ADR | Done when: fenced/inline executable 示例、完整 emitter fixture matrix 的字符串/对象形式 `next_actions` 与 `cli_equivalent` Panel mappings 使用同一 Clap parser/public visibility allowlist 验证，hidden command 与公开 command 上的 hidden flag/option 都失败，错误定位到 stable id/file/line/argv | Verify: `cargo test --test agent_contract_surfaces executable_examples_parse && cargo test --test agent_contract_surfaces panel_cli_equivalents_parse && cargo test --test agent_contract_surfaces removed_commands_fail && cargo test --test agent_contract_surfaces hidden_commands_fail && cargo test --test agent_contract_surfaces hidden_flags_fail && cargo test --test agent_contract_surfaces parse_failure_is_terminal` | Covers: B-004, B-005, B-006, B-008
- [ ] `SP523-T005` Owner: checker safety | Dependencies: SP523-T004 | Done when: checker 使用临时 HOME/root，重复运行确定且不改 source/index/refs/home | Verify: `cargo test --test agent_contract_surfaces checker_is_read_only_and_repeatable && cargo test --test agent_contract_surfaces checker_never_rewrites_sources` | Covers: B-007, B-011
- [ ] `SP523-T006` Owner: release | Dependencies: SP523-T001..T005 | Done when: release archive 与 Homebrew share 包含匹配的 binary/Skill/contract manifest；canonical digest 绑定 binary、Skill tree 与 inventory；唯一 staging 完整校验后在 lock 内原子发布，并发/取消/故障只产生完整旧版或新版；缺 binary、Skill metadata、surface/emitter inventory、manifest 任一项均失败 | Verify: local release archive/Homebrew smoke + `cargo test packaged_contract_mismatch_fails && cargo test packaged_contract_digests_match && cargo test homebrew_share_contract_matches && cargo test release_manifest_is_atomic_and_untracked && cargo test release_manifest_concurrent_publish && cargo test release_manifest_cancel_before_publish && cargo test packaged_contract_missing_inputs_fail_closed` | Covers: B-001, B-002, B-009, B-010, B-012, B-013

## Verification Tasks

- [ ] `SP523-T007` Owner: CI | Dependencies: SP523-T006 | Done when: contract checks 接入 `make check` 和 release workflow；PR job fetch 并通过 `LOOM_CONTRACT_DIFF_BASE` 传入明确 base SHA；故意加入 `skill save`、hidden/错误 flag、未登记或不可观测 emitter、未映射/route 漂移的 Panel mutation、缺 diff base、空 inventory、缺 binary/Skill/manifest、旧新版本错配均阻断 | Verify: `make check` and release workflow fixture job | Covers: B-003, B-004, B-005, B-008, B-009, B-010, B-013
- [ ] `SP523-T008` Owner: docs/review | Dependencies: SP523-T007 | Done when: ADR/CLI contract/CHANGELOG 与 append-only contract history 说明 version/range policy，range-policy gate 对显式 base 比较 before/after 并证明新增记录、保留旧记录与 migration note 同 diff；所有高上下文文件修改均为显式 patch | Verify: `git diff --check && cargo fmt --all -- --check && cargo test contract_range_requires_migration_note_with_explicit_base && cargo test contract_range_missing_diff_base_fails` | Covers: B-010, B-011

## Handoff

- Product invariant set: `B-001..B-013`。
- Task coverage union: `B-001..B-013`。
- Human gate: 维护者批准 parser 暴露方式、contract version 与 inventory ownership 后才能实现。
