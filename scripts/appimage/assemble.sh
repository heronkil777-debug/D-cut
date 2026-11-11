#!/usr/bin/env bash
# Builds the release binaries and assembles the AppImage. Runs inside the Ubuntu 22.04
# container (portable glibc) or, with --host, directly on the build machine.
# Usage: scripts/appimage/assemble.sh <repo_root> <tools_dir>
set -euo pipefail

repo_root="$1"
tools_dir="$2"
cd "${repo_root}"

cargo build --locked --release -p editor-ui -p editor-cli
bin_dir="${CARGO_TARGET_DIR:-${repo_root}/target}/release"

out_dir="${repo_root}/build/appimage"
app_dir="${out_dir}/D-cut.AppDir"
rm -rf "${app_dir}"
mkdir -p "${app_dir}/usr/bin" \
         "${app_dir}/usr/share/applications" \
         "${app_dir}/usr/share/metainfo" \
         "${app_dir}/usr/share/doc/d-cut" \
         "${app_dir}/usr/share/licenses/d-cut" \
         "${app_dir}/usr/share/icons/hicolor/256x256/apps" \
         "${app_dir}/usr/share/icons/hicolor/1024x1024/apps"
install -m 0755 "${bin_dir}/d-cut" "${app_dir}/usr/bin/d-cut"
install -m 0755 "${bin_dir}/d-cut-cli" "${app_dir}/usr/bin/d-cut-cli"
install -m 0755 packaging/linux/AppRun "${app_dir}/AppRun"
install -m 0644 packaging/linux/d-cut.desktop "${app_dir}/d-cut.desktop"
install -m 0644 packaging/linux/d-cut.desktop "${app_dir}/usr/share/applications/d-cut.desktop"
install -m 0644 packaging/linux/d-cut.metainfo.xml "${app_dir}/usr/share/metainfo/d-cut.appdata.xml"
install -m 0644 THIRD_PARTY_LICENSES "${app_dir}/usr/share/doc/d-cut/THIRD_PARTY_LICENSES"
install -m 0644 LICENSE "${app_dir}/usr/share/licenses/d-cut/LICENSE"
install -m 0644 assets/branding/d-cut-icon-256.png "${app_dir}/usr/share/icons/hicolor/256x256/apps/d-cut.png"
install -m 0644 assets/branding/d-cut-icon.png "${app_dir}/usr/share/icons/hicolor/1024x1024/apps/d-cut.png"
install -m 0644 assets/branding/d-cut-icon-256.png "${app_dir}/d-cut.png"
ln -sfn d-cut.png "${app_dir}/.DirIcon"

mkdir -p "${tools_dir}"
tool="${tools_dir}/appimagetool-x86_64.AppImage"
if [ ! -x "${tool}" ]; then
    curl --fail --location --retry 3 \
        https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage \
        --output "${tool}.part"
    chmod +x "${tool}.part"
    mv "${tool}.part" "${tool}"
fi
printf 'appimagetool sha256: %s\n' "$(sha256sum "${tool}" | cut -d' ' -f1)"

ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "${tool}" --no-appstream \
    "${app_dir}" "${out_dir}/D-cut-x86_64.AppImage"
