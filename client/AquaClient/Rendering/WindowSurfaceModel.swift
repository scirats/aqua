import CoreGraphics
import Foundation

/// Owned pixels for one surface, as received from the server.
struct SurfaceFrameData: Equatable, Sendable {
    var surfaceID: String
    var windowID: String
    var frameID: UInt64
    var width: UInt32
    var height: UInt32
    var stride: UInt32
    var format: UInt32
    var data: Data
}

/// A composed image for one window (CGImage is immutable and thread-safe).
struct SurfaceComposition: @unchecked Sendable {
    let windowID: String
    let image: CGImage?
    let frameIDs: [String: UInt64]
}

/// Composes the surfaces of one `RemoteWindow` into a single image, applying the
/// same backpressure rules as the server: **latest frame wins**, stale frames are
/// discarded, and memory is bounded (one frame per surface).
@MainActor
final class WindowSurfaceModel {
    let windowID: String
    private(set) var surfaces: [String: AquaSurface] = [:]
    private(set) var frames: [String: SurfaceFrameData] = [:]
    private(set) var displayedFrameID: [String: UInt64] = [:]
    private(set) var droppedFrames = 0
    private(set) var composedFrames = 0

    init(windowID: String) {
        self.windowID = windowID
    }

    func setSurfaces(_ list: [AquaSurface]) {
        for surface in list where surface.windowID == windowID {
            surfaces[surface.surfaceID] = surface
        }
        // Drop frames for surfaces that no longer exist.
        let valid = Set(surfaces.keys)
        frames = frames.filter { valid.contains($0.key) }
        displayedFrameID = displayedFrameID.filter { valid.contains($0.key) }
    }

    func removeSurface(_ surfaceID: String) {
        surfaces.removeValue(forKey: surfaceID)
        frames.removeValue(forKey: surfaceID)
        displayedFrameID.removeValue(forKey: surfaceID)
    }

    /// Latest-frame-wins with stale discard.
    func store(_ frame: SurfaceFrameData) {
        guard frame.windowID == windowID else { return }
        if let existing = frames[frame.surfaceID]?.frameID, frame.frameID <= existing {
            droppedFrames += 1
            return
        }
        if let presented = displayedFrameID[frame.surfaceID], frame.frameID <= presented {
            droppedFrames += 1
            return
        }
        frames[frame.surfaceID] = frame
    }

    func markPresented(surfaceID: String, frameID: UInt64) {
        displayedFrameID[surfaceID] = max(displayedFrameID[surfaceID] ?? 0, frameID)
    }

    /// Absolute (window-space) position of a surface, accumulating parent
    /// offsets. Wayland positions are top-left relative to the parent.
    func absolutePosition(for surfaceID: String) -> CGPoint? {
        guard var surface = surfaces[surfaceID] else { return nil }
        var x = surface.position.x
        var y = surface.position.y
        var guardCount = 0
        while let parentID = surface.parentSurfaceID, let parent = surfaces[parentID] {
            x += parent.position.x
            y += parent.position.y
            surface = parent
            guardCount += 1
            if guardCount > 32 { break }
        }
        return CGPoint(x: x, y: y)
    }

    private func orderedSurfaces() -> [AquaSurface] {
        surfaces.values.sorted { lhs, rhs in
            let lRoot = lhs.parentSurfaceID == nil
            let rRoot = rhs.parentSurfaceID == nil
            if lRoot != rRoot { return lRoot }
            return lhs.z < rhs.z
        }
    }

    /// Render the composed image at the root surface's logical size.
    func composition() -> CGImage? {
        guard let root = surfaces.values.first(where: { $0.role == AquaProtocol.SurfaceRole.toplevel }) else {
            return nil
        }
        let canvasWidth = Int(root.size.width)
        let canvasHeight = Int(root.size.height)
        guard canvasWidth > 0, canvasHeight > 0, canvasWidth <= 8192, canvasHeight <= 8192 else {
            return nil
        }
        guard let context = CGContext(
            data: nil,
            width: canvasWidth,
            height: canvasHeight,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            return nil
        }
        context.clear(CGRect(x: 0, y: 0, width: canvasWidth, height: canvasHeight))

        for surface in orderedSurfaces() {
            guard let frame = frames[surface.surfaceID],
                  let image = SurfacePixelFormat.makeImage(
                      width: frame.width,
                      height: frame.height,
                      stride: frame.stride,
                      format: frame.format,
                      data: frame.data
                  ),
                  let position = absolutePosition(for: surface.surfaceID) else {
                continue
            }
            // Wayland y grows downward; CGContext y grows upward.
            let rect = CGRect(
                x: position.x,
                y: CGFloat(canvasHeight) - position.y - CGFloat(image.height),
                width: CGFloat(image.width),
                height: CGFloat(image.height)
            )
            context.draw(image, in: rect)
        }

        composedFrames += 1
        return context.makeImage()
    }
}
