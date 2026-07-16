# GH536 Tasks: Adapter Differentiation Or Fidelity Tiers

Issue: https://github.com/majiayu000/loom/issues/536
Product spec: `specs/GH536/product.md`
Tech spec: `specs/GH536/tech.md`
Status: Maintainer decisions approved; ready for implementation

## Order

Tier field design -> built-in assignments + output -> prioritized `gemini-cli` upgrade -> final docs/contract.

## Tasks

- [x] `SP536-T001` Owner: maintainer | Dependencies: none | Decision: `verified`/`generic`; output always present; external adapters default `generic`; upgrade order `gemini-cli` → `windsurf`/`cline` → `cursor` | Verify: decision recorded on 2026-07-16
- [ ] `SP536-T002` Owner: adapters | Dependencies: `SP536-T001` | Done when: fidelity field added to the internal model and built-in table; codex/claude are `verified`, and every adapter not yet evidence-upgraded is `generic` (T005 may subsequently flip `gemini-cli`) | Verify: `cargo test agent_adapters`
- [ ] `SP536-T003` Owner: cli | Dependencies: `SP536-T002` | Done when: existing `workspace status --json` adapter rows emit the field and workspace doctor/skill diagnose messaging references generic tier; no new agent command is added | Verify: workspace status + workspace doctor + skill diagnose tests
- [ ] `SP536-T004` Owner: docs | Dependencies: `SP536-T002`, `SP536-T005` | Done when: SUPPORTED_AGENTS / AGENT_ADAPTERS / CLI contract show final per-agent tiers, including `gemini-cli` after its evidence-backed flip | Verify: contract test
- [ ] `SP536-T005` Owner: adapters | Dependencies: `SP536-T003` | Done when: `gemini-cli` is upgraded to verified with official-evidence-backed discovery/visibility/reload + tests | Verify: targeted adapter tests
- [ ] `SP536-T006` Owner: verification | Dependencies: all prior | Done when: full checks pass | Verify: `cargo check && cargo test`
