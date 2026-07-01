# GH388 Product Spec: Native Package Export Bridges

Issue: https://github.com/majiayu000/loom/issues/388
Parent: https://github.com/majiayu000/loom/issues/376
Status: Draft for implementation
Locale: en-US

## Goal

Add native packaging and export bridges so reviewed Loom-managed skills or
skillsets can be built into external agent/package distribution artifacts
without weakening Loom's registry, provenance, policy, eval, rollback, or
activation gates.

This is the outbound counterpart to safe discovery/install. Loom remains the
local control plane; exported artifacts are distribution artifacts, not proof
that a skill is active, visible, trusted, or installed.

## Users

1. Maintainers who want to ship reviewed skills as portable archives.
2. Teams that need deterministic artifacts with provenance and checksums.
3. Users who need agent-native bundles for Codex, Claude, npm-style packaging,
   or GitHub release assets.

## Scope For First PR

The first mergeable slice should implement the package planning and verification
foundation:

- `package plan` produces a dry-run manifest without writing artifacts;
- package source is a single skill or skillset with provenance and source
  digest;
- package checks include lint, safety/trust, eval, and approval status;
- package file manifests exclude secrets, private registry state, local
  absolute paths, and user-specific config;
- `package build` may start with one deterministic portable archive format;
- `package verify` validates manifest, checksums, metadata, and forbidden
  content.

Codex/Claude plugin formats can be added incrementally behind adapter metadata
and current format docs.

## Non-Goals

1. No first-party hosted marketplace.
2. No automatic submission to external marketplaces.
3. No bypass of Loom safety scan, provenance, lint, eval, approval, or release
   gates.
4. No packaging secrets, private registry state, local absolute paths, or
   user-specific config.
5. No claim that a packaged artifact is active or visible until installed,
   provisioned, and verified separately.
6. No direct use of `gh skill install` or host-specific install commands as a
   Loom apply path.

## Behavior Invariants

1. `package plan` is read-only.
2. `package build` revalidates the plan and source digest before writing.
3. Every artifact records source kind, source id, source ref, source digest,
   package format, Loom version, created-at timestamp, and checksums.
4. Packaging third-party-unreviewed or quarantined skills is blocked unless
   policy explicitly allows a draft/private artifact.
5. Package manifests must not contain local absolute paths, secrets, env values,
   user-specific config, or private registry state.
6. Unsupported fields for a package format fail clearly instead of being
   silently dropped.
7. Build output returns install/verify guidance, not active-state claims.
8. Publish is dry-run by default and uses provider-specific boundaries; external
   marketplaces remain outside Loom's local registry authority.
9. Rebuilding the same source ref and format should produce deterministic file
   content except for declared timestamp or attestation fields.

## User-Facing CLI

Required first-slice commands:

```bash
loom package plan <skill|skillset> --format agent-skills-archive [--agent <agent>] [--json]
loom package build <plan-id> --output <path> --idempotency-key <key> [--json]
loom package verify <artifact> [--format <format>] [--json]
```

Deferred commands and formats:

```bash
loom package plan <skill|skillset> --format codex-plugin|claude-plugin|npm|github-release [--agent <agent>] [--json]
loom package publish <artifact> --provider github-release|npm|manual --dry-run [--json]
```

## Package Formats

Initial format:

- `agent-skills-archive`: portable skill or skillset archive containing
  `SKILL.md`, references, scripts, assets, manifest, checksums, and provenance.

Incremental formats:

- `codex-plugin`: `.codex-plugin/plugin.json` and bundled skills/assets/scripts
  where supported by current Codex plugin metadata.
- `claude-plugin`: Claude-compatible plugin layout where supported by current
  docs and adapter metadata.
- `npm`: package layout that exposes skills and optional tool dependencies
  without executing install scripts by default.
- `github-release`: release asset manifest, checksums, and optional attestation
  metadata.

## Plan Model

`package plan` should return:

```json
{
  "plan_id": "pkgplan_...",
  "source": {
    "kind": "skill",
    "id": "fixflow",
    "source_ref": "v1.0.0",
    "source_digest": "sha256:..."
  },
  "format": "agent-skills-archive",
  "files": [
    {"path": "skills/fixflow/SKILL.md", "kind": "copied"},
    {"path": "manifest.json", "kind": "generated"},
    {"path": "checksums.txt", "kind": "generated"}
  ],
  "checks": {
    "portable_lint": "pass",
    "safety_scan": "pass",
    "eval_gate": "pass",
    "approval": "not_required"
  },
  "warnings": []
}
```

## Acceptance Criteria

1. `package plan` returns a dry-run package manifest without writing artifacts.
2. `package build` creates a deterministic artifact for at least one portable
   archive format.
3. The artifact records source ref, source digest, package format, created-at,
   Loom version, manifest, and checksums.
4. `package verify` catches checksum mismatch, stale source digest, forbidden
   absolute paths, secrets, and malformed plugin/package metadata.
5. Codex/Claude plugin formats are gated by adapter metadata and current docs.
6. Unsupported package fields fail clearly instead of being silently dropped.
7. Packaging third-party-unreviewed or quarantined skills is blocked unless
   policy explicitly allows a draft/private artifact.
8. Build output gives install and verify commands, not active-state claims.
9. Tests cover plan, build, verify, source-ref mismatch, safety block,
   local-path redaction, unsupported format, and deterministic rebuild.

## Open Questions

1. Whether package plans should be persisted in registry state or reproduced
   from command audit and source digest.
2. Whether `github-release` publishing should call external CLIs or only produce
   release-ready assets and instructions in v1.
3. Whether npm packaging should include optional dependencies or only metadata
   references until MCP provisioning is stable.
