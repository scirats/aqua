import Foundation

/// Minimal model of the future remote surface hierarchy.
///
/// This exists so the architecture does *not* assume `wl_surface == scene`.
/// A future `xdg_toplevel` maps to a `RemoteWindow`; the window then owns a
/// tree of surfaces (main surface, subsurfaces, popups). Phase 1 never renders
/// or transports these; it only preserves the shape of the relationship.
struct RemoteSurface: Identifiable, Hashable, Sendable {
    enum Kind: String, Hashable, Sendable {
        case main
        case subsurface
        case popup
    }

    let id: String
    var kind: Kind
    var parentID: String?
    var frame: CGRect
    var isMapped: Bool
}

struct RemoteSurfaceTree: Hashable, Sendable {
    /// The remote window this tree belongs to.
    var windowID: RemoteWindowID
    /// Flattened surfaces, ordered parents before children.
    var surfaces: [RemoteSurface]

    init(windowID: RemoteWindowID, surfaces: [RemoteSurface] = []) {
        self.windowID = windowID
        self.surfaces = surfaces
    }

    var mainSurface: RemoteSurface? {
        surfaces.first { $0.kind == .main }
    }

    var subsurfaces: [RemoteSurface] {
        surfaces.filter { $0.kind == .subsurface }
    }

    var popups: [RemoteSurface] {
        surfaces.filter { $0.kind == .popup }
    }
}
