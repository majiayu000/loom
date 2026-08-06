#!/bin/sh

set -eu

repository="majiayu000/loom"
repository_url="https://github.com/${repository}"
download_root="${LOOM_INSTALL_BASE_URL:-${repository_url}/releases/download}"
version="${LOOM_INSTALL_VERSION:-}"
target="${LOOM_INSTALL_TARGET:-}"
bin_dir="${LOOM_INSTALL_BIN_DIR:-${HOME}/.local/bin}"
data_dir="${LOOM_INSTALL_DATA_DIR:-${XDG_DATA_HOME:-${HOME}/.local/share}/loom}"

usage() {
  cat <<'EOF'
Install Loom from a verified GitHub Release archive.

Usage: install.sh [options]

Options:
  --version VERSION   Install a specific version (default: latest release)
  --target TARGET     Override the detected release target
  --bin-dir DIR       Install the loom binary here (default: ~/.local/bin)
  --data-dir DIR      Install bundled Skills/contracts here (default: ~/.local/share/loom)
  -h, --help          Show this help

Environment equivalents: LOOM_INSTALL_VERSION, LOOM_INSTALL_TARGET,
LOOM_INSTALL_BIN_DIR, LOOM_INSTALL_DATA_DIR, and LOOM_INSTALL_BASE_URL.
EOF
}

die() {
  printf 'loom installer: %s\n' "$*" >&2
  exit 1
}

need_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || die "--version requires a value"
      version="$2"
      shift 2
      ;;
    --target)
      [ "$#" -ge 2 ] || die "--target requires a value"
      target="$2"
      shift 2
      ;;
    --bin-dir)
      [ "$#" -ge 2 ] || die "--bin-dir requires a value"
      bin_dir="$2"
      shift 2
      ;;
    --data-dir)
      [ "$#" -ge 2 ] || die "--data-dir requires a value"
      data_dir="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1 (try --help)"
      ;;
  esac
done

need_command curl
need_command install
need_command tar

if [ -z "$target" ]; then
  machine="$(uname -m)"
  system="$(uname -s)"
  case "${system}:${machine}" in
    Darwin:arm64|Darwin:aarch64)
      target="aarch64-apple-darwin"
      ;;
    Darwin:x86_64)
      target="x86_64-apple-darwin"
      ;;
    Linux:x86_64|Linux:amd64)
      target="x86_64-unknown-linux-gnu"
      ;;
    *)
      die "no prebuilt release for ${system} ${machine}; install the current source with 'git clone https://github.com/majiayu000/loom.git && cd loom && cargo install --path .'"
      ;;
  esac
fi

if [ -z "$version" ]; then
  latest_url="$(curl -fsSL -o /dev/null -w '%{url_effective}' "${repository_url}/releases/latest")" \
    || die "could not resolve the latest Loom release"
  latest_tag="${latest_url##*/}"
  case "$latest_tag" in
    v[0-9]*) version="${latest_tag#v}" ;;
    *) die "GitHub returned an unexpected latest-release URL: $latest_url" ;;
  esac
fi

case "$version" in
  v*) version="${version#v}" ;;
esac

case "$version" in
  ''|*[!0-9A-Za-z.+-]*) die "invalid version: $version" ;;
esac
case "$version" in
  [0-9]*) ;;
  *) die "invalid version: $version" ;;
esac
case "$target" in
  ''|*[!0-9A-Za-z._-]*) die "invalid target: $target" ;;
esac

archive="skillloom-${version}-${target}.tar.gz"
release_url="${download_root}/v${version}"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/loom-install.XXXXXX")"
staged_binary=""
cleanup() {
  rm -rf "$tmp_dir"
  if [ -n "$staged_binary" ]; then
    rm -f "$staged_binary"
  fi
}
trap cleanup EXIT HUP INT TERM

printf 'Downloading Loom %s for %s...\n' "$version" "$target"
curl -fsSL "${release_url}/${archive}" -o "${tmp_dir}/${archive}" \
  || die "could not download ${release_url}/${archive}"
curl -fsSL "${release_url}/SHA256SUMS" -o "${tmp_dir}/SHA256SUMS" \
  || die "could not download ${release_url}/SHA256SUMS"

expected_checksum="$(awk -v archive="$archive" '
  {
    name = $2
    sub(/^\*/, "", name)
    sub(/^\.\//, "", name)
    if (name == archive) {
      print $1
      exit
    }
  }
' "${tmp_dir}/SHA256SUMS")"
[ -n "$expected_checksum" ] || die "SHA256SUMS has no entry for $archive"

if command -v shasum >/dev/null 2>&1; then
  actual_checksum="$(shasum -a 256 "${tmp_dir}/${archive}" | awk '{print $1}')"
elif command -v sha256sum >/dev/null 2>&1; then
  actual_checksum="$(sha256sum "${tmp_dir}/${archive}" | awk '{print $1}')"
else
  die "checksum verification requires shasum or sha256sum"
fi
[ "$actual_checksum" = "$expected_checksum" ] \
  || die "checksum mismatch for $archive"

tar -C "$tmp_dir" -xzf "${tmp_dir}/${archive}"
bundle="${tmp_dir}/skillloom-${version}-${target}"
[ -x "${bundle}/loom" ] || die "release archive does not contain an executable loom binary"
[ -f "${bundle}/contract-manifest.json" ] || die "release archive is missing contract-manifest.json"
[ -f "${bundle}/skills/loom-registry/SKILL.md" ] || die "release archive is missing the loom-registry Skill"
[ -f "${bundle}/contracts/agent-command-surfaces.toml" ] || die "release archive is missing the CLI contract inventory"

release_data_dir="${data_dir}/releases/${version}"
current_data_dir="${data_dir}/current"
if [ -e "$current_data_dir" ] && [ ! -L "$current_data_dir" ]; then
  die "refusing to replace non-symlink data path: $current_data_dir"
fi

mkdir -p "$bin_dir" "${release_data_dir}/skills" "${release_data_dir}/contracts"
staged_binary="${bin_dir}/.loom.install.$$"
install -m 0755 "${bundle}/loom" "$staged_binary"
cp -R "${bundle}/skills/." "${release_data_dir}/skills/"
cp -R "${bundle}/contracts/." "${release_data_dir}/contracts/"
install -m 0644 "${bundle}/contract-manifest.json" "${release_data_dir}/contract-manifest.json"
mv "$staged_binary" "${bin_dir}/loom"
staged_binary=""
ln -sfn "releases/${version}" "$current_data_dir"

printf '\nLoom %s installed successfully.\n' "$version"
printf '  binary: %s\n' "${bin_dir}/loom"
printf '  bundled Skill: %s\n' "${current_data_dir}/skills/loom-registry"
case ":${PATH}:" in
  *":${bin_dir}:"*) ;;
  *) printf '  PATH note: add %s to your PATH.\n' "$bin_dir" ;;
esac
printf '\nNext: run "loom init", then "loom panel".\n'
