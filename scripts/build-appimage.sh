#!/usr/bin/env bash
# Builds build/appimage/D-cut-x86_64.AppImage.
#
# Default: build inside an Ubuntu 22.04 container (Podman or Docker) so the binary only needs
# glibc >= 2.35 and runs on most distributions.
# --host:  build directly on this machine (no container). The AppImage then needs a glibc at
#          least as new as the build machine's (fine for an up-to-date Arch, not portable).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mode="container"
if [ "${1:-}" = "--host" ]; then
    mode="host"
fi
if [ ! -f "${repo_root}/Cargo.lock" ]; then
    printf '%s\n' 'Error: Cargo.lock is missing.' >&2
    exit 1
fi

output_dir="${repo_root}/build/appimage"
appimage_path="${output_dir}/D-cut-x86_64.AppImage"
mkdir -p "${output_dir}"

if [ "${mode}" = "container" ]; then
    container_engine=""
    if command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
        container_engine="podman"
    elif command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
        container_engine="docker"
    fi
    if [ -z "${container_engine}" ]; then
        cat >&2 <<'MSG'
Error: no usable container engine (Podman, or Docker with access to its socket).
  - Podman (rootless, recommended on Arch): sudo pacman -S podman
  - or Docker: sudo usermod -aG docker "$USER" and log in again
  - or build without a container (glibc of this machine): scripts/build-appimage.sh --host
MSG
        exit 1
    fi
    ffmpeg_version="7.1.5"
    ffmpeg_dir="${repo_root}/build/ffmpeg"
    tarball="${ffmpeg_dir}/ffmpeg-${ffmpeg_version}.tar.xz"
    mkdir -p "${ffmpeg_dir}"
    if [ ! -f "${tarball}" ] || [ ! -f "${tarball}.asc" ]; then
        curl -sS --fail --location --retry 5 --retry-all-errors \
            "https://ffmpeg.org/releases/ffmpeg-${ffmpeg_version}.tar.xz" -o "${tarball}"
        curl -sS --fail --location --retry 5 --retry-all-errors \
            "https://ffmpeg.org/releases/ffmpeg-${ffmpeg_version}.tar.xz.asc" -o "${tarball}.asc"
    fi
    if [ ! -f "${ffmpeg_dir}/ffmpeg-devel.asc" ]; then
        curl -sS --fail --location --retry 5 --retry-all-errors \
            https://ffmpeg.org/ffmpeg-devel.asc -o "${ffmpeg_dir}/ffmpeg-devel.asc"
    fi
    # Temporary keyring inside build/ (the user's ~/.gnupg is never touched).
    export GNUPGHOME="${ffmpeg_dir}/gnupg"
    mkdir -p -m 700 "${GNUPGHOME}"
    gpg --quiet --import "${ffmpeg_dir}/ffmpeg-devel.asc" 2>/dev/null || true
    gpg --quiet --verify "${tarball}.asc" "${tarball}"
    image_name="d-cut-appimage-builder:ubuntu-22.04"
    "${container_engine}" build --network=host \
        --file "${repo_root}/scripts/appimage/Dockerfile" \
        --build-arg "FFMPEG_VERSION=${ffmpeg_version}" \
        --tag "${image_name}" \
        "${repo_root}"
    user_args=(--user "$(id -u):$(id -g)")
    if [ "${container_engine}" = "podman" ]; then
        # Rootless Podman: map the host user into the container so /workspace stays writable.
        user_args=(--userns=keep-id "${user_args[@]}")
    fi
    "${container_engine}" run --rm --network=host \
        "${user_args[@]}" \
        --env HOME=/tmp \
        --env CARGO_HOME=/workspace/build/cargo-home \
        --env CARGO_TARGET_DIR=/workspace/build/target-container \
        --volume "${repo_root}:/workspace" \
        --workdir /workspace \
        "${image_name}" \
        bash -euo pipefail -c '
            mkdir -p /workspace/build/cargo-home
            scripts/appimage/assemble.sh /workspace /workspace/build/tools
        '
else
    printf '%s\n' 'Warning: host build; the AppImage requires a glibc at least as new as this machine.' >&2
    "${repo_root}/scripts/appimage/assemble.sh" "${repo_root}" "${repo_root}/build/tools"
fi

(cd "${output_dir}" && sha256sum "$(basename "${appimage_path}")" > "$(basename "${appimage_path}").sha256")
printf 'AppImage output: %s\n' "${appimage_path}"
