//! Edges against their vertices, and pcurves against their coedges.

use super::ValidationError;
use crate::world::topo::{CoedgeId, EdgeId, Topo};

/// Each end of the edge's tube lies completely inside its vertex's ball:
/// `|C(t) − V| + te ≤ tv` (which also gives `tv ≥ te`), and the edge has
/// length (see [`check_length`]).
pub(super) fn check(topo: &Topo, id: EdgeId) -> Result<(), ValidationError> {
    let edge = &topo.edges[id];
    let curve = &topo.curves[edge.curve];
    for (t, v) in [(edge.t0, edge.start), (edge.t1, edge.end)] {
        let vertex = &topo.vertices[v];
        let distance = (curve.point_at(t) - vertex.point).length();
        if distance + edge.tolerance > vertex.tolerance {
            return Err(ValidationError::EdgeEndOutsideVertex {
                edge: id,
                vertex: v,
                distance,
            });
        }
    }
    check_length(topo, id)
}

/// An edge must not collapse into its vertices: an open edge's two
/// vertices must not overlap (`|Vs − Ve| > ts + te`), and a closed edge's
/// curve must leave the vertex ball at its middle parameter (a sanity
/// sample, not a proof). A collapsed boundary piece is a pole coedge.
fn check_length(topo: &Topo, id: EdgeId) -> Result<(), ValidationError> {
    let edge = &topo.edges[id];
    let start = &topo.vertices[edge.start];
    let (distance, allowed) = if edge.is_closed() {
        let middle = topo.curves[edge.curve].point_at(0.5 * (edge.t0 + edge.t1));
        ((middle - start.point).length(), start.tolerance)
    } else {
        let end = &topo.vertices[edge.end];
        (
            (end.point - start.point).length(),
            start.tolerance + end.tolerance,
        )
    };
    if distance <= allowed {
        return Err(ValidationError::ZeroLengthEdge { edge: id, distance });
    }
    Ok(())
}

/// A pcurve's domain must contain the coedge's range (the edge's range for
/// an edge use, the pole's own range for a pole).
pub(super) fn check_coedge(topo: &Topo, id: CoedgeId) -> Result<(), ValidationError> {
    let coedge = &topo.coedges[id];
    let Some(pcurve) = coedge.pcurve else {
        return Ok(());
    };
    let (t0, t1) = topo.coedge_range(coedge);
    let (d0, d1) = topo.pcurves[pcurve].domain();
    if d0 <= t0 && t1 <= d1 {
        Ok(())
    } else {
        Err(ValidationError::PCurveRangeMismatch(id))
    }
}
