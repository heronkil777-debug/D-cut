# D-cut

D-cut is a local-first, open-source desktop video editor for Linux (Rust + egui/wgpu + FFmpeg),
targeting Arch Linux and distributed as an AppImage. No account, no cloud, no telemetry.

**Status: 0.1.0, early but usable.** Import video/audio, edit on a multi-track timeline
(split, trim, move, ripple delete, undo/redo), composite text and subtitles on the GPU,
apply 45 effect packages (29 effects + 16 transitions) with keyframes, adjust audio per clip,
use 540p proxies for heavy footage, autosave every 30 s, generate offline PT-BR captions
(whisper.cpp; you pick the model) and export MP4 (H.264 + AAC). See
[`docs/STATUS.md`](docs/STATUS.md) for what is verified and what is still missing.
Full scope: [`docs/BRIEFING.md`](docs/BRIEFING.md).

## Requirements

- `ffmpeg` and `ffprobe` on `PATH` for a source build (Arch: `pacman -S ffmpeg`); the AppImage
  ships its own FFmpeg 7.1.5.
- A Vulkan driver (Mesa RADV on AMD) for the wgpu preview.
- Optional: VAAPI (`/dev/dri/renderD128`) for the "Rápido" export profile.
- Optional: `whisper-cli` (bundled in the AppImage) plus a downloaded Whisper model for automatic
  captions. Speech works; sung vocals are not reliable in any Whisper model.

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
