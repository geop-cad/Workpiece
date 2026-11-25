//! Solids made here: a block for cutting with, a solid with a flat
//! face for every triangle of a mesh, and what is under a surface down
//! to a plane.

use std::sync::Arc;

use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::nurbs::NurbsError;
use shapecraft_core::world::topo::{Curve2, Curve3, NurbsSurface};

use crate::model::{Edge, Face, Solid, Use, Vertex, Walk};

/// The faces of a block, each as four corners counter-clockwise seen
/// from outside. Corner `i` is along the first, second and third axis
/// where bit 0, 1 and 2 of `i` is set.
const FACES: [[usize; 4]; 6] = [
    [0, 2, 3, 1],
    [4, 5, 7, 6],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 4, 6, 2],
    [1, 3, 7, 5],
];

/// The block from `corner` as far as `sizes` along each of `axes` (unit
/// vectors square to each other, the third the first two's cross), its
/// faces called `{id}/f0` … `{id}/f5`: `f1` is the one the third axis
/// points out of, `f0` the one opposite.
pub fn block(
    id: &str,
    corner: Point3,
    axes: [Point3; 3],
    sizes: [f64; 3],
) -> Result<Solid, NurbsError> {
    let at = |i: usize| {
        let mut p = corner;
        for (k, axis) in axes.iter().enumerate() {
            if i >> k & 1 == 1 {
                p += *axis * sizes[k];
            }
        }
        p
    };
    let tolerance = sizes.iter().fold(0.0_f64, |a, b| a.max(b.abs())) * 1e-12;
    let mut solid = Solid::default();
    for i in 0..8 {
        solid.vertices.push(Vertex {
            point: at(i),
            tolerance,
            name: None,
        });
    }
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let uv = [
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(1.0, 1.0),
        Point2::new(0.0, 1.0),
    ];
    for (f, corners) in FACES.iter().enumerate() {
        let [a, b, c, d] = corners.map(at);
        let surface = NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            vec![a, d, b, c],
            None,
        )?;
        let mut walk = Vec::new();
        for k in 0..4 {
            let (from, to) = (corners[k], corners[(k + 1) % 4]);
            let ends = (from.min(to), from.max(to));
            let edge = match edges.iter().position(|e| *e == ends) {
                Some(edge) => edge,
                None => {
                    edges.push(ends);
                    solid.edges.push(Edge {
                        curve: Arc::new(Curve3::line(at(ends.0), at(ends.1))?),
                        range: (0.0, 1.0),
                        ends: [ends.0, ends.1],
                        tolerance,
                        name: None,
                        also: Vec::new(),
                    });
                    edges.len() - 1
                }
            };
            // (Its curve in (u, v) runs the edge's own way.)
            let reversed = from > to;
            let (first, last) = if reversed {
                (uv[(k + 1) % 4], uv[k])
            } else {
                (uv[k], uv[(k + 1) % 4])
            };
            walk.push(Use {
                walk: Walk::Edge { edge, reversed },
                pcurve: Arc::new(Curve2::line(first, last)?),
            });
        }
        solid.faces.push(Face {
            surface: Arc::new(surface),
            outward: true,
            loops: vec![walk],
            tolerance,
            name: Some(format!("{id}/f{f}")),
            also: Vec::new(),
            smooth: Vec::new(),
        });
    }
    Ok(solid)
}

/// The solid a closed mesh bounds: `triangles` (each three of `points`,
/// counter-clockwise seen from outside) as flat faces, those that lie in
/// one plane beside each other made one. Its faces are called
/// `{id}/f0`, `{id}/f1`, …. An error for a mesh that is not closed: an
/// edge of it not between exactly two triangles that walk it opposite
/// ways.
pub fn faceted(
    id: &str,
    points: &[Point3],
    triangles: &[[usize; 3]],
) -> Result<Solid, crate::Refusal> {
    let failed = |e: NurbsError| crate::Refusal::Degenerate(e.to_string());
    use std::collections::HashMap;
    if triangles.is_empty() {
        return Err(crate::Refusal::Open("the mesh has no triangles".into()));
    }
    let (lo, hi) = points.iter().fold(
        (
            Point3::splat(f64::INFINITY),
            Point3::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let tolerance = (hi - lo).length() * 1e-10;
    let mut solid = Solid::default();
    for &point in points {
        solid.vertices.push(Vertex {
            point,
            tolerance,
            name: None,
        });
    }
    // Every edge, with how often it is walked each way.
    let mut edges: HashMap<(usize, usize), (usize, [usize; 2])> = HashMap::new();
    let uv = [
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(0.0, 1.0),
    ];
    for (f, corners) in triangles.iter().enumerate() {
        let [a, b, c] = corners.map(|i| points[i]);
        let surface = NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            vec![a, c, b, b + c - a],
            None,
        )
        .map_err(failed)?;
        let mut walk = Vec::new();
        for k in 0..3 {
            let (from, to) = (corners[k], corners[(k + 1) % 3]);
            let ends = (from.min(to), from.max(to));
            let reversed = from > to;
            let next = solid.edges.len();
            let (edge, walked) = edges.entry(ends).or_insert((next, [0, 0]));
            if *edge == next {
                solid.edges.push(Edge {
                    curve: Arc::new(Curve3::line(points[ends.0], points[ends.1]).map_err(failed)?),
                    range: (0.0, 1.0),
                    ends: [ends.0, ends.1],
                    tolerance,
                    name: None,
                    also: Vec::new(),
                });
            }
            walked[usize::from(reversed)] += 1;
            let (first, last) = if reversed {
                (uv[(k + 1) % 3], uv[k])
            } else {
                (uv[k], uv[(k + 1) % 3])
            };
            walk.push(Use {
                walk: Walk::Edge {
                    edge: *edge,
                    reversed,
                },
                pcurve: Arc::new(Curve2::line(first, last).map_err(failed)?),
            });
        }
        solid.faces.push(Face {
            surface: Arc::new(surface),
            outward: true,
            loops: vec![walk],
            tolerance,
            name: Some(format!("{id}/f{f}")),
            also: Vec::new(),
            smooth: Vec::new(),
        });
    }
    let open = edges
        .values()
        .filter(|(_, walked)| *walked != [1, 1])
        .count();
    if open != 0 {
        return Err(crate::Refusal::Open(format!(
            "the mesh is not closed: {open} of its edges are not between two triangles"
        )));
    }
    // (Vertices no triangle uses are not the solid's.)
    crate::tidy::tidy(&mut solid);
    Ok(solid)
}

/// The solid between `top` and its shadow on the plane through `origin`
/// across `normal`: everything straight under the surface, down to the
/// plane. The surface has to be all on the side of the plane `normal`
/// points to, end at its sides (clamped knots), and not fold over as
/// seen along the normal. Its faces are called `{id}/top`, `{id}/bottom`
/// and `{id}/side/e0` … `e3` (under the sides of the surface where v is
/// least, u is most, v is most and u is least).
pub fn under(
    id: &str,
    top: &NurbsSurface,
    origin: Point3,
    normal: Point3,
) -> Result<Solid, crate::Refusal> {
    let refuse = |why: &str| crate::Refusal::Degenerate(why.to_owned());
    let failed = |e: NurbsError| crate::Refusal::Degenerate(e.to_string());
    let normal = normal
        .try_normalize()
        .ok_or_else(|| refuse("a plane has a normal"))?;
    let (du, dv) = top.degrees();
    let (ku, kv) = top.knots();
    let (nu, nv) = top.counts();
    let clamped = |degree: usize, knots: &[f64], count: usize| {
        knots[0] == knots[degree] && knots[count] == knots[count + degree]
    };
    if !clamped(du, ku, nu) || !clamped(dv, kv, nv) {
        return Err(refuse("the surface does not end at its sides"));
    }
    let height = |p: Point3| (p - origin).dot(normal);
    let size = top
        .control_points()
        .iter()
        .fold(0.0_f64, |m, p| m.max(p.distance(top.control_point(0, 0))));
    if top
        .control_points()
        .iter()
        .any(|p| height(*p) <= size * 1e-9)
    {
        return Err(refuse(
            "the surface comes down to the plane: it has to be all above it",
        ));
    }
    let ((u0, u1), (v0, v1)) = top.domain();
    // Which way up the surface lies: everywhere the same, or it folds.
    let mut up = 0;
    for i in 0..=8 {
        for j in 0..=8 {
            let (u, v) = (
                u0 + (u1 - u0) * (f64::from(i) + 0.5) / 9.0,
                v0 + (v1 - v0) * (f64::from(j) + 0.5) / 9.0,
            );
            let (_, a, b) = top.derivatives_at(u, v);
            up += i32::from(a.cross(b).dot(normal) > 0.0);
        }
    }
    if up != 0 && up != 81 {
        return Err(refuse("the surface folds over as seen from the plane"));
    }
    let up = up == 81;
    let shadow: Vec<Point3> = top
        .control_points()
        .iter()
        .map(|p| *p - normal * height(*p))
        .collect();
    let bottom = NurbsSurface::new(
        du,
        dv,
        ku.to_vec(),
        kv.to_vec(),
        nu,
        nv,
        shadow,
        Some(top.weights().to_vec()),
    )
    .map_err(failed)?;
    let tolerance = size * 1e-12;
    let mut solid = Solid::default();
    // The corners in (u, v), counter-clockwise; the shadow's, then the
    // surface's.
    let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)];
    for surface in [&bottom, top] {
        for (u, v) in corners {
            solid.vertices.push(Vertex {
                point: surface.point_at(u, v),
                tolerance,
                name: None,
            });
        }
    }
    // The sides of a surface: its curve along each, with the degree,
    // the knots and the corners (of the four) it runs from and to.
    let side = |surface: &NurbsSurface, k: usize| -> Result<Curve3, NurbsError> {
        let (points, weights): (Vec<Point3>, Vec<f64>) = match k {
            0 | 2 => {
                let j = if k == 0 { 0 } else { nv - 1 };
                (0..nu)
                    .map(|i| (surface.control_point(i, j), surface.weights()[i * nv + j]))
                    .unzip()
            }
            _ => {
                let i = if k == 1 { nu - 1 } else { 0 };
                (0..nv)
                    .map(|j| (surface.control_point(i, j), surface.weights()[i * nv + j]))
                    .unzip()
            }
        };
        if k.is_multiple_of(2) {
            Curve3::new(du, ku.to_vec(), points, Some(weights))
        } else {
            Curve3::new(dv, kv.to_vec(), points, Some(weights))
        }
    };
    let ends = [[0, 1], [1, 2], [3, 2], [0, 3]];
    let range = |k: usize| {
        if k.is_multiple_of(2) {
            (u0, u1)
        } else {
            (v0, v1)
        }
    };
    // Edges 0–3 around the shadow, 4–7 around the surface, 8–11 up
    // from each corner.
    for (level, surface) in [&bottom, top].into_iter().enumerate() {
        for (k, ends) in ends.iter().enumerate() {
            solid.edges.push(Edge {
                curve: Arc::new(side(surface, k).map_err(failed)?),
                range: range(k),
                ends: ends.map(|c| c + 4 * level),
                tolerance,
                name: None,
                also: Vec::new(),
            });
        }
    }
    for c in 0..4 {
        let (a, b) = (solid.vertices[c].point, solid.vertices[c + 4].point);
        solid.edges.push(Edge {
            curve: Arc::new(Curve3::line(a, b).map_err(failed)?),
            range: (0.0, 1.0),
            ends: [c, c + 4],
            tolerance,
            name: None,
            also: Vec::new(),
        });
    }
    // A straight line in (u, v) over the parameters `range`.
    let line = |from: (f64, f64), to: (f64, f64), range: (f64, f64)| {
        Curve2::new(
            1,
            vec![range.0, range.0, range.1, range.1],
            vec![Point2::new(from.0, from.1), Point2::new(to.0, to.1)],
            None,
        )
        .map(Arc::new)
    };
    for (level, surface) in [&bottom, top].into_iter().enumerate() {
        let mut walk = Vec::new();
        for (k, ends) in ends.iter().enumerate() {
            walk.push(Use {
                walk: Walk::Edge {
                    edge: k + 4 * level,
                    reversed: k >= 2,
                },
                pcurve: line(corners[ends[0]], corners[ends[1]], range(k)).map_err(failed)?,
            });
        }
        solid.faces.push(Face {
            surface: Arc::new(surface.clone()),
            outward: up == (level == 1),
            loops: vec![walk],
            tolerance,
            name: Some(format!("{id}/{}", ["bottom", "top"][level])),
            also: Vec::new(),
            smooth: Vec::new(),
        });
    }
    for (k, ends) in ends.iter().enumerate() {
        let (low, high) = (&solid.edges[k].curve, &solid.edges[k + 4].curve);
        let count = low.control_points().len();
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for i in 0..count {
            for curve in [low, high] {
                points.push(curve.control_points()[i]);
                weights.push(curve.weights()[i]);
            }
        }
        let surface = NurbsSurface::new(
            low.degree(),
            1,
            low.knots().to_vec(),
            vec![0.0, 0.0, 1.0, 1.0],
            count,
            2,
            points,
            Some(weights),
        )
        .map_err(failed)?;
        let (s0, s1) = range(k);
        let step = |edge: usize, reversed: bool, pcurve: Arc<Curve2>| Use {
            walk: Walk::Edge { edge, reversed },
            pcurve,
        };
        let walk = vec![
            step(
                k,
                false,
                line((s0, 0.0), (s1, 0.0), (s0, s1)).map_err(failed)?,
            ),
            step(
                8 + ends[1],
                false,
                line((s1, 0.0), (s1, 1.0), (0.0, 1.0)).map_err(failed)?,
            ),
            step(
                k + 4,
                true,
                line((s0, 1.0), (s1, 1.0), (s0, s1)).map_err(failed)?,
            ),
            step(
                8 + ends[0],
                true,
                line((s0, 0.0), (s0, 1.0), (0.0, 1.0)).map_err(failed)?,
            ),
        ];
        solid.faces.push(Face {
            surface: Arc::new(surface),
            outward: up == (k < 2),
            loops: vec![walk],
            tolerance,
            name: Some(format!("{id}/side/e{k}")),
            also: Vec::new(),
            smooth: Vec::new(),
        });
    }
    Ok(solid)
}
