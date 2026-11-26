//! What a part says of how it is looked at: named views of it, and
//! where it is cut open to show its inside. Steps make them; the 3D
//! view and the drawings read them from the world, so both follow the
//! part.

use crate::math::{Point2, Point3};

/// A way of looking at the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewpoint {
    /// What is to the right of who looks: a unit vector of the model.
    pub right: Point3,
    /// What is up.
    pub up: Point3,
    /// What points at who looks.
    pub towards: Point3,
    /// Whether it is a parallel projection, not a perspective.
    pub orthographic: bool,
    /// What is looked at and from how far, where the view says so; else
    /// all of the model is looked at.
    pub at: Option<(Point3, f64)>,
}

impl Viewpoint {
    /// Looking against `towards` with `up` as near up as it can be.
    /// `None` where the two are along each other.
    pub fn looking(towards: Point3, up: Point3) -> Option<Self> {
        let towards = towards.try_normalize()?;
        let right = up.cross(towards).try_normalize()?;
        Some(Self {
            right,
            up: towards.cross(right),
            towards,
            orthographic: true,
            at: None,
        })
    }

    /// The same, turned by `angle` (radians, counter-clockwise as seen)
    /// about the line it looks along.
    #[must_use]
    pub fn turned(self, angle: f64) -> Self {
        let (s, c) = libm::sincos(angle);
        Self {
            right: self.right * c + self.up * s,
            up: self.up * c - self.right * s,
            ..self
        }
    }
}

/// How a section treats a part (ISO 128-50): shafts, screws, pins and
/// the like cut along their axis are drawn whole, not hatched.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Sectioned {
    /// Cut and hatched, as most parts are.
    #[default]
    Cut,
    /// Never cut: drawn whole where a section runs through it.
    Whole,
    /// Drawn whole where a section runs along the axis (in the part's
    /// own frame, a unit vector), cut where it runs across.
    WholeAlong(Point3),
}

impl Sectioned {
    /// Whether a cut whose surface is square to `normal` (in the part's
    /// own frame) cuts it.
    pub fn cuts(self, normal: Point3) -> bool {
        match self {
            Sectioned::Cut => true,
            Sectioned::Whole => false,
            Sectioned::WholeAlong(axis) => axis.dot(normal.normalize_or_zero()).abs() > 0.05,
        }
    }

    /// Whether a cut along `cut` (in the part's own frame) cuts it: it
    /// does unless every piece of its line leaves the part whole.
    pub fn cut_by(self, cut: &Cutting) -> bool {
        cut.path.windows(2).any(|w| {
            let along = (w[1] - w[0]).normalize_or_zero();
            self.cuts(cut.x * -along.y + cut.y * along.x)
        })
    }
}

/// Where a model is cut open: along `path`, a line of the plane through
/// `origin` along `x` and `y` (unit vectors, square to each other) that
/// is gone along from its first point to its last and runs on straight
/// past both ends; the cut is square to that plane, and what is to the
/// left of the line is taken away. One piece is a plane; several, one
/// after the other, an offset (stepped) or an aligned section.
#[derive(Debug, Clone, PartialEq)]
pub struct Cutting {
    /// A point of the plane the line lies in.
    pub origin: Point3,
    /// Along the plane.
    pub x: Point3,
    /// Along the plane, square to `x`.
    pub y: Point3,
    /// The corners of the line, in the plane: two or more.
    pub path: Vec<Point2>,
    /// Whether it cuts only along the line as far as it goes (a line
    /// sketched from a hub's middle to its rim: a half section), not on
    /// past its ends.
    pub bounded: bool,
}

impl Cutting {
    /// The cut along the plane through `origin` that takes away what
    /// is on the side `normal` points to.
    pub fn plane(origin: Point3, normal: Point3) -> Self {
        let normal = normal.normalize_or(Point3::Z);
        let x = normal.any_orthonormal_vector();
        Self {
            origin,
            x,
            y: normal,
            path: vec![Point2::new(-1.0, 0.0), Point2::new(1.0, 0.0)],
            bounded: false,
        }
    }

    /// The direction the cut is looked at from: who stands on the side
    /// taken away, square to the line's first piece.
    pub fn towards(&self) -> Point3 {
        let d = (self.path[1] - self.path[0]).normalize_or_zero();
        let left = d.perp();
        self.x * left.x + self.y * left.y
    }

    /// The same, the other side taken away.
    #[must_use]
    pub fn flipped(mut self) -> Self {
        self.path.reverse();
        self
    }
}
