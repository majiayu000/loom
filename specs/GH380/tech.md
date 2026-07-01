# GH380 Tech Spec: Provider Catalog And Safe Install

Issue: https://github.com/majiayu000/loom/issues/380
Product spec: `specs/GH380/product.md`
Status: Blocked design packet

## Current State

`docs/SKILL_PROVIDER_BOUNDARY.md` is the accepted V1 provider boundary. It says
Loom is a local control plane, not a marketplace or downloader. Existing
`skill add` already supports local paths, Git URLs, and
`github:owner/repo//subdir`, and records provenance in `state/registry/sources.json`
plus `loom.lock`.

GH380 should build on that contract instead of bypassing it.

Relevant files:

- `docs/SKILL_PROVIDER_BOUNDARY.md`
- `docs/LOOM_CLI_CONTRACT.md`
- `src/commands/provenance.rs`
- `src/commands/skill_cmds.rs`
- `tests/skill_provenance.rs`
- `tests/skill_policy.rs`

## State Model

Provider config:

```text
state/registry/providers.json
```

Recommended shape:

```json
{
  "schema_version": 1,
  "providers": [
    {
      "id": "github",
      "kind": "github",
      "url": "https://github.com",
      "capabilities": ["search", "preview", "fetch", "provenance"],
      "trust_default": "third-party-unreviewed",
      "requires_network": true,
      "created_at": "2026-07-01T00:00:00Z",
      "updated_at": "2026-07-01T00:00:00Z"
    }
  ]
}
```

Records must be deterministic before write. Malformed provider state must fail
without overwrite. Provider URLs must be credential-free before persistence:
reject userinfo, embedded tokens, password fields, and token-like query
parameters. If credentials are required in a later slice, store only a redacted
URL plus an external credential reference, never the secret value.

## Locator Parser

Support normalized locators:

```text
github:owner/repo//skills/foo@v1.2.3
corp-github:owner/repo//skills/foo@v1.2.3
local:/path/to/catalog//skills/foo@sha256:<digest>
```

Parser output should include:

- provider id resolved from the locator prefix, then provider kind from
  `state/registry/providers.json`
- owner/repo or local base
- subdir
- requested ref
- whether ref is pinned

The prefix namespace is provider ids. `github:` and `local:` are default
provider ids. Unknown prefixes fail with a provider-specific typed error such as
`PROVIDER_NOT_FOUND`; the implementation slice that introduces this error must
add it to `src/types.rs`, `docs/LOOM_CLI_CONTRACT.md`, and CLI envelope tests.
`team:` is reserved for a later org-provider slice and must fail as unsupported
in v1.
Built-in defaults such as `github:` and `local:` are synthesized in memory for
read-only parse, catalog, and preview flows in a fresh registry. Persisting or
customizing them still requires an explicit provider write command; read-only
commands must not seed provider state.

Pinned refs include immutable commit SHAs. Tags are acceptable only when policy
verifies immutability/signature and provenance records the resolved commit and
source digest. Branch names are moving refs and should fail under strict policy.
Local locators are pinned only when they include a content digest or reviewed
snapshot id; strict policy rejects mutable local directories without that pin.

## Provider Abstraction

Recommended trait:

```rust
trait SkillProvider {
    fn search(&self, query: &CatalogSearch) -> ProviderResult<Vec<CatalogResult>>;
    fn show(&self, locator: &SkillLocator) -> ProviderResult<CatalogResult>;
    fn preview(&self, locator: &SkillLocator) -> ProviderResult<CatalogPreview>;
    fn fetch(&self, locator: &SkillLocator, dest: &Path) -> ProviderResult<ProviderProvenance>;
}
```

Provider implementations:

- `local`: filesystem catalog preview/fetch.
- `github`: GitHub locator convenience over direct Git.
- `git`: internal fallback for Git-backed fetch.
- `gh_skill`: optional future read-only discovery/preview adapter.

No v1 `team` provider is exposed until its policy, membership, provenance, and
fetch semantics are specified.

Do not call `gh skill install` from Loom.

## Preview Contract

Preview fetches or reads content into an isolated temporary/staging area, then
inspects files without executing them.

Preview output:

- metadata
- file tree
- scripts present
- license
- provenance hints
- lint summary when possible
- safety scan summary when possible
- warnings

Scripts must not run. Build hooks must not run. Preview must not write registry
state or target directories.

## Install Dry-Run And Apply

`skill install --dry-run` should return:

- resolved locator
- pinned/ref policy result
- staging fetch plan
- lint result
- safety scan result
- provenance record that would be written
- trust state that would be assigned
- next actions

Apply path should reuse existing registry import, provenance, lockfile, command
audit, and policy patterns. If implementation chooses to route through existing
`skill add`, provider-specific install logic must still attach trust and scan
evidence.

## Policy And Trust

Trust values:

- `third-party-unreviewed`
- `reviewed`
- future org/team trust states from #381

Rules:

1. Public provider installs default to `third-party-unreviewed`.
2. Strict policy rejects unpinned moving refs.
3. Critical safety findings block install.
4. Network provider calls require explicit provider configuration or locator
   intent.
5. Secrets are never printed in provider config or preview output.

## Tests

Focused tests:

1. provider add/list/remove persists deterministic config.
2. locator parser handles GitHub/local/custom-provider locators, pin status, and
   reserved-but-unsupported `team:` errors.
3. search mock returns advisory catalog results.
4. preview mock never executes scripts.
5. install dry-run writes nothing.
6. pinned install creates provenance and lockfile records.
7. unpinned strict install fails.
8. critical scan failure blocks install.
9. public install defaults trust to `third-party-unreviewed`.

## Verification

```bash
git diff --check
cargo test --test skill_provenance
cargo test --test skill_policy
cargo check --workspace --all-targets --all-features
cargo test
```

## Handoff Notes

Use `Refs #380` for design-only or partial provider slices. Use `Fixes #380`
only after provider config, preview, dry-run install, pinned install,
provenance/lockfile, trust defaults, and policy gates are implemented and
verified.
