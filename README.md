# D-cut

D-cut is a local-first, open-source desktop video editor for Linux, being built in Rust for Arch Linux and distributed as an AppImage. The current checkout is an early, **unvalidated prototype**: it has an editor shell, project/timeline data types, reversible clip edits, JSON project persistence, SubRip parsing, and local FFmpeg/ffprobe CLI helpers. It is not yet a usable video editor or distributable AppImage.

The complete product scope and phase acceptance criteria are in [`docs/BRIEFING.md`](docs/BRIEFING.md); active work and verified blockers are tracked in [`docs/STATUS.md`](docs/STATUS.md).

## Development

The workspace is edition 2024 and pins Rust 1.95.0 in `rust-toolchain.toml`. Install Rust with `rustfmt` and `clippy`, then run:

```sh
cargo generate-lockfile
cargo build --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

For the media process helpers and CLI export integration, install FFmpeg/ffprobe (including H.264/AAC support) and run:

```sh
cargo build -p editor-cli
scripts/test-media-workflow.sh
```

The test script generates its media fixtures under the ignored `build/` directory and removes them when it exits.

Build the AppImage with Podman or Docker:

```sh
scripts/build-appimage.sh
```

The AppImage build still needs an FFmpeg 7+ bundle, dependency notices, and an actual container/runtime validation before it is distributable. The intended AppImage uses host Vulkan, Mesa, VAAPI, audio, and display drivers; if FUSE is unavailable, run it with `--appimage-extract-and-run`.
