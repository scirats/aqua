import UIKit

/// One `SceneDelegate` instance per `UIWindowScene`.
///
/// A scene either represents the Control Panel (no remote identity) or exactly
/// one `RemoteWindow`.
final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: windowScene)
        self.window = window

        if let identity = SceneSessionBinding.identity(fromUserInfo: session.userInfo) {
            configureRemoteWindowScene(windowScene, session: session, identity: identity)
        } else {
            windowScene.title = "Aqua"
            window.rootViewController = UINavigationController(rootViewController: ControlPanelViewController())
        }

        window.makeKeyAndVisible()
        Log.scene.notice("scene connected session=\(session.persistentIdentifier, privacy: .public)")
    }

    func sceneDidDisconnect(_ scene: UIScene) {
        let identifier = scene.session.persistentIdentifier
        Log.scene.notice("scene disconnected session=\(identifier, privacy: .public) — remote window kept")
        Task { @MainActor in
            await AppEnvironment.shared.coordinator.sceneDidDisconnect(identifier: identifier)
        }
    }

    func sceneDidBecomeActive(_ scene: UIScene) {
        Log.scene.debug("scene became active session=\(scene.session.persistentIdentifier, privacy: .public)")
    }

    func sceneWillResignActive(_ scene: UIScene) {
        Log.scene.debug("scene resigned active session=\(scene.session.persistentIdentifier, privacy: .public)")
    }

    func sceneDidEnterBackground(_ scene: UIScene) {
        Log.scene.debug("scene entered background session=\(scene.session.persistentIdentifier, privacy: .public)")
    }

    func sceneWillEnterForeground(_ scene: UIScene) {
        Log.scene.debug("scene entered foreground session=\(scene.session.persistentIdentifier, privacy: .public)")
    }

    /// Modern (iOS 26) geometry change callback. Replaces the deprecated
    /// `windowScene(_:didUpdateCoordinateSpace:...)`.
    func windowScene(
        _ windowScene: UIWindowScene,
        didUpdateEffectiveGeometry previousEffectiveGeometry: UIWindowScene.Geometry
    ) {
        let bounds = windowScene.effectiveGeometry.coordinateSpace.bounds
        let scale = Double(windowScene.traitCollection.displayScale)
        let viewport = RemoteViewport(width: Double(bounds.width), height: Double(bounds.height), scale: scale)

        let windowID = SceneSessionBinding.remoteWindowID(fromUserInfo: windowScene.session.userInfo)
        let label = windowID?.value ?? "control-panel"
        Log.viewport.notice("window=\(label, privacy: .public) viewportChanged \(viewport.logDescription, privacy: .public)")

        guard let windowID else { return }
        Task { @MainActor in
            await AppEnvironment.shared.viewportChanged(windowID, viewport: viewport)
        }
    }

    func stateRestorationActivity(for scene: UIScene) -> NSUserActivity? {
        guard let identity = SceneSessionBinding.identity(fromUserInfo: scene.session.userInfo) else {
            return nil
        }
        return RemoteWindowActivity.make(for: identity)
    }

    // MARK: - Private

    private func configureRemoteWindowScene(
        _ windowScene: UIWindowScene,
        session: UISceneSession,
        identity: RemoteWindowIdentity
    ) {
        let remoteWindowID = identity.windowID
        window?.rootViewController = RemoteWindowViewController(remoteWindowID: remoteWindowID)

        // Route matching banner/URL activations to this scene.
        windowScene.activationConditions.canActivateForTargetContentIdentifierPredicate =
            NSPredicate(format: "self BEGINSWITH %@", "\(RemoteWindowIdentity.scheme)://")
        windowScene.activationConditions.prefersToActivateForTargetContentIdentifierPredicate =
            NSPredicate(format: "self == %@", identity.urlString)

        let identifier = session.persistentIdentifier
        Task { @MainActor in
            await AppEnvironment.shared.coordinator.sceneDidConnect(
                identifier: identifier,
                remoteWindowID: remoteWindowID
            )
        }
    }
}
