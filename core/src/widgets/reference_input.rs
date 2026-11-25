use super::{MapFn, WidgetHandler, map_handler};
use crate::math::Point3;
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
    /// The small icon beside the caption, by name: that of the kind it
    /// takes ([`super::kind_icon`]) unless set.
    pub icon: Option<&'static str>,
    /// What the field takes.
    pub kind: ReferenceKind,
    /// What else it takes, of a field for references of several kinds
    /// (a plane or a straight edge).
    pub also: Vec<ReferenceKind>,
    /// The current source, if one is set.
    pub value: Option<Source>,
    /// The message for a picked source; [`accepts`](Self::accepts) it
    /// first.
    pub on_change: WidgetHandler<Source, M>,
    /// For a field that may stay empty: what it shows while it is (what
    /// nothing picked means, say "All bodies"), and the message that
    /// empties it. A field without is one the user has to fill.
    pub optional: Option<(String, WidgetHandler<(), M>)>,
    /// For a field that also wants to know where on what is picked the
    /// pointer was (a hole goes where its face is clicked): the message
    /// for a pick and that point, in place of `on_change`'s.
    pub on_pick_at: Option<WidgetHandler<(Source, Point3), M>>,
}

impl<M: 'static> ReferenceInput<M> {
    /// A field for references to `V`, showing `value`; picking `r`
    /// produces `on_change(r)`.
    pub fn new<V: crate::reference::Refers + 'static>(
        label: impl Into<String>,
        value: Option<Reference<V>>,
        on_change: impl Fn(Reference<V>) -> M + 'static,
    ) -> Self {
        let kind = V::KINDS.first().copied().unwrap_or(ReferenceKind::Body);
        Self {
            label: label.into(),
            icon: Some(super::kind_icon(kind)),
            kind,
            also: V::KINDS.iter().skip(1).copied().collect(),
            value: value.map(|r| r.source),
            on_change: Box::new(move |source| on_change(Reference::new(source))),
            optional: None,
            on_pick_at: None,
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
            icon: Some(super::kind_icon(kind)),
            kind,
            also: Vec::new(),
            value,
            on_change: Box::new(on_change),
            optional: None,
            on_pick_at: None,
        }
    }

    /// The same field, taking references of `kinds` (the first is what
    /// it says it takes).
    #[must_use]
    pub fn taking(mut self, kinds: &[ReferenceKind]) -> Self {
        if let Some((first, rest)) = kinds.split_first() {
            self.kind = *first;
            self.also = rest.to_vec();
        }
        self
    }

    /// Every kind it takes.
    pub fn kinds(&self) -> Vec<ReferenceKind> {
        std::iter::once(self.kind)
            .chain(self.also.iter().copied())
            .collect()
    }

    /// The same field, showing the icon `icon` (by name; none for
    /// `None`) beside its caption.
    #[must_use]
    pub fn with_icon(mut self, icon: impl Into<Option<&'static str>>) -> Self {
        self.icon = icon.into();
        self
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

    /// The same field, told where on what is picked the pointer was:
    /// picking `s` at `p` produces `on_pick_at(s, p)`.
    #[must_use]
    pub fn picked_at(mut self, on_pick_at: impl Fn(Source, Point3) -> M + 'static) -> Self {
        self.on_pick_at = Some(Box::new(move |(source, at)| on_pick_at(source, at)));
        self
    }

    /// Picks `source`, the pointer at `at` on it (where that is known):
    /// as [`set`](Self::set), for a field that does not ask where.
    pub fn set_at(&self, source: Source, at: Option<Point3>) -> M {
        match (&self.on_pick_at, at) {
            (Some(on_pick_at), Some(at)) => on_pick_at((source, at)),
            _ => self.set(source),
        }
    }

    /// Empties the field, if it may be empty and is not.
    pub fn clear(&self) -> Option<M> {
        let (_, on_clear) = self.optional.as_ref()?;
        self.value.is_some().then(|| on_clear(()))
    }

    /// Whether `source` resolves to the field's type in `world`; the
    /// picker filters candidates with it.
    pub fn accepts(&self, world: &World, source: &Source) -> bool {
        self.kinds().iter().any(|k| k.accepts(world, source))
    }

    /// Picks `source`. Callers check [`accepts`](Self::accepts) first; the
    /// operation reports a source that does not resolve.
    pub fn set(&self, source: Source) -> M {
        (self.on_change)(source)
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> ReferenceInput<N> {
        ReferenceInput {
            label: self.label,
            icon: self.icon,
            kind: self.kind,
            also: self.also,
            value: self.value,
            on_change: map_handler(self.on_change, f),
            optional: self
                .optional
                .map(|(empty, on_clear)| (empty, map_handler(on_clear, f))),
            on_pick_at: self.on_pick_at.map(|h| map_handler(h, f)),
        }
    }
}
