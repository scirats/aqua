import Foundation

/// Viewport of a remote window as observed on the iPad, in scene points.
///
/// In later phases this is what gets forwarded to the Linux compositor as an
/// `xdg_toplevel.configure` request. In phase 1 it is only logged.
struct RemoteViewport: Hashable, Codable, Sendable {
    var width: Double
    var height: Double
    var scale: Double

    init(width: Double, height: Double, scale: Double) {
        self.width = width
        self.height = height
        self.scale = scale
    }
}

extension RemoteViewport {
    var pixelWidth: Double { width * scale }
    var pixelHeight: Double { height * scale }

    /// Stable, log friendly representation: `width=1024 height=768 scale=2`.
    var logDescription: String {
        "width=\(Int(width.rounded())) height=\(Int(height.rounded())) scale=\(scale)"
    }
}
