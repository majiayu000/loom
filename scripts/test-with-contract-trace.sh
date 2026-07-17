#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
trace_file="$(mktemp "${TMPDIR:-/tmp}/loom-next-action-trace.XXXXXX")"

cleanup() {
  rm -f "$trace_file"
}
trap cleanup EXIT

cd "$repo_root"
LOOM_TEST_NEXT_ACTION_TRACE="$trace_file" cargo nextest run --no-fail-fast

jq -e -s '
  length > 0 and
  all(.[].emitter_id; type == "string") and
  all(.[].fixture_id; type == "string") and
  all(.[].payload_type; type == "string") and
  all(.[]; has("payload"))
' "$trace_file" >/dev/null

LOOM_CONTRACT_TRACE_INPUT="$trace_file" LOOM_CONTRACT_TRACE_EXPECTED_EMITTERS=57 \
  cargo test --test agent_contract_surfaces emitter_trace_payloads_parse -- --exact
