#!/bin/sh
set -eu

REPOSITORY="${COMMIT_WISP_REPOSITORY:-siray-code/commit-wisp}"
VERSION="${COMMIT_WISP_VERSION:-latest}"
INSTALL_DIR="${COMMIT_WISP_INSTALL_DIR:-${HOME}/.local/bin}"

usage() {
  cat <<'EOF'
Install commit-wisp from a GitHub Release.

Environment variables:
  COMMIT_WISP_VERSION      Release tag or version (default: latest)
  COMMIT_WISP_INSTALL_DIR  Destination directory (default: ~/.local/bin)
  COMMIT_WISP_REPOSITORY   GitHub owner/repository override
EOF
}

case "${1:-}" in
  -h|--help)
    usage
    exit 0
    ;;
  '') ;;
  *)
    printf 'error: unknown argument: %s\n' "$1" >&2
    usage >&2
    exit 2
    ;;
esac

case "$REPOSITORY" in
  *[!A-Za-z0-9._/-]*|*/*/*|/*|*/|'')
    printf 'error: invalid COMMIT_WISP_REPOSITORY\n' >&2
    exit 2
    ;;
esac

if [ "$VERSION" != "latest" ]; then
  case "$VERSION" in
    *[!A-Za-z0-9._-]*|'')
      printf 'error: invalid COMMIT_WISP_VERSION\n' >&2
      exit 2
      ;;
  esac
  case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac
fi

case "$(uname -s)" in
  Darwin) os="apple-darwin" ;;
  Linux)
    if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
      printf 'error: this release does not support musl-based Linux yet\n' >&2
      exit 1
    fi
    os="unknown-linux-gnu"
    ;;
  *)
    printf 'error: unsupported operating system: %s\n' "$(uname -s)" >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64|amd64) arch="x86_64" ;;
  arm64|aarch64) arch="aarch64" ;;
  *)
    printf 'error: unsupported architecture: %s\n' "$(uname -m)" >&2
    exit 1
    ;;
esac

command -v curl >/dev/null 2>&1 || {
  printf 'error: curl is required\n' >&2
  exit 1
}

asset="commit-wisp-${arch}-${os}.tar.gz"
if [ "$VERSION" = "latest" ]; then
  base_url="https://github.com/${REPOSITORY}/releases/latest/download"
else
  base_url="https://github.com/${REPOSITORY}/releases/download/${VERSION}"
fi

tmp_dir="$(mktemp -d 2>/dev/null || mktemp -d -t commit-wisp)"
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

printf 'Downloading %s...\n' "$asset"
curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
  "$base_url/$asset" --output "$tmp_dir/$asset"
curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
  "$base_url/SHA256SUMS" --output "$tmp_dir/SHA256SUMS"

expected="$(awk -v file="$asset" '$2 == file { print $1 }' "$tmp_dir/SHA256SUMS")"
if [ -z "$expected" ]; then
  printf 'error: %s is missing from SHA256SUMS\n' "$asset" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp_dir/$asset" | awk '{ print $1 }')"
elif command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "$tmp_dir/$asset" | awk '{ print $1 }')"
else
  printf 'error: sha256sum or shasum is required\n' >&2
  exit 1
fi

if [ "$actual" != "$expected" ]; then
  printf 'error: checksum verification failed for %s\n' "$asset" >&2
  exit 1
fi

tar -xzf "$tmp_dir/$asset" -C "$tmp_dir"
binary="$tmp_dir/${asset%.tar.gz}/commit-wisp"
[ -f "$binary" ] || {
  printf 'error: archive does not contain commit-wisp\n' >&2
  exit 1
}

mkdir -p "$INSTALL_DIR"
if command -v install >/dev/null 2>&1; then
  install -m 755 "$binary" "$INSTALL_DIR/commit-wisp"
else
  cp "$binary" "$INSTALL_DIR/commit-wisp"
  chmod 755 "$INSTALL_DIR/commit-wisp"
fi

printf 'Installed commit-wisp to %s/commit-wisp\n' "$INSTALL_DIR"
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *) printf 'Add %s to PATH to run commit-wisp.\n' "$INSTALL_DIR" ;;
esac
