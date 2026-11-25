//! What handles look like, as pieces that are both drawn and picked.
//!
//! Handles keep their size on screen: pieces are measured in pixels and
//! turned into model units with the size of a pixel at the handle, which
//! the view knows.

use crate::math::{Axis, Point3};

use super::{AngularHandle, Gizmo, GizmoPart, Handle, LinearHandle, PlanarHandle};

/// Length of a linear handle's arrow, in pixels.
pub const ARROW_PX: f64 = 60.0;
/// Radius of an angular handle's ring, in pixels.
pub const ANGLE_PX: f64 = 70.0;
/// A gizmo's rings, as a fraction of its size.
pub const RING_RADIUS: f64 = 0.7;
/// A gizmo's outer ring, that scales, as a fraction of its size: out
/// beyond its arrows' tips.
pub const SCALE_RADIUS: f64 = 1.25;
/// Radius of the ring that lies around a planar handle's knob, in its
/// plane, in pixels.
pub const PLANAR_PX: f64 = 14.0;

/// The geometry of a piece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// A round knob at a point.
    Knob(Point3),
    /// An arrow from one point to another.
    Arrow(Point3, Point3),
    /// A ring around `center` with unit `normal`.
    Ring {
        /// The center.
        center: Point3,
        /// The unit normal of its plane.
        normal: Point3,
        /// The radius.
        radius: f64,
    },
}

/// One piece of a handle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    /// What dragging it moves: a gizmo part, or `None` for a linear
    /// handle.
    pub part: Option<GizmoPart>,
    /// The axis it belongs to, for its color.
    pub axis: Option<Axis>,
    /// Its geometry.
    pub shape: Shape,
}

impl<M: 'static> Handle<M> {
    /// The point the handle's size is measured at.
    pub fn anchor(&self) -> Point3 {
        match self {
            Handle::Linear(h) => h.tip(),
            Handle::Gizmo(g) => g.origin(),
            Handle::Planar(h) => h.at,
            Handle::Angular(h) => h.center,
            Handle::Canvas(c) => c.place(c.anchor),
            Handle::Space(s) => s.anchor,
        }
    }

    /// The handle's pieces where a pixel at [`anchor`](Self::anchor) is
    /// `pixel` model units long, seen from the unit direction `facing`
    /// (towards the eye).
    pub fn pieces(&self, pixel: f64, facing: Point3) -> Vec<Piece> {
        match self {
            Handle::Linear(h) => linear(h, pixel),
            Handle::Gizmo(g) => gizmo(g, pixel, facing),
            Handle::Planar(h) => planar(h, pixel),
            Handle::Angular(h) => angular(h, pixel),
            // A canvas draws its own shapes (see `Canvas::shapes`).
            Handle::Canvas(_) | Handle::Space(_) => Vec::new(),
        }
    }
}

/// A knob at the tip and an arrow from it, outwards along the direction:
/// the same size wherever the tip is.
fn linear<M: 'static>(h: &LinearHandle<M>, pixel: f64) -> Vec<Piece> {
    let tip = h.tip();
    let piece = |shape| Piece {
        part: None,
        axis: None,
        shape,
    };
    vec![
        piece(Shape::Arrow(tip, tip + h.direction * ARROW_PX * pixel)),
        piece(Shape::Knob(tip)),
    ]
}

/// A knob, picked before what else is there (as a gizmo's centre is),
/// with a small ring around it in the plane it moves in.
fn planar<M: 'static>(h: &PlanarHandle<M>, pixel: f64) -> Vec<Piece> {
    let piece = |shape| Piece {
        part: Some(GizmoPart::Free),
        axis: None,
        shape,
    };
    let mut pieces = vec![
        piece(Shape::Ring {
            center: h.at,
            normal: h.normal,
            radius: PLANAR_PX * pixel,
        }),
        piece(Shape::Knob(h.at)),
    ];
    // (The tip of a direction: the arrow to it.)
    if let Some(from) = h.from {
        pieces.insert(0, piece(Shape::Arrow(from, h.at)));
    }
    pieces
}

/// A ring about the axis, and a knob on it at the angle.
fn angular<M: 'static>(h: &AngularHandle<M>, pixel: f64) -> Vec<Piece> {
    let radius = ANGLE_PX * pixel;
    let piece = |shape| Piece {
        part: None,
        axis: None,
        shape,
    };
    vec![
        piece(Shape::Ring {
            center: h.center,
            normal: h.axis,
            radius,
        }),
        piece(Shape::Knob(h.center + h.direction() * radius)),
    ]
}

/// A center knob that moves freely, an arrow along each axis and a ring
/// about it, `size` pixels across; and, for a gizmo that scales, a ring
/// around it all, facing the eye (`facing`).
fn gizmo<M: 'static>(g: &Gizmo<M>, pixel: f64, facing: Point3) -> Vec<Piece> {
    let o = g.origin();
    let size = g.size * pixel;
    let mut pieces = vec![Piece {
        part: Some(GizmoPart::Free),
        axis: None,
        shape: Shape::Knob(o),
    }];
    for axis in Axis::ALL {
        let direction = g.axis(axis);
        pieces.push(Piece {
            part: Some(GizmoPart::Translate(axis)),
            axis: Some(axis),
            shape: Shape::Arrow(o, o + direction * size),
        });
        pieces.push(Piece {
            part: Some(GizmoPart::Rotate(axis)),
            axis: Some(axis),
            shape: Shape::Ring {
                center: o,
                normal: direction,
                radius: size * RING_RADIUS,
            },
        });
    }
    if g.on_scale.is_some() {
        pieces.push(Piece {
            part: Some(GizmoPart::Scale),
            axis: None,
            shape: Shape::Ring {
                center: o,
                normal: facing,
                radius: size * SCALE_RADIUS,
            },
        });
    }
    pieces
}
