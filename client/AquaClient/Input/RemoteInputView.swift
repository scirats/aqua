import UIKit

/// Captures input from the content of a single `RemoteWindowScene`.
///
/// Touch, pointer and keyboard are kept as distinct neutral events. Touch is
/// **not** folded into pointer events here: the eventual Wayland side may want
/// `wl_touch` or pointer emulation depending on the application.
@MainActor
final class RemoteInputView: UIView {
    var inputRouter: InputRouter?

    private var lastScrollTranslation: CGPoint = .zero

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        setupGestureRecognizers()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var canBecomeFirstResponder: Bool { true }

    // MARK: - Touch / pointer buttons

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            if touch.isPointerType {
                let button = RemotePointerButton(event?.buttonMask ?? [])
                route(.pointerButton(button: button, isPressed: true, position: point(for: touch)))
            } else {
                route(.touchDown(id: touch.identity, position: point(for: touch)))
            }
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            if touch.isPointerType {
                route(.pointerMoved(position: point(for: touch)))
            } else {
                route(.touchMoved(id: touch.identity, position: point(for: touch)))
            }
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            if touch.isPointerType {
                let button = RemotePointerButton(event?.buttonMask ?? [])
                route(.pointerButton(button: button, isPressed: false, position: point(for: touch)))
            } else {
                route(.touchUp(id: touch.identity, position: point(for: touch)))
            }
        }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            if touch.isPointerType {
                let button = RemotePointerButton(event?.buttonMask ?? [])
                route(.pointerButton(button: button, isPressed: false, position: point(for: touch)))
            } else {
                route(.touchCancelled(id: touch.identity, position: point(for: touch)))
            }
        }
    }

    // MARK: - Keyboard

    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var handled = false
        for press in presses {
            guard let key = press.key else { continue }
            handled = true
            route(.keyDown(
                keyCode: UInt32(key.keyCode.rawValue),
                characters: key.characters,
                modifiers: RemoteModifiers(key.modifierFlags)
            ))
        }
        if !handled { super.pressesBegan(presses, with: event) }
    }

    override func pressesEnded(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var handled = false
        for press in presses {
            guard let key = press.key else { continue }
            handled = true
            route(.keyUp(
                keyCode: UInt32(key.keyCode.rawValue),
                characters: key.characters,
                modifiers: RemoteModifiers(key.modifierFlags)
            ))
        }
        if !handled { super.pressesEnded(presses, with: event) }
    }

    // MARK: - Pointer hover + scroll

    private func setupGestureRecognizers() {
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(handleHover(_:)))
        addGestureRecognizer(hover)

        let scroll = UIPanGestureRecognizer(target: self, action: #selector(handleScroll(_:)))
        scroll.allowedScrollTypesMask = .all
        scroll.maximumNumberOfTouches = 0
        addGestureRecognizer(scroll)
    }

    @objc private func handleHover(_ recognizer: UIHoverGestureRecognizer) {
        switch recognizer.state {
        case .began, .changed:
            let location = recognizer.location(in: self)
            route(.pointerMoved(position: RemotePoint(x: location.x, y: location.y)))
        default:
            break
        }
    }

    @objc private func handleScroll(_ recognizer: UIPanGestureRecognizer) {
        // Indirect (trackpad) scroll arrives with zero touches. Direct finger
        // drags are treated as touch events instead.
        guard recognizer.numberOfTouches == 0 else { return }

        let translation = recognizer.translation(in: self)
        switch recognizer.state {
        case .began:
            lastScrollTranslation = translation
        case .changed:
            let dx = translation.x - lastScrollTranslation.x
            let dy = translation.y - lastScrollTranslation.y
            lastScrollTranslation = translation
            if dx != 0 || dy != 0 {
                route(.scroll(delta: RemoteScrollDelta(dx: dx, dy: dy)))
            }
        default:
            lastScrollTranslation = .zero
        }
    }

    // MARK: - Helpers

    private func route(_ event: RemoteInputEvent) {
        inputRouter?.route(event)
    }

    private func point(for touch: UITouch) -> RemotePoint {
        let location = touch.location(in: self)
        return RemotePoint(x: location.x, y: location.y)
    }
}

private extension UITouch {
    var isPointerType: Bool {
        type == .indirect || type == .indirectPointer
    }

    /// Stable per-gesture identity for a touch sequence.
    var identity: Int { ObjectIdentifier(self).hashValue }
}

private extension RemotePointerButton {
    init(_ mask: UIEvent.ButtonMask) {
        self = mask.contains(.secondary) ? .right : .left
    }
}

private extension RemoteModifiers {
    init(_ flags: UIKeyModifierFlags) {
        var value: RemoteModifiers = []
        if flags.contains(.shift) { value.insert(.shift) }
        if flags.contains(.control) { value.insert(.control) }
        if flags.contains(.alternate) { value.insert(.option) }
        if flags.contains(.command) { value.insert(.command) }
        if flags.contains(.alphaShift) { value.insert(.capsLock) }
        self = value
    }
}
