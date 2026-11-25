//! A surface over a hole of any number of sides: fitted to the curves
//! around it, and running on from the faces beside them — tangent to
//! them (G1), or bending as they do too (G2) — where that is asked.
//!
//! The method: the sides are laid around a regular polygon in the
//! (u, v) of one bicubic surface (four sides: around its square), and
//! the surface's control points are found by least squares — through
//! the sides, leaving each across as the face beside it does, and as
//! smooth as it can be (its control net bending as little as it can)
//! elsewhere. The face is that surface trimmed by the sides
//! themselves, each the polygon's side in (u, v): the sides stay the
//! curves they are (the faces beside them share them), and what the
//! surface strays from them is their tolerance.

use std::sync::Arc;

use anyhow::{Context, ensure};
use workpiece_boolean::Solid;
use workpiece_boolean::model::{Edge, Face, Use, Vertex, Walk};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve2, Curve3, NurbsSurface};

/// How the surface leaves a side, at a place on it: across the side
/// (away from the face beside it, in that face's tangent plane), the
/// face's normal, and how much the face bends that way (its normal
/// curvature along `away`, towards `normal` positive).
#[derive(Debug, Clone, Copy)]
pub struct Across {
    /// The way out of the face beside it, square to the side.
    pub away: Point3,
    /// The face's normal there.
    pub normal: Point3,
    /// How much the face bends along `away`.
    pub bend: f64,
}

/// A side of the hole: a curve, the part of it that counts, and how the
/// surface leaves it (none: only through it).
pub struct Side<'a> {
    /// The curve.
    pub curve: Arc<Curve3>,
    /// The part of it.
    pub range: (f64, f64),
    /// How the surface leaves it, at a parameter of the curve; and
    /// whether it bends as the face does too (G2) or only leaves along
    /// it (G1).
    pub across: Option<(&'a dyn Fn(f64) -> Across, bool)>,
    /// Its name, for the edge.
    pub name: Option<String>,
}

/// The values of the cubic B-spline basis over `knots` at `t`, and of
/// its first and second derivatives, as many as there are control
/// points.
fn cubic(knots: &[f64], t: f64) -> [Vec<f64>; 3] {
    let n = knots.len() - 4;
    let mut out = [vec![0.0; n], vec![0.0; n], vec![0.0; n]];
    // (The values of degree 0 to 3, then derivatives from degree 2 and 1.)
    let span = (3..n).rev().find(|&s| knots[s] <= t).unwrap_or(3);
    let ratio = |x: f64, y: f64| if y == 0.0 { 0.0 } else { x / y };
    let mut by: Vec<Vec<f64>> = vec![vec![0.0; knots.len() - 1]];
    by[0][span] = 1.0;
    for p in 1..=3 {
        let below = &by[p - 1];
        let next: Vec<f64> = (0..knots.len() - 1 - p)
            .map(|i| {
                ratio(t - knots[i], knots[i + p] - knots[i]) * below[i]
                    + ratio(knots[i + p + 1] - t, knots[i + p + 1] - knots[i + 1]) * below[i + 1]
            })
            .collect();
        by.push(next);
    }
    let of = |p: usize, i: usize| by[p].get(i).copied().unwrap_or(0.0);
    let first = |p: usize, i: usize| {
        p as f64
            * (ratio(of(p - 1, i), knots[i + p] - knots[i])
                - ratio(of(p - 1, i + 1), knots[i + p + 1] - knots[i + 1]))
    };
    for i in 0..n {
        out[0][i] = of(3, i);
        out[1][i] = first(3, i);
        out[2][i] = 3.0
            * (ratio(first(2, i), knots[i + 3] - knots[i])
                - ratio(first(2, i + 1), knots[i + 4] - knots[i + 1]));
    }
    out
}

/// How much the surface is held to leave a side the way the face
/// beside it does, beside it lying in that face's tangent plane (which
/// is held as closely as the places on the sides): it picks which way
/// along the plane, and how fast.
const LEAVING: f64 = 0.05;

/// How much the places on the sides are held, beside the rest.
const PLACE: f64 = 30.0;

/// How smooth the surface is held to be within the sides.
const SMOOTH: f64 = 1e-2;

/// Solves the least-squares problem of `rows` (each its coefficients,
/// by unknown, and what it is to come to): the unknowns.
fn least_squares(unknowns: usize, rows: &[(Vec<(usize, f64)>, f64)]) -> anyhow::Result<Vec<f64>> {
    let n = unknowns;
    let mut normal = vec![vec![0.0; n]; n];
    let mut right = vec![0.0; n];
    for (coefficients, wanted) in rows {
        for &(i, a) in coefficients {
            for &(j, b) in coefficients {
                normal[i][j] += a * b;
            }
            right[i] += a * wanted;
        }
    }
    // (Symmetric and positive: elimination without pivots, rows below
    // the diagonal skipped where they are nothing already.)
    for c in 0..n {
        let pivot = normal[c][c];
        ensure!(
            pivot.abs() > 1e-300,
            "the surface over the hole could not be fitted"
        );
        let (top, below) = normal.split_at_mut(c + 1);
        let top = &top[c];
        for (r, row) in below.iter_mut().enumerate() {
            let m = row[c] / pivot;
            if m == 0.0 {
                continue;
            }
            for k in c..n {
                row[k] -= m * top[k];
            }
            right[c + 1 + r] -= m * right[c];
        }
    }
    let mut x = vec![0.0; n];
    for c in (0..n).rev() {
        let mut sum = right[c];
        for k in c + 1..n {
            sum -= normal[c][k] * x[k];
        }
        x[c] = sum / normal[c][c];
    }
    Ok(x)
}

/// The order `sides` are walked in around their hole, each from where
/// the one before ends (and whether it is walked against its curve);
/// and how large the hole is.
fn walked(sides: &[Side<'_>]) -> anyhow::Result<(Vec<(usize, bool)>, f64)> {
    let n = sides.len();
    ensure!(n >= 1, "a hole has a side");
    let end = |k: usize, last: bool| {
        let (a, b) = sides[k].range;
        sides[k].curve.point_at(if last { b } else { a })
    };
    // (How far its ends are apart; of a hole of one side, its places.)
    let mut size = (0..n)
        .flat_map(|k| [end(k, false), end(k, true)])
        .flat_map(|p| (0..n).map(move |k| (p, k)))
        .map(|(p, k)| p.distance(end(k, false)))
        .fold(0.0, f64::max);
    if n == 1 {
        let (a, b) = sides[0].range;
        for i in 0..=16 {
            let p = sides[0].curve.point_at(a + (b - a) * f64::from(i) / 16.0);
            size = size.max(p.distance(end(0, false)));
        }
    }
    ensure!(size > 0.0, "the sides have some length");
    let near = size * 1e-6;
    let mut around: Vec<(usize, bool)> = vec![(0, false)];
    let mut at = end(0, true);
    let mut left: Vec<usize> = (1..n).collect();
    while !left.is_empty() {
        let next = left
            .iter()
            .position(|&k| end(k, false).distance(at) <= near || end(k, true).distance(at) <= near)
            .context("the sides do not meet end to end around one hole")?;
        let k = left.remove(next);
        let flipped = end(k, false).distance(at) > near;
        at = end(k, !flipped);
        around.push((k, flipped));
    }
    ensure!(
        at.distance(end(0, false)) <= near,
        "the sides do not close around one hole"
    );
    Ok((around, size))
}

/// The flat face over the hole `sides` go around, if they all lie in
/// one plane (a hole of one side too: a closed curve), as a sheet of one
/// face named `{id}/face`; none if they do not.
pub fn flat_over(sides: &[Side<'_>], id: &str) -> anyhow::Result<Option<Solid>> {
    let (around, size) = walked(sides)?;
    let within = size * 1e-7;
    // Places along the hole, as it is walked.
    let mut places: Vec<Point3> = Vec::new();
    for &(k, flipped) in &around {
        let (a, b) = sides[k].range;
        for i in 0..16 {
            let f = f64::from(i) / 16.0;
            let f = if flipped { 1.0 - f } else { f };
            places.push(sides[k].curve.point_at(a + (b - a) * f));
        }
    }
    let origin = places[0];
    let normal: Point3 = (0..places.len())
        .map(|k| (places[k] - origin).cross(places[(k + 1) % places.len()] - origin))
        .sum();
    let Some(normal) = normal.try_normalize() else {
        return Ok(None);
    };
    if places
        .iter()
        .any(|p| (*p - origin).dot(normal).abs() > within)
    {
        return Ok(None);
    }
    // (Its normal is the way the hole is walked round: the face is on
    // the left of its loop.)
    let x = normal.any_orthonormal_vector();
    let y = normal.cross(x);
    let flat = |p: Point3| Point2::new((p - origin).dot(x), (p - origin).dot(y));
    let (mut low, mut high) = (Point2::INFINITY, Point2::NEG_INFINITY);
    for side in sides {
        for p in side.curve.control_points() {
            (low, high) = (low.min(flat(*p)), high.max(flat(*p)));
        }
    }
    let pad = (high - low).length() * 0.1 + within;
    let (low, high) = (low - Point2::splat(pad), high + Point2::splat(pad));
    let place = |p: Point2| origin + x * p.x + y * p.y;
    let surface = NurbsSurface::new(
        1,
        1,
        vec![low.x, low.x, high.x, high.x],
        vec![low.y, low.y, high.y, high.y],
        2,
        2,
        vec![
            place(low),
            place(Point2::new(low.x, high.y)),
            place(Point2::new(high.x, low.y)),
            place(high),
        ],
        None,
    )?;
    let n = around.len();
    let tolerance = within.max(1e-9);
    let mut out = Solid::default();
    for &(k, flipped) in &around {
        let (a, b) = sides[k].range;
        out.vertices.push(Vertex {
            point: sides[k].curve.point_at(if flipped { b } else { a }),
            tolerance: tolerance * 2.0,
            name: None,
        });
    }
    let mut walk = Vec::new();
    for (s, &(k, flipped)) in around.iter().enumerate() {
        let side = &sides[k];
        let (from, to) = (s, (s + 1) % n);
        let curve = &side.curve;
        let pcurve = Curve2::new(
            curve.degree(),
            curve.knots().to_vec(),
            curve.control_points().iter().map(|p| flat(*p)).collect(),
            Some(curve.weights().to_vec()),
        )?;
        out.edges.push(Edge {
            curve: curve.clone(),
            range: side.range,
            ends: if flipped { [to, from] } else { [from, to] },
            tolerance,
            name: side.name.clone(),
            also: Vec::new(),
        });
        walk.push(Use {
            walk: Walk::Edge {
                edge: s,
                reversed: flipped,
            },
            pcurve: Arc::new(pcurve),
        });
    }
    out.faces.push(Face {
        surface: Arc::new(surface),
        outward: true,
        loops: vec![walk],
        tolerance,
        name: Some(format!("{id}/face")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    Ok(Some(out))
}

/// The surface over the hole `sides` go around (they meet end to end,
/// in any order and either way along), as a sheet of one face named
/// `{id}/face`. See the module's documentation.
pub fn filled_over(sides: &[Side<'_>], id: &str) -> anyhow::Result<Solid> {
    let n = sides.len();
    ensure!(n >= 2, "a hole has two sides or more");
    let (around, size) = walked(sides)?;
    // Where along side `s` (in walking order) its parameter `f` (0 to 1
    // as it is walked) is, and the curve's own parameter there.
    let param = |s: usize, f: f64| {
        let (k, flipped) = around[s];
        let (a, b) = sides[k].range;
        let f = if flipped { 1.0 - f } else { f };
        a + (b - a) * f
    };
    let on = |s: usize, f: f64| sides[around[s].0].curve.point_at(param(s, f));
    // The polygon in (u, v), around counter-clockwise: the square for
    // four sides, else a regular polygon in it.
    let corners: Vec<Point2> = if n == 4 {
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 1.0),
        ]
    } else {
        (0..n)
            .map(|k| {
                let a = -std::f64::consts::FRAC_PI_2 - std::f64::consts::PI / n as f64
                    + std::f64::consts::TAU * k as f64 / n as f64;
                Point2::new(0.5 + 0.5 * a.cos(), 0.5 + 0.5 * a.sin())
            })
            .collect()
    };
    let uv = |s: usize, f: f64| corners[s] + (corners[(s + 1) % n] - corners[s]) * f;
    // How long a step in (u, v) is in space, about.
    let mut long = 0.0;
    for s in 0..n {
        for k in 0..32 {
            long += on(s, f64::from(k) / 32.0).distance(on(s, f64::from(k + 1) / 32.0));
        }
    }
    let flat: f64 = (0..n)
        .map(|s| corners[s].distance(corners[(s + 1) % n]))
        .sum();
    let scale = long / flat;
    let mut best: Option<(f64, NurbsSurface)> = None;
    for count in [8usize, 12, 16, 20] {
        let spans = count - 3;
        let mut knots = vec![0.0; 4];
        knots.extend((1..spans).map(|k| k as f64 / spans as f64));
        knots.extend([1.0; 4]);
        let index = |i: usize, j: usize| i * count + j;
        let h = 1.0 / spans as f64;
        let cells = count * count;
        // (The unknowns: the control points' x, then their y, then z.)
        let mut rows: Vec<(Vec<(usize, f64)>, f64)> = Vec::new();
        let each =
            |rows: &mut Vec<(Vec<(usize, f64)>, f64)>, row: &[(usize, f64)], wanted: Point3| {
                for (c, w) in [wanted.x, wanted.y, wanted.z].into_iter().enumerate() {
                    let shifted = row.iter().map(|&(i, a)| (c * cells + i, a)).collect();
                    rows.push((shifted, w));
                }
            };
        let samples = 6 * count;
        for s in 0..n {
            let corner = corners[(s + 1) % n] - corners[s];
            let inward = Point2::new(-corner.y, corner.x).normalize();
            let across = sides[around[s].0].across;
            for k in 0..=samples {
                let f = f64::from(k as u32) / samples as f64;
                let place = uv(s, f);
                let [bu, du, ddu] = cubic(&knots, place.x);
                let [bv, dv, ddv] = cubic(&knots, place.y);
                let nonzero = |b: &[f64], d: &[f64], dd: &[f64]| -> Vec<usize> {
                    (0..b.len())
                        .filter(|&i| b[i] != 0.0 || d[i] != 0.0 || dd[i] != 0.0)
                        .collect()
                };
                let (all_u, all_v) = (nonzero(&bu, &du, &ddu), nonzero(&bv, &dv, &ddv));
                let p = on(s, f);
                let mut row = Vec::new();
                for &i in &all_u {
                    for &j in &all_v {
                        let value = bu[i] * bv[j];
                        if value != 0.0 {
                            row.push((index(i, j), value * PLACE));
                        }
                    }
                }
                each(&mut rows, &row, p * PLACE);
                let Some((across, bends)) = across else {
                    continue;
                };
                // Across the side, into the hole: in the tangent plane
                // of the face beside it, and (a little) the way it
                // leaves; in step lengths of the control net.
                let a = across(param(s, f));
                let (nu, nv) = (inward.x, inward.y);
                // (Less and less towards the corners: where the faces
                // beside two sides lean to each other no surface leaves
                // both as they do, and its place counts first.)
                let fade = (f.min(1.0 - f) / 0.2).min(1.0);
                let fade = fade * fade * (3.0 - 2.0 * fade);
                let held = PLACE * fade;
                let mut first = Vec::new();
                for &i in &all_u {
                    for &j in &all_v {
                        let value = nu * du[i] * bv[j] + nv * bu[i] * dv[j];
                        if value != 0.0 {
                            first.push((index(i, j), value * h));
                        }
                    }
                }
                let normal = a.normal.normalize_or_zero();
                let lying: Vec<(usize, f64)> = (0..3)
                    .flat_map(|c| first.iter().map(move |&(i, v)| (c * cells + i, v)))
                    .zip((0..3).flat_map(|c| first.iter().map(move |_| normal[c])))
                    .map(|((i, v), n)| (i, v * n * held))
                    .collect();
                rows.push((lying, 0.0));
                let leaving: Vec<(usize, f64)> =
                    first.iter().map(|&(i, v)| (i, v * LEAVING)).collect();
                each(&mut rows, &leaving, a.away * (scale * h * LEAVING));
                if bends {
                    // Bending across the side as the face does: what it
                    // does along its normal.
                    let mut second = Vec::new();
                    for &i in &all_u {
                        for &j in &all_v {
                            let value = nu * nu * ddu[i] * bv[j]
                                + 2.0 * nu * nv * du[i] * dv[j]
                                + nv * nv * bu[i] * ddv[j];
                            if value != 0.0 {
                                second.push((index(i, j), value * h * h));
                            }
                        }
                    }
                    let row: Vec<(usize, f64)> = (0..3)
                        .flat_map(|c| {
                            second
                                .iter()
                                .map(move |&(i, v)| (c * cells + i, v * normal[c] * held))
                        })
                        .collect();
                    rows.push((row, a.bend * scale * scale * h * h * held));
                }
            }
        }
        // As smooth as it can be elsewhere: the control net bending
        // little, a little weight to it. (Outside the polygon, where
        // nothing else holds it, all the more: it runs on as plainly as
        // it can.)
        let inside = |p: Point2| {
            (0..n).all(|s| {
                let (a, b) = (corners[s], corners[(s + 1) % n]);
                (b - a).perp_dot(p - a) >= -1e-9
            })
        };
        let greville = |i: usize| knots[i + 1..i + 4].iter().sum::<f64>() / 3.0;
        for i in 0..count {
            for j in 0..count {
                let smooth = if inside(Point2::new(greville(i), greville(j))) {
                    SMOOTH
                } else {
                    0.3
                };
                let mut bent: Vec<Vec<(usize, f64)>> = Vec::new();
                if i > 0 && i + 1 < count {
                    bent.push(vec![
                        (index(i - 1, j), smooth),
                        (index(i, j), -2.0 * smooth),
                        (index(i + 1, j), smooth),
                    ]);
                }
                if j > 0 && j + 1 < count {
                    bent.push(vec![
                        (index(i, j - 1), smooth),
                        (index(i, j), -2.0 * smooth),
                        (index(i, j + 1), smooth),
                    ]);
                }
                if i + 1 < count && j + 1 < count {
                    let w = smooth * std::f64::consts::SQRT_2;
                    bent.push(vec![
                        (index(i, j), w),
                        (index(i + 1, j), -w),
                        (index(i, j + 1), -w),
                        (index(i + 1, j + 1), w),
                    ]);
                }
                for row in bent {
                    each(&mut rows, &row, Point3::ZERO);
                }
            }
        }
        let control = least_squares(3 * cells, &rows)?;
        let points: Vec<Point3> = (0..cells)
            .map(|i| Point3::new(control[i], control[cells + i], control[2 * cells + i]))
            .collect();
        let surface = NurbsSurface::new(3, 3, knots.clone(), knots, count, count, points, None)?;
        let mut worst = 0.0_f64;
        for s in 0..n {
            for k in 0..=64 {
                let f = f64::from(k) / 64.0;
                let place = uv(s, f);
                worst = worst.max(surface.point_at(place.x, place.y).distance(on(s, f)));
            }
        }
        let better = best.as_ref().is_none_or(|(was, _)| worst < *was);
        if better {
            best = Some((worst, surface));
        }
        if worst <= size * 1e-6 {
            break;
        }
    }
    let (worst, surface) = best.context("the surface over the hole could not be fitted")?;
    let surface = Arc::new(surface);
    // The face: the surface within the sides, each the polygon's side.
    let tolerance = (worst * 1.5).max(size * 1e-9);
    let mut out = Solid::default();
    for s in 0..n {
        out.vertices.push(Vertex {
            point: on(s, 0.0),
            tolerance: tolerance * 2.0,
            name: None,
        });
    }
    let mut walk = Vec::new();
    for (s, &(k, flipped)) in around.iter().enumerate() {
        let side = &sides[k];
        let (from, to) = (s, (s + 1) % n);
        // (The edge as the curve runs; its ends, and its curve in (u, v)
        // by the curve's own parameter.)
        let ends = if flipped { [to, from] } else { [from, to] };
        let (start, stop) = if flipped {
            (corners[to], corners[from])
        } else {
            (corners[from], corners[to])
        };
        out.edges.push(Edge {
            curve: side.curve.clone(),
            range: side.range,
            ends,
            tolerance,
            name: side.name.clone(),
            also: Vec::new(),
        });
        let (a, b) = side.range;
        let pcurve = Curve2::new(1, vec![a, a, b, b], vec![start, stop], None)?;
        walk.push(Use {
            walk: Walk::Edge {
                edge: s,
                reversed: flipped,
            },
            pcurve: Arc::new(pcurve),
        });
    }
    out.faces.push(Face {
        surface,
        outward: true,
        loops: vec![walk],
        tolerance,
        name: Some(format!("{id}/face")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    Ok(out)
}
