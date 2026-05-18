#!/usr/bin/env bash
set -euo pipefail

bin="${1:-target/release/loom}"
if [[ ! -x "$bin" ]]; then
  cargo build --release --locked
fi

max_bin_bytes=$((3 * 1024 * 1024))
bin_bytes="$(wc -c < "$bin" | tr -d ' ')"
if (( bin_bytes > max_bin_bytes )); then
  echo "release binary is ${bin_bytes} bytes; limit is ${max_bin_bytes}" >&2
  exit 1
fi

python3 - "$bin" <<'PY'
import gzip
import math
import pathlib
import subprocess
import sys
import time

bin_path = sys.argv[1]

def measure(args, limit_ms):
    subprocess.run(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    samples = []
    for _ in range(20):
        start = time.perf_counter()
        subprocess.run(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        samples.append((time.perf_counter() - start) * 1000)
    samples.sort()
    p95 = samples[math.ceil(len(samples) * 0.95) - 1]
    if p95 > limit_ms:
        raise SystemExit(f"{' '.join(args)} p95={p95:.1f}ms exceeds {limit_ms}ms")
    print(f"{' '.join(args)} p95={p95:.1f}ms")

measure([bin_path, "--version"], 300)
measure([bin_path, "--help"], 300)

dist = pathlib.Path("panel/dist")
if dist.exists():
    total = 0
    for path in dist.rglob("*"):
        if path.is_file() and path.suffix in {".css", ".html", ".js"}:
            total += len(gzip.compress(path.read_bytes(), compresslevel=9))
    limit = 100 * 1024
    if total > limit:
        raise SystemExit(f"panel gzip payload is {total} bytes; limit is {limit}")
    print(f"panel gzip payload={total} bytes")
PY
