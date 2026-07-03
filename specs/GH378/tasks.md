# GH378 Tasks: Capability Graph And Recommendations

Issue: https://github.com/majiayu000/loom/issues/378
Product spec: `specs/GH378/product.md`
Tech spec: `specs/GH378/tech.md`
Status: Implemented

## Scope For First PR

Implement the first read-only recommendation foundation:

```text
local index schema + deterministic lexical recommendations + semantic-disabled fallback + no activation
```

Do not implement:

```text
automatic activation, network embedding services by default, DAG workflow execution
```

## Tasks

- [x] `SP378-T001` Owner: index-model | Done when: derived `state/index` schemas are defined for lexical and capability records without making index data the source of truth | Verify: `git diff --check`
- [x] `SP378-T002` Owner: cli-index | Done when: `loom index build` writes only rebuildable derived `state/index` data, `loom index status` is read-only, and both are deterministic over current registry data without network access | Verify: `cargo test --test skill_inventory_cli`
- [x] `SP378-T003` Owner: recommend | Done when: `loom skill recommend` ranks skills and skillsets with transparent `kind`, `id`, `score_inputs`, `reasons`, `risks`, warnings, and suggested commands | Verify: `cargo test --test skill_inventory_cli`
- [x] `SP378-T004` Owner: semantic | Done when: `skill recommend --semantic` and `skill resolve --semantic` both fall back to lexical with a `semantic-disabled` warning when no local provider is configured | Verify: `cargo test --test skill_inventory_cli`
- [x] `SP378-T005` Owner: safety-policy | Done when: blocked/quarantined skills and skillsets with unsafe required members are never recommended for activation, negative trigger matches reduce ranking or filter activation recommendations, and dependency/eval gaps are surfaced as penalties or warnings | Verify: `cargo test --test skill_policy && cargo test --test skill_eval`
- [x] `SP378-T006` Owner: active-plan | Done when: `active recommend` returns a dry-run add/keep/remove plan with suggested commands and no mutation | Verify: `cargo test --test skill_inventory_cli`
- [x] `SP378-T007` Owner: regression | Done when: focused and full repository checks pass | Verify: `cargo check --workspace --all-targets --all-features && cargo test`

### SP378-T1: Define Derived Index Records

Owner: backend

Files:

- new index module
- docs or schema files for `state/index`
- `specs/GH378/*`

Done when:

- Lexical and capability index record shapes are documented.
- Records include schema version and source digest.
- Index files are rebuildable derived data.
- Optional embeddings are local-provider-only and not required for v1.

Verify:

```bash
git diff --check
```

### SP378-T2: Add Index CLI

Owner: backend
Depends on: SP378-T1

Files:

- `src/cli.rs`
- new index CLI module
- new index command module
- tests

Done when:

- `loom index build` rebuilds local derived index data.
- `loom index status` reports freshness, counts, and warnings.
- build does not write registry source, active views, agent config, target dirs,
  or MCP config.
- Build works without network access.
- Invalid skill records produce warnings instead of silent omission.

Verify:

```bash
cargo test --test skill_inventory_cli
```

### SP378-T3: Add Skill Recommendation

Owner: backend
Depends on: SP378-T1, SP378-T2

Files:

- `src/cli/discovery.rs`
- `src/commands/skill_inventory.rs`
- tests

Done when:

- `loom skill recommend <task>` returns ranked candidates.
- Output includes score inputs, reasons, risks, warnings, recommended action,
  and suggested commands.
- Lexical-only mode is deterministic.
- Tie-breaking is stable by score descending, result kind, result id, and source
  path when needed.
- Skillset results use read-only inspection commands unless a later lifecycle
  explicitly defines activation; they must not emit `skill activate
  <skillset-id>`.

Verify:

```bash
cargo test --test skill_inventory_cli
```

### SP378-T4: Add Semantic Disabled Fallback

Owner: backend
Depends on: SP378-T3

Done when:

- `skill recommend --semantic` and `skill resolve --semantic` do not call
  network services by default.
- Missing local semantic provider returns a warning and lexical fallback for
  both command surfaces.
- Output labels mode as `semantic-disabled`.

Verify:

```bash
cargo test --test skill_inventory_cli
```

### SP378-T5: Join Safety, Dependency, Eval, And Skillset Evidence

Owner: backend
Depends on: SP378-T3
Blocked by: #369, #370, #371, #377

Done when:

- Blocked/quarantined skills are excluded from activation recommendations.
- Skillsets with blocked, quarantined, policy-blocked, or dependency-unready
  required members are excluded from activation recommendations or degraded to
  read-only inspection with member-level risks.
- Missing dependencies reduce ranking and appear in risks.
- Positive eval evidence can boost ranking, negative trigger evidence reduces
  ranking or filters activation, and missing eval appears as a warning.
- Skillset recommendations explain member coherence.

Verify:

```bash
cargo test --test skill_policy
cargo test --test skill_eval
cargo test --test skillset_cli
```

### SP378-T6: Add Active Recommend Dry-Run Plan

Owner: backend
Depends on: SP378-T5
Blocked by: #367, #368

Done when:

- Command compares current active state to recommended state.
- Output includes add/keep/remove arrays.
- Suggested commands include dry-run activation/deactivation commands.
- No registry, target, agent config, or active-view mutation occurs.

Verify:

```bash
cargo test --test skill_inventory_cli
```

### SP378-T7: Full Verification

Owner: testing
Depends on: SP378-T1, SP378-T2, SP378-T3, SP378-T4, SP378-T5, SP378-T6

Done when:

- Focused tests cover acceptance criteria.
- Full check and test suites pass.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

`Fixes #378` is appropriate after the focused and full verification commands
pass because index build, deterministic recommendations, semantic-disabled
fallback, policy/dependency/eval filtering, skillset recommendations, and active
dry-run plans are implemented.
