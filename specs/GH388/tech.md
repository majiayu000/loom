# GH388 Tech Spec: Native Package Export Bridges

Issue: https://github.com/majiayu000/loom/issues/388
Product spec: `specs/GH388/product.md`
Status: Draft for implementation

## Design Summary

Add package export as a plan/build/verify workflow. Package planning is
read-only, package build revalidates the source and gates, and package verify
checks manifest integrity and forbidden content. Native agent formats are
format adapters over the same package plan model.

Loom must not use external install/publish commands to bypass its registry,
provenance, lockfile, policy, eval, or rollback contracts.

## Dependencies And Blocks

| Issue | Required capability |
|---|---|
| #365 | portable and agent-specific lint |
| #366 | single-skill status and inspect model |
| #370 | safety, trust, quarantine, and approval gates |
| #380 | catalog/provider boundaries and provenance policy |
| #377 | skillset source model for grouped packages |

The first implementation may support only single-skill portable archives if
skillset or adapter metadata is not yet available.

## Affected Areas

| Area | Files |
|---|---|
| CLI surface | `src/cli.rs` or split package args module |
| command dispatch | `src/commands/mod.rs`, `src/commands/helpers.rs` |
| package implementation | new `src/commands/package.rs` or module directory |
| provenance | existing provenance/lockfile modules |
| adapter metadata | agent adapter metadata after #373/#380 |
| tests | new `tests/package_export.rs`, CLI tests |
| docs/specs | `specs/GH388/*`, CLI contract docs |

## Package Plan Model

Suggested model:

```rust
struct PackagePlan {
    plan_id: String,
    source: PackageSource,
    format: PackageFormat,
    source_digest: String,
    source_ref: String,
    checks: PackageGateStatus,
    files: Vec<PackageFilePlan>,
    warnings: Vec<PackageFinding>,
}
```

Supported initial source kinds:

```text
skill
skillset
```

Supported initial formats:

```text
agent-skills-archive
```

Deferred formats:

```text
codex-plugin
claude-plugin
npm
github-release
```

## Plan Behavior

`package plan` should:

1. resolve the skill or skillset through registry state;
2. require a clean source ref or explicit draft packaging mode;
3. verify provenance and source digest through existing provenance modules;
4. consume portable/agent lint status;
5. consume safety/trust/quarantine status;
6. consume eval gate status when required by policy;
7. build a redacted file manifest;
8. reject local absolute paths, secrets, private registry state, and
   user-specific config;
9. return format-specific unsupported-field findings;
10. write no artifacts.

## Build Behavior

`package build` should:

1. load or reproduce the package plan;
2. revalidate source digest, source ref, policy, and gate statuses;
3. require an idempotency key;
4. build into a staging directory first;
5. copy source files through path allowlists;
6. generate `manifest.json`, `checksums.txt`, and provenance metadata;
7. avoid executable install hooks unless policy permits and the artifact marks
   them clearly;
8. atomically move the completed artifact to the requested output path;
9. return install/verify guidance without active-state claims.

## Verify Behavior

`package verify` should:

1. parse manifest and format metadata;
2. verify checksums;
3. verify source digest where the source is available;
4. scan for local absolute paths;
5. scan for secret-looking values and forbidden private registry state;
6. run portable lint on packaged skill content;
7. validate format-specific required files;
8. report unsupported or malformed plugin/package metadata as typed failures.

## Format Adapters

Each format adapter should implement:

1. supported source kinds;
2. generated files;
3. copied file allowlist;
4. forbidden file patterns;
5. required metadata;
6. verification rules;
7. install guidance text.

Codex and Claude plugin formats must be gated by adapter metadata and current
docs. If metadata is missing, the adapter returns unsupported rather than
dropping fields.

## Publish Boundary

`package publish` is deferred. When added, it should be dry-run-first and keep
provider boundaries explicit:

1. GitHub release or npm publishing may upload external artifacts, but it does
   not mutate Loom registry source truth.
2. Dry-run returns commands, release notes, checksums, and provider metadata.
3. Non-dry-run publish requires explicit provider credentials and approval.
4. Publishing success is not active/visible proof for any local agent.

## Test Plan

Focused tests:

1. plan single skill portable archive;
2. plan writes no artifacts;
3. build deterministic archive;
4. manifest contains source ref, digest, format, Loom version, and checksums;
5. verify detects checksum mismatch;
6. verify detects stale source digest when source is available;
7. verify rejects local absolute paths;
8. verify rejects secret-looking values;
9. safety/quarantine blocks package build;
10. unsupported format fails clearly;
11. repeated build with same source is deterministic.

Suggested commands:

```bash
git diff --check
cargo test --test package_export
cargo check --workspace --all-targets --all-features
```

Run SpecRail workflow validation for this packet when available.

## Rollback

The first slice should be isolated to package plan/build/verify commands,
format adapter models, tests, docs, and generated output paths. Rollback removes
the package command group and ignores/deletes generated package artifacts
without changing registry state, active projections, or skill source.

## Risks

1. Exported artifacts can leak local data. Mitigation: file allowlists,
   forbidden path scans, and secret scans.
2. Plugin formats can drift. Mitigation: adapter metadata gates and typed
   unsupported results.
3. Packaged artifact can be mistaken for active install. Mitigation: output
   verify/install guidance only, no active-state claims.
4. Determinism can churn artifacts. Mitigation: stable ordering and explicit
   timestamp/attestation exceptions.
