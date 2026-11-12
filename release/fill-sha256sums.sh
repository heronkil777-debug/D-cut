#!/usr/bin/env bash
# Fills sha256sums in release/aur/PKGBUILD with the checksum of the GitHub tag tarball and
# regenerates .SRCINFO. Works offline with a local tarball for testing.
#
# Usage:
#   release/fill-sha256sums.sh v0.1.0              # download from GitHub
#   release/fill-sha256sums.sh v0.1.0 /tmp/src.tar.gz   # use a local file
set -euo pipefail

tag="${1:-}"
local_tarball="${2:-}"
if [ -z "${tag}" ]; then
    printf 'Usage: %s <tag> [local-tarball]\n' "$0" >&2
    exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pkgbuild="${repo_root}/release/aur/PKGBUILD"
pkgver="$(sed -n 's/^pkgver=//p' "${pkgbuild}")"
if [ "${tag#v}" != "${pkgver}" ]; then
    printf 'Warning: tag %s does not match pkgver %s in the PKGBUILD.\n' "${tag}" "${pkgver}" >&2
fi

url="$(sed -n 's/^url="\(.*\)"$/\1/p' "${pkgbuild}")"
if [ -z "${url}" ]; then
    printf 'Error: could not read url= from %s\n' "${pkgbuild}" >&2
    exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
tarball="${local_tarball}"
if [ -z "${tarball}" ]; then
    tarball="${work}/source.tar.gz"
    printf 'Downloading %s/archive/refs/tags/%s.tar.gz\n' "${url}" "${tag}"
    curl --fail --location --retry 3 --output "${tarball}" \
        "${url}/archive/refs/tags/${tag}.tar.gz"
else
    printf 'Using local tarball %s\n' "${tarball}"
fi

sum="$(sha256sum "${tarball}" | cut -d' ' -f1)"
python3 - "${pkgbuild}" "${sum}" <<'PY'
import re
import sys
path, checksum = sys.argv[1:3]
text = open(path, encoding="utf-8").read()
text, count = re.subn(r"^sha256sums=\(.*\).*$", f"sha256sums=('{checksum}')", text, flags=re.M)
if count != 1:
    raise SystemExit(f"expected one sha256sums line, found {count}")
open(path, "w", encoding="utf-8").write(text)
PY
printf 'sha256sums updated: %s\n' "${sum}"
"${repo_root}/release/prepare-aur.sh"
