import Foundation

/// Transport abstraction for the remote window server.
///
/// Phase 1 ships `MockRemoteWindowService`. Later phases replace it with a
/// `QUICRemoteWindowService` without touching the rest of the app: everything
/// downstream only knows about this protocol and `RemoteWindowEvent`.
protocol RemoteWindowService: Sendable {
    /// Stream of window events (`window.created` / `.updated` / `.closed`).
    ///
    /// Designed for `for await event in await service.events()`.
    func events() async -> AsyncStream<RemoteWindowEvent>

    /// Requests that a new window of `application` be opened on the remote side.
    func openApplication(_ application: RemoteApplication) async

    /// Requests that a remote window be closed.
    func closeWindow(_ id: RemoteWindowID) async

    /// Reports an iPad-side viewport change (future `xdg_toplevel.configure`).
    func viewportDidChange(_ id: RemoteWindowID, viewport: RemoteViewport) async

    /// Reports an input event for a remote window.
    func inputDidOccur(_ id: RemoteWindowID, event: RemoteInputEvent) async
}
