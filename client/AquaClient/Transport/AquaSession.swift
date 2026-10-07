import CoreGraphics
import Foundation

/// Connection lifecycle as seen by the app. Deliberately distinct from
/// `RemoteWindowClosed`: a network drop must not look like a window closing.
enum RemoteConnectionState: Equatable {
    case disconnected
    case connecting
    case connected
    case reconnecting
    case incompatibleProtocol(String)
    case authenticationFailed(String)

    var label: String {
        switch self {
        case .disconnected: return "Disconnected"
        case .connecting: return "Connecting…"
        case .connected: return "Connected"
        case .reconnecting: return "Reconnecting…"
        case .incompatibleProtocol(let detail): return "Incompatible protocol (\(detail))"
        case .authenticationFailed(let detail): return "Authentication failed (\(detail))"
        }
    }

    var isTerminalFailure: Bool {
        switch self {
        case .incompatibleProtocol, .authenticationFailed: return true
        default: return false
        }
    }
}

/// A surface of a window's tree, as known on the client.
struct AquaSurface: Equatable, Sendable {
    var surfaceID: String
    var windowID: String
    var parentSurfaceID: String?
    var role: UInt32
    var position: CGPoint
    var size: CGSize
    var z: UInt32
}

/// Pure Aqua Protocol v1 session state: handshake result, snapshot
/// reconciliation, revision gating and window mirroring.
///
/// Contains no networking, so it is unit-testable directly.
struct AquaSession {
    struct HelloOutcome {
        var events: [RemoteWindowEvent] = []
        var error: String?
        var sessionChanged = false
    }

    private(set) var serverSessionID: String?
    private(set) var lastRevision: UInt64 = 0
    private(set) var windows: [String: RemoteWindow] = [:]
    private(set) var announced: Set<String> = []
    private(set) var surfaces: [String: AquaSurface] = [:]
    private(set) var capabilities = Capabilities()

    /// Apply ServerHello. On a new server session, all previously announced
    /// windows are closed and the mirror is reset.
    mutating func handleHello(_ hello: ServerHelloMessage) -> HelloOutcome {
        if !hello.error.isEmpty {
            return HelloOutcome(error: hello.error)
        }
        if hello.protocolVersion != AquaProtocol.version {
            return HelloOutcome(error: "unsupported_protocol_version")
        }
        capabilities = hello.capabilities

        var outcome = HelloOutcome()
        if let current = serverSessionID, current != hello.serverSessionID {
            // Server restarted: old windows are gone.
            for id in announced {
                if let window = windows.removeValue(forKey: id) {
                    outcome.events.append(.closed(window.id))
                }
            }
            announced.removeAll()
            surfaces.removeAll()
            outcome.sessionChanged = true
        }
        serverSessionID = hello.serverSessionID
        if hello.revision > lastRevision {
            lastRevision = hello.revision
        }
        return outcome
    }

    /// Reconcile a ServerSnapshot into the current mirror.
    ///
    /// Windows missing from the snapshot are closed (orphans); snapshot windows
    /// are created or updated. This never duplicates a window on reconnect.
    mutating func handleSnapshot(_ snapshot: ServerSnapshotMessage) -> [RemoteWindowEvent] {
        if snapshot.revision > lastRevision {
            lastRevision = snapshot.revision
        }
        let snapshotIDs = Set(snapshot.windows.map(\.windowID))
        var events: [RemoteWindowEvent] = []

        for id in announced.subtracting(snapshotIDs) {
            if let window = windows.removeValue(forKey: id) {
                events.append(.closed(window.id))
            }
            announced.remove(id)
        }

        for info in snapshot.windows {
            let window = Self.domainWindow(from: info)
            let existing = windows[info.windowID]
            windows[info.windowID] = window
            announced.insert(info.windowID)
            if existing == nil {
                events.append(.created(window))
            } else if existing != window {
                events.append(.updated(window))
            }
        }

        // Surfaces are authoritative from the snapshot.
        surfaces.removeAll()
        for info in snapshot.surfaces {
            surfaces[info.surfaceID] = Self.domainSurface(from: info)
        }
        return events
    }

    /// Apply an incremental server message with revision gating.
    mutating func handle(_ message: AquaServerMessage) -> [RemoteWindowEvent] {
        switch message {
        case .hello:
            // Handled by `handleHello`.
            return []
        case .snapshot:
            // Handled by `handleSnapshot`.
            return []
        case .windowCreated(let created):
            guard accept(revision: created.revision) else { return [] }
            let window = Self.domainWindow(from: created.window)
            windows[created.window.windowID] = window
            announced.insert(created.window.windowID)
            return [.created(window)]
        case .windowUpdated(let updated):
            guard accept(revision: updated.revision) else { return [] }
            let window = Self.domainWindow(from: updated.window)
            windows[updated.window.windowID] = window
            announced.insert(updated.window.windowID)
            return [.updated(window)]
        case .windowTitleChanged(let change):
            guard accept(revision: change.revision) else { return [] }
            guard var window = windows[change.windowID] else { return [] }
            window.title = change.title.isEmpty ? change.windowID : change.title
            windows[change.windowID] = window
            return [.updated(window)]
        case .windowApplicationChanged(let change):
            guard accept(revision: change.revision) else { return [] }
            guard var window = windows[change.windowID] else { return [] }
            let appID = change.applicationID.isEmpty ? "unknown" : change.applicationID
            window = RemoteWindow(
                id: window.id,
                applicationID: RemoteApplicationID(appID),
                applicationName: Self.applicationName(from: appID),
                title: window.title,
                minimumSize: window.minimumSize,
                maximumSize: window.maximumSize,
                state: window.state
            )
            windows[change.windowID] = window
            return [.updated(window)]
        case .windowStateChanged(let change):
            guard accept(revision: change.revision) else { return [] }
            guard var window = windows[change.windowID] else { return [] }
            window.state = Self.state(from: change.state)
            windows[change.windowID] = window
            return [.updated(window)]
        case .windowMapped:
            // `mapped` is not part of the client domain model yet.
            return []
        case .windowClosed(let closed):
            guard accept(revision: closed.revision) else { return [] }
            announced.remove(closed.windowID)
            guard let window = windows.removeValue(forKey: closed.windowID) else { return [] }
            return [.closed(window.id)]
        case .surfaceCreated(let created):
            guard accept(revision: created.revision) else { return [] }
            surfaces[created.surface.surfaceID] = Self.domainSurface(from: created.surface)
            return []
        case .surfaceUpdated(let updated):
            guard accept(revision: updated.revision) else { return [] }
            surfaces[updated.surface.surfaceID] = Self.domainSurface(from: updated.surface)
            return []
        case .surfaceDestroyed(let destroyed):
            guard accept(revision: destroyed.revision) else { return [] }
            surfaces.removeValue(forKey: destroyed.surfaceID)
            return []
        case .windowVideoConfig:
            // Phase 3C: decoder configuration for a future video data plane.
            // The window store does not consume it yet (SHM path unchanged).
            return []
        }
    }

    mutating func reset() {
        windows.removeAll()
        announced.removeAll()
        surfaces.removeAll()
        serverSessionID = nil
        lastRevision = 0
    }

    private mutating func accept(revision: UInt64) -> Bool {
        guard revision > lastRevision else { return false }
        lastRevision = revision
        return true
    }

    // MARK: - Mapping

    static func domainWindow(from info: WindowInfoMessage) -> RemoteWindow {
        let appID = info.applicationID.isEmpty ? "unknown" : info.applicationID
        return RemoteWindow(
            id: RemoteWindowID(info.windowID),
            applicationID: RemoteApplicationID(appID),
            applicationName: applicationName(from: appID),
            title: info.title.isEmpty ? info.windowID : info.title,
            minimumSize: info.hasMin ? CGSize(width: Int(info.minWidth), height: Int(info.minHeight)) : nil,
            maximumSize: info.hasMax ? CGSize(width: Int(info.maxWidth), height: Int(info.maxHeight)) : nil,
            state: state(from: info.state)
        )
    }

    static func domainSurface(from info: SurfaceInfoMessage) -> AquaSurface {
        AquaSurface(
            surfaceID: info.surfaceID,
            windowID: info.windowID,
            parentSurfaceID: info.parentSurfaceID.isEmpty ? nil : info.parentSurfaceID,
            role: info.role,
            position: CGPoint(x: Double(info.x), y: Double(info.y)),
            size: CGSize(width: Int(info.width), height: Int(info.height)),
            z: info.z
        )
    }

    /// Surfaces of a window, root first, then by z-order.
    func surfaces(for windowID: String) -> [AquaSurface] {
        surfaces.values
            .filter { $0.windowID == windowID }
            .sorted { lhs, rhs in
                let lRoot = lhs.parentSurfaceID == nil
                let rRoot = rhs.parentSurfaceID == nil
                if lRoot != rRoot { return lRoot }
                return lhs.z < rhs.z
            }
    }

    static func applicationName(from appID: String) -> String {
        appID.split(separator: ".").last.map(String.init) ?? appID
    }

    static func state(from value: UInt32) -> RemoteWindowState {
        switch value {
        case 1: return .maximized
        case 2: return .fullscreen
        case 3: return .minimized
        default: return .normal
        }
    }
}
