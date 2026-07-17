#!/usr/bin/env bash
set -euo pipefail

target=""
args=("$@")
for ((index = 0; index < ${#args[@]}; index++)); do
  if [[ "${args[$index]}" == "--target" ]] && ((index + 1 < ${#args[@]})); then
    target="${args[$((index + 1))]}"
  elif [[ "${args[$index]}" == --target=* ]]; then
    target="${args[$index]#--target=}"
  fi
done

host_linux_x86=false
if [[ -z "$target" && "$(uname -s)" == "Linux" && "$(uname -m)" == "x86_64" ]]; then
  host_linux_x86=true
fi

if [[ "$target" == "x86_64-unknown-linux-gnu" || "$host_linux_x86" == true ]]; then
  command -v clang >/dev/null || {
    echo "x86_64 Linux release builds require clang" >&2
    exit 1
  }
  command -v ld.lld >/dev/null || {
    echo "x86_64 Linux release builds require lld" >&2
    exit 1
  }
  export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C linker=clang -C link-arg=-fuse-ld=lld -C link-arg=-Wl,--icf=safe"
fi

if ((${#args[@]})); then
  cargo build --release --locked "${args[@]}"
else
  cargo build --release --locked
fi
