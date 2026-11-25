//! A field that picks a connector: a frame on something of a placed
//! part.

use super::{MapFn, WidgetHandler, map_handler};
use crate::math::Point3;
use crate::reference::Source;

/// What was picked for a connector: something of a placed part, and
/// where on it the pointer was.
#[derive(Debug, Clone)]
pub struct Picked {
    /// The instances it is in, from the part being edited down; empty
    /// for something of that part itself.
    pub instance: Vec<String>,
    /// What it is, in the world of the last of those instances.
    pub source: Source,
    /// Where on it the pointer was, in that world's coordinates.
    pub at: Point3,
    /// How far from a vertex or an edge counts as on it, there.
    pub near: f64,
    /// The world it is in: that of the last of those instances.
    pub within: std::sync::Arc<crate::world::World>,
}

/// Which placed parts a connector may be on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    /// Any, and the part being edited itself.
    #[default]
    Any,
    /// The placed part of this name, or what it places.
    Inside(String),
    /// Anything but the placed part of this name.
    Outside(String),
}

impl Scope {
    /// Whether something of the instance at `instance` (names from the
    /// part being edited down) is in it.
    pub fn takes(&self, instance: &[String]) -> bool {
        match self {
            Scope::Any => true,
            Scope::Inside(name) => instance.first() == Some(name),
            Scope::Outside(name) => instance.first() != Some(name),
        }
    }
}

/// A field that shows a connector and picks another in the viewport.
pub struct ConnectorInput<M> {
    /// The label.
    pub label: String,
    /// What the connector is on, in words; `None` while there is none.
    pub value: Option<String>,
    /// Makes the message for a pick.
    pub on_change: WidgetHandler<Picked, M>,
    /// Which placed parts it may be on.
    pub scope: Scope,
    /// Which pair of connectors of the step it is one of (a part that
    /// is placed has a pair for each of its mates): the first is 0.
    pub pair: usize,
}

impl<M: 'static> ConnectorInput<M> {
    /// A field labelled `label`, showing `value`.
    pub fn new(
        label: impl Into<String>,
        value: Option<String>,
        on_change: impl Fn(Picked) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            value,
            on_change: Box::new(on_change),
            scope: Scope::Any,
            pair: 0,
        }
    }

    /// The same field, taking only what is in `scope`.
    #[must_use]
    pub fn within(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    /// The same field, one of the pair `pair` of the step's connectors.
    #[must_use]
    pub fn of_pair(mut self, pair: usize) -> Self {
        self.pair = pair;
        self
    }

    /// The message for picking `picked`.
    pub fn set(&self, picked: Picked) -> M {
        (self.on_change)(picked)
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> ConnectorInput<N> {
        ConnectorInput {
            label: self.label,
            value: self.value,
            on_change: map_handler(self.on_change, f),
            scope: self.scope,
            pair: self.pair,
        }
    }
}
