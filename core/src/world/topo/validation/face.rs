//! Faces against the edges that bound them.

use std::collections::BTreeMap;

use super::ValidationError;
use crate::world::topo::{CoedgeKind, FaceId, Orientation, Topo};

/// For every coedge of the face: it has a pcurve, and the pcurve's image on
/// the surface is within the edge tolerance of the edge (within the vertex
/// tolerance of the vertex, for a pole) at start, middle and end — a
/// sanity check, not a proof. Edges are no tighter than the face and used
/// at most twice, and twice only as a seam.
pub(super) fn check(topo: &Topo, id: FaceId) -> Result<(), ValidationError> {
    let face = &topo.faces[id];
    let surface = &topo.surfaces[face.surface];
    let mut uses: BTreeMap<_, Vec<Orientation>> = BTreeMap::new();

    for &l in &face.loops {
        for &c in &topo.loops[l].coedges {
            let coedge = &topo.coedges[c];
            let pcurve = coedge.pcurve.ok_or(ValidationError::MissingPCurve {
                face: id,
                coedge: c,
            })?;
            let pcurve = &topo.pcurves[pcurve];

            // What the pcurve's image must stay close to: the edge curve
            // (within te), or the pole vertex (within tv).
            let (t0, t1) = topo.coedge_range(coedge);
            for t in [t0, 0.5 * (t0 + t1), t1] {
                let uv = pcurve.point_at(t);
                let on_surface = surface.point_at(uv.x, uv.y);
                let (target, allowed) = match coedge.kind {
                    CoedgeKind::Edge { edge, .. } => {
                        let e = &topo.edges[edge];
                        (topo.curves[e.curve].point_at(t), e.tolerance)
                    }
                    CoedgeKind::Pole { vertex, .. } => {
                        let v = &topo.vertices[vertex];
                        (v.point, v.tolerance)
                    }
                };
                let distance = (on_surface - target).length();
                if distance > allowed {
                    return Err(ValidationError::EdgeOffFace {
                        face: id,
                        coedge: c,
                        t,
                        distance,
                    });
                }
            }

            if let CoedgeKind::Edge { edge, orientation } = coedge.kind {
                if topo.edges[edge].tolerance < face.tolerance {
                    return Err(ValidationError::EdgeTighterThanFace { face: id, edge });
                }
                uses.entry(edge).or_default().push(orientation);
            }
        }
    }

    for (edge, orientations) in uses {
        let ok = match orientations[..] {
            [_] => true,
            [a, b] => a != b,
            _ => false,
        };
        if !ok {
            return Err(ValidationError::BadEdgeUseInFace { face: id, edge });
        }
    }
    Ok(())
}
