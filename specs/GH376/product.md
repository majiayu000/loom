# GH376 Product Spec: Advanced Skill Ecosystem Umbrella

Issue: https://github.com/majiayu000/loom/issues/376
Parent foundation: https://github.com/majiayu000/loom/issues/363
Status: Implemented; closeout evidence recorded
Locale: zh-CN

## Goal

Define the product boundary for Loom's advanced ecosystem layer after the
single-skill lifecycle foundation is stable.

The advanced layer should grow Loom from:

```text
single-skill lifecycle workbench
```

into:

```text
skill ecosystem control plane
```

It should answer:

1. Which groups of skills should be installed, activated, evaluated, and
   released together?
2. Which skills should be recommended for a task, workspace, agent, and risk
   policy?
3. Can multiple skills be composed into guarded workflows?
4. Can teams review, approve, and govern skill lifecycle actions?
5. Can local, devcontainer, cloud, Codex, and Claude environments be
   provisioned consistently?
6. Can skills be authored, refactored, compressed, and evaluated with
   guardrails?
7. Can Loom observe long-term skill usage, value, drift, cost, and risk?
8. Can required MCP servers be provisioned safely instead of only detected?

## Blocking Foundation

Production implementation under this epic is blocked until the relevant
single-skill primitives are merged and stable:

- #365 portable, agent-specific, and quality lint.
- #366 single-skill status model and `skill inspect`.
- #367 activate, deactivate, and active list semantics.
- #368 Codex visibility doctor and active-view reconcile.
- #369 real eval harness.
- #370 safety, trust, and quarantine.
- #371 dependency and MCP readiness.
- #373 adapter discovery, visibility, and reload semantics.

Individual advanced issues may proceed in design-only form before those
foundations merge.

## Block Issue Map

| Issue | Product Area |
|---|---|
| #377 | Skillsets and bundles |
| #378 | Capability graph, semantic retrieval, recommendations |
| #379 | DAG workflow orchestration |
| #380 | Marketplace, catalog, provider integrations |
| #381 | Team approval, org policy, RBAC |
| #382 | Cloud, devcontainer, remote provisioning |
| #383 | LLM-assisted authoring and refactoring |
| #384 | Compiled runtime interface |
| #385 | Telemetry and analytics dashboard |
| #386 | MCP server install and configuration automation |

## Phase Ordering

### Phase A: Data Foundation

- Skillset model.
- Capability graph.
- Telemetry schema.
- Policy, approval, and RBAC model.

### Phase B: Safe Operations

- Skillset activation and reconcile.
- Marketplace install with trust gates.
- MCP provisioning dry-run and apply.
- Remote/devcontainer provisioning dry-run and apply.

### Phase C: Intelligence Layer

- Explainable recommendations.
- DAG workflow plans.
- LLM-assisted authoring and refactoring.
- Compiled runtime artifacts.

### Phase D: Org And Product Layer

- Approval and RBAC UI.
- Analytics dashboard.
- Recommendation feedback loop.
- Marketplace and team catalog UX.

## Non-Goals

1. Do not weaken single-skill safety gates.
2. Do not bypass native agent skill systems.
3. Do not require network access by default.
4. Do not make marketplace install trusted by default.
5. Do not silently mutate user config, MCP config, cloud files, or
   devcontainer files without dry-run/plan/apply.
6. Do not duplicate single-skill status, visibility, eval, dependency, or
   safety logic inside advanced features.
7. Do not treat this umbrella issue as approval to implement all child issues in
   one PR.

## Global Acceptance Criteria

1. Every advanced feature consumes the single-skill status model rather than
   duplicating status logic.
2. Every mutating workflow has dry-run or plan output before apply.
3. Safety and trust policy is checked before activation, provisioning,
   marketplace install, and orchestration.
4. Recommendations are explainable and can be disabled.
5. Team/org features have an audited trail.
6. Docs clearly separate local single-skill workflows from advanced ecosystem
   workflows.
7. Child issue specs identify their blockers from #363 and state whether they
   are design-only, read-only, dry-run, or apply-capable.

Closeout evidence:

- Child issues #377 through #386 are implemented or ready for closure.
- #379 and #386 retain plan-first/apply-gated semantics; no advanced workflow
  silently mutates user or agent config.
- The #363 lifecycle foundation has closeout evidence and an E2E test.
