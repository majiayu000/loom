#!/usr/bin/env bash
set -euo pipefail

bin="${1:-target/release/loom}"
if [[ ! -x "$bin" ]]; then
  cargo build --release --locked
fi

# Hard ceiling: 5742 KiB. The durable plan/apply protocol, offline eval
# matrix, local skill scaffolding CLI, skillset foundation, portable YAML
# lint parser, single-skill inspect read model, single-skill activation
# commands, and safety/trust/quarantine/security-diff command surfaces expanded
# the accepted V2 budget. Runtime dependency readiness adds read-only
# declaration parsing and environment probes. Codex visibility repair adds a
# structured TOML parser/editor so `--fix-config` can patch config atomically
# without ad hoc string deletion. The real agent eval harness adds explicit
# run/trigger/compare planning and reporting surfaces. Skill preflight adds
# consolidated gate reporting, baseline evidence comparison, security-diff
# gating, and safe ref materialization for regression checks. Adapter
# visibility metadata adds versioned external adapter loading plus discovery
# root, visibility, reload, and adapter-driven target-selection read models,
# and the local recommendation foundation adds deterministic index,
# skill/skillset recommendation, semantic-disabled fallback, active dry-run
# planning, and recommendation safety precheck surfaces. Workflow DAG planning
# adds definition storage, topological validation, guarded plan/preflight
# checks, and deferred execution surfaces. Provider/catalog dry-run install
# planning adds safe provider persistence, locator parsing, local preview, and
# trust/provenance planning surfaces. Instruction surface inventory adds
# read-only scan/show/classify/doctor/migrate-plan commands for native
# instruction files. Org governance primitives add policy/approval/RBAC command
# surfaces and Git-backed state evaluation. Package export foundation adds
# plan/build/verify command surfaces plus deterministic archive, manifest,
# checksum, source freshness, and content-safety verification. MCP
# provisioning foundation adds read-only requirement/plan/doctor/catalog
# surfaces with source policy and redacted config planning. MCP review-thread
# fixes add structured config compatibility checks, source override trust
# handling, table-only doctor readiness, and nested/scalar metadata parsing.
# Devcontainer provisioning adds read-only plan/doctor surfaces, deterministic
# file previews, target policy gates, redacted registry clone planning, and
# dependency/secret readiness models. Provisioning review-gate hardening adds
# component-aware workspace matching, SSH/query credential redaction, reviewed
# registry-head checkout, remote secret requirements, and safety/trust policy
# findings. Tar provisioning artifacts add deterministic archive emission,
# manifest/checksum validation, source copy inspection, and active-view import
# dry-run verification. Reviewed provisioning apply gates add idempotency-key
# replay, lock-based key claiming, target preimage and symlink/non-file
# inspection, and write-boundary revalidation. The compiled runtime foundation
# adds read-only compile planning, source-digest and manifest models, sidecar
# verification, and artifact gate reporting, while the startup latency checks
# below continue to guard cold CLI responsiveness. The local telemetry
# foundation adds typed redacted event models, report/export/purge read models,
# and opt-in event writes for eval, safety, and activation paths. Assisted
# authoring adds guarded patch artifact generation and deferred apply-patch
# command surfaces with prompt redaction and source digest metadata. Reviewed
# authoring apply gates add idempotency replay, staging validation, source
# digest rechecks, contextual hunk application, preimage/index rollback, and
# Git-backed materialization. Compiled activation gates add artifact lookup and
# verification plumbing, and compiled projection adds validated artifact
# materialization with projection metadata. Eval-backed compiled promotion adds
# offline eval evidence recording and freshness validation for valid artifacts.
# User-scope `loom use` adds adapter-root target resolution, adoption safety,
# observed-target upgrade auditing, and exact target-root command plumbing.
# Lifecycle convergence adds `skill commit` direction detection, release anchors,
# and diagnose drift routing while deleting the old public lifecycle leaves. The
# core service layer adds command metadata classification plus typed
# projection/lifecycle service entrypoints for Panel mutations. Registry domain
# enums add shared serde/CLI/TS vocab exports for typed persisted state.
# Registry-backed sync/ops queue handling adds operation ack/purge/failure
# mutation paths plus guarded Git merge/rebase handling for operation journals.
# Skillset lifecycle closeout adds grouped activation/deactivation orchestration,
# member eval aggregation, release/rollback guards, and partial rollback
# recovery. Projection digest observation persistence adds registry observation
# events, status summaries, and diagnose write-back plumbing. Rollback
# projection reconciliation adds structured live projection recovery plans,
# registry snapshot failure reporting, and compiled activation recovery guards.
# Provider provenance outdated adds read-only stale pin reporting and
# review-only re-pin plan output. Skill inspect evidence surfacing adds
# read-only eval report discovery, dirty-source staleness checks, and
# policy/safety summary plumbing. Recommendation evidence hardening adds
# agent-scoped eval scoring, invalid-id guards, negative-delta penalties, and
# dependency-aware resolve/skillset ranking while keeping cold CLI startup
# guarded below.
max_bin_bytes=$((5742 * 1024))
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
