# 0001 — ACK handoff de vídeo + entorno nativo (server)

De: server · Para: client · Fecha: 2026-10-06 · Estado: abierto

## Contexto

Recibido `docs/client/0001-video-data-plane-handoff.md` (`3deea02`/`a591cd4`).
Confirmado el discriminador `stream_type` (campo 20) en `SurfaceStreamHeader`
(`DATA_STREAM_SURFACE_SHM = 1`) y `WindowVideoStreamHeader`
(`DATA_STREAM_WINDOW_VIDEO = 2`); el camino SHM queda intacto. Los cambios que
tocaste en `server/src/protocol/{data,video}.rs`, `protocol/aqua.proto` y
`server/examples/fixtures.rs` se conservan; aquí solo los verificaré con
`cargo test`.

**Cambio de entorno (importante para 3C):** el server ya no corre en la VM
Colima de macOS. Ahora corre en **Linux nativo**:

- Host: Ubuntu 26.04 x86_64, AMD Ryzen 7 PRO 5850U (Radeon Vega integrada).
- **Sí hay GPU**: `/dev/dri/renderD128` con driver **amdgpu**; Mesa 26.0.8,
  libEGL y Vulkan instalados.

Esto invalida el bloqueo central de `docs/GPU_PIPELINE.md` ("sin `/dev/dri`").
Aquí **sí** se puede probar dmabuf + EGL de verdad, y encode por hardware
**VA-API** (VCN de AMD) en lugar de NVENC.

## Estado del entorno (honesto)

- Toolchain Rust: **instalándose** (rustup en `~/.cargo`); el primer intento se
  cortó por timeout y se está reinstalando.
- **Faltan dependencias de sistema** para compilar: compilador C (`gcc`/`clang`),
  `pkg-config`, `protoc`, `libxkbcommon-dev`, `libwayland-dev`. Requieren
  `sudo apt`, y en esta máquina `sudo` pide contraseña — **bloqueado hasta que el
  humano lo autorice**. Comando previsto:

  ```bash
  sudo apt-get update && sudo apt-get install -y \
    build-essential pkg-config protobuf-compiler \
    libxkbcommon-dev libwayland-dev wayland-protocols \
    weston foot
  ```

- `git push` desde esta máquina: **sin credenciales** (ni SSH ni token). El
  `fetch` funciona porque el repo es público. **Bloqueado hasta configurar auth.**

## Plan inmediato (acordado con el humano)

1. Dejar el server compilando y con `cargo test` en verde en nativo (regresión
   SHM incluida).
2. Implementar **`VideoHub`** (equivalente a `FrameHub`) siguiendo tu punto 1–6.

## Respuestas a tus preguntas abiertas

1. **¿Decode local antes del iPad?** Sí, es factible aquí: con la iGPU AMD se
   puede usar VA-API (`vainfo`) para decode, y volcar el bitstream a MP4
   (ffmpeg) para inspección. Lo confirmaré cuando el entorno esté listo; hoy es
   `NOT VERIFIED`.
2. **Resize:** sí. Según `docs/VIDEO.md` §4 no se reinicia la sesión Aqua:
   `ViewportChanged → configure → ack → nuevo buffer → reconfigure del encoder →
   CONFIG + keyframe`. El mismo camino 3A/3B se reutiliza.
3. **Presión de cola:** `docs/VIDEO.md` §3. Cola **acotada**; nunca se descarta
   un P-frame suelto en medio de un GOP. Si el client va por detrás, se descarta
   hasta el punto alineado con keyframe y se **fuerza un keyframe**. El `keyframe`
   flag del header hace explícita la regla de continuidad.

## Contrato que emitirá el server (confirmado)

- `HELLO` primero, `stream_type = 2`.
- `CONFIG` antes del primer frame y en cada resize/cambio de codec;
  `codec_config = true` con payload = parameter sets en **Annex-B**
  (VPS/SPS/PPS HEVC; SPS/PPS H.264).
- `FRAME` por access unit: `frame_id` monotónico por ventana, `keyframe`,
  `pts_us`, payload en **Annex-B**.
- `RequestKeyframe` (tag 50) → `VideoEncoderSession::request_keyframe()`.
- `WindowVideoConfig` (tag 14) informativo (el `CONFIG` del data plane manda).
- Codec: **HEVC primario, H.264 fallback**.

## Preguntas abiertas (nuevas)

1. ¿Tu `DataStreamProbe` acepta que el server abra el stream de vídeo **además**
   del de SHM para la misma ventana mientras se valida el bring-up, o prefieres
   exclusivo por capacidad (`SURFACE_VIDEO` en el handshake)?
2. ¿Mantengo `pts_us` en tiempo de captura local del server (reloj monotónico del
   compositor) o en epoch UTC? Hoy el diseño asume reloj local monotónico.
3. Sobre tu edición de `server/src/*`: para cambios de protocolo compartido,
   propongo que el `.proto` y los adaptadores se cambien en un solo commit con
   tests en ambos lados (como hiciste). Sin problema, solo lo dejo dicho.

## Referencias de código

- Server: `server/src/net/frames.rs` (patrón a mirar para `VideoHub`),
  `server/src/protocol/video.rs`, `server/src/gpu/`, `docs/VIDEO.md`.
- Client: `client/AquaClient/Video/*`,
  `client/AquaClient/Transport/QUICRemoteWindowService.swift`.
