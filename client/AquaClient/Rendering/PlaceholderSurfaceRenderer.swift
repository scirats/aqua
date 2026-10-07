import UIKit

/// Renders a neutral placeholder instead of a live remote surface.
///
/// It deliberately draws **no Linux title bar**: iPadOS owns window chrome.
/// The content only represents the future remote surface.
@MainActor
final class PlaceholderSurfaceRenderer: RemoteSurfaceRendering {
    let view: UIView

    private let content = PlaceholderContent()

    init() {
        view = content
    }

    func update(remoteWindow: RemoteWindow?) {
        if let remoteWindow {
            content.applicationLabel.text = remoteWindow.applicationName
            content.windowIDLabel.text = "Remote Window \(remoteWindow.id.value)"
            content.statusLabel.text = "Waiting for surface"
        } else {
            content.applicationLabel.text = "Remote Window"
            content.windowIDLabel.text = "unresolved"
            content.statusLabel.text = "Waiting for remote window"
        }
    }

    func update(viewport: RemoteViewport) {
        content.viewportLabel.text = "viewport \(Int(viewport.width))×\(Int(viewport.height)) @\(viewport.scale)x"
    }

    func update(sceneSessionIdentifier: String?) {
        content.sessionLabel.text = sceneSessionIdentifier.map { "session \($0.prefix(8))" }
    }
}

/// Backing view. Subclassed so the gradient tracks `bounds` without the
/// renderer having to observe layout.
@MainActor
private final class PlaceholderContent: UIView {
    let applicationLabel = UILabel()
    let windowIDLabel = UILabel()
    let statusLabel = UILabel()
    let viewportLabel = UILabel()
    let sessionLabel = UILabel()

    private let gradientLayer = CAGradientLayer()
    private let pulseView = UIView()

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .systemBackground
        configureGradient()
        configureLabels()
        configureLayout()
        startPulsing()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        gradientLayer.frame = bounds
    }

    private func configureGradient() {
        gradientLayer.colors = [
            UIColor.systemIndigo.withAlphaComponent(0.35).cgColor,
            UIColor.systemTeal.withAlphaComponent(0.15).cgColor,
            UIColor.systemBackground.cgColor,
        ]
        gradientLayer.startPoint = CGPoint(x: 0, y: 0)
        gradientLayer.endPoint = CGPoint(x: 1, y: 1)
        layer.insertSublayer(gradientLayer, at: 0)
    }

    private func configureLabels() {
        applicationLabel.font = .preferredFont(forTextStyle: .largeTitle)
        applicationLabel.adjustsFontForContentSizeCategory = true
        applicationLabel.textColor = .label
        applicationLabel.textAlignment = .center

        windowIDLabel.font = .monospacedSystemFont(ofSize: 15, weight: .semibold)
        windowIDLabel.textColor = .secondaryLabel
        windowIDLabel.textAlignment = .center

        statusLabel.font = .preferredFont(forTextStyle: .headline)
        statusLabel.textColor = .secondaryLabel
        statusLabel.textAlignment = .center
        statusLabel.text = "Waiting for surface"

        viewportLabel.font = .monospacedSystemFont(ofSize: 13, weight: .regular)
        viewportLabel.textColor = .tertiaryLabel
        viewportLabel.textAlignment = .center
        viewportLabel.numberOfLines = 0

        sessionLabel.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        sessionLabel.textColor = .tertiaryLabel
        sessionLabel.textAlignment = .center
        sessionLabel.numberOfLines = 0

        pulseView.backgroundColor = .systemGreen
        pulseView.layer.cornerRadius = 5
        pulseView.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            pulseView.widthAnchor.constraint(equalToConstant: 10),
            pulseView.heightAnchor.constraint(equalToConstant: 10),
        ])
    }

    private func configureLayout() {
        let statusRow = UIStackView(arrangedSubviews: [pulseView, statusLabel])
        statusRow.axis = .horizontal
        statusRow.spacing = 8
        statusRow.alignment = .center

        let stack = UIStackView(arrangedSubviews: [
            applicationLabel,
            windowIDLabel,
            statusRow,
            viewportLabel,
            sessionLabel,
        ])
        stack.axis = .vertical
        stack.alignment = .center
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)

        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: centerYAnchor),
            stack.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 24),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -24),
        ])
    }

    private func startPulsing() {
        let animation = CABasicAnimation(keyPath: "opacity")
        animation.fromValue = 1.0
        animation.toValue = 0.25
        animation.duration = 0.9
        animation.autoreverses = true
        animation.repeatCount = .infinity
        pulseView.layer.add(animation, forKey: "pulse")
    }
}
