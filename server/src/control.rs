//! Local control channel used by the phase 2 demos.
//!
//! Commands arrive on stdin and are forwarded to the compositor loop through a
//! `calloop` channel. This replaces the future iPad transport: same idea
//! (`RemoteViewportChanged`, input events) but no networking.
//!
//! ```
//! use aqua_server::control::{parse, ControlCommand};
//! assert_eq!(
//!     parse("resize window-1 800 600"),
//!     ControlCommand::Resize { window: "window-1".into(), width: 800, height: 600 }
//! );
//! ```

/// Pointer button understood by the mock input source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

impl PointerButton {
    /// Linux input event code (`BTN_LEFT` etc.) as expected by Smithay.
    pub fn button_code(&self) -> u32 {
        match self {
            // BTN_LEFT / BTN_RIGHT / BTN_MIDDLE from linux/input-event-codes.h
            Self::Left => 0x110,
            Self::Right => 0x111,
            Self::Middle => 0x112,
        }
    }
}

/// A parsed control command.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlCommand {
    /// List known windows and surfaces.
    List,
    /// Request a new viewport for a window (becomes xdg_toplevel configure).
    Resize {
        window: String,
        width: i32,
        height: i32,
    },
    /// Give keyboard focus to a window.
    Focus { window: String },
    /// Move the pointer.
    PointerMove { x: f64, y: f64 },
    /// Press/release a pointer button.
    PointerButton {
        button: PointerButton,
        pressed: bool,
    },
    /// Scroll axis.
    Scroll { dx: f64, dy: f64 },
    /// Press/release a key. `key` is a single character or a raw evdev code.
    Key { key: String, pressed: bool },
    /// Stop the server.
    Quit,
    /// A line that could not be understood.
    Unknown(String),
    /// Blank input.
    Empty,
}

/// Parse a single control line.
pub fn parse(line: &str) -> ControlCommand {
    let line = line.trim();
    if line.is_empty() {
        return ControlCommand::Empty;
    }
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return ControlCommand::Empty;
    };

    match command {
        "list" | "windows" => ControlCommand::List,
        "quit" | "exit" => ControlCommand::Quit,
        "resize" => {
            let (Some(window), Some(width), Some(height)) =
                (parts.next(), parts.next(), parts.next())
            else {
                return ControlCommand::Unknown(line.to_string());
            };
            match (width.parse::<i32>(), height.parse::<i32>()) {
                (Ok(width), Ok(height)) => ControlCommand::Resize {
                    window: window.to_string(),
                    width,
                    height,
                },
                _ => ControlCommand::Unknown(line.to_string()),
            }
        }
        "focus" => match parts.next() {
            Some(window) => ControlCommand::Focus {
                window: window.to_string(),
            },
            None => ControlCommand::Unknown(line.to_string()),
        },
        "pointer" => match parts.next() {
            Some("move") => {
                let (Some(x), Some(y)) = (parts.next(), parts.next()) else {
                    return ControlCommand::Unknown(line.to_string());
                };
                match (x.parse::<f64>(), y.parse::<f64>()) {
                    (Ok(x), Ok(y)) => ControlCommand::PointerMove { x, y },
                    _ => ControlCommand::Unknown(line.to_string()),
                }
            }
            Some("button") => {
                let (Some(button), Some(state)) = (parts.next(), parts.next()) else {
                    return ControlCommand::Unknown(line.to_string());
                };
                let button = match button {
                    "left" => PointerButton::Left,
                    "right" => PointerButton::Right,
                    "middle" => PointerButton::Middle,
                    _ => return ControlCommand::Unknown(line.to_string()),
                };
                match state {
                    "down" | "pressed" => ControlCommand::PointerButton {
                        button,
                        pressed: true,
                    },
                    "up" | "released" => ControlCommand::PointerButton {
                        button,
                        pressed: false,
                    },
                    _ => ControlCommand::Unknown(line.to_string()),
                }
            }
            _ => ControlCommand::Unknown(line.to_string()),
        },
        "scroll" => {
            let (Some(dx), Some(dy)) = (parts.next(), parts.next()) else {
                return ControlCommand::Unknown(line.to_string());
            };
            match (dx.parse::<f64>(), dy.parse::<f64>()) {
                (Ok(dx), Ok(dy)) => ControlCommand::Scroll { dx, dy },
                _ => ControlCommand::Unknown(line.to_string()),
            }
        }
        "key" => {
            let (Some(key), Some(state)) = (parts.next(), parts.next()) else {
                return ControlCommand::Unknown(line.to_string());
            };
            match state {
                "down" | "pressed" => ControlCommand::Key {
                    key: key.to_string(),
                    pressed: true,
                },
                "up" | "released" => ControlCommand::Key {
                    key: key.to_string(),
                    pressed: false,
                },
                _ => ControlCommand::Unknown(line.to_string()),
            }
        }
        _ => ControlCommand::Unknown(line.to_string()),
    }
}

/// Map a character to a Linux evdev keycode (as used with an xkb keymap the
/// keycode is `evdev + 8`, handled by the adapter).
pub fn evdev_keycode_for_char(c: char) -> Option<u32> {
    // Minimal US-layout mapping sufficient for the phase 2 input demo.
    let code = match c.to_ascii_lowercase() {
        'a' => 30,
        'b' => 48,
        'c' => 46,
        'd' => 32,
        'e' => 18,
        'f' => 33,
        'g' => 34,
        'h' => 35,
        'i' => 23,
        'j' => 36,
        'k' => 37,
        'l' => 38,
        'm' => 50,
        'n' => 49,
        'o' => 24,
        'p' => 25,
        'q' => 16,
        'r' => 19,
        's' => 31,
        't' => 20,
        'u' => 22,
        'v' => 47,
        'w' => 17,
        'x' => 45,
        'y' => 21,
        'z' => 44,
        '1' => 2,
        '2' => 3,
        '3' => 4,
        '4' => 5,
        '5' => 6,
        '6' => 7,
        '7' => 8,
        '8' => 9,
        '9' => 10,
        '0' => 11,
        ' ' => 57,
        '\n' => 28,
        '\t' => 15,
        _ => return None,
    };
    Some(code)
}

/// Resolve a `key` command argument: either a single character or a raw evdev code.
pub fn resolve_keycode(key: &str) -> Option<u32> {
    if let Ok(code) = key.parse::<u32>() {
        return Some(code);
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => evdev_keycode_for_char(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_resize() {
        assert_eq!(
            parse("resize window-1 800 600"),
            ControlCommand::Resize {
                window: "window-1".into(),
                width: 800,
                height: 600
            }
        );
    }

    #[test]
    fn parses_pointer_and_key() {
        assert_eq!(
            parse("pointer move 100 100"),
            ControlCommand::PointerMove { x: 100.0, y: 100.0 }
        );
        assert_eq!(
            parse("pointer button left down"),
            ControlCommand::PointerButton {
                button: PointerButton::Left,
                pressed: true
            }
        );
        assert_eq!(
            parse("pointer button left up"),
            ControlCommand::PointerButton {
                button: PointerButton::Left,
                pressed: false
            }
        );
        assert_eq!(
            parse("key A down"),
            ControlCommand::Key {
                key: "A".into(),
                pressed: true
            }
        );
    }

    #[test]
    fn char_mapping() {
        assert_eq!(resolve_keycode("A"), Some(30));
        assert_eq!(resolve_keycode("a"), Some(30));
        assert_eq!(resolve_keycode("65"), Some(65));
        assert_eq!(resolve_keycode("hello"), None);
    }
}
