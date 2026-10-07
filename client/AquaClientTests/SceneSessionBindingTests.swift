import XCTest
@testable import AquaClient

final class SceneSessionBindingTests: XCTestCase {
    func testUserInfoRoundTrip() {
        let identity = RemoteWindowIdentity(server: "local", windowID: RemoteWindowID("firefox-42"))
        let userInfo = SceneSessionBinding.userInfo(for: identity)
        let recovered = SceneSessionBinding.identity(fromUserInfo: userInfo)

        XCTAssertEqual(recovered, identity)
        XCTAssertEqual(recovered?.windowID, RemoteWindowID("firefox-42"))
        XCTAssertEqual(recovered?.server, "local")
    }

    func testUserInfoMissingKeyReturnsNil() {
        let recovered = SceneSessionBinding.identity(fromUserInfo: ["something": "else"])
        XCTAssertNil(recovered)
    }

    func testIdentityURLRoundTrip() {
        let identity = RemoteWindowIdentity(windowID: RemoteWindowID("vscode-7"))
        XCTAssertEqual(identity.urlString, "remote://local/window/vscode-7")

        let recovered = RemoteWindowIdentity(urlString: "remote://local/window/vscode-7")
        XCTAssertEqual(recovered, identity)
    }

    func testIdentityRejectsForeignURLs() {
        XCTAssertNil(RemoteWindowIdentity(urlString: "https://example.com/window/x"))
        XCTAssertNil(RemoteWindowIdentity(urlString: "remote://local/not-a-window/x"))
        XCTAssertNil(RemoteWindowIdentity(urlString: "garbage"))
    }

    func testUserActivityCarriesIdentity() {
        let identity = RemoteWindowIdentity(windowID: RemoteWindowID("gimp-3"))
        let activity = RemoteWindowActivity.make(for: identity)

        XCTAssertEqual(activity.targetContentIdentifier, identity.urlString)
        XCTAssertEqual(RemoteWindowActivity.identity(from: activity), identity)
    }

    func testUserActivityFallsBackToUserInfo() {
        let identity = RemoteWindowIdentity(server: "lab", windowID: RemoteWindowID("files-1"))
        let activity = NSUserActivity(activityType: SceneSessionBinding.activityType)
        activity.userInfo = SceneSessionBinding.userInfo(for: identity)

        XCTAssertEqual(RemoteWindowActivity.identity(from: activity), identity)
    }
}
