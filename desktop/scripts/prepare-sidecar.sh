#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
target=${1:-$(rustc -vV | sed -n 's/^host: //p')}
cargo build --manifest-path "$repo/Cargo.toml" --locked --release --bin loom --target "$target" --target-dir "$repo/target"
suffix=
case "$target" in *windows*) suffix=.exe;; esac
mkdir -p "$repo/desktop/binaries"
cp "$repo/target/$target/release/loom$suffix" "$repo/desktop/binaries/loom-$target$suffix"
