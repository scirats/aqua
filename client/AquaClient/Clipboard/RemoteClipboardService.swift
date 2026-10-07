import Foundation

/// Abstraction over the future clipboard bridge:
///
///     Wayland data device <-> bridge protocol <-> UIPasteboard
///
/// Phase 1 never transfers data. The protocol exists so no remote sync logic is
/// hard-coded into the UI, and so `UIPasteboard` can be wired in later.
protocol RemoteClipboardService: Sendable {
    /// The remote side's clipboard changed.
    func remoteClipboardChanged(to text: String?) async

    /// The local `UIPasteboard` changed.
    func localClipboardChanged(to text: String?) async

    /// The last text known from the remote side, if any.
    func currentRemoteText() async -> String?
}

/// In-memory stand-in used in phase 1. Logs only.
actor MockRemoteClipboardService: RemoteClipboardService {
    private var remoteText: String?

    func remoteClipboardChanged(to text: String?) async {
        remoteText = text
        Log.clipboard.debug("remote clipboard changed length=\(text?.count ?? 0, privacy: .public)")
    }

    func localClipboardChanged(to text: String?) async {
        Log.clipboard.debug("local clipboard changed length=\(text?.count ?? 0, privacy: .public)")
    }

    func currentRemoteText() async -> String? {
        remoteText
    }
}
