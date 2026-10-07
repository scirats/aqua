import Foundation

// MARK: - Shared

struct Capabilities: Equatable {
    var bits: UInt32 = 0

    func contains(_ capability: UInt32) -> Bool { bits & capability != 0 }

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        if bits != 0 { writer.writeUInt32(1, bits) }
        return writer.data
    }

    static func decodeBody(_ data: Data) throws -> Capabilities {
        var reader = ProtobufReader(data)
        var value = Capabilities()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.bits = UInt32(try reader.readVarint())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

// MARK: - Client -> Server

struct ClientHelloMessage: Equatable {
    var protocolVersion: UInt32 = AquaProtocol.version
    var clientSessionID: String = ""
    var capabilities = Capabilities()
    var clientName: String = ""

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeUInt32(1, protocolVersion)
        writer.writeString(2, clientSessionID)
        writer.writeMessage(3, capabilities.encodeBody())
        writer.writeString(4, clientName)
        return writer.data
    }

    static func decodeBody(_ data: Data) throws -> ClientHelloMessage {
        var reader = ProtobufReader(data)
        var value = ClientHelloMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.protocolVersion = UInt32(try reader.readVarint())
            case 2: value.clientSessionID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.capabilities = try Capabilities.decodeBody(try reader.readLengthDelimited())
            case 4: value.clientName = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct ViewportChangedMessage: Equatable {
    var windowID: String = ""
    var width: Int32 = 0
    var height: Int32 = 0
    var scale: Double = 1
    var isFinal = false

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, windowID)
        writer.writeInt32(2, width)
        writer.writeInt32(3, height)
        writer.writeDouble(4, scale)
        writer.writeBool(5, isFinal)
        return writer.data
    }

    static func decodeBody(_ data: Data) throws -> ViewportChangedMessage {
        var reader = ProtobufReader(data)
        var value = ViewportChangedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 2: value.width = Int32(truncatingIfNeeded: try reader.readVarint())
            case 3: value.height = Int32(truncatingIfNeeded: try reader.readVarint())
            case 4: value.scale = try reader.readDouble()
            case 5: value.isFinal = try reader.readVarint() != 0
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct WindowFocusRequestedMessage: Equatable {
    var windowID: String = ""

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, windowID)
        return writer.data
    }
}

struct PointerMovedMessage: Equatable {
    var windowID: String = ""
    var x: Double = 0
    var y: Double = 0

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, windowID)
        writer.writeDouble(2, x)
        writer.writeDouble(3, y)
        return writer.data
    }
}

struct PointerButtonMessage: Equatable {
    var windowID: String = ""
    var button: UInt32 = 0
    var pressed = false
    var x: Double = 0
    var y: Double = 0

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, windowID)
        writer.writeUInt32(2, button)
        writer.writeBool(3, pressed)
        writer.writeDouble(4, x)
        writer.writeDouble(5, y)
        return writer.data
    }
}

struct PointerScrollMessage: Equatable {
    var windowID: String = ""
    var dx: Double = 0
    var dy: Double = 0

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, windowID)
        writer.writeDouble(2, dx)
        writer.writeDouble(3, dy)
        return writer.data
    }
}

struct KeyEventMessage: Equatable {
    var windowID: String = ""
    var keycode: UInt32 = 0
    var characters: String = ""
    var modifiers: UInt32 = 0
    var pressed = false

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, windowID)
        writer.writeUInt32(2, keycode)
        writer.writeString(3, characters)
        writer.writeUInt32(4, modifiers)
        writer.writeBool(5, pressed)
        return writer.data
    }
}

struct TouchEventMessage: Equatable {
    var windowID: String = ""
    var touchID: UInt32 = 0
    var phase: UInt32 = 0
    var x: Double = 0
    var y: Double = 0

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, windowID)
        writer.writeUInt32(2, touchID)
        writer.writeUInt32(3, phase)
        writer.writeDouble(4, x)
        writer.writeDouble(5, y)
        return writer.data
    }
}

// MARK: - Server -> Client

struct ServerHelloMessage: Equatable {
    var protocolVersion: UInt32 = 0
    var serverSessionID: String = ""
    var serverIdentity: String = ""
    var capabilities = Capabilities()
    var revision: UInt64 = 0
    var error: String = ""

    static func decodeBody(_ data: Data) throws -> ServerHelloMessage {
        var reader = ProtobufReader(data)
        var value = ServerHelloMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.protocolVersion = UInt32(try reader.readVarint())
            case 2: value.serverSessionID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.serverIdentity = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 4: value.capabilities = try Capabilities.decodeBody(try reader.readLengthDelimited())
            case 5: value.revision = try reader.readVarint()
            case 6: value.error = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct WindowInfoMessage: Equatable {
    var windowID = ""
    var applicationID = ""
    var title = ""
    var state: UInt32 = 0
    var mapped = false
    var hasMin = false
    var minWidth: Int32 = 0
    var minHeight: Int32 = 0
    var hasMax = false
    var maxWidth: Int32 = 0
    var maxHeight: Int32 = 0

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, windowID)
        writer.writeString(2, applicationID)
        writer.writeString(3, title)
        writer.writeUInt32(4, state)
        writer.writeBool(5, mapped)
        writer.writeBool(6, hasMin)
        writer.writeInt32(7, minWidth)
        writer.writeInt32(8, minHeight)
        writer.writeBool(9, hasMax)
        writer.writeInt32(10, maxWidth)
        writer.writeInt32(11, maxHeight)
        return writer.data
    }

    static func decodeBody(_ data: Data) throws -> WindowInfoMessage {
        var reader = ProtobufReader(data)
        var value = WindowInfoMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 2: value.applicationID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.title = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 4: value.state = UInt32(try reader.readVarint())
            case 5: value.mapped = try reader.readVarint() != 0
            case 6: value.hasMin = try reader.readVarint() != 0
            case 7: value.minWidth = Int32(truncatingIfNeeded: try reader.readVarint())
            case 8: value.minHeight = Int32(truncatingIfNeeded: try reader.readVarint())
            case 9: value.hasMax = try reader.readVarint() != 0
            case 10: value.maxWidth = Int32(truncatingIfNeeded: try reader.readVarint())
            case 11: value.maxHeight = Int32(truncatingIfNeeded: try reader.readVarint())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct ServerSnapshotMessage: Equatable {
    var revision: UInt64 = 0
    var serverSessionID = ""
    var windows: [WindowInfoMessage] = []
    var surfaces: [SurfaceInfoMessage] = []

    static func decodeBody(_ data: Data) throws -> ServerSnapshotMessage {
        var reader = ProtobufReader(data)
        var value = ServerSnapshotMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.serverSessionID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.windows.append(try WindowInfoMessage.decodeBody(try reader.readLengthDelimited()))
            case 4: value.surfaces.append(try SurfaceInfoMessage.decodeBody(try reader.readLengthDelimited()))
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct WindowCreatedMessage: Equatable {
    var revision: UInt64 = 0
    var window = WindowInfoMessage()

    static func decodeBody(_ data: Data) throws -> WindowCreatedMessage {
        var reader = ProtobufReader(data)
        var value = WindowCreatedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.window = try WindowInfoMessage.decodeBody(try reader.readLengthDelimited())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct WindowUpdatedMessage: Equatable {
    var revision: UInt64 = 0
    var window = WindowInfoMessage()

    static func decodeBody(_ data: Data) throws -> WindowUpdatedMessage {
        var reader = ProtobufReader(data)
        var value = WindowUpdatedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.window = try WindowInfoMessage.decodeBody(try reader.readLengthDelimited())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct WindowTitleChangedMessage: Equatable {
    var revision: UInt64 = 0
    var windowID = ""
    var title = ""

    static func decodeBody(_ data: Data) throws -> WindowTitleChangedMessage {
        var reader = ProtobufReader(data)
        var value = WindowTitleChangedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.title = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct WindowApplicationChangedMessage: Equatable {
    var revision: UInt64 = 0
    var windowID = ""
    var applicationID = ""

    static func decodeBody(_ data: Data) throws -> WindowApplicationChangedMessage {
        var reader = ProtobufReader(data)
        var value = WindowApplicationChangedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.applicationID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct WindowStateChangedMessage: Equatable {
    var revision: UInt64 = 0
    var windowID = ""
    var state: UInt32 = 0

    static func decodeBody(_ data: Data) throws -> WindowStateChangedMessage {
        var reader = ProtobufReader(data)
        var value = WindowStateChangedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.state = UInt32(try reader.readVarint())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct WindowMappedMessage: Equatable {
    var revision: UInt64 = 0
    var windowID = ""
    var mapped = false

    static func decodeBody(_ data: Data) throws -> WindowMappedMessage {
        var reader = ProtobufReader(data)
        var value = WindowMappedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.mapped = try reader.readVarint() != 0
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct WindowClosedMessage: Equatable {
    var revision: UInt64 = 0
    var windowID = ""

    static func decodeBody(_ data: Data) throws -> WindowClosedMessage {
        var reader = ProtobufReader(data)
        var value = WindowClosedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct SurfaceInfoMessage: Equatable {
    var surfaceID = ""
    var windowID = ""
    var parentSurfaceID = ""
    var role: UInt32 = 0
    var x: Int32 = 0
    var y: Int32 = 0
    var width: UInt32 = 0
    var height: UInt32 = 0
    var z: UInt32 = 0

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, surfaceID)
        writer.writeString(2, windowID)
        writer.writeString(3, parentSurfaceID)
        writer.writeUInt32(4, role)
        writer.writeInt32(5, x)
        writer.writeInt32(6, y)
        writer.writeUInt32(7, width)
        writer.writeUInt32(8, height)
        writer.writeUInt32(9, z)
        return writer.data
    }

    static func decodeBody(_ data: Data) throws -> SurfaceInfoMessage {
        var reader = ProtobufReader(data)
        var value = SurfaceInfoMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.surfaceID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 2: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.parentSurfaceID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 4: value.role = UInt32(try reader.readVarint())
            case 5: value.x = Int32(truncatingIfNeeded: try reader.readVarint())
            case 6: value.y = Int32(truncatingIfNeeded: try reader.readVarint())
            case 7: value.width = UInt32(try reader.readVarint())
            case 8: value.height = UInt32(try reader.readVarint())
            case 9: value.z = UInt32(try reader.readVarint())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct SurfaceCreatedMessage: Equatable {
    var revision: UInt64 = 0
    var surface = SurfaceInfoMessage()

    static func decodeBody(_ data: Data) throws -> SurfaceCreatedMessage {
        var reader = ProtobufReader(data)
        var value = SurfaceCreatedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.surface = try SurfaceInfoMessage.decodeBody(try reader.readLengthDelimited())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct SurfaceUpdatedMessage: Equatable {
    var revision: UInt64 = 0
    var surface = SurfaceInfoMessage()

    static func decodeBody(_ data: Data) throws -> SurfaceUpdatedMessage {
        var reader = ProtobufReader(data)
        var value = SurfaceUpdatedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.surface = try SurfaceInfoMessage.decodeBody(try reader.readLengthDelimited())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct SurfaceDestroyedMessage: Equatable {
    var revision: UInt64 = 0
    var surfaceID = ""
    var windowID = ""

    static func decodeBody(_ data: Data) throws -> SurfaceDestroyedMessage {
        var reader = ProtobufReader(data)
        var value = SurfaceDestroyedMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.revision = try reader.readVarint()
            case 2: value.surfaceID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

/// Server -> client: informational codec/stream config for a RemoteWindow.
struct WindowVideoConfigMessage: Equatable {
    var windowID = ""
    var codec: UInt32 = 0
    var chroma: UInt32 = 0
    var width: UInt32 = 0
    var height: UInt32 = 0
    var frameRate: UInt32 = 0
    var bitrateKbps: UInt32 = 0
    var gop: UInt32 = 0
    var lowLatency = false

    static func decodeBody(_ data: Data) throws -> WindowVideoConfigMessage {
        var reader = ProtobufReader(data)
        var value = WindowVideoConfigMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 2: value.codec = UInt32(try reader.readVarint())
            case 3: value.chroma = UInt32(try reader.readVarint())
            case 4: value.width = UInt32(try reader.readVarint())
            case 5: value.height = UInt32(try reader.readVarint())
            case 6: value.frameRate = UInt32(try reader.readVarint())
            case 7: value.bitrateKbps = UInt32(try reader.readVarint())
            case 8: value.gop = UInt32(try reader.readVarint())
            case 9: value.lowLatency = try reader.readVarint() != 0
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

/// Client -> server: ask the encoder for a keyframe.
struct RequestKeyframeMessage: Equatable {
    var windowID = ""
    var reason = ""

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, windowID)
        writer.writeString(2, reason)
        return writer.data
    }
}

// MARK: - Data plane (surface stream header)

struct DamageRectMessage: Equatable {
    var x: Int32 = 0
    var y: Int32 = 0
    var width: UInt32 = 0
    var height: UInt32 = 0

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeInt32(1, x)
        writer.writeInt32(2, y)
        writer.writeUInt32(3, width)
        writer.writeUInt32(4, height)
        return writer.data
    }

    static func decodeBody(_ data: Data) throws -> DamageRectMessage {
        var reader = ProtobufReader(data)
        var value = DamageRectMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.x = Int32(truncatingIfNeeded: try reader.readVarint())
            case 2: value.y = Int32(truncatingIfNeeded: try reader.readVarint())
            case 3: value.width = UInt32(try reader.readVarint())
            case 4: value.height = UInt32(try reader.readVarint())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

struct SurfaceStreamHeaderMessage: Equatable {
    var kind: UInt32 = 0
    var surfaceID = ""
    var windowID = ""
    var frameID: UInt64 = 0
    var width: UInt32 = 0
    var height: UInt32 = 0
    var stride: UInt32 = 0
    var format: UInt32 = 0
    var payloadLen: UInt64 = 0
    var damage: [DamageRectMessage] = []
    var streamType: UInt32 = AquaProtocol.DataStreamType.surfaceShm

    static func decodeBody(_ data: Data) throws -> SurfaceStreamHeaderMessage {
        var reader = ProtobufReader(data)
        var value = SurfaceStreamHeaderMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.kind = UInt32(try reader.readVarint())
            case 2: value.surfaceID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 4: value.frameID = try reader.readVarint()
            case 5: value.width = UInt32(try reader.readVarint())
            case 6: value.height = UInt32(try reader.readVarint())
            case 7: value.stride = UInt32(try reader.readVarint())
            case 8: value.format = UInt32(try reader.readVarint())
            case 9: value.payloadLen = try reader.readVarint()
            case 10: value.damage.append(try DamageRectMessage.decodeBody(try reader.readLengthDelimited()))
            case 20: value.streamType = UInt32(try reader.readVarint())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

/// Server-opened data-plane stream discriminator. Decoding a header into this
/// tiny message reads only `stream_type` (field 20) and skips everything else,
/// so it works for both `SurfaceStreamHeader` and `WindowVideoStreamHeader`
/// without knowing which one it is. A missing field (0) means SHM.
struct DataStreamProbe: Equatable {
    var streamType: UInt32 = 0

    static func decodeBody(_ data: Data) throws -> DataStreamProbe {
        var reader = ProtobufReader(data)
        var value = DataStreamProbe()
        while let (field, wire) = try reader.readTag() {
            if field == 20 {
                value.streamType = UInt32(try reader.readVarint())
            } else {
                try reader.skip(wire)
            }
        }
        return value
    }
}

struct FramePresentedMessage: Equatable {
    var surfaceID = ""
    var frameID: UInt64 = 0
    var presentationTimeUS: UInt64 = 0

    func encodeBody() -> Data {
        var writer = ProtobufWriter()
        writer.writeString(1, surfaceID)
        writer.writeUInt64(2, frameID)
        writer.writeUInt64(3, presentationTimeUS)
        return writer.data
    }
}

/// Data plane: one encoded stream per RemoteWindow (phase 3C).
///
/// `[be32 header_len][WindowVideoStreamHeader][payload_len raw bytes]`.
struct WindowVideoStreamHeaderMessage: Equatable {
    var kind: UInt32 = 0
    var windowID = ""
    var codec: UInt32 = 0
    var chroma: UInt32 = 0
    var width: UInt32 = 0
    var height: UInt32 = 0
    var frameID: UInt64 = 0
    var keyframe = false
    var ptsUS: UInt64 = 0
    var payloadLen: UInt64 = 0
    var codecConfig = false
    var streamType: UInt32 = 0

    static func decodeBody(_ data: Data) throws -> WindowVideoStreamHeaderMessage {
        var reader = ProtobufReader(data)
        var value = WindowVideoStreamHeaderMessage()
        while let (field, wire) = try reader.readTag() {
            switch field {
            case 1: value.kind = UInt32(try reader.readVarint())
            case 2: value.windowID = String(decoding: try reader.readLengthDelimited(), as: UTF8.self)
            case 3: value.codec = UInt32(try reader.readVarint())
            case 4: value.chroma = UInt32(try reader.readVarint())
            case 5: value.width = UInt32(try reader.readVarint())
            case 6: value.height = UInt32(try reader.readVarint())
            case 7: value.frameID = try reader.readVarint()
            case 8: value.keyframe = try reader.readVarint() != 0
            case 9: value.ptsUS = try reader.readVarint()
            case 10: value.payloadLen = try reader.readVarint()
            case 11: value.codecConfig = try reader.readVarint() != 0
            case 20: value.streamType = UInt32(try reader.readVarint())
            default: try reader.skip(wire)
            }
        }
        return value
    }
}

// MARK: - Envelopes

enum AquaServerMessage: Equatable {
    case hello(ServerHelloMessage)
    case snapshot(ServerSnapshotMessage)
    case windowCreated(WindowCreatedMessage)
    case windowUpdated(WindowUpdatedMessage)
    case windowTitleChanged(WindowTitleChangedMessage)
    case windowApplicationChanged(WindowApplicationChangedMessage)
    case windowStateChanged(WindowStateChangedMessage)
    case windowMapped(WindowMappedMessage)
    case windowClosed(WindowClosedMessage)
    case surfaceCreated(SurfaceCreatedMessage)
    case surfaceUpdated(SurfaceUpdatedMessage)
    case surfaceDestroyed(SurfaceDestroyedMessage)
    case windowVideoConfig(WindowVideoConfigMessage)

    static func decode(tag: UInt32, payload: Data) -> AquaServerMessage? {
        do {
            switch tag {
            case AquaProtocol.ServerTag.hello: return .hello(try .decodeBody(payload))
            case AquaProtocol.ServerTag.snapshot: return .snapshot(try .decodeBody(payload))
            case AquaProtocol.ServerTag.windowCreated: return .windowCreated(try .decodeBody(payload))
            case AquaProtocol.ServerTag.windowUpdated: return .windowUpdated(try .decodeBody(payload))
            case AquaProtocol.ServerTag.windowTitleChanged: return .windowTitleChanged(try .decodeBody(payload))
            case AquaProtocol.ServerTag.windowApplicationChanged:
                return .windowApplicationChanged(try .decodeBody(payload))
            case AquaProtocol.ServerTag.windowStateChanged: return .windowStateChanged(try .decodeBody(payload))
            case AquaProtocol.ServerTag.windowMapped: return .windowMapped(try .decodeBody(payload))
            case AquaProtocol.ServerTag.windowClosed: return .windowClosed(try .decodeBody(payload))
            case AquaProtocol.ServerTag.surfaceCreated: return .surfaceCreated(try .decodeBody(payload))
            case AquaProtocol.ServerTag.surfaceUpdated: return .surfaceUpdated(try .decodeBody(payload))
            case AquaProtocol.ServerTag.surfaceDestroyed: return .surfaceDestroyed(try .decodeBody(payload))
            case AquaProtocol.ServerTag.windowVideoConfig:
                return .windowVideoConfig(try .decodeBody(payload))
            default: return nil // unknown tag: forward compatible, ignore
            }
        } catch {
            return nil
        }
    }
}

enum AquaClientMessage: Equatable {
    case hello(ClientHelloMessage)
    case viewportChanged(ViewportChangedMessage)
    case focusRequested(WindowFocusRequestedMessage)
    case pointerMoved(PointerMovedMessage)
    case pointerButton(PointerButtonMessage)
    case pointerScroll(PointerScrollMessage)
    case key(KeyEventMessage)
    case touch(TouchEventMessage)
    case framePresented(FramePresentedMessage)
    case requestKeyframe(RequestKeyframeMessage)

    var tag: UInt32 {
        switch self {
        case .hello: return AquaProtocol.ClientTag.hello
        case .viewportChanged: return AquaProtocol.ClientTag.viewportChanged
        case .focusRequested: return AquaProtocol.ClientTag.windowFocusRequested
        case .pointerMoved: return AquaProtocol.ClientTag.pointerMoved
        case .pointerButton: return AquaProtocol.ClientTag.pointerButton
        case .pointerScroll: return AquaProtocol.ClientTag.pointerScroll
        case .key: return AquaProtocol.ClientTag.keyEvent
        case .touch: return AquaProtocol.ClientTag.touchEvent
        case .framePresented: return AquaProtocol.ClientTag.framePresented
        case .requestKeyframe: return AquaProtocol.ClientTag.requestKeyframe
        }
    }

    var body: Data {
        switch self {
        case .hello(let m): return m.encodeBody()
        case .viewportChanged(let m): return m.encodeBody()
        case .focusRequested(let m): return m.encodeBody()
        case .pointerMoved(let m): return m.encodeBody()
        case .pointerButton(let m): return m.encodeBody()
        case .pointerScroll(let m): return m.encodeBody()
        case .key(let m): return m.encodeBody()
        case .touch(let m): return m.encodeBody()
        case .framePresented(let m): return m.encodeBody()
        case .requestKeyframe(let m): return m.encodeBody()
        }
    }

    func encodeFrame() -> Data {
        AquaFrame.encode(tag: tag, payload: body)
    }
}
