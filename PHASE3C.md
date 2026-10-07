# Aqua — Fase 3C: GPU pipeline + hardware video

Documentos relacionados: [`docs/GPU_PIPELINE.md`](docs/GPU_PIPELINE.md) (investigación 3C.0),
[`docs/VIDEO.md`](docs/VIDEO.md) (diseño del plano de vídeo), [`docs/SURFACES.md`](docs/SURFACES.md),
[`docs/PROTOCOL.md`](docs/PROTOCOL.md), [`docs/TRANSPORT.md`](docs/TRANSPORT.md),
schema [`protocol/aqua.proto`](protocol/aqua.proto).

Status legend: **VERIFIED** / **NOT AVAILABLE** / **NOT MEASURED** / **RESEARCHED**
(ver `docs/GPU_PIPELINE.md`).

---

## Resumen honesto de una línea

En **este entorno no hay GPU** (`Apple M4` + VM Colima aarch64 sin `/dev/dri`
ni NVIDIA) y **no hay iPad físico**, así que los hitos físicos de 3C quedan
**NOT VERIFIED / NOT MEASURED**. Lo que sí se construyó y se probó es la
**frontera neutral** (protocolo de vídeo, `GpuBufferImporter`/`VideoEncoder`,
`zwp_linux_dmabuf_v1` vía Smithay, negociación de capacidades) y la regresión
del camino `wl_shm` de 3B sigue **VERIFIED**.

---

## 1. Limpieza documental (hecho)

`PHASE3B.md` afirmaba en secciones posteriores que el display físico y Tailscale
estaban `NOT VERIFIED`, contradiciendo el encabezado `VERIFIED` de la validación
física real (iPad mini A17 Pro + Tailscale). Corregido usando **solo** los
resultados reales:
- `PHASE3B.md` §"Qué falló / no se pudo verificar" y §"Reporte solicitado"
  (ítems 1–4): display físico y Tailscale → **VERIFIED**; RTT → **NOT MEASURED**.
- `docs/SURFACES.md`: nota "Physical iPad display is NOT VERIFIED" → **VERIFIED**.
- `PHASE3A.md` §"Qué falló" (ítems 2–3): marcados como superados por 3A.1/3B.

No se inventó ninguna métrica.

---

## 2. Fase 3C.0 — investigación (hecho)

Resultados en `docs/GPU_PIPELINE.md`. Lo esencial:

- Hardware real detectado: host **Apple M4**, VM Linux **Ubuntu 24.04 aarch64**,
  kernel **6.8.0-100-generic**, **sin** `/dev/dri`, **sin** `/dev/nvidia*`,
  **sin** módulo GPU. En consecuencia: GPU `NOT AVAILABLE`, NVENC
  `NOT AVAILABLE`, formatos/modificadores `NOT MEASURED`.
- Smithay 0.7: `wayland::dmabuf` disponible bajo `wayland_frontend`;
  `backend::allocator::dmabuf::Dmabuf` sin feature-gate. `create_global` sólo
  puede anunciar formatos honestos si hay un importador real.
- Hardware objetivo (`RESEARCHED`, no medido aquí): RTX 2060 = NVENC 7ª gen
  (H.264/HEVC; **sin AV1 encode**); iPad mini A17 Pro = decode H.264/HEVC/AV1.
- Decisión de arquitectura: **Modelo B** (una composición GPU y **un** encoder por
  `RemoteWindow`), rechazando A por límites de sesiones NVENC/VideoToolbox.
  Evidencia: razonamiento acotado por recursos, **no medido**.
- Codec: **HEVC primario, H.264 fallback**; AV1 excluido (RTX 2060 no lo codifica).

---

## 3. Qué se implementó (código, probado)

### Protocolo (`protocol/aqua.proto` + Rust + Swift)
- `VideoCodec`, `VideoChroma`, `WindowVideoStreamKind`.
- `WindowVideoConfig` (S→C, tag 14), `RequestKeyframe` (C→S, tag 50).
- `WindowVideoStreamHeader` (data plane): `[be32 header_len][header][payload raw]`
  con `kind` HELLO/CONFIG/FRAME, `frame_id`, `keyframe`, `pts_us`,
  `codec_config`. El `SurfaceStreamHeader` de SHM queda **intacto**.
- Paridad cross-language: fixtures Rust → tests Swift byte-exactos.

### Frontera neutral (`server/src/gpu/`)
- `GpuBufferImporter`, `GpuFrame`, `DmabufFormat`, `PlaneFd`, `SyncState`.
- `VideoEncoder`, `VideoEncoderSession`, `VideoEncoderConfig`, `EncodedFrame`,
  `VideoCodec`, `VideoChroma`, `EncodeError`.
- `NullGpuImporter` / `NullVideoEncoder` (headless): no anuncian nada.
- Ningún tipo `NVENC/CUDA/EGL/Vulkan/Smithay` cruza hacia `domain`/protocolo.

### linux-dmabuf (`server/src/wayland/dmabuf.rs`)
- `zwp_linux_dmabuf_v1` implementado con `DmabufHandler` + `delegate_dmabuf!`.
- El global **solo se crea** si `GpuBufferImporter::supported_formats()` no está
  vacío; en headless **no se crea** (`gpu: no importable formats; ... not
  advertised`, verificado en logs).
- Conversión `Dmabuf` → `GpuFrame` duplicando fds (`try_clone_to_owned`),
  **sin** `map_plane` (sin CPU readback en el camino normal).

### Negociación de capacidades
- `capability::advertised(has_video)` → `PHASE_3B | SURFACE_SHM` siempre,
  `| SURFACE_VIDEO` sólo si hay encoder. El servidor headless **no** anuncia vídeo.
- Cliente Swift: `phase3B = phase3A | surfaceShm`.
- `RequestKeyframe` enruta a `VideoEncoderSession::request_keyframe()`.

### Compositor
- El path SHM de 3B no cambia.
- Un buffer dmabuf se **observa** en commit (`frame.dmabuf_attached`:
  fourcc/modifier/planes/dimensiones). No se hace readback.

---

## 4. Milestones (estado)

| # | Qué | Estado aquí | Motivo |
|---|---|---|---|
| 1 | GPU client → linux-dmabuf → Aqua importa | **NOT VERIFIED** | no hay GPU/cliente EGL en el entorno |
| 2 | dmabuf → GPU → encoder → bitstream válido | **NOT VERIFIED** | no hay NVENC; encoder null |
| 3 | cliente GPU → … → iPad físico decode | **NOT VERIFIED** | no hay GPU ni iPad |
| 4 | aplicación GPU real (no Firefox primero) | **NOT DONE** | depende de 1–3 |
| 5 | multi-window GPU | **NOT DONE** | depende de 1–3 |

---

## 5. Reporte solicitado (23 puntos)

1. **Hardware/driver real**: host Apple M4 + VM Ubuntu 24.04 aarch64, kernel
   6.8.0-100; **sin** GPU (`NOT AVAILABLE`). Ver `docs/GPU_PIPELINE.md`.
2. **dmabuf support**: protocolo implementado vía Smithay; global creado sólo con
   importador real; headless **no anunciado**. `NOT VERIFIED` sobre GPU.
3. **formats/modifiers**: **NOT MEASURED** (no hay render node).
4. **Synchronization model**: diseñado (fence implícita primero; explícita
   `linux-drm-syncobj` diferida); **NOT MEASURED**.
5. **GPU import path**: `GpuBufferImporter` neutral; ruta física EGLImage→CUDA→
   NVENC documentada (`RESEARCHED`); **no implementada/ejecutada**.
6. **número real de copias**: **NOT MEASURED**. Garantía de diseño: sin
   `map_plane` en el camino normal.
7. **CPU readbacks**: SHM 1 copia (ya en 3B); dmabuf **0 readbacks** (no hay
   readback implementado). **NOT MEASURED** en hardware.
8. **codec elegido y por qué**: **HEVC** primario (RTX 2060 lo codifica, A17 Pro
   lo decodifica), **H.264** fallback; **AV1** excluido (RTX 2060 sin AV1 encode).
   `RESEARCHED`.
9. **encoder configuration**: `low_latency=true`, B-frames=0, lookahead off,
   keyframes bajo demanda, NV12 4:2:0; bitrate/GOP concretos `NOT MEASURED`.
10. **decoder/presentation strategy**: `AVSampleBufferDisplayLayer` para bring-up,
    `CVPixelBuffer`+Metal como candidato; **NOT MEASURED** (sin hardware).
11. **per-surface vs per-window**: **per-window (Modelo B)**; evidencia =
    acotamiento de sesiones NVENC/VideoToolbox y alineación con
    `xdg_toplevel ↔ UIWindowScene`; **NOT MEASURED** en hardware.
12. **encoder/decoder sessions**: **NOT MEASURED**.
13. **encode latency**: **NOT MEASURED**.
14. **decode latency**: **NOT MEASURED**.
15. **E2E latency**: **NOT MEASURED**.
16. **bitrate/FPS**: **NOT MEASURED**.
17. **calidad de texto/UI**: **NOT MEASURED** (criterio de aceptación definido en
    `docs/VIDEO.md` §8).
18. **resize**: ruta de control/protocolo definida (`ViewportChanged → configure
    → ack → CONFIG vídeo + keyframe`); el resize SHM de 3A sigue **VERIFIED**; el
    reconfigure de vídeo **NOT MEASURED**.
19. **multi-window**: **NOT DONE**.
20. **aplicación GPU real probada**: **NOT DONE** (se especifica un cliente
    EGL/dmabuf mínimo como primer paso antes de Firefox).
21. **SHM regression**: **VERIFIED** — `scripts/e2e.sh`: `weston-simple-shm`+
    `weston-simple-damage`, snapshot `windows=2 surfaces=2`, `frame_client`
    recibió 4 frames (250×250 y 300×200).
22. **tests**: servidor **44** (24 lib + 7 surface_tree + 2 transporte + 10
    registro + 1 doctest); cliente **66** (61 previos + 5 de protocolo 3C).
    Todos en verde; `cargo clippy --all-targets` sin warnings.
23. **deuda técnica**: ver §6.

---

## 6. Deuda técnica y siguiente paso

- **Todo el camino GPU/vídeo físico queda pendiente de hardware** (RTX 2060 +
  iPad): no se puede validar aquí. Es la deuda principal y está acotada a
  "ejecutar y medir", no a rediseñar.
- **Importador concreto** (`EGL_EXT_image_dma_buf_import` + `cuGraphicsEGLRegisterImage`
  + `nvEncodeAPI`) no implementado: requiere libEGL/libcuda en la máquina objetivo.
- **VideoHub / transporte QUIC de vídeo** (stream por `RemoteWindow`,
  colas acotadas con conciencia de keyframe) **no implementado** todavía; el
  contrato de wire y las abstracciones están listos. No se codificó a ciegas sin
  poder probarlo.
- **Decodificador iPad** (`VideoDecoder` + `AVSampleBufferDisplayLayer`/Metal) no
  implementado.
- **Frame callback** sigue a 60 Hz; el modelo B/C queda para medir (docs/VIDEO.md §6).
- **Sync explícita** (`linux-drm-syncobj`) diferida; Smithay la expone sólo con
  `backend_drm`, que no se habilita en headless.
- **`WindowVideoConfig`** se define y serializa pero el cliente aún no lo usa
  para configurar un decoder.

**No se avanza a la siguiente fase.** Cuando exista la máquina con RTX 2060 y el
iPad físico, el orden es: Milestone 1 → 2 → 3 → 4 → 5, midiendo todo lo marcado
como `NOT MEASURED`.
