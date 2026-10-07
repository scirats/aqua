# Fase 2 — Aqua Server (Linux / Wayland / Smithay)

> **Archived.** Historical phase-2 notes. The **stdin control channel** and
> `scripts/demo.sh` were removed during the cleanup (they were phase-2 demo
> scaffolding); the real input path is now the synthetic input from the iPad.
> References to `scripts/demo.sh` below are kept only as history.

Servidor/compositor Wayland headless que se comporta como servidor Wayland real
para aplicaciones, y traduce cada `xdg_toplevel` a un `RemoteWindow` neutro.
No hay red, ni QUIC, ni vídeo, ni iPad todavía.

```
real Wayland client
        │
        ▼
   xdg_toplevel            (WAYLAND WORLD)
        │
        ▼
   Aqua Server  (Rust + Smithay)
        │
        ▼
   RemoteWindow            (AQUA WORLD)
        │
        ▼
   RemoteEvent → RemoteEventSink → TracingEventSink   (hoy)
                                  → QUICRemoteEventSink (fase 3)
```

No se rehizo nada de la fase 1 (cliente iPadOS).

---

## Estado verificado

Todo lo siguiente se ejecutó de verdad con clientes Wayland reales dentro de un
contenedor Linux (`server/Dockerfile`):

- **Cliente real → xdg_toplevel → RemoteWindow.** `weston-simple-shm` se conecta
  a `WAYLAND_DISPLAY=wayland-aqua` y produce:
  `window.created`, `surface.created`, `window.title_changed`,
  `window.app_id_changed`, `surface.commit`, y al salir `surface.destroyed`,
  `window.closed`.
- **Múltiples ventanas y múltiples ventanas de la misma app.** Dos instancias de
  `weston-simple-shm` → `window-1` y `window-2`, ambos con
  `app_id = org.freedesktop.weston.simple-shm` (mismo `RemoteApplicationId`,
  distinto `RemoteWindowId`).
- **Subsuperficies.** `weston-subsurfaces` → `surface.created kind="subsurface"`
  con `parent`, perteneciendo a `window-1`; **no** crea ventana.
- **Popups.** Cliente dedicado (`scripts/popup_client.c`) crea un `xdg_popup`
  sobre un toplevel → `popup.created popup_id=surface-2 window_id=window-1` y
  `surface.created kind="popup" parent=surface-1`; **no** aparece `window.created`.
- **Metadatos de buffer.** `surface.commit width=101 height=101 shm=true`
  (leído de `wl_shm` vía `with_buffer_contents`). No se codifica nada.
- **Resize (configure/ack).** `resize window-1 640 480` →
  `viewport.changed` → `xdg_toplevel.configure sent` → `xdg_surface.ack_configure`
  → nuevo `surface.commit`.
- **Frame callbacks.** `weston-simple-damage` produjo 144 commits en 3 s
  (~48 fps) gracias a `wl_surface.frame`; ningún cliente se queda bloqueado.
- **Input.** `focus`, `pointer move`, `pointer button`, `key` llegan al cliente:
  se comprobó que `weston-terminal` repinta (`surface.commit`) tras inyectar
  teclas.
- **Ciclo de vida de cliente.** Al desconectar un cliente se cierran todas sus
  ventanas exactamente una vez.
- **Tests.** 14 tests sin compositor (10 de registry + 3 de parsing + 1 doctest).

### Cómo ejecutar

```bash
cd server

# Build del entorno Linux (la máquina de desarrollo es macOS; Smithay no compila en Darwin)
docker build -t aqua-server-dev .

# Tests
docker run --rm -v "$PWD":/work -v aqua-cargo:/usr/local/cargo/registry \
  -v aqua-target:/target -w /work -e CARGO_TARGET_DIR=/target \
  aqua-server-dev cargo test

# Demo completa (servidor + clientes reales + canal de control)
scripts/demo.sh
```

Manual:

```bash
# terminal 1 (contenedor)
export XDG_RUNTIME_DIR=/tmp/aqua-runtime; mkdir -p $XDG_RUNTIME_DIR; chmod 700 $XDG_RUNTIME_DIR
/target/debug/aqua-server
# -> socket = wayland-aqua

# terminal 2 (contenedor)
export XDG_RUNTIME_DIR=/tmp/aqua-runtime
export WAYLAND_DISPLAY=wayland-aqua
weston-simple-shm
```

Canal de control (stdin del servidor):

```
list
resize <window-id> <width> <height>
focus <window-id>
pointer move <x> <y>
pointer button <left|right|middle> <down|up>
scroll <dx> <dy>
key <char> <down|up>
quit
```

---

## Por qué el servidor es headless y sin renderer

Smithay 0.7 permite habilitar `wayland_frontend` + `desktop` **sin** ninguno de
los backends (`backend_drm`, `backend_egl`, `renderer_gl`, `backend_libinput`,
`backend_winit`, `backend_x11`, `xwayland`). El frontend Wayland no necesita un
renderer para funcionar: `CompositorState`, `XdgShellState`, `ShmState`,
`SeatState`, `OutputManagerState` y `DataDeviceState` se construyen y despachan
sin GPU.

Consecuencia: Aqua **observa** superficies y buffers (tamaño, formato) pero no
los dibuja. Eso es exactamente lo que la fase 2 pide. Los frame callbacks se
emiten con un reloj propio de 60 Hz (`send_frames_surface_tree` con
`throttle = Some(Duration::ZERO)`).

Salida de ejemplo (eventos reales):

```
window.created     id=window-1 app_id=None title=None
surface.created    surface_id=surface-1 window_id=window-1 kind="toplevel"
window.title_changed id=window-1 title=Some("simple-shm")
window.app_id_changed id=window-1 app_id=Some(RemoteApplicationId("org.freedesktop.weston.simple-shm"))
surface.commit     surface_id=surface-1 window_id=window-1
xdg_surface.ack_configure surface_id=surface-1
surface.created    surface_id=surface-2 window_id=window-1 kind="subsurface" parent=Some(RemoteSurfaceId(1))
popup.created      popup_id=surface-2 window_id=window-1
viewport.changed   id=window-1 width=800 height=600 scale=1.0
xdg_toplevel.configure sent (awaiting ack_configure) window=window-1
surface.destroyed  surface_id=surface-1 window_id=window-1
window.closed      id=window-1
```

---

## Separación Wayland / Aqua (frontera)

```
WAYLAND WORLD                        AQUA WORLD
─────────────                        ──────────
wl_surface / xdg_toplevel   ──────▶  RemoteWindow / RemoteSurface
xdg_popup                   ──────▶  RemoteSurface(Kind::Popup) del árbol
wl_seat / input             ──────▶  RemoteInputEvent  (fase 3; ya hay mock)
```

- `domain/` no importa **nada** de Wayland/Smithay. Se prueba sin compositor.
- `wayland/` es el único sitio con tipos Smithay; traduce y **nunca** los filtra
  hacia `domain/` ni hacia el futuro transporte.
- El transporte futurosolo verá `RemoteEvent` (ver `events/`).

Módulos:

```
src/
├── main.rs            binario
├── lib.rs             event loop calloop, socket, stdin, frame clock
├── control.rs         parser del canal de control (puro, testeable)
├── domain/            RemoteWindow, ids, estado, eventos, registry (puro)
├── events/            RemoteEventSink, TracingEventSink, RecordingSink
└── wayland/           adapter Smithay
    ├── state.rs       AquaState + bookkeeping de surfaces/clientes
    ├── compositor.rs  wl_compositor / wl_subcompositor / wl_shm
    ├── xdg_shell.rs   xdg_wm_base: toplevel, popup, configure/ack, estados
    ├── seat.rs        wl_seat, data device (clipboard groundwork), output
    ├── output.rs      output virtual 1920×1080@60
    └── input.rs       canal de control, inyección de input, frame clock
```

---

## What we learned from real Wayland clients

Esto es lo que **realmente ocurrió** al ejecutar clientes reales, no lo que
esperábamos:

1. **Los clientes necesitan `XDG_RUNTIME_DIR`.** Si el servidor crea uno propio
   internamente, el cliente lanzado por el usuario no lo ve y aborta
   (`XDG_RUNTIME_DIR is invalid or not set`). La demo exporta la misma variable
   para servidor y clientes.

2. **El flush de eventos salientes es explícito.** `dispatch_clients` sólo
   ocurre cuando el `fd` del display es legible. Tras inyectar input (o enviar
   un configure) no hay actividad del cliente, así que los eventos se quedaban
   en el buffer y el cliente nunca los recibía. Hubo que llamar a
   `DisplayHandle::flush_clients()` tras el canal de control y tras el frame
   clock. Sin esto, el teclado/ratón sintéticos parecían "no funcionar".

3. **Las subsuperficies no tienen callback propio.** `CompositorHandler` de
   Smithay **no** ofrece `new_subsurface`/`subsurface_destroyed`. Se descubren
   de forma perezosa en `commit` subiendo por `get_parent`. Consecuencia: la
   *destrucción* de una subsuperficie aislada no siempre es observable; se
   limpia cuando se cierra la ventana (documentado como limitación).

4. **`weston-subsurfaces` necesita Mesa software.** Es un cliente EGL; sin
   `libgl1-mesa-dri` (swrast/llvmpipe) aborta con
   `egl_state_create: Assertion 'ret == EGL_TRUE'`. No es un problema de Aqua:
   es el cliente buscando un rasterizador. Se añadió `libgl1-mesa-dri` a la
   imagen para poder probarlo.

5. **Los clientes stock no crean popups bajo demanda.** `weston-simple-shm`,
   `weston-subsurfaces` y `weston-simple-damage` no abren menús. Se escribió un
   cliente mínimo (`scripts/popup_client.c`, sin GPU y sin shm) para demostrar
   `xdg_popup`. `weston-terminal` tiene menú en click derecho
   (`button_handler ... case BTN_RIGHT -> show_menu`), pero no se disparó en
   nuestras pruebas de input sintético (queda como pendiente de afinar el foco
   del puntero para menús de libtoytoolkit).

6. **`app_id` y `title` llegan antes del primer commit** para clientes simples
   (`set_app_id`/`set_title` seguidos de `commit`), y Smithay los notifica con
   `app_id_changed`/`title_changed`. No hay que inventarlos desde el título.

7. **El buffer se puede describir sin renderer.** `with_buffer_contents`
   devuelve `BufferData { width, height, stride, format }` para buffers
   `wl_shm`; es la vía barata para metadata en fase 2. Para dmabuf no aplica
   (ver sección dmabuf).

8. **El configure/ack es obligatorio para mapear.** El cliente commitea sin
   buffer, el compositor envía configure, el cliente hace `ack_configure` y sólo
   entonces commitea el buffer. Aqua sigue este orden y por eso los clientes se
   mapean correctamente.

---

## Protocolos Wayland: implementados y pendientes

### Implementados (globals expuestos)

| Protocolo | Estado | Notas |
|---|---|---|
| `wl_compositor` (v4+) | ✅ | superficies y regiones |
| `wl_subcompositor` | ✅ | árbol de superficies |
| `wl_shm` | ✅ | Argb8888 / Xrgb8888 |
| `xdg_wm_base` (xdg-shell) | ✅ | toplevel, popup, positioner, configure/ack |
| `wl_seat` (+ keyboard/pointer) | ✅ | input sintético |
| `wl_output` + `zxdg_output_manager_v1` | ✅ | output virtual 1920×1080 |
| `wl_data_device_manager` | ✅ (groundwork) | clipboard/DnD, sin lógica |
| `wl_callback` (frame) | ✅ | frame clock propio a 60 Hz |

### Necesarios para aplicaciones modernas (fase 3+)

| Protocolo | Por qué |
|---|---|
| `wp_viewporter` / `wp_fractional_scale_v1` | escalado y propósito de superficie (GTK/Qt/foot lo piden) |
| `zxdg_decoration_manager_v1` (XDG decoration) | negociación de decoraciones cliente/servidor |
| `wp_cursor_shape_v1` | cursores sin `wl_pointer.set_cursor` |
| `zwp_linux_dmabuf_v1` | buffers GPU (GTK4, Firefox, Qt con GL) |
| `wp_presentation` | sincronización de presentación |
| `wp_relative_pointer`, `zwp_pointer_constraints` | captura de puntero (juegos) |
| `zwp_text_input_v3` / `wp_input_method` | IME |
| `wp_primary_selection` | selección primaria X11-like |
| `wl_drm` | legacy EGL |
| `zwp_tablet_v2`, `wp_content_protection`, etc. | casos específicos |

XWayland no se implementa (fuera de alcance).

---

## Asimetrías de estado (documentadas)

| Wayland | Aqua | Nota |
|---|---|---|
| `xdg_toplevel.title` | `RemoteWindow.title` | bidireccional sólo lectura; el título no crea identidad |
| `xdg_toplevel.app_id` | `RemoteWindow.application_id` | vacío ⇒ `None` |
| `State::Maximized` | `RemoteWindowState::Maximized` | configurable y observable |
| `State::Fullscreen` | `RemoteWindowState::Fullscreen` | configurable y observable |
| *(no existe)* | `RemoteWindowState::Minimized` | **asimétrico**: no hay estado `minimized` confirmable en xdg-shell; es sólo interno del compositor |
| `set_min_size` / `set_max_size` | `min_size` / `max_size` | sólo lectura, no hay handler dedicado; se leen en commit/ack |

El tamaño de ventana en Wayland **no** es `set(width,height)`: es una
negociación `configure` (compositor→cliente) / `ack_configure`
(cliente→compositor) seguida de un nuevo buffer. Aqua lo respeta.

---

## dmabuf (diseño, no implementado)

Para aplicaciones GPU (Firefox, GTK4, Qt con GL) necesitaremos
`zwp_linux_dmabuf_v1`:

- **Smithay**: módulo `wayland::dmabuf` (`DmabufState`, `DmabufGlobal`,
  `DmabufFeedback`), y `renderer::ImportDma`/`ImportMem` en la capa renderer.
- **Import**: el compositor recibe `zwp_linux_dmabuf_buffer` con fds + planes +
  modifiers; Smithay ofrece `Dmabuf`/`DmabufBuilder`. Importarlo a una textura
  requiere un renderer EGL/GBM (`backend_egl`, `backend_gbm`) o Vulkan.
- **Metadata** disponible: formato, tamaño, planes, strides, modifiers.
- **GPU/encoder**: la textura importada se usaría como entrada de un encoder
  (VA-API/NVENC/VideoToolbox en el Mac no aplica; en Linux, VA-API). Eso implica
  un renderer/EGL y, previsiblemente, un compositor **con** backend (surfaceless
  EGL) a partir de fase 3.
- **Fase 2**: no se implementa; los clientes dmabuf sin fallback no funcionarán
  (ver limitaciones). Se priorizó `wl_shm`.

---

## Cursor (diseño, no implementado)

`wl_pointer.set_cursor` entrega una `wl_surface`; no es una ventana.
El hueco ya existe: `SeatHandler::cursor_image(...)` recibe
`CursorImageStatus`. En fase 3 se modelará un `RemoteCursor` por sesión
(posición + imagen/rol) que se transmitirá aparte de las ventanas, nunca como
`RemoteWindow`.

---

## Decisiones que afectan a la fase 3

1. **Frontera limpia confirmada.** El transporte sólo debe consumir
   `RemoteEvent` + `RemoteBufferInfo`. Ningún tipo Smithay/Wayland debe cruzar.
2. **Nombres de eventos congelados** (`window.created`, `window.title_changed`,
   `window.app_id_changed`, `window.state_changed`, `window.geometry_changed`,
   `window.mapped`, `window.closed`, `surface.created`, `surface.commit`,
   `surface.destroyed`, `popup.created`, `popup.destroyed`, `viewport.changed`).
   Son el contrato neutral con el cliente iPadOS (que ya tiene tipos
   equivalentes).
3. **Identidad**: `RemoteWindowId` es un contador estable (`window-N`),
   independiente de título, `app_id` y PID. `app_id` mapea a
   `RemoteApplicationId`. El cliente iPadOS ya usa `remote://local/window/<id>`;
   en fase 3 habrá que decidir si el id del servidor se reutiliza tal cual o se
   genera un UUID estable por sesión.
4. **Serialización**: **no** se elige aún (Protobuf/FlatBuffers/MessagePack).
   Primero semántica; la decisión es de fase 3.
5. **Píxeles**: hoy Aqua observa commits pero no produce imágenes. En fase 3 hay
   que decidir entre (a) copiar buffers shm por CPU (simple, lento) o
   (b) dmabuf + encoder hardware. La arquitectura de eventos no cambia en
   ninguno de los dos casos.
6. **Configure como origen de verdad de tamaño**: el `RemoteViewportChanged` que
   llegará del iPad debe convertirse en `xdg_toplevel.configure`; el tamaño real
   sólo se conoce tras el `ack` + nuevo buffer. No se debe asumir que el tamaño
   pedido es el aplicado.
7. **Frame clock**: sin renderer se usa un reloj fijo. Cuando exista encoder, el
   frame callback debe emitirse tras presentar, no por tiempo fijo.
8. **Tokio**: no se introdujo. El loop es `calloop` (el que espera Smithay). El
   networking QUIC de fase 3 debería integrarse mediante un boundary (canal
   hacia el loop), no sustituyendo el loop.

---

## Limitaciones conocidas

- **Sin renderer**: no hay píxeles ni daño por región; sólo se informa de
  commits y metadata de buffer. No hay presentación.
- **DMA-BUF no soportado**: clientes que exigen dmabuf sin fallback a shm no
  funcionarán. Se priorizó `wl_shm` (spec). Firefox/GTK4-GL quedan para fase 3.
- **Destrucción de subsuperficies** no siempre observable (ver "What we
  learned", punto 3).
- **Puntero libre sin `enter` persistente**: tras un `focus`, el foco del puntero
  es el de la ventana enfocada; no hay hit-testing real contra geometría
  (no hay `Space`/geometría porque no renderizamos). Suficiente para la demo.
- **Menús de `weston-terminal`** no disparados con input sintético en nuestras
  pruebas (el popup se demostró con `scripts/popup_client.c`).
- **Popup sin posición real**: no se calcula la geometría del positioner contra
  un output; el popup se mapea pero no se reubica. `reposition_request` está
  implementado (envía `repositioned`) pero sin constraint solving.
- **Un único output** virtual, escala fija 1.
- **Sin decoraciones**: `zxdg_decoration` no implementado; los clientes usan
  decoración del lado cliente o ninguna.
- **XWayland fuera de alcance.**

---

## Tests

`cargo test` (sin compositor) cubre el registry puro:

1. múltiples toplevel de la misma app → ventanas distintas
2. títulos se actualizan
3. `app_id` se actualiza
4. destroy produce `window.closed` una sola vez
5. popup no crea ventana
6. subsurface no crea ventana
7. el id no depende del título
8. el configure de viewport mantiene la identidad
9. disconnect de cliente limpia sus recursos
10. surface lifecycle ≠ window lifecycle

más 3 tests de parsing del canal de control y 1 doctest.

La verificación "real" (clientes de verdad) es la demo de `scripts/demo.sh`.
