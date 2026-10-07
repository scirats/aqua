import Foundation

/// Per-window state machine for the video data plane.
///
/// Inter-frame codecs cannot drop arbitrary frames: a decoder must start (and
/// resume) at a keyframe. This model enforces that contract on the client:
///
/// - a new configuration forces `awaitingKeyframe`;
/// - frames before the first keyframe are dropped, and a keyframe is requested
///   once (not spammed);
/// - stale/duplicate frames (by `frameID`) are dropped.
///
/// It is pure logic (no AVFoundation/UIKit), so it is fully unit-testable.
final class VideoStreamModel {
    let windowID: String

    private(set) var configuration: WindowVideoConfiguration?
    private(set) var awaitingKeyframe = true
    private(set) var lastFrameID: UInt64?
    private(set) var forwardedFrames = 0
    private(set) var droppedFrames = 0
    private(set) var requestedKeyframes = 0

    /// Invoked when the model wants the server to emit a keyframe.
    var onRequestKeyframe: ((_ windowID: String, _ reason: String) -> Void)?

    private var keyframeRequestedWhileWaiting = false

    init(windowID: String) {
        self.windowID = windowID
    }

    enum Outcome: Equatable {
        /// The frame is decodable and should be handed to the decoder.
        case forward(EncodedVideoFrame)
        /// A keyframe was requested; keep waiting.
        case requestedKeyframe
        /// Waiting for the first keyframe (already requested).
        case waitingForKeyframe
        /// Older than an already-forwarded frame.
        case droppedStale
        /// Different window or no configuration.
        case ignored
    }

    /// Apply a (re)configuration. Any configuration change invalidates the
    /// decoder state, so a fresh keyframe is required.
    @discardableResult
    func apply(_ configuration: WindowVideoConfiguration) -> Outcome {
        self.configuration = configuration
        awaitingKeyframe = true
        lastFrameID = nil
        keyframeRequestedWhileWaiting = false
        return .waitingForKeyframe
    }

    /// Feed one encoded frame and decide what to do with it.
    @discardableResult
    func receive(_ frame: EncodedVideoFrame) -> Outcome {
        guard frame.windowID == windowID else { return .ignored }
        guard let configuration else {
            requestKeyframe(reason: "no configuration")
            return .requestedKeyframe
        }
        // A size/codec change means the decoder must be rebuilt from a keyframe.
        if frame.width != configuration.width || frame.height != configuration.height
            || frame.codec != configuration.codec {
            requestKeyframe(reason: "size or codec changed")
            return .requestedKeyframe
        }
        if let lastFrameID, frame.frameID <= lastFrameID {
            droppedFrames += 1
            return .droppedStale
        }
        if awaitingKeyframe {
            guard frame.keyframe else {
                droppedFrames += 1
                if !keyframeRequestedWhileWaiting {
                    keyframeRequestedWhileWaiting = true
                    requestKeyframe(reason: "waiting for keyframe")
                    return .requestedKeyframe
                }
                return .waitingForKeyframe
            }
            awaitingKeyframe = false
            keyframeRequestedWhileWaiting = false
        }
        lastFrameID = frame.frameID
        forwardedFrames += 1
        return .forward(frame)
    }

    func reset() {
        configuration = nil
        awaitingKeyframe = true
        lastFrameID = nil
        keyframeRequestedWhileWaiting = false
    }

    private func requestKeyframe(reason: String) {
        requestedKeyframes += 1
        onRequestKeyframe?(windowID, reason)
    }
}
