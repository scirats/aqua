import UIKit

/// Composition root for the iPad client.
///
/// Owns the single instances of the store, transport, coordinator and services.
/// It is `@MainActor` (UIKit lifecycle) but never mutates shared state from
/// arbitrary threads: the cross-actor state lives in `actor`s.
///
/// The transport is swappable: `MockRemoteWindowService` (local demo) or
/// `QUICRemoteWindowService` (Aqua Protocol v1 over QUIC). Everything above this
/// class is unaware of the difference.
@MainActor
final class AppEnvironment {
    static let shared = AppEnvironment()

    let store: RemoteWindowStore
    private(set) var service: any RemoteWindowService
    let registry: SceneSessionRegistry
    let backend: SceneSessionBackend
    let coordinator: SceneCoordinator
    let clipboard: any RemoteClipboardService

    private let mockService = MockRemoteWindowService()
    private var quicService: QUICRemoteWindowService?

    private var eventTask: Task<Void, Never>?
    private var updateContinuations: [UUID: AsyncStream<[RemoteWindow]>.Continuation] = [:]
    private var connectionContinuations: [UUID: AsyncStream<RemoteConnectionState>.Continuation] = [:]
    private var connectionStateTask: Task<Void, Never>?
    private var frameTask: Task<Void, Never>?
    private var surfaceTask: Task<Void, Never>?
    private var videoFrameTask: Task<Void, Never>?
    private var videoConfigTask: Task<Void, Never>?
    private var surfaceModels: [String: WindowSurfaceModel] = [:]
    private var compositionContinuations: [UUID: AsyncStream<SurfaceComposition>.Continuation] = [:]
    private var videoFrameContinuations: [UUID: AsyncStream<EncodedVideoFrame>.Continuation] = [:]
    private var videoConfigContinuations: [UUID: AsyncStream<WindowVideoConfiguration>.Continuation] = [:]
    private var currentConnectionState: RemoteConnectionState = .disconnected
    private var started = false

    private init() {
        let store = RemoteWindowStore()
        let registry = SceneSessionRegistry()
        let backend = UIKitSceneSessionBackend(application: .shared)

        self.store = store
        self.service = mockService
        self.registry = registry
        self.backend = backend
        self.coordinator = SceneCoordinator(store: store, registry: registry, backend: backend)
        self.clipboard = MockRemoteClipboardService()
    }

    /// Starts consuming the transport event stream.
    ///
    ///     for await event in service.events() { ... }
    ///
    /// This is the single place where remote events mutate the store and drive
    /// the scene coordinator.
    func start() {
        guard !started else { return }
        started = true
        Log.app.notice("Starting remote window system")

        startEventLoop(for: service)

        autoConnectIfRequested()

        if ProcessInfo.processInfo.arguments.contains(Self.autoOpenDemoArgument),
           ProcessInfo.processInfo.arguments.contains("-AquaUseMock") {
            scheduleDemoWindowOpens()
        }
    }

    /// `-AquaAutoConnect <host> <port> [fingerprint]` connects on launch. Used
    /// for headless end-to-end verification against a real server.
    private func autoConnectIfRequested() {
        let args = ProcessInfo.processInfo.arguments
        guard let index = args.firstIndex(of: "-AquaAutoConnect"), index + 2 < args.count else {
            return
        }
        let host = args[index + 1]
        let port = UInt16(args[index + 2]) ?? 52420
        let fingerprint = index + 3 < args.count && !args[index + 3].hasPrefix("-")
            ? args[index + 3]
            : nil
        connectQUIC(host: host, port: port, fingerprint: fingerprint)
    }

    /// Launch argument used to exercise the multi-scene flow without tapping
    /// the control panel (handy for automated verification, mock transport only).
    static let autoOpenDemoArgument = "-AquaAutoOpenDemo"
    /// Optional companion to `autoOpenDemoArgument`: closes every remote window
    /// a few seconds after opening, to exercise scene destruction.
    static let autoCloseDemoArgument = "-AquaAutoCloseDemo"

    private func scheduleDemoWindowOpens() {
        Task { @MainActor [weak self] in
            guard let self else { return }
            try? await Task.sleep(for: .seconds(0.6))
            await self.open(.MockCatalog.firefox)
            await self.open(.MockCatalog.firefox)
            await self.open(.MockCatalog.code)
            await self.open(.MockCatalog.terminal)

            guard ProcessInfo.processInfo.arguments.contains(Self.autoCloseDemoArgument) else { return }
            try? await Task.sleep(for: .seconds(6))
            await self.closeAllWindows()
        }
    }

    // MARK: - Transport selection

    /// Connect to a real Aqua server over QUIC and switch the event source.
    func connectQUIC(host: String, port: UInt16, fingerprint: String?) {
        let quic: QUICRemoteWindowService
        if let existing = quicService {
            quic = existing
        } else {
            let created = QUICRemoteWindowService()
            quicService = created
            observeConnectionStates(created)
            quic = created
        }
        service = quic
        startEventLoop(for: quic)
        observeSurfaceData(quic)
        Task { [weak self] in
            await quic.connect(host: host, port: port, expectedFingerprint: fingerprint)
            _ = self
        }
    }

    /// Disconnect and return to the local mock transport.
    func disconnectQUIC() {
        if let quic = quicService {
            Task { await quic.disconnect() }
        }
        service = mockService
        startEventLoop(for: mockService)
    }

    var isUsingQUIC: Bool { quicService != nil && service is QUICRemoteWindowService }

    // MARK: - Observation

    /// Async stream of window snapshots for the UI. Buffers only the newest.
    func windowUpdates() -> AsyncStream<[RemoteWindow]> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: [RemoteWindow].self,
            bufferingPolicy: .bufferingNewest(1)
        )
        let token = UUID()
        updateContinuations[token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { @MainActor in
                self?.updateContinuations[token] = nil
            }
        }
        Task { @MainActor in
            continuation.yield(await self.store.allWindows())
        }
        return stream
    }

    /// Composed surface images per window (phase 3B). The renderer consumes this.
    func surfaceCompositions() -> AsyncStream<SurfaceComposition> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: SurfaceComposition.self,
            bufferingPolicy: .bufferingNewest(1)
        )
        let token = UUID()
        compositionContinuations[token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { @MainActor in
                self?.compositionContinuations[token] = nil
            }
        }
        return stream
    }

    /// Encoded video frames for all windows (phase 3C). The video view filters
    /// by `windowID`, exactly like `surfaceCompositions()`.
    func videoFrames() -> AsyncStream<EncodedVideoFrame> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: EncodedVideoFrame.self,
            bufferingPolicy: .bufferingNewest(8)
        )
        let token = UUID()
        videoFrameContinuations[token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { @MainActor in
                self?.videoFrameContinuations[token] = nil
            }
        }
        return stream
    }

    /// Video configurations for all windows.
    func videoConfigurations() -> AsyncStream<WindowVideoConfiguration> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: WindowVideoConfiguration.self,
            bufferingPolicy: .bufferingNewest(4)
        )
        let token = UUID()
        videoConfigContinuations[token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { @MainActor in
                self?.videoConfigContinuations[token] = nil
            }
        }
        return stream
    }

    /// Ask the server encoder for a keyframe (video backpressure contract).
    func requestKeyframe(windowID: String, reason: String) {
        guard let quic = quicService else { return }
        Task { await quic.requestKeyframe(windowID: windowID, reason: reason) }
    }

    /// Connection lifecycle for the configuration UI.
    func connectionUpdates() -> AsyncStream<RemoteConnectionState> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: RemoteConnectionState.self,
            bufferingPolicy: .bufferingNewest(1)
        )
        let token = UUID()
        connectionContinuations[token] = continuation
        continuation.yield(currentConnectionState)
        continuation.onTermination = { [weak self] _ in
            Task { @MainActor in
                self?.connectionContinuations[token] = nil
            }
        }
        return stream
    }

    // MARK: - Operations

    func open(_ application: RemoteApplication) async {
        await service.openApplication(application)
    }

    func close(_ id: RemoteWindowID) async {
        await service.closeWindow(id)
    }

    func closeAllWindows() async {
        // Only the mock owns its windows; the real server does.
        if let mock = service as? MockRemoteWindowService {
            await mock.closeAll()
        }
    }

    func activate(_ window: RemoteWindow) async {
        await coordinator.open(window)
    }

    func viewportChanged(_ id: RemoteWindowID, viewport: RemoteViewport) async {
        await store.setViewport(viewport, for: id)
        await service.viewportDidChange(id, viewport: viewport)
    }

    func window(_ id: RemoteWindowID) async -> RemoteWindow? {
        await store.window(id)
    }

    /// Report a presented surface frame to the server (phase 3B feedback loop).
    func presentedFrame(surfaceID: String, frameID: UInt64) {
        guard let quic = quicService else { return }
        Task { await quic.presentedFrame(surfaceID: surfaceID, frameID: frameID) }
    }

    func allWindows() async -> [RemoteWindow] {
        await store.allWindows()
    }

    // MARK: - Private

    private func startEventLoop(for service: any RemoteWindowService) {
        eventTask?.cancel()
        eventTask = Task { [weak self] in
            guard let self else { return }
            let stream = await service.events()
            for await event in stream {
                await self.process(event)
            }
        }
    }

    private func observeSurfaceData(_ quic: QUICRemoteWindowService) {
        frameTask?.cancel()
        surfaceTask?.cancel()
        videoFrameTask?.cancel()
        videoConfigTask?.cancel()
        frameTask = Task { [weak self] in
            let stream = await quic.frames()
            for await frame in stream {
                self?.apply(frame: frame)
            }
        }
        surfaceTask = Task { [weak self] in
            let stream = await quic.surfaceUpdates()
            for await surfaces in stream {
                self?.apply(surfaces: surfaces)
            }
        }
        // Phase 3C: fan out encoded video frames/configs. Empty until the server
        // implements the video data plane; harmless otherwise.
        videoFrameTask = Task { [weak self] in
            let stream = await quic.videoFrames()
            for await frame in stream {
                guard let self else { return }
                for continuation in self.videoFrameContinuations.values {
                    continuation.yield(frame)
                }
            }
        }
        videoConfigTask = Task { [weak self] in
            let stream = await quic.videoConfigurations()
            for await configuration in stream {
                guard let self else { return }
                for continuation in self.videoConfigContinuations.values {
                    continuation.yield(configuration)
                }
            }
        }
    }

    private func model(for windowID: String) -> WindowSurfaceModel {
        if let existing = surfaceModels[windowID] {
            return existing
        }
        let created = WindowSurfaceModel(windowID: windowID)
        surfaceModels[windowID] = created
        return created
    }

    private func apply(frame: SurfaceFrameData) {
        guard !frame.windowID.isEmpty else { return }
        let model = model(for: frame.windowID)
        model.store(frame)
        recompose(frame.windowID)
    }

    private func apply(surfaces: [AquaSurface]) {
        let byWindow = Dictionary(grouping: surfaces, by: { $0.windowID })
        for (windowID, list) in byWindow {
            model(for: windowID).setSurfaces(list)
            recompose(windowID)
        }
    }

    private func recompose(_ windowID: String) {
        guard let model = surfaceModels[windowID] else { return }
        if model.frames.isEmpty { return }
        let composition = SurfaceComposition(
            windowID: windowID,
            image: model.composition(),
            frameIDs: model.frames.mapValues { $0.frameID }
        )
        for continuation in compositionContinuations.values {
            continuation.yield(composition)
        }
    }

    private func observeConnectionStates(_ quic: QUICRemoteWindowService) {
        connectionStateTask?.cancel()
        connectionStateTask = Task { [weak self] in
            let stream = await quic.connectionStates()
            for await state in stream {
                self?.broadcastConnectionState(state)
            }
        }
    }

    private func broadcastConnectionState(_ state: RemoteConnectionState) {
        currentConnectionState = state
        for continuation in connectionContinuations.values {
            continuation.yield(state)
        }
    }

    private func process(_ event: RemoteWindowEvent) async {
        let change = await store.apply(event)
        await coordinator.handle(change)
        await broadcastWindowSnapshot()
    }

    private func broadcastWindowSnapshot() async {
        guard !updateContinuations.isEmpty else { return }
        let windows = await store.allWindows()
        for continuation in updateContinuations.values {
            continuation.yield(windows)
        }
    }
}
