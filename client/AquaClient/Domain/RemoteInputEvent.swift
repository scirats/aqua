import Foundation

/// Neutral 2D point in remote-window content coordinates.
struct RemotePoint: Hashable, Codable, Sendable {
    var x: Double
    var y: Double

    init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }
}

/// Neutral 2D delta used for scroll events.
struct RemoteScrollDelta: Hashable, Codable, Sendable {
    var dx: Double
    var dy: Double

    init(dx: Double, dy: Double) {
        self.dx = dx
        self.dy = dy
    }
}

/// Pointer buttons. Mirrors the usual five button mouse layout.
enum RemotePointerButton: String, Codable, Sendable {
    case left
    case right
    case middle
    case back
    case forward
}

/// Modifier keys. Intentionally a plain `OptionSet` rather than anything tied
/// to `UIKeyModifierFlags`, so the domain stays UIKit free.
struct RemoteModifiers: OptionSet, Hashable, Codable, Sendable {
    let rawValue: UInt

    static let shift = RemoteModifiers(rawValue: 1 << 0)
    static let control = RemoteModifiers(rawValue: 1 << 1)
    static let option = RemoteModifiers(rawValue: 1 << 2)
    static let command = RemoteModifiers(rawValue: 1 << 3)
    static let capsLock = RemoteModifiers(rawValue: 1 << 4)
}

/// Platform neutral input event.
///
/// Touch is deliberately kept as an independent case instead of being
/// immediately folded into pointer events: Wayland supports `wl_touch`, and the
/// decision of whether to emulate a pointer is application dependent.
enum RemoteInputEvent: Sendable, Equatable {
    case pointerMoved(position: RemotePoint)
    case pointerButton(button: RemotePointerButton, isPressed: Bool, position: RemotePoint)
    case scroll(delta: RemoteScrollDelta)

    case keyDown(keyCode: UInt32, characters: String, modifiers: RemoteModifiers)
    case keyUp(keyCode: UInt32, characters: String, modifiers: RemoteModifiers)

    case touchDown(id: Int, position: RemotePoint)
    case touchMoved(id: Int, position: RemotePoint)
    case touchUp(id: Int, position: RemotePoint)
    case touchCancelled(id: Int, position: RemotePoint)
}

extension RemoteInputEvent {
    /// Stable, log friendly representation.
    var logDescription: String {
        switch self {
        case .pointerMoved(let p):
            return "pointerMoved x=\(p.x) y=\(p.y)"
        case .pointerButton(let button, let isPressed, let p):
            return "pointerButton button=\(button.rawValue) pressed=\(isPressed) x=\(p.x) y=\(p.y)"
        case .scroll(let d):
            return "scroll dx=\(d.dx) dy=\(d.dy)"
        case .keyDown(let code, let characters, let modifiers):
            return "keyDown keyCode=\(code) characters=\(characters.debugDescription) modifiers=\(modifiers.rawValue)"
        case .keyUp(let code, let characters, let modifiers):
            return "keyUp keyCode=\(code) characters=\(characters.debugDescription) modifiers=\(modifiers.rawValue)"
        case .touchDown(let id, let p):
            return "touchDown id=\(id) x=\(p.x) y=\(p.y)"
        case .touchMoved(let id, let p):
            return "touchMoved id=\(id) x=\(p.x) y=\(p.y)"
        case .touchUp(let id, let p):
            return "touchUp id=\(id) x=\(p.x) y=\(p.y)"
        case .touchCancelled(let id, let p):
            return "touchCancelled id=\(id) x=\(p.x) y=\(p.y)"
        }
    }
}
