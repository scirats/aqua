import Foundation

/// Events produced by a `RemoteWindowService`.
///
/// The mock and the future QUIC transport both emit exactly this shape, which
/// is why it is defined in the domain layer and contains no UIKit.
enum RemoteWindowEvent: Sendable, Equatable {
    case created(RemoteWindow)
    case updated(RemoteWindow)
    case closed(RemoteWindowID)
}

extension RemoteWindowEvent {
    var windowID: RemoteWindowID {
        switch self {
        case .created(let window): return window.id
        case .updated(let window): return window.id
        case .closed(let id): return id
        }
    }

    var kindDescription: String {
        switch self {
        case .created: return "window.created"
        case .updated: return "window.updated"
        case .closed: return "window.closed"
        }
    }
}
