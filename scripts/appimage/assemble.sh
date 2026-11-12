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

# Bundle FFmpeg when it was built (container). Codec and libav* libraries go to usr/lib with
# an $ORIGIN rpath; host driver/system libraries (libva, libdrm, libc, libz...) are not copied.
ffmpeg_prefix="${FFMPEG_PREFIX:-/opt/ffmpeg}"
if [ -x "${ffmpeg_prefix}/bin/ffmpeg" ]; then
    mkdir -p "${app_dir}/usr/lib" "${app_dir}/usr/share/licenses/ffmpeg"
    install -m 0755 "${ffmpeg_prefix}/bin/ffmpeg" "${ffmpeg_prefix}/bin/ffprobe" "${app_dir}/usr/bin/"
    for binary in ffmpeg ffprobe; do
        LD_LIBRARY_PATH="${ffmpeg_prefix}/lib" ldd "${ffmpeg_prefix}/bin/${binary}" \
            | awk '/=> \// {print $3}' \
            | grep -E '/opt/ffmpeg/lib/|libx264|libx265|libSvtAv1Enc|libnuma' || true
    done | sort -u | while read -r library; do
        cp -L "${library}" "${app_dir}/usr/lib/"
    done
    for file in "${app_dir}/usr/bin/ffmpeg" "${app_dir}/usr/bin/ffprobe" "${app_dir}"/usr/lib/*.so*; do
        patchelf --set-rpath '$ORIGIN/../lib:$ORIGIN' "${file}"
    done
    cp "${ffmpeg_prefix}"/COPYING.GPLv2 "${ffmpeg_prefix}"/COPYING.LGPLv2.1 "${ffmpeg_prefix}"/LICENSE.md \
        "${app_dir}/usr/share/licenses/ffmpeg/"
    for package in libx264-163 libx265-199 libsvtav1enc0 libnuma1; do
        if [ -f "/usr/share/doc/${package}/copyright" ]; then
            cp "/usr/share/doc/${package}/copyright" "${app_dir}/usr/share/licenses/ffmpeg/${package}.copyright"
        fi
    done
    {
        printf 'FFmpeg %s (GPL build, no nonfree)\n' "$(cat "${ffmpeg_prefix}/VERSION")"
        printf 'Source: https://ffmpeg.org/releases/ffmpeg-%s.tar.xz\n' "$(cat "${ffmpeg_prefix}/VERSION")"
        printf 'Source sha256: %s\n' "$(cut -d' ' -f1 /opt/ffmpeg-source.sha256 2>/dev/null)"
        printf 'Codec packages (Ubuntu 22.04, sources at https://packages.ubuntu.com/source/jammy/):\n'
        dpkg-query -W -f='  ${Package} ${Version}\n' libx264-dev libx265-dev libsvtav1enc-dev libnuma-dev 2>/dev/null || true
        printf 'Bundled libraries:\n'
        (cd "${app_dir}/usr/lib" && ls -1 | sed 's/^/  /')
        printf 'Build configuration:\n'
        LD_LIBRARY_PATH="${app_dir}/usr/lib" "${app_dir}/usr/bin/ffmpeg" -hide_banner -buildconf | sed 's/^/  /'
    } > "${app_dir}/usr/share/licenses/ffmpeg/BUILD-MANIFEST.txt"
    printf 'Bundled FFmpeg: %s\n' "$(du -sh "${app_dir}/usr/lib" | cut -f1) of libraries"
else
    printf '%s\n' 'Note: FFmpeg not bundled (provisional build; uses the host ffmpeg).'
fi

if [ -x /opt/whisper/bin/whisper-cli ]; then
    install -m 0755 /opt/whisper/bin/whisper-cli "${app_dir}/usr/bin/whisper-cli"
    mkdir -p "${app_dir}/usr/share/licenses/whisper.cpp"
    cp /opt/whisper/LICENSE "${app_dir}/usr/share/licenses/whisper.cpp/LICENSE"
    printf 'whisper.cpp %s, source sha256 %s\n' "$(cat /opt/whisper/VERSION)" \
        "$(cut -d' ' -f1 /opt/whisper-source.sha256)" > "${app_dir}/usr/share/licenses/whisper.cpp/BUILD.txt"
fi

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
