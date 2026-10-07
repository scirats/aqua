import Foundation

/// Pure mapping between a `RemoteWindowID` and the persistent identifier of the
/// `UISceneSession` that represents it.
///
/// Kept free of UIKit so the identity logic (the part that must survive
/// lifecycle transitions) can be unit tested directly.
actor SceneSessionRegistry {
    private var sessionForWindow: [RemoteWindowID: String] = [:]
    private var windowForSession: [String: RemoteWindowID] = [:]

    init() {}

    func bind(window: RemoteWindowID, session persistentIdentifier: String) {
        // Drop a stale binding if the same window was previously bound elsewhere.
        if let previous = sessionForWindow[window], previous != persistentIdentifier {
            windowForSession[previous] = nil
        }
        // A session only ever represents one remote window.
        if let previousWindow = windowForSession[persistentIdentifier], previousWindow != window {
            sessionForWindow[previousWindow] = nil
        }
        sessionForWindow[window] = persistentIdentifier
        windowForSession[persistentIdentifier] = window
    }

    func session(for window: RemoteWindowID) -> String? {
        sessionForWindow[window]
    }

    func window(for persistentIdentifier: String) -> RemoteWindowID? {
        windowForSession[persistentIdentifier]
    }

    /// Removes a binding by window. Returns the session identifier that was bound.
    @discardableResult
    func unbind(window: RemoteWindowID) -> String? {
        guard let session = sessionForWindow.removeValue(forKey: window) else { return nil }
        windowForSession[session] = nil
        return session
    }

    /// Removes a binding by session. Returns the window that was bound.
    @discardableResult
    func unbind(session persistentIdentifier: String) -> RemoteWindowID? {
        guard let window = windowForSession.removeValue(forKey: persistentIdentifier) else { return nil }
        sessionForWindow[window] = nil
        return window
    }

    var bindingCount: Int { sessionForWindow.count }
}
