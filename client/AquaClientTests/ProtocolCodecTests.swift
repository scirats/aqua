import XCTest
@testable import AquaClient

/// Cross-language codec tests. The hex fixtures are produced by the Rust server
/// (`cargo run --example fixtures`), so a pass proves Swift and Rust agree on
/// the exact Aqua Protocol v1 wire bytes.
final class ProtocolCodecTests: XCTestCase {
    private func hex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }

    private func data(hex: String) -> Data {
        var bytes = [UInt8]()
        var index = hex.startIndex
        while index < hex.endIndex {
            let next = hex.index(index, offsetBy: 2)
            bytes.append(UInt8(hex[index..<next], radix: 16)!)
            index = next
        }
        return Data(bytes)
    }

    private func decodeServerFrame(_ hex: String) throws -> AquaServerMessage {
        let frame = data(hex: hex)
        let header = frame.prefix(AquaProtocol.headerLength)
        let (tag, length) = AquaFrame.decodeHeader(Data(header))!
        let payload = frame.subdata(in: AquaProtocol.headerLength..<(AquaProtocol.headerLength + length))
        return try XCTUnwrap(AquaServerMessage.decode(tag: tag, payload: payload))
    }

    func testDecodeServerHello() throws {
        let message = try decodeServerFrame("000000010000001608011206736573732d311a04616263642202081e282a")
        guard case .hello(let hello) = message else { return XCTFail("expected hello") }
        XCTAssertEqual(hello.protocolVersion, 1)
        XCTAssertEqual(hello.serverSessionID, "sess-1")
        XCTAssertEqual(hello.serverIdentity, "abcd")
        XCTAssertEqual(hello.capabilities.bits, 0b11110)
        XCTAssertEqual(hello.revision, 42)
        XCTAssertEqual(hello.error, "")
    }

    func testDecodeServerSnapshot() throws {
        let message = try decodeServerFrame(
            "0000000200000040082a1206736573732d311a340a0877696e646f772d3112126f72672e676e6f6d652e5465726d696e616c1a085465726d696e616c20022801300138f40340ac02"
        )
        guard case .snapshot(let snapshot) = message else { return XCTFail("expected snapshot") }
        XCTAssertEqual(snapshot.revision, 42)
        XCTAssertEqual(snapshot.serverSessionID, "sess-1")
        XCTAssertEqual(snapshot.windows.count, 1)
        let window = snapshot.windows[0]
        XCTAssertEqual(window.windowID, "window-1")
        XCTAssertEqual(window.applicationID, "org.gnome.Terminal")
        XCTAssertEqual(window.title, "Terminal")
        XCTAssertEqual(window.state, 2)
        XCTAssertTrue(window.mapped)
        XCTAssertTrue(window.hasMin)
        XCTAssertEqual(window.minWidth, 500)
        XCTAssertEqual(window.minHeight, 300)
        XCTAssertFalse(window.hasMax)
    }

    func testDecodeWindowCreated() throws {
        let message = try decodeServerFrame(
            "0000000300000038082b12340a0877696e646f772d3112126f72672e676e6f6d652e5465726d696e616c1a085465726d696e616c20022801300138f40340ac02"
        )
        guard case .windowCreated(let created) = message else { return XCTFail("expected created") }
        XCTAssertEqual(created.revision, 43)
        XCTAssertEqual(created.window.windowID, "window-1")
        XCTAssertTrue(created.window.mapped)
    }

    func testEncodeClientHelloMatchesRust() {
        var hello = ClientHelloMessage()
        hello.protocolVersion = 1
        hello.clientSessionID = "client-1"
        hello.capabilities.bits = 0b11110
        hello.clientName = "iPad"
        XCTAssertEqual(
            hex(AquaClientMessage.hello(hello).encodeFrame()),
            "000000280000001608011208636c69656e742d311a02081e220469506164"
        )
    }

    func testEncodeViewportMatchesRust() {
        var viewport = ViewportChangedMessage()
        viewport.windowID = "window-1"
        viewport.width = 800
        viewport.height = 600
        viewport.scale = 2.0
        viewport.isFinal = true
        XCTAssertEqual(
            hex(AquaClientMessage.viewportChanged(viewport).encodeFrame()),
            "000000290000001b0a0877696e646f772d3110a00618d8042100000000000000402801"
        )
    }

    func testEncodeKeyMatchesRust() {
        var key = KeyEventMessage()
        key.windowID = "window-1"
        key.keycode = 0
        key.characters = "a"
        key.modifiers = 0
        key.pressed = true
        XCTAssertEqual(
            hex(AquaClientMessage.key(key).encodeFrame()),
            "0000002e0000000f0a0877696e646f772d311a01612801"
        )
    }

    func testEncodePointerButtonMatchesRust() {
        var button = PointerButtonMessage()
        button.windowID = "window-1"
        button.button = 272
        button.pressed = true
        button.x = 12.5
        button.y = 34.5
        XCTAssertEqual(
            hex(AquaClientMessage.pointerButton(button).encodeFrame()),
            "0000002c000000210a0877696e646f772d311090021801210000000000002940290000000000404140"
        )
    }

    func testDecodeWindowVideoConfig() throws {
        let message = try decodeServerFrame(
            "0000000e0000001b0a0877696e646f772d311002180120a00628d804303c38c03e4801"
        )
        guard case .windowVideoConfig(let config) = message else {
            return XCTFail("expected windowVideoConfig")
        }
        XCTAssertEqual(config.windowID, "window-1")
        XCTAssertEqual(config.codec, AquaProtocol.VideoCodec.hevc)
        XCTAssertEqual(config.chroma, AquaProtocol.VideoChroma.nv12)
        XCTAssertEqual(config.width, 800)
        XCTAssertEqual(config.height, 600)
        XCTAssertEqual(config.frameRate, 60)
        XCTAssertEqual(config.bitrateKbps, 8000)
        XCTAssertEqual(config.gop, 0)
        XCTAssertTrue(config.lowLatency)
    }

    func testEncodeRequestKeyframeMatchesRust() {
        var request = RequestKeyframeMessage()
        request.windowID = "window-1"
        request.reason = "resize"
        XCTAssertEqual(
            hex(AquaClientMessage.requestKeyframe(request).encodeFrame()),
            "00000032000000120a0877696e646f772d311206726573697a65"
        )
    }

    private func decodeVideoStreamHeader(_ hex: String) throws -> WindowVideoStreamHeaderMessage {
        let bytes = data(hex: hex)
        let headerLength = (Int(bytes[0]) << 24) | (Int(bytes[1]) << 16)
            | (Int(bytes[2]) << 8) | Int(bytes[3])
        let header = bytes.subdata(in: 4..<(4 + headerLength))
        return try WindowVideoStreamHeaderMessage.decodeBody(header)
    }

    func testDecodeWindowVideoStreamHello() throws {
        let header = try decodeVideoStreamHeader("0000000f0801120877696e646f772d31a00102")
        XCTAssertEqual(header.kind, AquaProtocol.WindowVideoStreamKind.hello)
        XCTAssertEqual(header.windowID, "window-1")
        XCTAssertEqual(header.streamType, AquaProtocol.DataStreamType.windowVideo)
    }

    func testDecodeWindowVideoStreamFrame() throws {
        let header = try decodeVideoStreamHeader(
            "000000230803120877696e646f772d311802200128a00630d804382a400148c0843d5003a00102aabbcc"
        )
        XCTAssertEqual(header.kind, AquaProtocol.WindowVideoStreamKind.frame)
        XCTAssertEqual(header.windowID, "window-1")
        XCTAssertEqual(header.codec, AquaProtocol.VideoCodec.hevc)
        XCTAssertEqual(header.chroma, AquaProtocol.VideoChroma.nv12)
        XCTAssertEqual(header.width, 800)
        XCTAssertEqual(header.height, 600)
        XCTAssertEqual(header.frameID, 42)
        XCTAssertTrue(header.keyframe)
        XCTAssertEqual(header.ptsUS, 1_000_000)
        XCTAssertEqual(header.payloadLen, 3)
        XCTAssertFalse(header.codecConfig)
    }

    func testDecodeWindowVideoStreamConfigCarriesCodecConfigFlag() throws {
        let header = try decodeVideoStreamHeader(
            "0000001d0802120877696e646f772d311802200128a00630d80450045801a00102deadbeef"
        )
        XCTAssertEqual(header.kind, AquaProtocol.WindowVideoStreamKind.config)
        XCTAssertEqual(header.payloadLen, 4)
        XCTAssertTrue(header.codecConfig)
        XCTAssertEqual(header.streamType, AquaProtocol.DataStreamType.windowVideo)
    }

    /// The stream discriminator must classify SHM and video without decoding the
    /// full header of either type.
    func testDataStreamProbeClassifiesShmAndVideo() throws {
        let surface = data(hex: "0000001a08011209737572666163652d311a0877696e646f772d31a00101")
        let surfaceHeader = try probeStreamHeader(surface)
        XCTAssertEqual(surfaceHeader.streamType, AquaProtocol.DataStreamType.surfaceShm)

        let video = data(hex: "0000000f0801120877696e646f772d31a00102")
        let videoHeader = try probeStreamHeader(video)
        XCTAssertEqual(videoHeader.streamType, AquaProtocol.DataStreamType.windowVideo)
    }

    private func probeStreamHeader(_ bytes: Data) throws -> DataStreamProbe {
        let headerLength = (Int(bytes[0]) << 24) | (Int(bytes[1]) << 16)
            | (Int(bytes[2]) << 8) | Int(bytes[3])
        return try DataStreamProbe.decodeBody(bytes.subdata(in: 4..<(4 + headerLength)))
    }

    func testUnknownTagIsIgnored() {
        let frame = AquaFrame.encode(tag: 9999, payload: Data([1, 2, 3]))
        let header = frame.prefix(AquaProtocol.headerLength)
        let (tag, length) = AquaFrame.decodeHeader(Data(header))!
        let payload = frame.subdata(in: AquaProtocol.headerLength..<(AquaProtocol.headerLength + length))
        XCTAssertNil(AquaServerMessage.decode(tag: tag, payload: payload))
    }
}
