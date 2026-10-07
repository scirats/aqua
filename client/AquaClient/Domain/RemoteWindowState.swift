import Foundation

/// High level state of a remote window.
///
/// The remote side owns this state. iPadOS scene state is *not* mirrored here:
/// a disconnected scene does not imply `.closed`.
enum RemoteWindowState: String, Codable, Sendable, CaseIterable {
    case normal
    case minimized
    case maximized
    case fullscreen
    /// Terminal state. Closed windows are removed from `RemoteWindowStore`.
    case closed

    var isClosed: Bool { self == .closed }
}
