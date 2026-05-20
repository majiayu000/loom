# Releasing Loom

Loom is distributed as the `skillloom` crate with a `loom` binary.

## Release Surfaces

- GitHub Release: built from tags matching `v*.*.*`; archives include bundled
  Panel assets, SHA256SUMS, and GitHub artifact attestations.
- crates.io: published when `CARGO_REGISTRY_TOKEN` is configured.
- Homebrew: opens a `loom` formula PR against `majiayu000/homebrew-tap` when `HOMEBREW_TAP_TOKEN` is configured.

The Homebrew formula installs the `loom` binary from GitHub Release archives. The crate name remains `skillloom` because `loom` is already used by an unrelated crates.io package.

## One-Time Setup

Configure repository secrets:

- `CARGO_REGISTRY_TOKEN`: crates.io token allowed to publish `skillloom`.
- `HOMEBREW_TAP_TOKEN`: GitHub token allowed to push branches and open PRs in `majiayu000/homebrew-tap`.

No extra secret is required for release attestations. The release workflow grants
the GitHub token `id-token`, `attestations`, and `artifact-metadata` write
permissions so `actions/attest` can publish provenance for archives and
SHA256SUMS.

## Release Steps

1. Update `Cargo.toml` version.
2. Run local verification:

   ```bash
   make fmt-check
   make lint
   make test
   make panel-typecheck
   make panel-test
   make panel-build
   make e2e
   cargo publish --dry-run --locked
   ```

3. Commit the version bump.
4. Tag and push:

   ```bash
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin main --tags
   ```

5. Watch the `Release` workflow.
6. Merge the Homebrew tap PR if the workflow opens one.

## Install Checks

After the release is published, verify the prebuilt archive first:

```bash
version=X.Y.Z
target=aarch64-apple-darwin
archive="skillloom-${version}-${target}.tar.gz"

curl -LO "https://github.com/majiayu000/loom/releases/download/v${version}/${archive}"
curl -LO "https://github.com/majiayu000/loom/releases/download/v${version}/SHA256SUMS"
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify "${archive}" --repo majiayu000/loom
tar -xzf "${archive}"
"skillloom-${version}-${target}/loom" --version
"skillloom-${version}-${target}/loom" --help
"skillloom-${version}-${target}/loom" --json --root "$(mktemp -d)" workspace status
"skillloom-${version}-${target}/loom" --root "$(mktemp -d)" panel --port 0
```

Then verify package-manager paths:

```bash
cargo binstall skillloom
cargo install skillloom
loom --help
loom --version
```

After the Homebrew PR is merged:

```bash
brew install majiayu000/tap/loom
loom --help
loom --version
```
