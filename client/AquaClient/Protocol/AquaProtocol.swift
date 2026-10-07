import Foundation

/// Aqua Protocol v1 constants. Mirrors `protocol/aqua.proto` and the Rust
/// `protocol` module.
enum AquaProtocol {
    /// Semantic protocol version carried in the handshake.
    static let version: UInt32 = 1

    /// QUIC ALPN identifier. Must match the server.
    static let alpn = "aqua/1"

    /// Frame header: `be32(typeTag)` + `be32(payloadLength)`.
    static let headerLength = 8
    static let maxPayload = 8 * 1024 * 1024

    enum Capability {
        static let windows: UInt32 = 1 << 1
        static let pointer: UInt32 = 1 << 2
        static let keyboard: UInt32 = 1 << 3
        static let touch: UInt32 = 1 << 4
        static let clipboard: UInt32 = 1 << 5
        static let dragDrop: UInt32 = 1 << 6
        static let surfaceVideo: UInt32 = 1 << 7
        static let surfaceShm: UInt32 = 1 << 8
        static let cursor: UInt32 = 1 << 9

        /// Capabilities the client negotiates in phase 3A.
        static let phase3A: UInt32 = windows | pointer | keyboard | touch

        /// Phase 3B adds the raw SHM surface data plane (always supported).
        static let phase3B: UInt32 = phase3A | surfaceShm
    }

    enum ServerTag {
        static let hello: UInt32 = 1
        static let snapshot: UInt32 = 2
        static let windowCreated: UInt32 = 3
        static let windowUpdated: UInt32 = 4
        static let windowTitleChanged: UInt32 = 5
        static let windowApplicationChanged: UInt32 = 6
        static let windowStateChanged: UInt32 = 7
        static let windowMapped: UInt32 = 8
        static let windowClosed: UInt32 = 9
        static let surfaceCreated: UInt32 = 10
        static let surfaceDestroyed: UInt32 = 11
        static let surfaceUpdated: UInt32 = 13
        static let pong: UInt32 = 12
        static let windowVideoConfig: UInt32 = 14
    }

    enum ClientTag {
        static let hello: UInt32 = 40
        static let viewportChanged: UInt32 = 41
        static let windowFocusRequested: UInt32 = 42
        static let pointerMoved: UInt32 = 43
        static let pointerButton: UInt32 = 44
        static let pointerScroll: UInt32 = 45
        static let keyEvent: UInt32 = 46
        static let touchEvent: UInt32 = 47
        static let ping: UInt32 = 48
        static let framePresented: UInt32 = 49
        static let requestKeyframe: UInt32 = 50
    }

    enum VideoCodec {
        static let h264: UInt32 = 1
        static let hevc: UInt32 = 2
        static let av1: UInt32 = 3
    }

    enum VideoChroma {
        static let nv12: UInt32 = 1
        static let p010: UInt32 = 2
    }

    enum WindowVideoStreamKind {
        static let hello: UInt32 = 1
        static let config: UInt32 = 2
        static let frame: UInt32 = 3
    }

    /// Server-opened data-plane stream discriminator (`stream_type` field 20).
    enum DataStreamType {
        static let surfaceShm: UInt32 = 1
        static let windowVideo: UInt32 = 2
    }

    enum SurfaceRole {
        static let toplevel: UInt32 = 1
        static let subsurface: UInt32 = 2
        static let popup: UInt32 = 3
    }

    enum SurfaceFormat {
        static let argb8888: UInt32 = 1
        static let xrgb8888: UInt32 = 2
    }

    enum SurfaceStreamKind {
        static let hello: UInt32 = 1
        static let frame: UInt32 = 2
    }
}

/// Aqua frame header helpers.
enum AquaFrame {
    static func encode(tag: UInt32, payload: Data) -> Data {
        var data = Data(capacity: AquaProtocol.headerLength + payload.count)
        data.append(UInt8((tag >> 24) & 0xff))
        data.append(UInt8((tag >> 16) & 0xff))
        data.append(UInt8((tag >> 8) & 0xff))
        data.append(UInt8(tag & 0xff))
        let length = UInt32(payload.count)
        data.append(UInt8((length >> 24) & 0xff))
        data.append(UInt8((length >> 16) & 0xff))
        data.append(UInt8((length >> 8) & 0xff))
        data.append(UInt8(length & 0xff))
        data.append(payload)
        return data
    }

    /// Parse an 8-byte header into `(tag, payloadLength)`.
    static func decodeHeader(_ header: Data) -> (tag: UInt32, length: Int)? {
        guard header.count == AquaProtocol.headerLength else { return nil }
        let bytes = [UInt8](header)
        let tag = (UInt32(bytes[0]) << 24) | (UInt32(bytes[1]) << 16) | (UInt32(bytes[2]) << 8) | UInt32(bytes[3])
        let length = (UInt32(bytes[4]) << 24) | (UInt32(bytes[5]) << 16) | (UInt32(bytes[6]) << 8) | UInt32(bytes[7])
        return (tag, Int(length))
    }
}
