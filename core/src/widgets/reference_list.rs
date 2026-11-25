use super::{MapFn, WidgetHandler, map_handler};
use crate::reference::{ReferenceKind, Source};
use crate::world::World;

/// A box of references of several kinds, picked one after another:
/// picking one adds it, picking one already there removes it.
pub struct ReferenceList<M> {
    /// The caption.
    pub label: String,
    /// The small icon beside the caption, by name: that of the first
    /// kind it takes ([`super::kind_icon`]) unless set.
    pub icon: Option<&'static str>,
    /// What it takes: a reference of any of these kinds.
    pub kinds: Vec<ReferenceKind>,
    /// The references, in the order picked.
    pub values: Vec<Source>,
    /// Whether it has to hold something: empty, it is what is picked
    /// next.
    pub required: bool,
    /// The message for the edited references.
    pub on_change: WidgetHandler<Vec<Source>, M>,
    /// For a box that also wants to know where on what is picked the
    /// pointer was (a curve through points on a face): the message for
    /// a pick there, in place of adding or removing it.
    pub on_pick_at: Option<WidgetHandler<(Source, crate::math::Point3), M>>,
}

impl<M: 'static> ReferenceList<M> {
    /// A box taking references of `kinds`, holding `values`.
    pub fn new(
        label: impl Into<String>,
        kinds: Vec<ReferenceKind>,
        values: Vec<Source>,
        on_change: impl Fn(Vec<Source>) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            icon: kinds.first().map(|k| super::kind_icon(*k)),
            kinds,
            values,
            required: false,
            on_change: Box::new(on_change),
            on_pick_at: None,
        }
    }

    /// A box taking references to `V`, holding `values`.
    pub fn of<V: crate::reference::Refers + 'static>(
        label: impl Into<String>,
        values: &[crate::Reference<V>],
        on_change: impl Fn(Vec<crate::Reference<V>>) -> M + 'static,
    ) -> Self {
        let sources = crate::Reference::sources(values);
        Self::new(label, V::KINDS.to_vec(), sources, move |all| {
            on_change(crate::Reference::each(all))
        })
    }

    /// The same box, showing the icon `icon` (by name; none for `None`)
    /// beside its caption.
    #[must_use]
    pub fn with_icon(mut self, icon: impl Into<Option<&'static str>>) -> Self {
        self.icon = icon.into();
        self
    }

    /// The box, as one that has to hold something.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// The same box, told where on what is picked the pointer was:
    /// picking `s` at `p` produces `on_pick_at(s, p)`.
    #[must_use]
    pub fn picked_at(
        mut self,
        on_pick_at: impl Fn(Source, crate::math::Point3) -> M + 'static,
    ) -> Self {
        self.on_pick_at = Some(Box::new(move |(source, at)| on_pick_at(source, at)));
        self
    }

    /// Picks `source`, the pointer at `at` on it (where that is known):
    /// as [`toggle`](Self::toggle), for a box that does not ask where.
    pub fn pick_at(&self, source: Source, at: Option<crate::math::Point3>) -> M {
        match (&self.on_pick_at, at) {
            (Some(on_pick_at), Some(at)) => on_pick_at((source, at)),
            _ => self.toggle(source),
        }
    }

    /// Whether `source` is of a kind the box takes, in `world`.
    pub fn accepts(&self, world: &World, source: &Source) -> bool {
        self.kinds.iter().any(|k| k.accepts(world, source))
    }

    /// Adds `source`, or removes it if it is there.
    pub fn toggle(&self, source: Source) -> M {
        let mut values = self.values.clone();
        match values.iter().position(|v| *v == source) {
            Some(i) => {
                values.remove(i);
            }
            None => values.push(source),
        }
        (self.on_change)(values)
    }

    /// Removes the reference at `i`.
    pub fn remove(&self, i: usize) -> M {
        let mut values = self.values.clone();
        if i < values.len() {
            values.remove(i);
        }
        (self.on_change)(values)
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> ReferenceList<N> {
        ReferenceList {
            label: self.label,
            icon: self.icon,
            kinds: self.kinds,
            values: self.values,
            required: self.required,
            on_change: map_handler(self.on_change, f),
            on_pick_at: self.on_pick_at.map(|h| map_handler(h, f)),
        }
    }
}
