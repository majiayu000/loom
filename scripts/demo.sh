#!/usr/bin/env bash
# Loom end-to-end demo — spins up a throwaway registry, imports a skill,
# creates a target + binding, projects the skill, and points at the Panel.
# Intended for README walkthroughs and asciinema recordings.
#
# Usage:
#   ./scripts/demo.sh              # uses a fresh temp registry (left on disk
#                                  # so the printed `loom panel` command works)
#   ./scripts/demo.sh /custom/dir  # reuses /custom/dir (also left on disk)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOOM_BIN="${LOOM_BIN:-$REPO_ROOT/target/debug/loom}"

if [[ ! -x "$LOOM_BIN" ]]; then
  (cd "$REPO_ROOT" && cargo build -q)
fi

if [[ ! -x "$LOOM_BIN" ]]; then
  echo "loom binary not found: $LOOM_BIN" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required for the demo" >&2
  exit 1
fi

if [[ $# -ge 1 ]]; then
  ROOT="$1"
else
  ROOT="$(mktemp -d -t loom-demo-XXXXXX)"
fi

TARGET_DIR="$ROOT/agent/.claude/skills"
WORKSPACE_DIR="$ROOT/workspace-demo"
SEED_DIR="$ROOT/seed/demo-skill"

cleanup() {
  echo
  echo "ℹ️  Demo artifacts left at: $ROOT"
  echo "    Remove with: rm -rf \"$ROOT\""
}
trap cleanup EXIT

step() {
  echo
  echo "────────────────────────────────────────"
  echo "▶ $*"
  echo "────────────────────────────────────────"
}

LOOM=("$LOOM_BIN" --root "$ROOT")

step "1. Initialize a fresh Loom registry at $ROOT"
mkdir -p "$TARGET_DIR" "$WORKSPACE_DIR" "$SEED_DIR"
cat >"$SEED_DIR/SKILL.md" <<'EOF'
# demo-skill

Use this demo skill to verify Loom import, binding, projection, and capture.
EOF
"${LOOM[@]}" workspace init

step "2. Import demo-skill into the registry"
"${LOOM[@]}" skill add "$SEED_DIR" --name demo-skill

step "3. Register $TARGET_DIR as a managed Claude target"
target_json="$("${LOOM[@]}" --json target add \
  --agent claude \
  --path "$TARGET_DIR" \
  --ownership managed)"
printf '%s\n' "$target_json" | jq .
target_id="$(printf '%s' "$target_json" | jq -r '.data.target.target_id')"

step "4. Bind $WORKSPACE_DIR to $target_id"
binding_json="$("${LOOM[@]}" --json workspace binding add \
  --agent claude \
  --profile demo \
  --matcher-kind path-prefix \
  --matcher-value "$WORKSPACE_DIR" \
  --target "$target_id")"
printf '%s\n' "$binding_json" | jq .
binding_id="$(printf '%s' "$binding_json" | jq -r '.data.binding.binding_id')"

step "5. Project demo-skill through $binding_id"
"${LOOM[@]}" skill project demo-skill --binding "$binding_id" --method symlink

step "6. Inspect registered targets"
"${LOOM[@]}" target list

step "7. Workspace status snapshot"
"${LOOM[@]}" workspace status

step "Demo complete"
cat <<EOF

Launch the visual panel against this registry:

    $LOOM_BIN --root "$ROOT" panel

Then open http://localhost:43117 in your browser.
EOF
