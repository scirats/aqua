import UIKit

/// Content controller for exactly one `RemoteWindow`.
///
/// The window itself is managed by iPadOS: this controller only provides the
/// content of the scene. There is no Linux title bar and no artificial desktop.
final class RemoteWindowViewController: UIViewController {
    let remoteWindowID: RemoteWindowID

    private let surfaceView = RemoteSurfaceView()
    private let imageView = UIImageView()
    private let remoteInputView = RemoteInputView()
    private let environment = AppEnvironment.shared

    // Phase 3C video pipeline (bring-up decoder + keyframe-aware stream model).
    private let videoDecoder: VideoDecoding = AVSampleBufferDisplayLayerDecoder()
    private lazy var videoView: UIView = videoDecoder.view
    private var videoModel: VideoStreamModel?
    private var videoFrameTask: Task<Void, Never>?
    private var videoConfigTask: Task<Void, Never>?

    private var observationTask: Task<Void, Never>?
    private var compositionTask: Task<Void, Never>?
    private var lastViewport: RemoteViewport?
    private var currentWindow: RemoteWindow?
    private var hasSurfaceContent = false
    private var hasVideoContent = false

    init(remoteWindowID: RemoteWindowID) {
        self.remoteWindowID = remoteWindowID
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    deinit {
        observationTask?.cancel()
        compositionTask?.cancel()
        videoFrameTask?.cancel()
        videoConfigTask?.cancel()
    }

    override func loadView() {
        remoteInputView.inputRouter = InputRouter(remoteWindowID: remoteWindowID, service: environment.service)

        imageView.contentMode = .scaleAspectFit
        imageView.backgroundColor = .black
        imageView.isUserInteractionEnabled = false
        imageView.isHidden = true

        videoView.backgroundColor = .black
        videoView.isUserInteractionEnabled = false
        videoView.isHidden = true

        for subview in [surfaceView, imageView, videoView] as [UIView] {
            subview.translatesAutoresizingMaskIntoConstraints = false
            remoteInputView.addSubview(subview)
            NSLayoutConstraint.activate([
                subview.leadingAnchor.constraint(equalTo: remoteInputView.leadingAnchor),
                subview.trailingAnchor.constraint(equalTo: remoteInputView.trailingAnchor),
                subview.topAnchor.constraint(equalTo: remoteInputView.topAnchor),
                subview.bottomAnchor.constraint(equalTo: remoteInputView.bottomAnchor),
            ])
        }
        view = remoteInputView
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        surfaceView.render(remoteWindow: currentWindow)
        observationTask = Task { [weak self] in
            guard let self else { return }
            for await windows in self.environment.windowUpdates() {
                self.apply(windows)
            }
        }
        compositionTask = Task { [weak self] in
            guard let self else { return }
            for await composition in self.environment.surfaceCompositions() {
                self.apply(composition)
            }
        }
        startVideoPipeline()
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        remoteInputView.becomeFirstResponder()
        surfaceView.render(sceneSessionIdentifier: view.window?.windowScene?.session.persistentIdentifier)
        updateViewportIfNeeded()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        updateViewportIfNeeded()
    }

    // MARK: - Private

    private func apply(_ windows: [RemoteWindow]) {
        guard let window = windows.first(where: { $0.id == remoteWindowID }) else { return }
        currentWindow = window
        surfaceView.render(remoteWindow: window)
    }

    /// Phase 3B: show real pixels once the server streams a composed surface.
    private func apply(_ composition: SurfaceComposition) {
        guard composition.windowID == remoteWindowID.value else { return }
        // Encoded video takes precedence when the server offers it.
        guard !hasVideoContent else { return }
        guard let image = composition.image else { return }
        imageView.image = UIImage(cgImage: image)
        if !hasSurfaceContent {
            hasSurfaceContent = true
            surfaceView.isHidden = true
            imageView.isHidden = false
        }
        // Presentation feedback (best-effort; drives future frame callbacks).
        for (surfaceID, frameID) in composition.frameIDs {
            environment.presentedFrame(surfaceID: surfaceID, frameID: frameID)
        }
    }

    // MARK: - Phase 3C video pipeline

    private func startVideoPipeline() {
        let model = VideoStreamModel(windowID: remoteWindowID.value)
        model.onRequestKeyframe = { [weak self] windowID, reason in
            self?.environment.requestKeyframe(windowID: windowID, reason: reason)
        }
        videoModel = model

        videoConfigTask = Task { [weak self] in
            guard let self else { return }
            for await configuration in self.environment.videoConfigurations() {
                guard configuration.windowID == self.remoteWindowID.value else { continue }
                self.videoDecoder.configure(configuration)
                self.videoModel?.apply(configuration)
            }
        }
        videoFrameTask = Task { [weak self] in
            guard let self else { return }
            for await frame in self.environment.videoFrames() {
                guard frame.windowID == self.remoteWindowID.value else { continue }
                self.consume(frame)
            }
        }
    }

    private func consume(_ frame: EncodedVideoFrame) {
        guard let videoModel else { return }
        switch videoModel.receive(frame) {
        case .forward(let decodable):
            videoDecoder.decode(decodable)
            if !hasVideoContent {
                hasVideoContent = true
                surfaceView.isHidden = true
                imageView.isHidden = true
                videoView.isHidden = false
            }
        case .requestedKeyframe, .waitingForKeyframe, .droppedStale, .ignored:
            break
        }
    }

    /// Detects an iPad-side viewport change and registers it with the transport.
    ///
    /// Future flow:
    ///
    ///     iPadOS resize -> RemoteWindowViewportChanged -> mock service
    ///         -> protocol -> Linux compositor -> xdg_toplevel.configure
    private func updateViewportIfNeeded() {
        let bounds = view.bounds
        guard bounds.width > 0, bounds.height > 0 else { return }

        let scale = Double(view.window?.windowScene?.traitCollection.displayScale
            ?? traitCollection.displayScale)
        let viewport = RemoteViewport(
            width: Double(bounds.width),
            height: Double(bounds.height),
            scale: scale
        )
        guard viewport != lastViewport else { return }
        lastViewport = viewport
        surfaceView.render(viewport: viewport)

        let id = remoteWindowID
        Task { [environment] in
            await environment.viewportChanged(id, viewport: viewport)
        }
    }
}
