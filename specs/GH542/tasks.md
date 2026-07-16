# GH542 Tasks: skill stats lifecycle governance report

Issue: https://github.com/majiayu000/loom/issues/542
Product spec: `specs/GH542/product.md`
Tech spec: `specs/GH542/tech.md`
Status: Pending maintainer approval
Depends on: GH541

## Order

Mount-point decision -> aggregator + classifier -> CLI/envelope -> contract docs/tests.

## Tasks

- [ ] `SP542-T001` Owner: maintainer | Dependencies: none | Done when: 命令挂载点（`skill stats` vs `telemetry stats`，与 GH535 归组对齐）与错误率最小样本门槛拍板 | Verify: decision recorded here
- [ ] `SP542-T002` Owner: skill | Dependencies: `SP542-T001`, GH541 landed | Done when: `skill_stats.rs` 聚合器 + 互斥完备分类器（active/zombie/unbound-unused/unbound-but-used + single-runtime 标记 + orphan 列表）落地 | Verify: table-driven classifier tests
- [ ] `SP542-T003` Owner: cli | Dependencies: `SP542-T002` | Done when: args/过滤/排序/envelope（含 telemetry_empty、window_events）可用 | Verify: integration tests over fixture registry + events
- [ ] `SP542-T004` Owner: docs | Dependencies: `SP542-T003` | Done when: `docs/LOOM_CLI_CONTRACT.md` 增补命令面与 envelope 字段 | Verify: `cargo test --test cli_surface`
- [ ] `SP542-T005` Owner: verification | Dependencies: all prior | Done when: full checks pass | Verify: `cargo check && cargo test`
