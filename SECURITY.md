# Security Policy

## Supported Versions

Security fixes are released from the latest `main` branch and the latest
published `vX.Y.Z` release. Older releases may receive a fix only when the
maintainer can reproduce the issue and the patch applies cleanly without
changing public behavior.

## Reporting a Vulnerability

Use GitHub's private vulnerability reporting flow for this repository when it is
available. If the private report button is unavailable, open a public issue that
only says you have a vulnerability to report and asks for a private contact
path. Do not include exploit details, secrets, tokens, private paths, or
proof-of-concept payloads in a public issue.

Please include:

- The affected Loom version or commit.
- The operating system and install path used.
- A minimal reproduction that avoids real secrets or private registry content.
- The impact you observed or expect.

The maintainer will acknowledge valid reports as time permits, coordinate a fix
on a private branch when needed, and publish release notes after the fix is
available.

## Dependency and Release Security

- Dependency updates are reviewed through normal pull requests and must pass CI.
- Release archives are built by GitHub Actions from version tags matching
  `v*.*.*`.
- Every release archive is hashed into `SHA256SUMS`.
- Release archives and `SHA256SUMS` receive GitHub artifact attestations.
- Prebuilt archives are smoke-tested with `loom --version`, `loom --help`,
  `loom --json --root "$(mktemp -d)" workspace status`, and
  `loom --root "$(mktemp -d)" panel --port 0` before publication.

Users should verify downloaded archives with:

```bash
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify "<archive>.tar.gz" --repo majiayu000/loom
```
