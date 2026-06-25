# Security Policy

This document describes Loom's threat model, the boundaries Loom enforces, and
how to report vulnerabilities.

## Supported Versions

Loom is early-stage software. Security fixes target the latest release and
`main`. Released versions on crates.io may receive backports at the
maintainer's discretion; older minor versions are not guaranteed to receive
patches.

## Reporting a Vulnerability

Please do not open a public issue for suspected vulnerabilities. Use GitHub
private vulnerability reporting for this repository, or email the maintainer
listed on the GitHub profile with enough detail to reproduce the issue.

Include:

- affected version or commit
- operating system and install method
- exact command or Panel route involved
- expected impact and any safe proof of concept

We will acknowledge reports as quickly as practical, triage severity, and
publish a fix or advisory when needed.

## Trust Model

Loom is a local-first tool. The default trust boundaries are:

| Surface | Trust assumption |
|---|---|
| The user running `loom` | Fully trusted; can read and modify the registry. |
| Files under `--root` (default `~/.loom-registry`) | Trusted as written by Loom or the user. |
| Files under registered target paths (`~/.claude/skills`, `~/.codex/skills`, etc.) | Trusted to be readable by Loom; writes only happen when ownership is `managed`. |
| Other processes on the host | Treated as untrusted; Loom does not defend against a co-resident attacker with write access to the registry. |
| Remote registries pulled via `sync` | Trusted only after manual review of the upstream commit and remote URL. |

Loom does not currently sign commits or verify upstream commit signatures.

## Enforcement Surfaces

### Hard write guard

The CLI refuses to write to a `--root` directory that matches the Loom tool
repository checkout. This prevents accidental writes that would mutate the
development copy of Loom itself.

### Ownership tiers

Every registered target carries an explicit ownership tier:

- `managed` - Loom is allowed to write.
- `observed` - Loom reads and imports; projection writes are blocked.
- `external` - Loom does not touch the directory.

Skill projections under `observed` and `external` targets are not overwritten
by `skill project` operations.

### Read / write command split

Read commands such as `workspace status`, `workspace doctor`, `target list`,
`target show`, `sync status`, and `skill verify` do not mutate registry state,
Git refs, the Git index, live target directories, or the pending queue. They
may write durable command audit events under `state/events/commands.jsonl`.

State-changing registry commands record a `RegistryOperationRecord` under
`state/registry/ops/operations.jsonl` with the operation intent, payload,
effects, and timestamps. The history branch (`refs/heads/loom-history`)
mirrors these events through Git, which gives audited registry mutations a
verifiable commit ancestor.

### Skill source integrity check

`loom skill verify <name>` compares `skills/<name>` against the committed
source tree and reports modified, staged, or untracked files. It is the local
integrity primitive for detecting source edits that bypassed `skill save`.
The check is read-only aside from command audit events.

### Skill policy check

`loom skill policy <name>` reports declared skill capabilities, source-file
risk signals, and provenance digest drift before projection. It detects
signals such as scripts, executable files, binary-looking content, very large
files, generated artifact directories, declared shell/network/secret
capabilities, and prompt-injection heuristics.

Projection evaluates the binding's `policy_profile` before touching the live
target directory. Built-in `audit-only` and `safe-capture` profiles report
findings without blocking. Built-in `deny-risky` and `strict` profiles block
high-risk findings with `POLICY_BLOCKED`. Unknown policy profile names are
accepted as organization-defined hooks but only produce a warning until local
policy handling is implemented.

These checks are not a sandbox and do not prove a skill is safe. Prompt
injection scanning is heuristic and should be treated as a review signal only.

### Skill evals

`loom skill eval <name>` runs offline fixtures under `skills/<name>/evals/`
and reports trigger quality, task checks, efficiency metrics, permissions used,
and artifact checks. Eval success is quality evidence for regression tracking,
not a sandbox, malware verdict, or guarantee that a skill is safe to project.
Rubric or LLM graders must be explicit in fixtures and reproducible enough for
review before their scores are trusted.

## Dependency and Release Trust

- Rust dependencies are locked in `Cargo.lock`; Panel dependencies are locked
  in `panel/bun.lock` and `panel/package-lock.json`.
- Dependabot tracks Cargo, Panel npm, and GitHub Actions updates.
- Release archives are built by GitHub Actions from version tags, smoke-tested,
  checksummed in `SHA256SUMS`, and attested with GitHub artifact attestations.
- Users should prefer release archives or Homebrew over source installs when
  they need a guaranteed bundled Panel.

## Limitations Known At This Time

- Loom does not cryptographically sign commits. An attacker who can write to
  the registry directly can rewrite history.
- Loom policy checks report heuristic risk signals, but they do not execute
  code in a sandbox and do not prove skill contents are benign. Projecting a
  skill into an agent directory still makes its contents available to that
  agent.
- Loom does not verify SSH or HTTPS endpoints beyond what the configured Git
  client validates. Skills pulled via `sync pull` inherit the trust level of
  the upstream registry.
- Projection methods that read live target content can be affected by malicious
  symlinks in directories the user has chosen to trust. Keep registry and
  target directory permissions tight.

## Roadmap

- Optional signing for `skill save` and `skill release` commits.
- `skill verify --at <ref>` to compare the working tree against an arbitrary
  historical revision.
- Cross-registry signature verification during `sync pull` and `replay`.

## Secrets

Loom must not commit API tokens, private keys, registry credentials, or
generated secret material. Use environment variables or GitHub repository
secrets for release automation.
