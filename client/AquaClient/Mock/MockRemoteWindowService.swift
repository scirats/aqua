import Foundation

/// Local, in-memory stand-in for the future Linux / QUIC server.
///
/// It implements `RemoteWindowService` and is the *only* thing in phase 1 that
/// fabricates remote windows. Everything downstream (store, coordinator,
/// scenes) treats it exactly like a real transport would.
///
///     MockRemoteWindowService -> RemoteWindow created -> SceneCoordinator
///         -> activateSceneSessionForRequest -> real UIWindowScene
actor MockRemoteWindowService: RemoteWindowService {
    private let applications: [RemoteApplication]
    private let automaticTitleUpdates: Bool

    private var windows: [RemoteWindowID: RemoteWindow] = [:]
    private var openCounters: [RemoteApplicationID: Int] = [:]
    private var continuations: [UUID: AsyncStream<RemoteWindowEvent>.Continuation] = [:]

    init(
        applications: [RemoteApplication] = RemoteApplication.MockCatalog.all,
        automaticTitleUpdates: Bool = true
    ) {
        self.applications = applications
        self.automaticTitleUpdates = automaticTitleUpdates
    }

    // MARK: - RemoteWindowService

    func events() async -> AsyncStream<RemoteWindowEvent> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: RemoteWindowEvent.self,
            bufferingPolicy: .unbounded
        )
        let token = UUID()
        continuations[token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeContinuation(token) }
        }
        return stream
    }

    func openApplication(_ application: RemoteApplication) async {
        let index = (openCounters[application.id] ?? 0) + 1
        openCounters[application.id] = index

        let windowID = RemoteWindowID("\(application.id.value)-\(index)")
        var window = RemoteWindow(
            id: windowID,
            applicationID: application.id,
            applicationName: application.name,
            title: application.defaultWindowTitle ?? application.name,
            minimumSize: Self.minimumSize(for: application),
            maximumSize: Self.maximumSize(for: application),
            state: .normal
        )
        window.state = .normal
        windows[windowID] = window
        Log.service.notice("mock emitted window.created id=\(windowID.value, privacy: .public) app=\(application.id.value, privacy: .public)")
        emit(.created(window))

        if automaticTitleUpdates {
            scheduleAutomaticUpdate(for: windowID)
        }
    }

    func closeWindow(_ id: RemoteWindowID) async {
        guard windows.removeValue(forKey: id) != nil else {
            Log.service.debug("mock close ignored, unknown id=\(id.value, privacy: .public)")
            return
        }
        Log.service.notice("mock emitted window.closed id=\(id.value, privacy: .public)")
        emit(.closed(id))
    }

    func viewportDidChange(_ id: RemoteWindowID, viewport: RemoteViewport) async {
        // Phase 1: register only.
        Log.viewport.notice("window=\(id.value, privacy: .public) viewportChanged \(viewport.logDescription, privacy: .public)")
    }

    func inputDidOccur(_ id: RemoteWindowID, event: RemoteInputEvent) async {
        // Phase 1: register only.
        Log.input.debug("window=\(id.value, privacy: .public) input \(event.logDescription, privacy: .public)")
    }

    // MARK: - Mock-only helpers

    func availableApplications() -> [RemoteApplication] {
        applications
    }

    func snapshot() -> [RemoteWindow] {
        windows.values.sorted { $0.id.value < $1.id.value }
    }

    func closeAll() async {
        let ids = windows.keys.sorted { $0.value < $1.value }
        for id in ids {
            await closeWindow(id)
        }
    }

    // MARK: - Private

    private func removeContinuation(_ token: UUID) {
        continuations[token] = nil
    }

    private func emit(_ event: RemoteWindowEvent) {
        for continuation in continuations.values {
            continuation.yield(event)
        }
    }

    private func scheduleAutomaticUpdate(for id: RemoteWindowID) {
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(1.2))
            await self?.applyAutomaticUpdate(id)
        }
    }

    private func applyAutomaticUpdate(_ id: RemoteWindowID) {
        guard var window = windows[id], window.state != .closed else { return }
        window.title = Self.updatedTitle(for: window)
        windows[id] = window
        Log.service.debug("mock emitted window.updated id=\(id.value, privacy: .public) title=\(window.title, privacy: .public)")
        emit(.updated(window))
    }

    private static func updatedTitle(for window: RemoteWindow) -> String {
        switch window.applicationID.value {
        case "firefox": return "\(window.applicationName) — New Tab"
        case "vscode": return "\(window.applicationName) — main.swift"
        case "terminal": return "user@linux: ~"
        case "files": return "Home"
        case "gimp": return "Untitled — GIMP"
        default: return window.title
        }
    }

    private static func minimumSize(for application: RemoteApplication) -> CGSize? {
        switch application.id.value {
        case "terminal": return CGSize(width: 500, height: 300)
        case "firefox": return CGSize(width: 400, height: 300)
        default: return nil
        }
    }

    private static func maximumSize(for application: RemoteApplication) -> CGSize? {
        nil
    }
}
