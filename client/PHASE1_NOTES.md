# Fase 1 — Cliente iPadOS (`RemoteWindow` ↔ `UIWindowScene`)

Cliente iPadOS nativo que valida la arquitectura de ventanas y el lifecycle de
iPadOS. No hay Linux, Wayland, Rust, red, QUIC ni Tailscale todavía: la fuente
remota es `MockRemoteWindowService`.

## Qué se ha verificado en la práctica

Ejecutado en el simulador `iPad Pro 11-inch (M5)` (iOS 27 SDK, target iPadOS 26):

- Se crean **UIWindowScene reales** por cada `RemoteWindow`, incluido dos
  ventanas de la misma aplicación (`firefox-1`, `firefox-2`).
- Identidad estable `RemoteWindowID ↔ UISceneSession` vía
  `NSUserActivity.targetContentIdentifier` + `UISceneSession.userInfo`.
- Títulos y restricciones de tamaño aplicados con APIs públicas
  (`UISceneSizeRestrictions`).
- Cambios de geometría observados con
  `windowScene(_:didUpdateEffectiveGeometry:)` y enviados al transporte
  (solo log en fase 1).
- Cierre de una `RemoteWindow` → `requestSceneSessionDestruction` → la scene
  correspondiente desaparece.
- Desconexión de scene **no** cierra la ventana remota (UI lifecycle ≠ remote
  lifecycle).
- 24 tests unitarios de la lógica sin UIKit, todos en verde.

### Cómo reproducir

```bash
cd client
xcodebuild build -project AquaClient.xcodeproj -scheme AquaClient \
  -destination 'platform=iOS Simulator,name=iPad Pro 11-inch (M5)'

xcodebuild test -project AquaClient.xcodeproj -scheme AquaClient \
  -destination 'platform=iOS Simulator,name=iPad Pro 11-inch (M5)'
```

Demo manual: arranca la app y usa el **Control Panel** (botones `Open Firefox`,
`Open Firefox Window #2`, `Open Visual Studio Code`, `Open Terminal`, ...). Swipe
sobre una fila de "Remote Windows" para cerrarla.

Verificación headless (sin tocar la UI):

```bash
DEV=<udid-simulador>
xcrun simctl install $DEV <path>/AquaClient.app
xcrun simctl launch $DEV com.scirats.aqua -AquaAutoOpenDemo -AquaAutoCloseDemo
# logs estructurados:
xcrun simctl spawn $DEV log stream --level debug --predicate 'subsystem == "com.scirats.aqua"'
```

Los argumentos de lanzamiento `-AquaAutoOpenDemo` / `-AquaAutoCloseDemo` solo
existen para verificación automatizada; el uso normal es el Control Panel.

## Hallazgos sobre las APIs (verificados contra el SDK instalado)

1. **`requestSceneSessionActivation` está deprecada.**
   Se usa la API moderna:
   `UIApplication.activateSceneSession(for:errorHandler:)` (iOS 17+) junto con
   `UISceneSessionActivationRequest(session:)` / `UISceneSessionActivationRequest()`.
   Nota de nombres Swift: `UIScene.ActivationRequestOptions`, no
   `UISceneActivationRequestOptions`.

2. **`UISceneDestructionCondition` no sirve en iPadOS.**
   Sus constructores `userInitiatedDismissal` y `systemDisconnection` solo están
   disponibles en visionOS. En iPadOS 26/27 **no hay señal pública fiable** que
   distinga "el usuario cerró la ventana" de "el sistema desconectó la scene".
   Por eso la arquitectura trata `sceneDidDisconnect` como **no destructivo** y
   el cierre remoto se dirige explícitamente desde el modelo (`window.closed`).
   `application(_:didDiscardSceneSessions:)` se usa solo para soltar bindings.

3. **Restricciones de tamaño:** la API pública es `UIWindowScene.sizeRestrictions`
   (`minimumSize` / `maximumSize`, iOS 13+). Devuelve `nil` si la plataforma no
   soporta redimensionado. El sistema solo las trata como *preferencia*; puede
   no respetarlas.

4. **Geometría / resize:** `windowScene(_:didUpdateEffectiveGeometry:)` (iOS 26)
   sustituye a `didUpdateCoordinateSpace...` (deprecada). El tipo Swift es
   `UIWindowScene.Geometry`. `effectiveGeometry.isInteractivelyResizing` (iOS 26)
   permite saber si el usuario está arrastrando el borde.

5. **Manifest multi-ventana:** generado con
   `INFOPLIST_KEY_UIApplicationSceneManifest_Generation = YES`, que produce
   `UIApplicationSupportsMultipleScenes = true`. Las configuraciones de scene se
   resuelven dinámicamente en
   `application(_:configurationForConnecting:options:)`.

6. **Restauración:** la identidad se persiste en `UISceneSession.userInfo`
   (plist) y como `NSUserActivity` (`targetContentIdentifier` + `userInfo`).
   `stateRestorationActivity(for:)` devuelve la actividad para restaurar.

## Limitaciones conocidas de la fase 1

- **Visibilidad simultánea de ventanas**: crear varias `UIWindowScene` funciona,
  pero mostrar varias a la vez depende del windowing del sistema (Stage Manager /
  modo ventana) y del dispositivo. La app pide sesiones reales; iPadOS decide la
  presentación. No se simula un escritorio.
- **Restauración tras terminar la app**: las sesiones se restauran con su
  `remoteWindowID`, pero el mock es en memoria y no reanuncia ventanas. Una scene
  restaurada sin ventana en el store muestra el placeholder "Waiting for remote
  window". Cuando exista el servidor real, debe reanunciar sus `RemoteWindow`
  (o el cliente deberá cerrar scenes huérfanas).
- **Teclado virtual**: `RemoteInputView` se hace first responder y captura
  `pressesBegan/Ended` (Magic Keyboard / Bluetooth). La captura de texto por
  teclado en pantalla no se habilita todavía: en fase 1 el objetivo es registrar
  eventos, no editar texto.
- **Input**: se registra y se reenvía al mock (log). No hay red.
- **Clipboard**: solo la abstracción (`RemoteClipboardService`), sin `UIPasteboard`.
- **Drag & Drop**: solo diseño (abajo), sin implementación.

## Diseño recomendado: Drag & Drop (fases futuras, APIs públicas)

```
Wayland data device (wl_data_device / wl_data_source)
        ↕
bridge protocol (offer, mime types, start-drag, drop, finish)
        ↕
UIDragInteraction / UIDropInteraction
```

- **Origen local (iPad → Linux)**: `UIDragInteraction` con un
  `UIDragItem`/`NSItemProvider`. Al iniciar el arrastre, notificar al bridge
  `dragStarted(mimeTypes:)`. Al soltar fuera de la app, `dragSessionDidEnd` +
  `Operation.cancel` → `dragFinished`. Cargar el `NSItemProvider` bajo demanda.
- **Destino local (Linux → iPad)**: en `dropInteraction(_:sessionDidUpdate:)`
  devolver una `UIDropProposal` según los MIME types ofrecidos por el bridge.
  En `performDrop` escribir los datos en `UIPasteboard`/fichero temporal y
  emitir `dropReceived(mime, data)`.
- **MIME ↔ UTType**: mapear `text/plain`, `text/uri-list`,
  `image/png`... a `UTType` y viceversa en una tabla explícita.
- Todo debe pasar por el transporte (`RemoteClipboardService`/`RemoteWindowService`),
  nunca por acceso directo a `UIPasteboard` desde la UI.

## Estructura

```
App/        AppDelegate, SceneDelegate, AppEnvironment (composition root)
Domain/     RemoteWindow, IDs, RemoteWindowState, eventos, input, viewport
Windowing/  RemoteWindowStore (actor), SceneSessionRegistry (actor),
            SceneSessionBinding, SceneCoordinator, SceneSessionBackend,
            UIKitSceneSessionBackend, RemoteWindowActivity
Rendering/  RemoteSurfaceView, RemoteSurfaceRenderer, PlaceholderSurfaceRenderer,
            RemoteSurfaceTree
Input/      InputRouter, RemoteInputView
Clipboard/  RemoteClipboardService
Transport/  RemoteWindowService (protocol), Logging
Mock/       MockRemoteWindowService
UI/         ControlPanelViewController, RemoteWindowViewController
```

`RemoteWindowService` es un protocolo. Sustituir
`MockRemoteWindowService` por `QUICRemoteWindowService` no toca el resto.
