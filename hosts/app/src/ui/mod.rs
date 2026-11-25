//! The window's areas, each drawing its part of the view and collecting
//! the messages the user's input produces.

pub mod edit_dialog;
pub mod file_dialog;
pub mod materials;
pub mod overview;
pub mod picture;
pub mod previews;
pub mod properties;
pub mod ribbon;
pub mod sheet;
pub mod table;
pub mod templates;
pub mod timeline;
pub mod viewport;

use eframe::egui;
use workpiece_core::{Hotkey, Key};

/// What shows when the pointer rests on a button: its name with its
/// hotkey, and what it does.
pub fn tip(label: &str, hotkey: Option<Hotkey>, hint: &str) -> String {
    let mut text = match hotkey {
        Some(hotkey) => format!("{label}  ({hotkey})"),
        None => label.to_owned(),
    };
    if !hint.is_empty() {
        text += "\n";
        text += hint;
    }
    text
}

/// Whether `hotkey` was pressed just now; the press is used up.
pub fn pressed(input: &mut egui::InputState, hotkey: Hotkey) -> bool {
    let key = match hotkey.key {
        Key::Char(c) => egui::Key::from_name(&c.to_string()),
        Key::Enter => Some(egui::Key::Enter),
        Key::Escape => Some(egui::Key::Escape),
        Key::Delete => Some(egui::Key::Delete),
        Key::Space => Some(egui::Key::Space),
        Key::Home => Some(egui::Key::Home),
    };
    let mut modifiers = egui::Modifiers::NONE;
    if hotkey.ctrl {
        modifiers |= egui::Modifiers::COMMAND;
    }
    if hotkey.shift {
        modifiers |= egui::Modifiers::SHIFT;
    }
    key.is_some_and(|key| input.consume_key(modifiers, key))
}
