#!/usr/bin/env bash
#
# Phase 2 end-to-end demo.
#
# Runs the Aqua Wayland server and several real Wayland clients inside a Linux
# container (the dev host may be macOS), then drives the control channel:
#   - two `weston-simple-shm` windows (same app_id, distinct RemoteWindowIds)
#   - a subsurface client (weston-subsurfaces)
#   - a dedicated popup client (xdg_popup, must NOT create a window)
#   - resize / focus / pointer / key commands
#
# Usage:  server/scripts/demo.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> Building Linux dev image"
docker build -t aqua-server-dev "$ROOT"

echo "==> Running demo"
docker run --rm \
  -v "$ROOT":/work \
  -v aqua-target:/target \
  -v aqua-cargo:/usr/local/cargo/registry \
  -w /work \
  -e CARGO_TARGET_DIR=/target \
  -e RUST_LOG=aqua=debug \
  aqua-server-dev \
  bash -c '
    set -e
    show() { sed -e "s/\x1b\[[0-9;]*m//g"; }

    cargo build --quiet

    # Build the popup client against the system xdg-shell protocol.
    XML=/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml
    wayland-scanner client-header "$XML" /tmp/xdg-shell-client-protocol.h
    wayland-scanner private-code  "$XML" /tmp/xdg-shell-protocol.c
    cc /work/scripts/popup_client.c /tmp/xdg-shell-protocol.c \
       -I/tmp $(pkg-config --cflags --libs wayland-client) -o /tmp/popup_client

    export XDG_RUNTIME_DIR=/tmp/aqua-runtime
    mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"

    rm -f /tmp/aqua-ctrl; mkfifo /tmp/aqua-ctrl
    /target/debug/aqua-server < /tmp/aqua-ctrl 2>&1 | show &
    SERVER=$!
    exec 3>/tmp/aqua-ctrl
    sleep 1

    export WAYLAND_DISPLAY=wayland-aqua

    echo "--- two windows of the same application ---"
    weston-simple-shm & sleep 1
    weston-simple-shm & sleep 1

    echo "--- a window with subsurfaces ---"
    weston-subsurfaces & sleep 2

    echo "--- control channel: list / focus / resize / input ---"
    echo "list" >&3
    echo "focus window-1" >&3
    echo "resize window-1 800 600" >&3
    echo "pointer move 100 100" >&3
    echo "pointer button left down" >&3
    echo "pointer button left up" >&3
    echo "key A down" >&3
    echo "key A up" >&3
    sleep 2

    echo "--- popup client (xdg_popup must not create a window) ---"
    /tmp/popup_client
    sleep 1

    echo "quit" >&3
    exec 3>&-
    wait "$SERVER" 2>/dev/null || true
  '
