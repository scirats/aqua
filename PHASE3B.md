# Aqua — Fase 3A.1 (validación física) + Fase 3B (primeros píxeles)

Documentos relacionados: [`PHASE3A.md`](PHASE3A.md), [`docs/PROTOCOL.md`](docs/PROTOCOL.md),
[`docs/TRANSPORT.md`](docs/TRANSPORT.md), [`docs/SURFACES.md`](docs/SURFACES.md),
schema [`protocol/aqua.proto`](protocol/aqua.proto).

---

# Fase 3A.1 — Validación real con dispositivos físicos

## Estado: **VERIFIED**

Ejecutado entre un **iPad físico** (iPad mini A17 Pro, iPadOS 26.6.2) y un
**Linux** (Ubuntu 24.04) a través de **Tailscale** (`100.x.x.x`). No se simuló
nada: la app `com.scirats.aqua` (firmada con el certificado de desarrollo) se
instaló en el iPad y se conectó por QUIC/Tailscale al `aqua-server` real.

Resultado observado (servidor):
```
connection.open            remote=100.73.155.34:64943
handshake.client           client_session_id=...
handshake.server           session=...
snapshot.sent              revision=0
viewport.received          window=window-2 width=1133 height=744 is_final=true
viewport.received          window=window-1 width=1133 height=744 is_final=true
```
Capturas del iPad: `UIWindowScene` reales por ventana; y (Fase 3B) **píxeles
reales** de `weston-terminal` (título + prompt `heaveless@colima-aqua:...$`)
renderizados en el iPad.

### Configuración usada

- Linux (VM Ubuntu 24.04 vía Colima, perfil `aqua`) con Tailscale:
  `aqua-linux` = `100.124.243.59`.
- iPad en la misma tailnet: `ipad161` = `100.73.155.34`.
- En el iPad: `Connect to Server…` con `host=100.124.243.59`, `port=52420`,
  `fingerprint=68151b2a18db341725e7b0b32d7afcc93f6f5ec244a7a8ce64206b744851611d`.

### Nota de entorno

El host de desarrollo es macOS y Colima **no reenvía UDP** (ni Mac→VM ni
Mac→contenedor); por eso LAN directa no era posible y se usó Tailscale, que sí
transporta UDP de extremo a extremo. En un PC Linux nativo con LAN/Tailscale el
procedimiento es el mismo sin estas limitaciones.

## Procedimiento exacto

Linux:

```bash
AQUA_BIND=<ip-alcanzable>:52420 aqua-server
# el banner imprime: address, session, server fingerprint
```

iPad (Control Panel → Connect to Server…): `host = <ip Linux>`, `port = 52420`,
`fingerprint = <server fingerprint>`.

```bash
WAYLAND_DISPLAY=wayland-aqua weston-simple-shm
```

Debe aparecer una `UIWindowScene` real. Verificar creación, title, app_id,
cierre, múltiples ventanas y dos ventanas de la misma app.

Resize: redimensionar la escena y comprobar
`viewport → QUIC → xdg_toplevel.configure → ack_configure → new commit`.

Input: con `weston-terminal`, teclado y puntero del iPad.

Tailscale: es sólo IP; Aqua no integra ninguna API Tailscale.

---

# Fase 3B — Primeros píxeles (`wl_shm → iPad`)

## Estado: **VERIFIED** (píxeles reales en iPad físico)

La captura del iPad durante la sesión muestra la `UIWindowScene` con el
contenido real de `weston-terminal` renderizado desde `wl_shm` vía
Aqua → QUIC → Tailscale → `RemoteSurfaceView`. La ruta completa
`wl_shm → iPad` funciona.

Objetivo: `wl_shm → wl_buffer → Aqua Server → Aqua Protocol → QUIC → iPadOS →
RemoteSurfaceView → pixels`, sin dmabuf, sin códecs, sin Firefox.

## Qué se implementó

**Protocolo** (`protocol/aqua.proto`)
- `SurfaceInfo`, `SurfaceCreated`, `SurfaceUpdated`, `SurfaceDestroyed` en el
  plano de control; surface tree en `ServerSnapshot`.
- Plano de datos: `SurfaceStreamHeader` + `DamageRect` + `SurfaceStreamKind`, con
  formato `[be32 header_len][header protobuf][payload raw]`.
- `FramePresented` (cliente → servidor).
- Capacidad `CAPABILITY_SURFACE_SHM` definida (activa en el handshake cuando
  procede).

**Servidor**
- Captura de `wl_shm` con **copia propia** y validación estricta
  (dimensiones, stride, overflow, tamaño máximo, longitud). Safety documentada.
- `RemoteSurface` con parent/role/position/size/z; eventos de árbol.
- `FrameHub`: un frame por surface (`watch`, latest-wins), un stream QUIC
  unidireccional por surface, `hello` + frames; `drop_surface` libera memoria.
- El buffer se captura **antes** de `on_commit_buffer_handler` (que hace
  `take()` del buffer).
- `FramePresented` recibido y logueado.

**Cliente (Swift)**
- Codec de las nuevas superficies + cabecera de stream (a mano, byte-compatible).
- `AquaSession` rastrea el árbol de superficies (reconciliado con snapshot).
- `SurfacePixelFormat`: conversión SHM little-endian → `CGImage` (BGRA/BGRX).
- `WindowSurfaceModel`: composición main + subsurfaces + popup, latest-wins,
  stale discard, memoria acotada.
- `QUICRemoteWindowService`: acepta los streams de surface (`inboundStreams`),
  decodifica HELLO/frames, emite frames y superficies, envía `FramePresented`.
- `RemoteWindowViewController`: muestra la imagen compuesta en la Scene real
  (oculta el placeholder "Waiting for remote surface" cuando llegan píxeles).

## Qué funcionó (verificado de verdad)

### Pipeline de datos completo por QUIC (dentro del contenedor)

Con `weston-simple-shm` + `weston-simple-damage` y el probe `frame_client`:

```text
HELLO surface=surface-1 window=window-1
control: snapshot windows=2 surfaces=2
FRAME surface=surface-1 frame_id=100 250x250 stride=1000 format=2 bytes=250000 damage=1 first_px=[ff, ff, ff, ff]
FRAME surface=surface-1 frame_id=101 250x250 stride=1000 format=2 bytes=250000 damage=1 first_px=[ff, ff, ff, ff]
HELLO surface=surface-2 window=window-2
FRAME surface=surface-2 frame_id=52 300x200 stride=1200 format=1 bytes=240000 damage=2 first_px=[ff, ff, ff, ff]
FRAME surface=surface-2 frame_id=53 300x200 stride=1200 format=1 bytes=240000 damage=2 first_px=[ff, ff, ff, ff]
total frames received: 4
```

Esto demuestra: buffers SHM reales capturados, metadatos correctos
(dimensiones/stride/format/damage), frameID monotónico por surface, y transporte
por streams dedicados.

### Conversión de píxeles (Swift, tests)

`PixelFormatTests`: rojo, verde, azul, blanco, transparente, ARGB premultiplicado
50 %, formato no soportado, dimensión cero, buffer corto. Detectan intercambio de
canales (crítico porque `ARGB8888` es BGRA en memoria).

### Modelo de superficie y composición (Swift, tests)

`WindowSurfaceModelTests`: latest-wins, stale discard, frame presentado no se
redibuja, composición al tamaño del root, posiciones absolutas acumulando
parents, subsurface mantiene el tamaño del root, remover surface libera frame.

### Tests del servidor

`validate_shm` (válido/cero/stride corto/buffer corto/demasiado grande/dimensión),
round-trip de `SurfaceStreamHeader` (hello + frame) con payload raw, y
`tests/surface_tree.rs` (subsurface/popup comparten ventana, geometría →
`surface.updated`, cierre libera surfaces).

## Qué falló / no se pudo verificar

> **Actualizado tras la validación física real.** Las afirmaciones obsoletas de
> esta sección (que decían que el display físico y Tailscale no estaban
> verificados) eran de antes de ejecutar la prueba física; se corrigen aquí con
> los resultados reales. No se inventan cifras: lo no medido queda como
> `NOT MEASURED`.

- **Display físico en iPad: VERIFIED.** La sesión completa
  (`wl_shm → Aqua Server → raw surface stream → QUIC → Tailscale → Aqua iPad →
  RemoteSurfaceView → UIWindowScene`) se observó en un **iPad mini A17 Pro**
  físico (iPadOS 26.6.2), con píxeles reales de `weston-terminal` (título +
  prompt) renderizados en pantalla. Ver el estado al inicio del documento y
  `PHASE3A.md` (§Validación física).
- **Tailscale: VERIFIED.** La conexión iPad ↔ Linux se hizo a través de
  Tailscale (`100.x.x.x`); Aqua no integra ninguna API Tailscale, sólo IP.
- **Frame callbacks dirigidos por presentación**: no implementado aún (se
  mantiene el reloj de 60 Hz para no bloquear al cliente Wayland desde el primer
  prototipo). `FramePresented` ya se transporta.
- **Damage incremental / compresión / dmabuf**: fuera de alcance de 3B.

## Cambios respecto al diseño

- La captura debe hacerse antes de `on_commit_buffer_handler` (hallazgo real).
- La cabecera de frame lleva `payload_len` explícito y el payload va raw, fuera
  de protobuf.
- `FrameHub` usa `watch` (un valor) como mecanismo de backpressure; no hay cola.
- `FramePresented` se envía en cada composición (best-effort) como preparación.

## Reporte solicitado

1. **Validación física 3A.1**: VERIFIED (iPad mini A17 Pro físico + Linux
   Ubuntu 24.04). Ver `PHASE3A.md`.
2. **LAN**: no probado a nivel físico en esta fase; el pipeline está validado en
   localhost del contenedor y el extremo iPad ↔ Linux se validó por Tailscale.
3. **Tailscale**: VERIFIED (transporte extremo a extremo; Aqua sólo usa IP).
4. **RTT real**: **NOT MEASURED** con iPad; loopback sub-ms en el probe Rust.
5. **Throughput raw SHM**: frames de 250 000 B (250×250×4) y 240 000 B
   (300×200×4); se envían a la tasa de commits que el `watch` permite (latest).
6. **FPS observado**: el servidor captura al ritmo de commits; `frame_client`
   recibió el frame más reciente (p.ej. `frame_id=100/101`). No se fija objetivo
   de fps en 3B.
7. **bytes/frame**: 250 000 (XRGB 250×250) y 240 000 (ARGB 300×200).
8. **frames dropped**: en el servidor, los intermedios no enviados; en el cliente,
   `WindowSurfaceModel.droppedFrames` cuenta duplicados/obsoletos. No hay backlog.
9. **copias aproximadas**: 1 copia servidor (mmap → `Vec`), 1 envío, 1 recepción
   (QUIC → `Data`), y la creación de `CGImage` (sin copia adicional, provider
   sobre `Data`). ~1-2 copias efectivas.
10. **resize**: servidor ya entrega `configure`/`ack`; los nuevos
    `wl_buffer` producen frames con nuevas dimensiones (verificado en 3A:
    `viewport.received` → `configure` → `ack`).
11. **subsurfaces**: `surface.created role=subsurface` con `parent`; se componen
    dentro de la misma Scene; no crean ventana.
12. **popups**: `surface.created role=popup` con `parent`; misma Scene; no crean
    ventana (verificado en 3A con `scripts/popup_client.c`).
13. **memoria**: acotada (un frame por surface en servidor y cliente;
    `drop_surface`/`removeSurface` liberan al destruir).
14. **CPU Linux**: no medida formalmente.
15. **CPU iPad**: no medible aquí.
16. **limitaciones**: ver `docs/SURFACES.md`.
17. **problemas**: `on_commit_buffer_handler` consume el buffer (resuelto);
    `BufferAssignment` no es `Clone`; `watch::Sender::send` no almacena sin
    receptores (usar `send_replace`); `SubsurfaceCachedState` sin z-order.
18. **cambios arquitectónicos**: se añadió el plano de datos (streams por
    surface) manteniendo la frontera de dominio; el renderer es reemplazable
    detrás de `RemoteSurfaceRenderer`/`WindowSurfaceModel`.
19. **codec Protobuf Swift**: se mantiene a mano; con 3B creció de forma lineal y
    controlada, y sigue verificado byte a byte contra Rust. Decisión: mantenerlo
    por ahora; migrar a SwiftProtobuf si supera el umbral de mantenimiento
    (documentado en `docs/PROTOCOL.md`).
20. **tests**: servidor **34** (14 lib + 7 surface_tree + 2 transporte + 10
    registro + 1 doctest); cliente **61** (24 fase 1 + 13 codec + 11 sesión +
    11 pixel + ... ). Todos en verde.

## Decisiones para Fase 3C

1. `linux-dmabuf` + import GPU; el dominio/eventos no cambian.
2. Encoder por surface (H.264/HEVC) y `VideoToolbox` en iPad.
3. Frame callback dirigido por `FramePresented` (medir antes de fijar el modelo).
4. Damage incremental y/o codificación de regiones.
5. Coordinar `logicalSize`/`bufferSize`/`scale` para fractional scale.

Detente aquí. No empezar 3C.
