# GH536 Tasks: Adapter Differentiation Or Fidelity Tiers

Issue: https://github.com/majiayu000/loom/issues/536
Product spec: `specs/GH536/product.md`
Tech spec: `specs/GH536/tech.md`
Status: Maintainer decisions approved; ready for implementation

## Order

Tier field design -> built-in assignments + output -> docs/contract -> prioritized per-agent upgrades.

## Tasks

- [x] `SP536-T001` Owner: maintainer | Dependencies: none | Decision: `verified`/`generic`; output always present; external adapters default `generic`; upgrade order `gemini-cli` → `windsurf`/`cline` → `cursor` | Verify: decision recorded on 2026-07-16
- [ ] `SP536-T002` Owner: adapters | Dependencies: `SP536-T001` | Done when: fidelity field added to schema and built-in table (codex/claude=verified, rest=generic) | Verify: `cargo test agent_adapters`
- [ ] `SP536-T003` Owner: cli | Dependencies: `SP536-T002` | Done when: `agent list/inspect --json` emit the field; diagnose messaging references generic tier | Verify: agent command tests
- [ ] `SP536-T004` Owner: docs | Dependencies: `SP536-T002` | Done when: SUPPORTED_AGENTS / AGENT_ADAPTERS / CLI contract show per-agent tiers | Verify: contract test
- [ ] `SP536-T005` Owner: adapters | Dependencies: `SP536-T003` | Done when: `gemini-cli` is upgraded to verified with official-evidence-backed discovery/visibility/reload + tests | Verify: targeted adapter tests
- [ ] `SP536-T006` Owner: verification | Dependencies: all prior | Done when: full checks pass | Verify: `cargo check && cargo test`
