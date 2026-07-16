#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
guard="$script_dir/module-ceiling.sh"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/loom-module-ceiling.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT

mkdir -p "$fixture/src/tests" "$fixture/src/generated" "$fixture/scripts"
allowlist="$fixture/scripts/module-ceiling-allowlist.txt"

write_lines() {
  local path="$1"
  local count="$2"
  mkdir -p "$(dirname "$path")"
  awk -v count="$count" 'BEGIN { for (i = 1; i <= count; i++) print "// fixture line " i }' > "$path"
}

run_guard() {
  "$guard" --root "$fixture" --allowlist "$allowlist"
}

expect_failure() {
  local expected="$1"
  if run_guard >"$fixture/output.log" 2>&1; then
    echo "expected module ceiling failure containing: $expected" >&2
    exit 1
  fi
  if ! grep -F "$expected" "$fixture/output.log" >/dev/null; then
    echo "missing expected failure: $expected" >&2
    cat "$fixture/output.log" >&2
    exit 1
  fi
}

write_lines "$fixture/src/ok.rs" 700
write_lines "$fixture/src/warning.rs" 701
write_lines "$fixture/src/allowed.rs" 801
write_lines "$fixture/src/tests/ignored.rs" 900
write_lines "$fixture/src/ignored_tests.rs" 900
write_lines "$fixture/src/generated/bindings.rs" 900
printf 'src/allowed.rs\t801\t#999\n' > "$allowlist"

run_guard >"$fixture/output.log" 2>&1
grep -F "WARNING src/warning.rs 701 800" "$fixture/output.log" >/dev/null
grep -F "ALLOWLIST src/allowed.rs 801 800 baseline=801 issue=#999" "$fixture/output.log" >/dev/null
grep -F "module-ceiling: passed warnings=1 allowlisted=1" "$fixture/output.log" >/dev/null

write_lines "$fixture/src/unallowlisted.rs" 801
expect_failure "src/unallowlisted.rs 801 800 not-allowlisted"
rm "$fixture/src/unallowlisted.rs"

write_lines "$fixture/src/allowed.rs" 802
expect_failure "src/allowed.rs 802 800 baseline-growth=801 issue=#999"

write_lines "$fixture/src/allowed.rs" 850
printf 'src/allowed.rs\t981\t#999\n' > "$allowlist"
expect_failure "src/allowed.rs 850 800 baseline-decrease=981 update-required issue=#999"

printf 'src/allowed.rs\t850\t#999\n' > "$allowlist"
run_guard >"$fixture/output.log" 2>&1
grep -F "ALLOWLIST src/allowed.rs 850 800 baseline=850 issue=#999" "$fixture/output.log" >/dev/null

write_lines "$fixture/src/allowed.rs" 851
expect_failure "src/allowed.rs 851 800 baseline-growth=850 issue=#999"

write_lines "$fixture/src/allowed.rs" 800
expect_failure "src/allowed.rs 800 800 stale-allowlist baseline=850 issue=#999"

write_lines "$fixture/src/allowed.rs" 850
awk 'BEGIN { for (i = 1; i <= 800; i++) print "// fixture line " i; printf "// fixture line 801" }' > "$fixture/src/no_final_newline.rs"
expect_failure "src/no_final_newline.rs 801 800 not-allowlisted"
rm "$fixture/src/no_final_newline.rs"

write_lines "$fixture/external.rs" 900
ln -s ../external.rs "$fixture/src/symlinked.rs"
expect_failure "src/symlinked.rs symlink-source-path-unsupported"
rm "$fixture/src/symlinked.rs" "$fixture/external.rs"

write_lines "$fixture/src/allowed.rs" 801
printf 'src/missing.rs\t801\t#1000\n' > "$allowlist"
expect_failure "src/missing.rs missing-or-excluded allowlist entry issue=#1000"

printf 'src/allowed.rs 801 #999\n' > "$allowlist"
expect_failure "allowlist:1 malformed entry"

echo "module-ceiling-test: passed"
