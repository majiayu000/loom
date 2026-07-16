#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 [--root <repo-root>] [--allowlist <file>]" >&2
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
allowlist_file=""

while (( $# > 0 )); do
  case "$1" in
    --root)
      [[ $# -ge 2 ]] || { usage; exit 2; }
      repo_root="$2"
      shift 2
      ;;
    --allowlist)
      [[ $# -ge 2 ]] || { usage; exit 2; }
      allowlist_file="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

repo_root="$(cd "$repo_root" && pwd)"
if [[ -z "$allowlist_file" ]]; then
  allowlist_file="$repo_root/scripts/module-ceiling-allowlist.txt"
elif [[ "$allowlist_file" != /* ]]; then
  allowlist_file="$repo_root/$allowlist_file"
fi

warn_limit=700
hard_limit=800
tab=$'\t'
allow_paths=()
allow_baselines=()
allow_issues=()
allow_seen=()
errors=0
warnings=0
allowlisted=0

error() {
  echo "ERROR $*" >&2
  errors=$((errors + 1))
}

is_test_or_generated() {
  case "$1" in
    */tests/*|*_tests.rs|*/generated/*|*_generated.rs|*/generated.rs)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

find_allow_index() {
  local wanted="$1"
  local i
  for ((i = 0; i < ${#allow_paths[@]}; i++)); do
    if [[ "${allow_paths[$i]}" == "$wanted" ]]; then
      echo "$i"
      return 0
    fi
  done
  return 1
}

if [[ ! -f "$allowlist_file" ]]; then
  echo "ERROR allowlist file is missing: $allowlist_file" >&2
  exit 1
fi

line_number=0
while IFS= read -r line || [[ -n "$line" ]]; do
  line_number=$((line_number + 1))
  [[ -z "$line" || "$line" == \#* ]] && continue

  if [[ "$line" != *"$tab"*"$tab"* ]]; then
    error "allowlist:$line_number malformed entry (expected path<TAB>baseline_lines<TAB>issue-ref)"
    continue
  fi

  path="${line%%"$tab"*}"
  rest="${line#*"$tab"}"
  baseline="${rest%%"$tab"*}"
  issue_ref="${rest#*"$tab"}"
  if [[ -z "$path" || -z "$baseline" || -z "$issue_ref" || "$issue_ref" == *"$tab"* ]]; then
    error "allowlist:$line_number malformed entry (expected exactly 3 fields)"
    continue
  fi
  if [[ "$path" == /* || "$path" == *".."* || "$path" != src/*.rs ]]; then
    error "allowlist:$line_number invalid production Rust path: $path"
    continue
  fi
  if is_test_or_generated "$path"; then
    error "allowlist:$line_number test/generated path cannot be allowlisted: $path"
    continue
  fi
  if [[ ! "$baseline" =~ ^[0-9]+$ ]] || (( baseline <= hard_limit )); then
    error "allowlist:$line_number baseline must be an integer above $hard_limit: $baseline"
    continue
  fi
  if [[ ! "$issue_ref" =~ ^#[0-9]+$ ]]; then
    error "allowlist:$line_number issue-ref must look like #123: $issue_ref"
    continue
  fi
  if find_allow_index "$path" >/dev/null; then
    error "allowlist:$line_number duplicate path: $path"
    continue
  fi

  allow_paths+=("$path")
  allow_baselines+=("$baseline")
  allow_issues+=("$issue_ref")
  allow_seen+=(0)
done < "$allowlist_file"

if [[ ! -d "$repo_root/src" ]]; then
  error "source directory is missing: $repo_root/src"
else
  while IFS= read -r file; do
    relative_path="${file#"$repo_root/"}"
    is_test_or_generated "$relative_path" && continue

    lines="$(wc -l < "$file" | tr -d ' ')"
    allow_index=""
    if allow_index="$(find_allow_index "$relative_path")"; then
      allow_seen[$allow_index]=1
      baseline="${allow_baselines[$allow_index]}"
      issue_ref="${allow_issues[$allow_index]}"
      if (( lines <= hard_limit )); then
        error "$relative_path $lines $hard_limit stale-allowlist baseline=$baseline issue=$issue_ref"
      elif (( lines > baseline )); then
        error "$relative_path $lines $hard_limit baseline-growth=$baseline issue=$issue_ref"
      else
        echo "ALLOWLIST $relative_path $lines $hard_limit baseline=$baseline issue=$issue_ref"
        allowlisted=$((allowlisted + 1))
      fi
    elif (( lines > hard_limit )); then
      error "$relative_path $lines $hard_limit not-allowlisted"
    elif (( lines > warn_limit )); then
      echo "WARNING $relative_path $lines $hard_limit"
      warnings=$((warnings + 1))
    fi
  done < <(find "$repo_root/src" -type f -name '*.rs' -print | LC_ALL=C sort)
fi

for ((i = 0; i < ${#allow_paths[@]}; i++)); do
  if [[ "${allow_seen[$i]}" != 1 ]]; then
    error "${allow_paths[$i]} missing-or-excluded allowlist entry issue=${allow_issues[$i]}"
  fi
done

if (( errors > 0 )); then
  echo "module-ceiling: failed errors=$errors warnings=$warnings allowlisted=$allowlisted" >&2
  exit 1
fi

echo "module-ceiling: passed warnings=$warnings allowlisted=$allowlisted hard_limit=$hard_limit warn_limit=$warn_limit"
