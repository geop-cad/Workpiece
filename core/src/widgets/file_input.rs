use super::{MapFn, WidgetHandler, map_handler};

/// A field the user fills by choosing a file (in a file dialog), by its
/// path relative to the working directory.
pub struct FileInput<M> {
    /// The caption.
    pub label: String,
    /// The current path.
    pub value: String,
    /// The extension the files must have, without the dot.
    pub extension: String,
    /// The message for a chosen path.
    pub on_change: WidgetHandler<String, M>,
}

impl<M: 'static> FileInput<M> {
    /// A field showing `value` that takes files ending in `.extension`;
    /// choosing `path` produces `on_change(path)`.
    pub fn new(
        label: impl Into<String>,
        value: impl Into<String>,
        extension: impl Into<String>,
        on_change: impl Fn(String) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            extension: extension.into(),
            on_change: Box::new(on_change),
        }
    }

    /// Chooses `path`.
    pub fn set(&self, path: impl Into<String>) -> M {
        (self.on_change)(path.into())
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> FileInput<N> {
        FileInput {
            label: self.label,
            value: self.value,
            extension: self.extension,
            on_change: map_handler(self.on_change, f),
        }
    }
}
