#!/usr/bin/env bash
# Regenerates release/aur/.SRCINFO from release/aur/PKGBUILD (required by the AUR) and copies a
# ready-to-push folder to build/release/aur.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
aur="${repo_root}/release/aur"

if [ -z "$(sed -n 's/^sha256sums=(\(.*\))/\1/p' "${aur}/PKGBUILD")" ] \
    || grep -q "sha256sums=('SKIP')" "${aur}/PKGBUILD"; then
    printf 'Warning: sha256sums is still SKIP. Run release/fill-sha256sums.sh <tag> first.\n' >&2
fi

(cd "${aur}" && makepkg --printsrcinfo > .SRCINFO)

out="${repo_root}/build/release/aur"
rm -rf "${out}"
mkdir -p "${out}"
cp "${aur}/PKGBUILD" "${aur}/.SRCINFO" "${out}/"
printf 'AUR folder ready: %s\n' "${out}"
printf '  cd %s && git init && git remote add origin ssh://aur@aur.archlinux.org/d-cut.git\n' "${out}"
printf '  git add PKGBUILD .SRCINFO && git commit -m "d-cut %s" && git push origin master\n' \
    "$(sed -n 's/^pkgver=//p' "${aur}/PKGBUILD")"
