//! A field that picks a joint of the assembly by pointing at it.

use super::{MapFn, WidgetHandler, map_handler};
use crate::world::JointKind;

/// A field that shows a joint and picks another in the viewport: while
/// it picks, the joints it takes are drawn where they are, each to be
/// pointed at and clicked.
pub struct JointInput<M> {
    /// The caption.
    pub label: String,
    /// The joint's name; `None` while there is none.
    pub value: Option<String>,
    /// The kinds of joint it takes.
    pub kinds: Vec<JointKind>,
    /// Makes the message for a pick: the joint's name.
    pub on_change: WidgetHandler<String, M>,
}

impl<M: 'static> JointInput<M> {
    /// A field labelled `label`, showing `value`, that takes joints of
    /// `kinds`.
    pub fn new(
        label: impl Into<String>,
        value: Option<String>,
        kinds: Vec<JointKind>,
        on_change: impl Fn(String) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            value,
            kinds,
            on_change: Box::new(on_change),
        }
    }

    /// Whether it takes a joint of `kind`.
    pub fn takes(&self, kind: JointKind) -> bool {
        self.kinds.contains(&kind)
    }

    /// The message for picking the joint `name`.
    pub fn set(&self, name: String) -> M {
        (self.on_change)(name)
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> JointInput<N> {
        JointInput {
            label: self.label,
            value: self.value,
            kinds: self.kinds,
            on_change: map_handler(self.on_change, f),
        }
    }
}
