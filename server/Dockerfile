# Linux build/run environment for the Aqua Wayland server.
#
# The dev host is macOS, where Smithay/Wayland cannot build. This image gives a
# real Linux userspace (no GPU, no DRM/KMS needed) with:
#   - Rust stable (>= 1.80.1, Smithay's MSRV)
#   - libxkbcommon (required by Smithay's seat/keyboard abstraction)
#   - weston test clients (weston-simple-shm, weston-terminal)
#   - foot, a modern Wayland terminal
#
# The repository is bind-mounted at /work; cargo caches live inside the image's
# user home so rebuilds are incremental.
FROM rust:1-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        protobuf-compiler \
        libxkbcommon-dev \
        libxkbcommon-x11-dev \
        libwayland-dev \
        libwayland-client0 \
        wayland-protocols \
        libgl1-mesa-dri \
        weston \
        foot \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /work

RUN rustup component add rustfmt clippy
