# GH388 Tasks: Native Package Export Bridges

Issue: https://github.com/majiayu000/loom/issues/388
Product spec: `specs/GH388/product.md`
Tech spec: `specs/GH388/tech.md`
Status: Draft for implementation

## Scope For First PR

Implement only the package export foundation:

```text
package plan + one deterministic portable archive build + package verify
```

Do not implement in the first PR:

```text
hosted marketplace submission, automatic external publish, plugin format claims
without adapter metadata, secret packaging, or active-state claims
```

## Tasks

- [ ] `SP388-T1` Owner: implementation | Done when: package plan/build/verify CLI parses and command ids classify plan/verify as read-only and build as write/output-producing | Verify: `cargo test --test cli_surface`
- [ ] `SP388-T2` Owner: implementation | Done when: package plan resolves skill/skillset source, provenance, gate status, file manifest, and forbidden-content findings without writes | Verify: `cargo test --test package_export`
- [ ] `SP388-T3` Owner: implementation | Done when: portable archive build revalidates plan/source digest, stages output, writes manifest/checksums, and is deterministic | Verify: `cargo test --test package_export`
- [ ] `SP388-T4` Owner: implementation | Done when: package verify detects checksum mismatch, stale source digest, forbidden paths/secrets, malformed metadata, and lint failures | Verify: `cargo test --test package_export`
- [ ] `SP388-T5` Owner: implementation | Done when: Codex/Claude/npm/GitHub format adapters are gated by adapter metadata and return typed unsupported results until implemented | Verify: `cargo test --test package_export`
- [ ] `SP388-T6` Owner: implementation | Done when: CLI docs/specs cover package boundaries and repository checks pass | Verify: `git diff --check && cargo check --workspace --all-targets --all-features`

### SP388-T1: Add CLI Surface

Owner: implementation

Files:

- `src/cli.rs` or a split package args module
- `src/commands/mod.rs`
- `src/commands/helpers.rs`

Done when:

- `loom package plan <skill|skillset> --format <format> [--agent <agent>] [--json]` parses.
- `loom package build <plan-id> --output <path> --idempotency-key <key> [--json]` parses.
- `loom package verify <artifact> [--format <format>] [--json]` parses.
- deferred publish is absent or returns typed not-implemented.
- command classification treats plan/verify as read-only and build as
  output-producing.

Verify:

```bash
cargo test --test cli_surface
```

### SP388-T2: Implement Package Plan

Owner: implementation
Depends on: SP388-T1

Done when:

- plan resolves skill source from registry state.
- plan resolves skillset source when #377 read model exists.
- plan includes source ref and digest.
- plan consumes lint, safety/trust, eval, and approval gate status.
- plan returns a redacted file manifest.
- plan rejects or warns on local absolute paths, private registry state,
  user-specific config, and unsupported format fields.
- plan writes no artifacts.

Verify:

```bash
cargo test --test package_export
```

### SP388-T3: Implement Portable Archive Build

Owner: implementation
Depends on: SP388-T2

Done when:

- build revalidates plan, source digest, policy, and gate status.
- build requires an idempotency key.
- build stages files before moving the final artifact.
- generated artifact includes `manifest.json`, checksums, provenance metadata,
  skill source files, references, scripts, and assets allowed by policy.
- repeated build from identical inputs is deterministic except declared
  timestamp or attestation fields.
- build output returns install/verify guidance and no active-state claim.

Verify:

```bash
cargo test --test package_export
```

### SP388-T4: Implement Package Verify

Owner: implementation
Depends on: SP388-T3

Done when:

- verify parses manifest and metadata.
- verify checks all checksums.
- verify detects stale source digest when source is available.
- verify scans for local absolute paths and secret-looking values.
- verify runs portable lint on packaged skill content.
- verify reports malformed or unsupported format metadata as typed failures.

Verify:

```bash
cargo test --test package_export
```

### SP388-T5: Add Native Format Adapter Gates

Owner: implementation
Depends on: SP388-T2

Done when:

- `codex-plugin`, `claude-plugin`, `npm`, and `github-release` formats are
  represented in the format enum.
- unsupported formats fail clearly until adapter metadata and docs are wired.
- implemented adapters define copied allowlists, generated files, required
  metadata, forbidden file patterns, and verify rules.
- no adapter silently drops unsupported fields.

Verify:

```bash
cargo test --test package_export
```

### SP388-T6: Update Docs And Final Checks

Owner: implementation
Depends on: SP388-T1, SP388-T2, SP388-T3, SP388-T4

Done when:

- CLI contract documents package plan/build/verify and publish boundary.
- provider boundary docs link outbound packaging to Loom registry authority.
- tests cover first-slice acceptance criteria.
- repository checks pass.

Verify:

```bash
git diff --check
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

- Use `Refs #388` for a first-slice PR unless plan, build, verify, native
  adapters, publish boundary, and every acceptance criterion are implemented.
- Do not use `Fixes #388` until deterministic build and verify are implemented
  and native plugin/package formats are either supported or explicitly split.
- Do not package secrets, private registry state, local absolute paths, or
  user-specific config.
