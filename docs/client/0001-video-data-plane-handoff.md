# 0001 — Handoff: plano de datos de vídeo (client listo para recibir)

De: client · Para: server · Fecha: 2026-10-06 · Estado: abierto

## Contexto

El client ya tiene cableado el pipeline de vídeo y está listo para consumir
streams por `RemoteWindow`. Trabajo hecho en `3deea02`:

- `DataStreamType` (`stream_type`, campo 20) en `SurfaceStreamHeader` y
  `WindowVideoStreamHeader`.
- `VideoStreamModel` (keyframe-aware), `VideoBitstream` (Annex-B ↔ AVCC,
  parameter sets H.264/HEVC), `AVSampleBufferDisplayLayerDecoder`.
- Transporte: `videoFrames()`, `videoConfigurations()`,
  `requestKeyframe(windowID:reason:)`; discriminación SHM vs vídeo con
  `DataStreamProbe`.
- Tests: client 78, server 44, regresión SHM VERIFIED.

Falta el lado server (no hay hardware GPU en el entorno del client).

## Qué necesito del otro lado

1. **`VideoHub`** (equivalente a `FrameHub`): un stream QUIC unidireccional por
   `RemoteWindow` (Modelo B), con colas **acotadas** y conciencia de keyframe.
2. **HELLO** como primer mensaje del stream, con `stream_type = 2`.
3. **CONFIG** antes del primer frame y en cada resize/cambio de codec:
   - `codec_config = true` y payload = parameter sets en **Annex-B**
     (VPS/SPS/PPS para HEVC; SPS/PPS para H.264).
4. **FRAME** por unidad de acceso:
   - `frame_id` monotónico por ventana, flag `keyframe`, `pts_us`,
     payload en **Annex-B** (el client lo convierte a AVCC).
5. **`RequestKeyframe`** (tag 50) → `VideoEncoderSession::request_keyframe()`.
   El client lo envía cuando arranca, tras un reset o al cambiar tamaño/codec.
6. **`WindowVideoConfig`** (tag 14) opcional, informativo (el `CONFIG` del data
   plane manda).

## Contrato de wire (ya en el repo)

```
[be32 header_len][WindowVideoStreamHeader protobuf][payload_len raw bytes]

kind=HELLO  : window_id, stream_type=2
kind=CONFIG : window_id, codec, chroma, width, height, codec_config, payload
kind=FRAME  : window_id, codec, chroma, width, height, frame_id, keyframe,
              pts_us, payload
```

- `WindowVideoStreamHeader.stream_type = DATA_STREAM_WINDOW_VIDEO (2)`.
- SHM: `SurfaceStreamHeader.stream_type = DATA_STREAM_SURFACE_SHM (1)`.
- Codec: **HEVC primario, H.264 fallback** (`docs/VIDEO.md`).

## Preguntas abiertas

1. ¿El server podrá probar el decode localmente (ffmpeg/gst) antes del iPad?
2. ¿Resize: reconfigura el encoder y manda `CONFIG` + keyframe sin reiniciar la
   sesión Aqua? (esperado según `docs/VIDEO.md` §4).
3. ¿Cómo maneja el server la presión de cola si el client va por detrás: fuerza
   keyframe o descarta GOPs viejos?

## Referencias de código

- Protocolo: `protocol/aqua.proto`, `server/src/protocol/video.rs`,
  `server/src/protocol/data.rs`, `server/src/protocol/messages.rs`.
- Client: `client/AquaClient/Video/*`,
  `client/AquaClient/Transport/QUICRemoteWindowService.swift`.
- Documentos: `docs/VIDEO.md`, `docs/GPU_PIPELINE.md`, `PHASE3C.md`.
