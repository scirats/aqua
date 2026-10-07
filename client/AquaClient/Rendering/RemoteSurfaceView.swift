import UIKit

/// The content view of one `RemoteWindowViewController`.
///
/// It owns a `RemoteSurfaceRendering` implementation and forwards model updates
/// to it. Swapping the renderer never requires changes to the window model or
/// the scene coordinator.
@MainActor
final class RemoteSurfaceView: UIView {
    private let renderer: RemoteSurfaceRendering

    init(renderer: RemoteSurfaceRendering = PlaceholderSurfaceRenderer()) {
        self.renderer = renderer
        super.init(frame: .zero)
        backgroundColor = .systemBackground

        let rendered = renderer.view
        rendered.translatesAutoresizingMaskIntoConstraints = false
        addSubview(rendered)
        NSLayoutConstraint.activate([
            rendered.leadingAnchor.constraint(equalTo: leadingAnchor),
            rendered.trailingAnchor.constraint(equalTo: trailingAnchor),
            rendered.topAnchor.constraint(equalTo: topAnchor),
            rendered.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func render(remoteWindow: RemoteWindow?) {
        renderer.update(remoteWindow: remoteWindow)
    }

    func render(viewport: RemoteViewport) {
        renderer.update(viewport: viewport)
    }

    func render(sceneSessionIdentifier: String?) {
        renderer.update(sceneSessionIdentifier: sceneSessionIdentifier)
    }
}
