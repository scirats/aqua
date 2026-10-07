import UIKit

/// Builds the `NSUserActivity` used to activate / restore a remote window scene.
///
/// The activity's `targetContentIdentifier` is our stable `remote://` identity.
/// iPadOS matches it against `UISceneActivationConditions` and uses it to route
/// a request to the correct scene session.
enum RemoteWindowActivity {
    static func make(for identity: RemoteWindowIdentity) -> NSUserActivity {
        let activity = NSUserActivity(activityType: SceneSessionBinding.activityType)
        activity.title = "Remote Window"
        activity.targetContentIdentifier = identity.urlString
        activity.userInfo = SceneSessionBinding.userInfo(for: identity) as [AnyHashable: Any]
        activity.isEligibleForHandoff = false
        activity.isEligibleForSearch = false
        return activity
    }

    static func make(for windowID: RemoteWindowID) -> NSUserActivity {
        make(for: RemoteWindowIdentity(windowID: windowID))
    }

    static func identity(from activity: NSUserActivity) -> RemoteWindowIdentity? {
        if let target = activity.targetContentIdentifier, let identity = RemoteWindowIdentity(urlString: target) {
            return identity
        }
        if let raw = activity.userInfo as? [String: Any] {
            return SceneSessionBinding.identity(fromUserInfo: raw)
        }
        return nil
    }
}
