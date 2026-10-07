//! Input helpers shared by the synthetic input path (events injected from the
//! iPad over the Aqua protocol).
//!
//! [`resolve_keycode`] maps a character (or a raw evdev code) to a Linux evdev
//! keycode; [`PointerButton`] names the three buttons and their Linux codes.

/// Pointer button understood by the synthetic input path.
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

/// Map a character to a Linux evdev keycode (as used with an xkb keymap the
/// keycode is `evdev + 8`, handled by the adapter).
pub fn evdev_keycode_for_char(c: char) -> Option<u32> {
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

/// Resolve a key argument: either a single character or a raw evdev code.
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
    fn char_mapping() {
        assert_eq!(resolve_keycode("A"), Some(30));
        assert_eq!(resolve_keycode("a"), Some(30));
        assert_eq!(resolve_keycode("65"), Some(65));
        assert_eq!(resolve_keycode("hello"), None);
    }
}
