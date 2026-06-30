# Advanced Skill Ecosystem TODO List

Date: 2026-06-30
Status: Local planning TODO
Route: implx fallback, design backlog only

## Startup Evidence

- Repository: `majiayu000/loom`
- Remote refresh: `git fetch --prune origin`
- Current branch: `codex/audit-fixes-spec-plan`
- Current base: `origin/main` at `73f3a043e72a06923f0115e47e9ba040c5f0387f`
- Local working tree: clean when this TODO was created
- Open PRs: none
- Open issue queue: #363 through #386
- Spec packets: no local `specs/GH376` through `specs/GH386` packets exist yet

This document is not an implementation spec. It is the local TODO bridge from
the already-open advanced ecosystem issues to future SpecRail packets and
implementation slices.

## Boundary

The single-skill lifecycle epic is #363. The advanced ecosystem epic is #376.

Do not implement advanced ecosystem runtime behavior until the relevant
single-skill primitives are stable:

- #365 portable, agent-specific, and quality lint
- #366 single-skill status model and `skill inspect`
- #367 activate, deactivate, and active list semantics
- #368 Codex visibility doctor and active-view reconcile
- #369 real eval harness
- #370 safety, trust, and quarantine
- #371 dependency and MCP readiness
- #373 adapter discovery, visibility, and reload semantics

Advanced work must consume these primitives instead of duplicating status,
activation, policy, or readiness logic.

## Issue Map

| Issue | Scope | Status |
|---|---|---|
| #376 | Advanced ecosystem epic | Open |
| #377 | Skillsets and bundles | Open |
| #378 | Capability graph, semantic retrieval, recommendations | Open |
| #379 | DAG workflow orchestration | Open |
| #380 | Marketplace, catalog, provider integrations | Open |
| #381 | Team approvals, org policy, RBAC | Open |
| #382 | Cloud, devcontainer, remote provisioning | Open |
| #383 | LLM-assisted authoring and refactoring | Open |
| #384 | Compiled runtime interface | Open |
| #385 | Telemetry and analytics dashboard | Open |
| #386 | MCP server install and config automation | Open |

## Implementation Order

### 0. SpecRail Setup

- [ ] Create `specs/GH376/product.md`, `tech.md`, and `tasks.md` only if the
  epic needs a repo-local umbrella packet.
- [ ] Prefer one spec packet per implementation issue: `specs/GH377` through
  `specs/GH386`.
- [ ] Keep each packet explicit about blocked dependencies from #363.
- [ ] Run `python3 checks/check_workflow.py --repo . --spec-dir specs/GH<issue>`
  after each packet is added if SpecRail checks are present in this repository.

### 1. Data Foundation

- [ ] #377: Define the `skillset` read/write model.
- [ ] #381: Define the org policy, roles, and approval state model.
- [ ] #385: Define local telemetry event schema and privacy rules.
- [ ] #378: Define local capability index records after #377 has a member model.

Done when:

- Skillsets, policy, approval, telemetry, and capability records have stable
  schemas.
- Schemas are deterministic and compatible with existing registry state.
- No mutating command bypasses existing audit and policy patterns.

### 2. Safe Operations

- [ ] #377: Add `skillset create/add/remove/show/lint`.
- [ ] #377: Add `skillset activate --dry-run` before real activation.
- [ ] #380: Add provider config, catalog preview, and install dry-run.
- [ ] #381: Add policy check and approval request lifecycle.
- [ ] #386: Add MCP requirement listing and dry-run provisioning plan.
- [ ] #382: Add provisioning dry-run plans for devcontainer and shell targets.

Done when:

- Every workflow has dry-run or plan output before apply.
- Public or third-party content defaults to `third-party-unreviewed`.
- Quarantined or blocked skills cannot be activated, installed into active
  views, provisioned, or included in skillsets.
- Secrets are detected as requirements but are never printed or copied.

### 3. Intelligence Layer

- [ ] #378: Add deterministic lexical-only `loom index build`.
- [ ] #378: Add explainable `skill recommend` without mutation.
- [ ] #379: Add workflow DAG model and `workflow plan`.
- [ ] #383: Add LLM-assisted patch artifact generation with mock provider tests.
- [ ] #384: Add `skill compile --dry-run` and artifact verification model.

Done when:

- Recommendations are explainable and policy-aware.
- No recommendation or workflow plan silently activates a skill.
- DAG plans reject cycles, missing skills, blocked skills, and unsafe nodes.
- LLM-assisted output is a reviewable patch artifact, not an implicit edit.
- Compiled artifacts remain derived from portable source skills and are
  invalidated by source digest changes.

### 4. Apply Paths And UI

- [ ] #377: Implement safe skillset activation with rollback or precise recovery.
- [ ] #379: Add `workflow apply` only after plan/preflight semantics are stable.
- [ ] #380: Add pinned provider install with provenance and lockfile records.
- [ ] #381: Enforce policy from install, activate, release, trust, quarantine,
  provider, and sync commands.
- [ ] #382: Add atomic provisioning apply for devcontainer and shell outputs.
- [ ] #386: Add idempotent MCP config apply with approval tokens.
- [ ] #385: Add local telemetry reports and Panel dashboard.

Done when:

- Apply commands revalidate their plans and require idempotency keys.
- Config writes are atomic and preserve user-authored config where possible.
- Panel consumes backend read models and does not invent independent mutation
  semantics.

## Per-Issue TODO

### #377 Skillsets And Bundles

- [x] Spec packet: data model, CLI surface, rollback behavior, tests.
- [x] First slice: `skillset create/add/remove/show/lint`.
- [x] Use the current skill inventory read model for each member summary.
- [ ] Use #367 activation path for each member.
- [ ] Define partial activation rollback and recovery commands.
- [ ] Add eval aggregation only after #369 reports exist.

### #378 Capability Graph And Recommendations

- [ ] Spec packet: index schema, ranking signals, explainability contract.
- [ ] Start with deterministic lexical mode and no network embeddings.
- [ ] Filter blocked and quarantined skills.
- [ ] Penalize missing dependencies and missing eval evidence.
- [ ] Return activation next actions only, not mutations.

### #379 DAG Workflow Orchestration

- [ ] Spec packet: DAG schema, node policy, approval tokens, plan/apply.
- [ ] Start with `workflow plan` and `workflow preflight`.
- [ ] Reject cycles, excessive depth, missing skills, and unsafe nodes.
- [ ] Defer autonomous execution until plan/apply gates are proven.

### #380 Marketplace, Catalog, And Providers

- [ ] Spec packet: provider config, locator grammar, preview, install dry-run.
- [ ] Preserve the V1 boundary: providers discover or fetch, Loom owns local
  registry state, policy, provenance, projection, audit, rollback, and eval.
- [ ] Never execute previewed code.
- [ ] Reject unpinned moving refs under strict policy.
- [ ] Default public installs to `third-party-unreviewed`.

### #381 Team Approval, Org Policy, And RBAC

- [ ] Spec packet: role model, policy file, approval request model.
- [ ] Keep v1 Git-backed and local-service-free.
- [ ] Require policy checks inside mutating commands.
- [ ] Make approvals append-only and auditable.
- [ ] Keep local safety gates in force even when org policy allows an action.

### #382 Cloud, Devcontainer, And Remote Provisioning

- [ ] Spec packet: target kinds, plan model, devcontainer output, apply rules.
- [ ] Start with `provision plan --target devcontainer`.
- [ ] Use adapter metadata from #373 for target paths.
- [ ] Include active skills, skillsets, dependencies, and policy gates.
- [ ] Never copy secrets by default.

### #383 LLM-Assisted Authoring And Refactoring

- [ ] Spec packet: patch artifact model, provider abstraction, redaction rules.
- [ ] Start with mock provider tests.
- [ ] Make all generation dry-run by default.
- [ ] Require explicit patch apply with idempotency key.
- [ ] Run lint, scan, and eval preflight before and after patch apply.

### #384 Compiled Runtime Interface

- [ ] Spec packet: compile artifact layout, source digest, verification gates.
- [ ] Start with `skill compile --dry-run`.
- [ ] Preserve `SKILL.md` as source truth.
- [ ] Do not require compiled artifacts for normal activation.
- [ ] Require eval evidence before claiming runtime benefit.

### #385 Telemetry And Analytics

- [ ] Spec packet: event schema, privacy rules, aggregation, export, purge.
- [ ] Make telemetry opt-in unless using already-existing command audit events.
- [ ] Store hashes by default for workspace and session identity.
- [ ] Do not store raw prompts, code, env values, or secrets by default.
- [ ] Add Panel dashboard only after backend reports exist.

### #386 MCP Provisioning

- [ ] Spec packet: requirement model, MCP plan/apply, catalog source policy.
- [ ] Parse requirements from `loom.skill.toml`, `SKILL.md` metadata,
  compatibility text, and agent-specific metadata.
- [ ] Plan config diffs without writing.
- [ ] Require provenance and pinned package versions for MCP server sources.
- [ ] Never print or store secret values.

## Gates Before Any Implementation PR

- [ ] Fetch remote and remap open PRs before starting a slice.
- [ ] Confirm no existing PR already covers the issue.
- [ ] Read the issue body and any `specs/GH<issue>` packet.
- [ ] Use `Refs #<issue>` for partial slices.
- [ ] Use closing keywords only when every acceptance criterion is implemented
  and verified.
- [ ] Run focused tests for touched behavior.
- [ ] Run repository deterministic checks.
- [ ] Check GitHub PR head SHA, CI/check rollup, `mergeStateStatus`, and
  GraphQL review threads before readiness claims.
- [ ] Do not merge without explicit human authorization in the current
  conversation.

## implx Handoff

```yaml
implx_handoff:
  route: implement_queue
  issue_to_pr_map:
    "#376": none
    "#377": none
    "#378": none
    "#379": none
    "#380": none
    "#381": none
    "#382": none
    "#383": none
    "#384": none
    "#385": none
    "#386": none
  approved_specs: none
  threads:
    mode: single_agent
    lanes: []
    fallback_reason: "local TODO planning only; no implementation lanes needed"
  gates:
    route_gate: "blocked on future SpecRail packets or explicit implementation request"
    pr_gate: "not applicable; no PR created"
    review_threads: "not applicable"
    merge_authorization: "not requested"
  closure_audit:
    local_doc: "docs/plan/advanced-skill-ecosystem-todolist.md"
```
