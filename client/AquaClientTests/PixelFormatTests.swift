import XCTest
@testable import AquaClient

/// Pixel-format tests with known colour patterns. These detect channel swaps
/// (a real risk: `ARGB8888` is *not* `R,G,B,A` in memory on little-endian).
final class PixelFormatTests: XCTestCase {
    private func image(bytes: [UInt8], format: UInt32, width: UInt32 = 1, height: UInt32 = 1) -> CGImage? {
        let stride = width * 4
        return SurfacePixelFormat.makeImage(
            width: width,
            height: height,
            stride: stride,
            format: format,
            data: Data(bytes)
        )
    }

    private func assertPixel(_ bytes: [UInt8], format: UInt32, r: UInt8, g: UInt8, b: UInt8, a: UInt8, accuracy: Int = 0) {
        guard let image = image(bytes: bytes, format: format),
              let sample = SurfacePixelFormat.sampleTopLeft(image) else {
            return XCTFail("failed to build/sample image for \(bytes)")
        }
        XCTAssertEqual(Int(sample.r), Int(r), accuracy: accuracy, "red channel")
        XCTAssertEqual(Int(sample.g), Int(g), accuracy: accuracy, "green channel")
        XCTAssertEqual(Int(sample.b), Int(b), accuracy: accuracy, "blue channel")
        XCTAssertEqual(Int(sample.a), Int(a), accuracy: accuracy, "alpha channel")
    }

    // BGRA in memory for XRGB8888 (little-endian).
    func testXrgbRed() { assertPixel([0x00, 0x00, 0xFF, 0x00], format: AquaProtocol.SurfaceFormat.xrgb8888, r: 255, g: 0, b: 0, a: 255) }
    func testXrgbGreen() { assertPixel([0x00, 0xFF, 0x00, 0x00], format: AquaProtocol.SurfaceFormat.xrgb8888, r: 0, g: 255, b: 0, a: 255) }
    func testXrgbBlue() { assertPixel([0xFF, 0x00, 0x00, 0x00], format: AquaProtocol.SurfaceFormat.xrgb8888, r: 0, g: 0, b: 255, a: 255) }
    func testXrgbWhite() { assertPixel([0xFF, 0xFF, 0xFF, 0x00], format: AquaProtocol.SurfaceFormat.xrgb8888, r: 255, g: 255, b: 255, a: 255) }

    func testArgbOpaqueRed() {
        assertPixel([0x00, 0x00, 0xFF, 0xFF], format: AquaProtocol.SurfaceFormat.argb8888, r: 255, g: 0, b: 0, a: 255)
    }

    func testArgbTransparent() {
        assertPixel([0x00, 0x00, 0x00, 0x00], format: AquaProtocol.SurfaceFormat.argb8888, r: 0, g: 0, b: 0, a: 0)
    }

    func testArgbPremultipliedHalfRed() {
        // 50% alpha, premultiplied red: R=128, A=128 -> BGRA [0,0,128,128].
        assertPixel([0x00, 0x00, 0x80, 0x80], format: AquaProtocol.SurfaceFormat.argb8888, r: 128, g: 0, b: 0, a: 128, accuracy: 2)
    }

    func testUnsupportedFormatRejected() {
        XCTAssertNil(image(bytes: [0, 0, 0, 0], format: 99))
    }

    func testZeroDimensionRejected() {
        XCTAssertNil(SurfacePixelFormat.makeImage(width: 0, height: 1, stride: 0, format: AquaProtocol.SurfaceFormat.xrgb8888, data: Data()))
    }

    func testShortBufferRejected() {
        // 2x2 requires stride*height == 16, only 4 bytes provided.
        XCTAssertNil(SurfacePixelFormat.makeImage(width: 2, height: 2, stride: 8, format: AquaProtocol.SurfaceFormat.xrgb8888, data: Data([0, 0, 0, 0])))
    }

    func testMultiPixelImage() {
        // 2x1: red then green (XRGB).
        let bytes: [UInt8] = [0x00, 0x00, 0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00]
        let image = SurfacePixelFormat.makeImage(width: 2, height: 1, stride: 8, format: AquaProtocol.SurfaceFormat.xrgb8888, data: Data(bytes))
        XCTAssertEqual(image?.width, 2)
        XCTAssertEqual(image?.height, 1)
    }
}
