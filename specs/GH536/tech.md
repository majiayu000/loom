# GH536 Tech Spec - Adapter differentiation or fidelity tiers

Issue: https://github.com/majiayu000/loom/issues/536
Product spec: `specs/GH536/product.md`
Route: `write_spec`
Human gate: maintainer decisions approved on 2026-07-16

## 1. Current Behavior

`src/agent_adapters/metadata.rs:88-104`: only codex/claude get dedicated discovery roots; others map defaults to `role: "legacy-default"`. `built_in_reload` (`metadata.rs:141-157`) returns identical strategy for claude/codex differing only in notes. Visibility falls to `default_visibility()` for 8/10 agents. No fidelity signal exists in adapter output.

## 2. Proposed Design

1. Add a required internal `fidelity` field to adapter metadata, enum `verified` | `generic`; existing `workspace status --json` rows under `data.agent_adapters.adapters` always emit it.
2. Built-in table marks codex/claude `verified`, others `generic`.
3. `loom workspace status --json` includes the field through `AgentAdapterRegistry::adapters_json`; workspace doctor/skill diagnose messaging references it when reporting generic-tier agents. Do not add new `agent list/inspect` commands.
4. Docs: per-agent tier table in `docs/SUPPORTED_AGENTS.md` and `docs/AGENT_ADAPTERS.md`.
5. Follow-up (separate tasks, incremental): research + implement dedicated discovery/visibility/reload in evidence order (`gemini-cli`, then `windsurf`/`cline`, then `cursor`), flipping a tier only with targeted tests.

## 3. Affected Areas

1. `src/agent_adapters.rs` (schema struct)
2. `src/agent_adapters/metadata.rs`
3. `src/commands/workspace_cmds/status.rs` (existing metadata output)
4. `src/commands/workspace_cmds/doctor.rs` (generic-tier diagnostics)
5. `src/commands/skill_diagnose.rs` if it consumes adapter metadata
6. `docs/SUPPORTED_AGENTS.md`, `docs/AGENT_ADAPTERS.md`, `docs/LOOM_CLI_CONTRACT.md`
7. adapter tests

## 4. Output Contract

`workspace status --json` rows under `data.agent_adapters.adapters` always include `fidelity: "verified"|"generic"`. External adapter files do not gain a self-asserted fidelity input; they resolve to `generic` until schema-validated evidence rules are defined.

## 5. Verification Plan

1. `cargo test agent_adapters`
2. Contract test update for new field in `docs/LOOM_CLI_CONTRACT.md`
3. `cargo check && cargo test`

## 6. Rollback Plan

The field is required on CLI metadata output but absent from external adapter input schemas. Output addition is additive; rollback can drop the emitted field without state migration, while external v1/v2 input remains unchanged.

## 7. Product Mapping

1. Invariant 1-2 map to the enum field and built-in table assignments.
2. Invariant 3 maps to per-agent upgrade tasks each carrying tests.
3. Invariant 4 maps to the docs table updates with contract test coverage.
