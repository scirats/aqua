import XCTest
@testable import AquaClient

@MainActor
final class SceneCoordinatorTests: XCTestCase {
    private func makeWindow(_ id: String, app: String = "firefox") -> RemoteWindow {
        RemoteWindow(
            id: RemoteWindowID(id),
            applicationID: RemoteApplicationID(app),
            applicationName: app,
            title: id
        )
    }

    func testCreatedRequestsNewSession() async {
        let (coordinator, _, backend) = makeSystem()
        let window = makeWindow("firefox-1")

        await coordinator.handle(.created(window))

        XCTAssertEqual(backend.requested.map(\.windowID.value), ["firefox-1"])
        XCTAssertTrue(backend.activated.isEmpty)
    }

    func testRequestingExistingWindowActivatesInsteadOfReopening() async {
        let (coordinator, _, backend) = makeSystem()
        let window = makeWindow("firefox-1")

        await coordinator.handle(.created(window))
        await coordinator.sceneDidConnect(
            identifier: "session-firefox-1",
            remoteWindowID: window.id
        )
        await coordinator.handle(.created(window))

        XCTAssertEqual(backend.requested.count, 1)
        XCTAssertEqual(backend.activated, ["session-firefox-1"])
    }

    func testSceneDisconnectDoesNotCloseRemoteWindow() async {
        let (coordinator, store, backend) = makeSystem()
        let window = makeWindow("terminal-1")

        await store.apply(.created(window))
        await coordinator.handle(.created(window))
        await coordinator.sceneDidConnect(
            identifier: "session-terminal-1",
            remoteWindowID: window.id
        )
        await coordinator.sceneDidDisconnect(identifier: "session-terminal-1")

        XCTAssertTrue(backend.destroyed.isEmpty, "Disconnect must not destroy the scene session")
        let stillPresent = await store.contains(window.id)
        XCTAssertTrue(stillPresent, "Remote window must survive a UI scene disconnect")

        let binding = await coordinator.sessionIdentifier(for: window.id)
        XCTAssertEqual(binding, "session-terminal-1", "The scene binding must survive a disconnect for restoration")
    }

    func testClosedEventDestroysSceneAndStoreEntry() async {
        let (coordinator, store, backend) = makeSystem()
        let window = makeWindow("vscode-1")

        await store.apply(.created(window))
        await coordinator.handle(.created(window))
        await coordinator.sceneDidConnect(
            identifier: "session-vscode-1",
            remoteWindowID: window.id
        )

        await store.apply(.closed(window.id))
        await coordinator.handle(.closed(window.id))

        XCTAssertEqual(backend.destroyed, ["session-vscode-1"])
        let stillPresent = await store.contains(window.id)
        XCTAssertFalse(stillPresent, "A remote window closed event removes the store entry")
    }

    func testUpdatePushesMetadata() async {
        let (coordinator, _, backend) = makeSystem()
        var window = makeWindow("firefox-1")
        window.title = "Firefox — New Tab"

        await coordinator.handle(.updated(window))

        XCTAssertEqual(backend.metadata.last?.title, "Firefox — New Tab")
    }

    // MARK: - Helpers

    private func makeSystem() -> (SceneCoordinator, RemoteWindowStore, FakeSceneSessionBackend) {
        let store = RemoteWindowStore()
        let registry = SceneSessionRegistry()
        let backend = FakeSceneSessionBackend()
        let coordinator = SceneCoordinator(store: store, registry: registry, backend: backend)
        return (coordinator, store, backend)
    }
}

@MainActor
private final class FakeSceneSessionBackend: SceneSessionBackend {
    var sessions: [RemoteWindowID: String] = [:]
    var activated: [String] = []
    var requested: [RemoteWindowIdentity] = []
    var destroyed: [String] = []
    var metadata: [RemoteWindow] = []
    var disconnected: [String] = []

    func sessionIdentifier(forRemoteWindow id: RemoteWindowID) -> String? {
        sessions[id]
    }

    func activateSession(identifier: String) {
        activated.append(identifier)
    }

    func requestNewSession(identity: RemoteWindowIdentity) {
        requested.append(identity)
        sessions[identity.windowID] = "session-\(identity.windowID.value)"
    }

    func destroySession(identifier: String) {
        destroyed.append(identifier)
    }

    func applyMetadata(remoteWindow: RemoteWindow) {
        metadata.append(remoteWindow)
    }

    func sceneDidDisconnect(identifier: String) {
        disconnected.append(identifier)
    }
}
