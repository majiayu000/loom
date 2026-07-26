# GH600 Tasks - 安全识别当前 Codex Skill 入口读取

Issue: https://github.com/majiayu000/loom/issues/600
Product spec: `specs/GH600/product.md`
Tech spec: `specs/GH600/tech.md`
Status: implx auto implementation and local verification complete; independent review and exact-head CI pending

## 顺序

规范/负例锁定 → typed tool schema → segment/token parser → trusted path →
Codex Context 集成 → E2E/privacy → contract/全量验证。

## Tasks

- [x] `SP600-T1` Owner: spec | Dependencies: none | Done when: product 以稳定 `B-001..B-008` 固化 command association、trusted-root、schema rejection、privacy/compatibility 边界；tech manifest issue=600、complete=true 且 paths 穷尽；tasks 覆盖所有 invariants | Verify: product ID set 与 task Covers union 均为 `B-001..B-008` | Covers: B-001, B-002, B-003, B-004, B-005, B-006, B-007, B-008
- [x] `SP600-T2` Owner: parser | Dependencies: SP600-T1 | Done when: `exec_command`/`exec` recognized schema 使用 typed result，所有 malformed JSON/arguments/missing-or-invalid `cmd` 返回不含 raw value 的稳定 reason；unknown tools 与合法 non-read 保持 ignored | Verify: `cargo test --locked commands::telemetry::ingest::codex::tests::recognized_tool_schema_drift_is_rejected` | Covers: B-002, B-004, B-008
- [x] `SP600-T3` Owner: parser | Dependencies: SP600-T2 | Done when: provenance-aware std-only tokenizer 正确区分 single/double/escaped home，逐 verb 闭集 grammar 只返回真实 file operands，并对 sed program、option values、`--`、help/version、control operators 与复杂 substitution fail closed | Verify: `cargo test --locked commands::telemetry::ingest::codex::tool_read::shell` | Covers: B-001, B-002, B-008
- [x] `SP600-T4` Owner: parser | Dependencies: SP600-T3 | Done when: actual-home roots 与 plugin cache 使用 normalized component-aware `Path` 校验，拒绝 prefix spoof、`..`、home 外及 nested `.codex` | Verify: `cargo test --locked commands::telemetry::ingest::codex::tool_read::tests::trusted_paths_are_component_aware` | Covers: B-003, B-008
- [x] `SP600-T5` Owner: Codex ingest | Dependencies: SP600-T2, SP600-T3, SP600-T4 | Done when: typed rejection 接入 `ParseOutcome`，合法 tool read 继续使用 stable function-call identity 与 per-turn dedupe，拒绝不污染 retry state；legacy injection tests 不变 | Verify: `cargo test --locked commands::telemetry::ingest::codex` | Covers: B-004, B-005, B-007
- [x] `SP600-T6` Owner: integration tests | Dependencies: SP600-T5 | Done when: tracked fixture/E2E 同时覆盖 exec/exec_command、8 verb 正例、quote/escape/option/program 负例、missing-home Ignored、stable rejected reasons、重复 ingest，并证明 raw command/path/prompt 不进入 event/cursor/envelope | Verify: `cargo test --locked --test telemetry_ingest codex_tool_reads_are_precise_rejected_and_private && cargo test --locked --test telemetry_ingest parser_fixtures_and_repeated_ingest_are_deterministic` | Covers: B-001, B-002, B-003, B-004, B-005, B-006, B-008
- [x] `SP600-T7` Owner: docs/verification | Dependencies: SP600-T6 | Done when: CLI contract 同步且 command-surface line metadata 保持有效；fmt/check/focused/E2E 与 diff audit全部通过 | Verify: `cargo fmt --all -- --check && cargo check --locked && cargo test --locked commands::telemetry::ingest::codex && cargo test --locked --test telemetry_ingest && git diff --check` | Covers: B-001, B-002, B-003, B-004, B-005, B-006, B-007, B-008

## Handoff

- Product invariant set: `B-001..B-008`.
- Task coverage union: `B-001..B-008`.
- `implx auto` 已授权同一 mixed implementation PR 内 drafting + implementation；
  独立 review、CI、PR gate 与 merge 仍由 coordinator 负责。
