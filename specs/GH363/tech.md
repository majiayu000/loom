# GH363 Tech Spec: Single-Skill Lifecycle Workbench

Issue: https://github.com/majiayu000/loom/issues/363
Product spec: `specs/GH363/product.md`
Status: Draft epic coordination spec

## Design Summary

Coordinate the single-skill lifecycle around one shared read model and a set of
explicit mutating workflows. Child issues may land incrementally, but they must
compose into the same lifecycle status card and must not create parallel
status, activation, eval, safety, dependency, or Panel semantics.

## Shared Status Model

The shared status card should expose:

```json
{
  "skill": "fixflow",
  "source": {},
  "provenance": {},
  "spec": {},
  "runtime": {},
  "quality": {},
  "eval": {},
  "safety": {},
  "dependencies": {},
  "release": {}
}
```

Rules:

1. every section must distinguish `pass`, `warning`, `fail`, `blocked`, and
   `missing`;
2. absent data is not equivalent to success;
3. read commands must not mutate registry, projection, or target state;
4. Panel and docs consume this model rather than inventing independent status.

## Dependency Order

Recommended implementation order:

1. #364 scaffolding and manifest;
2. #365 lint expansion;
3. #366 inspect/status model;
4. #373 adapter discovery and visibility metadata;
5. #371 dependency and MCP readiness;
6. #370 safety/trust/quarantine;
7. #367 activation/deactivation/list;
8. #368 Codex visibility doctor;
9. #369 real eval harness;
10. #372 improve/regression workflow;
11. #374 docs and migration guide;
12. #375 Panel detail page.

## Cross-Issue Contracts

### Source And Provenance

Scaffolding/import must record source identity and local manifest data. Later
features should read this source state instead of deriving their own identity.

### Lint And Spec

Lint must parse portable Agent Skills metadata and expose agent-specific
compatibility. Later activation, packaging, and docs must use lint outputs
rather than reimplementing spec checks.

### Runtime

Activation state, projection state, installed state, and visible-to-agent state
are separate. Commands must not report one as proof of another.

### Eval

Eval results must include with-skill and no-skill baselines when claiming skill
value. Offline fixture checks can be a fast path but cannot replace real agent
evidence for product claims.

### Safety And Dependencies

Safety, trust, quarantine, script risk, dependency readiness, and MCP readiness
must be visible before activation and must block or warn according to policy.

### Release And Rollback

Release and rollback must be auditable and tied to source/provenance state.
Rollback must not silently leave stale activation or projection state.

## Affected Areas

| Area | Child issues |
|---|---|
| skill source/scaffold | #364 |
| lint/spec status | #365 |
| inspect/read model | #366 |
| activation/runtime | #367, #368, #373 |
| eval/quality | #369, #372 |
| safety/dependencies | #370, #371 |
| docs/Panel | #374, #375 |

## Test Strategy

Each child issue should add focused tests for its own slice and at least one
integration assertion that its result appears in the shared status model. The
epic is complete only when an end-to-end test can cover:

```text
new/import -> lint -> inspect/provenance -> safety/trust scan -> dependency/MCP readiness -> activate -> doctor -> eval -> improve -> release -> rollback/deactivate
```

Suggested final commands:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
(
  cd panel
  bun run typecheck
  bun run test
)
```

Panel commands are required when #375 or Panel API files are touched.

## Rollback

The epic coordination spec is documentation-only. Implementation rollback is
owned by each child issue. The shared rule is that rollback must preserve source
truth, registry consistency, audit history, and clear active/projection state.

## Risks

1. Child issues duplicate status logic. Mitigation: shared inspect/read model.
2. Projection is mistaken for active/visible state. Mitigation: runtime sections
   keep those states separate.
3. Eval or safety claims are made from missing data. Mitigation: missing is
   first-class and never pass.
4. Panel diverges from CLI. Mitigation: Panel consumes backend read models.
