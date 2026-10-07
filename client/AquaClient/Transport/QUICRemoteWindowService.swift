import CryptoKit
import Foundation
import Network
import Security
import os

/// QUIC implementation of `RemoteWindowService` (Aqua Protocol v1).
///
/// This is a drop-in replacement for `MockRemoteWindowService`; the rest of the
/// app (store, scene coordinator, view controllers) does not know it exists.
/// It only translates between the neutral domain and the wire.
@available(iOS 26.0, *)
actor QUICRemoteWindowService: RemoteWindowService {
    private let log = Logger(subsystem: "com.scirats.aqua", category: "quic")

    private var session = AquaSession()
    private var connection: NetworkConnection<QUIC>?
    private var controlStream: QUIC.Stream<QUICStream>?
    private var runTask: Task<Void, Never>?
    private var readyContinuation: CheckedContinuation<Void, Error>?
    private var viewportFlushTask: Task<Void, Never>?

    private var eventStream: AsyncStream<RemoteWindowEvent>?
    private var eventContinuation: AsyncStream<RemoteWindowEvent>.Continuation?
    private var stateContinuation: AsyncStream<RemoteConnectionState>.Continuation?

    private var host = ""
    private var port: UInt16 = 52420
    private var expectedFingerprint: String?
    private var observedFingerprint: String?
    private var shouldStayConnected = false
    private var currentState: RemoteConnectionState = .disconnected
    private var pendingViewports: [String: RemoteViewport] = [:]
    private var frameContinuations: [UUID: AsyncStream<SurfaceFrameData>.Continuation] = [:]
    private var surfaceContinuations: [UUID: AsyncStream<[AquaSurface]>.Continuation] = [:]
    private var videoFrameContinuations: [UUID: AsyncStream<EncodedVideoFrame>.Continuation] = [:]
    private var videoConfigContinuations: [UUID: AsyncStream<WindowVideoConfiguration>.Continuation] = [:]
    private var clientSessionID = UUID().uuidString

    // MARK: - RemoteWindowService

    func events() async -> AsyncStream<RemoteWindowEvent> {
        if let eventStream {
            return eventStream
        }
        let (stream, continuation) = AsyncStream.makeStream(
            of: RemoteWindowEvent.self,
            bufferingPolicy: .unbounded
        )
        eventContinuation = continuation
        eventStream = stream
        return stream
    }

    /// Data-plane frames (one stream per surface, latest-wins on the consumer).
    func frames() -> AsyncStream<SurfaceFrameData> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: SurfaceFrameData.self,
            bufferingPolicy: .bufferingNewest(8)
        )
        let token = UUID()
        frameContinuations[token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeFrameContinuation(token) }
        }
        return stream
    }

    /// Surface-tree updates for all windows.
    func surfaceUpdates() -> AsyncStream<[AquaSurface]> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: [AquaSurface].self,
            bufferingPolicy: .bufferingNewest(1)
        )
        let token = UUID()
        surfaceContinuations[token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeSurfaceContinuation(token) }
        }
        continuation.yield(Array(session.surfaces.values))
        return stream
    }

    /// Encoded window video frames (one stream per RemoteWindow, latest-wins on
    /// the consumer). Empty until the server implements the video data plane.
    func videoFrames() -> AsyncStream<EncodedVideoFrame> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: EncodedVideoFrame.self,
            bufferingPolicy: .bufferingNewest(8)
        )
        let token = UUID()
        videoFrameContinuations[token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeVideoFrameContinuation(token) }
        }
        return stream
    }

    /// Window video configurations (control-plane `WindowVideoConfig` and
    /// data-plane `CONFIG`). The data-plane one carries the codec parameter sets.
    func videoConfigurations() -> AsyncStream<WindowVideoConfiguration> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: WindowVideoConfiguration.self,
            bufferingPolicy: .bufferingNewest(4)
        )
        let token = UUID()
        videoConfigContinuations[token] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeVideoConfigContinuation(token) }
        }
        return stream
    }

    /// Ask the server encoder for a keyframe (decoder reset / dropped GOP).
    func requestKeyframe(windowID: String, reason: String) async {
        var message = RequestKeyframeMessage()
        message.windowID = windowID
        message.reason = reason
        await send(.requestKeyframe(message))
    }

    private func removeFrameContinuation(_ token: UUID) { frameContinuations[token] = nil }
    private func removeSurfaceContinuation(_ token: UUID) { surfaceContinuations[token] = nil }
    private func removeVideoFrameContinuation(_ token: UUID) { videoFrameContinuations[token] = nil }
    private func removeVideoConfigContinuation(_ token: UUID) { videoConfigContinuations[token] = nil }

    /// Report that the iPad presented a surface frame (feedback loop).
    func presentedFrame(surfaceID: String, frameID: UInt64) async {
        var message = FramePresentedMessage()
        message.surfaceID = surfaceID
        message.frameID = frameID
        message.presentationTimeUS = UInt64(Date().timeIntervalSince1970 * 1_000_000)
        await send(.framePresented(message))
    }

    /// The server owns window creation; the client is a viewer in phase 3A.
    func openApplication(_ application: RemoteApplication) async {
        log.info("openApplication ignored: server owns windows (\(application.id.value, privacy: .public))")
    }

    func closeWindow(_ id: RemoteWindowID) async {
        log.info("closeWindow ignored in phase 3A (\(id.value, privacy: .public))")
    }

    func viewportDidChange(_ id: RemoteWindowID, viewport: RemoteViewport) async {
        pendingViewports[id.value] = viewport
        scheduleViewportFlush()
    }

    func inputDidOccur(_ id: RemoteWindowID, event: RemoteInputEvent) async {
        guard let message = Self.clientMessage(windowID: id.value, event: event) else { return }
        await send(message)
        log.debug("input.sent \(id.value, privacy: .public) \(event.logDescription, privacy: .public)")
    }

    // MARK: - Connection control

    func connectionStates() -> AsyncStream<RemoteConnectionState> {
        let (stream, continuation) = AsyncStream.makeStream(
            of: RemoteConnectionState.self,
            bufferingPolicy: .bufferingNewest(4)
        )
        stateContinuation = continuation
        continuation.yield(currentState)
        return stream
    }

    func connect(host: String, port: UInt16, expectedFingerprint: String?) {
        self.host = host
        self.port = port
        self.expectedFingerprint = expectedFingerprint?
            .replacingOccurrences(of: ":", with: "")
            .lowercased()
        self.clientSessionID = UUID().uuidString
        shouldStayConnected = true
        if runTask == nil {
            runTask = Task { [weak self] in
                await self?.runLoop()
            }
        }
    }

    func disconnect() {
        shouldStayConnected = false
        runTask?.cancel()
        runTask = nil
        readyContinuation?.resume(throwing: CancellationError())
        readyContinuation = nil
        connection = nil
        controlStream = nil
        session.reset()
        setState(.disconnected)
    }

    var state: RemoteConnectionState { currentState }
    var serverSessionID: String? { session.serverSessionID }
    var pinnedFingerprint: String? { expectedFingerprint ?? observedFingerprint }

    // MARK: - Run loop

    private func runLoop() async {
        var attempt = 0
        while shouldStayConnected {
            setState(attempt == 0 ? .connecting : .reconnecting)
            do {
                try await runOnce()
                attempt = 0
            } catch let failure as ConnectionFailure {
                if failure.isFatal {
                    setState(
                        failure.kind == .protocolMismatch
                            ? .incompatibleProtocol(failure.detail)
                            : .authenticationFailed(failure.detail)
                    )
                    shouldStayConnected = false
                    break
                }
                log.warning("connection ended: \(failure.detail, privacy: .public)")
            } catch is CancellationError {
                break
            } catch {
                log.warning("connection error: \(error.localizedDescription, privacy: .public)")
            }
            guard shouldStayConnected else { break }
            attempt += 1
            let backoff = min(Double(attempt), 5)
            try? await Task.sleep(for: .seconds(backoff))
        }
        teardown()
        setState(.disconnected)
    }

    private func runOnce() async throws {
        guard let nwPort = NWEndpoint.Port(rawValue: port) else {
            throw ConnectionFailure(kind: .other, detail: "invalid port", isFatal: true)
        }
        session.reset()

        let connection = NetworkConnection(to: .hostPort(host: .init(host), port: nwPort)) {
            QUIC(alpn: [AquaProtocol.alpn])
                .tls.certificateValidator { [weak self] _, trust in
                    let fingerprint = Self.fingerprint(of: trust)
                    guard let self else { return false }
                    return await self.validateServer(fingerprint: fingerprint)
                }
        }
        self.connection = connection

        _ = connection.onStateUpdate { [weak self] _, state in
            Task { await self?.handleState(state) }
        }
        _ = connection.start()

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            self.readyContinuation = continuation
        }

        let stream = try await connection.openStream(directionality: .bidirectional)
        controlStream = stream

        var hello = ClientHelloMessage()
        hello.protocolVersion = AquaProtocol.version
        hello.clientSessionID = clientSessionID
        hello.capabilities.bits = AquaProtocol.Capability.phase3B
        hello.clientName = "iPadOS"
        try await stream.send(AquaClientMessage.hello(hello).encodeFrame())
        log.info("handshake.client session=\(self.clientSessionID, privacy: .public)")

        // Accept server-opened unidirectional data-plane streams (raw SHM
        // surface streams and/or encoded window video streams).
        Task { [weak self] in
            guard let self else { return }
            try? await connection.inboundStreams { inbound in
                await self.readInboundStream(inbound)
            }
        }

        try await receiveLoop(stream: stream)
    }

    /// Reads the first header of a server-opened data stream, then dispatches to
    /// the SHM surface reader or the window video reader based on `stream_type`.
    private func readInboundStream(_ stream: QUIC.Stream<QUICStream>) async {
        do {
            let firstHeader = try await readRawHeader(stream)
            let probe = (try? DataStreamProbe.decodeBody(firstHeader)) ?? DataStreamProbe()
            if probe.streamType == AquaProtocol.DataStreamType.windowVideo {
                try await readWindowVideoStream(stream, firstHeader: firstHeader)
            } else {
                try await readSurfaceStream(stream, firstHeader: firstHeader)
            }
        } catch {
            return
        }
    }

    /// Reads one raw SHM surface stream: a HELLO then a sequence of frames.
    private func readSurfaceStream(_ stream: QUIC.Stream<QUICStream>, firstHeader: Data) async throws {
        var headerData = firstHeader
        var surfaceID = ""
        while !Task.isCancelled {
            let header = try SurfaceStreamHeaderMessage.decodeBody(headerData)
            let payload = try await readPayload(stream, length: header.payloadLen)
            switch header.kind {
            case AquaProtocol.SurfaceStreamKind.hello:
                surfaceID = header.surfaceID
                log.debug("surface.stream.hello surface=\(surfaceID, privacy: .public)")
            case AquaProtocol.SurfaceStreamKind.frame:
                let frame = SurfaceFrameData(
                    surfaceID: header.surfaceID.isEmpty ? surfaceID : header.surfaceID,
                    windowID: header.windowID,
                    frameID: header.frameID,
                    width: header.width,
                    height: header.height,
                    stride: header.stride,
                    format: header.format,
                    data: payload
                )
                for continuation in frameContinuations.values {
                    continuation.yield(frame)
                }
            default:
                break
            }
            headerData = try await readRawHeader(stream)
        }
    }

    /// Reads one encoded window video stream: HELLO, CONFIG and FRAME messages.
    private func readWindowVideoStream(_ stream: QUIC.Stream<QUICStream>, firstHeader: Data) async throws {
        var headerData = firstHeader
        var windowID = ""
        while !Task.isCancelled {
            let header = try WindowVideoStreamHeaderMessage.decodeBody(headerData)
            let payload = try await readPayload(stream, length: header.payloadLen)
            let resolvedWindowID = header.windowID.isEmpty ? windowID : header.windowID
            switch header.kind {
            case AquaProtocol.WindowVideoStreamKind.hello:
                windowID = header.windowID
                log.debug("window.video.hello window=\(windowID, privacy: .public)")
            case AquaProtocol.WindowVideoStreamKind.config:
                let configuration = WindowVideoConfiguration(
                    windowID: resolvedWindowID,
                    codec: header.codec,
                    chroma: header.chroma,
                    width: header.width,
                    height: header.height,
                    codecConfiguration: header.codecConfig ? payload : nil
                )
                for continuation in videoConfigContinuations.values {
                    continuation.yield(configuration)
                }
            case AquaProtocol.WindowVideoStreamKind.frame:
                let frame = EncodedVideoFrame(
                    windowID: resolvedWindowID,
                    codec: header.codec,
                    chroma: header.chroma,
                    width: header.width,
                    height: header.height,
                    frameID: header.frameID,
                    keyframe: header.keyframe,
                    ptsUS: header.ptsUS,
                    data: payload
                )
                for continuation in videoFrameContinuations.values {
                    continuation.yield(frame)
                }
            default:
                break
            }
            headerData = try await readRawHeader(stream)
        }
    }

    private func readRawHeader(_ stream: QUIC.Stream<QUICStream>) async throws -> Data {
        let lengthData = try await stream.receive(exactly: 4).content
        let bytes = [UInt8](lengthData)
        let headerLength = (Int(bytes[0]) << 24) | (Int(bytes[1]) << 16) | (Int(bytes[2]) << 8) | Int(bytes[3])
        guard headerLength > 0, headerLength <= 64 * 1024 else {
            throw ConnectionFailure(kind: .other, detail: "bad stream header", isFatal: true)
        }
        return try await stream.receive(exactly: headerLength).content
    }

    private func readPayload(_ stream: QUIC.Stream<QUICStream>, length: UInt64) async throws -> Data {
        guard length <= 64 * 1024 * 1024 else {
            throw ConnectionFailure(kind: .other, detail: "payload too large", isFatal: true)
        }
        guard length > 0 else { return Data() }
        return try await stream.receive(exactly: Int(length)).content
    }

    private func receiveLoop(stream: QUIC.Stream<QUICStream>) async throws {
        while !Task.isCancelled {
            let headerMessage = try await stream.receive(exactly: AquaProtocol.headerLength)
            guard let (tag, length) = AquaFrame.decodeHeader(headerMessage.content) else {
                continue
            }
            guard length <= AquaProtocol.maxPayload else {
                throw ConnectionFailure(kind: .other, detail: "frame too large", isFatal: true)
            }
            var payload = Data()
            if length > 0 {
                payload = try await stream.receive(exactly: length).content
            }
            guard let message = AquaServerMessage.decode(tag: tag, payload: payload) else {
                log.debug("ignoring unknown message tag=\(tag, privacy: .public)")
                continue
            }
            await handleServerMessage(message)
        }
    }

    private func handleServerMessage(_ message: AquaServerMessage) async {
        switch message {
        case .hello(let hello):
            let outcome = session.handleHello(hello)
            if let error = outcome.error {
                log.error("handshake rejected: \(error, privacy: .public)")
                setState(
                    error == "unsupported_protocol_version"
                        ? .incompatibleProtocol(error)
                        : .authenticationFailed(error)
                )
                return
            }
            if outcome.sessionChanged {
                log.notice("server session changed -> \(hello.serverSessionID, privacy: .public)")
            }
            log.info("handshake.server session=\(hello.serverSessionID, privacy: .public) revision=\(hello.revision, privacy: .public)")
            emit(outcome.events)
        case .snapshot(let snapshot):
            let events = session.handleSnapshot(snapshot)
            log.info("snapshot.received revision=\(snapshot.revision, privacy: .public) windows=\(snapshot.windows.count, privacy: .public)")
            emit(events)
        case .windowVideoConfig(let config):
            let configuration = WindowVideoConfiguration(
                windowID: config.windowID,
                codec: config.codec,
                chroma: config.chroma,
                width: config.width,
                height: config.height,
                codecConfiguration: nil
            )
            for continuation in videoConfigContinuations.values {
                continuation.yield(configuration)
            }
        default:
            emit(session.handle(message))
        }
        broadcastSurfaces()
    }

    private func broadcastSurfaces() {
        let list = Array(session.surfaces.values)
        for continuation in surfaceContinuations.values {
            continuation.yield(list)
        }
    }

    private func handleState(_ state: NetworkChannel<QUIC>.State) async {
        switch state {
        case .ready:
            log.info("connection.ready")
            readyContinuation?.resume()
            readyContinuation = nil
            setState(.connected)
        case .waiting(let error):
            log.debug("connection.waiting \(error.localizedDescription, privacy: .public)")
        case .preparing, .setup:
            break
        case .failed(let error):
            log.warning("connection.failed \(error.localizedDescription, privacy: .public)")
            readyContinuation?.resume(throwing: error)
            readyContinuation = nil
        case .cancelled:
            readyContinuation?.resume(throwing: CancellationError())
            readyContinuation = nil
        @unknown default:
            break
        }
    }

    private func validateServer(fingerprint: String) async -> Bool {
        observedFingerprint = fingerprint
        guard let expected = expectedFingerprint else {
            // Trust-on-first-use for development; the fingerprint is kept for
            // the session and can be pinned explicitly. See docs/TRANSPORT.md.
            log.warning("TOFU: pinning server \(fingerprint, privacy: .public) for this session")
            return true
        }
        let matches = expected == fingerprint
        if !matches {
            log.error("certificate rejected: expected \(expected, privacy: .public) got \(fingerprint, privacy: .public)")
        }
        return matches
    }

    private func teardown() {
        readyContinuation?.resume(throwing: CancellationError())
        readyContinuation = nil
        viewportFlushTask?.cancel()
        viewportFlushTask = nil
        pendingViewports.removeAll()
        connection = nil
        controlStream = nil
    }

    // MARK: - Sending

    private func send(_ message: AquaClientMessage) async {
        guard let stream = controlStream else { return }
        do {
            try await stream.send(message.encodeFrame())
        } catch {
            log.debug("send failed: \(error.localizedDescription, privacy: .public)")
        }
    }

    private func scheduleViewportFlush() {
        guard viewportFlushTask == nil else { return }
        viewportFlushTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(33))
            await self?.flushViewports()
        }
    }

    private func flushViewports() async {
        viewportFlushTask = nil
        let pending = pendingViewports
        pendingViewports.removeAll()
        for (windowID, viewport) in pending {
            var message = ViewportChangedMessage()
            message.windowID = windowID
            message.width = Int32(viewport.width.rounded())
            message.height = Int32(viewport.height.rounded())
            message.scale = viewport.scale
            message.isFinal = true
            await send(.viewportChanged(message))
            log.info("viewport.sent window=\(windowID, privacy: .public) width=\(message.width, privacy: .public) height=\(message.height, privacy: .public)")
        }
    }

    private func emit(_ events: [RemoteWindowEvent]) {
        for event in events {
            eventContinuation?.yield(event)
            log.info("window.event \(event.kindDescription, privacy: .public)")
        }
    }

    private func setState(_ state: RemoteConnectionState) {
        guard state != currentState else { return }
        currentState = state
        stateContinuation?.yield(state)
        log.info("connection.state \(state.label, privacy: .public)")
    }

    // MARK: - Mapping

    private static func clientMessage(windowID: String, event: RemoteInputEvent) -> AquaClientMessage? {
        switch event {
        case .pointerMoved(let position):
            var message = PointerMovedMessage()
            message.windowID = windowID
            message.x = position.x
            message.y = position.y
            return .pointerMoved(message)
        case .pointerButton(let button, let pressed, let position):
            var message = PointerButtonMessage()
            message.windowID = windowID
            message.button = buttonCode(button)
            message.pressed = pressed
            message.x = position.x
            message.y = position.y
            return .pointerButton(message)
        case .scroll(let delta):
            var message = PointerScrollMessage()
            message.windowID = windowID
            message.dx = delta.dx
            message.dy = delta.dy
            return .pointerScroll(message)
        case .keyDown(let keyCode, let characters, let modifiers):
            return .key(keyMessage(windowID: windowID, keyCode: keyCode, characters: characters, modifiers: modifiers, pressed: true))
        case .keyUp(let keyCode, let characters, let modifiers):
            return .key(keyMessage(windowID: windowID, keyCode: keyCode, characters: characters, modifiers: modifiers, pressed: false))
        case .touchDown(let id, let position):
            return .touch(touchMessage(windowID: windowID, id: id, phase: 0, position: position))
        case .touchMoved(let id, let position):
            return .touch(touchMessage(windowID: windowID, id: id, phase: 1, position: position))
        case .touchUp(let id, let position):
            return .touch(touchMessage(windowID: windowID, id: id, phase: 2, position: position))
        case .touchCancelled(let id, let position):
            return .touch(touchMessage(windowID: windowID, id: id, phase: 3, position: position))
        }
    }

    private static func keyMessage(windowID: String, keyCode: UInt32, characters: String, modifiers: RemoteModifiers, pressed: Bool) -> KeyEventMessage {
        var message = KeyEventMessage()
        message.windowID = windowID
        // The client only guarantees characters; the server resolves the
        // evdev keycode. keycode stays 0 (unknown) unless already evdev.
        message.keycode = 0
        message.characters = characters
        message.modifiers = UInt32(truncatingIfNeeded: modifiers.rawValue)
        message.pressed = pressed
        return message
    }

    private static func touchMessage(windowID: String, id: Int, phase: UInt32, position: RemotePoint) -> TouchEventMessage {
        var message = TouchEventMessage()
        message.windowID = windowID
        message.touchID = UInt32(truncatingIfNeeded: id)
        message.phase = phase
        message.x = position.x
        message.y = position.y
        return message
    }

    /// Linux evdev button codes (`linux/input-event-codes.h`).
    private static func buttonCode(_ button: RemotePointerButton) -> UInt32 {
        switch button {
        case .left: return 0x110
        case .right: return 0x111
        case .middle: return 0x112
        case .back: return 0x113
        case .forward: return 0x114
        }
    }

    private static func fingerprint(of trust: sec_trust_t) -> String {
        let secTrust = sec_trust_copy_ref(trust).takeRetainedValue()
        guard
            let chain = SecTrustCopyCertificateChain(secTrust) as? [SecCertificate],
            let certificate = chain.first
        else {
            return ""
        }
        let data = SecCertificateCopyData(certificate) as Data
        return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }
}

/// Internal failure classification for the reconnect loop.
private struct ConnectionFailure: Error {
    enum Kind { case protocolMismatch, authentication, other }
    var kind: Kind
    var detail: String
    var isFatal: Bool
}
