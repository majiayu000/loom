# GH453 Tasks: Core Layering And State Convergence

Issue: https://github.com/majiayu000/loom/issues/453
Product spec: `specs/GH453/product.md`
Tech spec: `specs/GH453/tech.md`
Status: Draft for review

## Order

#460 (vocab enums) and #461 (adapter metadata) in parallel -> #458 (service
layer, per domain) -> #459 (ops-log unification, after its migration plan is
reviewed).

## Tasks

- [ ] `SP453-T001` Owner: vocab (#460) | Done when: `src/core/vocab.rs`
  defines AgentKind/Ownership/ProjectionMethod/Health/MatcherKind with serde
  spellings matching current on-disk values, and existing fixture files load
  unchanged | Verify: `cargo test --test workspace_init && cargo test state_model`
- [ ] `SP453-T002` Owner: vocab (#460) | Done when: `state_model` stores the
  enums (agent via validated newtype per ADR 2.1 reader/writer asymmetry),
  `src/cli/agent_kind.rs` is deleted, and unknown ownership/method/health
  values fail load with a typed SCHEMA error | Verify:
  `cargo test && cargo check --workspace --all-targets --all-features`
- [ ] `SP453-T003` Owner: adapters (#461) | Done when: claude has real
  discovery_roots/visibility/reload metadata with tests parallel to
  `tests/codex_visibility.rs`, and capabilities derive from metadata instead
  of one shared literal | Verify: `cargo test --test workspace_init adapter`
- [ ] `SP453-T004` Owner: adapters (#461) | Done when: `health_checks` is
  either stored, emitted in `adapters_json()`, and consumed by
  `workspace doctor`, or removed from both record types and
  `docs/schemas/agent-adapter-v2.schema.json`; no parsed-then-dropped fields
  remain | Verify: `cargo test adapter && git diff --check`
- [ ] `SP453-T005` Owner: layering (#458) | Done when: `CommandMeta` replaces
  `command_records_audit` and `command_requires_durable_audit` with one
  exhaustive metadata source, leaving a single match over the Command tree |
  Verify: `cargo test --test cli_surface && cargo test audit`
- [ ] `SP453-T006` Owner: layering (#458) | Done when: projection and
  lifecycle domains live in `src/core/` with typed inputs/outputs, their
  `cmd_*` handlers are parse->service->envelope only, and behavior is
  unchanged | Verify: `cargo test && ./scripts/e2e-agent-flow.sh`
- [ ] `SP453-T007` Owner: layering (#458) | Done when: panel projection and
  lifecycle mutation routes call core services directly while
  `ensure_mutation_authorized`, locks, audit, and envelope parity tests stay
  green | Verify: `cargo test panel`
- [ ] `SP453-T008` Owner: ops-log (#459) | Done when: a reviewed migration
  plan documents journal-backed replacements for pending-queue semantics
  including the `loom-history` interplay | Verify: doc review on the PR;
  `git diff --check`
- [ ] `SP453-T009` Owner: ops-log (#459) | Done when: sync and ops command
  families run on the unified journal with equivalence tests against
  recorded pending-queue fixtures, and `pending_ops.*` writers and files are
  deleted | Verify:
  `cargo test --test sync --test ops && ./scripts/e2e-agent-flow.sh`
- [ ] `SP453-T010` Owner: docs | Done when:
  `docs/LOOM_ARCHITECTURE_DECISIONS.md` sections 1 and 2.1 record the
  closures and `docs/LOOM_STATE_MIGRATION_NOTES.md` covers the ops-log
  migration | Verify: `git diff --check && make fmt-check`
