//! The topological entities and the ids that connect them.
//!
//! The hierarchy is the Parasolid/ACIS one that B-rep kernels commonly use:
//!
//! ```text
//! Body ─▶ Lump ─▶ Shell ─▶ (Face, Orientation) ─▶ Loop ─▶ Coedge ─▶ Edge ─▶ Vertex
//!                              │                           │          │
//!                         NurbsSurface                  pcurve    NurbsCurve
//!                                                     (or a pole: Coedge ─▶ Vertex)
//! ```
//!
//! * References point **down** only. Upward adjacency (which coedges use an
//!   edge, which face owns a loop) is derived by walking (see
//!   [`super::Topo::coedges_of_edge`]). This keeps every entity immutable
//!   once built, so sub-structures can be shared between bodies.
//! * A [`Coedge`] is one *use* of the boundary by a loop and carries the
//!   pcurve of that use. Usually it walks an edge in one direction: edges
//!   used by two coedges are manifold, one is a boundary/sheet edge, more
//!   than two is non-manifold.
//! * Seams are an edge used twice by the same loop with opposite
//!   orientations and different pcurves.
//! * Poles (sphere poles, cone apexes) have no edge. The loop stays at the
//!   singular vertex while its pcurve crosses the collapsed side of the
//!   (u, v) domain: a [`CoedgeKind::Pole`] coedge. The degeneracy is a
//!   property of the face's parametrisation, so it lives on the per-face
//!   use, and every [`Edge`] is a real curve with length.
//! * Orientation is stored at the *use* (coedge on edge, shell on face),
//!   never on the shared entity.

use super::arena::Id;
use super::nurbs::{Curve2, Curve3, NurbsSurface};
use crate::math::Point3;

/// Id of a vertex.
pub type VertexId = Id<Vertex>;
/// Id of an edge.
pub type EdgeId = Id<Edge>;
/// Id of a coedge.
pub type CoedgeId = Id<Coedge>;
/// Id of a loop.
pub type LoopId = Id<Loop>;
/// Id of a face.
pub type FaceId = Id<Face>;
/// Id of a shell.
pub type ShellId = Id<Shell>;
/// Id of a lump.
pub type LumpId = Id<Lump>;
/// Id of a body.
pub type BodyId = Id<Body>;
/// Id of a model-space NURBS curve.
pub type CurveId = Id<Curve3>;
/// Id of a parameter-space NURBS curve.
pub type PCurveId = Id<Curve2>;
/// Id of a NURBS surface.
pub type SurfaceId = Id<NurbsSurface>;

/// Whether a use agrees with the natural direction of what it uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Orientation {
    /// Same direction as the edge's curve / the surface's normal.
    #[default]
    Forward,
    /// Opposite direction.
    Reversed,
}

impl Orientation {
    /// The other orientation.
    #[must_use]
    pub fn flipped(self) -> Self {
        match self {
            Orientation::Forward => Orientation::Reversed,
            Orientation::Reversed => Orientation::Forward,
        }
    }

    /// Composition: `Reversed` of `Reversed` is `Forward`.
    #[must_use]
    pub fn compose(self, other: Self) -> Self {
        if self == other {
            Orientation::Forward
        } else {
            Orientation::Reversed
        }
    }
}

/// A point in model space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// Its position.
    pub point: Point3,
    /// Radius of the ball around `point` that the true vertex lies in.
    pub tolerance: f64,
}

/// A bounded piece `[t0, t1]` of a NURBS curve between two vertices,
/// oriented by increasing `t`. Shared by every coedge that uses it. An edge
/// always has a curve and a length; collapsed boundary pieces at surface
/// singularities are [`CoedgeKind::Pole`] coedges, not edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    /// The curve.
    pub curve: CurveId,
    /// Start parameter (at `start`).
    pub t0: f64,
    /// End parameter (at `end`), `t0 < t1`.
    pub t1: f64,
    /// Vertex at `t0`.
    pub start: VertexId,
    /// Vertex at `t1`; equal to `start` for a closed edge.
    pub end: VertexId,
    /// Radius of the tube around the curve that the true edge lies in.
    pub tolerance: f64,
}

impl Edge {
    /// `true` when start and end are the same vertex.
    pub fn is_closed(&self) -> bool {
        self.start == self.end
    }
}

/// What a coedge walks along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoedgeKind {
    /// An edge, along (`Forward`) or against (`Reversed`) its curve. The
    /// pcurve is same-parameter with the edge: defined on the edge's range
    /// and running in the edge's direction.
    Edge {
        /// The edge used.
        edge: EdgeId,
        /// Along or against the edge's curve.
        orientation: Orientation,
    },
    /// No edge: the loop stays at `vertex`, a surface singularity (sphere
    /// pole, cone apex), while the pcurve runs over `[t0, t1]` along the
    /// side of the (u, v) domain that collapses there. The pcurve runs in
    /// walking order.
    Pole {
        /// The singular point.
        vertex: VertexId,
        /// Start parameter of the pcurve.
        t0: f64,
        /// End parameter of the pcurve, `t0 < t1`.
        t1: f64,
    },
}

/// One use of the boundary by a loop (a "fin" in Parasolid, a "half-edge"
/// in mesh terms): an oriented edge use, or a pole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coedge {
    /// What is walked along.
    pub kind: CoedgeKind,
    /// The image of this use in the face's (u, v) space. `None` until an
    /// operation computes it.
    pub pcurve: Option<PCurveId>,
}

impl Coedge {
    /// An oriented use of `edge`.
    pub fn edge(edge: EdgeId, orientation: Orientation, pcurve: Option<PCurveId>) -> Self {
        Self {
            kind: CoedgeKind::Edge { edge, orientation },
            pcurve,
        }
    }

    /// A pole use at `vertex` whose pcurve covers `[t0, t1]`.
    pub fn pole(vertex: VertexId, t0: f64, t1: f64, pcurve: Option<PCurveId>) -> Self {
        Self {
            kind: CoedgeKind::Pole { vertex, t0, t1 },
            pcurve,
        }
    }

    /// The edge used, `None` for a pole.
    pub fn edge_id(&self) -> Option<EdgeId> {
        match self.kind {
            CoedgeKind::Edge { edge, .. } => Some(edge),
            CoedgeKind::Pole { .. } => None,
        }
    }
}

/// A closed cycle of coedges, in walking order. Walking with the face's
/// natural normal up, the face's material is on the left: an outer loop is
/// counter-clockwise in (u, v), a hole clockwise.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loop {
    /// Coedges in walking order; each ends where the next starts.
    pub coedges: Vec<CoedgeId>,
}

/// A region of a NURBS surface bounded by loops. Its natural normal is the
/// surface's `∂u × ∂v`.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// The carrier surface.
    pub surface: SurfaceId,
    /// Boundary loops. Empty means the whole (closed) surface.
    pub loops: Vec<LoopId>,
    /// Half-thickness of the slab around the surface that the true face
    /// lies in.
    pub tolerance: f64,
}

/// A face as used by a shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceUse {
    /// The face.
    pub face: FaceId,
    /// `Forward` when the shell's outward side is the face's natural normal.
    pub orientation: Orientation,
}

/// A connected set of face uses. In a solid every shell is closed and its
/// effective normals point out of the material.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shell {
    /// The face uses.
    pub faces: Vec<FaceUse>,
}

/// One connected volume (or sheet) of a body: the outer shell first, then
/// any inner void shells.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lump {
    /// Outer shell first, voids after.
    pub shells: Vec<ShellId>,
}

/// What structure a body claims to have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum BodyKind {
    /// Closed shells; every edge used by exactly two coedges.
    #[default]
    Solid,
    /// Open shells allowed; every edge used by one or two coedges.
    Sheet,
    /// Only free edges and vertices, no faces.
    Wire,
    /// Anything, including non-manifold edges and vertices.
    General,
}

/// The unit operations take and return.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Body {
    /// Solid, sheet, wire or general.
    pub kind: BodyKind,
    /// Connected components with faces.
    pub lumps: Vec<LumpId>,
    /// Edges outside any face (wire and general bodies).
    pub free_edges: Vec<EdgeId>,
    /// Vertices outside any edge (general bodies, e.g. a point body).
    pub free_vertices: Vec<VertexId>,
}

macro_rules! entity_ids {
    ($( $variant:ident($ty:ty) ),* $(,)?) => {
        /// Any id the topology stores, as one type. This is the "uid" that
        /// persistent names map to.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum EntityId {
            $(
                #[doc = concat!("A `", stringify!($ty), "`.")]
                $variant($ty),
            )*
        }

        $(
            impl From<$ty> for EntityId {
                fn from(id: $ty) -> Self {
                    EntityId::$variant(id)
                }
            }

            impl TryFrom<EntityId> for $ty {
                type Error = EntityId;
                fn try_from(id: EntityId) -> Result<Self, EntityId> {
                    match id {
                        EntityId::$variant(x) => Ok(x),
                        other => Err(other),
                    }
                }
            }
        )*
    };
}

entity_ids! {
    Vertex(VertexId),
    Edge(EdgeId),
    Coedge(CoedgeId),
    Loop(LoopId),
    Face(FaceId),
    Shell(ShellId),
    Lump(LumpId),
    Body(BodyId),
    Curve(CurveId),
    PCurve(PCurveId),
    Surface(SurfaceId),
}
