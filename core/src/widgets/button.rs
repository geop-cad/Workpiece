use super::{MapFn, WidgetHandler, map_handler};
use crate::hotkey::Hotkey;

/// A push button.
pub struct Button<M> {
    /// The caption.
    pub label: String,
    /// The keys that press it.
    pub hotkey: Option<Hotkey>,
    /// The message a click produces.
    pub on_click: WidgetHandler<(), M>,
}

impl<M: 'static> Button<M> {
    /// A button whose click produces `on_click()`.
    pub fn new(label: impl Into<String>, on_click: impl Fn() -> M + 'static) -> Self {
        Self {
            label: label.into(),
            hotkey: None,
            on_click: Box::new(move |()| on_click()),
        }
    }

    /// The same button, pressed by `hotkey`.
    #[must_use]
    pub fn with_hotkey(mut self, hotkey: Hotkey) -> Self {
        self.hotkey = Some(hotkey);
        self
    }

    /// Clicks the button.
    pub fn click(&self) -> M {
        (self.on_click)(())
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Button<N> {
        Button {
            label: self.label,
            hotkey: self.hotkey,
            on_click: map_handler(self.on_click, f),
        }
    }
}
