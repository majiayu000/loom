# Security Policy

This document describes Loom's threat model, the boundaries Loom enforces, and
how to report vulnerabilities.

## Supported Versions

Security fixes target the latest `main` branch. Released versions on
crates.io may receive backports at the maintainer's discretion; older minor
versions are not guaranteed to receive patches.

## Trust Model

Loom is a local-first tool. The default trust boundaries are:

| Surface | Trust assumption |
|---|---|
| The user running `loom` | Fully trusted; can read and modify the registry. |
| Files under `--root` (default `~/.loom-registry`) | Trusted as written by Loom or the user. |
| Files under registered target paths (`~/.claude/skills`, `~/.codex/skills`, …) | Trusted to be readable by Loom; writes only happen when ownership is `managed`. |
| Other processes on the host | Treated as untrusted; Loom does not defend against a co-resident attacker with write access to the registry. |
| Remote registries pulled via `sync` | Trusted only after manual review of the upstream commit and remote URL. |

Loom does not currently sign commits or verify upstream commit signatures.
See [Roadmap](#roadmap) for future work.

## Enforcement Surfaces

Loom enforces the following invariants today:

### Hard write guard
The CLI refuses to write to a `--root` directory that matches the Loom tool
repository checkout (see `ensure_write_repo_ready`). This prevents accidental
writes that would mutate the development copy of Loom itself.

### Ownership tiers
Every registered target carries an explicit ownership tier:

- `managed` — Loom is the only writer.
- `observed` — Loom only reads; capture flows pull edits back into the registry.
- `external` — Loom does not touch the directory.

Skill projections under `observed` and `external` targets are guaranteed not
to be overwritten by `skill project` operations.

### Read / write command split
Read commands (`workspace status`, `workspace doctor`, `target list`,
`target show`, `sync status`, `skill verify`) never append to the audit log
or the registry operation history. Write commands write durable audit events
through the registry operations pipeline.

### Audit log
Write commands record a `RegistryOperationRecord` under
`state/registry/ops/operations.jsonl` with the operation intent, payload,
effects, and timestamps. The history branch (`refs/heads/loom-history`)
mirrors these events through git, which gives every audited mutation a
verifiable commit ancestor.

### Skill source integrity check
`loom skill verify <name>` compares the working tree under
`skills/<name>` against the last `loom skill save` commit and reports any
modified, staged, or untracked files. It is the local integrity primitive
for detecting edits that bypassed `skill save`. The check is read-only and
does not modify state.

## Limitations Known At This Time

- Loom does not cryptographically sign commits. An attacker who can write to
  the registry directly can rewrite history.
- Loom does not validate skill contents (no script sandboxing, no static
  analysis). Projecting a skill into an agent directory makes its contents
  available to that agent verbatim.
- Loom does not verify SSH or HTTPS endpoints beyond what the configured
  git client validates. Skills pulled via `sync pull` inherit the trust
  level of the upstream registry.
- Projection methods `symlink` and `materialize` follow symlinks during
  capture. A malicious agent directory could in principle redirect a
  capture into a different filesystem location. Restrict the `--root`
  parent directory's ACLs accordingly.

## Roadmap

Tracked, but not yet implemented:

- Optional Ed25519 signing of `skill save` / `skill release` commits, with
  a per-registry trusted-keys file.
- `skill verify --at <ref>` to compare the working tree against an
  arbitrary historical revision rather than the most recent save commit.
- Cross-registry signature verification during `sync pull` and `replay`.

## Reporting a Vulnerability

Report security issues privately:

1. Open a [GitHub Security Advisory](https://github.com/majiayu000/loom/security/advisories/new)
   on this repository (preferred — encrypted, allows coordinated disclosure).
2. If you cannot use Security Advisories, email the maintainer listed in
   `Cargo.toml` with a description of the issue and a minimal reproduction.

Please **do not** open public GitHub issues for security reports.

Expected response times:

- Acknowledgement: within 7 days.
- Initial assessment: within 14 days.
- Fix or detailed remediation plan: within 30 days for issues classified as
  high or critical; longer for lower-severity issues.

Coordinated disclosure is preferred; the maintainer will work with reporters
to align on a disclosure timeline.
