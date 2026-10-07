import Foundation

/// Single source of truth for remote windows.
///
/// Implemented as an `actor` so callers can mutate and read it from any
/// isolation domain without an unsafe global mutable state. The store is
/// intentionally UIKit free: it must be unit testable without a simulator.
actor RemoteWindowStore {
    private var windows: [RemoteWindowID: RemoteWindow] = [:]
    private var viewports: [RemoteWindowID: RemoteViewport] = [:]

    init() {}

    // MARK: - Queries

    func window(_ id: RemoteWindowID) -> RemoteWindow? {
        windows[id]
    }

    func viewport(_ id: RemoteWindowID) -> RemoteViewport? {
        viewports[id]
    }

    /// Windows in a deterministic order (sorted by id) so UI/tests are stable.
    func allWindows() -> [RemoteWindow] {
        windows.values.sorted { $0.id.value < $1.id.value }
    }

    func windows(forApplication id: RemoteApplicationID) -> [RemoteWindow] {
        windows.values
            .filter { $0.applicationID == id }
            .sorted { $0.id.value < $1.id.value }
    }

    var count: Int { windows.count }

    func contains(_ id: RemoteWindowID) -> Bool {
        windows[id] != nil
    }

    // MARK: - Mutations

    /// Inserts or replaces a window. Returns `false` when the id already exists.
    @discardableResult
    func register(_ window: RemoteWindow) -> Bool {
        guard windows[window.id] == nil else { return false }
        windows[window.id] = window
        return true
    }

    func upsert(_ window: RemoteWindow) {
        windows[window.id] = window
    }

    /// Removes a window. Returns the removed value, or `nil` when absent.
    @discardableResult
    func remove(_ id: RemoteWindowID) -> RemoteWindow? {
        viewports[id] = nil
        return windows.removeValue(forKey: id)
    }

    func setViewport(_ viewport: RemoteViewport, for id: RemoteWindowID) {
        viewports[id] = viewport
    }

    /// Applies a service event. Returns the resulting change for observers.
    @discardableResult
    func apply(_ event: RemoteWindowEvent) -> RemoteWindowChange {
        switch event {
        case .created(let window):
            windows[window.id] = window
            return .created(window)
        case .updated(let window):
            windows[window.id] = window
            return .updated(window)
        case .closed(let id):
            viewports[id] = nil
            windows[id] = nil
            return .closed(id)
        }
    }
}

/// Result of applying an event, handed to the `SceneCoordinator`.
enum RemoteWindowChange: Sendable, Equatable {
    case created(RemoteWindow)
    case updated(RemoteWindow)
    case closed(RemoteWindowID)
}
