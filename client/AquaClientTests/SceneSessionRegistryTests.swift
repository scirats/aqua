import XCTest
@testable import AquaClient

final class SceneSessionRegistryTests: XCTestCase {
    func testBindAndResolveBothDirections() async {
        let registry = SceneSessionRegistry()
        let window = RemoteWindowID("firefox-42")

        await registry.bind(window: window, session: "session-A")

        let session = await registry.session(for: window)
        XCTAssertEqual(session, "session-A")

        let resolvedWindow = await registry.window(for: "session-A")
        XCTAssertEqual(resolvedWindow, window)
    }

    func testRebindingSameWindowDropsStaleSession() async {
        let registry = SceneSessionRegistry()
        let window = RemoteWindowID("firefox-42")

        await registry.bind(window: window, session: "session-A")
        await registry.bind(window: window, session: "session-B")

        let session = await registry.session(for: window)
        XCTAssertEqual(session, "session-B")
        let stale = await registry.window(for: "session-A")
        XCTAssertNil(stale)
        let count = await registry.bindingCount
        XCTAssertEqual(count, 1)
    }

    func testRebindingSameSessionDropsStaleWindow() async {
        let registry = SceneSessionRegistry()
        await registry.bind(window: RemoteWindowID("w1"), session: "session-A")
        await registry.bind(window: RemoteWindowID("w2"), session: "session-A")

        let first = await registry.session(for: RemoteWindowID("w1"))
        XCTAssertNil(first)
        let second = await registry.window(for: "session-A")
        XCTAssertEqual(second, RemoteWindowID("w2"))
        let count = await registry.bindingCount
        XCTAssertEqual(count, 1)
    }

    func testUnbind() async {
        let registry = SceneSessionRegistry()
        let window = RemoteWindowID("terminal-1")
        await registry.bind(window: window, session: "session-C")

        let removedSession = await registry.unbind(window: window)
        XCTAssertEqual(removedSession, "session-C")
        let session = await registry.session(for: window)
        XCTAssertNil(session)

        await registry.bind(window: window, session: "session-C")
        let removedWindow = await registry.unbind(session: "session-C")
        XCTAssertEqual(removedWindow, window)
    }
}
