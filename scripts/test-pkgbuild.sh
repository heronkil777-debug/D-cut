#!/usr/bin/env bash
# Builds the AUR PKGBUILD locally from the current commit (no network for the source, no sudo).
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="${repo_root}/build/pkgbuild-test"
rm -rf "${work}"
mkdir -p "${work}"
version="$(sed -n 's/^pkgver=//p' "${repo_root}/release/aur/PKGBUILD")"
git -C "${repo_root}" archive --format=tar.gz --prefix="D-cut-${version}/" -o "${work}/d-cut-${version}.tar.gz" HEAD
sed "s|^source=.*|source=(\"d-cut-${version}.tar.gz\")|" "${repo_root}/release/aur/PKGBUILD" > "${work}/PKGBUILD"
cd "${work}"
# --nodeps: rustup-installed cargo is not registered with pacman; the tools exist.
makepkg --force --noconfirm --cleanbuild --nodeps
ls -l "${work}"/*.pkg.tar.*
