# D-cut

D-cut is a local-first, open-source desktop video editor for Linux (Rust + egui/wgpu + FFmpeg),
targeting Arch Linux and distributed as an AppImage. No account, no cloud, no telemetry.

**Status: early but usable single-timeline editor.** Import video/audio, see real frames in
the preview, play with audio, split / trim / move / delete / ripple-delete clips with
undo/redo, save/open projects and export MP4 (H.264 + AAC). See [`docs/STATUS.md`](docs/STATUS.md)
for what is verified and what is missing (text/subtitles, effects, transitions, keyframes,
auto-captions are not implemented yet). Full scope: [`docs/BRIEFING.md`](docs/BRIEFING.md).

## Requirements

- `ffmpeg` and `ffprobe` on `PATH` (Arch: `pacman -S ffmpeg`). The AppImage does not bundle FFmpeg yet.
- A Vulkan driver (Mesa RADV on AMD) for the wgpu preview.
- Optional: VAAPI (`/dev/dri/renderD128`) for the "Rápido" export profile.

## Run

```sh
cargo run --release -p editor-ui -- [video files or project.dcut]
# or the AppImage
./D-cut-x86_64.AppImage [files]
./D-cut-x86_64.AppImage --cli render-project project.dcut out.mp4      # headless export
./D-cut-x86_64.AppImage --appimage-extract-and-run                     # if FUSE is unavailable
```

Shortcuts: Space play/pause · S (or Ctrl+B) split at playhead · Delete delete · Shift+Delete
ripple delete · Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y · Ctrl+S save · Ctrl+O open · Ctrl+I import ·
Ctrl+E export · ←/→ one frame (Shift: one second) · Home/End · Ctrl+wheel zoom · F3 debug HUD ·
Ctrl+Q quit. Drag clip bodies to move them, drag clip edges to trim (snaps to the playhead and
clip edges).

Debug: `D_CUT_HUD=1` starts with the HUD visible. Logs (local only):
`~/.local/state/d-cut/logs/d-cut.log`.

## Development

```sh
cargo build --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace        # includes real FFmpeg integration tests (skipped if ffmpeg is missing)
scripts/test-media-workflow.sh   # after `cargo build -p editor-cli`
```

## AppImage

```sh
scripts/build-appimage.sh          # Ubuntu 22.04 container via Podman or Docker (portable glibc)
scripts/build-appimage.sh --host   # no container: needs glibc >= the build machine's
```

The AppImage uses the host's Vulkan/Mesa/VAAPI/audio/display libraries. It needs FUSE 2/3 to
mount; without FUSE run it with `--appimage-extract-and-run`.

License: AGPL-3.0-or-later.
