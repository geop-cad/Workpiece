//! The [`Canvas`]: a handle that is a whole plane. It takes every pointer
//! event on the plane and hands it, as a point in the plane's
//! coordinates, to the operation, which decides what a hover, a click or a
//! drag does; and it draws what the operation says: lines, dots,
//! arrowheads and text. A sketch is edited through one.

use std::rc::Rc;

use crate::math::{Point2, Point3};
use crate::ray::Ray;
use crate::widgets::MapFn;
use crate::world::DatumPlane;

/// How the plane is seen: what the canvas needs to size things in pixels
/// and to know which way is horizontal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasView {
    /// The size of a pixel, in the plane's units.
    pub pixel: f64,
    /// The screen's right direction in the plane's coordinates (unit).
    pub right: Point2,
    /// The screen's up direction in the plane's coordinates (unit): a
    /// quarter turn on from `right` for a plane seen from its front,
    /// back from it for one seen from behind.
    pub up: Point2,
    /// About where the middle of the view is, in the plane's
    /// coordinates: what is drawn without end is drawn across the view
    /// from there.
    pub middle: Point2,
}

impl CanvasView {
    /// The view of a plane with directions `x` and `y` (unit, square to
    /// each other) from a camera whose right and up are `right` and
    /// `up` (and where a pixel of the plane is `pixel` long), looking at
    /// `middle`.
    ///
    /// The screen's right and up are not simply what `right` and `up`
    /// drop to on the plane: seen edge-on, one of them has no length
    /// there (and the other all of it), and what is left of a vector
    /// that is nearly square to the plane turns with every rounding. The
    /// longer of the two is the one that is kept, and the other one is
    /// a quarter turn from it (the way round it was, seen from the front
    /// or the back of the plane), so that the pair is a sound frame at
    /// every angle.
    pub fn through(
        x: Point3,
        y: Point3,
        right: Point3,
        up: Point3,
        pixel: f64,
        middle: Point2,
    ) -> Self {
        let (r, u) = (
            Point2::new(right.dot(x), right.dot(y)),
            Point2::new(up.dot(x), up.dot(y)),
        );
        // Seen from the front (the way the plane's x, y, z turn) or from
        // the back: the sign of the way the view turns about its normal.
        let turn = if (right.cross(up)).dot(x.cross(y)) < 0.0 {
            -1.0
        } else {
            1.0
        };
        let (right, up) = if r.length() >= u.length() {
            let r = r.try_normalize().unwrap_or(Point2::X);
            (r, r.perp() * turn)
        } else {
            let u = u.try_normalize().unwrap_or(Point2::Y);
            (u.perp() * -turn, u)
        };
        Self {
            pixel,
            right,
            up,
            middle,
        }
    }
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            pixel: 0.01,
            right: Point2::X,
            up: Point2::Y,
            middle: Point2::ZERO,
        }
    }
}

/// What the pointer did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasEventKind {
    /// It moved, no button down.
    Hover,
    /// The left button went down.
    Press,
    /// It moved with the left button down (after a press).
    Drag,
    /// The left button went up after a drag.
    Release,
    /// A left click (down and up in place).
    Click,
    /// A second click right after one.
    DoubleClick,
}

/// A pointer event on the plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasEvent {
    /// What happened.
    pub kind: CanvasEventKind,
    /// Where, in the plane's coordinates.
    pub at: Point2,
    /// Whether Ctrl or Shift is held: add to what is selected.
    pub add: bool,
    /// How the plane is seen.
    pub view: CanvasView,
}

/// What a shape is, which decides its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tone {
    /// Geometry that can still move.
    Free,
    /// Geometry that is fully constrained.
    Constrained,
    /// Construction geometry.
    Construction,
    /// Geometry projected from the model.
    Projected,
    /// Under the pointer.
    Hover,
    /// Selected.
    Selected,
    /// In conflict.
    Conflict,
    /// Dimensions.
    Dimension,
    /// What a tool is about to make.
    Preview,
    /// Constraint badges.
    Badge,
}

/// Something a canvas draws, in the plane's coordinates. Widths and sizes
/// are in pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasShape {
    /// A polyline.
    Polyline {
        /// Its points.
        points: Vec<Point2>,
        /// Its color.
        tone: Tone,
        /// Its width.
        width: f32,
        /// Dashed (construction lines, previews).
        dashed: bool,
    },
    /// A round dot.
    Dot {
        /// Its center.
        at: Point2,
        /// Its color.
        tone: Tone,
        /// Its diameter.
        size: f32,
    },
    /// A filled, slender arrowhead, as on a drawing's dimension lines.
    Arrowhead {
        /// Where it points.
        tip: Point2,
        /// The unit direction it points in.
        direction: Point2,
        /// Its color.
        tone: Tone,
    },
    /// Text, centered at a point and written along a direction.
    Text {
        /// Its center.
        at: Point2,
        /// The unit direction of its baseline.
        along: Point2,
        /// The text.
        text: String,
        /// Small text set above the line after it: an upper deviation.
        upper: String,
        /// Small text set below the line after it: a lower deviation.
        lower: String,
        /// Its color.
        tone: Tone,
        /// In a small box: a constraint's badge.
        badge: bool,
    },
}

/// How long an arrowhead is, in pixels, and how wide: where it is drawn.
pub const HEAD_LENGTH: f64 = 12.0;
/// See [`HEAD_LENGTH`].
pub const HEAD_WIDTH: f64 = 4.4;

/// What a canvas draws, for a view.
pub type CanvasDraw = Rc<dyn Fn(&CanvasView) -> Vec<CanvasShape>>;

/// The handler of a canvas's events.
pub type CanvasHandler<M> = Rc<dyn Fn(CanvasEvent) -> Option<M>>;

/// What a canvas says to text entered into its field: the message, or
/// why the text is not taken.
pub type CanvasEnter<M> = Rc<dyn Fn(&str) -> Result<M, String>>;

/// A text field at a point of the canvas, with the keyboard: a
/// dimension's value is typed where the dimension is. Enter (or a click
/// elsewhere) enters the text, Escape cancels.
pub struct CanvasInput<M> {
    /// What the field is for: another key is another field, which starts
    /// from its own text.
    pub key: String,
    /// Its center, in the plane's coordinates.
    pub at: Point2,
    /// The text it starts with, selected.
    pub text: String,
    /// The message for text entered, or why it is not taken (the field
    /// stays, saying so).
    pub on_enter: CanvasEnter<M>,
    /// The message that closes the field without entering anything.
    pub on_cancel: M,
}

impl<M: 'static> CanvasInput<M> {
    /// A field for `key` at `at` starting with `text`.
    pub fn new(
        key: impl Into<String>,
        at: Point2,
        text: impl Into<String>,
        on_enter: impl Fn(&str) -> Result<M, String> + 'static,
        on_cancel: M,
    ) -> Self {
        Self {
            key: key.into(),
            at,
            text: text.into(),
            on_enter: Rc::new(on_enter),
            on_cancel,
        }
    }

    /// The message for `text` entered.
    pub fn enter(&self, text: &str) -> Result<M, String> {
        (self.on_enter)(text)
    }

    fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> CanvasInput<N> {
        let (on_enter, g) = (self.on_enter, f.clone());
        CanvasInput {
            key: self.key,
            at: self.at,
            text: self.text,
            on_enter: Rc::new(move |text| on_enter(text).map(|m| g(m))),
            on_cancel: f(self.on_cancel),
        }
    }
}

/// A plane that takes every pointer event on it and draws what its
/// operation says.
pub struct Canvas<M> {
    /// The plane.
    pub plane: DatumPlane,
    /// A point of the plane near what is drawn: where the size of a pixel
    /// is measured.
    pub anchor: Point2,
    /// What it draws, for a view.
    pub draw: CanvasDraw,
    /// The message for an event, if it changes anything.
    pub on_event: CanvasHandler<M>,
    /// The text field open on it, if one is.
    pub input: Option<CanvasInput<M>>,
}

impl<M: 'static> Canvas<M> {
    /// A canvas on `plane` drawing `draw` and answering events with
    /// `on_event`.
    pub fn new(
        plane: DatumPlane,
        anchor: Point2,
        draw: impl Fn(&CanvasView) -> Vec<CanvasShape> + 'static,
        on_event: impl Fn(CanvasEvent) -> Option<M> + 'static,
    ) -> Self {
        Self {
            plane,
            anchor,
            draw: Rc::new(draw),
            on_event: Rc::new(on_event),
            input: None,
        }
    }

    /// The same canvas with the text field `input` open on it.
    #[must_use]
    pub fn with_input(mut self, input: Option<CanvasInput<M>>) -> Self {
        self.input = input;
        self
    }

    /// The point of the plane with coordinates `p`, in model space.
    pub fn place(&self, p: Point2) -> Point3 {
        self.plane.pose().transform_point(p.extend(0.0))
    }

    /// The plane coordinates of the model-space point `p` (dropped onto
    /// the plane).
    pub fn locate(&self, p: Point3) -> Point2 {
        self.plane.pose().inverse().transform_point(p).truncate()
    }

    /// The plane coordinates of the model-space direction `v` (dropped
    /// onto the plane; not normalized).
    pub fn locate_vector(&self, v: Point3) -> Point2 {
        self.plane.pose().inverse().transform_vector(v).truncate()
    }

    /// Where `ray` crosses the plane, from either side, in the plane's
    /// coordinates. `None` when the plane is seen edge-on.
    pub fn hit(&self, ray: &Ray) -> Option<Point2> {
        let (o, n) = (self.plane.origin(), self.plane.normal());
        let n = if ray.direction().dot(n) > 0.0 { -n } else { n };
        let t = ray.intersect_plane(o, n)?;
        Some(self.locate(ray.at(t)))
    }

    /// The message for `event`.
    pub fn event(&self, event: CanvasEvent) -> Option<M> {
        (self.on_event)(event)
    }

    /// What it draws in `view`.
    pub fn shapes(&self, view: &CanvasView) -> Vec<CanvasShape> {
        (self.draw)(view)
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> Canvas<N> {
        let input = self.input.map(|input| input.map_rc(f));
        let (on_event, f) = (self.on_event, f.clone());
        Canvas {
            plane: self.plane,
            anchor: self.anchor,
            draw: self.draw,
            on_event: Rc::new(move |e| on_event(e).map(|m| f(m))),
            input,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ray::Projection;

    #[test]
    fn events_land_in_plane_coordinates() {
        // The yz plane: its x is the model's y, its y the model's z.
        let canvas = Canvas::new(
            DatumPlane::yz(),
            Point2::ZERO,
            |_| Vec::new(),
            |e| Some(e.at),
        );
        let ortho = Projection::Orthographic { radius: 0.05 };
        let ray = Ray::new(Point3::new(5.0, 2.0, 3.0), -Point3::X, ortho).unwrap();
        let at = canvas.hit(&ray).unwrap();
        assert!(at.abs_diff_eq(Point2::new(2.0, 3.0), 1e-12));
        // From behind as well.
        let back = Ray::new(Point3::new(-5.0, 2.0, 3.0), Point3::X, ortho).unwrap();
        assert!(canvas.hit(&back).unwrap().abs_diff_eq(at, 1e-12));
        assert!(
            canvas
                .place(at)
                .abs_diff_eq(Point3::new(0.0, 2.0, 3.0), 1e-12)
        );
        let event = CanvasEvent {
            kind: CanvasEventKind::Click,
            at,
            add: false,
            view: CanvasView::default(),
        };
        assert_eq!(canvas.event(event), Some(at));
    }
}
