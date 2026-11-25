use super::{MapFn, WidgetHandler, map_handler};

/// How a choice shows its options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shown {
    /// As suits how many there are: a few side by side, many as a
    /// drop-down list.
    #[default]
    Auto,
    /// As a drop-down list, however few.
    DropDown,
    /// As a list that is searched by typing: for a catalogue.
    Searched,
}

/// A choice of one of several options, some possibly unavailable.
pub struct Choice<M> {
    /// How the options show.
    pub shown: Shown,
    /// The caption.
    pub label: String,
    /// The small icon beside the caption, by name: what the caption's
    /// words say ([`super::label_icon`]) unless set.
    pub icon: Option<&'static str>,
    /// The options' labels.
    pub options: Vec<String>,
    /// The options' icons, by name, where they have one: a choice whose
    /// options all have one shows them as buttons with their pictures,
    /// a few side by side; in a drop-down list they stand before the
    /// labels.
    pub icons: Vec<Option<&'static str>>,
    /// Which options can be chosen now; the others show greyed out.
    pub enabled: Vec<bool>,
    /// Why each option that cannot be chosen cannot (empty for the ones
    /// that can, or when there is nothing to say).
    pub reasons: Vec<String>,
    /// The chosen option.
    pub selected: usize,
    /// The message for choosing option `i`.
    pub on_change: WidgetHandler<usize, M>,
}

impl<M: 'static> Choice<M> {
    /// A choice of `options` with `selected` chosen; choosing `i` produces
    /// `on_change(i)`.
    pub fn new(
        label: impl Into<String>,
        options: Vec<String>,
        selected: usize,
        on_change: impl Fn(usize) -> M + 'static,
    ) -> Self {
        let label = label.into();
        Self {
            shown: Shown::Auto,
            icon: super::label_icon(&label),
            label,
            icons: vec![None; options.len()],
            enabled: vec![true; options.len()],
            reasons: vec![String::new(); options.len()],
            options,
            selected,
            on_change: Box::new(on_change),
        }
    }

    /// The same choice, showing the icon `icon` (by name; none for
    /// `None`) beside its caption.
    #[must_use]
    pub fn with_icon(mut self, icon: impl Into<Option<&'static str>>) -> Self {
        self.icon = icon.into();
        self
    }

    /// The same choice, its options shown with the icons `icons` (by
    /// name; in the order of the options).
    #[must_use]
    pub fn with_icons(mut self, icons: Vec<Option<&'static str>>) -> Self {
        self.icons = icons;
        self.icons.resize(self.options.len(), None);
        self
    }

    /// Whether every option has an icon: then they show as pictures.
    pub fn pictured(&self) -> bool {
        !self.icons.is_empty() && self.icons.iter().all(Option::is_some)
    }

    /// The same choice, as a drop-down list.
    #[must_use]
    pub fn drop_down(mut self) -> Self {
        self.shown = Shown::DropDown;
        self
    }

    /// The same choice, as a list that is searched by typing.
    #[must_use]
    pub fn searched(mut self) -> Self {
        self.shown = Shown::Searched;
        self
    }

    /// The same choice with only the options `enabled` takes available.
    pub fn with_enabled(mut self, enabled: Vec<bool>) -> Self {
        self.enabled = enabled;
        self
    }

    /// The same choice, saying for each unavailable option why it is.
    #[must_use]
    pub fn with_reasons(mut self, reasons: Vec<String>) -> Self {
        self.reasons = reasons;
        self
    }

    /// Why option `i` cannot be chosen, if it cannot and there is a
    /// reason.
    pub fn reason(&self, i: usize) -> Option<&str> {
        let enabled = self.enabled.get(i).copied().unwrap_or(false);
        self.reasons
            .get(i)
            .map(String::as_str)
            .filter(|r| !enabled && !r.is_empty())
    }

    /// Chooses option `i`; no message for an option that does not exist
    /// or is unavailable.
    pub fn set(&self, i: usize) -> Option<M> {
        self.enabled
            .get(i)
            .copied()
            .unwrap_or(false)
            .then(|| (self.on_change)(i))
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Choice<N> {
        Choice {
            shown: self.shown,
            label: self.label,
            icon: self.icon,
            options: self.options,
            icons: self.icons,
            enabled: self.enabled,
            reasons: self.reasons,
            selected: self.selected,
            on_change: map_handler(self.on_change, f),
        }
    }
}
