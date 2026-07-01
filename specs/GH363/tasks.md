# GH363 Tasks: Single-Skill Lifecycle Workbench

Issue: https://github.com/majiayu000/loom/issues/363
Product spec: `specs/GH363/product.md`
Tech spec: `specs/GH363/tech.md`
Status: Draft epic coordination spec

## Scope For This Packet

This packet coordinates child issues. It does not implement the lifecycle by
itself.

## Tasks

- [ ] `SP363-T1` Owner: #364 | Done when: `skill new` and Loom-local manifest create/import one skill with durable source identity | Verify: child issue checks
- [ ] `SP363-T2` Owner: #365 | Done when: lint reports portable, agent-specific, quality, resources, and progressive disclosure status | Verify: child issue checks
- [ ] `SP363-T3` Owner: #366 | Done when: `skill inspect` exposes shared source/spec/runtime/quality/safety/dependency/release status sections | Verify: child issue checks
- [ ] `SP363-T4` Owner: #367/#368/#373 | Done when: activation, visibility, adapter discovery, and doctor states are separate and explainable | Verify: child issue checks
- [ ] `SP363-T5` Owner: #369/#372 | Done when: eval and improve/regression workflows provide baseline-backed evidence before release claims | Verify: child issue checks
- [ ] `SP363-T6` Owner: #370/#371 | Done when: safety, trust, quarantine, dependency, and MCP readiness gates are visible before activation | Verify: child issue checks
- [ ] `SP363-T7` Owner: #374/#375 | Done when: docs and Panel render the same lifecycle state from backend/CLI read models | Verify: child issue checks
- [ ] `SP363-T8` Owner: integration | Done when: end-to-end lifecycle test covers new/import through rollback/deactivate without hidden registry inspection | Verify: `cargo test`

### SP363-T1: Source And Manifest Foundation

Owner: #364

Done when:

- one skill can be created or imported;
- source identity and manifest metadata are durable;
- provenance/drift can be consumed by inspect.

Verify:

```bash
cargo test --test skill_new_cli
```

### SP363-T2: Lint And Spec Status

Owner: #365

Done when:

- portable Agent Skills frontmatter parses;
- agent-specific compatibility is reported;
- quality and progressive disclosure findings are structured.

Verify:

```bash
cargo test --test skill_lint
```

### SP363-T3: Shared Inspect Model

Owner: #366

Done when:

- `skill inspect` exposes source, spec, runtime, quality, safety,
  dependencies, and release sections;
- missing data is represented as missing or blocked, not pass.

Verify:

```bash
cargo test --test skill_inspect
```

### SP363-T4: Activation And Visibility

Owner: #367, #368, #373

Done when:

- activation/deactivation/list are first-class operations;
- projection state is not reported as proof of active or visible state;
- Codex visibility doctor explains missing path, disabled config, stale
  projection, broken link, and restart-required cases;
- adapter metadata owns discovery roots and reload notes.

Verify:

```bash
cargo test --test skill_activate
cargo test --test skill_doctor
```

### SP363-T5: Eval And Improvement

Owner: #369, #372

Done when:

- eval can compare with-skill and no-skill baselines;
- improve/regression workflows require preflight and post-change checks;
- release claims are backed by eval evidence.

Verify:

```bash
cargo test --test skill_eval
```

### SP363-T6: Safety And Readiness

Owner: #370, #371

Done when:

- safety scan, trust, quarantine, script risk, dependency readiness, and MCP
  readiness are visible before activation;
- missing or failed gates do not silently degrade.

Verify:

```bash
cargo test --test skill_policy
```

### SP363-T7: Docs And Panel

Owner: #374, #375

Done when:

- docs explain the lifecycle and migration from projection-only workflows;
- Panel consumes backend read models for inspect, visibility, eval, and safety;
- Panel does not invent independent lifecycle state.

Verify:

```bash
(
  cd panel
  bun run typecheck
  bun run test
)
```

### SP363-T8: End-To-End Lifecycle

Owner: integration

Done when:

- an integration test covers create/import, lint, inspect, safety/trust scan,
  dependency/MCP readiness, activate, doctor, eval, improve/regression, release,
  rollback, and deactivate;
- the test does not require manual hidden registry inspection.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

- Use `Refs #363` for this coordination packet.
- Do not use `Fixes #363` until child issue implementations satisfy the epic
  acceptance criteria and end-to-end verification passes.
- Advanced ecosystem work in #376 should consume this lifecycle status model
  instead of creating parallel state.
