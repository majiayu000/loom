#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
trace_file="$(mktemp "${TMPDIR:-/tmp}/loom-next-action-trace.XXXXXX")"
expected_ids="$(mktemp "${TMPDIR:-/tmp}/loom-next-action-expected.XXXXXX")"
observed_ids="$(mktemp "${TMPDIR:-/tmp}/loom-next-action-observed.XXXXXX")"

cleanup() {
  rm -f "$trace_file" "$expected_ids" "$observed_ids"
}
trap cleanup EXIT

cd "$repo_root"
LOOM_TEST_NEXT_ACTION_TRACE="$trace_file" cargo nextest run --no-fail-fast

jq -e -s '
  length > 0 and
  all(.[]; (.emitter_id | type == "string") and has("payload"))
' "$trace_file" >/dev/null

awk '
  /^\[\[next_action_emitter\]\]$/ { emitters = 1; next }
  /^\[\[panel_mutation\]\]$/ { emitters = 0 }
  emitters && /^id = "/ {
    sub(/^id = "/, "")
    sub(/"$/, "")
    print
  }
' docs/agent-command-surfaces.toml | sort -u >"$expected_ids"

jq -r '.emitter_id' "$trace_file" | sort -u >"$observed_ids"
diff -u "$expected_ids" "$observed_ids"
LOOM_CONTRACT_TRACE_INPUT="$trace_file" LOOM_CONTRACT_TRACE_EXPECTED_EMITTERS=57 \
  cargo test --test agent_contract_surfaces emitter_trace_payloads_parse -- --exact
