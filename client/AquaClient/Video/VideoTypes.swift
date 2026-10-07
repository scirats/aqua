import Foundation

/// Codec identifiers mirroring `aqua.v1.VideoCodec`.
enum AquaVideoCodec: UInt32 {
    case h264 = 1
    case hevc = 2
    case av1 = 3

    var displayName: String {
        switch self {
        case .h264: return "H.264"
        case .hevc: return "HEVC"
        case .av1: return "AV1"
        }
    }
}

/// Chroma layout mirroring `aqua.v1.VideoChroma`.
enum AquaVideoChroma: UInt32 {
    case nv12 = 1
    case p010 = 2
}

/// Decoder configuration for one `RemoteWindow` video stream.
///
/// `codecConfiguration` carries the parameter sets (VPS/SPS/PPS) when the server
/// sends a data-plane `CONFIG` with `codec_config = true`.
struct WindowVideoConfiguration: Equatable, Sendable {
    var windowID: String
    var codec: UInt32
    var chroma: UInt32
    var width: UInt32
    var height: UInt32
    var codecConfiguration: Data?

    var videoCodec: AquaVideoCodec? { AquaVideoCodec(rawValue: codec) }
}

/// One encoded access unit as received on a window video stream.
struct EncodedVideoFrame: Equatable, Sendable {
    var windowID: String
    var codec: UInt32
    var chroma: UInt32
    var width: UInt32
    var height: UInt32
    var frameID: UInt64
    var keyframe: Bool
    var ptsUS: UInt64
    var data: Data

    var videoCodec: AquaVideoCodec? { AquaVideoCodec(rawValue: codec) }
}
