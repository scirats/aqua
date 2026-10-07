# Aqua — Video data plane (Fase 3C design)

Companion to `docs/GPU_PIPELINE.md` (hardware investigation) and
`docs/SURFACES.md` (surface tree / SHM plane). This file specifies the **video
data plane** and its configuration. It separates what is *decided from research*
from what is *measured on hardware* (almost nothing here — see the status
legend in `GPU_PIPELINE.md`).

The rule from the phase stands: **the encoding unit may be a `RemoteWindow`
even though the semantic unit stays `RemoteSurfaceTree`.**

---

## 1. Codec and configuration

| field | choice | basis |
|---|---|---|
| primary codec | **HEVC (H.265)** | RTX 2060 encodes it; A17 Pro decodes it efficiently (`RESEARCHED`) |
| fallback codec | **H.264 (AVC)** | widest support, lowest risk (`RESEARCHED`) |
| AV1 | not chosen | RTX 2060 has no AV1 encoder (`RESEARCHED`) |
| profile | HEVC Main (8-bit) / Main10 (10-bit); H.264 High | `RESEARCHED`; concrete profile `NOT MEASURED` |
| bit depth | 8-bit default, 10-bit optional | `NOT MEASURED` |
| chroma | 4:2:0 (`VideoChroma::NV12`); optional 4:2:0 10-bit (`P010`) | `RESEARCHED` |
| GOP | long, keyframe-on-demand | the encoder emits keyframes only at start, on loss/reset, or on `RequestKeyframe` |
| B-frames | **0** (disabled) | UI latency: no reordering delay |
| lookahead | **off** | UI latency |
| rate control | low-latency CBR or capped VBR | UI needs stable latency more than ratio; exact mode `NOT MEASURED` |
| low-latency knob | `low_latency = true` in config | maps to NVENC "low latency" preset / no B / no lookahead |

The wire config type is `WindowVideoConfig` (`protocol/aqua.proto`). Its fields
are informational to the client; the authoritative stream configuration is
`WindowVideoStreamHeader{kind=CONFIG}` on the data plane.

**Why HEVC and not AV1:** choosing by "theoretical ratio" would pick AV1, but the
target encoder *cannot produce AV1*. HEVC gives the best ratio that the named
hardware can actually encode.

---

## 2. Data plane shape: one encoded stream per `RemoteWindow`

```
RemoteWindow (xdg_toplevel)
   └── RemoteSurfaceTree (root + subsurfaces + popups)
             │  GPU composition (Model B; see GPU_PIPELINE.md §5)
             ▼
        one hardware encoder session
             │  encoded access units
             ▼
        one unidirectional QUIC stream  ──►  one VideoToolbox decoder
```

This is **separate from** the SHM plane, which remains **one stream per
`RemoteSurface`** (`SurfaceStreamHeader`, kinds HELLO/FRAME). Both coexist:

```
QUIC connection
├── control stream                 (handshake, windows, surfaces, viewport, video config)
├── surface SHM streams            one per RemoteSurface   (raw pixels, latest-wins)   [3B, kept]
└── window video streams           one per RemoteWindow    (HEVC/H.264, inter-frame)     [3C]
```

A client that cannot do video simply uses the SHM streams. Capability bits
decide: `CAPABILITY_SURFACE_SHM` vs `CAPABILITY_SURFACE_VIDEO`.

### Video stream framing (mirrors the SHM plane)

```
[be32 header_len][WindowVideoStreamHeader (protobuf)][payload_len raw bytes]
```

- `kind = HELLO`: identifies the `window_id`.
- `kind = CONFIG`: codec, chroma, width, height. Sent before the first picture
  and again on resize / codec change. `codec_config = true` marks a
  parameter-set-only payload (VPS/SPS/PPS), which carries no picture.
- `kind = FRAME`: one access unit, `frame_id` (monotonic per window),
  `keyframe` flag, `pts_us`, `payload_len`, then the raw bitstream.

Bitstreams are **never** wrapped in protobuf. The small structured header is
protobuf; the body is raw bytes.

---

## 3. Backpressure: inter-frame codecs cannot "drop any frame"

The SHM plane uses *latest-frame-wins* because every frame is independently
decodable. That is **wrong for video**: dropping a P/B frame corrupts the
picture until the next keyframe.

Design:

- **Bounded queues only.** Server and client each keep a small bounded queue
  (size ≥ 1, small). No unbounded backlog is ever allowed.
- **Keyframe awareness.** A conforming encoder emits a keyframe at stream start
  and on request. The server tracks the last keyframe sent.
- **Drop policy.** When a bounded queue is full, drop a frame only if a *newer
  decodable* frame can replace it: i.e. drop to the most recent keyframe-aligned
  point, never an arbitrary P-frame in the middle of a GOP. For latency, prefer
  to keep the newest frames; when the client signals it is behind, the server
  **forces a keyframe**.
- **`RequestKeyframe`** (client → server): sent when the decoder was reset,
  after a dropped GOP, after a resize, or after reconnection. The server calls
  `VideoEncoderSession::request_keyframe()`.
- **Continuity rule.** A decoder must never receive a P-frame before the
  matching keyframe. The protocol makes this explicit with the `keyframe` flag.

The SHM plane keeps its existing `watch`-based latest-wins path unchanged.

---

## 4. Resize without restarting the session

```
UIWindowScene resize
   ▼
ViewportChanged (existing control-plane message)
   ▼
xdg_toplevel.configure → ack_configure → client redraws
   ▼
new dmabuf size (GPU app)
   ▼
GPU composition produces the new size
   ▼
VideoEncoderSession::reconfigure(new_size)  →  emits a fresh keyframe
   ▼
WindowVideoStreamHeader{kind=CONFIG} + keyframe
   ▼
VideoToolbox reconfigure (new session / new CMFormatDescription)
   ▼
correct image
```

No session restart: only the encoder and decoder are reconfigured. The existing
3A/3B resize path (`viewport → configure → ack → commit`) is reused verbatim.

---

## 5. iPad decode + presentation (strategy; NOT MEASURED)

Candidates evaluated (all `RESEARCHED`; none benchmarked here):

| approach | latency | composition | stale-frame drop | resize | multi-window | notes |
|---|---|---|---|---|---|---|
| `AVSampleBufferDisplayLayer` | low | limited | needs manual flush | re-create | one layer per window | simplest; may lack the control we need |
| `CVPixelBuffer` + `Metal` (decode → texture) | lowest | full | full control | easy | one view per window | most control, most code |
| `AVPlayerLayer` | higher | limited | handled | awkward | awkward | **not suitable** for interactive UI |

Decision: implement a `VideoDecoder` abstraction and start with
`AVSampleBufferDisplayLayer` for bring-up, moving to `CVPixelBuffer`+Metal only
if measurement shows it is needed. **Do not assume Metal is required; do not
assume `AVSampleBufferDisplayLayer` is sufficient — measure.** Which wins is
`NOT MEASURED` here because there is no hardware.

What the client must do regardless:

- Feed the decoder with `keyframe` awareness; discard until the first keyframe.
- On `CONFIG` change (including resize), rebuild the format description/session.
- Drop stale frames using the same bounded/bounded-keyframe policy as the server.
- Report `FramePresented` for pacing feedback (see §7).

---

## 6. Frame callbacks (do not couple `wl_surface.frame` to the RTT)

3B left the 60 Hz clock in place. With video, comparing the options:

- **A. callback on remote presentation** (`FramePresented`): most accurate, but
  makes the Wayland client's render loop depend on network RTT — usually wrong
  for an interactive UI.
- **B. callback when Aqua accepts the frame** (import/encode success): decouples
  the client from RTT; matches the local compositor timing.
- **C. hybrid pacing**: accept-based under normal conditions, presentation-based
  when the link is fast and idle.

Decision: implement **B as the default** and keep **C** behind measurement.
`FramePresented` continues to be transported for metrics. The exact policy is
`NOT MEASURED`.

---

## 7. Metrics to collect on real hardware (all `NOT MEASURED`)

capture/import latency, encode latency, decode latency, presentation latency,
end-to-end latency, bitrate, FPS, GPU utilization, VRAM, Linux CPU, iPad CPU,
frames produced/encoded/dropped, keyframes, encoder sessions, decoder sessions.
None are invented here.

---

## 8. UI is not video: the quality bar

Visual validation must use **terminal text, small fonts, editor-like content,
thin lines, icons, high-contrast UI, scrolling and animation** — not just
animations. The first GPU client is a minimal known-pattern Wayland/EGL/dmabuf
client (isolate the pipeline), then a real GPU app, then Firefox (only if the
stack allows). Text clarity under HEVC is a first-class acceptance criterion,
and is `NOT MEASURED` until seen on the physical iPad.
