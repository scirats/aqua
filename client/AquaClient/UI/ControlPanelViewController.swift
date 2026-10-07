import UIKit

/// Control panel shown by the initial (non-remote) scene.
///
/// Section 0 configures the Aqua QUIC transport (host/port/connect). The other
/// sections drive the local mock transport for offline demos.
final class ControlPanelViewController: UITableViewController {
    private let environment = AppEnvironment.shared
    private var windows: [RemoteWindow] = []
    private var connectionState: RemoteConnectionState = .disconnected
    private var windowTask: Task<Void, Never>?
    private var connectionTask: Task<Void, Never>?

    private struct OpenAction {
        let title: String
        let application: RemoteApplication
    }

    private let openActions: [OpenAction] = [
        OpenAction(title: "Open Firefox", application: .MockCatalog.firefox),
        OpenAction(title: "Open Firefox Window #2", application: .MockCatalog.firefox),
        OpenAction(title: "Open Visual Studio Code", application: .MockCatalog.code),
        OpenAction(title: "Open Terminal", application: .MockCatalog.terminal),
        OpenAction(title: "Open Files", application: .MockCatalog.files),
        OpenAction(title: "Open GIMP", application: .MockCatalog.gimp),
    ]

    private let connectionSection = 0
    private let openSection = 1
    private let windowsSection = 2
    private static let cellIdentifier = "Cell"
    private static let hostKey = "aqua.host"
    private static let portKey = "aqua.port"
    private static let fingerprintKey = "aqua.fingerprint"

    deinit {
        windowTask?.cancel()
        connectionTask?.cancel()
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        title = "Aqua Control Panel"
        navigationItem.largeTitleDisplayMode = .always
        navigationController?.navigationBar.prefersLargeTitles = true
        tableView.register(UITableViewCell.self, forCellReuseIdentifier: Self.cellIdentifier)
        tableView.allowsMultipleSelection = false

        windowTask = Task { [weak self] in
            guard let self else { return }
            for await windows in self.environment.windowUpdates() {
                self.windows = windows
                self.tableView.reloadSections(IndexSet(integer: self.windowsSection), with: .none)
            }
        }

        connectionTask = Task { [weak self] in
            guard let self else { return }
            for await state in self.environment.connectionUpdates() {
                self.connectionState = state
                self.tableView.reloadSections(IndexSet(integer: self.connectionSection), with: .none)
            }
        }
    }

    // MARK: - Table view

    override func numberOfSections(in tableView: UITableView) -> Int { 3 }

    override func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        switch section {
        case connectionSection: return environment.isUsingQUIC ? 2 : 1
        case openSection: return openActions.count
        case windowsSection: return windows.isEmpty ? 0 : windows.count + 1
        default: return 0
        }
    }

    override func tableView(_ tableView: UITableView, titleForHeaderInSection section: Int) -> String? {
        switch section {
        case connectionSection: return "Aqua Server (QUIC)"
        case openSection: return "Open New Window (mock local)"
        case windowsSection: return windows.isEmpty ? nil : "Remote Windows (\(windows.count))"
        default: return nil
        }
    }

    override func tableView(_ tableView: UITableView, titleForFooterInSection section: Int) -> String? {
        switch section {
        case connectionSection:
            return connectionState.label
                + (environment.isUsingQUIC ? "" : " — using local mock")
        case windowsSection where windows.isEmpty:
            return "Each remote window becomes its own UIWindowScene managed by iPadOS."
        default:
            return nil
        }
    }

    override func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: Self.cellIdentifier, for: indexPath)
        var content = cell.defaultContentConfiguration()

        switch indexPath.section {
        case connectionSection:
            if indexPath.row == 0 {
                content.text = "Connect to Server…"
                content.image = UIImage(systemName: "network")
            } else {
                content.text = "Disconnect"
                content.textProperties.color = .systemRed
                content.image = UIImage(systemName: "xmark.circle")
            }
            cell.accessoryType = .none
        case openSection:
            let action = openActions[indexPath.row]
            content.text = action.title
            content.secondaryText = action.application.desktopIdentifier
            content.image = UIImage(systemName: "plus.rectangle.on.rectangle")
            cell.accessoryType = .none
        default:
            if isCloseAllRow(indexPath) {
                content.text = "Close All Remote Windows"
                content.textProperties.color = .systemRed
                content.image = UIImage(systemName: "xmark.circle")
                cell.accessoryType = .none
            } else {
                let window = windows[indexPath.row]
                content.text = window.title
                content.secondaryText = "\(window.applicationName) · \(window.id.value) · \(window.state.rawValue)"
                content.image = UIImage(systemName: "macwindow")
                cell.accessoryType = .disclosureIndicator
            }
        }

        cell.contentConfiguration = content
        return cell
    }

    override func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)

        switch indexPath.section {
        case connectionSection:
            if indexPath.row == 0 {
                presentConnectAlert()
            } else {
                environment.disconnectQUIC()
            }
        case openSection:
            let application = openActions[indexPath.row].application
            Task { [environment] in await environment.open(application) }
        default:
            if isCloseAllRow(indexPath) {
                Task { [environment] in await environment.closeAllWindows() }
            } else {
                let window = windows[indexPath.row]
                Task { [environment] in await environment.activate(window) }
            }
        }
    }

    override func tableView(
        _ tableView: UITableView,
        trailingSwipeActionsConfigurationForRowAt indexPath: IndexPath
    ) -> UISwipeActionsConfiguration? {
        guard indexPath.section == windowsSection, !isCloseAllRow(indexPath) else { return nil }
        let window = windows[indexPath.row]
        let close = UIContextualAction(style: .destructive, title: "Close") { [environment] _, _, completion in
            Task {
                await environment.close(window.id)
                completion(true)
            }
        }
        close.image = UIImage(systemName: "xmark")
        return UISwipeActionsConfiguration(actions: [close])
    }

    // MARK: - Helpers

    private func isCloseAllRow(_ indexPath: IndexPath) -> Bool {
        indexPath.section == windowsSection && indexPath.row == windows.count
    }

    private func presentConnectAlert() {
        let defaults = UserDefaults.standard
        let alert = UIAlertController(
            title: "Connect to Aqua Server",
            message: "Enter the server IP (LAN or Tailscale 100.x.x.x) and port. "
                + "Optionally pin the server certificate fingerprint (no colons).",
            preferredStyle: .alert
        )
        alert.addTextField { field in
            field.placeholder = "Host"
            field.text = defaults.string(forKey: Self.hostKey) ?? "127.0.0.1"
            field.keyboardType = .URL
            field.autocapitalizationType = .none
        }
        alert.addTextField { field in
            field.placeholder = "Port"
            field.text = defaults.string(forKey: Self.portKey) ?? "52420"
            field.keyboardType = .numberPad
        }
        alert.addTextField { field in
            field.placeholder = "Fingerprint (optional)"
            field.text = defaults.string(forKey: Self.fingerprintKey)
            field.autocapitalizationType = .none
            field.autocorrectionType = .no
        }
        alert.addAction(UIAlertAction(title: "Cancel", style: .cancel))
        alert.addAction(UIAlertAction(title: "Connect", style: .default) { [weak self, weak alert] _ in
            guard let self, let fields = alert?.textFields else { return }
            let host = fields[0].text?.trimmingCharacters(in: .whitespaces) ?? "127.0.0.1"
            let portValue = UInt16(fields[1].text ?? "") ?? 52420
            let fingerprint = fields[2].text?.trimmingCharacters(in: .whitespaces)
            defaults.set(host, forKey: Self.hostKey)
            defaults.set(String(portValue), forKey: Self.portKey)
            if let fingerprint, !fingerprint.isEmpty {
                defaults.set(fingerprint, forKey: Self.fingerprintKey)
            }
            self.environment.connectQUIC(
                host: host,
                port: portValue,
                fingerprint: (fingerprint?.isEmpty ?? true) ? nil : fingerprint
            )
        })
        present(alert, animated: true)
    }
}
