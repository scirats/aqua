import XCTest
@testable import AquaClient

final class MockRemoteWindowServiceTests: XCTestCase {
    func testOpenEmitsCreatedWithUniqueIDsForSameApplication() async {
        let service = MockRemoteWindowService(automaticTitleUpdates: false)
        let stream = await service.events()
        var iterator = stream.makeAsyncIterator()

        await service.openApplication(.MockCatalog.firefox)
        await service.openApplication(.MockCatalog.firefox)

        guard case .created(let first) = await iterator.next() else {
            return XCTFail("Expected first created event")
        }
        guard case .created(let second) = await iterator.next() else {
            return XCTFail("Expected second created event")
        }

        XCTAssertEqual(first.id, RemoteWindowID("firefox-1"))
        XCTAssertEqual(second.id, RemoteWindowID("firefox-2"))
        XCTAssertNotEqual(first.id, second.id)
        XCTAssertEqual(first.applicationID, second.applicationID)
    }

    func testCloseEmitsClosedAndRemovesWindow() async {
        let service = MockRemoteWindowService(automaticTitleUpdates: false)
        let stream = await service.events()
        var iterator = stream.makeAsyncIterator()

        await service.openApplication(.MockCatalog.terminal)
        _ = await iterator.next()

        await service.closeWindow(RemoteWindowID("terminal-1"))
        guard case .closed(let id) = await iterator.next() else {
            return XCTFail("Expected closed event")
        }
        XCTAssertEqual(id, RemoteWindowID("terminal-1"))

        let snapshot = await service.snapshot()
        XCTAssertTrue(snapshot.isEmpty)
    }

    func testTerminalReportsMinimumSize() async {
        let service = MockRemoteWindowService(automaticTitleUpdates: false)
        let stream = await service.events()
        var iterator = stream.makeAsyncIterator()

        await service.openApplication(.MockCatalog.terminal)
        guard case .created(let window) = await iterator.next() else {
            return XCTFail("Expected created event")
        }
        XCTAssertEqual(window.minimumSize, CGSize(width: 500, height: 300))
    }

    func testEventsDriveStoreThroughApply() async {
        let service = MockRemoteWindowService(automaticTitleUpdates: false)
        let store = RemoteWindowStore()
        let stream = await service.events()
        var iterator = stream.makeAsyncIterator()

        await service.openApplication(.MockCatalog.code)
        if let event = await iterator.next() {
            await store.apply(event)
        }

        await service.closeWindow(RemoteWindowID("vscode-1"))
        if let event = await iterator.next() {
            await store.apply(event)
        }

        let count = await store.count
        XCTAssertEqual(count, 0)
    }
}
