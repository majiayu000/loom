# GH375 Tasks: Panel Single-Skill Detail Page

Issue: https://github.com/majiayu000/loom/issues/375
Product spec: `specs/GH375/product.md`
Tech spec: `specs/GH375/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement the first read-model-backed Panel detail page slice:

```text
skill inspect read model + /api/v1 skill inspect route + detail page navigation + render tests
```

Do not implement:

```text
Panel-only activation, config repair, quarantine/trust mutation, eval execution, rollback without CLI confirmation
```

## Tasks

- [ ] `SP375-T001` Owner: backend-read-model | Done when: `loom --json skill inspect <skill>` returns source, spec, runtime, eval, safety, and next-action sections from shared backend logic | Verify: `cargo test --test skill_inspect`
- [ ] `SP375-T002` Owner: panel-api | Done when: `GET /api/v1/skills/{skill_name}/inspect` wraps the CLI/shared inspect read model and returns structured errors for missing skills | Verify: `cargo test panel::tests`
- [ ] `SP375-T003` Owner: frontend-routing | Done when: skill list rows and command-palette skill results open a stable single-skill detail route or route-equivalent state | Verify: `cd panel && bun run test -- SkillDetailPage`
- [ ] `SP375-T004` Owner: frontend-detail | Done when: detail page renders source, spec/compatibility, runtime visibility, eval evidence, safety/trust, and next actions from inspect data | Verify: `cd panel && bun run test -- SkillDetailPage`
- [ ] `SP375-T005` Owner: frontend-states | Done when: disabled-by-config, needs-restart, missing projection, warning, error, and empty eval/safety states are visually and semantically distinct | Verify: `cd panel && bun run test -- SkillDetailPage`
- [ ] `SP375-T006` Owner: safety | Done when: dangerous actions are copyable CLI commands or existing confirmed mutation flows, and no Panel mutation bypasses CLI safety checks | Verify: `cargo test panel::tests::security && cd panel && bun run test -- SkillDetailPage`
- [ ] `SP375-T007` Owner: regression | Done when: TS typecheck, frontend tests, Rust check, and Rust tests all pass | Verify: `(cd panel && bun run typecheck && bun run test) && cargo check --workspace --all-targets --all-features && cargo test`

### SP375-T1: Add Shared Skill Inspect Read Model

Owner: backend

Files:

- `src/cli.rs`
- `src/commands/mod.rs`
- new or existing skill inspect command module
- tests for the inspect read model

Done when:

- `loom --json skill inspect <skill>` exists.
- Output includes `source`, `spec`, `runtime`, `eval`, `safety`, and
  `next_actions`.
- Missing data is represented as empty/null evidence, not a pass verdict.
- Missing skills return a structured error.

Verify:

```bash
cargo test --test skill_inspect
```

### SP375-T2: Add Panel Inspect API Route

Owner: backend
Depends on: SP375-T1

Files:

- `src/panel/mod.rs`
- `src/panel/handlers/skills.rs`
- `src/panel/tests/handlers/skill_endpoints.rs`
- `src/panel/tests/security.rs`
- `docs/LOOM_API_CONTRACT.md`

Done when:

- `GET /api/v1/skills/{skill_name}/inspect` is registered.
- Handler calls the same function as CLI inspect.
- Route is listed in the v1 API contract.
- Security tests include the new read route and do not add a mutation route.

Verify:

```bash
cargo test panel::tests
```

### SP375-T3: Add Frontend API Types

Owner: frontend
Depends on: SP375-T2

Files:

- `panel/src/lib/api/client.ts`
- optional `panel/src/lib/api/skill_inspect.ts`
- frontend API tests

Done when:

- Inspect payload is typed.
- Client unwraps the standard command envelope.
- API errors render through existing error handling.

Verify:

```bash
cd panel && bun run typecheck
```

### SP375-T4: Add Detail Navigation

Owner: frontend
Depends on: SP375-T3

Files:

- `panel/src/pages/PanelApp.tsx`
- `panel/src/pages/panel/SkillsPage.tsx`
- `panel/src/components/panel/CommandPalette.tsx`
- `panel/src/lib/types.ts`

Done when:

- Clicking a skill row opens the detail page or detail route state.
- Command palette skill entries open the same detail state.
- Refreshing or navigating back does not lose the selected detail tab; state
  must be stored in URL/hash routing or another refresh-safe route mechanism.
- Unknown skill ids show a structured empty/error state.

Verify:

```bash
cd panel && bun run test -- SkillDetailPage
```

### SP375-T5: Render Detail Sections

Owner: frontend
Depends on: SP375-T4

Files:

- `panel/src/pages/panel/SkillDetailPage.tsx`
- `panel/src/pages/panel/SkillDetailPage.test.tsx`
- panel CSS files as needed

Done when:

- Source section shows path, entrypoint, drift, ref, and provenance.
- Spec section shows portable, Codex, Claude, and findings.
- Runtime section shows per-agent active/projected/visible/enabled states.
- Eval section shows offline status, with-skill/no-skill summaries, trigger
  precision/recall, and baseline delta when present.
- Safety section shows trust, scan, quarantine, blocked state, and findings.
- Next actions render suggested commands.

Verify:

```bash
cd panel && bun run test -- SkillDetailPage
```

### SP375-T6: Preserve Safety Gates

Owner: safety
Depends on: SP375-T5

Files:

- `panel/src/pages/panel/SkillDetailPage.tsx`
- `src/panel/tests/security.rs`
- related mutation tests if new actions are added

Done when:

- Dangerous actions are command-copy affordances or existing confirmed mutation
  flows.
- Read-only mode disables mutation affordances.
- No frontend path calls a mutation endpoint that lacks CLI authorization and
  confirmation.

Verify:

```bash
cargo test panel::tests::security
cd panel && bun run test -- SkillDetailPage
```

### SP375-T7: Full Verification

Owner: testing
Depends on: SP375-T1, SP375-T2, SP375-T3, SP375-T4, SP375-T5, SP375-T6

Done when:

- Frontend typecheck passes.
- Frontend test suite passes.
- Rust check and full test suite pass.
- Tests cover pass, warning, error, disabled-by-config, needs-restart, and
  empty eval/safety states.

Verify:

```bash
git diff --check
cd panel && bun run typecheck
cd panel && bun run test
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #375` for partial backend or frontend slices. Use `Fixes #375` only
when the inspect read model, Panel API route, detail navigation, rendering, and
safety tests are complete.
