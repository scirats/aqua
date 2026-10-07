# Fase 3A — Conectar Linux ↔ iPadOS

> **Validación física (Fase 3A.1): VERIFIED.** Ejecutado entre un iPad físico
> (iPad mini A17 Pro, iPadOS 26.6.2) y un Linux (Ubuntu 24.04) sobre **Tailscale**
> (`100.x.x.x`); Aqua no integra ninguna API de Tailscale, sólo IP. El iPad
> completó el handshake QUIC, recibió el `ServerSnapshot`, y se crearon
> `UIWindowScene` reales por ventana (confirmado por `viewport.received` en el
> servidor y capturas de pantalla del dispositivo). Además, la Fase 3B mostró
> **píxeles reales** de `weston-terminal` en el iPad (ver `PHASE3B.md`).
> RTT/Tailscale numérico no medido formalmente.

Objetivo: demostrar

```text
xdg_toplevel ↔ RemoteWindow ↔ Aqua Protocol ↔ QUIC ↔ RemoteWindow ↔ UIWindowScene
```

en las dos direcciones (ventanas, resize, focus, input) **sin transportar un
solo frame gráfico**.

Documentos relacionados: [`docs/PROTOCOL.md`](docs/PROTOCOL.md),
[`docs/TRANSPORT.md`](docs/TRANSPORT.md), schema
[`protocol/aqua.proto`](protocol/aqua.proto).

---

## Qué se implementó

**Servidor (Rust, `server/`)**
- Módulo `protocol/`: codec de framing + mensajes `aqua.v1` (`prost`), tags,
  cabecera `be32(type) be32(len)`.
- Módulo `net/`: servidor QUIC con **Quinn** sobre un runtime Tokio dedicado,
  TLS 1.3 con certificado self-signed + verificación por fingerprint, ALPN
  `aqua/1`.
- Frontera con calloop por canales (`ServerEvent` hacia el core,
  `UnboundedSender<ServerMessage>` hacia cada cliente). Sin `Arc<Mutex<AquaState>>`.
- Identidad: `ServerIdentity` (fingerprint del certificado) + `ServerSessionID`
  (UUID por ejecución) + `revision` monotónica.
- Snapshot atómico en la conexión (Hello + Snapshot + eventos, en orden, desde el
  hilo de calloop).
- `ViewportChanged` del cliente → `xdg_toplevel.configure` real → `ack_configure`.
- Input del cliente (`PointerMoved/Button/Scroll`, `KeyEvent`, `TouchEvent`) →
  `wl_seat` existente.
- El servidor ya no emite `surface.commit` al cliente (solo metadata de ventanas).

**Cliente (Swift, `client/`)**
- `Protocol/`: codec protobuf a mano + framing + mensajes, con tests byte-exactos
  contra fixtures de Rust.
- `Transport/AquaSession.swift`: lógica pura de sesión (handshake, reconciliación
  de snapshot, revisiones, cambios de sesión), testeada sin red.
- `Transport/QUICRemoteWindowService.swift`: implementación de
  `RemoteWindowService` sobre la **API Swift moderna `Network`** (iOS 26:
  `NetworkConnection<QUIC>`, `openStream`, `receive/send`), TLS con
  `certificateValidator` (pin / TOFU).
- `AppEnvironment` intercambia `MockRemoteWindowService` ↔
  `QUICRemoteWindowService` sin cambiar store/coordinator/VCs.
- UI mínima de configuración: host, puerto, fingerprint, Connect/Disconnect, y
  estado de conexión.
- Argumentos de lanzamiento `-AquaAutoConnect <host> <port> [fp]` para E2E.

---

## Qué funcionó (verificado de verdad)

### 1. Pipeline del servidor completo por QUIC (dentro del contenedor)

Con `weston-simple-shm` + `weston-terminal` como clientes Wayland reales y un
cliente QUIC (`cargo run --example quic_client`):

```text
connected to 127.0.0.1:52420
ServerHello version=1 session=c79590c4-... revision=0 error=""
Snapshot revision=0 windows=2
  window window-1 app="org.freedesktop.weston.simple-shm" title="simple-shm" state=0 mapped=false
  window window-2 app="org.freedesktop.weston.wayland-terminal" title="Wayland Terminal" state=0 mapped=false
sent ViewportChanged for window-1 (800x600)
```

Y en el servidor:

```text
handshake.client client_session_id=cb4a45a4-...
handshake.server session=c79590c4-... revision=0
snapshot.sent client_id=1 revision=0
viewport.received window=window-1 width=800 height=600 is_final=true
viewport.changed id=window-1 width=800 height=600
xdg_toplevel.configure sent (awaiting ack_configure) window=window-1
xdg_surface.ack_configure surface_id=surface-1
```

Esto demuestra el camino completo **Wayland → RemoteWindow → Aqua → QUIC →
cliente** y de vuelta **cliente → QUIC → configure Wayland**.

### 2. Transporte QUIC real Rust↔Rust (localhost)

`server/tests/transport.rs`: servidor Quinn real + cliente Quinn real, TLS real,
handshake, `ServerHello`, `ServerSnapshot`, `WindowCreated`, y un comando
`ViewportChanged` recibido por el core. Además, el rechazo de versión
incompatible con `error = "unsupported_protocol_version"`.

### 3. Codec cross-language byte-exacto

`server/examples/fixtures` genera hex; `ProtocolCodecTests` (Swift) decodifica
esos bytes y re-codifica exactamente los mismos frames para `ClientHello`,
`ViewportChanged`, `KeyEvent`, `PointerButton`. Un test dedicado verifica que
tags desconocidos se ignoran.

### 4. Lógica de sesión/reconciliación (Swift, sin red)

`AquaSessionTests`: version mismatch, creación desde snapshot, múltiples ventanas
de la misma app, cierre de huérfanas, revisiones monotónicas + deduplicación,
reconexión sin duplicar, cambio de `ServerSessionID` cierra las ventanas viejas.

### 5. TLS por pinning

El servidor imprime `server = 68151b2a...`; el cliente Rust construye un
verificador que solo acepta ese fingerprint. El cliente Swift compara el SHA-256
del certificado presentado con el fingerprint esperado (o TOFU documentado).

---

## Qué falló / no se pudo verificar (y por qué)

1. **iOS simulator ↔ servidor en contenedor (UDP).** La máquina de desarrollo es
   macOS; el servidor corre en Linux vía Colima. Colima **no reenvía UDP** del
   host a los contenedores, y el host tampoco alcanza la VM por UDP (probado con
   un eco UDP: los paquetes no llegan). El simulador comparte la red del host, así
   que la conexión QUIC se quedaba en `Connecting…` sin alcanzar el servidor.
   Es una limitación del entorno de desarrollo, no del protocolo. En un PC Linux
   real con LAN/Tailscale no existe.
   - Mitigación: codec verificado byte a byte y API `Network` verificada por
     typecheck; el pipeline del servidor verificado con un cliente QUIC real.
2. **iPad físico.** NOT AVAILABLE en el entorno del contenedor inicial; **VERIFIED
   después** por la Fase 3A.1 con un iPad mini A17 Pro físico (ver PHASE3B.md).
3. **Tailscale.** No instalado en el entorno inicial; **VERIFIED después** por la
   Fase 3A.1 a través de Tailscale (`100.x.x.x`). Por diseño Aqua solo necesita
   una IP alcanzable, sin SDK ni comandos `tailscale`.
4. **Menús/popups de `weston-terminal` como input remoto:** no se dispararon; el
   popup ya se demostró en fase 2 con `scripts/popup_client.c`.
5. **`wl_touch`** no está servido aún por el seat (fase 2 solo keyboard+pointer);
   el mensaje `TouchEvent` se acepta y se registra, documentado.

---

## Cambios respecto al diseño original

- **Serialización**: el diseño sugería elegir Protobuf/FlatBuffers/MessagePack/CBOR.
  Elegimos **Protobuf**, pero el codec de Swift es **a mano** para no añadir
  SwiftProtobuf + protoc al build de iOS. Rust usa `prost` (generado).
- **Streams QUIC**: se descubrió que la API moderna `Network` de Apple **sí**
  expone streams (`openStream`), a diferencia de `NWConnection`. Aun así, 3A usa
  un único stream de control bidireccional y demultiplexa por tipo para
  simplicidad; los streams de input y los datagramas quedan diseñados para 3B.
- **`surface.commit` no se transmite** (§42): el alto ritmo de commits de fase 2
  se queda local.
- **Snapshot+eventos**: se resolvió la carrera **por construcción** (el core
  encola Hello, Snapshot y eventos en orden en el hilo de calloop), sin números
  de secuencia extra.
- **`-AquaAutoConnect`**: se añadió para verificación headless.

---

## Cómo ejecutar

```bash
# Servidor (Linux, contenedor) + cliente de prueba, todo por QUIC:
cd server
scripts/e2e.sh

# Tests del servidor (protocolo + transporte + dominio):
docker run --rm -v "$PWD/..":/work -v aqua-cargo:/usr/local/cargo/registry \
  -v aqua-target:/target -w /work/server -e CARGO_TARGET_DIR=/target \
  aqua-server-dev cargo test

# Servidor escuchando para un cliente real (LAN/Tailscale):
AQUA_BIND=0.0.0.0:52420 aqua-server
# El banner imprime address, session y server fingerprint.

# Cliente iPadOS: build + tests
cd client
xcodebuild test -project AquaClient.xcodeproj -scheme AquaClient \
  -destination 'platform=iOS Simulator,name=iPad Pro 11-inch (M5)'
# En la app: Control Panel → Connect to Server… (host, puerto, fingerprint)
```

---

## Resultados y métricas

- **RTT**: no medido end-to-end (bloqueado por el reenvío UDP de Colima). El
  transporte local Rust↔Rust tiene RTT sub-milisegundo en loopback; la medición
  real LAN/Tailscale queda para cuando exista un entorno Linux nativo.
- **Reconexión**: el cliente tiene bucle de reconexión con backoff (1→5 s) y
  reconciliación por snapshot; cubierto por tests de `AquaSession`, no probado
  contra cortes reales de red por la limitación anterior.
- **Tests**: servidor **16** (3 control + 2 transporte + 10 registro + 1 doctest);
  cliente **40** (24 fase 1 + 8 codec + 8 sesión). Todos en verde.
- **Fingerprint servidor**: `68151b2a18db341725e7b0b32d7afcc93f6f5ec244a7a8ce64206b744851611d`.

---

## Decisiones para Fase 3B (frames/vídeo)

1. Los píxeles requieren acceso a buffers: `surface.commit` volverá a ser
   relevante, ahora con datos. Habrá que decidir **shm (copia CPU)** vs
   **dmabuf + encoder hardware**, sin cambiar la capa de eventos.
2. `surface.created`/`surface.destroyed` y el `RemoteSurfaceTree` deben empezar a
   transmitirse (hoy se ignoran) para reconstruir el árbol visual.
3. Separar input de alta frecuencia a **datagrams** y medir latencia antes de
   fijar el modelo de frames.
4. El frame callback debe emitirse tras presentar, no por reloj fijo.
5. Considerar codegen SwiftProtobuf cuando el esquema crezca.
6. Evaluar `inboundStreams` para que el servidor abra streams dedicados
   (p. ej. control vs. vídeo por ventana).

---

## Deuda técnica

- El cliente Swift no se pudo probar hablando QUIC con el servidor en este
  entorno (limitación de red). Probar en Linux nativo + iPad/Tailscale.
- El servidor es 1 sesión/muchos clientes en memoria; no hay autenticación mutua
  (solo TLS servidor) — `ClientSessionID` es una aserción, no una identidad
  criptográfica. Aceptable para 1:1; documentado.
- `AquaState` tiene un espejo de ventanas implícito vía `WindowRegistry`; el
  snapshot se construye desde el registry (correcto) pero conviene formalizar.
- El reenvío de input para `weston-terminal` (menús) no se validó.
