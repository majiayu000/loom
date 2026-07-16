# GH536 Tech Spec - Adapter differentiation or fidelity tiers

Issue: https://github.com/majiayu000/loom/issues/536
Product spec: `specs/GH536/product.md`
Route: `write_spec`
Human gate: maintainer approval required before implementation

## 1. Current Behavior

`src/agent_adapters/metadata.rs:88-104`: only codex/claude get dedicated discovery roots; others map defaults to `role: "legacy-default"`. `built_in_reload` (`metadata.rs:141-157`) returns identical strategy for claude/codex differing only in notes. Visibility falls to `default_visibility()` for 8/10 agents. No fidelity signal exists in adapter output.

## 2. Proposed Design

1. Add a `fidelity` field to the adapter metadata struct (`agent_adapters.rs` schema + serialization), enum `verified` | `generic`.
2. Built-in table marks codex/claude `verified`, others `generic`.
3. `loom agent list/inspect` envelopes include the field; doctor/diagnose messaging references it when reporting generic-tier agents.
4. Docs: per-agent tier table in `docs/SUPPORTED_AGENTS.md` and `docs/AGENT_ADAPTERS.md`.
5. Follow-up (separate tasks, incremental): research + implement dedicated discovery/visibility/reload for prioritized agents, flipping their tier with tests.

## 3. Affected Areas

1. `src/agent_adapters.rs` (schema struct)
2. `src/agent_adapters/metadata.rs`
3. `src/commands/agent_cmds*` (output)
4. `src/commands/skill_diagnose.rs` if it consumes adapter metadata
5. `docs/SUPPORTED_AGENTS.md`, `docs/AGENT_ADAPTERS.md`, `docs/LOOM_CLI_CONTRACT.md`
6. adapter tests

## 4. Output Contract

`agent list/inspect --json` rows gain `fidelity: "verified"|"generic"`. External adapter files may not claim `verified` unless schema-validated evidence rules are defined (default `generic`).

## 5. Verification Plan

1. `cargo test agent_adapters`
2. Contract test update for new field in `docs/LOOM_CLI_CONTRACT.md`
3. `cargo check && cargo test`

## 6. Rollback Plan

Field removal is a schema change; keep it additive (new optional field) so rollback is dropping the field emission without state migration.

## 7. Product Mapping

1. Invariant 1-2 map to the enum field and built-in table assignments.
2. Invariant 3 maps to per-agent upgrade tasks each carrying tests.
3. Invariant 4 maps to the docs table updates with contract test coverage.
