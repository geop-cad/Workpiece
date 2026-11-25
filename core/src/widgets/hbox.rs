use super::{MapFn, Widget};

/// Children laid out side by side.
pub struct HBox<M> {
    /// The children, in layout order.
    pub children: Vec<Widget<M>>,
}

impl<M: 'static> HBox<M> {
    /// A box of `children`.
    pub fn new(children: Vec<Widget<M>>) -> Self {
        Self { children }
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> HBox<N> {
        HBox {
            children: self.children.into_iter().map(|w| w.map_rc(f)).collect(),
        }
    }
}
