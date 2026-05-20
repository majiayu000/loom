# Security Policy

## Supported Versions

Loom is early-stage software. Security fixes target the latest release and `main`.

## Reporting a Vulnerability

Please do not open a public issue for suspected vulnerabilities. Use GitHub private vulnerability reporting for this repository, or email the maintainer listed on the GitHub profile with enough detail to reproduce the issue.

Include:

- affected version or commit
- operating system and install method
- exact command or Panel route involved
- expected impact and any safe proof of concept

We will acknowledge reports as quickly as practical, triage severity, and publish a fix or advisory when needed.

## Dependency and Release Trust

- Rust dependencies are locked in `Cargo.lock`; Panel dependencies are locked in `panel/bun.lock` and `panel/package-lock.json`.
- Dependabot tracks Cargo, Panel npm, and GitHub Actions updates.
- Release archives are built by GitHub Actions from version tags, smoke-tested, checksummed in `SHA256SUMS`, and attested with GitHub artifact attestations.
- Users should prefer release archives or Homebrew over source installs when they need a guaranteed bundled Panel.

## Secrets

Loom must not commit API tokens, private keys, registry credentials, or generated secret material. Use environment variables or GitHub repository secrets for release automation.
