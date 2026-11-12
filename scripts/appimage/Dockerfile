FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:${PATH}

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        clang \
        curl \
        file \
        git \
        libasound2-dev \
        libgl1-mesa-dev \
        libvulkan-dev \
        libwayland-dev \
        libx11-dev \
        libxcursor-dev \
        libxi-dev \
        libxinerama-dev \
        libxcb-render0-dev \
        libxcb-shape0-dev \
        libxcb-xfixes0-dev \
        libxkbcommon-dev \
        libxkbcommon-x11-dev \
        libxrandr-dev \
        pkg-config \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 --fail --silent --show-error https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain 1.95.0 --profile minimal --component rustfmt --component clippy

WORKDIR /workspace

# ---- FFmpeg 7.1 (GPL, no nonfree) bundled into the AppImage ----------------------------
# Codecs from Ubuntu 22.04 packages (x264, x265, SVT-AV1); VAAPI links to the host libva at
# runtime (libva is on the AppImage excludelist and is not bundled).
ARG FFMPEG_VERSION=7.1.5
RUN apt-get update && apt-get install -y --no-install-recommends \
        nasm patchelf libx264-dev libx265-dev libsvtav1enc-dev libnuma-dev \
        libva-dev libdrm-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
# The tarball is downloaded and GPG-verified on the host by scripts/build-appimage.sh.
COPY build/ffmpeg/ffmpeg-${FFMPEG_VERSION}.tar.xz /tmp/ffmpeg.tar.xz
RUN sha256sum /tmp/ffmpeg.tar.xz | tee /opt/ffmpeg-source.sha256 \
    && mkdir -p /tmp/ffmpeg-src && tar -xJf /tmp/ffmpeg.tar.xz -C /tmp/ffmpeg-src --strip-components=1 \
    && cd /tmp/ffmpeg-src \
    && ./configure --prefix=/opt/ffmpeg \
        --enable-gpl --disable-nonfree --disable-autodetect \
        --enable-shared --disable-static --disable-doc --disable-ffplay --disable-debug \
        --enable-libx264 --enable-libx265 --enable-libsvtav1 \
        --enable-vaapi --enable-libdrm --enable-zlib --enable-pthreads \
    && make -j"$(nproc)" && make install \
    && cp COPYING.GPLv2 COPYING.LGPLv2.1 LICENSE.md /opt/ffmpeg/ \
    && echo "${FFMPEG_VERSION}" > /opt/ffmpeg/VERSION \
    && rm -rf /tmp/ffmpeg-src /tmp/ffmpeg.tar.xz

# ---- whisper.cpp (MIT) for local speech recognition -------------------------------------
# Portable CPU build: AVX2/FMA/F16C baseline, no OpenMP, static ggml; only whisper-cli ships.
ARG WHISPER_VERSION=1.9.5
RUN apt-get update && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
COPY build/whisper/whisper.cpp-${WHISPER_VERSION}.tar.gz /tmp/whisper.tar.gz
RUN sha256sum /tmp/whisper.tar.gz | tee /opt/whisper-source.sha256 \
    && mkdir -p /tmp/whisper && tar -xzf /tmp/whisper.tar.gz -C /tmp/whisper --strip-components=1 \
    && cmake -S /tmp/whisper -B /tmp/whisper/build -DCMAKE_BUILD_TYPE=Release \
        -DBUILD_SHARED_LIBS=OFF -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON \
        -DWHISPER_SDL2=OFF -DWHISPER_CURL=OFF -DGGML_OPENMP=OFF -DGGML_NATIVE=OFF \
        -DGGML_AVX=ON -DGGML_AVX2=ON -DGGML_FMA=ON -DGGML_F16C=ON \
    && cmake --build /tmp/whisper/build -j"$(nproc)" --target whisper-cli \
    && mkdir -p /opt/whisper/bin \
    && cp /tmp/whisper/build/bin/whisper-cli /opt/whisper/bin/ \
    && cp /tmp/whisper/LICENSE /opt/whisper/LICENSE \
    && echo "${WHISPER_VERSION}" > /opt/whisper/VERSION \
    && rm -rf /tmp/whisper /tmp/whisper.tar.gz
