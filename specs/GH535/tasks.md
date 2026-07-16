# GH535 Tasks: Skill Command Surface Convergence

Issue: https://github.com/majiayu000/loom/issues/535
Product spec: `specs/GH535/product.md`
Tech spec: `specs/GH535/tech.md`
Status: Maintainer decisions approved; ready for implementation

## Order

Taxonomy decision -> ownership map -> enum/dispatch move -> contract/docs/tests -> verification.

## Tasks

- [x] `SP535-T001` Owner: maintainer | Dependencies: none | Decision: nested `skill author`; `compile`/`eval`/`improve`/`regression` remain operational; no compatibility aliases | Verify: decision recorded on 2026-07-16
- [x] `SP535-T002` Owner: cli | Dependencies: `SP535-T001` | Done when: every current skill subcommand mapped to exactly one group in the table below | Verify: table assigns 44/44 commands at `bb9b738`
- [ ] `SP535-T003` Owner: cli | Dependencies: `SP535-T002` | Done when: enums and dispatch moved, envelope `cmd` values updated, no aliases kept | Verify: `cargo check`
- [ ] `SP535-T004` Owner: contract | Dependencies: `SP535-T003` | Done when: `cli_surface` budgets updated, old paths blacklisted, docs/README synced | Verify: `cargo test --test cli_surface`
- [ ] `SP535-T005` Owner: verification | Dependencies: all prior | Done when: full suite passes and repo grep shows old paths only in explicit blacklist tests and migration/CHANGELOG/spec history, never in live callers | Verify: `cargo test`

## Ownership Map

| Group | Count | Commands |
| --- | ---: | --- |
| `skill author` | 7 | `draft`, `extract`, `rewrite`, `tune-description`, `generate-evals`, `apply-patch`, `new` |
| `skill` operational | 37 | `list`, `inspect`, `deps`, `compile`, `activate`, `deactivate`, `active`, `search`, `recommend`, `resolve`, `used`, `feedback`, `add`, `install`, `project`, `commit`, `improve`, `regression`, `release`, `rollback`, `diff`, `history`, `trash`, `provenance`, `lint`, `policy`, `scan`, `trust`, `quarantine`, `unquarantine`, `visibility`, `diagnose`, `eval`, `watch`, `monitor-observed`, `import-observed`, `orphan` |
