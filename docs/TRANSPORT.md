# Aqua Transport — QUIC

This document records the transport research, the choices, and the way QUIC is
wired into the server without disturbing the Wayland loop.

## Scope

Aqua Protocol v1 is a small, private protocol between **one Aqua server (Linux)**
and **one iPad client**. The transport is QUIC (RFC 9000/9001/9002, TLS 1.3). It
carries only window/input/viewport metadata in phase 3A — **no pixels**.

```text
Aqua Protocol
      ↓
     QUIC          (this document)
      ↓
      IP
      ↓
  Tailscale        (optional, outside Aqua)
      ↓
  WireGuard
```

Tailscale is **only** IP connectivity. Aqua never calls a `tailscale` binary and
never links a Tailscale SDK. `serverAddress = 100.x.x.x` works exactly like a
LAN IP or `127.0.0.1`.

## Rust: QUIC library comparison

| | Quinn 0.11 ✅ | quiche 0.30 | s2n-quic 1.90 |
|---|---|---|---|
| Language | pure Rust | Rust + BoringSSL | Rust + s2n-tls |
| Standard QUIC v1 | yes | yes | yes |
| TLS 1.3 | rustls (ring/aws-lc-rs) | BoringSSL | s2n-tls |
| Bidirectional / unidirectional streams | yes | yes | yes |
| QUIC DATAGRAM | yes | yes | yes |
| Async runtime | Tokio-native | runtime-agnostic (you drive it) | Tokio-native |
| Build deps | none (rustls) | BoringSSL + cmake | s2n-tls C libs |
| Maintenance | very active | active | active |
| Fit here | excellent | heavy (FFI) | good, AWS-centric |

**Chosen: Quinn 0.11.** Reasons: pure-Rust, no C toolchain, first-class
Tokio + rustls, streams and datagrams, and the smallest amount of glue. quiche
was rejected for the BoringSSL build burden; s2n-quic adds a C dependency for no
benefit in this private 1:1 setting.

## iPadOS: Apple QUIC API

Verified against the installed SDK (iPhoneSimulator 27.0).

- Legacy C API: `nw_parameters_create_quic`, `nw_quic_*` in
  `Network.framework/Headers/quic_options.h`.
- **Modern Swift API (iOS 26+)** in
  `Network.framework/Modules/Network.swiftmodule`: `NetworkConnection<QUIC>`,
  `QUIC(alpn:)`, `QUICStream`, `QUIC.Network.Stream`, `openStream(directionality:)`,
  `inboundStreams`, `datagrams`, and TLS via `QUIC.tls.certificateValidator` /
  `peerAuthentication` / `localIdentity`.

**Chosen: the modern Swift `Network` API on the client.** It exposes real QUIC
multiplexed streams (`openStream`), which the old `NWConnection` abstraction did
not. The client uses:
- `NetworkConnection(to: .hostPort(host:port:)) { QUIC(alpn: ["aqua/1"]).tls
  .certificateValidator { ... } }`
- `connection.onStateUpdate { ... }`, `connection.start()`
- `try await connection.openStream(directionality: .bidirectional)`
- `try await stream.send(Data)`, `try await stream.receive(exactly:)`

ALPN is `aqua/1` on both sides and must match.

## TLS strategy

QUIC mandates TLS 1.3. Requirements: a private, peer-to-peer deployment without a
public CA.

- The server presents a **self-signed development certificate**
  (`server/certs/dev-cert.pem`, SAN `localhost/127.0.0.1/::1`). Its SHA-256
  fingerprint is the server's long-term **`ServerIdentity`**.
- The client validates the certificate against a **pinned fingerprint**, not
  "accept anything":
  - Rust test/probe client: `net::tls::client_config(fingerprint)` builds a
    `rustls` verifier that accepts only the pinned SHA-256 fingerprint.
  - Swift client: `QUIC.tls.certificateValidator` computes the presented
    certificate's SHA-256 and compares to the expected fingerprint. If no
    fingerprint is configured it performs **trust-on-first-use** for the session
    and logs a warning; `expectedFingerprint` (from the Connect dialog) enables
    strict pinning.
- `ServerIdentity` (long-term, cert fingerprint) is deliberately separate from
  `ServerSessionID` (per-run UUID).
- "Accept any certificate" is **never** the production behavior. The TOFU path
  is a development convenience and is logged.

### Verified

The dev certificate fingerprint is stable and is printed in the server banner:

```
server = 68151b2a18db341725e7b0b32d7afcc93f6f5ec244a7a8ce64206b744851611d
```

## Linux threading / calloop boundary

Smithay/calloop stays the owner of all Wayland state. The QUIC runtime (Tokio)
runs on a **dedicated thread pool** and never touches `AquaState`.

```text
┌──────────────────────────────────────────────┐
│ Aqua server process                            │
│                                                │
│  calloop thread (Wayland owner)                │
│    AquaState + WindowRegistry + revision       │
│        │  ServerEvent   ▲   ServerMessage       │
│        ▼  (calloop chan) │   (tokio chan)       │
│  ┌──────────────────────┴───────────────────┐  │
│  │ Tokio runtime (2 workers) — Quinn        │  │
│  │   ClientCommand  →  calloop              │  │
│  │   ServerMessage  →  (tokio chan)         │  │
│  └──────────────────────────────────────────┘  │
└──────────────────────────────────────────────┘
```

- Network → core: `calloop::channel::Sender<ServerEvent>` (from Tokio tasks;
  the sink trait `net::ServerEventSink` also has a `std::sync::mpsc` impl for
  tests).
- Core → network: one `tokio::sync::mpsc::UnboundedSender<ServerMessage>` per
  connected client, stored in `AquaState`.

There is **no** `Arc<Mutex<AquaState>>`. Wayland state never crosses threads.

Tokio is **not** the main loop; it exists only for the transport. This satisfies
the phase-2 constraint and leaves room for phase 3B (frames) without redesigning
the compositor.

## TLS / UDP environment note (this dev host)

The dev host is macOS. The server is built and run inside a Linux container
(Colima). Colima does **not** forward UDP from the host into containers, and the
host cannot reach the Colima VM over UDP either. As a result, the iOS simulator
(which shares the host network) could not complete the QUIC handshake against the
containerized server in this environment. This is an environment limitation, not
a protocol one; the Rust↔Rust QUIC test and the in-container probe both pass
(see PHASE3A.md). On a real Linux PC with a LAN/Tailscale address there is no
such restriction.
