import Foundation

/// Stable identifier of a single remote window.
///
/// Conceptually this maps to a Wayland `xdg_toplevel` in later phases, but the
/// client never mentions Wayland types. It is instance based, not global.
struct RemoteWindowID: Hashable, Codable, Sendable, CustomStringConvertible {
    let value: String

    init(_ value: String) {
        self.value = value
    }

    var description: String { value }
}

/// Stable identifier of a remote application (e.g. Firefox).
///
/// A single `RemoteApplicationID` can own many `RemoteWindowID`s. The unit of
/// window management is always the `RemoteWindow`, never the application.
struct RemoteApplicationID: Hashable, Codable, Sendable, CustomStringConvertible {
    let value: String

    init(_ value: String) {
        self.value = value
    }

    var description: String { value }
}

/// Transport neutral identity of a remote window.
///
/// Encodes to a stable URL-like string. That string is used as the
/// `NSUserActivity.targetContentIdentifier` so iPadOS can route an existing
/// scene session back to the remote window it represents.
///
/// Format: `remote://<server>/window/<remote-window-id>`
struct RemoteWindowIdentity: Hashable, Sendable, CustomStringConvertible {
    static let scheme = "remote"

    let server: String
    let windowID: RemoteWindowID

    init(server: String = "local", windowID: RemoteWindowID) {
        self.server = server
        self.windowID = windowID
    }

    init?(urlString: String) {
        guard let components = URLComponents(string: urlString),
              components.scheme == Self.scheme else {
            return nil
        }
        let parts = components.path.split(separator: "/").map(String.init)
        guard parts.count == 2, parts[0] == "window" else { return nil }
        self.server = components.host ?? "local"
        self.windowID = RemoteWindowID(parts[1])
    }

    var urlString: String {
        "\(Self.scheme)://\(server)/window/\(windowID.value)"
    }

    var description: String { urlString }
}
