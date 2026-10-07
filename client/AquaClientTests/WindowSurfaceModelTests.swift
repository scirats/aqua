import XCTest
@testable import AquaClient

@MainActor
final class WindowSurfaceModelTests: XCTestCase {
    private func surface(
        _ id: String,
        role: UInt32,
        parent: String? = nil,
        x: Double = 0,
        y: Double = 0,
        width: Double,
        height: Double,
        z: UInt32 = 0
    ) -> AquaSurface {
        AquaSurface(
            surfaceID: id,
            windowID: "window-1",
            parentSurfaceID: parent,
            role: role,
            position: CGPoint(x: x, y: y),
            size: CGSize(width: width, height: height),
            z: z
        )
    }

    private func frame(_ surfaceID: String, _ frameID: UInt64, red: Bool = true, width: UInt32 = 2, height: UInt32 = 2) -> SurfaceFrameData {
        var data = Data()
        let pixel: [UInt8] = red ? [0x00, 0x00, 0xFF, 0x00] : [0xFF, 0xFF, 0xFF, 0x00] // XRGB red / white
        for _ in 0..<Int(width * height) { data.append(contentsOf: pixel) }
        return SurfaceFrameData(
            surfaceID: surfaceID,
            windowID: "window-1",
            frameID: frameID,
            width: width,
            height: height,
            stride: width * 4,
            format: AquaProtocol.SurfaceFormat.xrgb8888,
            data: data
        )
    }

    func testFrameIDMonotonicLatestWins() {
        let model = WindowSurfaceModel(windowID: "window-1")
        model.setSurfaces([surface("surface-1", role: AquaProtocol.SurfaceRole.toplevel, width: 2, height: 2)])
        model.store(frame("surface-1", 1))
        model.store(frame("surface-1", 2))
        model.store(frame("surface-1", 3))
        XCTAssertEqual(model.frames["surface-1"]?.frameID, 3)
        XCTAssertEqual(model.droppedFrames, 0)
    }

    func testStaleFrameDiscarded() {
        let model = WindowSurfaceModel(windowID: "window-1")
        model.setSurfaces([surface("surface-1", role: AquaProtocol.SurfaceRole.toplevel, width: 2, height: 2)])
        model.store(frame("surface-1", 5))
        model.store(frame("surface-1", 4)) // stale
        model.store(frame("surface-1", 5)) // duplicate
        XCTAssertEqual(model.frames["surface-1"]?.frameID, 5)
        XCTAssertEqual(model.droppedFrames, 2)
    }

    func testPresentedFrameIsNotRedrawn() {
        let model = WindowSurfaceModel(windowID: "window-1")
        model.setSurfaces([surface("surface-1", role: AquaProtocol.SurfaceRole.toplevel, width: 2, height: 2)])
        model.store(frame("surface-1", 5))
        model.markPresented(surfaceID: "surface-1", frameID: 5)
        model.store(frame("surface-1", 3))
        XCTAssertEqual(model.droppedFrames, 1)
        XCTAssertEqual(model.frames["surface-1"]?.frameID, 5)
    }

    func testCompositionProducesRootSizedImage() {
        let model = WindowSurfaceModel(windowID: "window-1")
        model.setSurfaces([surface("surface-1", role: AquaProtocol.SurfaceRole.toplevel, width: 4, height: 3)])
        model.store(frame("surface-1", 1, red: true, width: 4, height: 3))
        let image = model.composition()
        XCTAssertEqual(image?.width, 4)
        XCTAssertEqual(image?.height, 3)
        let sample = image.flatMap { SurfacePixelFormat.sampleTopLeft($0) }
        XCTAssertEqual(sample?.r, 255)
        XCTAssertEqual(sample?.g, 0)
        XCTAssertEqual(sample?.b, 0)
    }

    func testAbsolutePositionAccumulatesParents() {
        let model = WindowSurfaceModel(windowID: "window-1")
        model.setSurfaces([
            surface("surface-1", role: AquaProtocol.SurfaceRole.toplevel, width: 10, height: 10),
            surface("surface-2", role: AquaProtocol.SurfaceRole.subsurface, parent: "surface-1", x: 3, y: 4, width: 4, height: 4, z: 1),
            surface("surface-3", role: AquaProtocol.SurfaceRole.subsurface, parent: "surface-2", x: 1, y: 1, width: 2, height: 2, z: 0),
        ])
        XCTAssertEqual(model.absolutePosition(for: "surface-1"), CGPoint(x: 0, y: 0))
        XCTAssertEqual(model.absolutePosition(for: "surface-2"), CGPoint(x: 3, y: 4))
        XCTAssertEqual(model.absolutePosition(for: "surface-3"), CGPoint(x: 4, y: 5))
    }

    func testSubsurfaceCompositionKeepsRootSize() {
        let model = WindowSurfaceModel(windowID: "window-1")
        model.setSurfaces([
            surface("surface-1", role: AquaProtocol.SurfaceRole.toplevel, width: 4, height: 4),
            surface("surface-2", role: AquaProtocol.SurfaceRole.subsurface, parent: "surface-1", x: 1, y: 1, width: 2, height: 2, z: 1),
        ])
        model.store(frame("surface-1", 1, red: false, width: 4, height: 4))
        model.store(frame("surface-2", 1, red: true, width: 2, height: 2))
        let image = model.composition()
        XCTAssertEqual(image?.width, 4)
        XCTAssertEqual(image?.height, 4)
        XCTAssertEqual(model.surfaces.count, 2)
    }

    func testRemoveSurfaceDropsFrame() {
        let model = WindowSurfaceModel(windowID: "window-1")
        model.setSurfaces([
            surface("surface-1", role: AquaProtocol.SurfaceRole.toplevel, width: 4, height: 4),
            surface("surface-2", role: AquaProtocol.SurfaceRole.subsurface, parent: "surface-1", width: 2, height: 2, z: 1),
        ])
        model.store(frame("surface-2", 1))
        model.removeSurface("surface-2")
        XCTAssertNil(model.frames["surface-2"])
        XCTAssertNil(model.surfaces["surface-2"])
    }
}
