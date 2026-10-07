import XCTest
@testable import AquaClient

/// Keyframe-aware backpressure contract for the client video plane.
///
/// Inter-frame codecs cannot drop arbitrary frames, so these tests pin the rules:
/// start at a keyframe, drop stale frames, rebuild on configuration change.
final class VideoStreamModelTests: XCTestCase {
    private func config(
        windowID: String = "window-1",
        codec: UInt32 = AquaProtocol.VideoCodec.hevc,
        width: UInt32 = 800,
        height: UInt32 = 600
    ) -> WindowVideoConfiguration {
        WindowVideoConfiguration(
            windowID: windowID,
            codec: codec,
            chroma: AquaProtocol.VideoChroma.nv12,
            width: width,
            height: height,
            codecConfiguration: nil
        )
    }

    private func frame(
        _ id: UInt64,
        keyframe: Bool,
        width: UInt32 = 800,
        height: UInt32 = 600,
        codec: UInt32 = AquaProtocol.VideoCodec.hevc,
        windowID: String = "window-1"
    ) -> EncodedVideoFrame {
        EncodedVideoFrame(
            windowID: windowID,
            codec: codec,
            chroma: AquaProtocol.VideoChroma.nv12,
            width: width,
            height: height,
            frameID: id,
            keyframe: keyframe,
            ptsUS: id * 1000,
            data: Data([0x00, 0x00, 0x00, 0x01, 0x09, 0x10])
        )
    }

    func testForwardsFirstKeyframe() {
        let model = VideoStreamModel(windowID: "window-1")
        model.apply(config())
        guard case .forward(let forwarded) = model.receive(frame(1, keyframe: true)) else {
            return XCTFail("expected forward")
        }
        XCTAssertEqual(forwarded.frameID, 1)
        XCTAssertEqual(model.forwardedFrames, 1)
        XCTAssertFalse(model.awaitingKeyframe)
    }

    func testDropsUntilKeyframeAndRequestsOnce() {
        let model = VideoStreamModel(windowID: "window-1")
        var requests: [String] = []
        model.onRequestKeyframe = { _, reason in requests.append(reason) }
        model.apply(config())

        XCTAssertEqual(model.receive(frame(1, keyframe: false)), .requestedKeyframe)
        XCTAssertEqual(model.receive(frame(2, keyframe: false)), .waitingForKeyframe)
        XCTAssertEqual(model.droppedFrames, 2)
        XCTAssertEqual(requests.count, 1, "must not spam keyframe requests")
    }

    func testStaleAndDuplicateFramesDropped() {
        let model = VideoStreamModel(windowID: "window-1")
        model.apply(config())
        _ = model.receive(frame(5, keyframe: true))
        XCTAssertEqual(model.receive(frame(5, keyframe: true)), .droppedStale)
        XCTAssertEqual(model.receive(frame(4, keyframe: false)), .droppedStale)
        XCTAssertEqual(model.forwardedFrames, 1)
    }

    func testConfigurationChangeRequiresFreshKeyframe() {
        let model = VideoStreamModel(windowID: "window-1")
        model.apply(config())
        _ = model.receive(frame(1, keyframe: true))

        _ = model.apply(config(width: 1024, height: 768))
        XCTAssertTrue(model.awaitingKeyframe)
        XCTAssertEqual(
            model.receive(frame(2, keyframe: false, width: 1024, height: 768)),
            .requestedKeyframe
        )
        guard case .forward = model.receive(frame(3, keyframe: true, width: 1024, height: 768)) else {
            return XCTFail("expected forward after keyframe")
        }
    }

    func testSizeChangeRequestsKeyframe() {
        let model = VideoStreamModel(windowID: "window-1")
        model.apply(config())
        _ = model.receive(frame(1, keyframe: true))
        XCTAssertEqual(
            model.receive(frame(2, keyframe: true, width: 900, height: 600)),
            .requestedKeyframe
        )
    }

    func testForeignWindowIgnored() {
        let model = VideoStreamModel(windowID: "window-1")
        model.apply(config())
        XCTAssertEqual(model.receive(frame(1, keyframe: true, windowID: "window-2")), .ignored)
    }
}
