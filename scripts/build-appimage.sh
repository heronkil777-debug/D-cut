#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ ! -f "${repo_root}/Cargo.lock" ]; then
    printf '%s\n' 'Error: Cargo.lock is missing; resolve dependencies and commit the lockfile before packaging.' >&2
    exit 1
fi

image_name="d-cut-appimage-builder:ubuntu-22.04"
output_dir="${repo_root}/build/appimage"
appimage_path="${output_dir}/D-cut-x86_64.AppImage"

if command -v podman >/dev/null 2>&1; then
    container_engine=(podman)
elif command -v docker >/dev/null 2>&1; then
    container_engine=(docker)
else
    printf '%s\n' 'Error: Podman or Docker is required to build the AppImage.' >&2
    exit 1
fi

mkdir -p "${output_dir}"
"${container_engine[@]}" build \
    --file "${repo_root}/scripts/appimage/Dockerfile" \
    --tag "${image_name}" \
    "${repo_root}"

"${container_engine[@]}" run --rm \
    --user "$(id -u):$(id -g)" \
    --env HOME=/tmp \
    --env CARGO_HOME=/workspace/build/cargo-home \
    --env APPIMAGE_EXTRACT_AND_RUN=1 \
    --volume "${repo_root}:/workspace" \
    --workdir /workspace \
    "${image_name}" \
    bash -euo pipefail -c '
        mkdir -p /workspace/build/cargo-home
        cargo build --locked --release --workspace
        app_dir="/workspace/build/appimage/D-cut.AppDir"
        rm -rf "${app_dir}"
        mkdir -p "${app_dir}/usr/bin" \
                 "${app_dir}/usr/share/applications" \
                 "${app_dir}/usr/share/metainfo" \
                 "${app_dir}/usr/share/doc/d-cut" \
                 "${app_dir}/usr/share/licenses/d-cut" \
                 "${app_dir}/usr/share/icons/hicolor/1024x1024/apps"
        install -m 0755 target/release/d-cut "${app_dir}/usr/bin/d-cut"
        install -m 0755 target/release/d-cut-cli "${app_dir}/usr/bin/d-cut-cli"
        install -m 0644 packaging/linux/d-cut.desktop "${app_dir}/d-cut.desktop"
        install -m 0644 packaging/linux/d-cut.desktop "${app_dir}/usr/share/applications/d-cut.desktop"
        install -m 0644 packaging/linux/d-cut.metainfo.xml "${app_dir}/usr/share/metainfo/d-cut.metainfo.xml"
        install -m 0644 THIRD_PARTY_LICENSES "${app_dir}/usr/share/doc/d-cut/THIRD_PARTY_LICENSES"
        install -m 0644 LICENSE "${app_dir}/usr/share/licenses/d-cut/LICENSE"
        install -m 0644 assets/branding/d-cut-icon.png "${app_dir}/usr/share/icons/hicolor/1024x1024/apps/d-cut.png"
        install -m 0644 assets/branding/d-cut-icon.png "${app_dir}/d-cut.png"
        ln -sfn d-cut.png "${app_dir}/.DirIcon"
        install -m 0755 packaging/linux/AppRun "${app_dir}/AppRun"
        curl --fail --location --retry 3 \
            https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage \
            --output /tmp/appimagetool.AppImage
        chmod +x /tmp/appimagetool.AppImage
        APPIMAGE_EXTRACT_AND_RUN=1 /tmp/appimagetool.AppImage \
            "${app_dir}" /workspace/build/appimage/D-cut-x86_64.AppImage
    '

sha256sum "${appimage_path}" > "${appimage_path}.sha256"
printf 'AppImage output: %s\n' "${appimage_path}"
