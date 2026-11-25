use super::{MapFn, WidgetHandler, map_handler};
use crate::hotkey::Hotkey;

/// A check mark: a property that is on or off, such as whether a
/// dimension is driven.
pub struct Check<M> {
    /// The caption beside the mark.
    pub label: String,
    /// What it means, in a sentence.
    pub hint: String,
    /// Whether it is on.
    pub value: bool,
    /// The keys that switch it, if any.
    pub hotkey: Option<Hotkey>,
    /// Why it cannot be switched now; `None` when it can.
    pub disabled: Option<String>,
    /// The message for switching it to the value given.
    pub on_change: WidgetHandler<bool, M>,
}

impl<M: 'static> Check<M> {
    /// A check mark showing `value`; switching it to `v` produces
    /// `on_change(v)`.
    pub fn new(
        label: impl Into<String>,
        value: bool,
        on_change: impl Fn(bool) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            hint: String::new(),
            value,
            hotkey: None,
            disabled: None,
            on_change: Box::new(on_change),
        }
    }

    /// The same check mark, saying what it means.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// The same check mark, switched by `hotkey` too.
    #[must_use]
    pub fn with_hotkey(mut self, hotkey: Hotkey) -> Self {
        self.hotkey = Some(hotkey);
        self
    }

    /// The same check mark, unavailable for `reason` if there is one.
    #[must_use]
    pub fn disabled(mut self, reason: Option<String>) -> Self {
        self.disabled = reason;
        self
    }

    /// Switches it over; no message while it is unavailable.
    pub fn toggle(&self) -> Option<M> {
        self.disabled
            .is_none()
            .then(|| (self.on_change)(!self.value))
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Check<N> {
        Check {
            label: self.label,
            hint: self.hint,
            value: self.value,
            hotkey: self.hotkey,
            disabled: self.disabled,
            on_change: map_handler(self.on_change, f),
        }
    }
}
