use super::{MapFn, WidgetHandler, map_handler};
use crate::reference::{ReferenceKind, Source};
use crate::world::World;

/// A box of references of several kinds, picked one after another:
/// picking one adds it, picking one already there removes it.
pub struct ReferenceList<M> {
    /// The caption.
    pub label: String,
    /// What it takes: a reference of any of these kinds.
    pub kinds: Vec<ReferenceKind>,
    /// The references, in the order picked.
    pub values: Vec<Source>,
    /// Whether it has to hold something: empty, it is what is picked
    /// next.
    pub required: bool,
    /// The message for the edited references.
    pub on_change: WidgetHandler<Vec<Source>, M>,
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
            kinds,
            values,
            required: false,
            on_change: Box::new(on_change),
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

    /// The box, as one that has to hold something.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
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
            kinds: self.kinds,
            values: self.values,
            required: self.required,
            on_change: map_handler(self.on_change, f),
        }
    }
}
