#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cli="${repo_root}/target/debug/d-cut-cli"
work_dir="${repo_root}/build/media-workflow-test"

for command_name in ffmpeg ffprobe; do
    if ! command -v "${command_name}" >/dev/null 2>&1; then
        printf 'Error: %s is required for media workflow tests.\n' "${command_name}" >&2
        exit 1
    fi
done
if [ ! -x "${cli}" ]; then
    printf 'Error: build the CLI first: cargo build -p editor-cli\n' >&2
    exit 1
fi

mkdir -p "${work_dir}"
trap 'rm -rf "${work_dir}"' EXIT

ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i 'testsrc2=size=640x360:rate=30' \
    -f lavfi -i 'sine=frequency=440:sample_rate=48000' \
    -t 3 -c:v libx264 -pix_fmt yuv420p -c:a aac -ar 48000 \
    "${work_dir}/source.mp4"

"${cli}" probe "${work_dir}/source.mp4"
"${cli}" export-quality "${work_dir}/source.mp4" "${work_dir}/export.mp4" \
    0 2 640 360 30 1 22

python3 - "${work_dir}/export.mp4" <<'PY'
import json
import subprocess
import sys

path = sys.argv[1]
raw = subprocess.check_output([
    'ffprobe', '-v', 'error', '-show_streams', '-show_format', '-of', 'json', path
], text=True)
info = json.loads(raw)
streams = info['streams']
video = next(stream for stream in streams if stream['codec_type'] == 'video')
audio = next(stream for stream in streams if stream['codec_type'] == 'audio')
assert video['codec_name'] == 'h264', video
assert audio['codec_name'] == 'aac', audio
assert (video['width'], video['height']) == (640, 360), video
assert video['pix_fmt'] == 'yuv420p', video
assert video['avg_frame_rate'] == '30/1', video
assert abs(float(info['format']['duration']) - 2.0) <= 1.0 / 30.0, info['format']
print('ffprobe integration checks passed')
PY

printf '%s\n' 'Media workflow tests passed.'
