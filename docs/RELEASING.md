# Releasing Loom

Loom is distributed as the `skillloom` crate with a `loom` binary.

## Release Surfaces

- GitHub Release: built from tags matching `v*.*.*`.
- crates.io: published when `CARGO_REGISTRY_TOKEN` is configured.
- Homebrew: opens a `loom` formula PR against `majiayu000/homebrew-tap` when `HOMEBREW_TAP_TOKEN` is configured.

The Homebrew formula installs the `loom` binary from GitHub Release archives. The crate name remains `skillloom` because `loom` is already used by an unrelated crates.io package.

## One-Time Setup

Configure repository secrets:

- `CARGO_REGISTRY_TOKEN`: crates.io token allowed to publish `skillloom`.
- `HOMEBREW_TAP_TOKEN`: GitHub token allowed to push branches and open PRs in `majiayu000/homebrew-tap`.

## Release Steps

1. Update `Cargo.toml` version.
2. Run local verification:

   ```bash
   make fmt-check
   make lint
   make test
   make panel-build
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

After the release is published:

```bash
cargo install skillloom
loom --help
```

After the Homebrew PR is merged:

```bash
brew install majiayu000/tap/loom
loom --help
```
