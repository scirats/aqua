import Foundation

/// Abstraction over the UIKit scene-session machinery.
///
/// `SceneCoordinator` never touches `UISceneSession` directly. This keeps the
/// coordination logic (in particular the rule that *UI lifecycle != remote
/// lifecycle*) unit testable with a fake backend.
@MainActor
protocol SceneSessionBackend: AnyObject {
    /// Persistent identifier of the scene session currently representing `id`.
    func sessionIdentifier(forRemoteWindow id: RemoteWindowID) -> String?

    /// Brings an existing scene session to the foreground.
    func activateSession(identifier: String)

    /// Asks iPadOS to create a brand new scene session for this identity.
    func requestNewSession(identity: RemoteWindowIdentity)

    /// Asks iPadOS to destroy the scene session and disconnect its scene.
    func destroySession(identifier: String)

    /// Pushes model metadata (title, size restrictions) to a connected scene.
    func applyMetadata(remoteWindow: RemoteWindow)

    /// Called when UIKit reports that a remote window's scene disconnected.
    /// Deliberately separate from `destroySession`.
    func sceneDidDisconnect(identifier: String)
}

extension SceneSessionBackend {
    func sceneDidDisconnect(identifier: String) {}
}
