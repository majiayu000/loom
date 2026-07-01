#!/usr/bin/env bash
set -euo pipefail

bin="${1:-target/release/loom}"
if [[ ! -x "$bin" ]]; then
  cargo build --release --locked
fi

# Hard ceiling: 3840 KiB. The durable plan/apply protocol, offline eval
# matrix, local skill scaffolding CLI, skillset foundation, portable YAML
# lint parser, single-skill inspect read model, and single-skill activation
# commands expanded the accepted V2 budget. Codex visibility repair adds a
# structured TOML parser/editor so `--fix-config` can patch config atomically
# without ad hoc string deletion, while the startup latency checks below
# continue to guard cold CLI responsiveness.
max_bin_bytes=$((3840 * 1024))
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
if not dist.is_dir():
    raise SystemExit("panel/dist is missing; run `make panel-build` before perf-smoke")

total = 0
for path in dist.rglob("*"):
    rel = path.relative_to(dist).as_posix()
    if not path.is_file():
        continue
    if rel == "index.html" or rel.endswith(".css") or rel.startswith("assets/base-") or rel.startswith("assets/panel-"):
        total += len(gzip.compress(path.read_bytes(), compresslevel=9))
# Soft target: 100 KiB. Hard ceiling: 104 KiB (~4% buffer for chunk-
# split jitter after #169 React 19 upgrade landed at ~100.06 KiB on main).
limit = 104 * 1024
soft = 100 * 1024
if total > limit:
    raise SystemExit(f"panel gzip payload is {total} bytes; limit is {limit}")
if total > soft:
    print(f"panel gzip payload={total} bytes (over {soft}-byte soft target, under {limit}-byte ceiling)")
else:
    print(f"panel gzip payload={total} bytes")
PY
