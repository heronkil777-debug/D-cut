# Agent instructions

## Project
- Build a local-first, open-source Linux desktop video editor from scratch in Rust; the selected path is D.
- The goal is a useful editor for the owner's videos, not feature parity with CapCut.
- Communicate in Brazilian Portuguese. Source code, identifiers, commits, and comments are in English.
- Read `AGENTS.md`, `docs/STATUS.md`, and `docs/DECISIONS.md` at the start of every session. If these conflict with `docs/BRIEFING.md`, decisions win; tell the user and reconcile both docs.
- Follow `docs/BRIEFING.md` in full. Normally work one phase per message and wait for explicit `ok` before the next phase; if the user explicitly authorizes a multi-phase batch, that instruction overrides the pacing rule for that batch.
- Present or record the phase acceptance criteria before implementation; explicit authorization to batch phases also authorizes proceeding without a per-phase confirmation.
- After the alignment `ok`, maintain `docs/BRIEFING.md`, `docs/DECISIONS.md`, `docs/STATUS.md`, and this file. Update briefing and decisions together when a decision changes the project specification.

## Safety and collaboration
- Never run `sudo`, `pacman`, or `yay`; never change dotfiles; do not delete outside this repository.
- Do not push, force-push, publish a release, or publish packages unless the user explicitly asks.
- Work only on the current Arena branch; never rename or switch branches.
- Do not copy code, shaders, or assets from Concat/CapCraft without explicit approval.
- Ask before adding a heavy dependency, changing architecture, or deciding a license; state what, why, and alternative in three lines, then wait.
- Common Rust dependencies do not require advance approval. Use `cargo add` to choose actual crate versions and commit `Cargo.lock`; pin Rust in `rust-toolchain.toml` (edition 2024).
- No invented crate APIs: check docs.rs, cargo documentation, or source code when unsure.
- Never claim a build, test, or runtime check that was not performed. Separate command/result verification from what could not be verified.
- If blocked by missing toolchain, library, or GPU, say so immediately and give exact user commands. For visual behavior not visible here, provide a manual checklist.
- After three attempts at the same blocker, stop and ask the user rather than layering workarounds.

## Engineering rules
- Commands model edits and support undo/redo; property-test apply/undo round trips.
- Preview and export share the render path. Audio is the playback master clock.
- Keep I/O and decode off the UI thread; budget frame caches conservatively; UI frame target is under 16 ms.
- Handle phone rotation metadata, VFR timestamps, color matrix/range, and SDR conversion warnings for HDR.
- Avoid `unwrap`/`expect` in production paths. Every `unsafe` has a `// SAFETY:` comment.
- Use `thiserror` in libraries, `anyhow` in binaries, and `tracing` to local files only. UI text belongs in a localization catalog from the start.
- Golden-image tests are mandatory for effects. Generate small media fixtures with FFmpeg; do not commit large/copyrighted video.
- No network calls except opt-in model downloads and opt-in update checks.

## Build and verification
- Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`; report exact results honestly.
- From Phase 1 onward, produce the AppImage and report its status. Do not claim CI is green unless checked.
- End every phase with the briefing's ordered acceptance report and a local `phase-N` tag; stop for the user's `ok`.
- AppImage builds target an old glibc container and must not bundle host Vulkan/Mesa/VAAPI/audio/display drivers. Follow the FUSE fallback and license/notice requirements in the briefing.

## Commands (verified 2026-11-11)
- `cargo test --workspace` runs real FFmpeg integration tests (`crates/editor-media/tests/`); they skip if ffmpeg is absent.
- `scripts/build-appimage.sh` (container) or `scripts/build-appimage.sh --host` (no container, host glibc).
- GUI debug: `D_CUT_HUD=1` (HUD on), `D_CUT_AUTOTEST=play:3,quit:20` (auto play/close for measurements; logs in `~/.local/state/d-cut/logs/d-cut.log`).
- `D_CUT_FFMPEG` / `D_CUT_FFPROBE` override the FFmpeg binaries.
