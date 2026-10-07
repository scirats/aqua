import UIKit

/// Neutral decoder boundary for one `RemoteWindow` video stream.
///
/// `VideoStreamModel` decides *what* to decode; a `VideoDecoding` implementation
/// owns *how*. Swapping `AVSampleBufferDisplayLayer` for a `CVPixelBuffer`+Metal
/// path later must not touch the stream model or the transport
/// (see `docs/VIDEO.md`).
@MainActor
protocol VideoDecoding: AnyObject {
    /// The view that presents decoded frames.
    var view: UIView { get }

    /// Apply a configuration. When `codecConfiguration` is present it carries
    /// the parameter sets needed to build the format description.
    func configure(_ configuration: WindowVideoConfiguration)

    /// Decode and present one encoded access unit.
    func decode(_ frame: EncodedVideoFrame)

    /// Drop decoder state (call before a new keyframe after an error/resize).
    func reset()
}
