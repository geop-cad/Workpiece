use super::{MapFn, WidgetHandler, map_handler};
use crate::hotkey::Hotkey;

/// A tool: an icon button with a name below, for ribbons and context
/// menus. It can be *active* (the tool in use, a toggle that is on) and
/// can be unavailable, saying why.
pub struct ToolButton<M> {
    /// The icon's name, e.g. `"line"`; the host draws it.
    pub icon: &'static str,
    /// The caption.
    pub label: String,
    /// What it does, in a sentence.
    pub hint: String,
    /// The keys that press it.
    pub hotkey: Hotkey,
    /// Whether it shows as in use.
    pub active: bool,
    /// Why it cannot be pressed now; `None` when it can.
    pub disabled: Option<String>,
    /// Whether a ribbon shows it full-sized, its icon above its name,
    /// rather than as a small icon among others.
    pub large: bool,
    /// The message a click produces.
    pub on_click: WidgetHandler<(), Option<M>>,
}

impl<M: 'static> ToolButton<M> {
    /// A tool whose click produces `on_click()`.
    pub fn new(
        icon: &'static str,
        label: impl Into<String>,
        hotkey: Hotkey,
        on_click: impl Fn() -> M + 'static,
    ) -> Self {
        Self {
            icon,
            label: label.into(),
            hint: String::new(),
            hotkey,
            active: false,
            disabled: None,
            large: false,
            on_click: Box::new(move |()| Some(on_click())),
        }
    }

    /// The same tool, saying what it does.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// The same tool, shown as in use if `active`.
    #[must_use]
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// The same tool, unavailable for `reason` if there is one.
    #[must_use]
    pub fn disabled(mut self, reason: Option<String>) -> Self {
        self.disabled = reason;
        self
    }

    /// The same tool, shown full-sized in a ribbon.
    #[must_use]
    pub fn large(mut self) -> Self {
        self.large = true;
        self
    }

    /// Whether it can be pressed.
    pub fn enabled(&self) -> bool {
        self.disabled.is_none()
    }

    /// Clicks the tool; no message while it is unavailable.
    pub fn click(&self) -> Option<M> {
        self.enabled().then(|| (self.on_click)(())).flatten()
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> ToolButton<N> {
        let (on_click, f) = (self.on_click, f.clone());
        ToolButton {
            icon: self.icon,
            label: self.label,
            hint: self.hint,
            hotkey: self.hotkey,
            active: self.active,
            disabled: self.disabled,
            large: self.large,
            on_click: Box::new(move |()| on_click(()).map(|m| f(m))),
        }
    }
}

/// A labelled group of widgets: a section of a ribbon.
pub struct Group<M> {
    /// The caption.
    pub label: String,
    /// The children, in layout order.
    pub children: Vec<super::Widget<M>>,
}

impl<M: 'static> Group<M> {
    /// A group of `children` called `label`.
    pub fn new(label: impl Into<String>, children: Vec<super::Widget<M>>) -> Self {
        Self {
            label: label.into(),
            children,
        }
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Group<N> {
        Group {
            label: self.label,
            children: self.children.into_iter().map(|w| w.map_rc(f)).collect(),
        }
    }
}

/// How a [`Label`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mood {
    /// Plain.
    #[default]
    Plain,
    /// All is well (fully constrained).
    Good,
    /// Something needs attention.
    Warning,
    /// Something is wrong.
    Error,
}

/// A line of text: a status or a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The caption above the text; may be empty.
    pub label: String,
    /// The text.
    pub text: String,
    /// How it reads.
    pub mood: Mood,
}

impl Label {
    /// The text `text`, captioned `label`.
    pub fn new(label: impl Into<String>, text: impl Into<String>, mood: Mood) -> Self {
        Self {
            label: label.into(),
            text: text.into(),
            mood,
        }
    }
}

/// A text field.
pub struct TextInput<M> {
    /// The caption.
    pub label: String,
    /// The small icon beside the caption, by name.
    pub icon: Option<&'static str>,
    /// The current text.
    pub value: String,
    /// Whether it takes several lines: Enter breaks the line.
    pub multiline: bool,
    /// The message for entered text.
    pub on_change: WidgetHandler<String, M>,
}

impl<M: 'static> TextInput<M> {
    /// A field showing `value`; entering `s` produces `on_change(s)`.
    pub fn new(
        label: impl Into<String>,
        value: impl Into<String>,
        on_change: impl Fn(String) -> M + 'static,
    ) -> Self {
        let label = label.into();
        Self {
            icon: super::label_icon(&label),
            label,
            value: value.into(),
            multiline: false,
            on_change: Box::new(on_change),
        }
    }

    /// The same field, of several lines.
    #[must_use]
    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// Enters `text`.
    pub fn set(&self, text: impl Into<String>) -> M {
        (self.on_change)(text.into())
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> TextInput<N> {
        TextInput {
            label: self.label,
            icon: self.icon,
            value: self.value,
            multiline: self.multiline,
            on_change: map_handler(self.on_change, f),
        }
    }
}
