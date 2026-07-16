#!/bin/sh
set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
  printf 'usage: %s <version> <SHA256SUMS> [output]\n' "$0" >&2
  exit 2
fi

tag="$1"
checksums="$2"
output="${3:-/dev/stdout}"
version="${tag#v}"

case "$tag" in
  v[0-9]*.[0-9]*.[0-9]*) ;;
  *) printf 'error: version must be a v-prefixed semantic version\n' >&2; exit 2 ;;
esac

checksum() {
  value="$(awk -v file="$1" '$2 == file { print $1 }' "$checksums")"
  case "$value" in
    ''|*[!0-9a-fA-F]*) printf 'error: checksum not found for %s\n' "$1" >&2; exit 1 ;;
  esac
  if [ "${#value}" -ne 64 ]; then
    printf 'error: invalid checksum for %s\n' "$1" >&2
    exit 1
  fi
  printf '%s' "$value"
}

mac_arm="$(checksum commit-wisp-aarch64-apple-darwin.tar.gz)"
mac_intel="$(checksum commit-wisp-x86_64-apple-darwin.tar.gz)"
linux_arm="$(checksum commit-wisp-aarch64-unknown-linux-gnu.tar.gz)"
linux_intel="$(checksum commit-wisp-x86_64-unknown-linux-gnu.tar.gz)"
base_url="https://github.com/siray-code/commit-wisp/releases/download/${tag}"

mkdir -p "$(dirname "$output")"
sed \
  -e "s|@VERSION@|$version|g" \
  -e "s|@BASE_URL@|$base_url|g" \
  -e "s|@MAC_ARM_SHA@|$mac_arm|g" \
  -e "s|@MAC_INTEL_SHA@|$mac_intel|g" \
  -e "s|@LINUX_ARM_SHA@|$linux_arm|g" \
  -e "s|@LINUX_INTEL_SHA@|$linux_intel|g" \
  "$(dirname "$0")/../packaging/homebrew/commit-wisp.rb.in" > "$output"
