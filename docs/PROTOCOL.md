# Aqua Protocol v1

The contract between the Aqua server (Linux) and the Aqua client (iPadOS).
Schema: [`protocol/aqua.proto`](../protocol/aqua.proto) (normative).

## Serialization: comparison and choice

| | Protobuf ✅ | FlatBuffers | MessagePack | CBOR |
|---|---|---|---|---|
| Rust support | prost (codegen) | flatbuffers | rmp-serde | ciborium |
| Swift support | SwiftProtobuf | FlatBuffers Swift | package | package |
| Schema evolution | first-class (field numbers) | good (vtables) | none (schema-less) | none |
| Zero-copy | no | yes | no | no |
| Size | compact | compact | compact | compact |
| Debugging | `protoc --decode` | json schema | generic | cbor2 |
| Streams | length-delimited messages | length-prefixed | length-prefixed | length-prefixed |
| Verdict | best fit | overkill (no zero-copy need) | no schema | no schema |

**Chosen: Protocol Buffers wire format.** Reasons: explicit schema with field
numbers (real forward/backward compatibility), excellent Rust tooling (`prost`),
compact, self-describing on the wire (wire types), and debuggable with standard
`protoc` tooling. FlatBuffers' zero-copy has no value for metadata-only messages
and adds complexity. MessagePack/CBOR are schema-less, which makes the contract
implicit — rejected.

### Hand-written Swift codec

The Rust side uses `prost` code generation from `protocol/aqua.proto`. The Swift
side uses a **small hand-written protobuf reader/writer**
(`client/.../Protocol/ProtobufCodec.swift` + `AquaMessages.swift`) instead of
adding the SwiftProtobuf SwiftPM dependency and a second `protoc` toolchain to
the build. The wire bytes are standard protobuf, so it stays interoperable.

This is verified by **byte-exact cross-language fixtures**: the Rust
`examples/fixtures` dumps hex for representative messages and the Swift
`ProtocolCodecTests` both decode them and re-encode matching frames. Proto3
default-omission semantics are implemented in the Swift writer so the bytes match
`prost` exactly.

If the protocol grows large, switch the Swift side to SwiftProtobuf code
generation; the `.proto` is already the source of truth.

## Framing

QUIC streams are byte streams; messages are framed:

```text
frame   := be32(type_tag) be32(payload_len) payload
payload := Protocol Buffers body (aqua.v1)
```

The **type tag** lives in the frame header, not the protobuf body. An unknown
tag is skipped by length (forward compatible). `payload_len` is capped at 8 MiB.

## Identity

| Concept | Value | Lifetime | Notes |
|---|---|---|---|
| `ServerIdentity` | SHA-256 of the server cert | long-term | TLS pinning target |
| `ServerSessionID` | UUID v4 | one server run | namespaces windows/events |
| `ClientSessionID` | UUID v4 | one client connection | **not** the IP |
| `RemoteWindowID` | `window-N` (string) | while the toplevel lives | not derived from title/PID |
| `RemoteSurfaceID` | `surface-N` | while the surface lives | not a window |
| `revision` | `u64`, monotonic | one server session | one per observable mutation |

The distributed identity is `(ServerSessionID, RemoteWindowID)`. If the server
restarts, the new `ServerSessionID` makes the client discard the old session
(close its windows) before reconciling the new snapshot — `old/window-1` can
never be confused with `new/window-1`.

Client identity is a `ClientSessionID`, never the transport address (IPs change
between Wi-Fi/cellular, Tailscale reconnects, etc.).

## Versioning

- Semantic version in the handshake: `protocolVersion = 1`.
- QUIC ALPN: `aqua/1`.
- ALPN is **not** the semantic version. A version mismatch is an explicit
  protocol rejection: the server replies with `ServerHello{ error:
  "unsupported_protocol_version" }` and closes. Messages of unknown version are
  never interpreted.

## Capability negotiation

Both sides send a `Capabilities` bitset in the handshake. Phase 3A advertises
`windows | pointer | keyboard | touch`. `clipboard`, `dragDrop`, `surfaceVideo`,
`surfaceShm`, `cursor` are defined but not active. The client must not assume
the server supports everything, and vice versa.

## Handshake

```text
Client → Server   ClientHello { protocolVersion, clientSessionID, capabilities, clientName }
Server → Client   ServerHello { protocolVersion, serverSessionID, serverIdentity, capabilities, revision, error? }
Server → Client   ServerSnapshot { revision, serverSessionID, windows[] }
```

Handshake runs on a client-opened **bidirectional control stream**.

## Snapshot + events (no race)

The client may connect *after* windows already exist, so the server always sends
a **snapshot at the current revision** right after `ServerHello`. Consistency is
guaranteed by construction: the server core processes `ClientConnected` and every
subsequent domain event on its single calloop thread, pushing `ServerHello`,
`ServerSnapshot`, then live events into that client's outbound channel **in
order**. There is no interleaving window in which an event can be lost between
snapshot and stream.

Reconciliation on the client (`AquaSession.handleSnapshot`):

```text
SERVER                                 iPad
window A  ───────────────────────────► create scene
window B  ───────────────────────────► create scene
local scene C (not in snapshot)  ────► WindowClosed (destroy/mark orphan)
```

For each snapshot window: create if unknown, update if changed. Announced
windows missing from the snapshot are closed. Reconnecting to the **same**
session and receiving an identical snapshot emits **no** events (no duplicate
scenes).

## Revision

Every observable mutation increments a per-session `revision`. The client gates
incoming events: `revision <= lastRevision` is ignored (duplicate/old). Gaps are
logged (possible loss) — the future fix is to request a fresh snapshot. QUIC
guarantees order within a stream; `revision` is about Aqua state, independent of
the transport.

## Streams

Phase 3A opens a single client-initiated **bidirectional control stream**. The
server accepts it (and any additional client streams) and demultiplexes by
message type. This is deliberately simple; the design scales to:

```text
QUIC connection
├── control stream   (handshake, window lifecycle, viewport, focus)  — reliable
├── input stream     (keyboard, pointer buttons)                     — reliable
└── datagrams        (pointer motion, touch motion, future frames)   — unreliable/low-latency
```

Pointer/touch **motion** is the first candidate for datagrams once frame rates
matter; phase 3A keeps it on the reliable stream to measure latency first.

## Backpressure / coalescing

- **Viewport**: the client coalesces resize bursts with a ~33 ms flush window and
  always sends the latest size (the VC also reads
  `effectiveGeometry.isInteractivelyResizing` in phase 3B). The server never
  assumes requested size == actual size: it configures and waits for
  `ack_configure` + a new commit.
- **Pointer/touch motion**: latest-position-wins semantics; the client does not
  queue an unbounded backlog.
- **Server → client**: window metadata only, low rate. `surface.commit` is
  **not** transmitted (the client cannot use pixels yet), so the high-frequency
  commit stream never touches the network.

## Messages

Server → client: `ServerHello`, `ServerSnapshot`, `WindowCreated`,
`WindowUpdated`, `WindowTitleChanged`, `WindowApplicationChanged`,
`WindowStateChanged`, `WindowMapped`, `WindowClosed`, `SurfaceCreated`,
`SurfaceDestroyed`, `Pong`.

Client → server: `ClientHello`, `ViewportChanged`, `WindowFocusRequested`,
`PointerMoved`, `PointerButton`, `PointerScroll`, `KeyEvent`, `TouchEvent`,
`Ping`.

`WindowTitleChanged`/`WindowApplicationChanged`/`WindowStateChanged`/`WindowMapped`
are the incremental form of `WindowUpdated`; the client folds them into its
window mirror.

## Not in the protocol (deliberately)

No Wayland types: never `wl_pointer.*`, `wl_surface`, `xdg_toplevel`. No pixels
in 3A. No transport-specific fields: the domain never sees QUIC.
