import Foundation

/// Neutral description of a remote window.
///
/// This is the fundamental unit of windowing. One `RemoteWindow` eventually
/// maps to exactly one `UIWindowScene` on the iPad, and to an `xdg_toplevel`
/// on the Linux side in later phases.
struct RemoteWindow: Identifiable, Hashable, Codable, Sendable {
    let id: RemoteWindowID
    let applicationID: RemoteApplicationID
    var applicationName: String
    var title: String
    var minimumSize: CGSize?
    var maximumSize: CGSize?
    var state: RemoteWindowState

    init(
        id: RemoteWindowID,
        applicationID: RemoteApplicationID,
        applicationName: String,
        title: String,
        minimumSize: CGSize? = nil,
        maximumSize: CGSize? = nil,
        state: RemoteWindowState = .normal
    ) {
        self.id = id
        self.applicationID = applicationID
        self.applicationName = applicationName
        self.title = title
        self.minimumSize = minimumSize
        self.maximumSize = maximumSize
        self.state = state
    }
}

extension CGSize {
    /// A best effort `CGSize` decoded from an optional transport payload.
    var isUsableMinimum: Bool { width > 0 && height > 0 }
}
