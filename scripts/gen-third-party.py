#!/usr/bin/env python3
"""Regenerates THIRD_PARTY_LICENSES from Cargo.lock (cargo metadata) plus bundled assets/FFmpeg."""
import collections, json, subprocess

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--locked",
     "--filter-platform", "x86_64-unknown-linux-gnu"]))
packages = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
roots = set(meta["workspace_members"])
seen, stack = set(), list(roots)
while stack:
    current = stack.pop()
    if current in seen:
        continue
    seen.add(current)
    for dep in nodes[current]["deps"]:
        if any(kind["kind"] is None for kind in dep["dep_kinds"]):
            stack.append(dep["pkg"])
rows = sorted((packages[i]["name"], packages[i]["version"], packages[i].get("license") or "see crate")
              for i in seen if i not in roots)
counts = collections.Counter(row[2] for row in rows)
out = f"""D-cut — third-party notices
============================

D-cut itself is licensed AGPL-3.0-or-later (see LICENSE).

1. Bundled font
   Noto Sans Bold (assets/fonts/NotoSans-Bold.ttf), Copyright 2022 The Noto Project Authors,
   SIL Open Font License 1.1 — full text in assets/fonts/NotoSans-OFL.txt.

2. FFmpeg (portable AppImage only)
   FFmpeg 7.1.5 built with --enable-gpl --disable-nonfree (GPL-2.0-or-later as a whole), with
   libx264 (GPL-2.0-or-later), libx265 (GPL-2.0-or-later), SVT-AV1 (BSD-2-Clause-Patent /
   BSD-3-Clause-Clear per Ubuntu 22.04 package) and libnuma (LGPL-2.1). No nonfree components,
   no libfdk_aac (AAC uses FFmpeg's native encoder). License texts, Debian copyright files,
   the source tarball URL/SHA-256 and the exact build configuration are shipped inside the
   AppImage at usr/share/licenses/ffmpeg/ (BUILD-MANIFEST.txt). Corresponding source:
   https://ffmpeg.org/releases/ffmpeg-7.1.5.tar.xz and the Ubuntu 22.04 (jammy) source packages
   x264, x265, svt-av1 and numactl. Host libraries (libva, libdrm, zlib, glibc, Vulkan, ALSA,
   Wayland/X11) are not bundled. The provisional host build does not bundle FFmpeg.

3. Speech recognition (Phase 5)
   whisper.cpp v1.9.5 (whisper-cli, bundled in the portable AppImage): MIT License,
   Copyright (c) 2023-2026 The ggml authors — https://github.com/ggml-org/whisper.cpp
   (license text shipped at usr/share/licenses/whisper.cpp/LICENSE).
   Whisper model weights (downloaded only on explicit user request, never bundled):
   ggml-small.bin from https://huggingface.co/ggerganov/whisper.cpp (license: MIT; original
   weights by OpenAI, MIT License, Copyright (c) 2022 OpenAI — https://github.com/openai/whisper).
   SHA-256 1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b, 487601967 bytes.

4. Rust crates
   Generated from Cargo.lock with `cargo metadata` (normal dependencies reachable from the
   workspace for x86_64-unknown-linux-gnu); `cargo deny check licenses` passes with deny.toml.
   Full license texts are in each crate's source distribution.

License summary (crate count):
"""
out += "\n".join(f"  {count:4d}  {name}" for name, count in counts.most_common())
out += "\n\nCrates:\n" + "\n".join(f"  {name} {version} — {lic}" for name, version, lic in rows) + "\n"
open("THIRD_PARTY_LICENSES", "w").write(out)
print(len(rows), "crates")
