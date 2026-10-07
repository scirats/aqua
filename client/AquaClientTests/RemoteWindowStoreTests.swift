import XCTest
@testable import AquaClient

final class RemoteWindowStoreTests: XCTestCase {
    private func makeWindow(_ id: String, app: String = "firefox", title: String = "Firefox") -> RemoteWindow {
        RemoteWindow(
            id: RemoteWindowID(id),
            applicationID: RemoteApplicationID(app),
            applicationName: app,
            title: title
        )
    }

    func testRegisterAndRemove() async {
        let store = RemoteWindowStore()
        let window = makeWindow("firefox-1")

        let inserted = await store.register(window)
        XCTAssertTrue(inserted)
        let countAfterInsert = await store.count
        XCTAssertEqual(countAfterInsert, 1)

        let removed = await store.remove(window.id)
        XCTAssertEqual(removed, window)
        let countAfterRemove = await store.count
        XCTAssertEqual(countAfterRemove, 0)
    }

    func testDuplicateIDIsRejected() async {
        let store = RemoteWindowStore()
        let window = makeWindow("firefox-1")

        let first = await store.register(window)
        let second = await store.register(window)
        XCTAssertTrue(first)
        XCTAssertFalse(second)
        let count = await store.count
        XCTAssertEqual(count, 1)
    }

    func testMultipleWindowsForSameApplication() async {
        let store = RemoteWindowStore()
        await store.register(makeWindow("firefox-1"))
        await store.register(makeWindow("firefox-2"))
        await store.register(makeWindow("firefox-3"))
        await store.register(makeWindow("vscode-1", app: "vscode"))

        let firefox = await store.windows(forApplication: RemoteApplicationID("firefox"))
        XCTAssertEqual(firefox.map(\.id.value), ["firefox-1", "firefox-2", "firefox-3"])

        let all = await store.allWindows()
        XCTAssertEqual(all.count, 4)
    }

    func testApplyEventsUpdateStore() async {
        let store = RemoteWindowStore()
        var window = makeWindow("firefox-1")

        let created = await store.apply(.created(window))
        XCTAssertEqual(created, .created(window))

        window.title = "Mozilla Firefox — New Tab"
        await store.apply(.updated(window))
        let updatedTitle = await store.window(window.id)?.title
        XCTAssertEqual(updatedTitle, "Mozilla Firefox — New Tab")

        await store.apply(.closed(window.id))
        let count = await store.count
        XCTAssertEqual(count, 0)
    }

    func testViewportTracking() async {
        let store = RemoteWindowStore()
        let window = makeWindow("terminal-1")
        await store.register(window)
        let viewport = RemoteViewport(width: 1024, height: 768, scale: 2)
        await store.setViewport(viewport, for: window.id)
        let stored = await store.viewport(window.id)
        XCTAssertEqual(stored, viewport)

        await store.remove(window.id)
        let afterRemove = await store.viewport(window.id)
        XCTAssertNil(afterRemove)
    }
}
