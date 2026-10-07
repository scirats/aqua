import XCTest
@testable import AquaClient

final class AquaSessionTests: XCTestCase {
    private func info(_ id: String, app: String = "org.gnome.Terminal", title: String = "Terminal", revisionState: UInt32 = 0) -> WindowInfoMessage {
        var info = WindowInfoMessage()
        info.windowID = id
        info.applicationID = app
        info.title = title
        info.state = revisionState
        return info
    }

    private func hello(session: String, revision: UInt64 = 0, version: UInt32 = AquaProtocol.version, error: String = "") -> ServerHelloMessage {
        var hello = ServerHelloMessage()
        hello.protocolVersion = version
        hello.serverSessionID = session
        hello.revision = revision
        hello.error = error
        return hello
    }

    private func snapshot(session: String, revision: UInt64, windows: [WindowInfoMessage]) -> ServerSnapshotMessage {
        var snapshot = ServerSnapshotMessage()
        snapshot.revision = revision
        snapshot.serverSessionID = session
        snapshot.windows = windows
        return snapshot
    }

    func testVersionMismatchIsReported() {
        var session = AquaSession()
        let outcome = session.handleHello(hello(session: "s1", version: 999))
        XCTAssertEqual(outcome.error, "unsupported_protocol_version")
    }

    func testSnapshotCreatesWindows() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1"))
        let events = session.handleSnapshot(snapshot(session: "s1", revision: 10, windows: [info("window-1"), info("window-2")]))
        XCTAssertEqual(events.count, 2)
        if case .created(let window) = events[0] {
            XCTAssertEqual(window.id, RemoteWindowID("window-1"))
        } else {
            XCTFail("expected created")
        }
        XCTAssertEqual(session.windows.count, 2)
    }

    func testMultipleWindowsSameApplication() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1"))
        _ = session.handleSnapshot(snapshot(session: "s1", revision: 1, windows: [
            info("window-1", app: "org.mozilla.firefox"),
            info("window-2", app: "org.mozilla.firefox"),
        ]))
        XCTAssertEqual(session.windows["window-1"]?.applicationID, session.windows["window-2"]?.applicationID)
        XCTAssertNotEqual(session.windows["window-1"]?.id, session.windows["window-2"]?.id)
    }

    func testSnapshotClosesOrphans() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1"))
        _ = session.handleSnapshot(snapshot(session: "s1", revision: 1, windows: [info("window-1"), info("window-2")]))
        // Reconnect with only window-1 present.
        let events = session.handleSnapshot(snapshot(session: "s1", revision: 5, windows: [info("window-1")]))
        XCTAssertEqual(events.count, 1)
        XCTAssertEqual(events[0], .closed(RemoteWindowID("window-2")))
        XCTAssertEqual(session.windows.count, 1)
    }

    func testRevisionIsMonotonicAndDeduplicates() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1"))
        _ = session.handleSnapshot(snapshot(session: "s1", revision: 10, windows: [info("window-1")]))

        var created = WindowCreatedMessage()
        created.revision = 11
        created.window = info("window-2")
        XCTAssertEqual(session.handle(.windowCreated(created)).count, 1)
        // Replaying revision 11 must be ignored.
        XCTAssertEqual(session.handle(.windowCreated(created)).count, 0)
        // Older revision ignored.
        created.revision = 5
        XCTAssertEqual(session.handle(.windowCreated(created)).count, 0)
        XCTAssertEqual(session.lastRevision, 11)
    }

    func testReconnectDoesNotDuplicateWindows() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1", revision: 3))
        _ = session.handleSnapshot(snapshot(session: "s1", revision: 3, windows: [info("window-1"), info("window-2")]))

        // Reconnect: same server session, same windows.
        _ = session.handleHello(hello(session: "s1", revision: 3))
        let events = session.handleSnapshot(snapshot(session: "s1", revision: 3, windows: [info("window-1"), info("window-2")]))
        XCTAssertTrue(events.isEmpty, "identical snapshot must emit no events, got \(events)")
        XCTAssertEqual(session.windows.count, 2)
    }

    func testServerRestartClosesOldWindows() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1"))
        _ = session.handleSnapshot(snapshot(session: "s1", revision: 1, windows: [info("window-1"), info("window-2")]))

        // New server session (restart): old windows must close.
        let outcome = session.handleHello(hello(session: "s2", revision: 0))
        XCTAssertTrue(outcome.sessionChanged)
        let closed = outcome.events.filter { if case .closed = $0 { return true } else { return false } }
        XCTAssertEqual(closed.count, 2)

        let events = session.handleSnapshot(snapshot(session: "s2", revision: 1, windows: [info("window-1")]))
        XCTAssertEqual(events, [.created(AquaSession.domainWindow(from: info("window-1")))])
    }

    func testTitleAndStateUpdates() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1"))
        _ = session.handleSnapshot(snapshot(session: "s1", revision: 1, windows: [info("window-1")]))

        var title = WindowTitleChangedMessage()
        title.revision = 2
        title.windowID = "window-1"
        title.title = "user@linux: ~"
        let events = session.handle(.windowTitleChanged(title))
        XCTAssertEqual(events.count, 1)
        XCTAssertEqual(session.windows["window-1"]?.title, "user@linux: ~")

        var state = WindowStateChangedMessage()
        state.revision = 3
        state.windowID = "window-1"
        state.state = 2
        _ = session.handle(.windowStateChanged(state))
        XCTAssertEqual(session.windows["window-1"]?.state, .fullscreen)
    }

    // MARK: - Surfaces

    private func surfaceInfo(_ id: String, window: String = "window-1", parent: String = "", role: UInt32 = AquaProtocol.SurfaceRole.toplevel, x: Int32 = 0, y: Int32 = 0, width: UInt32 = 10, height: UInt32 = 10, z: UInt32 = 0) -> SurfaceInfoMessage {
        var info = SurfaceInfoMessage()
        info.surfaceID = id
        info.windowID = window
        info.parentSurfaceID = parent
        info.role = role
        info.x = x
        info.y = y
        info.width = width
        info.height = height
        info.z = z
        return info
    }

    func testSnapshotReconcilesSurfaces() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1"))
        var snap = snapshot(session: "s1", revision: 1, windows: [info("window-1")])
        snap.surfaces = [
            surfaceInfo("surface-1"),
            surfaceInfo("surface-2", parent: "surface-1", role: AquaProtocol.SurfaceRole.subsurface, x: 2, y: 3, z: 1),
        ]
        _ = session.handleSnapshot(snap)
        XCTAssertEqual(session.surfaces.count, 2)
        XCTAssertEqual(session.surfaces["surface-2"]?.parentSurfaceID, "surface-1")
        XCTAssertEqual(session.surfaces["surface-2"]?.position, CGPoint(x: 2, y: 3))
        XCTAssertEqual(session.surfaces(for: "window-1").first?.surfaceID, "surface-1")
    }

    func testSurfaceEventsAreRevisionGated() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1"))
        _ = session.handleSnapshot(snapshot(session: "s1", revision: 10, windows: [info("window-1")]))

        var created = SurfaceCreatedMessage()
        created.revision = 11
        created.surface = surfaceInfo("surface-9")
        _ = session.handle(.surfaceCreated(created))
        XCTAssertNotNil(session.surfaces["surface-9"])

        var destroyed = SurfaceDestroyedMessage()
        destroyed.revision = 12
        destroyed.surfaceID = "surface-9"
        _ = session.handle(.surfaceDestroyed(destroyed))
        XCTAssertNil(session.surfaces["surface-9"])

        // Replaying an old revision is ignored.
        _ = session.handle(.surfaceCreated(created))
        XCTAssertNil(session.surfaces["surface-9"])
    }

    func testSurfacesClearedOnServerRestart() {
        var session = AquaSession()
        _ = session.handleHello(hello(session: "s1"))
        var snap = snapshot(session: "s1", revision: 1, windows: [info("window-1")])
        snap.surfaces = [surfaceInfo("surface-1")]
        _ = session.handleSnapshot(snap)
        XCTAssertEqual(session.surfaces.count, 1)

        _ = session.handleHello(hello(session: "s2"))
        XCTAssertTrue(session.surfaces.isEmpty)
    }
}
