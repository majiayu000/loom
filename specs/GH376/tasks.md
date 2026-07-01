# GH376 Tasks: Advanced Skill Ecosystem Umbrella

Issue: https://github.com/majiayu000/loom/issues/376
Product spec: `specs/GH376/product.md`
Tech spec: `specs/GH376/tech.md`
Status: Blocked design umbrella

## Scope For First PR

Add only the umbrella SpecRail packet:

```text
phase gates + child issue map + shared technical principles + no runtime implementation
```

Do not implement:

```text
skillset activation, recommendations, workflows, marketplace install, RBAC, provisioning, authoring, compile, telemetry, MCP apply
```

## Tasks

- [ ] `SP376-T001` Owner: planning | Done when: GH376 umbrella spec defines the advanced ecosystem product boundary, blockers, phases, and non-goals | Verify: `git diff --check`
- [ ] `SP376-T002` Owner: planning | Done when: child issues #377-#386 have a shared requirement template for blockers, read models, plan/apply, safety, audit, and tests | Verify: `python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir specs/GH376`
- [ ] `SP376-T003` Owner: planning | Done when: the umbrella spec states that implementation remains blocked by #363 primitives unless a child issue is explicitly design-only or read-only | Verify: `rg -n "blocked|single-skill|dry-run|plan" specs/GH376`
- [ ] `SP376-T004` Owner: regression | Done when: repository checks pass from this session | Verify: `cargo check --workspace --all-targets --all-features && cargo test`

### SP376-T1: Capture Umbrella Product Boundary

Owner: planning

Files:

- `specs/GH376/product.md`

Done when:

- Product thesis is captured.
- #377 through #386 are mapped to product areas.
- Phase ordering is explicit.
- Non-goals prohibit weakened safety gates, implicit network dependence,
  trusted-by-default marketplace install, and silent config mutation.

Verify:

```bash
git diff --check
```

### SP376-T2: Capture Shared Technical Gates

Owner: planning
Depends on: SP376-T1

Files:

- `specs/GH376/tech.md`

Done when:

- Advanced features are required to consume single-skill read models.
- Plan-before-apply is mandatory for mutating workflows.
- Policy, audit, command envelope, and Panel safety reuse are stated.
- Child spec requirements are listed.

Verify:

```bash
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir specs/GH376
```

### SP376-T3: Keep Runtime Work Blocked

Owner: planning
Depends on: SP376-T1, SP376-T2

Done when:

- This PR does not modify runtime code.
- Handoff notes forbid closing #376 until child issues are handled or replaced.
- Tasks make clear that child implementation PRs need their own focused specs
  and tests.

Verify:

```bash
git diff --check
```

### SP376-T4: Verify Umbrella Packet

Owner: testing
Depends on: SP376-T1, SP376-T2, SP376-T3

Done when:

- SpecRail workflow check passes.
- Rust check and full test suites pass from this session.

Verify:

```bash
git diff --check
python3 /Users/apple/Desktop/code/AI/tool/specrail/checks/check_workflow.py --repo /Users/apple/Desktop/code/AI/tool/specrail --spec-dir specs/GH376
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #376`. Do not use `Fixes #376` for this umbrella spec packet.
