# Skill Provider Boundary

Status: Accepted for V1 provider planning
Issue: https://github.com/majiayu000/loom/issues/347
Checked: 2026-06-26

## Decision

Loom is a local skill control plane, not a skill marketplace or downloader.
Provider integrations may discover and resolve upstream skill content, but Loom
owns the registry, `loom.lock`, policy, plan/apply, projection, audit, rollback,
and eval contracts after content enters a registry.

This keeps one authority for local state:

1. providers answer "where can this source be fetched, and what upstream ref did
   it resolve to?"
2. Loom answers "is this source allowed, pinned, projected, audited, rolled
   back, and evaluated for this team or workspace?"

## Current Provider Roles

| Surface | Owns | Must Not Own |
|---------|------|--------------|
| GitHub / `gh skill` | upstream discovery, preview, publish, upstream metadata, optional source resolution | Loom registry state, `loom.lock`, target bindings, live projections, rollback, eval, local/team policy |
| direct Git URL | clone/fetch, requested ref resolution, commit/tree identity | policy decisions, projection writes, mutable target installs |
| GitHub locator | normalized GitHub repository URL, subdirectory, requested ref | `gh` authentication assumptions, target placement |
| local directory | source copy and artifact digest | remote provenance claims |
| future enterprise registry | authenticated catalog metadata and artifact fetch | Loom registry mutation semantics unless the result is imported through Loom commands |

## `gh skill` Preview Surface

GitHub documents `gh skill` as preview and subject to change. The command group
currently includes:

| Command | Upstream role | Loom boundary |
|---------|---------------|---------------|
| `gh skill search` | discover public GitHub repositories containing skills | advisory candidate list only; normalize selected result to a Loom provider record |
| `gh skill preview` | render remote `SKILL.md` and browse related files without install | optional human review source; no registry write by itself |
| `gh skill install` | install a GitHub or local skill into host-specific agent directories | do not use as a Loom apply path; it bypasses registry, policy, bindings, and audit |
| `gh skill list` | scan installed skills across known host directories | advisory observation input only; registered Loom state remains authoritative |
| `gh skill update` | compare installed frontmatter tree SHA against upstream and update host directories | do not use for managed Loom projections; Loom updates must go through provenance refresh, plan/apply, and projection commands |
| `gh skill publish` | validate and publish skills through GitHub releases | upstream publishing path; it is not `loom skill release` and must not write Loom registry state |

Minimum detected version for any optional `gh skill` provider is `gh >= 2.90.0`.
An implementation must also check `gh skill --help` at runtime because preview
command shapes may change without notice. If the version or help probe fails,
the provider must degrade to direct Git support instead of blocking local path,
Git URL, or GitHub locator imports.

Sources checked:

1. GitHub CLI manual for `gh skill`: https://cli.github.com/manual/gh_skill
2. GitHub changelog announcing `gh skill` and `v2.90.0`: https://github.blog/changelog/2026-04-16-manage-agent-skills-with-github-cli/

## Provider Abstraction

Provider records should normalize every source into the same provenance fields
before Loom writes registry state:

```json
{
  "provider": "github",
  "locator": "github:owner/repo//skills/example",
  "requested_ref": "v1.2.0",
  "resolved_commit": "abc123...",
  "source_tree_hash": "def456...",
  "subdir": "skills/example",
  "artifact_digest": "sha256:...",
  "resolver": "loom/git",
  "resolver_version": "0.1.4",
  "resolved_at": "2026-06-26T00:00:00Z"
}
```

Provider-specific notes:

1. `local_path` records a local path locator and artifact digest. It must not
   claim a remote commit unless the source is also a Git repository and Loom
   records that as `git`.
2. `git` records requested ref, resolved commit, source tree hash, subdir, and
   artifact digest. This is the fallback for GitHub repos when `gh` is missing
   or preview behavior changes.
3. `github` is a locator convenience over Git. It resolves
   `github:owner/repo//subdir` to `https://github.com/owner/repo.git` and must
   not require `gh` authentication for public imports.
4. `gh_skill` may be added later as an optional resolver for discovery or
   preview metadata. It must emit the same normalized fields and may not install
   directly into agent directories.
5. `enterprise_registry` is reserved for future authenticated catalogs. It must
   produce immutable artifact identity before Loom imports the source.

## Provenance And Lockfile Contract

Provider output is not complete until Loom captures it in both
`state/registry/sources.json` and the deterministic root `loom.lock`.

Rules:

1. successful imports must record provider, locator, requested ref, resolved
   commit when Git-backed, source tree hash when Git-backed, source subdir,
   artifact digest, import time, and importer version
2. provider metadata injected by another tool may be preserved as advisory
   source metadata, but `loom.lock` remains Loom's authority for local control
   plane decisions
3. `skill provenance verify` compares canonical source bytes against both
   `sources.json` and `loom.lock`; provider claims alone do not prove integrity
4. `skill provenance refresh` is the only V1 write path that updates source
   provenance without changing projections or live target directories
5. future provider-specific metadata must be additive and deterministic so
   repeated imports or refreshes do not churn `loom.lock`

## Runtime Guardrails

1. Do not introduce a hard runtime dependency on `gh skill` for `skill add`,
   `use`, `plan use`, `apply`, `skill project`, or `skill eval`.
2. Do not call `gh skill install` from Loom apply flows because it writes
   directly into host directories outside Loom's target/binding/projection
   model.
3. Do not call `gh skill update` for managed projections. Use provenance
   refresh, review, plan/apply, and explicit projection updates instead.
4. Read-only provider probes must return structured warnings when unavailable,
   not silently downgrade source identity.
5. Any future provider write must first pass the provenance/lockfile contract
   and then reuse existing Loom registry write/audit semantics.

## Acceptance Mapping

This design closes issue #347 by making the following boundaries explicit:

1. Loom is a local control plane instead of a marketplace.
2. Current `gh skill` commands are listed with their upstream role and Loom
   boundary.
3. `gh >= 2.90.0` and `gh skill --help` probing are required before any
   optional `gh skill` integration.
4. direct Git remains the fallback implementation for environments without
   compatible `gh skill`.
5. provider output feeds the existing provenance and `loom.lock` design instead
   of bypassing it.
