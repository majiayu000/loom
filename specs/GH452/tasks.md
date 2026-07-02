# GH452 Tasks: CLI Concept Convergence And Command-Surface Budget

Issue: https://github.com/majiayu000/loom/issues/452
Product spec: `specs/GH452/product.md`
Tech spec: `specs/GH452/tech.md`
Status: Draft for review

## Order

#457 (additive errors) -> #456 (use user scope) -> #455 (read surfaces) ->
#454 (lifecycle verbs, breaking, last).

## Tasks

- [ ] `SP452-T001` Owner: errors (#457) | Done when: `CommandFailure` and
  `ErrorBody` carry optional `next_actions`, empty-omitted, with one helper
  module mapping error codes to suggestions | Verify: `cargo test envelope`
- [ ] `SP452-T002` Owner: errors (#457) | Done when: BINDING_NOT_FOUND,
  TARGET_NOT_FOUND, SKILL_NOT_FOUND, STATE_NOT_INITIALIZED,
  TARGET_NOT_MANAGED sites emit runnable next_actions and human rendering
  prints hints | Verify: `cargo test --test cli_surface && cargo test not_found`
- [ ] `SP452-T003` Owner: use-flow (#456) | Done when: `UseScope::User`
  exists, resolution routes through adapter discovery roots for all agents,
  and `--target-root` means the exact directory | Verify:
  `cargo test --test use_flow_cli`
- [ ] `SP452-T004` Owner: use-flow (#456) | Done when: `--adopt` registers or
  upgrades the target with an audit entry, and writing into an unadopted
  observed directory fails with TARGET_NOT_MANAGED + next_actions | Verify:
  `cargo test --test use_flow_cli && ./scripts/e2e-agent-flow.sh`
- [ ] `SP452-T005` Owner: read-surfaces (#455) | Done when: `search` absorbs
  resolve (`--for-task`) and recommend (`--explain`), `inspect --brief`
  absorbs show, and the deleted commands are gone from CLI, docs, and panel
  routes | Verify: `cargo test --test cli_surface --test skill_inventory_cli`
- [ ] `SP452-T006` Owner: read-surfaces (#455) | Done when: top-level
  `doctor` is removed, `policy` composes the scan module without duplicate
  assessment code, and `tests/cli_surface.rs` asserts the recorded leaf count
  | Verify: `cargo test --test cli_surface`
- [ ] `SP452-T007` Owner: lifecycle (#454) | Done when: `skill commit` lands
  with direction auto-detect (drift-only, source-only, both -> typed
  ambiguity, neither -> noop) | Verify: `cargo test --test skill_commit`
- [ ] `SP452-T008` Owner: lifecycle (#454) | Done when: `release --anchor`
  absorbs snapshot, `diagnose --check drift` preserves verify exit codes, and
  `capture`/`save`/`snapshot`/`verify` are deleted everywhere including the
  panel mutation route table | Verify:
  `cargo test && cargo check --workspace --all-targets --all-features`
- [ ] `SP452-T009` Owner: docs | Done when: README lifecycle table loses the
  mnemonic, `docs/LOOM_CLI_CONTRACT.md` and
  `docs/LOOM_COMPLETE_GUIDE_ZH.md` reflect the final surface, and the
  surface-budget rule is recorded in `docs/LOOM_ARCHITECTURE_DECISIONS.md` |
  Verify: `git diff --check && make fmt-check`
