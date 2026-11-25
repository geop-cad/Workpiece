//! The [`Space`]: a handle that is all of the scene. Where a
//! [`Canvas`](super::Canvas) takes the pointer on a plane, a space takes
//! it as the ray it is, and hands it to the operation, which decides
//! what a hover, a click or a drag does to what it shows in three
//! dimensions; and it draws what the operation says: lines, dots and
//! filled triangles. A mesh is edited through one.

use std::rc::Rc;

use crate::math::Point3;
use crate::ray::{Projection, Ray};
use crate::widgets::MapFn;

use super::{CanvasEventKind, Tone};

/// How the scene is seen: what a space needs to know what is beside
/// what on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpaceView {
    /// The screen's right direction (unit).
    pub right: Point3,
    /// The screen's up direction (unit).
    pub up: Point3,
    /// The size of a pixel at the space's anchor.
    pub pixel: f64,
}

impl Default for SpaceView {
    fn default() -> Self {
        Self {
            right: Point3::X,
            up: Point3::Y,
            pixel: 0.01,
        }
    }
}

impl SpaceView {
    /// The direction the eye looks in (unit).
    pub fn forward(&self) -> Point3 {
        self.up.cross(self.right)
    }

    /// Where `p` shows on the screen, seen along rays like `ray`, in
    /// coordinates that grow rightwards and upwards: the same for
    /// every point one ray goes through. `None` for a point behind
    /// the eye.
    pub fn on_screen(&self, ray: &Ray, p: Point3) -> Option<[f64; 2]> {
        match ray.projection() {
            Projection::Orthographic { .. } => Some([p.dot(self.right), p.dot(self.up)]),
            Projection::Perspective { .. } => {
                let d = p - ray.start();
                let depth = d.dot(self.forward());
                (depth > 1e-12).then(|| [d.dot(self.right) / depth, d.dot(self.up) / depth])
            }
        }
    }

    /// The point that shows at `at` on the screen (in the coordinates
    /// of [`on_screen`](Self::on_screen)), `depth` in front of where
    /// `ray` starts.
    pub fn at_screen(&self, ray: &Ray, at: [f64; 2], depth: f64) -> Point3 {
        let across = self.right * at[0] + self.up * at[1];
        match ray.projection() {
            Projection::Orthographic { .. } => {
                let forward = self.forward();
                across + forward * (ray.start().dot(forward) + depth)
            }
            Projection::Perspective { .. } => ray.start() + (self.forward() + across) * depth,
        }
    }
}

/// A pointer event in the scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpaceEvent {
    /// What happened.
    pub kind: CanvasEventKind,
    /// The ray of the pointer.
    pub ray: Ray,
    /// Whether Ctrl or Shift is held: add to what is selected.
    pub add: bool,
    /// How the scene is seen.
    pub view: SpaceView,
}

/// Something a space draws. Widths and sizes are in pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum SpaceShape {
    /// A line.
    Line {
        /// Its ends.
        ends: [Point3; 2],
        /// Its color.
        tone: Tone,
        /// Its width.
        width: f32,
        /// How opaque it is: fainter for what lies behind.
        opacity: f32,
        /// Dashed (previews).
        dashed: bool,
    },
    /// A round dot.
    Dot {
        /// Its center.
        at: Point3,
        /// Its color.
        tone: Tone,
        /// Its diameter.
        size: f32,
        /// How opaque it is.
        opacity: f32,
    },
    /// A filled triangle, see-through.
    Triangle {
        /// Its corners.
        corners: [Point3; 3],
        /// Its color.
        tone: Tone,
        /// How opaque it is.
        opacity: f32,
    },
}

/// What a space draws, for a view.
pub type SpaceDraw = Rc<dyn Fn(&SpaceView) -> Vec<SpaceShape>>;

/// The handler of a space's events.
pub type SpaceHandler<M> = Rc<dyn Fn(SpaceEvent) -> Option<M>>;

/// All of the scene, taking every pointer event that no other handle
/// takes, and drawing what its operation says.
pub struct Space<M> {
    /// A point near what is drawn: where the size of a pixel is
    /// measured.
    pub anchor: Point3,
    /// What it draws, for a view.
    pub draw: SpaceDraw,
    /// The message for an event, if it changes anything.
    pub on_event: SpaceHandler<M>,
}

impl<M: 'static> Space<M> {
    /// A space drawing `draw` and answering events with `on_event`.
    pub fn new(
        anchor: Point3,
        draw: impl Fn(&SpaceView) -> Vec<SpaceShape> + 'static,
        on_event: impl Fn(SpaceEvent) -> Option<M> + 'static,
    ) -> Self {
        Self {
            anchor,
            draw: Rc::new(draw),
            on_event: Rc::new(on_event),
        }
    }

    /// The message for `event`.
    pub fn event(&self, event: SpaceEvent) -> Option<M> {
        (self.on_event)(event)
    }

    /// What it draws in `view`.
    pub fn shapes(&self, view: &SpaceView) -> Vec<SpaceShape> {
        (self.draw)(view)
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Space<N> {
        let (on_event, f) = (self.on_event, f.clone());
        Space {
            anchor: self.anchor,
            draw: self.draw,
            on_event: Rc::new(move |e| on_event(e).map(|m| f(m))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_on_one_ray_show_at_one_place() {
        let view = SpaceView::default();
        let eye = Point3::new(0.0, 0.0, 10.0);
        let projection = Projection::Perspective { slope: 0.01 };
        let ray = Ray::new(eye, Point3::new(0.1, 0.2, -1.0), projection).unwrap();
        let near = view.on_screen(&ray, ray.at(1.0)).unwrap();
        let far = view.on_screen(&ray, ray.at(7.0)).unwrap();
        assert!((near[0] - far[0]).abs() < 1e-12 && (near[1] - far[1]).abs() < 1e-12);
        assert!(view.on_screen(&ray, eye + Point3::Z).is_none());
        assert_eq!(view.forward(), -Point3::Z);
        let back = view.at_screen(&ray, near, 3.0);
        let again = view.on_screen(&ray, back).unwrap();
        assert!((near[0] - again[0]).abs() < 1e-12 && (near[1] - again[1]).abs() < 1e-12);
    }
}
