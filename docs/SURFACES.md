# Aqua Surface Model and Frame Format (Phase 3B)

How a Wayland surface tree becomes pixels inside one `UIWindowScene`.

## Surface model

```
RemoteWindow  (xdg_toplevel)   -> one UIWindowScene
   └── RemoteSurfaceTree
        ├── RemoteSurface role=toplevel     (the main wl_surface)
        ├── RemoteSurface role=subsurface   (wl_subsurface)
        └── RemoteSurface role=popup        (xdg_popup)
```

Critical rules:

- `xdg_toplevel != wl_surface`. A window owns a *tree* of surfaces.
- `xdg_popup != UIWindowScene`. A popup is drawn inside its window's scene.

`RemoteSurface` (Rust `domain::RemoteSurface`, Swift `AquaSurface`):

| field | meaning |
|---|---|
| `surface_id` | `surface-N`, stable while the surface lives; namespaced by `ServerSessionID` |
| `window_id` | owning `RemoteWindow` |
| `parent_surface_id` | parent surface (empty for the root toplevel surface) |
| `role` | `toplevel` / `subsurface` / `popup` |
| `x`, `y` | position relative to the parent surface (logical units) |
| `width`, `height` | last committed surface size (logical units) |
| `z` | stacking order among siblings (Wayland tree order; subcompositor has no explicit z) |

Subsurface position comes from Smithay `SubsurfaceCachedState.location`. Z-order
is the traversal order of the surface tree (the subcompositor defines it by
insertion, not by a numeric property).

## Control-plane messages

Added to `protocol/aqua.proto` (`SurfaceInfo`, `SurfaceCreated`, `SurfaceUpdated`,
`SurfaceDestroyed`) and included in `ServerSnapshot.surfaces` so a late client can
rebuild the whole tree. Control-plane still uses Protocol Buffers + the
`[be32 type][be32 len][payload]` framing (unchanged from 3A).

## Data plane

Frames never travel on the control stream and never become `RemoteEvent`s.

```
QUIC connection
├── control stream      (window + surface lifecycle, viewport, focus)
└── surface streams      one unidirectional server->client stream per RemoteSurface
```

Each surface stream is a sequence of:

```text
[be32 header_len][SurfaceStreamHeader (protobuf)][payload_len raw bytes]
```

- First message: `kind = HELLO` (`surface_id`, `window_id`).
- Every later message: `kind = FRAME` with `frame_id`, `width`, `height`,
  `stride`, `format`, `payload_len`, `damage[]`, followed by the raw pixels.

Raw pixels are **not** wrapped in protobuf (uploading megabytes as one protobuf
message adds nothing). The small structured header is protobuf; the body is raw.

## Pixel format

Wayland `WL_SHM_FORMAT_ARGB8888` / `XRGB8888` are packed 32-bit values stored
little-endian, so memory order is **`B, G, R, A`** (BGRA / BGRX) — *not*
`R, G, B, A`. Confirmed and locked by unit tests with red/green/blue/white/
transparent patterns.

CoreGraphics mapping (client `SurfacePixelFormat`):

| Wayland | memory | `CGBitmapInfo` |
|---|---|---|
| ARGB8888 | BGRA | `premultipliedFirst | byteOrder32Little` |
| XRGB8888 | BGRX | `noneSkipFirst | byteOrder32Little` |

`logicalSize`, `bufferSize` and `scale` are kept distinct in the model; phase 3B
uses `scale = 1` but the distinction is preserved for fractional scale later.

## Memory safety

Before any allocation the server validates `width`, `height`, `stride`,
`buffer_len`:

- reject zero dimensions;
- reject `width`/`height` > 8192;
- reject `stride < width * 4`;
- checked `stride * height` (overflow rejected);
- reject frames > 32 MiB;
- reject `stride * height > buffer_len`.

The bytes are **copied** into an owned `Vec<u8>` *inside* `with_buffer_contents`
while the client buffer is pinned. No borrowed pointer to client memory is ever
retained. (A `wl_shm` buffer can be mutated by the client at any time.)

## Backpressure

Latest-frame-wins, bounded by construction, on both ends:

- **Server**: a `tokio::sync::watch` holds at most **one** frame per surface.
  A new frame overwrites the previous one (`send_replace`). No unbounded queue
  can exist. The stream writer sends the latest frame; if several commits happen
  before the network drains, intermediate frames are simply never sent.
- **Client**: `WindowSurfaceModel.store` keeps one frame per surface and discards
  any frame whose `frame_id <= last` (stale/duplicate), counting `droppedFrames`.
- `frame_id` is monotonic per surface.

## Frame lifecycle

```text
wl_surface.commit
   │  (capture BEFORE on_commit_buffer_handler, which takes the buffer)
   ▼
capture_surface_buffer  -> validated, owned Vec<u8>
   ▼
FrameHub.publish         -> watch (latest only), catalog announce
   ▼
per-surface QUIC uni stream writer
   ▼
iPad inboundStream -> SurfaceStreamHeader + payload
   ▼
WindowSurfaceModel.store -> composition() -> CGImage
   ▼
RemoteWindowViewController image view
   ▼
FramePresented { surface, frame_id }  -> server (feedback)
```

## Composition

`WindowSurfaceModel.composition()` renders the root surface's logical size and
draws every surface at its **absolute** position (parent offsets accumulated),
root first then by z-order, into one `CGImage`. Subsurfaces and popups are drawn
inside the same image — never as separate scenes. Wayland's y-down and
CoreGraphics' y-up are reconciled when placing each image.

## What we learned from real Wayland buffers

- **`on_commit_buffer_handler` consumes the buffer.** Smithay's commit helper
  calls `SurfaceAttributes.buffer.take()`. Capturing *after* it yields nothing;
  the capture must happen at the very start of `CompositorHandler::commit`,
  before that call. This was the single non-obvious bug of the phase.
- Real shm frames observed in-container:
  - `weston-simple-shm` → 250×250, stride 1000, format XRGB8888, 250 000 B,
    1 damage rect, first pixel `ff ff ff ff`.
  - `weston-simple-damage` → 300×200, stride 1200, format ARGB8888, 240 000 B,
    2 damage rects.
- Clients commit many times per second; the control stream would be swamped if
  commits were forwarded. Keeping frames off the control plane is essential.
- `SubsurfaceCachedState` exposes only `location`; there is no z-order property.
  Z is tree order.
- Damage is available (`SurfaceAttributes.damage`, `Damage::Surface`/`Buffer`);
  phase 3B sends it as metadata but still transmits full frames.

## Limitations (phase 3B)

- **Full-frame copy per commit.** No damage-region delta, no compression. This is
  deliberate for correctness; `weston-simple-shm` at 250×250 is trivial.
- **No dmabuf.** Only `wl_shm` (`ARGB8888`/`XRGB8888`). GPU clients (GTK4 GL,
  Firefox) are out of scope.
- **Frame callbacks still use the phase-2 60 Hz clock.** `FramePresented` is
  transported and logged, but `wl_surface.frame` is not yet driven by it, to
  avoid an initial deadlock.
- **Physical iPad display: VERIFIED** (see PHASE3B.md): real `wl_shm` pixels from
  `weston-terminal` were observed on a physical iPad mini A17 Pro via
  Tailscale. (The earlier "NOT VERIFIED" note referred to the pre-physical setup,
  where the dev host could not forward UDP from the simulator to the Linux
  container.)
- No cursor, no clipboard, no drag & drop.
