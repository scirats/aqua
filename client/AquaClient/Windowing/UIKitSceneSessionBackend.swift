import UIKit

/// `SceneSessionBackend` implemented with public UIKit scene APIs.
///
/// Verified against the iOS 27 SDK:
///  - `UIApplication.activateSceneSessionForRequest(_:errorHandler:)` (iOS 17+)
///    is the current replacement for the deprecated
///    `requestSceneSessionActivation(_:userActivity:options:errorHandler:)`.
///  - `UIApplication.requestSceneSessionDestruction(_:options:errorHandler:)`
///    (iOS 13+).
///  - `UISceneSession.userInfo`, `UISceneSession.persistentIdentifier` (iOS 13+).
///  - `UIWindowScene.sizeRestrictions` (iOS 13+, may be `nil` when resizing is
///    unsupported).
@MainActor
final class UIKitSceneSessionBackend: SceneSessionBackend {
    private weak var application: UIApplication?

    init(application: UIApplication) {
        self.application = application
    }

    // MARK: - SceneSessionBackend

    func sessionIdentifier(forRemoteWindow id: RemoteWindowID) -> String? {
        guard let application else { return nil }
        return application.openSessions.first { session in
            SceneSessionBinding.remoteWindowID(fromUserInfo: session.userInfo) == id
        }?.persistentIdentifier
    }

    func activateSession(identifier: String) {
        guard let application, let session = session(withIdentifier: identifier) else {
            Log.scene.error("Cannot activate unknown session=\(identifier, privacy: .public)")
            return
        }
        var request = UISceneSessionActivationRequest(session: session)
        request.options = makeActivationOptions(application: application)
        application.activateSceneSession(for: request, errorHandler: logError)
    }

    func requestNewSession(identity: RemoteWindowIdentity) {
        guard let application else { return }
        var request = UISceneSessionActivationRequest()
        request.userActivity = RemoteWindowActivity.make(for: identity)
        request.options = makeActivationOptions(application: application)
        application.activateSceneSession(for: request, errorHandler: logError)
    }

    func destroySession(identifier: String) {
        guard let application, let session = session(withIdentifier: identifier) else {
            Log.scene.error("Cannot destroy unknown session=\(identifier, privacy: .public)")
            return
        }
        let options = UIWindowSceneDestructionRequestOptions()
        options.windowDismissalAnimation = .standard
        application.requestSceneSessionDestruction(session, options: options, errorHandler: logError)
    }

    func applyMetadata(remoteWindow: RemoteWindow) {
        guard let scene = connectedWindowScene(for: remoteWindow.id) else {
            Log.scene.debug("Metadata deferred, no connected scene for window=\(remoteWindow.id.value, privacy: .public)")
            return
        }
        scene.title = remoteWindow.title
        applySizeRestrictions(to: scene, remoteWindow: remoteWindow)
    }

    func sceneDidDisconnect(identifier: String) {
        // Intentionally no remote-side effect.
    }

    // MARK: - Helpers

    private func session(withIdentifier identifier: String) -> UISceneSession? {
        application?.openSessions.first { $0.persistentIdentifier == identifier }
    }

    private func connectedWindowScene(for id: RemoteWindowID) -> UIWindowScene? {
        guard let application else { return nil }
        return application.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first { SceneSessionBinding.remoteWindowID(fromUserInfo: $0.session.userInfo) == id }
    }

    private func makeActivationOptions(application: UIApplication) -> UIScene.ActivationRequestOptions {
        let options = UIScene.ActivationRequestOptions()
        options.requestingScene = application.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first { $0.activationState == .foregroundActive }
        return options
    }

    private func applySizeRestrictions(to scene: UIWindowScene, remoteWindow: RemoteWindow) {
        guard let restrictions = scene.sizeRestrictions else {
            // Documented limitation: `sizeRestrictions` is `nil` on platforms or
            // configurations that do not support interactive scene resizing.
            Log.scene.debug("sizeRestrictions unavailable for window=\(remoteWindow.id.value, privacy: .public)")
            return
        }
        if let minimum = remoteWindow.minimumSize, minimum.isUsableMinimum {
            restrictions.minimumSize = minimum
        }
        if let maximum = remoteWindow.maximumSize, maximum.isUsableMinimum {
            restrictions.maximumSize = maximum
        }
        Log.scene.debug("Applied size restrictions min=\(String(describing: remoteWindow.minimumSize), privacy: .public) max=\(String(describing: remoteWindow.maximumSize), privacy: .public) window=\(remoteWindow.id.value, privacy: .public)")
    }

    private func logError(_ error: Error) {
        Log.scene.error("Scene session request failed: \(error.localizedDescription, privacy: .public)")
    }
}
