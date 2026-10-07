import Foundation
import os

/// Translates the remote window model into UIKit scene sessions.
///
/// Responsibilities:
///
///     RemoteWindow created  -> create/activate UIWindowScene
///     RemoteWindow title    -> update scene.title
///     RemoteWindow closed   -> destroy scene session
///     requested again       -> activate existing scene
///
/// Important invariant: a scene disconnect is *not* a remote window close.
/// Only an explicit `.closed` event tears down the session and the store entry.
@MainActor
final class SceneCoordinator {
    private let store: RemoteWindowStore
    private let registry: SceneSessionRegistry
    private let backend: SceneSessionBackend
    /// Guards against duplicate activation requests while a new session is being
    /// created but has not connected yet.
    private var pendingWindowIDs: Set<RemoteWindowID> = []

    init(store: RemoteWindowStore, registry: SceneSessionRegistry, backend: SceneSessionBackend) {
        self.store = store
        self.registry = registry
        self.backend = backend
    }

    // MARK: - Event entry point

    func handle(_ change: RemoteWindowChange) async {
        switch change {
        case .created(let window):
            await open(window)
        case .updated(let window):
            await update(window)
        case .closed(let id):
            await close(id)
        }
    }

    // MARK: - Operations

    func open(_ window: RemoteWindow) async {
        if let existing = backend.sessionIdentifier(forRemoteWindow: window.id) {
            Log.scene.debug("Activating existing scene for window=\(window.id.value, privacy: .public)")
            backend.activateSession(identifier: existing)
            return
        }
        if let bound = await registry.session(for: window.id) {
            Log.scene.debug("Activating bound scene for window=\(window.id.value, privacy: .public)")
            backend.activateSession(identifier: bound)
            return
        }
        guard !pendingWindowIDs.contains(window.id) else {
            Log.scene.debug("Scene request already pending for window=\(window.id.value, privacy: .public)")
            return
        }
        pendingWindowIDs.insert(window.id)
        Log.scene.notice("Requesting new scene for window=\(window.id.value, privacy: .public)")
        backend.requestNewSession(identity: RemoteWindowIdentity(windowID: window.id))
    }

    func update(_ window: RemoteWindow) async {
        guard window.state != .closed else {
            await close(window.id)
            return
        }
        backend.applyMetadata(remoteWindow: window)
    }

    func close(_ id: RemoteWindowID) async {
        pendingWindowIDs.remove(id)
        var identifier = backend.sessionIdentifier(forRemoteWindow: id)
        if identifier == nil {
            identifier = await registry.session(for: id)
        }
        if let identifier {
            Log.scene.notice("Destroying scene for closed window=\(id.value, privacy: .public)")
            backend.destroySession(identifier: identifier)
        } else {
            Log.scene.debug("Closed window=\(id.value, privacy: .public) had no scene")
        }
        await registry.unbind(window: id)
    }

    // MARK: - Lookup

    /// Session identifier currently representing `id`, if any.
    ///
    /// Prefers the in-memory binding (which survives disconnection) and falls
    /// back to asking the backend (for scenes requested before connect).
    func sessionIdentifier(for id: RemoteWindowID) async -> String? {
        if let bound = await registry.session(for: id) {
            return bound
        }
        return backend.sessionIdentifier(forRemoteWindow: id)
    }

    // MARK: - UIKit callbacks (translated by the backend owner)

    /// Called once a scene has connected and its session identity is known.
    func sceneDidConnect(identifier: String, remoteWindowID: RemoteWindowID) async {        pendingWindowIDs.remove(remoteWindowID)
        await registry.bind(window: remoteWindowID, session: identifier)
        Log.scene.notice("Scene connected window=\(remoteWindowID.value, privacy: .public) session=\(identifier, privacy: .public)")
        if let window = await store.window(remoteWindowID) {
            backend.applyMetadata(remoteWindow: window)
        }
    }

    /// Called when UIKit disconnects a scene (background, termination, ...).
    ///
    /// The binding is intentionally kept: the session identifier remains valid
    /// for restoration, and the remote window must survive.
    func sceneDidDisconnect(identifier: String) async {
        backend.sceneDidDisconnect(identifier: identifier)
        let windowID = await registry.window(for: identifier)
        Log.scene.notice("Scene disconnected session=\(identifier, privacy: .public) window=\(windowID?.value ?? "unknown", privacy: .public) — remote window kept")
    }

    /// Called when the system discards scene sessions for good.
    func sceneSessionsDiscarded(identifiers: [String]) async {
        for identifier in identifiers {
            await registry.unbind(session: identifier)
        }
        Log.scene.notice("Discarded \(identifiers.count, privacy: .public) scene session(s)")
    }

    var pendingRequests: Set<RemoteWindowID> { pendingWindowIDs }
}
