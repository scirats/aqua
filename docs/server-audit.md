# Aqua — Auditoría de la idea original

Auditoría honesta (a fecha de esta fase) de si el proyecto cumple la **visión
inicial**, separándola de las **hipótesis de implementación** que fuimos
probando. Fuentes: los documentos internos (`PHASE3A/3B/3C.md`, `docs/*`) y el
código real del repo. Nada de lo no medido se marca como verificado.

Leyenda: **VERIFIED** (ejecutado/hecho) · **PARTIAL** · **PENDING**.

## 1. La idea, en una frase

> Ejecutar aplicaciones Linux **normalmente** en Linux, pero hacer que cada
> ventana (`xdg_toplevel`) se comporte en el iPad como una **ventana nativa**
> (`UIWindowScene`); Linux sigue siendo autoritativo, el iPad es cliente nativo
> (no una pantalla remota tipo VNC).

## 2. Invariantes de la visión vs estado real

| # | Invariante original | Estado | Evidencia / nota |
|---|---|---|---|
| 1 | Linux ejecuta las apps sin modificarlas | **VERIFIED** | clientes Wayland reales (`weston-simple-shm/damage`, `weston-terminal`). |
| 2 | Aqua es un **servidor Wayland real** (Smithay) | **VERIFIED** | `server/src/wayland/*`; headless, sin renderer. |
| 3 | Identidad = `xdg_toplevel`, no proceso | **VERIFIED** | `domain::WindowRegistry`; tests `multiple_toplevels_same_app_are_distinct_windows`. |
| 4 | `xdg_toplevel` ↔ `UIWindowScene` | **VERIFIED** | 3A.1 + simulador + iPad físico. |
| 5 | Múltiples ventanas reales | **VERIFIED** | 2 ventanas de la misma app → 2 escenas. |
| 6 | `RemoteSurfaceTree` ≠ ventana (root/subsurface/popup) | **VERIFIED** | `domain/surface.rs`, `docs/SURFACES.md`; popup/subsurface nunca crean ventana. |
| 7 | Protocolo propio | **VERIFIED** | Protobuf + framing; `protocol/aqua.proto`. |
| 8 | QUIC | **VERIFIED** | Quinn; `docs/TRANSPORT.md`; tests Rust↔Rust. |
| 9 | Tailscale sólo conectividad IP | **VERIFIED** | sin SDK/binario; sólo IP. |
| 10 | Linux → iPad físico | **VERIFIED** | 3A.1 (Tailscale) y revalidado por LAN en esta fase. |
| 11 | Píxeles reales Linux → iPad | **VERIFIED** | SHM (3B) y **HEVC hardware** (esta fase). |
| 12 | Resize iPad → Wayland | **VERIFIED** | `viewport → configure → ack → commit`. |
| 13 | Arquitectura de input | **PARTIAL** | keyboard+pointer OK; `wl_touch` no servido (se acepta y se registra); cursor no modelado. |
| 14 | Pipeline GPU dmabuf nativo | **PENDING** | importer `null`; format/modifier reales ya enumerados (EGL), sin import aún. |
| 15 | Encoding por hardware | **PARTIAL** | VA-API (ffmpeg) funciona, pero **desde SHM (CPU)**, no dmabuf → hay copia/subida. |
| 16 | Decoding por hardware | **PARTIAL** | `AVSampleBufferDisplayLayer`; **decodifica HEVC en iPad físico** (verificado por el cliente). |
| 17 | Decidir per-surface vs per-window | **PARTIAL** | Implementado: SHM = **per-surface**; vídeo = **per-window** (Model B). Decidido *por implementación*, **no medido**. |
| 18 | Frame pacing / damage / híbrido | **PENDING** | frame callbacks a 60 Hz fijos; damage se envía como metadato pero **se mandan frames completos**; no hay camino híbrido. |
| 19 | Cursor | **PENDING** | `SeatHandler::cursor_image` hueco; no se modela `RemoteCursor`. |
| 20 | Clipboard / drag & drop | **PENDING** | `wl_data_device_manager` como groundwork, sin lógica. |
| 21 | Audio | **PENDING** | no contemplado aún. |
| 22 | Apps GPU complejas (GTK4/Qt GL) | **PENDING** | dependen de dmabuf. |
| 23 | Firefox/Chromium | **PENDING** | fuera de alcance por ahora. |

**No hay desviación estructural**: los invariantes 1–12 (el núcleo de la idea)
están intactos.

## 3. Dónde el proyecto SÍ podría estar desviándose (riesgos)

Estos son los puntos que la auditoría debe vigilar, porque son donde una
hipótesis de implementación podría colonizar la arquitectura:

1. **"Todo = vídeo" (deriva real y activa).** El servidor actual **encodea y
   emite vídeo para cada commit de la root, sin condición**. La visión decía
   "quizá híbrido: estático → damage/raw; dinámico → vídeo". Hoy no hay ese
   gate: una terminal quieta igual genera vídeo a 60 fps. *Recomendación:* no
   fijar "todo es vídeo"; añadir condición (damage sostenido / tasa de cambio /
   capacidad del cliente) antes de considerar resuelto el data plane.
2. **`1 wl_surface = 1 encoder`.** No está en el código: SHM es per-surface y
   vídeo es per-window. Correcto respecto a la intención. Pero **la decisión
   sigue sin medirse**; no debe darse por cerrada.
3. **Frontera neutral (encoder).** Bien preservada: `gpu::VideoEncoder`,
   `GpuBufferImporter`, `VideoHub` son neutrales; VA-API/NVENC/VideoToolbox no
   cruzan a `domain`/protocolo. Cambiar de encoder no toca `RemoteWindow` ni
   `UIWindowScene`. Esto es fiel a la idea.
4. **Frame pacing acoplado al reloj.** Sigue a 60 Hz fijo; `docs/VIDEO.md` §6
   dice "model B (aceptación) por defecto" pero **no está implementado**. Riesgo
   de throttling artificial y de no medir latencia real.
5. **Cliente nativo vs pantalla.** Correcto hoy: la app del iPad modela
   ventanas/escenas, no un canvas. El camino de vídeo no debe convertirse en
   "un canvas con vídeo": el `RemoteWindow` sigue siendo la unidad.
6. **Semántico a largo plazo.** No iniciado (terminal/editor/agentes propios por
   estado en vez de píxeles). La frontera de dominio lo permitiría; no hay
   deriva, sólo trabajo futuro.

## 4. Veredicto

- **La idea principal se está cumpliendo.** Los 12 invariantes centrales están
  verificados; los pendientes son exploraciones de *cómo* transportar píxeles,
  que era exactamente la decisión que dejamos abierta.
- **`dmabuf + VA-API/NVENC + VideoToolbox` no es la idea inicial**: es **una
  hipótesis de implementación** a probar contra alternativas (damage, híbrido,
  pacing por presentación). La auditoría debe evitar que Aqua se adapte a
  VA-API en vez de adaptar el mejor pipeline a Aqua.
- La decisión **per-surface vs per-window** y la **híbrido/damage vs vídeo**
  siguen abiertas y deben resolverse **con medición**, no por inercia de 3B/3C.

## 5. Fuentes

Públicas: Wayland (docs/core/xdg-shell/subcompositor/presentation/linux-dmabuf),
Smithay, Apple (UIWindowScene, escenas, Metal, VideoToolbox, CoreGraphics,
Pasteboard, Drag&Drop), QUIC (RFC 9000/9001/9002), Protobuf, Tailscale
(conceptos), NVIDIA Video Codec SDK, Linux DMA-BUF/DRM, VA-API, FFmpeg.
Internas: `PHASE3A/3B/3C.md`, `docs/*`, `protocol/aqua.proto` y las validaciones
físicas del proyecto.
