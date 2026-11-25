//! Hotkeys: the key combination that presses a button.
//!
//! Every button of the program has one; the host shows it when the
//! pointer rests on the button, and presses the button when the keys are
//! pressed (while no text field has the keyboard).

use std::fmt;

/// A key of the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    /// A letter (upper case) or a digit.
    Char(char),
    /// Enter.
    Enter,
    /// Escape.
    Escape,
    /// Delete.
    Delete,
    /// The space bar.
    Space,
    /// Home.
    Home,
}

/// A key with the modifiers held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hotkey {
    /// The key.
    pub key: Key,
    /// With Ctrl (Cmd on a Mac).
    pub ctrl: bool,
    /// With Shift.
    pub shift: bool,
}

impl Hotkey {
    /// `key` alone.
    pub const fn named(key: Key) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
        }
    }

    /// Shift and `key`.
    pub const fn shift_named(key: Key) -> Self {
        Self {
            key,
            ctrl: false,
            shift: true,
        }
    }

    /// The letter or digit `c` alone.
    pub const fn key(c: char) -> Self {
        Self::named(Key::Char(c.to_ascii_uppercase()))
    }

    /// Ctrl and the letter or digit `c`.
    pub const fn ctrl(c: char) -> Self {
        Self {
            ctrl: true,
            ..Self::key(c)
        }
    }

    /// Shift and the letter or digit `c`.
    pub const fn shift(c: char) -> Self {
        Self {
            shift: true,
            ..Self::key(c)
        }
    }

    /// Ctrl, Shift and the letter or digit `c`.
    pub const fn ctrl_shift(c: char) -> Self {
        Self {
            shift: true,
            ..Self::ctrl(c)
        }
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        match self.key {
            Key::Char(c) => write!(f, "{c}"),
            Key::Enter => f.write_str("Enter"),
            Key::Escape => f.write_str("Esc"),
            Key::Delete => f.write_str("Del"),
            Key::Space => f.write_str("Space"),
            Key::Home => f.write_str("Home"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_read_as_pressed() {
        assert_eq!(Hotkey::key('l').to_string(), "L");
        assert_eq!(Hotkey::ctrl_shift('z').to_string(), "Ctrl+Shift+Z");
        assert_eq!(Hotkey::named(Key::Delete).to_string(), "Del");
    }
}
