# Releasing Loom

Loom is distributed as the `skillloom` crate with a `loom` binary.

## Release Surfaces

- GitHub Release: built from tags matching `v*.*.*`; each archive includes the `loom` binary plus README/LICENSE.
- crates.io: published when `CARGO_REGISTRY_TOKEN` is configured.
- Homebrew: opens a `loom` formula PR against `majiayu000/homebrew-tap` when `HOMEBREW_TAP_TOKEN` is configured.

GitHub Release archives are the preferred install path. They are built with the Panel frontend bundled into the Rust binary and smoke-tested before upload. The release workflow publishes:

- `skillloom-<version>-aarch64-apple-darwin.tar.gz`
- `skillloom-<version>-x86_64-apple-darwin.tar.gz`
- `skillloom-<version>-x86_64-unknown-linux-gnu.tar.gz`
- `SHA256SUMS`

The release workflow also creates GitHub artifact attestations for the `.tar.gz` archives. The Homebrew formula installs the `loom` binary from GitHub Release archives. The crate name remains `skillloom` because `loom` is already used by an unrelated crates.io package.

## One-Time Setup

Configure repository secrets:

- `CARGO_REGISTRY_TOKEN`: crates.io token allowed to publish `skillloom`.
- `HOMEBREW_TAP_TOKEN`: GitHub token allowed to push branches and open PRs in `majiayu000/homebrew-tap`.

## Release Steps

1. Update `Cargo.toml` version.
2. Run local verification:

   ```bash
   cd panel && bun install --frozen-lockfile
   cd panel && bun run typecheck
   cd panel && bun run test
   cd panel && bun run build
   make fmt-check
   make lint
   make test
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

After the release is published:

```bash
tag=vX.Y.Z
version="${tag#v}"
target=aarch64-apple-darwin
archive="skillloom-${version}-${target}.tar.gz"

gh release download "$tag" --repo majiayu000/loom --pattern "$archive" --pattern SHA256SUMS
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify "$archive" --repo majiayu000/loom

tmp="$(mktemp -d)"
tar -C "$tmp" -xzf "$archive"
bin="$tmp/skillloom-${version}-${target}/loom"
"$bin" --version
"$bin" --help >/dev/null
root="$(mktemp -d)"
"$bin" --json --root "$root" workspace status >/dev/null
"$bin" --root "$root" panel --port 0 >/tmp/loom-panel-smoke.log 2>&1 &
panel_pid="$!"
sleep 1
kill -0 "$panel_pid"
kill "$panel_pid"
wait "$panel_pid" 2>/dev/null || true
```

After the Homebrew PR is merged:

```bash
brew install majiayu000/tap/loom
loom --help
loom --version
```

`cargo install skillloom` remains supported as a source-build path, but it is not the recommended first install path for users who want a guaranteed bundled Panel. Source builds that bundle Panel assets require the Panel frontend inputs and Bun during compile time; run `cd panel && bun install --frozen-lockfile` before building from source when the bundled Panel is required. Otherwise the CLI can build without embedded Panel assets.

## Future Install Surfaces

Only add `cargo-binstall` metadata after a tagged GitHub Release has been downloaded and verified with the archive layout above. The metadata must point at `skillloom-<version>-<target>.tar.gz` and install the nested `loom` binary from `skillloom-<version>-<target>/loom`.
