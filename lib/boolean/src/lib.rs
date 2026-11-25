//! Booleans of solids: union, intersection and difference of B-reps on
//! NURBS geometry.
//!
//! The method is the general fuse: find where
//! the two solids' boundaries meet, cut the faces of both along it, and
//! keep the pieces the operation wants. What holds it up:
//!
//! * **One way to find where things meet.** Every pair — a point and a
//!   curve, two curves, a curve and a surface — goes through the same
//!   search: Bézier clipping of the pieces of their NURBS, the contact
//!   polished by Newton's method (`solve`). Nothing is asked of a curve
//!   or a surface but its control points; a line is not treated
//!   differently from a spline, nor a plane from a torus.
//! * **By tolerance, everywhere.** Vertices, edges and faces each have
//!   their own tolerance; two things meet where they come within the
//!   sum of theirs — crossing, touching, or running along each other,
//!   which the search reports as one long contact.
//! * **Up by dimension.** Vertices on vertices, then on edges, edges on
//!   edges, vertices and edges on faces, and only then faces on faces,
//!   whose crossings are followed from vertex to vertex (`trace`). Each
//!   step takes what is below it as settled, so a face is never cut by
//!   a curve that half runs along an edge.
//! * **Measured, not assumed.** A curve fitted where two faces cross is
//!   stepped as finely as keeps it — and its images in both faces'
//!   (u, v) — within a billionth of the solids' size, and what it still
//!   strays becomes its edge's tolerance.
//! * **Refusal is a result.** What cannot be decided — solids that
//!   touch along a line and would share an edge four ways, crossings
//!   that run through a point where the faces are tangent — is a
//!   [`Refusal`] that says why, never a solid that is a little wrong.
//!
//! [`measure`] gives the volume and area of a solid from its surfaces
//! and loops themselves (Green's theorem), which is what the benchmark
//! suite (`bench/boolean`) grades results with.
mod apart;
mod bezier;
mod cells;
mod drawn;
mod fit;
mod flat;
pub mod fuse;
pub mod geometry;
mod grid;
mod inertia;
pub mod io;
mod known;
mod loops;
pub mod measure;
pub mod model;
mod place;
mod query;
mod shapes;
mod shown;
mod sliced;
mod solve;
mod stitch;
mod tidy;
mod trace;

#[cfg(test)]
mod tests;

pub use apart::{Meeting, distance, interference};
pub use drawn::{Drawn, project};
pub use fuse::{Outcome, combine, meeting_curves};
pub use inertia::{Spread, spread};
pub use io::{Out, Sheet, read, read_sheet, remove, write, write_sheet};
pub use measure::{Measures, face_area, measure};
pub use model::Solid;
pub use place::{Placer, Side};
pub use shapes::{block, faceted, under};
pub use shown::{Shown, ShownFace, display};
pub use sliced::plane_section;
pub use stitch::stitched;

/// What to make of two solids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    /// Everything in either.
    Union,
    /// What is in both.
    Intersect,
    /// The first without what is in the second.
    Cut,
}

/// Why a boolean was not done.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Refusal {
    /// The solids meet in a way too close to decide.
    #[error("the bodies meet too closely to tell apart: {0}")]
    Degenerate(String),
    /// The pieces kept do not close up.
    #[error("the result would not be a closed solid: {0}")]
    Open(String),
    /// Something is not a solid body.
    #[error("{0} is not a solid body")]
    NotSolid(String),
}

/// Whether the point `p` is inside `solid`; `None` if it is on its
/// boundary, or too near it to tell.
pub fn holds(solid: &Solid, p: workpiece_core::math::Point3) -> Option<bool> {
    Holder::new(solid).holds(p)
}

/// A solid made ready to be asked, of one point after another, whether
/// it is inside it: as [`holds`] tells it, what is searched of the
/// solid worked out once.
pub struct Holder<'a> {
    solid: &'a Solid,
    data: query::SolidData,
    all: Vec<usize>,
}

impl<'a> Holder<'a> {
    /// What answers for `solid`.
    pub fn new(solid: &'a Solid) -> Self {
        Self {
            solid,
            data: query::SolidData::new(solid),
            all: (0..solid.faces.len()).collect(),
        }
    }

    /// Whether the point `p` is inside the solid; `None` if it is on
    /// its boundary, or too near it to tell.
    pub fn holds(&self, p: workpiece_core::math::Point3) -> Option<bool> {
        query::inside(self.solid, &self.data, &self.all, p)
    }
}

/// Whether the point `p` is on face `face` of `solid`, within `tol`:
/// inside its loops, or on them.
pub fn lies_on(solid: &Solid, face: usize, p: workpiece_core::math::Point3, tol: f64) -> bool {
    // (Only that face is made ready: not the whole solid for one of
    // its faces.)
    let data = query::FaceData::new(solid, &solid.faces[face]);
    query::on_face(&solid.faces[face], &data, p, tol).is_some()
}

/// `first` combined by `op` with each of `rest`, one after the other:
/// the union of them all, what `first` and all of them share, or
/// `first` without any of them.
pub fn combine_all(op: Op, first: &Solid, rest: &[&Solid]) -> Result<Solid, Refusal> {
    let mut made = first.clone();
    for next in rest {
        made = combine(op, &made, next)?.solid;
    }
    Ok(made)
}
