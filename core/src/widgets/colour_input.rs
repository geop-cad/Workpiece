use super::{MapFn, WidgetHandler, map_handler};

/// A field that picks a colour, or none.
pub struct ColourInput<M> {
    /// The caption.
    pub label: String,
    /// The colour: red, green and blue, as a screen has them. None:
    /// no colour is given.
    pub value: Option<[u8; 3]>,
    /// The message for a picked colour, or for giving none.
    pub on_change: WidgetHandler<Option<[u8; 3]>, M>,
}

impl<M: 'static> ColourInput<M> {
    /// A field showing `value`; picking `c` produces `on_change(c)`.
    pub fn new(
        label: impl Into<String>,
        value: Option<[u8; 3]>,
        on_change: impl Fn(Option<[u8; 3]>) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            value,
            on_change: Box::new(on_change),
        }
    }

    /// Picks `colour`.
    pub fn set(&self, colour: Option<[u8; 3]>) -> M {
        (self.on_change)(colour)
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> ColourInput<N> {
        ColourInput {
            label: self.label,
            value: self.value,
            on_change: map_handler(self.on_change, f),
        }
    }
}
