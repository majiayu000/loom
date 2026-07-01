# GH384 Tech Spec: Compiled Runtime Interface

Issue: https://github.com/majiayu000/loom/issues/384
Product spec: `specs/GH384/product.md`
Status: Draft for implementation

## Design Summary

Add a compiler planning layer that derives a bounded runtime view from portable
skill source. The compiler must be deterministic, local, and gate-aware:

1. Load the skill through the existing registry and portable skill read model.
2. Parse `SKILL.md` frontmatter and body with structured parsers.
3. Build a compile plan with activation text, references index, boundary
   records, tool interface records, source digest, and gate statuses.
4. Keep `--dry-run` read-only.
5. Verify existing artifacts before compiled activation can use them.
6. Reuse lint, safety, dependency, eval, and adapter status instead of
   duplicating policy decisions.

## Dependencies And Blocks

GH384 depends on these primitives:

| Issue | Required capability |
|---|---|
| #365 | Portable and agent-specific lint status |
| #366 | `skill inspect` status model |
| #369 | Eval evidence and regression thresholds |
| #370 | Safety, trust, quarantine, and policy gates |
| #371 | Dependency and MCP readiness status |
| #373 | Adapter-specific projection and visibility status |
| #378 | Capability graph consumers of compiled metadata |

The first implementation may expose missing dependency gates as `blocked` or
`missing`, but it must not mark artifacts `valid` when a required gate is not
available.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs` or split skill compile args module |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs` |
| compiler model | new `src/commands/skill_compile.rs` or module directory |
| skill inspection | existing inspect/status module after #366 |
| activation projection | existing activation/adapters after #367/#373 |
| tests | new `tests/skill_compile.rs`, plus inspect/activation tests when wired |
| docs/specs | `specs/GH384/*`, CLI contract docs if command text changes |

## Data Model

Suggested artifact path:

```text
state/compiled/skills/<skill>/<artifact-id>/
```

Suggested Rust model:

```rust
struct CompiledArtifactManifest {
    schema_version: u32,
    artifact_id: String,
    skill: String,
    agent: String,
    profile: String,
    source_ref: String,
    source_tree_oid: Option<String>,
    source_digest: String,
    compiler_version: String,
    status: ArtifactStatus,
    gates: CompileGateStatus,
    token_estimate: CompileTokenEstimate,
    created_at: String,
}
```

Status values:

```text
planned | experimental | valid | stale | blocked | invalid
```

Gate values:

```text
pass | warning | missing | blocked | fail
```

## Source Digest

The digest must be deterministic and include every source input that affects the
compiled artifact:

1. `SKILL.md` bytes.
2. Portable metadata that affects activation.
3. Referenced files that are indexed or summarized.
4. Script paths and executable metadata that are exposed to the runtime
   interface.
5. Compiler version and agent profile.

When a source digest does not match the manifest digest, verification returns a
typed stale result. It must not silently fall back to the artifact.

## Compile Planning

`skill compile --dry-run` should:

1. Resolve the skill through the existing registry read path.
2. Validate the skill exists and is not quarantined or blocked by policy.
3. Parse frontmatter and body through existing lint parser components where
   available.
4. Extract trigger boundaries, non-goals, safety constraints, dependencies,
   tool requirements, and workflow steps.
5. Build `activation.md` content from deterministic sections.
6. Build `references.index.json` with paths, roles, and load conditions.
7. Build `boundaries.json` with triggers, non-triggers, deferred operations,
   and required handoff fields.
8. Build `tool-interface.json` from allowed tools and script entrypoints.
9. Estimate source and activation token counts with a deterministic local
   estimator.
10. Return planned paths, planned content hashes, token estimates, and gate
    statuses.

## Verification

`skill compile verify` should:

1. Load `manifest.json`.
2. Ensure all required files exist.
3. Validate each JSON file with typed schema parsing.
4. Recompute the source digest and compare it with the manifest.
5. Run or consume current lint status.
6. Run or consume current safety/trust status.
7. Run or consume dependency readiness.
8. Require eval evidence before returning `valid`.
9. Return a structured report for `skill inspect`.

Verification failures should use typed errors for malformed input and structured
blocked status for unavailable gates. Missing eval evidence should block
promotion to `valid`, not pretend success.

## Activation Projection

Compiled activation is deferred until #367 and #373 are stable. When wired:

1. `--compiled` requires an artifact whose verification status is `valid`.
2. If no valid artifact exists, the command fails with a typed next action.
3. If an agent adapter cannot consume native compiled artifacts, Loom may
   materialize an agent-compatible `SKILL.md` derived from `activation.md`.
4. Materialized projections must preserve links to source skill files and
   artifact manifest metadata.
5. Normal activation without `--compiled` must not depend on compiled artifacts.

## Test Plan

Focused tests:

1. dry-run returns a plan and writes no files;
2. small skill returns no-op with explanation;
3. manifest parses and round-trips deterministically;
4. missing artifact files fail verification;
5. source edit causes stale digest verification result;
6. lint, safety, dependency, or eval gate failure prevents `valid`;
7. inspect includes artifact status once inspect wiring exists;
8. compiled activation rejects missing or stale artifacts once activation wiring
   exists.

Suggested commands:

```bash
git diff --check
cargo test --test skill_compile
cargo check --workspace --all-targets --all-features
```

Run SpecRail workflow validation for this packet when available.

## Rollback

The first slice should be isolated to CLI parsing, compile planning,
verification models, tests, docs, and optional generated artifact paths.
Rollback removes the command group and compiler module without changing portable
skill source, normal activation, or registry projection behavior.

## Risks

1. Compiled artifacts can become a second source of truth. Mitigation: always
   verify against source digest and keep `SKILL.md` authoritative.
2. Summarization can remove safety constraints. Mitigation: deterministic
   section extraction and safety gate comparison before `valid`.
3. Token savings can be overstated. Mitigation: local estimates are advisory
   until eval evidence exists.
4. Agent adapters can drift. Mitigation: include agent/profile in the digest
   and verification report.
