import UIKit

/// Application entry point.
///
/// Adopts the scene lifecycle (required on iOS 27). Static scene configuration
/// comes from the generated `UIApplicationSceneManifest`; the concrete
/// `UISceneConfiguration` is chosen dynamically per connecting session so that
/// the initial/control-panel scene differs from remote window scenes.
@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        AppEnvironment.shared.start()
        return true
    }

    func application(
        _ application: UIApplication,
        configurationForConnecting connectingSceneSession: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let identity = resolveIdentity(in: connectingSceneSession, options: options)
        let configuration = UISceneConfiguration(
            name: identity == nil ? "ControlPanel" : "RemoteWindow",
            sessionRole: connectingSceneSession.role
        )
        configuration.delegateClass = SceneDelegate.self
        return configuration
    }

    func application(
        _ application: UIApplication,
        didDiscardSceneSessions sceneSessions: Set<UISceneSession>
    ) {
        let identifiers = sceneSessions.map(\.persistentIdentifier)
        Task { @MainActor in
            await AppEnvironment.shared.coordinator.sceneSessionsDiscarded(identifiers: identifiers)
        }
    }

    // MARK: - Identity resolution

    /// Determines the remote window represented by a connecting session and
    /// stamps it onto `session.userInfo` so it survives restoration.
    private func resolveIdentity(
        in session: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> RemoteWindowIdentity? {
        if let activity = options.userActivities.first(where: { $0.activityType == SceneSessionBinding.activityType }),
           let identity = RemoteWindowActivity.identity(from: activity) {
            session.userInfo = SceneSessionBinding.userInfo(for: identity)
            return identity
        }
        if let identity = SceneSessionBinding.identity(fromUserInfo: session.userInfo) {
            return identity
        }
        if let activity = session.stateRestorationActivity,
           let identity = RemoteWindowActivity.identity(from: activity) {
            session.userInfo = SceneSessionBinding.userInfo(for: identity)
            return identity
        }
        return nil
    }
}
