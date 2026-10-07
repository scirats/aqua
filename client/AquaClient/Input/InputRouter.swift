import Foundation
import os

/// Captures iPad input for a single remote window and forwards neutral
/// `RemoteInputEvent`s to the transport.
///
/// Phase 1 only logs (via the mock service). The future path is:
///
///     iPad keyboard / pointer / touch -> InputRouter -> RemoteInputEvent
///         -> Linux Wayland seat
@MainActor
final class InputRouter {
    let remoteWindowID: RemoteWindowID
    private let service: any RemoteWindowService

    init(remoteWindowID: RemoteWindowID, service: any RemoteWindowService) {
        self.remoteWindowID = remoteWindowID
        self.service = service
    }

    func route(_ event: RemoteInputEvent) {
        Log.input.debug("window=\(self.remoteWindowID.value, privacy: .public) \(event.logDescription, privacy: .public)")
        let id = remoteWindowID
        Task { [service] in
            await service.inputDidOccur(id, event: event)
        }
    }
}
