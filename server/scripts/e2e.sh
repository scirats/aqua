#!/usr/bin/env bash
#
# Phase 3A end-to-end: Aqua server (Wayland + QUIC) with real Wayland clients
# and a real QUIC client, all on localhost inside the Linux dev container.
#
# This proves the full pipeline:
#   weston client -> xdg_toplevel -> RemoteWindow -> Aqua Protocol -> QUIC -> client
# and back: client ViewportChanged -> QUIC -> xdg_toplevel.configure -> ack.
#
# Usage: server/scripts/e2e.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

docker build -t aqua-server-dev "$ROOT"

docker run --rm \
  -v "$ROOT/..":/work \
  -v aqua-target:/target \
  -v aqua-cargo:/usr/local/cargo/registry \
  -w /work/server \
  -e CARGO_TARGET_DIR=/target \
  aqua-server-dev \
  bash -c '
    set -e
    cargo build --quiet --bin aqua-server --example quic_client --example frame_client

    export XDG_RUNTIME_DIR=/tmp/aqua-runtime
    mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
    export AQUA_BIND=127.0.0.1:52420
    export RUST_LOG=aqua=debug

    /target/debug/aqua-server > /tmp/server.log 2>&1 &
    SERVER=$!
    sleep 1.5

    export WAYLAND_DISPLAY=wayland-aqua
    weston-simple-shm >/dev/null 2>&1 & sleep 0.8
    weston-simple-damage >/dev/null 2>&1 & sleep 1.5

    echo "================ CONTROL + WINDOWS (quic_client) ================"
    /target/debug/examples/quic_client 127.0.0.1:52420 window-1

    echo "================ SURFACE FRAMES (frame_client) =================="
    /target/debug/examples/frame_client 127.0.0.1:52420

    sleep 1
    kill "$SERVER" 2>/dev/null || true

    echo "================ SERVER ===================="
    sed -e "s/\x1b\[[0-9;]*m//g" /tmp/server.log \
      | grep -E "handshake|snapshot|viewport|configure|ack_configure|window.created|frame.captured"
  '
