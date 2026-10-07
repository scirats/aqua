# Aqua — GPU pipeline investigation (Fase 3C.0)

> **Update — real hardware (native Linux + AMD).** The environment described in
> §1 below (macOS + Colima, **no** GPU) was the original one. The server now runs
> on **native Linux with a real AMD GPU**, so those "no GPU / NOT AVAILABLE"
> conclusions no longer apply. The measured facts of the new environment are in
> §0. The rest of this document is kept as the original investigation.

## 0. Measured on the real target (native Linux + AMD)

- Ubuntu 26.04 x86_64, AMD Ryzen 7 PRO 5850U; GPU AMD Radeon (Renoir), driver
  `amdgpu`, node `/dev/dri/renderD128`; Mesa 26.0.8, libva 1.23.
- **VA-API** (`vainfo`, renderD128): encode **H.264** ConstrainedBaseline/Main/High
  and **HEVC Main/Main10** (`VAEntrypointEncSlice`); decode H.264/HEVC Main/Main10/VP9.
- **Encoder real**: `ffmpeg 8.0.1` + `hevc_vaapi`/`h264_vaapi`,
  `-rc_mode CBR -b:v 12M -g 120 -bf 0`, Annex-B with AUD. **VERIFIED**: HEVC
  decoded on a physical iPad.
- **dmabuf / EGL**: `EGL 1.5 Mesa`; `EGL_EXT_image_dma_buf_import` and
  `..._modifiers` = yes; **67 importable formats**.
  - Relevant: **AR24/XR24/AB24/XB24** (RGBA/BGRA 8-bit) with 10 modifiers each;
    **NV12/P010** with 6 modifiers each.
  - AMD modifier e.g. `0x020000044051ba01` decodes (via `drm_fourcc.h` macros) to
    vendor AMD(`0x02`), TILE_VERSION=1 (GFX9), TILE=26
    (`AMD_FMT_MOD_TILE_GFX9_64K_D_X`), **DCC=1**, DCC_INDEPENDENT_64B=1,
    PIPE_XOR_BITS=2, PIPE=2; plus `LINEAR`(0).
- **Observed real GPU client** (`weston-simple-egl`, `xdg_toplevel`): dmabuf
  **AR24**, modifier `0x020000044051ba01`, **2 planes**:
  - plane 0: `offset=0`, `stride=1024` (250 px → 1000 B, padded to 256 B);
  - plane 1 (DCC metadata): `offset=262144` (=256 KiB colour, 1024×256), `stride=512`.
  - Sync: **implicit** (`wl_surface.attach`+`commit`, no explicit fences) →
    `SyncState::Unknown`.
- Requirement for Mesa to use dmabuf: the `zwp_linux_dmabuf_v1` default feedback
  must carry the **real `main_device`** (render node `dev_t`). Without it Mesa
  fails with `fd -1`.
- **Copy count**: `NOT MEASURED` yet (the real EGLImage→VA import and the encode
  from dmabuf is Milestone 2). The current encoder path is
  **SHM →(CPU BGRA)→ ffmpeg → VA-API** (an upload, not a readback).

### M2/M3 achieved (dmabuf → sidecar → iPad, 0 CPU copies)

The GPU-native path is now **VERIFIED** on the physical iPad:

```
weston-simple-egl (xdg_toplevel, linux-dmabuf AR24 LINEAR, 1 plane)
  → Aqua: SCM_RIGHTS to the `aqua-va-encode` sidecar
      → DRM_PRIME_2 import → VPP ARGB→NV12 (GPU) → hevc_vaapi (libav, same VADisplay)
  → Annex-B CONFIG(VPS/SPS/PPS) + FRAME(access unit, keyframe flag)
  → VideoHub → QUIC → VideoToolbox on the iPad → UIWindowScene
```

- `zwp_linux_dmabuf_v1` advertises only **LINEAR** modifiers so GPU clients
  allocate importable buffers (the AMD tiling+DCC modifiers are unproven for
  VA import).
- Pixel copies on the CPU: **0** (plane fds travel via `SCM_RIGHTS`; import, VPP
  and encode run on the GPU). The only CPU-copy path that remains is the legacy
  SHM fallback (upload).
- The sidecar protocol lives in `server/src/gpu/sidecar.rs`.
- Client for text clarity: the Ubuntu `foot` package is built **without EGL**
  (pixman/`wl_shm` only), so it exercises the SHM path, not dmabuf; a GPU text
  client (EGL) or Firefox is still pending (Milestone 4).


---

This document is the **mandatory investigation** that precedes any encoder work.
Its rule is simple: **everything here is either measured on real hardware or
explicitly marked `NOT MEASURED` / `NOT AVAILABLE`.** No invented RTT, CPU, GPU,
throughput or latency numbers.

Status legend used throughout Aqua docs:

| label | meaning |
|---|---|
| **VERIFIED** | observed on the hardware described, reproducible |
| **NOT AVAILABLE** | the hardware/API is absent from the environment used |
| **NOT MEASURED** | the hardware/API may exist but no number was taken here |
| **RESEARCHED** | taken from vendor/driver documentation, not measured here |

---

## 1. What this environment actually is (measured)

The Aqua dev host is **not** the target Linux box. It is:

```
host  : macOS 25.6.0, Apple M4, arm64
Linux : Colima VM (Virtualization.Framework), Ubuntu 24.04, kernel 6.8.0-100-generic, aarch64
```

Commands run and their real output:

```console
$ uname -m                 # inside the Colima VM
aarch64

$ ls /sys/class/drm        # inside the Colima VM
version                    # <- no card0, no renderD128

$ ls /dev/dri /dev/nvidia* # inside the Colima VM and inside aqua-server-dev
ls: cannot access '/dev/dri': No such file or directory
ls: cannot access '/dev/nvidia*': No such file or directory

$ lsmod | grep -iE 'nvidia|amdgpu|i915|virtio_gpu|drm'
(none)

$ which nvidia-smi vaapi vainfo gst-inspect-1.0
(none)

$ ls /usr/share/glvnd/egl_vendor.d   # inside the container
50_mesa.json                          # Mesa present, but no DRM render node
```

### Conclusion of the detection (the important part)

**There is no GPU-accelerated path in this environment.**

- The host is an **Apple M4 (arm64)**, so the expected **RTX 2060 cannot be
  present**: it is an x86-64 discrete PCIe card and there is no PCIe passthrough
  into the Colima VM.
- The Linux VM has **no DRM render node** (`/dev/dri` absent) and **no GPU
  kernel module**.
- The `aqua-server-dev` container has **Mesa** (so an EGL/GL software stack is
  installable) but **no device node to import from**.

Consequences, stated without euphemism:

| Item | Result here |
|---|---|
| GPU | `NOT AVAILABLE` (Apple M4 host; no discrete GPU passed to Linux) |
| Driver / kernel | kernel 6.8 aarch64; **no** nvidia/amdgpu/i915 loaded |
| Wayland / DRM | Wayland **frontend** works (Smithay headless, already VERIFIED in 3A/3B); **DRM/KMS** `NOT AVAILABLE` |
| NVENC capabilities | `NOT AVAILABLE` |
| Supported codecs (hw) | `NOT MEASURED` (no encoder visible) |
| Supported formats/modifiers | `NOT MEASURED` (no render node) |
| iPad physical device | `NOT AVAILABLE` in this environment (see 3A.1/3B) |

Every performance and capability number that depends on the RTX 2060 or the
iPad GPU is therefore **`NOT MEASURED`** in this document. The design below is
built from **`RESEARCHED`** vendor/driver facts and is written so it can be
validated later on real hardware without redesign.

---

## 2. Target hardware (RESEARCHED, not measured here)

Because the phase names an RTX 2060 and an iPad mini A17 Pro, their *published*
capabilities are recorded so the pipeline choice is defensible — but they are
**not** confirmed on this machine.

### 2.1 Encode side — NVIDIA RTX 2060 (Turing, TU106)

- NVENC generation: **7th** (Turing). `RESEARCHED`.
- Hardware **encode**: **H.264/AVC** and **HEVC/H.265** (4:2:0, 8-bit and
  10-bit/P010). `RESEARCHED`.
- Hardware **AV1 encode**: **not supported** on Turing; AV1 encode requires Ada
  (RTX 40 series). `RESEARCHED`.
- NVENC **concurrent sessions**: consumer drivers historically limit this
  (NVIDIA raised the limit over time); the exact limit on a given driver is
  `NOT MEASURED`. This is a decisive input for Model A vs Model B (§5).

### 2.2 Decode side — iPad mini A17 Pro (A17 Pro SoC)

- VideoToolbox hardware decode for **H.264** and **HEVC** is available on all
  modern Apple SoCs. `RESEARCHED`.
- **AV1 decode** is present on A17 Pro / 3rd-gen Apple silicon. `RESEARCHED`.
- Concurrent **VideoToolbox** decoder sessions: the practical limit is
  `NOT MEASURED` (Apple does not publish a hard number; it depends on SoC,
  resolution and bitrate). Another decisive input for Model A vs Model B (§5).
- Presentation APIs to evaluate: `AVSampleBufferDisplayLayer`, `CVPixelBuffer` +
  `Metal`, and `AVPlayerLayer` (§6). Which is best is `NOT MEASURED`.

---

## 3. Smithay API research (VERIFIED against smithay 0.7.0 source)

The pinned dependency is `smithay 0.7` with
`default-features = false, features = ["wayland_frontend", "desktop"]`.
Inspecting `smithay-0.7.0/src`:

- `wayland::dmabuf` (protocol `zwp_linux_dmabuf_v1`) is available under the
  `wayland_frontend` feature. It provides:
  - `DmabufState`, `DmabufGlobal`;
  - `DmabufHandler` (`dmabuf_state`, `dmabuf_imported`);
  - `delegate_dmabuf!` (wires all globals/dispatches);
  - `ImportNotifier` (`successful` / `failed` / `invalid_format` / …);
  - `get_dmabuf(&WlBuffer) -> &Dmabuf`.
- `backend::allocator::dmabuf::Dmabuf` is **not** feature-gated (it only needs
  `drm-fourcc`, a mandatory dep). It exposes `num_planes`, `handles()` (fds),
  `offsets()`, `strides()`, `has_modifier()`, `node()`, and `map_plane()`
  (the latter is a **CPU mmap** — the thing we must NOT use on the normal path).
- `Format = drm_fourcc::DrmFormat { code: DrmFourcc, modifier: DrmModifier }`.
- `create_global(display, formats)` produces a **v3** global; the v4 feedback
  path (`create_global_with_default_feedback`) needs a `DmabufFeedbackBuilder`
  with a `main_device: dev_t` — which **requires a real DRM node**. Without a
  render node we cannot honestly build feedback.

**Decision taken from this:** Aqua implements `zwp_linux_dmabuf_v1` through
Smithay's `DmabufHandler`, but **only creates the global when a
`GpuBufferImporter` reports at least one importable `(format, modifier)` pair.**
With no GPU (this environment) the global is *not* created, and no formats are
advertised — exactly the rule "do not announce combinations you cannot import".

### 3.1 What we did NOT do

- We did **not** hand-roll `zwp_linux_dmabuf_v1`; Smithay already does it.
- We did **not** enable `backend_drm` / `backend_gbm` / `renderer_gl`; Aqua is
  not a renderer and must keep building headless.
- We did **not** use `Dmabuf::map_plane` on the normal path (CPU readback).

---

## 4. NVIDIA import path research

Two candidate routes from a Wayland `linux-dmabuf` buffer to NVENC:

### Route 1 — EGL import → CUDA registration → NVENC (`RESEARCHED`)

```
dmabuf fd + modifier
        │  EGL_EXT_image_dma_buf_import[_modifiers]
        ▼
  EGLImage (GL)
        │  cuGraphicsEGLRegisterImage
        ▼
  CUdeviceptr (CUDA)
        │  NV_ENC (nvEncodeAPI), input as CUDA device ptr / registered resource
        ▼
     bitstream
```

This is the canonical zero-copy route on Linux + NVIDIA: the GPU reads the
dmabuf directly and NVENC consumes the CUDA resource; no planes are mapped by
the CPU. It needs `libEGL` + `libcuda` + `nvEncodeAPI` and a real DRM node.

### Route 2 — GStreamer `nvv4l2`/`nvh264enc`/`nvhevcenc` (`RESEARCHED`)

A GStreamer element can accept a dmabuf-backed input (e.g. via
`GstGLMemory`/`GstDmaBufMemory`) and hand it to NVENC. Convenient, but it pulls
in a large plugin stack and makes the accounting of copies harder to prove.
Rejected as the *primary* design; may be used as a local bring-up aid.

### The path we must avoid

```
dmabuf → CPU readback (map_plane) → Vec<u8> → upload → NVENC
```

It is a legitimate **debug/fallback** path (useful to validate the encoder with
synthetic frames), but it must never be the normal path.

**Copy accounting (design intent, `NOT MEASURED`):**

| stage | Route 1 (target) | Route 2 fallback (must avoid normally) |
|---|---|---|
| import | 0 CPU copies (fd handoff) | 0 if kept as dmabuf, ≥1 if mapped |
| color convert | GPU (EGL/CUDA) | CPU or GPU |
| encode input | 0 CPU copies | 1+ (readback + upload) |
| bitstream out | 1 (encoder → byte buffer) | same |

Because we cannot run Route 1 here, the number of copies is `NOT MEASURED`; the
design only guarantees there is **no `map_plane` on the normal path**.

---

## 5. Architecture decision: A (per-surface) vs B (per-window) vs C (hybrid)

Reminder of the two different concepts:

- **Semantic unit** = `RemoteWindow` → `RemoteSurfaceTree` (root + subsurfaces +
  popups). Aqua keeps `RemoteSurface`, `SurfaceCreated/Updated/Destroyed`
  regardless of transport.
- **Encoding unit** may be different. It does **not** have to equal the semantic
  unit.

### Model A — one encoder/stream per `RemoteSurface`

```
RemoteWindow → { root: encoder, subsurface: encoder, popup: encoder } → N decoders → composition
```

- Pros: no GPU composition server-side; each surface is independent; closest to
  the existing "one QUIC stream per surface" shape.
- Cons: **N NVENC sessions and N VideoToolbox decoders per window**. Concurrent
  NVENC session limits (consumer NVIDIA) and VideoToolbox session pressure make
  this dangerous for multi-window. Small subsurfaces/popups waste sessions and
  bitrate. Text clarity of the pieces is *not* improved.

### Model B — GPU-compose the surface tree, one encoder/stream per `RemoteWindow`

```
RemoteWindow tree → GPU composition (Smithay renderer / dedicated GPU op)
                 → ONE encoder → ONE decoder → UIWindowScene
```

- Pros: **one NVENC session and one decoder per window**, constant regardless of
  surface count; matches `xdg_toplevel ↔ UIWindowScene`; resize is a single
  reconfiguration; popups/subsurfaces composited with the same GPU that owns
  the dmabufs.
- Cons: requires a real GPU compositor step before encode (the exact step that
  is `NOT AVAILABLE` here); loses per-surface independence (not needed since the
  semantic tree is preserved in the control plane).

### Model C — hybrid

- Only justified if a *semantically useful overlay* needs independent transport
  (e.g. a future cursor/video layer). No such need exists yet.

### Decision

**Model B is the chosen architecture** (one encoded stream per `RemoteWindow`,
GPU-composited from the surface tree), with Model A retained as a fallback only
if per-window GPU composition proves impossible on the target box.

Rationale is **resource-bounded and protocol-shaped**, not measured:

1. NVENC concurrent sessions and VideoToolbox decoders are scarce; per-window
   makes session count a function of *windows*, not *surfaces*.
2. The domain already models one `UIWindowScene` per `RemoteWindow`; one
   encoded stream per window keeps the transport and the presentation model
   aligned.
3. Popups/subsurfaces are composited *inside* the window (already the rule in
   3B), so there is no place for them to become independent streams.

The supporting numbers (encoder/decoder session limits, GPU utilization, VRAM,
bandwidth, latency, composition cost) are **`NOT MEASURED`** and are the first
thing to collect on real hardware before this becomes final.

**Important:** choosing Model B for video does **not** remove
`RemoteSurface`/`SurfaceCreated`/`SurfaceUpdated`/`SurfaceDestroyed`, and does
**not** change the existing per-surface SHM data plane. The semantic tree and
the SHM fallback stay.

---

## 6. Codec choice (RESEARCHED)

Evaluation against the actual requirement — an *interactive Linux UI*, not a
video player:

| codec | RTX 2060 encode | A17 Pro decode | UI/text suitability | verdict |
|---|---|---|---|---|
| **H.264** | yes (NVENC 7th gen) | yes | good, very mature low-latency tooling | **fallback** |
| **HEVC** | yes (NVENC 7th gen) | yes | better compression at same quality; good text at high bitrate | **primary** |
| **AV1** | **no** (Turing lacks AV1 encode) | yes (A17 Pro decode) | excellent ratio, but cannot encode on RTX 2060 | **not chosen** |

**Decision: HEVC primary, H.264 fallback.** AV1 is excluded because the RTX 2060
cannot encode it; it is not chosen "for ratio". Exact bitrate/GOP/profile are
`NOT MEASURED`; the intended starting point is documented in `docs/VIDEO.md`.

---

## 7. Synchronization (strategy; behaviour NOT MEASURED)

A dmabuf arriving on the wire does **not** mean its contents are ready. The
client may still be writing to it; Wayland clients commonly use an implicit
`dma_fence` and/or explicit sync (`zwp_linux_dmabuf` feedback /
`wp_linux_drm_syncobj` on newer stacks).

Strategy:

1. **Implicit sync first.** When Aqua imports/hands a dmabuf to the GPU encoder,
   the GPU operation must wait on the buffer's implicit fence. On the EGL/CUDA
   route the driver’s import + `cuStreamSynchronize`/`EGL sync` covers this; on a
   raw fd handoff the encoder backend must wait on the `dma_fence`.
2. **Explicit sync later.** `linux-drm-syncobj-v1` explicit fences are the
   correct long-term model; Smithay exposes `wayland::drm_syncobj` only under
   `backend_drm`, which we do **not** enable headless. Deferred.
3. **Never consume a buffer before its fence signals.** Documented as a hard
   rule; a CPU `map_plane` would silently ignore this and is another reason it
   is not the normal path.

Because there is no GPU here, no fence was exercised: `NOT MEASURED`.

---

## 8. What was actually built in 3C (code)

Even without hardware, the *boundary* is real and testable:

- `server/src/gpu/` — neutral traits `GpuBufferImporter`, `GpuFrame`,
  `VideoEncoder`, `VideoEncoderSession`, `EncodedFrame`, plus `VideoCodec`
  and a headless `NullGpuImporter` / `NullVideoEncoder`.
- `server/src/wayland/dmabuf.rs` — `zwp_linux_dmabuf_v1` via Smithay
  (`DmabufHandler` + `delegate_dmabuf!`), global created **only** when the
  importer advertises formats.
- `protocol/aqua.proto` — `VideoCodec`, `VideoChroma`, `WindowVideoConfig`,
  `RequestKeyframe`, `WindowVideoStreamHeader` (the last two with the SHM
  `SurfaceStreamHeader` kept untouched).
- Capability negotiation advertises `CAPABILITY_SURFACE_SHM` always and
  `CAPABILITY_SURFACE_VIDEO` **only when a real encoder session exists**.

No NVENC/CUDA/EGL/Vulkan/Smithay type crosses into `RemoteWindow` /
`RemoteSurface` / the Aqua protocol domain.

The default build ships the **null importer and null encoder**, so a headless
run advertises SHM and does not pretend to have video. On a real RTX 2060 box,
a concrete importer/encoder is dropped in behind the same traits.

---

## 9. Honest summary for 3C.0

| Question from the phase | Answer here |
|---|---|
| GPU | `NOT AVAILABLE` (Apple M4 host; no GPU in the Colima VM) |
| driver/kernel | kernel 6.8 aarch64, no GPU module |
| Wayland/DRM support | Wayland frontend works; DRM/KMS `NOT AVAILABLE` |
| NVENC capabilities | `NOT AVAILABLE` |
| supported codecs | `NOT MEASURED` |
| supported formats | `NOT MEASURED` |
| supported modifiers | `NOT MEASURED` |
| dmabuf → GPU → NVENC without readback | implemented behind neutral traits; **not executed** |
| real copy count | `NOT MEASURED` |

Physical verification (Milestones 1–5 in `PHASE3C.md`) requires a Linux box with
the RTX 2060 plus a physical iPad and is **NOT DONE** in this environment.
