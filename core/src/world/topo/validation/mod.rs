//! Validity rules of the topology.
//!
//! `check` runs the rules that concern one entity; the topology calls it
//! on every insert in debug builds. `check_all` runs it on everything.
//!
//! | Rule | Checked on | File |
//! |---|---|---|
//! | Tolerances are finite and `≥ 0` | vertex, edge, face | `tolerance` |
//! | Edge ends lie *completely* inside their vertex balls: `|C(t) − V| + te ≤ tv` (so `tv ≥ te`) | edge | `edge` |
//! | An edge has length: its vertices don't overlap, `|Vs − Ve| > ts + te`; a closed one leaves its vertex ball at the middle parameter | edge | `edge` |
//! | A pcurve covers its coedge's parameter range (the edge's, or the pole's own) | coedge | `edge` |
//! | Every coedge of a face has a pcurve | face | `face` |
//! | `te ≥ tf` for every edge of a face | face | `face` |
//! | The edge lies on the face: `|S(p(t)) − C(t)| ≤ te` at start, middle and end; for a pole coedge the surface collapses there: `|S(p(t)) − V| ≤ tv` | face | `face` |
//! | An edge is used at most twice by one face, and twice only as a seam (opposite orientations) | face | `face` |
//! | Each coedge / loop / face / shell / lump has at most one owner and appears once in it | loop, face, shell, lump, body | `ownership` |
//! | A shell is non-empty and edge-connected | shell | `shell` |
//! | No two vertices of a body overlap: `|p − q| > tp + tq` | body | `body` |
//! | Solid: every edge used exactly twice, in opposite directions; sheet: once or twice, opposite when twice | body | `body` |
//!
//! Not checked yet: loop winding (outer loop counter-clockwise in (u, v)),
//! self-intersection of edges and faces, and shells actually enclosing a
//! volume.

mod body;
mod edge;
mod face;
mod ownership;
mod shell;
mod tolerance;

use super::{BodyId, CoedgeId, EdgeId, EntityId, FaceId, ShellId, Topo, VertexId};

/// A broken validity rule.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ValidationError {
    /// A tolerance is negative or not finite.
    #[error("{id:?} has invalid tolerance {tolerance}")]
    BadTolerance {
        /// The entity.
        id: EntityId,
        /// Its tolerance.
        tolerance: f64,
    },
    /// An edge end is not completely inside its vertex's ball.
    #[error("end of {edge:?} is {distance} from {vertex:?}, too far for the tolerances")]
    EdgeEndOutsideVertex {
        /// The edge.
        edge: EdgeId,
        /// The vertex at that end.
        vertex: VertexId,
        /// Distance from the curve end to the vertex point.
        distance: f64,
    },
    /// An edge collapses into its vertices: its two vertices overlap, or a
    /// closed edge never leaves its vertex ball (it should be a pole
    /// coedge instead).
    #[error("{edge:?} has no length ({distance} between its ends' balls)")]
    ZeroLengthEdge {
        /// The edge.
        edge: EdgeId,
        /// Distance between the two vertices (open edge), or from the
        /// curve's middle to its vertex (closed edge).
        distance: f64,
    },
    /// A pcurve's domain does not contain its edge's range.
    #[error("pcurve of {0:?} does not cover its edge's range")]
    PCurveRangeMismatch(CoedgeId),
    /// A coedge on a face has no pcurve.
    #[error("{coedge:?} on {face:?} has no pcurve")]
    MissingPCurve {
        /// The face.
        face: FaceId,
        /// The coedge.
        coedge: CoedgeId,
    },
    /// An edge has a smaller tolerance than a face it bounds.
    #[error("{edge:?} has a smaller tolerance than {face:?}")]
    EdgeTighterThanFace {
        /// The face.
        face: FaceId,
        /// The edge.
        edge: EdgeId,
    },
    /// The pcurve image on the surface is too far from the edge.
    #[error("{coedge:?} is {distance} off {face:?} at t = {t}")]
    EdgeOffFace {
        /// The face.
        face: FaceId,
        /// The coedge.
        coedge: CoedgeId,
        /// The edge parameter sampled.
        t: f64,
        /// Distance between surface point and edge point.
        distance: f64,
    },
    /// An edge is used by one face more than twice, or twice in the same
    /// direction.
    #[error("{edge:?} is used invalidly by {face:?}")]
    BadEdgeUseInFace {
        /// The face.
        face: FaceId,
        /// The edge.
        edge: EdgeId,
    },
    /// An entity appears more than once among the children of its kind of
    /// owner (two loops share a coedge, a loop lists it twice, ...).
    #[error("{id:?} is used {count} times; at most once is allowed")]
    MultipleOwners {
        /// The shared entity.
        id: EntityId,
        /// How often it appears.
        count: usize,
    },
    /// A shell without faces.
    #[error("{0:?} has no faces")]
    EmptyShell(ShellId),
    /// A shell whose faces are not connected through edges.
    #[error("{0:?} is not connected")]
    DisconnectedShell(ShellId),
    /// Two vertices of one body overlap.
    #[error("{a:?} and {b:?} overlap ({distance} apart)")]
    VerticesOverlap {
        /// One vertex.
        a: VertexId,
        /// The other.
        b: VertexId,
        /// Their distance.
        distance: f64,
    },
    /// An edge has the wrong number of uses for the body's kind.
    #[error("{edge:?} is used {uses} times in {body:?}")]
    BadEdgeUseCount {
        /// The body.
        body: BodyId,
        /// The edge.
        edge: EdgeId,
        /// Number of coedges using it.
        uses: usize,
    },
    /// The two uses of an edge run the same way: neighbouring faces are
    /// oriented inconsistently.
    #[error("{edge:?} is used twice in the same direction in {body:?}")]
    InconsistentOrientation {
        /// The body.
        body: BodyId,
        /// The edge.
        edge: EdgeId,
    },
}

/// Runs every rule that concerns `id` itself.
pub(super) fn check(topo: &Topo, id: EntityId) -> Result<(), ValidationError> {
    match id {
        EntityId::Vertex(v) => tolerance::check(id, topo.vertices[v].tolerance),
        EntityId::Edge(e) => {
            tolerance::check(id, topo.edges[e].tolerance)?;
            edge::check(topo, e)
        }
        EntityId::Coedge(c) => edge::check_coedge(topo, c),
        EntityId::Loop(l) => ownership::loop_coedges(topo, l),
        EntityId::Face(f) => {
            tolerance::check(id, topo.faces[f].tolerance)?;
            ownership::face_loops(topo, f)?;
            face::check(topo, f)
        }
        EntityId::Shell(s) => {
            ownership::shell_faces(topo, s)?;
            shell::check(topo, s)
        }
        EntityId::Lump(l) => ownership::lump_shells(topo, l),
        EntityId::Body(b) => {
            ownership::body_lumps(topo, b)?;
            body::check(topo, b)
        }
        // Validated on construction.
        EntityId::Curve(_) | EntityId::PCurve(_) | EntityId::Surface(_) => Ok(()),
    }
}

/// Runs [`check`] on every entity and collects every violation.
pub(super) fn check_all(topo: &Topo) -> Result<(), Vec<ValidationError>> {
    let ids = topo
        .vertices
        .ids()
        .map(EntityId::from)
        .chain(topo.edges.ids().map(EntityId::from))
        .chain(topo.coedges.ids().map(EntityId::from))
        .chain(topo.loops.ids().map(EntityId::from))
        .chain(topo.faces.ids().map(EntityId::from))
        .chain(topo.shells.ids().map(EntityId::from))
        .chain(topo.lumps.ids().map(EntityId::from))
        .chain(topo.bodies.ids().map(EntityId::from));
    let errors: Vec<_> = ids.filter_map(|id| check(topo, id).err()).collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// The tests expect inserts to refuse invalid entities, which only happens
// with debug assertions on.
#[cfg(all(test, debug_assertions))]
mod tests;
