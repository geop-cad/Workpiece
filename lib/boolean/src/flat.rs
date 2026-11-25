//! A flat face laid out evenly, and where an edge goes through its
//! plane: told from the edge's Bézier points alone, where that is
//! certain.

use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::NurbsSurface;

use crate::bezier::{Piece, roots};

/// A plane a surface is, its parameters going evenly along two ways.
pub(crate) struct Flat {
    origin: Point3,
    normal: Point3,
    along: [Point3; 2],
    domain: [(f64, f64); 2],
}

/// Where a curve goes through a plane: its parameter, the place in the
/// plane's parameters, and the point.
pub(crate) type Through = (f64, Point2, Point3);

impl Flat {
    /// `surface` as a plane, if it is one laid out evenly: bilinear
    /// between four corners of a parallelogram.
    pub(crate) fn of(surface: &NurbsSurface) -> Option<Flat> {
        let p = surface.control_points();
        if surface.degrees() != (1, 1)
            || p.len() != 4
            || surface.weights().iter().any(|w| *w != 1.0)
        {
            return None;
        }
        // (The first parameter slow: p[1] is along v, p[2] along u.)
        let (eu, ev) = (p[2] - p[0], p[1] - p[0]);
        let size = p[0].distance(p[3]);
        if (p[3] - p[2] - p[1] + p[0]).length() > 1e-12 * size {
            return None;
        }
        let normal = eu.cross(ev).try_normalize()?;
        let (du, dv) = surface.domain();
        Some(Flat {
            origin: p[0],
            normal,
            along: [eu, ev],
            domain: [du, dv],
        })
    }

    /// The plane's parameters of the point `q` of it.
    fn uv(&self, q: Point3) -> Point2 {
        let [eu, ev] = self.along;
        let a = q - self.origin;
        let (uu, uv, vv) = (eu.dot(eu), eu.dot(ev), ev.dot(ev));
        let (au, av) = (a.dot(eu), a.dot(ev));
        let det = uu * vv - uv * uv;
        let (s, t) = ((au * vv - av * uv) / det, (av * uu - au * uv) / det);
        let [(u0, u1), (v0, v1)] = self.domain;
        Point2::new(u0 + s * (u1 - u0), v0 + t * (v1 - v0))
    }

    /// How the curve of the Bézier `pieces` meets the part of the plane
    /// over the box `over` of its parameters, counting what is within
    /// `tol` as met — where that is certain from the pieces' points:
    ///
    /// - every one of them clear of the plane, and the curve through
    ///   it as often as they change sides (it cannot then come near
    ///   the plane anywhere else);
    /// - only its ends in the plane, the rest clear of it on one side;
    /// - all of them in the plane, and what they span either all
    ///   outside the box or (at the five places `lying` are looked for)
    ///   inside it.
    ///
    /// `None` where it is not: a touch, a place at the rim of the box,
    /// a curve in the plane that is partly over the box.
    pub(crate) fn meets(&self, pieces: &[Piece], tol: f64, over: [(f64, f64); 2]) -> Option<Met> {
        let clear = tol * 16.0 + 1e-12;
        // Where a place of the plane is to the box: inside it, outside
        // of it, or (`None`) at its rim.
        let rims: [f64; 2] = std::array::from_fn(|k| {
            let (d0, d1) = self.domain[k];
            clear / self.along[k].length() * (d1 - d0).abs()
        });
        let inside = |uv: Point2| -> Option<bool> {
            let mut inside = true;
            for k in 0..2 {
                let (lo, hi) = over[k];
                let x = uv[k];
                if x < lo - rims[k] || x > hi + rims[k] {
                    inside = false;
                } else if x < lo + rims[k] || x > hi - rims[k] {
                    return None;
                }
            }
            Some(inside)
        };
        let height =
            |h: &[f64; 4]| (Point3::new(h[0], h[1], h[2]) - self.origin * h[3]).dot(self.normal);
        let place = |h: &[f64; 4]| {
            let point = Point3::new(h[0], h[1], h[2]) / h[3];
            point - self.normal * (point - self.origin).dot(self.normal)
        };
        if pieces
            .iter()
            .any(|p| p.n[1] != 1 || p.cp.iter().any(|h| h[3] <= 0.0))
        {
            return None;
        }
        // All of it in the plane.
        let within = |h: &[f64; 4]| (height(h) / h[3]).abs() <= tol * 0.5;
        if pieces.iter().all(|p| p.cp.iter().all(within)) {
            // (What its points span: it is in there.)
            let (mut lo, mut hi) = (Point2::INFINITY, Point2::NEG_INFINITY);
            for h in pieces.iter().flat_map(|p| &p.cp) {
                let uv = self.uv(place(h));
                (lo, hi) = (lo.min(uv), hi.max(uv));
            }
            let apart = (0..2).any(|k| hi[k] < over[k].0 - rims[k] || lo[k] > over[k].1 + rims[k]);
            if apart {
                return Some(Met::Through(Vec::new()));
            }
            // The places a curve is looked at to tell whether it lies
            // in a face.
            let (first, last) = (pieces.first()?, pieces.last()?);
            let (t0, t1) = (first.range[0].0, last.range[0].1);
            for share in [0.5, 0.15, 0.85, 0.3, 0.7] {
                let t = t0 + (t1 - t0) * share;
                let piece = pieces.iter().find(|p| t <= p.range[0].1).unwrap_or(last);
                let (a, b) = piece.range[0];
                let h = casteljau(&piece.cp, (t - a) / (b - a));
                if inside(self.uv(place(&h))) != Some(true) {
                    return None;
                }
            }
            return Some(Met::Lying);
        }
        let mut found = Vec::new();
        let mut heights = Vec::new();
        let count = pieces.len();
        for (k, piece) in pieces.iter().enumerate() {
            heights.clear();
            heights.extend(piece.cp.iter().map(height));
            let n = heights.len();
            let (t0, t1) = piece.range[0];
            // An end of the curve in the plane: met there, if the rest
            // is clear of the plane.
            let mut from = 0;
            let mut to = n;
            if k == 0 && (heights[0] / piece.cp[0][3]).abs() <= tol {
                from = 1;
            }
            if k + 1 == count && (heights[n - 1] / piece.cp[n - 1][3]).abs() <= tol {
                to = n - 1;
            }
            if from >= to {
                return None;
            }
            let inner = heights[from..to].iter().zip(&piece.cp[from..to]);
            if inner.into_iter().any(|(d, h)| (d / h[3]).abs() <= clear) {
                return None;
            }
            let changes = heights[from..to]
                .windows(2)
                .filter(|w| (w[0] > 0.0) != (w[1] > 0.0))
                .count();
            let ends = (from == 1, to == n - 1);
            if (ends.0 || ends.1) && changes > 0 {
                return None;
            }
            for (end, i, t) in [(ends.0, 0, t0), (ends.1, n - 1, t1)] {
                if end {
                    let point = place(&piece.cp[i]);
                    let uv = self.uv(point);
                    if inside(uv)? {
                        found.push((t, uv, point));
                    }
                }
            }
            if changes == 0 {
                continue;
            }
            let at = roots(&heights, 0.0)?;
            if at.len() != changes {
                return None;
            }
            for r in at {
                let point = place(&casteljau(&piece.cp, r));
                let uv = self.uv(point);
                if inside(uv)? {
                    found.push((t0 + (t1 - t0) * r, uv, point));
                }
            }
        }
        Some(Met::Through(found))
    }
}

/// How a curve meets a flat face's plane, over the face.
pub(crate) enum Met {
    /// It lies in it.
    Lying,
    /// It goes through it at these places (or ends in it there).
    Through(Vec<Through>),
}

/// The point at `t` of the Bézier curve of the points `cp`.
fn casteljau(cp: &[[f64; 4]], t: f64) -> [f64; 4] {
    let mut row = cp.to_vec();
    for k in 1..row.len() {
        for i in 0..row.len() - k {
            let next = row[i + 1];
            for (x, y) in row[i].iter_mut().zip(next) {
                *x += (y - *x) * t;
            }
        }
    }
    row[0]
}

/// `solid` with its flat faces that are laid out around a point (what a
/// line sweeps turning about an axis it is square to: a ring or a disc,
/// with a seam, or coming to a point in its middle) laid out evenly
/// instead, between their rims alone: the seam and the point are no
/// part of such a face. What crosses a face laid out evenly does so
/// along curves that are simple in its parameters, which around a point
/// they are not. `None` if there is no such face.
pub(crate) fn evened(solid: &crate::model::Solid) -> Option<crate::model::Solid> {
    use crate::model::{Use, Walk};
    use shapecraft_core::world::topo::Curve2;
    use std::sync::Arc;
    let mut out = solid.clone();
    let mut changed = false;
    for (f, face) in solid.faces.iter().enumerate() {
        if Flat::of(&face.surface).is_some() {
            continue;
        }
        // The edges it walks both ways (seams), and whether it stays
        // at a point anywhere.
        let walked = |edge: usize| {
            let steps = face.loops.iter().flatten();
            steps
                .filter(|s| matches!(s.walk, Walk::Edge { edge: e, .. } if e == edge))
                .count()
        };
        let mut odd = false;
        for step in face.loops.iter().flatten() {
            odd |= match step.walk {
                Walk::Edge { edge, .. } => walked(edge) == 2,
                Walk::Pole { .. } => true,
            };
        }
        if !odd {
            continue;
        }
        // Flat: all of its points in one plane.
        let points = face.surface.control_points();
        let size = points
            .iter()
            .fold(0.0_f64, |m, p| m.max(p.distance(points[0])));
        let ((u0, u1), (v0, v1)) = face.surface.domain();
        let normal = [(0.37, 0.41), (0.61, 0.33), (0.45, 0.71)]
            .into_iter()
            .find_map(|(s, t)| {
                let (_, du, dv) = face
                    .surface
                    .derivatives_at(u0 + (u1 - u0) * s, v0 + (v1 - v0) * t);
                du.cross(dv).try_normalize()
            });
        let Some(normal) = normal else {
            continue;
        };
        let origin = face.surface.point_at((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        if size <= 0.0
            || points
                .iter()
                .any(|p| (*p - origin).dot(normal).abs() > 1e-12 * size)
            || face.surface.weights().iter().any(|w| *w <= 0.0)
        {
            continue;
        }
        // What is left of its loops without the seams and the points,
        // each stretch that closes a loop of its own.
        let mut loops: Vec<Vec<Use>> = Vec::new();
        let mut whole = true;
        for walk in &face.loops {
            let kept: Vec<&Use> = walk
                .iter()
                .filter(|s| match s.walk {
                    Walk::Edge { edge, .. } => walked(edge) == 1,
                    Walk::Pole { .. } => false,
                })
                .collect();
            let mut chain: Vec<Use> = Vec::new();
            for step in kept {
                chain.push(step.clone());
                let first = solid.ends(&chain[0]).0;
                if solid.ends(step).1 == first {
                    loops.push(std::mem::take(&mut chain));
                }
            }
            whole &= chain.is_empty();
        }
        if !whole || loops.is_empty() {
            continue;
        }
        // Laid out over what its edges span, the normal as it was.
        let eu = normal.any_orthonormal_vector();
        let ev = normal.cross(eu);
        let (mut lo, mut hi) = (Point2::INFINITY, Point2::NEG_INFINITY);
        let flatly = |p: Point3| Point2::new((p - origin).dot(eu), (p - origin).dot(ev));
        for step in loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                for p in solid.edges[edge].curve.control_points() {
                    let q = flatly(*p);
                    (lo, hi) = (lo.min(q), hi.max(q));
                }
            }
        }
        let pad = Point2::splat((hi - lo).length() * 0.02);
        let (lo, hi) = (lo - pad, hi + pad);
        let at = |x: f64, y: f64| origin + eu * x + ev * y;
        let Ok(surface) = NurbsSurface::bilinear(
            at(lo.x, lo.y),
            at(hi.x, lo.y),
            at(lo.x, hi.y),
            at(hi.x, hi.y),
        ) else {
            continue;
        };
        let mut fine = true;
        for step in loops.iter_mut().flatten() {
            let Walk::Edge { edge, .. } = step.walk else {
                continue;
            };
            let curve = &solid.edges[edge].curve;
            let points = curve
                .control_points()
                .iter()
                .map(|p| (flatly(*p) - lo) / (hi - lo))
                .collect();
            match Curve2::new(
                curve.degree(),
                curve.knots().to_vec(),
                points,
                Some(curve.weights().to_vec()),
            ) {
                Ok(pcurve) => step.pcurve = Arc::new(pcurve),
                Err(_) => fine = false,
            }
        }
        if !fine {
            continue;
        }
        // (The outer loop first: the one that reaches farthest.)
        let reach = |walk: &Vec<Use>| {
            let steps = walk.iter();
            steps
                .filter_map(|s| match s.walk {
                    Walk::Edge { edge, .. } => Some(&solid.edges[edge]),
                    Walk::Pole { .. } => None,
                })
                .flat_map(|e| e.curve.control_points().iter())
                .fold(0.0_f64, |m, p| m.max(p.distance(origin)))
        };
        loops.sort_by(|a, b| reach(b).total_cmp(&reach(a)));
        out.faces[f].surface = Arc::new(surface);
        out.faces[f].loops = loops;
        changed = true;
    }
    if !changed {
        return None;
    }
    // Without the edges and vertices no face has any more.
    let mut edge_used = vec![false; out.edges.len()];
    for step in out.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        if let Walk::Edge { edge, .. } = step.walk {
            edge_used[edge] = true;
        }
    }
    let mut vertex_used = vec![false; out.vertices.len()];
    for (edge, used) in out.edges.iter().zip(&edge_used) {
        if *used {
            vertex_used[edge.ends[0]] = true;
            vertex_used[edge.ends[1]] = true;
        }
    }
    for step in out.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        if let Walk::Pole { vertex, .. } = step.walk {
            vertex_used[vertex] = true;
        }
    }
    let renumber = |used: &[bool]| -> Vec<usize> {
        let mut next = 0;
        used.iter()
            .map(|u| {
                let here = next;
                next += usize::from(*u);
                here
            })
            .collect()
    };
    let (edge_at, vertex_at) = (renumber(&edge_used), renumber(&vertex_used));
    let mut k = 0;
    out.edges.retain(|_| {
        k += 1;
        edge_used[k - 1]
    });
    let mut k = 0;
    out.vertices.retain(|_| {
        k += 1;
        vertex_used[k - 1]
    });
    for edge in &mut out.edges {
        edge.ends = edge.ends.map(|v| vertex_at[v]);
    }
    for face in &mut out.faces {
        face.smooth.retain(|e| edge_used[*e]);
        for e in &mut face.smooth {
            *e = edge_at[*e];
        }
        for step in face.loops.iter_mut().flatten() {
            step.walk = match step.walk {
                Walk::Edge { edge, reversed } => Walk::Edge {
                    edge: edge_at[edge],
                    reversed,
                },
                Walk::Pole { vertex, range } => Walk::Pole {
                    vertex: vertex_at[vertex],
                    range,
                },
            };
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bezier::curve_pieces;
    use shapecraft_core::world::topo::Curve3;

    #[test]
    fn an_edge_is_told_through_in_or_clear_of_a_flat_face() -> anyhow::Result<()> {
        let p = Point3::new;
        // The square from (0, 0, 0) to (2, 2, 0).
        let surface = NurbsSurface::bilinear(
            p(0.0, 0.0, 0.0),
            p(2.0, 0.0, 0.0),
            p(0.0, 2.0, 0.0),
            p(2.0, 2.0, 0.0),
        )?;
        let flat = Flat::of(&surface).expect("a flat face");
        let over = [(0.0, 1.0), (0.0, 1.0)];
        let meets = |a: Point3, b: Point3| -> anyhow::Result<Option<Met>> {
            let line = Curve3::line(a, b)?;
            Ok(flat.meets(&curve_pieces(&line, 0.0, 1.0), 1e-7, over))
        };
        // Through its middle; clear above it; through its plane beside it.
        let Some(Met::Through(found)) = meets(p(1.0, 1.0, -1.0), p(1.0, 1.0, 3.0))? else {
            anyhow::bail!("a line through the face is told");
        };
        assert_eq!(found.len(), 1);
        assert!((found[0].0 - 0.25).abs() < 1e-12 && found[0].2.distance(p(1.0, 1.0, 0.0)) < 1e-12);
        assert!(
            matches!(meets(p(0.0, 0.0, 1.0), p(2.0, 2.0, 2.0))?, Some(Met::Through(f)) if f.is_empty())
        );
        assert!(
            matches!(meets(p(5.0, 1.0, -1.0), p(5.0, 1.0, 1.0))?, Some(Met::Through(f)) if f.is_empty())
        );
        // Ending in it; lying in it; at its rim (not told).
        assert!(
            matches!(meets(p(1.0, 1.0, 0.0), p(1.0, 1.0, 1.0))?, Some(Met::Through(f)) if f.len() == 1)
        );
        assert!(matches!(
            meets(p(0.5, 0.5, 0.0), p(1.5, 1.0, 0.0))?,
            Some(Met::Lying)
        ));
        assert!(meets(p(2.0, 1.0, -1.0), p(2.0, 1.0, 1.0))?.is_none());
        Ok(())
    }
}
