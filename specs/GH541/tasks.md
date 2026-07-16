# GH541 Tasks: Telemetry ingestion from agent session logs

Issue: https://github.com/majiayu000/loom/issues/541
Product spec: `specs/GH541/product.md`
Tech spec: `specs/GH541/tech.md`
Status: Pending maintainer approval

## Order

Anchor decision -> cursor + parsers -> matching + dedup -> CLI/envelope -> contract docs/tests.

## Tasks

- [ ] `SP541-T001` Owner: maintainer | Dependencies: none | Done when: Claude transcript skill-invocation 锚点集合与 Codex 识别锚点用真实样本核定并记录于此；退役 skill 归属策略（Open Question 2）拍板 | Verify: decision recorded here
- [ ] `SP541-T002` Owner: telemetry | Dependencies: `SP541-T001` | Done when: `ingest/cursor.rs` 高水位读写 + `claude.rs`/`codex.rs` 解析器落地，env 可覆盖日志根目录 | Verify: parser unit tests over fixtures
- [ ] `SP541-T003` Owner: telemetry | Dependencies: `SP541-T002` | Done when: read-model 匹配 + deterministic event_id 去重 + emitter 持久化打通 | Verify: idempotency integration test
- [ ] `SP541-T004` Owner: cli | Dependencies: `SP541-T003` | Done when: `loom telemetry ingest` args/envelope（含 dry-run 快照无副作用）可用 | Verify: dry-run snapshot test + `--json` envelope test
- [ ] `SP541-T005` Owner: docs | Dependencies: `SP541-T004` | Done when: `docs/LOOM_CLI_CONTRACT.md` 增补命令面与 envelope 字段 | Verify: `cargo test --test cli_surface`
- [ ] `SP541-T006` Owner: verification | Dependencies: all prior | Done when: full checks pass | Verify: `cargo check && cargo test`
