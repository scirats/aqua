import Foundation

/// A remote application that can own one or more windows.
struct RemoteApplication: Hashable, Codable, Sendable {
    let id: RemoteApplicationID
    var name: String
    /// Loose analogue of a desktop application id (e.g. `org.mozilla.firefox`).
    /// Kept as opaque metadata, never interpreted by the client.
    var desktopIdentifier: String?
    /// Preferred default window title used when the remote side does not send one.
    var defaultWindowTitle: String?
}

extension RemoteApplication {
    /// Default catalog served by `MockRemoteWindowService`.
    enum MockCatalog {
        static let firefox = RemoteApplication(
            id: RemoteApplicationID("firefox"),
            name: "Firefox",
            desktopIdentifier: "org.mozilla.firefox"
        )
        static let code = RemoteApplication(
            id: RemoteApplicationID("vscode"),
            name: "Visual Studio Code",
            desktopIdentifier: "com.microsoft.VSCode",
            defaultWindowTitle: "Visual Studio Code"
        )
        static let terminal = RemoteApplication(
            id: RemoteApplicationID("terminal"),
            name: "Terminal",
            desktopIdentifier: "org.gnome.Terminal"
        )
        static let files = RemoteApplication(
            id: RemoteApplicationID("files"),
            name: "Files",
            desktopIdentifier: "org.gnome.Nautilus"
        )
        static let gimp = RemoteApplication(
            id: RemoteApplicationID("gimp"),
            name: "GIMP",
            desktopIdentifier: "org.gimp.GIMP"
        )

        static let all: [RemoteApplication] = [firefox, code, terminal, files, gimp]

        static func application(for id: RemoteApplicationID) -> RemoteApplication? {
            all.first { $0.id == id }
        }
    }
}
