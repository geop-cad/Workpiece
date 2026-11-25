//! 3D handles: widgets the user drags in the viewport.
//!
//! A handle turns a drag of pick [`crate::Ray`]s into a new value itself
//! (picking which handle is grabbed happens elsewhere); its `on_drag`
//! [`WidgetHandler`](crate::WidgetHandler) turns the result (a float for [`LinearHandle`], a
//! [`crate::Pose`] for [`Gizmo`]) into a message, as widgets do.
//!
//! A [`Canvas`] is different: it hands every pointer event on its plane to
//! its operation, which implements hovering, clicking and dragging itself.
//!
//! A drag step takes the pick ray at the previous pointer position (`from`)
//! and at the current one (`to`). The UI rebuilds handles from the
//! arguments after every step, so each step moves the value by the change
//! between the two.

mod angular;
pub mod canvas;
mod gizmo;
mod linear;
mod planar;
pub mod shape;
pub mod space;

use std::rc::Rc;

pub use angular::AngularHandle;
pub use canvas::{
    Canvas, CanvasDraw, CanvasEnter, CanvasEvent, CanvasEventKind, CanvasHandler, CanvasInput,
    CanvasShape, CanvasView, HEAD_LENGTH, HEAD_WIDTH, Tone,
};
pub use gizmo::{Gizmo, GizmoPart};
pub use linear::LinearHandle;
pub use planar::PlanarHandle;
pub use space::{Space, SpaceDraw, SpaceEvent, SpaceHandler, SpaceShape, SpaceView};

use crate::widgets::MapFn;

/// Any handle, producing messages of type `M`.
pub enum Handle<M> {
    /// An arrow dragged along a line.
    Linear(LinearHandle<M>),
    /// A translate/rotate gizmo.
    Gizmo(Gizmo<M>),
    /// A knob moved over a plane.
    Planar(PlanarHandle<M>),
    /// A knob turned about an axis.
    Angular(AngularHandle<M>),
    /// A plane that takes every pointer event on it.
    Canvas(Canvas<M>),
    /// All of the scene, taking every pointer event as its ray.
    Space(Space<M>),
}

impl<M: 'static> Handle<M> {
    /// The same handle, its messages converted by `f`.
    pub fn map<N: 'static>(self, f: impl Fn(M) -> N + 'static) -> Handle<N> {
        self.map_rc(&(Rc::new(f) as MapFn<M, N>))
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Handle<N> {
        match self {
            Handle::Linear(h) => Handle::Linear(h.map_rc(f)),
            Handle::Gizmo(h) => Handle::Gizmo(h.map_rc(f)),
            Handle::Planar(h) => Handle::Planar(h.map_rc(f)),
            Handle::Angular(h) => Handle::Angular(h.map_rc(f)),
            Handle::Canvas(h) => Handle::Canvas(h.map_rc(f)),
            Handle::Space(h) => Handle::Space(h.map_rc(f)),
        }
    }
}

impl<M> From<LinearHandle<M>> for Handle<M> {
    fn from(h: LinearHandle<M>) -> Self {
        Handle::Linear(h)
    }
}

impl<M> From<AngularHandle<M>> for Handle<M> {
    fn from(h: AngularHandle<M>) -> Self {
        Handle::Angular(h)
    }
}

impl<M> From<PlanarHandle<M>> for Handle<M> {
    fn from(h: PlanarHandle<M>) -> Self {
        Handle::Planar(h)
    }
}

impl<M> From<Gizmo<M>> for Handle<M> {
    fn from(h: Gizmo<M>) -> Self {
        Handle::Gizmo(h)
    }
}

impl<M> From<Canvas<M>> for Handle<M> {
    fn from(h: Canvas<M>) -> Self {
        Handle::Canvas(h)
    }
}

impl<M> From<Space<M>> for Handle<M> {
    fn from(h: Space<M>) -> Self {
        Handle::Space(h)
    }
}
