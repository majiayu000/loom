#!/usr/bin/env bash
set -euo pipefail

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

ROOT_BASE="${1:-/tmp/loom-agent-e2e-$(uuidgen)}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOOM_BIN="${LOOM_BIN:-$REPO_ROOT/target/debug/loom}"

if [[ ! -x "$LOOM_BIN" ]]; then
  (cd "$REPO_ROOT" && cargo build -q)
fi

if [[ ! -x "$LOOM_BIN" ]]; then
  echo "loom binary not found: $LOOM_BIN" >&2
  exit 1
fi

json_field() {
  local body="$1"
  local expr="$2"
  printf '%s' "$body" | jq -r "$expr"
}

assert_ok() {
  local body="$1"
  local label="$2"
  local ok
  ok="$(json_field "$body" '.ok')"
  if [[ "$ok" != "true" ]]; then
    echo "FAILED [$label]: $(printf '%s' "$body" | jq -c '.error')" >&2
    exit 1
  fi
}

run_json() {
  local repo="$1"
  shift
  "$LOOM_BIN" --json --root "$repo" "$@"
}

run_json_expect_fail() {
  local repo="$1"
  local expected_code="$2"
  shift 2
  local out rc code
  set +e
  out="$("$LOOM_BIN" --json --root "$repo" "$@" 2>/dev/null)"
  rc=$?
  set -e
  if [[ $rc -eq 0 ]]; then
    echo "FAILED [expected failure]: command unexpectedly succeeded: $*" >&2
    exit 1
  fi
  code="$(json_field "$out" '.error.code')"
  if [[ "$code" != "$expected_code" ]]; then
    echo "FAILED [expected code $expected_code got $code]: $*" >&2
    echo "$out" >&2
    exit 1
  fi
  printf '%s' "$out"
}

scenario_a() {
  local root="$ROOT_BASE/a"
  local repo="$root/repo"
  local seed="$root/seed/demo-skill-a"
  local target_dir="$root/agent/.claude/skills"
  local workspace="$root/workspace-a"
  local add_json target_json bind_json proj_json cap_json status_json
  local target_id bind_id instance_id materialized_path marker
  marker="live edit from scenario A"

  mkdir -p "$seed" "$target_dir" "$workspace"
  cat >"$seed/SKILL.md" <<'EOF'
# demo-skill-a
seed line A
EOF

  add_json="$(run_json "$repo" skill add "$seed" --name demo-skill-a)"
  assert_ok "$add_json" "A skill add"

  target_json="$(run_json "$repo" target add --agent claude --path "$target_dir" --ownership managed)"
  assert_ok "$target_json" "A target add"
  target_id="$(json_field "$target_json" '.data.target.target_id')"

  bind_json="$(run_json "$repo" workspace binding add --agent claude --profile workspace-a --matcher-kind path-prefix --matcher-value "$workspace" --target "$target_id")"
  assert_ok "$bind_json" "A binding add"
  bind_id="$(json_field "$bind_json" '.data.binding.binding_id')"

  proj_json="$(run_json "$repo" skill project demo-skill-a --binding "$bind_id" --method symlink)"
  assert_ok "$proj_json" "A project"
  instance_id="$(json_field "$proj_json" '.data.projection.instance_id')"
  materialized_path="$(json_field "$proj_json" '.data.projection.materialized_path')"

  printf '\n%s\n' "$marker" >> "$materialized_path/SKILL.md"

  cap_json="$(run_json "$repo" skill capture demo-skill-a --instance "$instance_id" --message "e2e capture A")"
  assert_ok "$cap_json" "A capture"
  grep -q "$marker" "$repo/skills/demo-skill-a/SKILL.md"

  status_json="$(run_json "$repo" workspace status)"
  assert_ok "$status_json" "A status"

  printf 'A PASS target=%s binding=%s instance=%s\n' "$target_id" "$bind_id" "$instance_id"
}

scenario_b() {
  local root="$ROOT_BASE/b"
  local repo="$root/repo"
  local seed="$root/seed/demo-skill-b"
  local target_dir="$root/agent/.claude-work/skills"
  local workspace="$root/workspace-b"
  local target_json bind_json proj_json cap_json
  local target_id bind_id instance_id materialized_path marker
  marker="live edit from scenario B"

  mkdir -p "$seed" "$target_dir" "$workspace"
  cat >"$seed/SKILL.md" <<'EOF'
# demo-skill-b
seed line B
EOF

  assert_ok "$(run_json "$repo" skill add "$seed" --name demo-skill-b)" "B skill add"
  target_json="$(run_json "$repo" target add --agent claude --path "$target_dir" --ownership managed)"
  assert_ok "$target_json" "B target add"
  target_id="$(json_field "$target_json" '.data.target.target_id')"

  bind_json="$(run_json "$repo" workspace binding add --agent claude --profile workspace-b --matcher-kind path-prefix --matcher-value "$workspace" --target "$target_id")"
  assert_ok "$bind_json" "B binding add"
  bind_id="$(json_field "$bind_json" '.data.binding.binding_id')"

  proj_json="$(run_json "$repo" skill project demo-skill-b --binding "$bind_id" --method copy)"
  assert_ok "$proj_json" "B project"
  instance_id="$(json_field "$proj_json" '.data.projection.instance_id')"
  materialized_path="$(json_field "$proj_json" '.data.projection.materialized_path')"

  printf '\n%s\n' "$marker" >> "$materialized_path/SKILL.md"
  cap_json="$(run_json "$repo" skill capture demo-skill-b --instance "$instance_id" --message "e2e capture B")"
  assert_ok "$cap_json" "B capture"
  grep -q "$marker" "$repo/skills/demo-skill-b/SKILL.md"

  assert_ok "$(run_json "$repo" target show "$target_id")" "B target show"
  assert_ok "$(run_json "$repo" workspace binding show "$bind_id")" "B binding show"

  printf 'B PASS target=%s binding=%s instance=%s\n' "$target_id" "$bind_id" "$instance_id"
}

scenario_c() {
  local root="$ROOT_BASE/c"
  local repo="$root/repo"
  local seed="$root/seed/demo-skill-c"
  local target_dir_1="$root/agent/.claude/skills"
  local target_dir_2="$root/agent/.claude-work/skills"
  local workspace="$root/workspace-c"
  local target_json_1 target_json_2 bind_json proj_json_1 proj_json_2
  local target_id_1 target_id_2 bind_id instance_id_2 materialized_path_2 marker
  marker="live edit from scenario C"

  mkdir -p "$seed" "$target_dir_1" "$target_dir_2" "$workspace"
  cat >"$seed/SKILL.md" <<'EOF'
# demo-skill-c
seed line C
EOF

  assert_ok "$(run_json "$repo" skill add "$seed" --name demo-skill-c)" "C skill add"
  target_json_1="$(run_json "$repo" target add --agent claude --path "$target_dir_1" --ownership managed)"
  assert_ok "$target_json_1" "C target add #1"
  target_id_1="$(json_field "$target_json_1" '.data.target.target_id')"

  target_json_2="$(run_json "$repo" target add --agent claude --path "$target_dir_2" --ownership managed)"
  assert_ok "$target_json_2" "C target add #2"
  target_id_2="$(json_field "$target_json_2" '.data.target.target_id')"

  bind_json="$(run_json "$repo" workspace binding add --agent claude --profile workspace-c --matcher-kind path-prefix --matcher-value "$workspace" --target "$target_id_1")"
  assert_ok "$bind_json" "C binding add"
  bind_id="$(json_field "$bind_json" '.data.binding.binding_id')"

  proj_json_1="$(run_json "$repo" skill project demo-skill-c --binding "$bind_id" --method symlink)"
  assert_ok "$proj_json_1" "C project default target"
  proj_json_2="$(run_json "$repo" skill project demo-skill-c --binding "$bind_id" --target "$target_id_2" --method copy)"
  assert_ok "$proj_json_2" "C project explicit target"
  instance_id_2="$(json_field "$proj_json_2" '.data.projection.instance_id')"
  materialized_path_2="$(json_field "$proj_json_2" '.data.projection.materialized_path')"

  printf '\n%s\n' "$marker" >> "$materialized_path_2/SKILL.md"
  assert_ok "$(run_json "$repo" skill capture demo-skill-c --instance "$instance_id_2" --message "e2e capture C")" "C capture"
  grep -q "$marker" "$repo/skills/demo-skill-c/SKILL.md"

  assert_ok "$(run_json "$repo" target show "$target_id_1")" "C target show #1"
  assert_ok "$(run_json "$repo" target show "$target_id_2")" "C target show #2"

  printf 'C PASS target1=%s target2=%s binding=%s instance2=%s\n' "$target_id_1" "$target_id_2" "$bind_id" "$instance_id_2"
}

scenario_d() {
  local root="$ROOT_BASE/d"
  local repo="$root/repo"
  local seed="$root/seed/demo-skill-d"
  local target_dir="$root/agent/.codex/skills"
  local workspace="$root/workspace-d"
  local target_json bind_json proj_json
  local target_id bind_id instance_id materialized_path marker
  local fail_bind_json fail_remove_json
  marker="live edit from scenario D"

  mkdir -p "$seed" "$target_dir" "$workspace"
  cat >"$seed/SKILL.md" <<'EOF'
# demo-skill-d
seed line D
EOF

  assert_ok "$(run_json "$repo" skill add "$seed" --name demo-skill-d)" "D skill add"
  target_json="$(run_json "$repo" target add --agent codex --path "$target_dir" --ownership managed)"
  assert_ok "$target_json" "D target add"
  target_id="$(json_field "$target_json" '.data.target.target_id')"

  bind_json="$(run_json "$repo" workspace binding add --agent codex --profile workspace-d --matcher-kind path-prefix --matcher-value "$workspace" --target "$target_id")"
  assert_ok "$bind_json" "D binding add"
  bind_id="$(json_field "$bind_json" '.data.binding.binding_id')"

  proj_json="$(run_json "$repo" skill project demo-skill-d --binding "$bind_id" --method symlink)"
  assert_ok "$proj_json" "D project"
  instance_id="$(json_field "$proj_json" '.data.projection.instance_id')"
  materialized_path="$(json_field "$proj_json" '.data.projection.materialized_path')"

  printf '\n%s\n' "$marker" >> "$materialized_path/SKILL.md"
  assert_ok "$(run_json "$repo" skill capture demo-skill-d --instance "$instance_id" --message "e2e capture D")" "D capture"
  grep -q "$marker" "$repo/skills/demo-skill-d/SKILL.md"

  fail_bind_json="$(run_json_expect_fail "$repo" TARGET_NOT_FOUND workspace binding add --agent codex --profile bad --matcher-kind path-prefix --matcher-value "$workspace/bad" --target missing-target-id)"
  fail_remove_json="$(run_json_expect_fail "$repo" DEPENDENCY_CONFLICT target remove "$target_id")"
  [[ "$(json_field "$fail_bind_json" '.error.code')" == "TARGET_NOT_FOUND" ]]
  [[ "$(json_field "$fail_remove_json" '.error.code')" == "DEPENDENCY_CONFLICT" ]]

  printf 'D PASS target=%s binding=%s instance=%s\n' "$target_id" "$bind_id" "$instance_id"
}

mkdir -p "$ROOT_BASE"
echo "Running Loom agent E2E in: $ROOT_BASE"

scenario_a
scenario_b
scenario_c
scenario_d

echo "ALL PASS"
echo "Artifacts: $ROOT_BASE"
