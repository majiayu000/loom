#!/usr/bin/env bash
# Loom end-to-end demo — spins up a throwaway registry, registers a
# target directory, surfaces workspace status, and points at the Panel.
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

if [[ $# -ge 1 ]]; then
  ROOT="$1"
else
  ROOT="$(mktemp -d -t loom-demo-XXXXXX)"
fi

TARGET_DIR="$ROOT/skills-target"

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
mkdir -p "$TARGET_DIR"
"${LOOM[@]}" workspace init

step "2. Register $TARGET_DIR as a managed Claude target"
"${LOOM[@]}" target add \
  --agent claude \
  --path "$TARGET_DIR" \
  --ownership managed

step "3. Inspect registered targets"
"${LOOM[@]}" target list

step "4. Workspace status snapshot"
"${LOOM[@]}" workspace status

step "✅ Demo complete"
cat <<EOF

Launch the visual panel against this registry:

    $LOOM_BIN --root "$ROOT" panel

Then open http://localhost:43117 in your browser.
EOF
