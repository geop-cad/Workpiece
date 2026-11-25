use super::{MapFn, WidgetHandler, map_handler};
use crate::reference::{Reference, ReferenceKind, Source};
use crate::world::World;

/// A field the user fills by picking something in the world that resolves
/// to the field's reference type: one built for `Reference<DatumPlane>`
/// only takes planes. The type is fixed by [`new`](Self::new); the field
/// itself holds plain [`Source`]s, so every reference field has the same
/// widget type.
pub struct ReferenceInput<M> {
    /// The caption.
    pub label: String,
    /// What the field takes.
    pub kind: ReferenceKind,
    /// The current source, if one is set.
    pub value: Option<Source>,
    /// The message for a picked source; [`accepts`](Self::accepts) it
    /// first.
    pub on_change: WidgetHandler<Source, M>,
    /// For a field that may stay empty: what it shows while it is (what
    /// nothing picked means, say "All bodies"), and the message that
    /// empties it. A field without is one the user has to fill.
    pub optional: Option<(String, WidgetHandler<(), M>)>,
}

impl<M: 'static> ReferenceInput<M> {
    /// A field for references to `V`, showing `value`; picking `r`
    /// produces `on_change(r)`.
    pub fn new<V: crate::reference::Refers + 'static>(
        label: impl Into<String>,
        value: Option<Reference<V>>,
        on_change: impl Fn(Reference<V>) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            kind: V::KINDS.first().copied().unwrap_or(ReferenceKind::Body),
            value: value.map(|r| r.source),
            on_change: Box::new(move |source| on_change(Reference::new(source))),
            optional: None,
        }
    }

    /// A field for references of `kind`, showing `value`; picking `s`
    /// produces `on_change(s)`. For fields whose kind is chosen at run
    /// time (a construction's slots).
    pub fn of_kind(
        label: impl Into<String>,
        kind: ReferenceKind,
        value: Option<Source>,
        on_change: impl Fn(Source) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            kind,
            value,
            on_change: Box::new(on_change),
            optional: None,
        }
    }

    /// The same field, free to stay empty: it then shows `empty`, and
    /// emptying it produces `on_clear()`.
    #[must_use]
    pub fn optional(
        mut self,
        empty: impl Into<String>,
        on_clear: impl Fn() -> M + 'static,
    ) -> Self {
        self.optional = Some((empty.into(), Box::new(move |()| on_clear())));
        self
    }

    /// Empties the field, if it may be empty and is not.
    pub fn clear(&self) -> Option<M> {
        let (_, on_clear) = self.optional.as_ref()?;
        self.value.is_some().then(|| on_clear(()))
    }

    /// Whether `source` resolves to the field's type in `world`; the
    /// picker filters candidates with it.
    pub fn accepts(&self, world: &World, source: &Source) -> bool {
        self.kind.accepts(world, source)
    }

    /// Picks `source`. Callers check [`accepts`](Self::accepts) first; the
    /// operation reports a source that does not resolve.
    pub fn set(&self, source: Source) -> M {
        (self.on_change)(source)
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> ReferenceInput<N> {
        ReferenceInput {
            label: self.label,
            kind: self.kind,
            value: self.value,
            on_change: map_handler(self.on_change, f),
            optional: self
                .optional
                .map(|(empty, on_clear)| (empty, map_handler(on_clear, f))),
        }
    }
}
