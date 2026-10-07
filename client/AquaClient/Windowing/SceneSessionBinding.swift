import Foundation

/// Encodes and decodes a `RemoteWindowID` into scene session state that iPadOS
/// persists for us (`UISceneSession.userInfo` is restricted to plist types).
///
/// This is the mechanism that lets us rebuild:
///
///     UISceneSession -> RemoteWindowID -> RemoteWindow
///
/// after the app is backgrounded, terminated and restored.
enum SceneSessionBinding {
    /// Key stored in `UISceneSession.userInfo`.
    static let remoteWindowIDKey = "com.scirats.aqua.remoteWindowID"
    /// Key stored in `UISceneSession.userInfo`.
    static let serverKey = "com.scirats.aqua.server"
    /// `NSUserActivity.activityType` used when activating a remote window scene.
    static let activityType = "com.scirats.aqua.remoteWindow"

    static let defaultServer = "local"

    static func userInfo(for identity: RemoteWindowIdentity) -> [String: Any] {
        [
            remoteWindowIDKey: identity.windowID.value,
            serverKey: identity.server,
        ]
    }

    static func userInfo(for windowID: RemoteWindowID) -> [String: Any] {
        userInfo(for: RemoteWindowIdentity(windowID: windowID))
    }

    static func remoteWindowID(fromUserInfo userInfo: [String: Any]?) -> RemoteWindowID? {
        guard let value = userInfo?[remoteWindowIDKey] as? String else { return nil }
        return RemoteWindowID(value)
    }

    static func identity(fromUserInfo userInfo: [String: Any]?) -> RemoteWindowIdentity? {
        guard let windowID = remoteWindowID(fromUserInfo: userInfo) else { return nil }
        let server = (userInfo?[serverKey] as? String) ?? defaultServer
        return RemoteWindowIdentity(server: server, windowID: windowID)
    }

    static func remoteWindowID(from identity: RemoteWindowIdentity) -> RemoteWindowID {
        identity.windowID
    }
}
