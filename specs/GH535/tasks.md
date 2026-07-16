# GH535 Tasks: Skill Command Surface Convergence

Issue: https://github.com/majiayu000/loom/issues/535
Product spec: `specs/GH535/product.md`
Tech spec: `specs/GH535/tech.md`
Status: Pending maintainer approval

## Order

Taxonomy decision -> ownership map -> enum/dispatch move -> contract/docs/tests -> verification.

## Tasks

- [ ] `SP535-T001` Owner: maintainer | Dependencies: none | Done when: taxonomy chosen (top-level `author` vs `skill author`) and boundary commands (`compile`, `eval`) assigned | Verify: decision recorded here
- [ ] `SP535-T002` Owner: cli | Dependencies: `SP535-T001` | Done when: every current skill subcommand mapped to exactly one group in a table in this file | Verify: table complete, 42/42 assigned
- [ ] `SP535-T003` Owner: cli | Dependencies: `SP535-T002` | Done when: enums and dispatch moved, envelope `cmd` values updated, no aliases kept | Verify: `cargo check`
- [ ] `SP535-T004` Owner: contract | Dependencies: `SP535-T003` | Done when: `cli_surface` budgets updated, old paths blacklisted, docs/README synced | Verify: `cargo test --test cli_surface`
- [ ] `SP535-T005` Owner: verification | Dependencies: all prior | Done when: full suite passes and repo grep shows no live old-path references | Verify: `cargo test`
