//! Whole-body rules: vertex separation and manifold edge use.

use std::collections::{BTreeMap, BTreeSet};

use super::ValidationError;
use crate::world::topo::{BodyId, BodyKind, CoedgeKind, Orientation, Topo};

/// No two vertices of the body overlap, and edge uses fit the body kind.
pub(super) fn check(topo: &Topo, id: BodyId) -> Result<(), ValidationError> {
    let body = &topo.bodies[id];

    // Every vertex and every edge use reachable from the body.
    let mut vertices: BTreeSet<_> = body.free_vertices.iter().copied().collect();
    let mut uses: BTreeMap<_, Vec<Orientation>> = BTreeMap::new();
    for &e in &body.free_edges {
        vertices.extend([topo.edges[e].start, topo.edges[e].end]);
    }
    for &l in &body.lumps {
        for &s in &topo.lumps[l].shells {
            for u in &topo.shells[s].faces {
                for &lp in &topo.faces[u.face].loops {
                    for &c in &topo.loops[lp].coedges {
                        let coedge = &topo.coedges[c];
                        let (start, end) = topo.coedge_ends(coedge);
                        vertices.extend([start, end]);
                        if let CoedgeKind::Edge { edge, orientation } = coedge.kind {
                            // The direction the edge is walked in, seen from
                            // the shell's outside.
                            let effective = orientation.compose(u.orientation);
                            uses.entry(edge).or_default().push(effective);
                        }
                    }
                }
            }
        }
    }

    let vertices: Vec<_> = vertices.into_iter().collect();
    for (i, &a) in vertices.iter().enumerate() {
        for &b in &vertices[i + 1..] {
            let (va, vb) = (&topo.vertices[a], &topo.vertices[b]);
            let distance = (va.point - vb.point).length();
            if distance <= va.tolerance + vb.tolerance {
                return Err(ValidationError::VerticesOverlap { a, b, distance });
            }
        }
    }

    let allowed = match body.kind {
        BodyKind::Solid => 2..=2,
        BodyKind::Sheet => 1..=2,
        BodyKind::Wire | BodyKind::General => return Ok(()),
    };
    for (edge, dirs) in uses {
        if !allowed.contains(&dirs.len()) {
            return Err(ValidationError::BadEdgeUseCount {
                body: id,
                edge,
                uses: dirs.len(),
            });
        }
        if let [a, b] = dirs[..]
            && a == b
        {
            return Err(ValidationError::InconsistentOrientation { body: id, edge });
        }
    }
    Ok(())
}
