# GH380 Product Spec: Provider Catalog And Safe Install

Issue: https://github.com/majiayu000/loom/issues/380
Parent: https://github.com/majiayu000/loom/issues/376
Status: Blocked design packet
Locale: zh-CN

## Goal

Add catalog and provider integrations for safe discovery, preview, dry-run
install, provenance, policy review, and rollback-ready registry import.

Provider support must not turn Loom into an untrusted one-click installer.
Providers may discover, preview, fetch, or resolve upstream skill content. Loom
continues to own local registry state, `loom.lock`, policy, projection, audit,
rollback, and eval.

## Blocking Dependencies

Production implementation is blocked by:

- #365 portable/agent/quality lint.
- #366 single-skill inspect/status.
- #370 safety/trust/quarantine.
- #371 dependency and MCP readiness.

## User-Facing Commands

Target command surface:

```bash
loom provider add <id> --kind github|local --url <url>
loom provider list
loom provider remove <id>
loom catalog search <query> [--provider <provider-id>] [--allow-network] [--agent <agent>] [--json]
loom catalog show <locator> [--json]
loom catalog preview <locator> [--ref <ref>] [--json]
loom skill install <locator> --name <skill> [--ref <branch|tag|sha>] [--trust third-party-unreviewed|reviewed] [--review-evidence <id>] [--policy-profile <profile>] [--dry-run]
```

If a locator already contains `@ref`, `--ref` must either match that ref or the
command must fail with a typed conflicting-ref error. Provenance records must
show the authoritative resolved ref.

Locator examples:

```text
github:owner/repo//skills/foo@v1.2.3
corp-github:owner/repo//skills/foo@v1.2.3
local:/path/to/catalog//skills/foo@sha256:<digest>
```

## Non-Goals

1. No first-party hosted marketplace in v1.
2. No auto-trusting public skills.
3. No remote code execution during preview.
4. No install without provenance and lockfile records.
5. No direct `gh skill install` or host-agent directory writes from Loom apply
   paths.
6. No automatic activation after install.
7. No provider metadata that overrides Loom policy decisions.

## Provider Model

Provider records describe capability and trust defaults:

```json
{
  "id": "github",
  "kind": "github",
  "capabilities": ["search", "preview", "fetch", "provenance"],
  "trust_default": "third-party-unreviewed",
  "requires_network": true
}
```

Locator prefixes are provider ids, not only hard-coded provider kinds. The
built-in default provider ids are `github` and `local`; custom ids such as
`corp-github` use the same prefix position in locators, for example
`corp-github:owner/repo//skills/foo@v1.2.3`. Team/org providers are deferred
until a policy-backed provider contract defines search, preview, fetch, and
provenance semantics.

Provider unavailability should return structured warnings for read-only
surfaces. Local and direct Git/GitHub locators must not require optional `gh
skill` support.

## Catalog Result Model

Catalog results should be advisory until installed through Loom:

```json
{
  "locator": "github:owner/repo//skills/fixflow@v1.2.3",
  "name": "fixflow",
  "description": "Use when diagnosing and fixing failing tests or CI failures.",
  "source": {
    "provider": "github",
    "repo": "owner/repo",
    "ref": "v1.2.3",
    "subdir": "skills/fixflow"
  },
  "signals": {
    "stars": null,
    "last_updated": null,
    "license": "MIT",
    "verified": false
  },
  "warnings": ["third-party-unreviewed"]
}
```

## Preview Behavior

Preview must not execute scripts. It should show:

- `SKILL.md` metadata.
- File tree.
- Scripts present.
- License and provenance signals.
- Preliminary scan findings.
- Estimated risk and trust.
- Suggested install dry-run command.

Catalog search is local-only by default. Network-backed providers such as GitHub
may be queried only when the provider is explicitly selected and the command uses
an explicit network opt-in such as `--allow-network`; locator-based preview still
counts as explicit locator intent.

## Install Behavior

`skill install` should:

1. Resolve locator.
2. Require a pinned ref or warn/fail depending on policy.
3. Fetch into staging.
4. Run portable lint.
5. Run safety scan.
6. Create provenance and lockfile records.
7. Import into registry.
8. Mark trust as `third-party-unreviewed` unless `reviewed` is backed by an
   auditable review/approval evidence id accepted by policy.
9. Return next actions: inspect, scan, activate.

Existing remote `loom skill add <git-url|github:...>` paths must either route
through the same provider pin, scan, provenance, trust, and policy gates once
GH380 lands, or be explicitly deprecated/disabled for remote provider content so
they cannot bypass `skill install`.

## Policy Gates

1. Strict policy rejects unpinned moving refs. Tags are treated as mutable unless
   policy verifies tag immutability/signature and records the resolved commit
   digest. Install uses the explicit `--policy-profile` when provided, otherwise
   the registry default policy profile; if neither is configured, behavior must
   fail closed for unpinned refs instead of silently allowing them.
2. Critical scan findings block install unless a future explicit risk override
   exists and policy permits it.
3. Public skills default to `third-party-unreviewed`.
4. Activation remains a separate command with its own safety gates.

## Acceptance Criteria

1. Providers can be listed, added, and removed.
2. GitHub/local providers support preview and install dry-run.
3. Install creates provenance/lockfile records and trust state.
4. Public skills default to `third-party-unreviewed`.
5. Preview never executes code.
6. Strict policy rejects unpinned refs.
7. Tests cover provider config, search mock, preview mock, install dry-run,
   pinned install, unpinned rejection, scan failure, and provenance record
   creation.
