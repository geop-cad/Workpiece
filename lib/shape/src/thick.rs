//! What growing, shrinking, shelling and thickening are made of: the
//! surface a fixed way off another, and the slab between a face and
//! that.
//!
//! A body grown by `d` is every point within `d` of it, shrunk every
//! point at least `d` inside it, and a shell's wall every point of the
//! body within `d` of its faces: all of them are the body with, or
//! without, the points within `d` of its faces, edges and corners. Those
//! are built here — a face's *slab* (between it and the surface `d` off
//! it), an edge's *tube*, a corner's ball — and combined as bodies are.
//!
//! The surface off another is fitted in that surface's own splines (its
//! degrees, knots and weights), with more knots wherever that is not
//! close enough: the surface off a plane, a cylinder, a ball, a cone or
//! a torus is one of the same kind and is met exactly, and any other as
//! closely as asked.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, ensure};
use workpiece_boolean::Solid;
use workpiece_boolean::model::{Edge, Face, Use, Vertex, Walk};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve2, Curve3, NurbsSurface};

use crate::sweep::{basis, through};

/// The unit normal of `surface` at (u, v), the way ∂u × ∂v points; where
/// the surface collapses to a point (a pole), the normal a little way in
/// from it.
pub fn normal(surface: &NurbsSurface, uv: Point2) -> Point3 {
    let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
    let n = du.cross(dv);
    let ((u0, u1), (v0, v1)) = surface.domain();
    let span = Point2::new(u1 - u0, v1 - v0);
    let large = (du * span.x).length().max((dv * span.y).length());
    if n.length() * span.x * span.y > 1e-9 * large * large {
        return n.normalize();
    }
    // (From two places on the way to the middle, carried back to the
    // pole: what the normal comes to there.)
    let middle = Point2::new(u0 + u1, v0 + v1) / 2.0;
    let at = |k: f64| {
        let near = uv + (middle - uv) * (1e-5 * k);
        let (_, du, dv) = surface.derivatives_at(near.x, near.y);
        du.cross(dv).normalize_or_zero()
    };
    (at(1.0) * 2.0 - at(2.0)).normalize_or_zero()
}

/// Where the control points of a spline of `degree` over `knots` weigh
/// most: as many places as it has control points.
fn sites(degree: usize, knots: &[f64]) -> Vec<f64> {
    let n = knots.len() - degree - 1;
    (0..n)
        .map(|j| knots[j + 1..=j + degree].iter().sum::<f64>() / degree as f64)
        .collect()
}

/// The matrix of a spline's basis functions at its [`sites`].
fn at_sites(degree: usize, knots: &[f64]) -> Vec<Vec<f64>> {
    let n = knots.len() - degree - 1;
    sites(degree, knots)
        .into_iter()
        .map(|t| {
            let mut row = vec![0.0; n];
            let (first, values) = basis(degree, knots, t);
            row[first..first + values.len()].copy_from_slice(&values);
            row
        })
        .collect()
}

/// The spans of `knots` that are not empty, each with the places in it
/// a fit is checked at.
fn checks(degree: usize, knots: &[f64]) -> Vec<(f64, f64, [f64; 2])> {
    let n = knots.len() - degree - 1;
    (degree..n)
        .filter(|&k| knots[k + 1] > knots[k])
        .map(|k| {
            let (a, b) = (knots[k], knots[k + 1]);
            (a, b, [a + (b - a) * 0.29, a + (b - a) * 0.71])
        })
        .collect()
}

/// The surface that is `surface` moved `off` along its normal ∂u × ∂v
/// everywhere, to within `within`.
pub fn offset_surface(
    surface: &NurbsSurface,
    off: &dyn Fn(Point3) -> f64,
    within: f64,
) -> anyhow::Result<NurbsSurface> {
    let moved = |u: f64, v: f64| {
        let at = surface.point_at(u, v);
        at + normal(surface, Point2::new(u, v)) * off(at)
    };
    refit(surface, &moved, within)
}

/// The surface that is at `moved(u, v)` over the (u, v) of `surface`,
/// to within `within`: over `surface`'s own knots, and more where it
/// takes them.
pub fn refit(
    surface: &NurbsSurface,
    moved: &dyn Fn(f64, f64) -> Point3,
    within: f64,
) -> anyhow::Result<NurbsSurface> {
    let (p, q) = surface.degrees();
    let (mut ku, mut kv) = (surface.knots().0.to_vec(), surface.knots().1.to_vec());
    // The surface's own weight as a function of (u, v): what its
    // weighted coordinates are scaled by.
    let (nu, nv) = surface.counts();
    let weigh = NurbsSurface::new(
        p,
        q,
        ku.clone(),
        kv.clone(),
        nu,
        nv,
        surface
            .weights()
            .iter()
            .map(|w| Point3::new(*w, 0.0, 0.0))
            .collect(),
        None,
    )?;
    let weight = |u: f64, v: f64| weigh.point_at(u, v).x;
    for _ in 0..12 {
        let (su, sv) = (sites(p, &ku), sites(q, &kv));
        // Along v at every u site, then along u.
        let wanted: Vec<Vec<[f64; 4]>> = sv
            .iter()
            .map(|&v| {
                su.iter()
                    .map(|&u| {
                        let (x, w) = (moved(u, v), weight(u, v));
                        [x.x * w, x.y * w, x.z * w, w]
                    })
                    .collect()
            })
            .collect();
        let rows = through(at_sites(q, &kv), wanted).context("a surface could not be moved")?;
        // (Now a row a v control point, a column a u site.)
        let columns: Vec<Vec<[f64; 4]>> = (0..su.len())
            .map(|i| rows.iter().map(|row| row[i]).collect())
            .collect();
        let solved = through(at_sites(p, &ku), columns).context("a surface could not be moved")?;
        // solved[i][j]: u control point i, v control point j.
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for column in &solved {
            for h in column {
                points.push(Point3::new(h[0], h[1], h[2]) / h[3]);
                weights.push(h[3]);
            }
        }
        let fitted = NurbsSurface::new(
            p,
            q,
            ku.clone(),
            kv.clone(),
            su.len(),
            sv.len(),
            points,
            Some(weights),
        )?;
        // Is it the moved surface between those places too?
        let (cu, cv) = (checks(p, &ku), checks(q, &kv));
        let (mut more_u, mut more_v) = (Vec::new(), Vec::new());
        for (a, b, us) in &cu {
            let off_here = cv.iter().any(|(_, _, vs)| {
                us.iter().any(|&u| {
                    vs.iter()
                        .any(|&v| fitted.point_at(u, v).distance(moved(u, v)) > within)
                })
            });
            if off_here {
                more_u.push((a + b) / 2.0);
            }
        }
        for (a, b, vs) in &cv {
            let off_here = cu.iter().any(|(_, _, us)| {
                us.iter().any(|&u| {
                    vs.iter()
                        .any(|&v| fitted.point_at(u, v).distance(moved(u, v)) > within)
                })
            });
            if off_here {
                more_v.push((a + b) / 2.0);
            }
        }
        if (more_u.is_empty() && more_v.is_empty()) || ku.len() + kv.len() > 600 {
            return Ok(fitted);
        }
        ku.extend(more_u);
        ku.sort_by(f64::total_cmp);
        kv.extend(more_v);
        kv.sort_by(f64::total_cmp);
    }
    anyhow::bail!("a surface could not be moved off itself closely enough")
}

/// The edge's own curve from `range.0` to `range.1` and the curve
/// `other` beside it, as two curves over the same knots (the edge's,
/// and more where `other` asks for them): what a ruled face between the
/// two is laid on.
fn pair(
    curve: &Curve3,
    range: (f64, f64),
    other: &dyn Fn(f64) -> Point3,
    within: f64,
) -> anyhow::Result<(Curve3, Curve3)> {
    let degree = curve.degree();
    let slack = (range.1 - range.0) * 1e-9;
    let mut knots = vec![range.0; degree + 1];
    let theirs = curve.knots();
    let mut k = 0;
    while k < theirs.len() {
        let same = theirs[k..].iter().take_while(|v| **v == theirs[k]).count();
        if theirs[k] > range.0 + slack && theirs[k] < range.1 - slack {
            knots.extend(std::iter::repeat_n(theirs[k], same.min(degree)));
        }
        k += same;
    }
    knots.extend(std::iter::repeat_n(range.1, degree + 1));
    let weigh = Curve3::new(
        degree,
        theirs.to_vec(),
        curve
            .weights()
            .iter()
            .map(|w| Point3::new(*w, 0.0, 0.0))
            .collect(),
        None,
    )?;
    let weight = |t: f64| weigh.point_at(t).x;
    let mut before = f64::INFINITY;
    for _ in 0..14 {
        let at = sites(degree, &knots);
        let fit = |of: &dyn Fn(f64) -> Point3| -> anyhow::Result<Curve3> {
            let wanted: Vec<Vec<[f64; 4]>> = at
                .iter()
                .map(|&t| {
                    let (x, w) = (of(t), weight(t));
                    vec![[x.x * w, x.y * w, x.z * w, w]]
                })
                .collect();
            let solved =
                through(at_sites(degree, &knots), wanted).context("an edge could not be moved")?;
            let (points, weights): (Vec<Point3>, Vec<f64>) = solved
                .iter()
                .map(|h| (Point3::new(h[0][0], h[0][1], h[0][2]) / h[0][3], h[0][3]))
                .unzip();
            Ok(Curve3::new(degree, knots.clone(), points, Some(weights))?)
        };
        let (own, beside) = (fit(&|t| curve.point_at(t))?, fit(other)?);
        let spans = checks(degree, &knots);
        let worst = spans
            .iter()
            .flat_map(|(_, _, ts)| ts.iter())
            .map(|&t| beside.point_at(t).distance(other(t)))
            .fold(0.0, f64::max);
        let off: Vec<(f64, f64)> = spans
            .into_iter()
            .filter(|(_, _, ts)| {
                ts.iter()
                    .any(|&t| beside.point_at(t).distance(other(t)) > within)
            })
            .map(|(a, b, _)| (a, b))
            .collect();
        let mut more: Vec<f64> = off.iter().map(|(a, b)| (a + b) / 2.0).collect();
        // (Off on both sides of a knot, what it is moved to has a kink
        // there — where the foot of a surface goes from one of its
        // pieces to the next: the curve is let to have one too, or no
        // number of knots about it brings it close.)
        for pair in off.windows(2) {
            let at = pair[0].1;
            let has = knots.iter().filter(|k| **k == at).count();
            if pair[1].0 == at && has < degree {
                more.extend(std::iter::repeat_n(at, degree - has));
            }
        }
        if more.is_empty() || knots.len() > 800 {
            return Ok((own, beside));
        }
        // (What it is moved to is itself known only so closely — the
        // foot of a surface fitted to as much: where more knots have
        // stopped bringing the curve nearer, and it is as near as
        // counts for nothing here, that is as close as it gets.)
        if worst <= within * 16.0 && worst >= before * 0.5 {
            return Ok((own, beside));
        }
        before = worst;
        knots.extend(more);
        knots.sort_by(f64::total_cmp);
    }
    anyhow::bail!("an edge could not be moved off itself closely enough")
}

/// The slab of face `face` of `solid`: the solid between the face and
/// the surface `distance` off it — out of the body for a distance more
/// than nothing, into it for less. Its faces are called `{id}/near`
/// (the face itself), `{id}/far` and `{id}/side{k}`.
pub fn slab(solid: &Solid, face: usize, distance: f64, id: &str) -> anyhow::Result<Solid> {
    ensure!(distance != 0.0, "a slab is of some thickness");
    slab_by(solid, face, &|_| distance, id)
}

/// As [`slab`], between the face and the surface that is off it by
/// what `by` says at each point of it — all out of the body or all into
/// it; where that is nothing (along an edge the face turns about), the
/// slab comes to an edge.
pub fn slab_by(
    solid: &Solid,
    face: usize,
    by: &dyn Fn(Point3) -> f64,
    id: &str,
) -> anyhow::Result<Solid> {
    let f = &solid.faces[face];
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let within = size * 1e-9;
    // Nothing, as near as matters.
    let none = within * 16.0;
    // Which way: the same all over the face.
    let ways: Vec<f64> = f
        .loops
        .iter()
        .flatten()
        .flat_map(|step| {
            let (a, b) = solid.span(step);
            (0..=4).map(move |k| {
                let uv = step.pcurve.point_at(a + (b - a) * f64::from(k) / 4.0);
                by(f.surface.point_at(uv.x, uv.y))
            })
        })
        .collect();
    let first = ways.iter().find(|d| d.abs() > none);
    let out_of = *first.context("a slab is of some thickness")? > 0.0;
    ensure!(
        ways.iter().all(|d| d.abs() <= none || (*d > 0.0) == out_of),
        "the face would be moved out of the body here and into it there"
    );
    // Along the surface's own normal.
    let flip = if f.outward { 1.0 } else { -1.0 };
    let off = if out_of == f.outward { 1.0 } else { -1.0 };
    let far = Arc::new(offset_surface(&f.surface, &|p| by(p) * flip, within)?);
    // As far off as the face is round, all of it comes to one point (a
    // ball's face to its centre): the slab is what is between the face
    // and that point.
    {
        let ((u0, u1), (v0, v1)) = far.domain();
        let at = |i: usize, j: usize| {
            far.point_at(
                u0 + (u1 - u0) * i as f64 / 6.0,
                v0 + (v1 - v0) * j as f64 / 6.0,
            )
        };
        let centre = at(3, 3);
        if (0..=6).all(|i| (0..=6).all(|j| at(i, j).distance(centre) <= none)) {
            return fan_to(solid, face, centre, off, id);
        }
    }
    // That far off, the face has to be a face still: not turned inside
    // out (further off than it bends).
    let mut inside_out = false;
    for step in f.loops.iter().flatten() {
        let (a, b) = solid.span(step);
        for k in 0..=8 {
            let uv = step
                .pcurve
                .point_at(a + (b - a) * (f64::from(k) + 0.5) / 9.0);
            // (Where the face comes to a point, a little way in from
            // it: the surface fitted off it comes to its point only as
            // nearly as it is fitted, and has no normal to tell by
            // there.)
            let ((u0, u1), (v0, v1)) = f.surface.domain();
            let (_, du, dv) = f.surface.derivatives_at(uv.x, uv.y);
            let large = (du * (u1 - u0)).length().max((dv * (v1 - v0)).length());
            let pointed = du.cross(dv).length() * (u1 - u0) * (v1 - v0) <= 1e-6 * large * large;
            let uv = if pointed {
                uv + (Point2::new(u0 + u1, v0 + v1) / 2.0 - uv) * 1e-3
            } else {
                uv
            };
            inside_out |= by(f.surface.point_at(uv.x, uv.y)).abs() > none
                && normal(&far, uv).dot(normal(&f.surface, uv)) <= 0.0;
        }
    }
    if inside_out {
        // A round as far off as its axis, or further: what is between
        // it and its axis. (What is past the axis is [`past_axis`].)
        if let Ok(round) = crate::thread::cylinder(solid, face) {
            let foot = |p: Point3| round.origin + round.axis * (p - round.origin).dot(round.axis);
            let to_axis = f.loops.iter().flatten().all(|step| {
                let (a, _) = solid.span(step);
                let uv = step.pcurve.point_at(a);
                let p = f.surface.point_at(uv.x, uv.y);
                let moved = far.point_at(uv.x, uv.y);
                (moved - p).dot(foot(p) - p) > 0.0 && moved.distance(p) >= round.radius - none
            });
            if to_axis {
                return wedge_to(solid, face, &foot, id);
            }
        }
        // A round of any other run (about a rim, in a root): what is
        // between it and the curve it is round about.
        if let Some((radius, to)) = spine(solid, face, off) {
            let reaches = f.loops.iter().flatten().all(|step| {
                let (a, _) = solid.span(step);
                let uv = step.pcurve.point_at(a);
                let p = f.surface.point_at(uv.x, uv.y);
                far.point_at(uv.x, uv.y).distance(p) >= radius - none
            });
            if reaches {
                return wedge_to(solid, face, &to, id);
            }
        }
        // A cone moved in past its tip: the moved cone comes to a tip
        // of its own, lower on the axis, and what would be beyond it —
        // the cone crossing itself — is no part of it.
        if let Some(tipped) = to_own_tip(&f.surface, &far, none) {
            let tipped = Arc::new(tipped);
            let sound = f.loops.iter().flatten().all(|step| {
                let (a, b) = solid.span(step);
                (0..=8).all(|k| {
                    let uv = step
                        .pcurve
                        .point_at(a + (b - a) * (f64::from(k) + 0.5) / 9.0);
                    normal(&tipped, uv).dot(normal(&f.surface, uv)) > 0.0
                })
            });
            if sound {
                return slab_between(solid, face, tipped, off, id);
            }
        }
        anyhow::bail!("the face bends more tightly than it is thickened: that is not made yet");
    }
    slab_between(solid, face, far, off, id)
}

/// The solid `face` of `solid` sweeps as it is moved by `by` without
/// turning: between the face and itself that far on, its sides along
/// the move. All of the face has to move out of the body, or all into
/// it (along its edges it may slide in itself). Its faces are called as
/// a [`slab`]'s; the second value says whether it is out of the body.
pub fn slab_along(
    solid: &Solid,
    face: usize,
    by: Point3,
    id: &str,
) -> anyhow::Result<(Solid, bool)> {
    let f = &solid.faces[face];
    ensure!(by.length() > 0.0 && by.is_finite(), "moved by some way");
    let flip = if f.outward { 1.0 } else { -1.0 };
    let mut ways = Vec::new();
    for step in f.loops.iter().flatten() {
        let (a, b) = solid.span(step);
        for k in 0..=8 {
            let uv = step.pcurve.point_at(a + (b - a) * f64::from(k) / 8.0);
            ways.push(normal(&f.surface, uv).dot(by) * flip);
        }
    }
    let none = by.length() * 1e-9;
    let first = ways.iter().find(|d| d.abs() > none);
    let out_of = *first.context("the face would only slide in itself")? > 0.0;
    ensure!(
        ways.iter().all(|d| d.abs() <= none || (*d > 0.0) == out_of),
        "the face would be moved out of the body here and into it there"
    );
    let surface = &f.surface;
    let (p, q) = surface.degrees();
    let (ku, kv) = surface.knots();
    let (nu, nv) = surface.counts();
    let far = NurbsSurface::new(
        p,
        q,
        ku.to_vec(),
        kv.to_vec(),
        nu,
        nv,
        surface.control_points().iter().map(|c| *c + by).collect(),
        Some(surface.weights().to_vec()),
    )?;
    let off = if out_of == f.outward { 1.0 } else { -1.0 };
    Ok((slab_between(solid, face, Arc::new(far), off, id)?, out_of))
}

/// The stretch of `surface` from `range.0` to `range.1` of its v (or,
/// not `along_v`, its u), laid out over all of that parameter's domain:
/// the surface itself there, with the control points of that stretch
/// alone. A stretch that runs on past the end of the domain goes on
/// from its start, where the surface closes on itself (to within
/// `within`). `None` where that cannot be had.
pub(crate) fn stretch(
    surface: &NurbsSurface,
    along_v: bool,
    range: (f64, f64),
    within: f64,
) -> Option<NurbsSurface> {
    let (degrees, knots, counts) = (surface.degrees(), surface.knots(), surface.counts());
    let (degree, knots, rows, across) = if along_v {
        (degrees.1, knots.1, counts.0, counts.1)
    } else {
        (degrees.0, knots.0, counts.1, counts.0)
    };
    let (lo, hi) = (knots[degree], knots[across]);
    let (from, to) = range;
    if !(from >= lo && from < hi && to > from && to - from <= hi - lo) {
        return None;
    }
    let at = |row: usize, k: usize| {
        if along_v {
            row * counts.1 + k
        } else {
            k * counts.1 + row
        }
    };
    // Each row as a curve in weighted coordinates (x·w, y·w, z·w) and
    // the weights as a curve beside it: both are cut alike.
    let mut pieces: Vec<(Vec<Point3>, Vec<f64>)> = Vec::with_capacity(rows);
    let mut cut_knots: Option<Vec<f64>> = None;
    for row in 0..rows {
        let weighted: Vec<Point3> = (0..across)
            .map(|k| surface.control_points()[at(row, k)] * surface.weights()[at(row, k)])
            .collect();
        let weights: Vec<Point3> = (0..across)
            .map(|k| Point3::new(surface.weights()[at(row, k)], 0.0, 0.0))
            .collect();
        let whole = Curve3::new(degree, knots.to_vec(), weighted, None).ok()?;
        let beside = Curve3::new(degree, knots.to_vec(), weights, None).ok()?;
        let cut = |curve: &Curve3| -> Option<(Vec<Point3>, Vec<f64>)> {
            if to <= hi {
                let piece = curve.trimmed(from, to).ok()?;
                return Some((piece.control_points().to_vec(), piece.knots().to_vec()));
            }
            // To the end, and on from the start: one curve, where the
            // two meet in one point.
            let first = curve.trimmed(from, hi).ok()?;
            let second = curve.trimmed(lo, to - (hi - lo)).ok()?;
            let (a, b) = (first.control_points(), second.control_points());
            if a[a.len() - 1].distance(b[0]) > within {
                return None;
            }
            let mut points = a.to_vec();
            points.extend_from_slice(&b[1..]);
            let mut all = first.knots()[..first.knots().len() - 1].to_vec();
            all.extend(second.knots()[degree + 1..].iter().map(|k| k + (hi - lo)));
            Some((points, all))
        };
        let (points, cut_at) = cut(&whole)?;
        let (beside, _) = cut(&beside)?;
        let weights: Vec<f64> = beside.iter().map(|w| w.x).collect();
        if weights.iter().any(|w| *w <= 0.0) {
            return None;
        }
        let points = points.iter().zip(&weights).map(|(p, w)| *p / *w).collect();
        cut_knots.get_or_insert(cut_at);
        pieces.push((points, weights));
    }
    // Over the whole domain: the knots moved and stretched to it.
    let cut_knots = cut_knots?;
    let laid: Vec<f64> = cut_knots
        .iter()
        .map(|k| lo + (k - from) * (hi - lo) / (to - from))
        .collect();
    let n = pieces.first()?.0.len();
    let (count_u, count_v) = if along_v { (rows, n) } else { (n, rows) };
    let mut points = vec![Point3::ZERO; count_u * count_v];
    let mut weights = vec![1.0; count_u * count_v];
    for (row, (ps, ws)) in pieces.iter().enumerate() {
        for k in 0..n {
            let i = if along_v {
                row * count_v + k
            } else {
                k * count_v + row
            };
            (points[i], weights[i]) = (ps[k], ws[k]);
        }
    }
    let (ku, kv) = if along_v {
        (surface.knots().0.to_vec(), laid)
    } else {
        (laid, surface.knots().1.to_vec())
    };
    NurbsSurface::new(
        degrees.0,
        degrees.1,
        ku,
        kv,
        count_u,
        count_v,
        points,
        Some(weights),
    )
    .ok()
}

/// A body of one face that closes on itself both ways (a torus), grown
/// by `distance` past where its hole closes: the grown face would pass
/// through itself there, and what is left of it — the part that still
/// faces out — comes to a point at either end, as an apple does. `None`
/// for bodies that are not such, or do not grow that far.
fn closed_over(solid: &Solid, distance: f64, id: &str) -> Option<Solid> {
    let [f] = &solid.faces[..] else {
        return None;
    };
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let within = size * 1e-9;
    let none = within * 16.0;
    let surface = &f.surface;
    let flip = if f.outward { 1.0 } else { -1.0 };
    let far = offset_surface(surface, &|_| distance * flip, within).ok()?;
    let ((u0, u1), (v0, v1)) = surface.domain();
    // Whether the grown face still faces as the face does.
    let faces = |u: f64, v: f64| {
        let uv = Point2::new(u, v);
        normal(&far, uv).dot(normal(surface, uv)) > 0.0
    };
    // Across which way it turns over: v (then `swap` is false) or u.
    for swap in [false, true] {
        let (a0, a1, b0, b1) = if swap {
            (v0, v1, u0, u1)
        } else {
            (u0, u1, v0, v1)
        };
        let at = |a: f64, b: f64| if swap { (b, a) } else { (a, b) };
        let ok = |a: f64, b: f64| {
            let (u, v) = at(a, b);
            faces(u, v)
        };
        // Along b at the middle of a: where it stops facing out, and
        // where it starts again.
        let middle = (a0 + a1) / 2.0;
        let steps = 256;
        let b_at = |k: usize| b0 + (b1 - b0) * (k as f64 + 0.5) / steps as f64;
        let flags: Vec<bool> = (0..steps).map(|k| ok(middle, b_at(k))).collect();
        let turns: Vec<usize> = (0..steps)
            .filter(|&k| flags[k] != flags[(k + 1) % steps])
            .collect();
        let [first, second] = turns[..] else {
            continue;
        };
        let root = |k: usize| {
            let (mut lo, mut hi) = (b_at(k), b_at(k) + (b1 - b0) / steps as f64);
            let was = flags[k];
            for _ in 0..60 {
                let mid = (lo + hi) / 2.0;
                let b = b0 + (mid - b0).rem_euclid(b1 - b0);
                if ok(middle, b) == was {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (lo + hi) / 2.0
        };
        // From where it starts facing out, around to where it stops.
        let (start, end) = if flags[(first + 1) % steps] {
            (root(first), root(second))
        } else {
            (root(second), root(first) + (b1 - b0))
        };
        let wrapped = |b: f64| b0 + (b - b0).rem_euclid(b1 - b0);
        // There the grown face comes to a point, all the way around.
        let pointed = |b: f64| {
            let (u, v) = at(a0, wrapped(b));
            let tip = far.point_at(u, v);
            (1..8).all(|k| {
                let (u, v) = at(a0 + (a1 - a0) * f64::from(k) / 8.0, wrapped(b));
                far.point_at(u, v).distance(tip) <= none
            })
        };
        if !pointed(start) || !pointed(end) {
            continue;
        }
        // That part of it, laid out over all of the face's (u, v).
        let part = |u: f64, v: f64| {
            let (a, b) = if swap { (v, u) } else { (u, v) };
            let b = start + (end - start) * (b - b0) / (b1 - b0);
            let (u, v) = at(a, wrapped(b));
            far.point_at(u, v)
        };
        // (As the surface itself where that can be had: its own control
        // points over that stretch. A new surface fitted to it as
        // closely has many thousands of them, where an arc turned
        // about an axis has a handful.)
        let stretched = stretch(&far, !swap, (start, end), within);
        // (Its side where the other parameter starts is then its first
        // row of control points, as it is.)
        let side = stretched.as_ref().and_then(|skin| {
            let (counts, knots, degrees) = (skin.counts(), skin.knots(), skin.degrees());
            let (n, knots, degree) = if swap {
                (counts.0, knots.0, degrees.0)
            } else {
                (counts.1, knots.1, degrees.1)
            };
            let at = |k: usize| if swap { k * counts.1 } else { k };
            Curve3::new(
                degree,
                knots.to_vec(),
                (0..n).map(|k| skin.control_points()[at(k)]).collect(),
                Some((0..n).map(|k| skin.weights()[at(k)]).collect()),
            )
            .ok()
        });
        let skin = match stretched {
            Some(skin) => Arc::new(skin),
            None => Arc::new(refit(&far, &part, within).ok()?),
        };
        // One face of it: around from a seam, to the point at either end.
        let on = |a: f64, b: f64| {
            let (u, v) = at(a, b);
            skin.point_at(u, v)
        };
        let like = Curve3::new(
            3,
            vec![b0, b0, b0, b0, b1, b1, b1, b1],
            (0..4)
                .map(|k| on(a0, b0 + (b1 - b0) * f64::from(k) / 3.0))
                .collect(),
            None,
        )
        .ok()?;
        let seam = match side {
            Some(side) => side,
            None => pair(&like, (b0, b1), &|b| on(a0, b), within).ok()?.1,
        };
        let mut out = Solid::default();
        for b in [b0, b1] {
            out.vertices.push(Vertex {
                point: on(a0, b),
                tolerance: within * 4.0,
                name: None,
            });
        }
        out.edges.push(Edge {
            curve: Arc::new(seam),
            range: (b0, b1),
            ends: [0, 1],
            tolerance: within * 4.0,
            name: Some(format!("{id}/seam")),
            also: Vec::new(),
        });
        let line = |from: (f64, f64), to: (f64, f64), range: (f64, f64)| {
            let uv = |x: (f64, f64)| {
                let (u, v) = at(x.0, x.1);
                Point2::new(u, v)
            };
            Curve2::new(
                1,
                vec![range.0, range.0, range.1, range.1],
                vec![uv(from), uv(to)],
                None,
            )
            .map(Arc::new)
        };
        // Around the (a, b) rectangle: counter-clockwise in (u, v), or
        // (with a and b swapped) the other way.
        let mut walk = vec![
            Use {
                walk: Walk::Pole {
                    vertex: 0,
                    range: (0.0, 1.0),
                },
                pcurve: line((a0, b0), (a1, b0), (0.0, 1.0)).ok()?,
            },
            Use {
                walk: Walk::Edge {
                    edge: 0,
                    reversed: false,
                },
                pcurve: line((a1, b0), (a1, b1), (b0, b1)).ok()?,
            },
            Use {
                walk: Walk::Pole {
                    vertex: 1,
                    range: (0.0, 1.0),
                },
                pcurve: line((a1, b1), (a0, b1), (0.0, 1.0)).ok()?,
            },
            Use {
                walk: Walk::Edge {
                    edge: 0,
                    reversed: true,
                },
                pcurve: line((a0, b0), (a0, b1), (b0, b1)).ok()?,
            },
        ];
        if swap {
            // (The same rectangle walked the other way: each step turned
            // around, the poles' curves with them.)
            walk.reverse();
            for step in &mut walk {
                match &mut step.walk {
                    Walk::Edge { reversed, .. } => *reversed = !*reversed,
                    Walk::Pole { .. } => {
                        let (from, to) = (step.pcurve.point_at(0.0), step.pcurve.point_at(1.0));
                        step.pcurve = Arc::new(Curve2::line(to, from).ok()?);
                    }
                }
            }
        }
        out.faces.push(Face {
            surface: skin,
            outward: true,
            loops: vec![walk],
            tolerance: f.tolerance.max(within),
            name: f.name.clone(),
            also: Vec::new(),
            smooth: Vec::new(),
        });
        if workpiece_boolean::measure(&out).volume < 0.0 {
            out.faces[0].outward = false;
        }
        return Some(out);
    }
    None
}

/// `far` — `surface`, which comes to a point along one side and runs
/// straight from there to the other, moved off itself — ending in a
/// point of its own where its straight lines meet: what a cone moved
/// inwards is, short of where it would cross itself. `None` for
/// surfaces that are not such.
fn to_own_tip(surface: &NurbsSurface, far: &NurbsSurface, none: f64) -> Option<NurbsSurface> {
    let (p, q) = far.degrees();
    let (nu, nv) = far.counts();
    if surface.counts() != (nu, nv) || surface.degrees() != (p, q) {
        return None;
    }
    let (ku, kv) = far.knots();
    let at = |s: &NurbsSurface, i: usize, j: usize| s.control_points()[i * nv + j];
    // Which side is the point, if it is straight across from there.
    let sides: [(bool, usize); 4] = [(true, 0), (true, nv - 1), (false, 0), (false, nu - 1)];
    for (along_u, side) in sides {
        let (straight, count) = if along_u {
            (q == 1 && nv == 2, nu)
        } else {
            (p == 1 && nu == 2, nv)
        };
        if !straight {
            continue;
        }
        let pick = |s: &NurbsSurface, k: usize, pole: bool| {
            let other = 1 - side.min(1);
            let which = if pole { side.min(1) } else { other };
            if along_u {
                at(s, k, which)
            } else {
                at(s, which, k)
            }
        };
        let tip = pick(surface, 0, true);
        if (0..count).any(|k| pick(surface, k, true).distance(tip) > none) {
            continue;
        }
        // Where two of the moved surface's lines meet, and all of them.
        let (a0, b0) = (pick(far, 0, true), pick(far, 0, false));
        let k1 = (1..count).max_by(|x, y| {
            let turn = |k: usize| {
                (pick(far, k, false) - pick(far, k, true))
                    .cross(b0 - a0)
                    .length()
            };
            turn(*x).total_cmp(&turn(*y))
        })?;
        let (a1, b1) = (pick(far, k1, true), pick(far, k1, false));
        let (d0, d1) = (b0 - a0, b1 - a1);
        let across = d0.cross(d1);
        if across.length_squared() <= 1e-24 * d0.length_squared() * d1.length_squared() {
            continue;
        }
        let s0 = (a1 - a0).cross(d1).dot(across) / across.length_squared();
        let meet = a0 + d0 * s0;
        if !(0.0..1.0).contains(&s0) {
            continue;
        }
        let all_meet = (0..count).all(|k| {
            let (a, b) = (pick(far, k, true), pick(far, k, false));
            (meet - a).cross(b - a).length() <= none * (b - a).length().max(1.0)
        });
        if !all_meet {
            continue;
        }
        let mut points = far.control_points().to_vec();
        for k in 0..count {
            let index = if along_u {
                k * nv + side.min(1)
            } else {
                side.min(1) * nv + k
            };
            points[index] = meet;
        }
        return NurbsSurface::new(
            p,
            q,
            ku.to_vec(),
            kv.to_vec(),
            nu,
            nv,
            points,
            Some(far.weights().to_vec()),
        )
        .ok();
    }
    None
}

/// Of a round face (part of a cylinder) moved `distance` off itself
/// through its axis and on: what it passes through past the axis —
/// between the axis and the round, inside out, it ends as. `None` for a
/// face that is no such round, or does not go that far.
pub fn past_axis(solid: &Solid, face: usize, distance: f64, id: &str) -> Option<Solid> {
    let f = &solid.faces[face];
    let round = crate::thread::cylinder(solid, face).ok()?;
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let within = size * 1e-9;
    if distance.abs() <= round.radius + within * 64.0 {
        return None;
    }
    // (Towards the axis: into the body for a rod's side, out of it for a
    // hole's wall.)
    if (distance > 0.0) != round.hole {
        return None;
    }
    let flip = if f.outward { 1.0 } else { -1.0 };
    let far = Arc::new(offset_surface(&f.surface, &|_| distance * flip, within).ok()?);
    let off = if (distance > 0.0) == f.outward {
        1.0
    } else {
        -1.0
    };
    let crossed = slab_between(solid, face, far, off, id).ok()?;
    let name = format!("{id}/far");
    let k = crossed
        .faces
        .iter()
        .position(|x| x.name.as_deref() == Some(&name))?;
    let sheet = crate::surface::faces_of(&crossed, &[k]);
    let foot = |p: Point3| round.origin + round.axis * (p - round.origin).dot(round.axis);
    wedge_to(&sheet, 0, &foot, &format!("{id}/past")).ok()
}

/// The two ways a surface bends most and least at (u, v): for each, how
/// much (towards its normal ∂u × ∂v, if more than nothing) and the way
/// in (u, v).
fn bends(surface: &NurbsSurface, uv: Point2) -> Option<[(f64, Point2); 2]> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (hu, hv) = ((u1 - u0) * 1e-5, (v1 - v0) * 1e-5);
    // (A step in from the surface's rim, where it is taken there.)
    let at = Point2::new(uv.x.clamp(u0 + hu, u1 - hu), uv.y.clamp(v0 + hv, v1 - hv));
    let d = |x: f64, y: f64| {
        let (_, du, dv) = surface.derivatives_at(x, y);
        (du, dv)
    };
    let (du, dv) = d(at.x, at.y);
    let n = du.cross(dv).try_normalize()?;
    let ((ua, va), (ub, vb)) = (d(at.x + hu, at.y), d(at.x - hu, at.y));
    let ((uc, vc), (ud, vd)) = (d(at.x, at.y + hv), d(at.x, at.y - hv));
    let duu = (ua - ub) / (2.0 * hu);
    let dvv = (vc - vd) / (2.0 * hv);
    let duv = ((va - vb) / (2.0 * hu) + (uc - ud) / (2.0 * hv)) / 2.0;
    let (e, f, g) = (du.dot(du), du.dot(dv), dv.dot(dv));
    let (l, m, k) = (duu.dot(n), duv.dot(n), dvv.dot(n));
    let det = e * g - f * f;
    if det <= 0.0 {
        return None;
    }
    // The shape operator, in (u, v): its own values and ways.
    let (a, b, c, dd) = (
        (l * g - m * f) / det,
        (m * g - k * f) / det,
        (m * e - l * f) / det,
        (k * e - m * f) / det,
    );
    let (mean, gauss) = ((a + dd) / 2.0, a * dd - b * c);
    let spread = (mean * mean - gauss).max(0.0).sqrt();
    let way = |bend: f64| {
        let w = if (a - bend).abs() > (dd - bend).abs() {
            Point2::new(b, bend - a)
        } else {
            Point2::new(bend - dd, c)
        };
        if w.length() > 0.0 {
            w
        } else if (a - bend).abs() <= (dd - bend).abs() {
            Point2::X
        } else {
            Point2::Y
        }
    };
    let (most, least) = (mean + spread, mean - spread);
    Some([(most, way(most)), (least, way(least))])
}

/// The curve face `face` of `solid` is round about on the side `off` of
/// its surface's normal (1 or −1), if it is round about one: a pipe of
/// one radius along it — a cylinder's wall, a round about a rim or in a
/// root. The radius, and where on that curve each point of the face is
/// round about.
fn spine(solid: &Solid, face: usize, off: f64) -> Option<(f64, impl Fn(Point3) -> Point3)> {
    let f = &solid.faces[face];
    let surface = f.surface.clone();
    let mut radius: Option<f64> = None;
    let mut seeds: Vec<(Point3, Point2)> = Vec::new();
    for step in f.loops.iter().flatten() {
        let (a, b) = solid.span(step);
        for k in 0..=4 {
            let uv = step
                .pcurve
                .point_at(a + (b - a) * (f64::from(k) + 0.5) / 5.0);
            let both = bends(&surface, uv)?;
            let (bend, way) = if off > 0.0 { both[0] } else { both[1] };
            let here = 1.0 / (bend * off);
            if !(here > 0.0 && here.is_finite()) {
                return None;
            }
            let r = *radius.get_or_insert(here);
            if (here - r).abs() > r * 1e-5 {
                return None;
            }
            // (Round about one place, the way it bends most.)
            let p = surface.point_at(uv.x, uv.y);
            let centre = p + normal(&surface, uv) * (off * r);
            let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
            let along = (du * way.x + dv * way.y).length();
            if along > 0.0 {
                let next = uv + way * (r * 1e-2 / along);
                let ((u0, u1), (v0, v1)) = surface.domain();
                let next = Point2::new(next.x.clamp(u0, u1), next.y.clamp(v0, v1));
                let q = surface.point_at(next.x, next.y) + normal(&surface, next) * (off * r);
                if q.distance(centre) > r * 1e-5 {
                    return None;
                }
            }
            seeds.push((p, uv));
        }
    }
    let radius = radius?;
    let to = move |p: Point3| {
        let seed = seeds
            .iter()
            .min_by(|a, b| a.0.distance(p).total_cmp(&b.0.distance(p)))
            .map_or(Point2::ZERO, |s| s.1);
        let (uv, _) = workpiece_boolean::geometry::foot(&surface, p, seed);
        surface.point_at(uv.x, uv.y) + normal(&surface, uv) * (off * radius)
    };
    Some((radius, to))
}

/// The solid between face `face` of `solid` and where `to` puts each of
/// its points — all of them onto one curve (a round's axis): the face,
/// and from each of its edges a face to that curve.
fn wedge_to(
    solid: &Solid,
    face: usize,
    to: &dyn Fn(Point3) -> Point3,
    id: &str,
) -> anyhow::Result<Solid> {
    let f = &solid.faces[face];
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let within = size * 1e-9;
    let none = within * 16.0;
    let tolerance = f.tolerance.max(within);
    // Which side of the face's surface the curve is on.
    let first = f
        .loops
        .iter()
        .flatten()
        .next()
        .context("a face has edges")?;
    let uv = first.pcurve.point_at(solid.span(first).0);
    let at = f.surface.point_at(uv.x, uv.y);
    let off = normal(&f.surface, uv).dot(to(at) - at).signum();
    let mut out = Solid::default();
    let mut near: BTreeMap<usize, usize> = BTreeMap::new();
    let mut twin: BTreeMap<usize, usize> = BTreeMap::new();
    let mut ups: BTreeMap<usize, usize> = BTreeMap::new();
    let mut edges: BTreeMap<usize, usize> = BTreeMap::new();
    // The edges on the curve, by the vertices they run between.
    let mut on_curve: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    let walked = |edge: usize| {
        let steps = f.loops.iter().flatten();
        steps
            .filter(|s| matches!(s.walk, Walk::Edge { edge: other, .. } if other == edge))
            .count()
    };
    let line = |from: Point2, to: Point2, range: (f64, f64)| -> anyhow::Result<Arc<Curve2>> {
        Ok(Arc::new(Curve2::new(
            1,
            vec![range.0, range.0, range.1, range.1],
            vec![from, to],
            None,
        )?))
    };
    let uv = Point2::new;
    let mut loops = Vec::new();
    let mut sides: Vec<Face> = Vec::new();
    for walk in &f.loops {
        let mut steps = Vec::new();
        for step in walk {
            let mut vertex = |out: &mut Solid, v: usize| -> usize {
                if let Some(&k) = near.get(&v) {
                    return k;
                }
                out.vertices.push(solid.vertices[v].clone());
                let lower = out.vertices.len() - 1;
                near.insert(v, lower);
                // Its twin on the curve: one with any that is there.
                let point = to(solid.vertices[v].point);
                let there = (0..out.vertices.len()).find(|k| {
                    twin.values().any(|t| t == k) && out.vertices[*k].point.distance(point) <= none
                });
                let upper = there.unwrap_or_else(|| {
                    out.vertices.push(Vertex {
                        point,
                        tolerance: within * 4.0,
                        name: None,
                    });
                    out.vertices.len() - 1
                });
                twin.insert(v, upper);
                lower
            };
            let Walk::Edge { edge, reversed } = step.walk else {
                let Walk::Pole { vertex: v, range } = step.walk else {
                    unreachable!()
                };
                steps.push(Use {
                    walk: Walk::Pole {
                        vertex: vertex(&mut out, v),
                        range,
                    },
                    pcurve: step.pcurve.clone(),
                });
                continue;
            };
            let e = &solid.edges[edge];
            let ends = [vertex(&mut out, e.ends[0]), vertex(&mut out, e.ends[1])];
            let fresh = !edges.contains_key(&edge);
            let own = *edges.entry(edge).or_insert_with(|| {
                out.edges.push(Edge { ends, ..e.clone() });
                out.edges.len() - 1
            });
            steps.push(Use {
                walk: Walk::Edge {
                    edge: own,
                    reversed,
                },
                pcurve: step.pcurve.clone(),
            });
            if !fresh || walked(edge) > 1 {
                continue;
            }
            let mut up = |out: &mut Solid, v: usize| -> anyhow::Result<usize> {
                if let Some(&up) = ups.get(&v) {
                    return Ok(up);
                }
                let (lower, upper) = (near[&v], twin[&v]);
                let (from, to) = (out.vertices[lower].point, out.vertices[upper].point);
                out.edges.push(Edge {
                    curve: Arc::new(Curve3::line(from, to)?),
                    range: (0.0, 1.0),
                    ends: [lower, upper],
                    tolerance,
                    name: Some(format!("{id}/up{}", out.edges.len())),
                    also: Vec::new(),
                });
                ups.insert(v, out.edges.len() - 1);
                Ok(out.edges.len() - 1)
            };
            let (first, last) = (up(&mut out, e.ends[0])?, up(&mut out, e.ends[1])?);
            let (t0, t1) = e.range;
            let along = |t: f64| to(e.curve.point_at(t));
            let (own_curve, beside) = pair(&e.curve, e.range, &along, within)?;
            let (mut points, mut weights) = (Vec::new(), Vec::new());
            for k in 0..own_curve.control_points().len() {
                for curve in [&own_curve, &beside] {
                    points.push(curve.control_points()[k]);
                    weights.push(curve.weights()[k]);
                }
            }
            let ruled = NurbsSurface::new(
                own_curve.degree(),
                1,
                own_curve.knots().to_vec(),
                vec![0.0, 0.0, 1.0, 1.0],
                own_curve.control_points().len(),
                2,
                points,
                Some(weights),
            )?;
            let (a, b) = (twin[&e.ends[0]], twin[&e.ends[1]]);
            // All of the edge to one point of the curve (an arc about
            // the axis): the face closes in it. Else along the curve:
            // on the edge another of the face's edges may have put
            // there already.
            let top = if a == b && along((t0 + t1) / 2.0).distance(along(t0)) <= none {
                Use {
                    walk: Walk::Pole {
                        vertex: a,
                        range: e.range,
                    },
                    pcurve: line(uv(t1, 1.0), uv(t0, 1.0), e.range)?,
                }
            } else {
                let key = (a.min(b), a.max(b));
                match on_curve.get(&key) {
                    None => {
                        out.edges.push(Edge {
                            curve: Arc::new(beside),
                            range: e.range,
                            ends: [a, b],
                            tolerance: e.tolerance.max(within * 2.0),
                            name: Some(format!("{id}/axis{}", on_curve.len())),
                            also: Vec::new(),
                        });
                        on_curve.insert(key, out.edges.len() - 1);
                        Use {
                            walk: Walk::Edge {
                                edge: out.edges.len() - 1,
                                reversed: true,
                            },
                            pcurve: line(uv(t0, 1.0), uv(t1, 1.0), e.range)?,
                        }
                    }
                    Some(&there) => {
                        // As that edge runs: the same way as this one,
                        // or against it.
                        let other = &out.edges[there];
                        let (s0, s1) = other.range;
                        let same = other.ends[0] == a;
                        let (from, until) = if same { (t0, t1) } else { (t1, t0) };
                        let middle = other.curve.point_at((s0 + s1) / 2.0);
                        ensure!(
                            middle.distance(beside.point_at((t0 + t1) / 2.0)) <= within * 1e3,
                            "two edges of a round come to its axis at different paces"
                        );
                        Use {
                            walk: Walk::Edge {
                                edge: there,
                                reversed: same,
                            },
                            pcurve: line(uv(from, 1.0), uv(until, 1.0), other.range)?,
                        }
                    }
                }
            };
            let around = vec![
                Use {
                    walk: Walk::Edge {
                        edge: own,
                        reversed: false,
                    },
                    pcurve: line(uv(t0, 0.0), uv(t1, 0.0), e.range)?,
                },
                Use {
                    walk: Walk::Edge {
                        edge: last,
                        reversed: false,
                    },
                    pcurve: line(uv(t1, 0.0), uv(t1, 1.0), (0.0, 1.0))?,
                },
                top,
                Use {
                    walk: Walk::Edge {
                        edge: first,
                        reversed: true,
                    },
                    pcurve: line(uv(t0, 0.0), uv(t0, 1.0), (0.0, 1.0))?,
                },
            ];
            sides.push(Face {
                surface: Arc::new(ruled),
                outward: (off > 0.0) == !reversed,
                loops: vec![around],
                tolerance,
                name: Some(format!("{id}/side{}", sides.len())),
                also: Vec::new(),
                smooth: Vec::new(),
            });
        }
        loops.push(steps);
    }
    out.faces.push(Face {
        surface: f.surface.clone(),
        outward: off < 0.0,
        loops,
        tolerance,
        name: Some(format!("{id}/near")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    out.faces.extend(sides);
    Ok(out)
}

/// The solid between face `face` of `solid` and the point `tip`: the
/// face, and from each of its edges a face to the point. `off` says
/// which side of the face's surface the point is on, as for
/// [`slab_between`].
fn fan_to(solid: &Solid, face: usize, tip: Point3, off: f64, id: &str) -> anyhow::Result<Solid> {
    let f = &solid.faces[face];
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let within = size * 1e-9;
    let tolerance = f.tolerance.max(within);
    let mut out = Solid::default();
    let mut near: BTreeMap<usize, usize> = BTreeMap::new();
    let mut edges: BTreeMap<usize, usize> = BTreeMap::new();
    let mut ups: BTreeMap<usize, usize> = BTreeMap::new();
    out.vertices.push(Vertex {
        point: tip,
        tolerance: within * 4.0,
        name: None,
    });
    let walked = |edge: usize| {
        let steps = f.loops.iter().flatten();
        steps
            .filter(|s| matches!(s.walk, Walk::Edge { edge: other, .. } if other == edge))
            .count()
    };
    let mut loops = Vec::new();
    let mut sides = Vec::new();
    for walk in &f.loops {
        let mut steps = Vec::new();
        for step in walk {
            let mut vertex = |out: &mut Solid, v: usize| {
                *near.entry(v).or_insert_with(|| {
                    out.vertices.push(solid.vertices[v].clone());
                    out.vertices.len() - 1
                })
            };
            let Walk::Edge { edge, reversed } = step.walk else {
                let Walk::Pole { vertex: v, range } = step.walk else {
                    unreachable!()
                };
                steps.push(Use {
                    walk: Walk::Pole {
                        vertex: vertex(&mut out, v),
                        range,
                    },
                    pcurve: step.pcurve.clone(),
                });
                continue;
            };
            let e = &solid.edges[edge];
            let ends = [vertex(&mut out, e.ends[0]), vertex(&mut out, e.ends[1])];
            let fresh = !edges.contains_key(&edge);
            let own = *edges.entry(edge).or_insert_with(|| {
                out.edges.push(Edge { ends, ..e.clone() });
                out.edges.len() - 1
            });
            steps.push(Use {
                walk: Walk::Edge {
                    edge: own,
                    reversed,
                },
                pcurve: step.pcurve.clone(),
            });
            // (An edge the face walks twice is where its surface closes
            // on itself: nothing is between it and the point but the
            // slab.)
            if !fresh || walked(edge) > 1 {
                continue;
            }
            // The edges from its ends to the point.
            let mut up = |out: &mut Solid, v: usize| -> anyhow::Result<usize> {
                if let Some(&up) = ups.get(&v) {
                    return Ok(up);
                }
                let from = out.vertices[v].point;
                out.edges.push(Edge {
                    curve: Arc::new(Curve3::line(from, tip)?),
                    range: (0.0, 1.0),
                    ends: [v, 0],
                    tolerance,
                    name: Some(format!("{id}/up{}", out.edges.len())),
                    also: Vec::new(),
                });
                ups.insert(v, out.edges.len() - 1);
                Ok(out.edges.len() - 1)
            };
            let (first, last) = (up(&mut out, ends[0])?, up(&mut out, ends[1])?);
            let piece = e.curve.trimmed(e.range.0, e.range.1)?;
            let (mut points, mut weights) = (Vec::new(), Vec::new());
            for (p, w) in piece.control_points().iter().zip(piece.weights()) {
                points.extend([*p, tip]);
                weights.extend([*w, *w]);
            }
            let fan = NurbsSurface::new(
                piece.degree(),
                1,
                piece.knots().to_vec(),
                vec![0.0, 0.0, 1.0, 1.0],
                piece.control_points().len(),
                2,
                points,
                Some(weights),
            )?;
            let line =
                |from: Point2, to: Point2, range: (f64, f64)| -> anyhow::Result<Arc<Curve2>> {
                    Ok(Arc::new(Curve2::new(
                        1,
                        vec![range.0, range.0, range.1, range.1],
                        vec![from, to],
                        None,
                    )?))
                };
            let uv = Point2::new;
            let (t0, t1) = e.range;
            let around = vec![
                Use {
                    walk: Walk::Edge {
                        edge: own,
                        reversed: false,
                    },
                    pcurve: line(uv(t0, 0.0), uv(t1, 0.0), e.range)?,
                },
                Use {
                    walk: Walk::Edge {
                        edge: last,
                        reversed: false,
                    },
                    pcurve: line(uv(t1, 0.0), uv(t1, 1.0), (0.0, 1.0))?,
                },
                Use {
                    walk: Walk::Pole {
                        vertex: 0,
                        range: e.range,
                    },
                    pcurve: line(uv(t1, 1.0), uv(t0, 1.0), e.range)?,
                },
                Use {
                    walk: Walk::Edge {
                        edge: first,
                        reversed: true,
                    },
                    pcurve: line(uv(t0, 0.0), uv(t0, 1.0), (0.0, 1.0))?,
                },
            ];
            sides.push(Face {
                surface: Arc::new(fan),
                outward: (off > 0.0) == !reversed,
                loops: vec![around],
                tolerance,
                name: Some(format!("{id}/side{}", sides.len())),
                also: Vec::new(),
                smooth: Vec::new(),
            });
        }
        loops.push(steps);
    }
    out.faces.push(Face {
        surface: f.surface.clone(),
        outward: off < 0.0,
        loops,
        tolerance,
        name: Some(format!("{id}/near")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    out.faces.extend(sides);
    Ok(out)
}

/// The solid between face `face` of `solid` and `far`, a surface laid
/// out as the face's own is: its sides run from each point of the
/// face's edges to the point of `far` at the same (u, v). `off` says
/// which side of the face's surface `far` is on: that of its normal
/// ∂u × ∂v (more than nothing) or the other.
fn slab_between(
    solid: &Solid,
    face: usize,
    far: Arc<NurbsSurface>,
    off: f64,
    id: &str,
) -> anyhow::Result<Solid> {
    let f = &solid.faces[face];
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let within = size * 1e-9;
    let none = within * 16.0;
    let mut out = Solid::default();
    // The face's vertices, and each one's twin on the far surface with
    // the edge between the two.
    let mut near: BTreeMap<usize, usize> = BTreeMap::new();
    let mut twin: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    let tolerance = f.tolerance.max(within);
    for step in f.loops.iter().flatten() {
        let (a, b) = solid.ends(step);
        let (ta, tb) = solid.span(step);
        for (v, t) in [(a, ta), (b, tb)] {
            if near.contains_key(&v) {
                continue;
            }
            let uv = step.pcurve.point_at(t);
            let from = solid.vertices[v].point;
            let to = far.point_at(uv.x, uv.y);
            let lower = out.vertices.len();
            out.vertices.push(Vertex {
                point: from,
                tolerance: solid.vertices[v].tolerance,
                name: None,
            });
            near.insert(v, lower);
            // (Where the two come together, the vertex is its own twin.)
            if from.distance(to) <= none.max(tolerance) {
                twin.insert(v, (lower, usize::MAX));
                continue;
            }
            out.vertices.push(Vertex {
                point: to,
                tolerance: solid.vertices[v].tolerance.max(within * 4.0),
                name: None,
            });
            twin.insert(v, (lower + 1, usize::MAX));
        }
    }
    // The edge from a vertex to its twin, where a face between an edge
    // and its twin ends there.
    let walked = |edge: usize| {
        let steps = f.loops.iter().flatten();
        steps
            .filter(|s| matches!(s.walk, Walk::Edge { edge: other, .. } if other == edge))
            .count()
    };
    for step in f.loops.iter().flatten() {
        let Walk::Edge { edge, .. } = step.walk else {
            continue;
        };
        if walked(edge) > 1 {
            continue;
        }
        for v in solid.edges[edge].ends {
            if twin[&v].1 != usize::MAX || twin[&v].0 == near[&v] {
                continue;
            }
            let (lower, upper) = (near[&v], twin[&v].0);
            let (from, to) = (out.vertices[lower].point, out.vertices[upper].point);
            out.edges.push(Edge {
                curve: Arc::new(Curve3::line(from, to)?),
                range: (0.0, 1.0),
                ends: [lower, upper],
                tolerance,
                name: Some(format!("{id}/up{}", out.edges.len())),
                also: Vec::new(),
            });
            twin.insert(v, (upper, out.edges.len() - 1));
        }
    }
    // Its edges, each with its twin and the face between them.
    let mut made: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    let mut sides: Vec<Face> = Vec::new();
    for step in f.loops.iter().flatten() {
        let Walk::Edge { edge, reversed } = step.walk else {
            continue;
        };
        if made.contains_key(&edge) {
            continue;
        }
        let e = &solid.edges[edge];
        let along = |t: f64| {
            let uv = step.pcurve.point_at(t);
            far.point_at(uv.x, uv.y)
        };
        let (own, beside) = pair(&e.curve, e.range, &along, within)?;
        let (own, beside) = (Arc::new(own), Arc::new(beside));
        let lower = out.edges.len();
        // (An edge the face is not moved off at all is its own twin.)
        let still = (0..=8).all(|k| {
            let t = e.range.0 + (e.range.1 - e.range.0) * f64::from(k) / 8.0;
            own.point_at(t).distance(beside.point_at(t)) <= none
        });
        if still {
            out.edges.push(Edge {
                curve: own.clone(),
                range: e.range,
                ends: [near[&e.ends[0]], near[&e.ends[1]]],
                tolerance: e.tolerance.max(within),
                name: e.name.clone(),
                also: Vec::new(),
            });
            made.insert(edge, (lower, lower));
            continue;
        }
        out.edges.push(Edge {
            curve: own.clone(),
            range: e.range,
            ends: [near[&e.ends[0]], near[&e.ends[1]]],
            tolerance: e.tolerance.max(within),
            name: e.name.clone(),
            also: Vec::new(),
        });
        out.edges.push(Edge {
            curve: beside.clone(),
            range: e.range,
            ends: [twin[&e.ends[0]].0, twin[&e.ends[1]].0],
            tolerance: e.tolerance.max(within * 2.0),
            name: Some(format!("{id}/far{}", made.len())),
            also: Vec::new(),
        });
        made.insert(edge, (lower, lower + 1));
        // An edge the face walks twice is where its surface closes on
        // itself: nothing is between it and its twin but the slab.
        if walked(edge) > 1 {
            continue;
        }
        // The face between the two: from the edge (0) to its twin (1).
        let (t0, t1) = e.range;
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for k in 0..own.control_points().len() {
            for curve in [&own, &beside] {
                points.push(curve.control_points()[k]);
                weights.push(curve.weights()[k]);
            }
        }
        let ruled = NurbsSurface::new(
            own.degree(),
            1,
            own.knots().to_vec(),
            vec![0.0, 0.0, 1.0, 1.0],
            own.control_points().len(),
            2,
            points,
            Some(weights),
        )?;
        let line = |from: Point2, to: Point2, range: (f64, f64)| -> anyhow::Result<Arc<Curve2>> {
            Ok(Arc::new(Curve2::new(
                1,
                vec![range.0, range.0, range.1, range.1],
                vec![from, to],
                None,
            )?))
        };
        let uv = Point2::new;
        // Up at one end and down at the other: along the edge between
        // a vertex and its twin, or — where the two are one — on the
        // spot.
        let up = |v: usize, t: f64, upwards: bool| -> anyhow::Result<Use> {
            let (from, to) = if upwards { (0.0, 1.0) } else { (1.0, 0.0) };
            if twin[&v].0 == near[&v] {
                return Ok(Use {
                    walk: Walk::Pole {
                        vertex: near[&v],
                        range: (0.0, 1.0),
                    },
                    pcurve: line(uv(t, from), uv(t, to), (0.0, 1.0))?,
                });
            }
            Ok(Use {
                walk: Walk::Edge {
                    edge: twin[&v].1,
                    reversed: !upwards,
                },
                pcurve: line(uv(t, 0.0), uv(t, 1.0), (0.0, 1.0))?,
            })
        };
        let walk = vec![
            Use {
                walk: Walk::Edge {
                    edge: lower,
                    reversed: false,
                },
                pcurve: line(uv(t0, 0.0), uv(t1, 0.0), e.range)?,
            },
            up(e.ends[1], t1, true)?,
            Use {
                walk: Walk::Edge {
                    edge: lower + 1,
                    reversed: true,
                },
                pcurve: line(uv(t0, 1.0), uv(t1, 1.0), e.range)?,
            },
            up(e.ends[0], t0, false)?,
        ];
        sides.push(Face {
            surface: Arc::new(ruled),
            outward: (off > 0.0) == !reversed,
            loops: vec![walk],
            tolerance,
            name: Some(format!("{id}/side{}", sides.len())),
            also: Vec::new(),
            smooth: Vec::new(),
        });
    }
    // Where the face comes to a point its sides leave at an angle (a
    // cone's tip), the far surface does not: the normals there fan out,
    // and it ends in a ring around the point's twin. Between the point
    // and that ring is a face of its own.
    let mut rings: BTreeMap<usize, usize> = BTreeMap::new();
    for step in f.loops.iter().flatten() {
        let Walk::Pole { vertex, range } = step.walk else {
            continue;
        };
        let (a, b) = range;
        let over = |t: f64| {
            let uv = step.pcurve.point_at(t);
            far.point_at(uv.x, uv.y)
        };
        if over(a).distance(over((a + b) / 2.0)) <= none.max(tolerance) {
            continue;
        }
        let (lower, upper) = (near[&vertex], twin[&vertex].0);
        ensure!(
            upper != lower && !rings.contains_key(&vertex),
            "the face comes to a point in a way that is not thickened yet"
        );
        let tip = out.vertices[lower].point;
        // The ring: a cubic through the far surface along the point's
        // side, as close as anything here.
        let like = Curve3::new(
            3,
            vec![a, a, a, a, b, b, b, b],
            (0..4)
                .map(|k| over(a + (b - a) * f64::from(k) / 3.0))
                .collect(),
            None,
        )?;
        let (_, ring) = pair(&like, range, &over, within)?;
        let ring = Arc::new(ring);
        out.edges.push(Edge {
            curve: ring.clone(),
            range,
            ends: [upper, upper],
            tolerance: tolerance.max(within * 2.0),
            name: Some(format!("{id}/apex{}", rings.len())),
            also: Vec::new(),
        });
        let around = out.edges.len() - 1;
        rings.insert(vertex, around);
        if twin[&vertex].1 == usize::MAX {
            out.edges.push(Edge {
                curve: Arc::new(Curve3::line(tip, out.vertices[upper].point)?),
                range: (0.0, 1.0),
                ends: [lower, upper],
                tolerance,
                name: Some(format!("{id}/up{}", out.edges.len())),
                also: Vec::new(),
            });
            twin.insert(vertex, (upper, out.edges.len() - 1));
        }
        let up = twin[&vertex].1;
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for (p, w) in ring.control_points().iter().zip(ring.weights()) {
            points.extend([tip, *p]);
            weights.extend([*w, *w]);
        }
        let fan = NurbsSurface::new(
            ring.degree(),
            1,
            ring.knots().to_vec(),
            vec![0.0, 0.0, 1.0, 1.0],
            ring.control_points().len(),
            2,
            points,
            Some(weights),
        )?;
        let line = |from: Point2, to: Point2, range: (f64, f64)| -> anyhow::Result<Arc<Curve2>> {
            Ok(Arc::new(Curve2::new(
                1,
                vec![range.0, range.0, range.1, range.1],
                vec![from, to],
                None,
            )?))
        };
        let uv = Point2::new;
        let walk = vec![
            Use {
                walk: Walk::Pole {
                    vertex: lower,
                    range,
                },
                pcurve: line(uv(a, 0.0), uv(b, 0.0), range)?,
            },
            Use {
                walk: Walk::Edge {
                    edge: up,
                    reversed: false,
                },
                pcurve: line(uv(b, 0.0), uv(b, 1.0), (0.0, 1.0))?,
            },
            Use {
                walk: Walk::Edge {
                    edge: around,
                    reversed: true,
                },
                pcurve: line(uv(a, 1.0), uv(b, 1.0), range)?,
            },
            Use {
                walk: Walk::Edge {
                    edge: up,
                    reversed: true,
                },
                pcurve: line(uv(a, 0.0), uv(a, 1.0), (0.0, 1.0))?,
            },
        ];
        // Which way is out of the slab there: away from what is between
        // the face and the far surface, a little way in from the point.
        let middle = (a + b) / 2.0;
        let at_pole = step.pcurve.point_at(middle);
        let ((u0, u1), (v0, v1)) = f.surface.domain();
        let centre = Point2::new((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        let inside_uv = at_pole.lerp(centre, 0.05);
        let inside = (f.surface.point_at(inside_uv.x, inside_uv.y)
            + far.point_at(inside_uv.x, inside_uv.y))
            / 2.0;
        let on_fan = fan.point_at(middle, 0.5);
        let outward = normal(&fan, uv(middle, 0.5)).dot(on_fan - inside) > 0.0;
        sides.push(Face {
            surface: Arc::new(fan),
            outward,
            loops: vec![walk],
            tolerance,
            name: Some(format!("{id}/tip{}", sides.len())),
            also: Vec::new(),
            smooth: Vec::new(),
        });
    }
    // The face itself and its twin, walked as the face is.
    let copy = |which: usize| -> Vec<Vec<Use>> {
        f.loops
            .iter()
            .map(|walk| {
                walk.iter()
                    .map(|step| Use {
                        walk: match step.walk {
                            Walk::Edge { edge, reversed } => Walk::Edge {
                                edge: if which == 0 {
                                    made[&edge].0
                                } else {
                                    made[&edge].1
                                },
                                reversed,
                            },
                            // (On the far surface, around the ring
                            // the point's twin is on, if there is one.)
                            Walk::Pole { vertex, .. }
                                if which == 1 && rings.contains_key(&vertex) =>
                            {
                                Walk::Edge {
                                    edge: rings[&vertex],
                                    reversed: false,
                                }
                            }
                            Walk::Pole { vertex, range } => Walk::Pole {
                                vertex: if which == 0 {
                                    near[&vertex]
                                } else {
                                    twin[&vertex].0
                                },
                                range,
                            },
                        },
                        pcurve: step.pcurve.clone(),
                    })
                    .collect()
            })
            .collect()
    };
    out.faces.push(Face {
        surface: f.surface.clone(),
        outward: off < 0.0,
        loops: copy(0),
        tolerance,
        name: Some(format!("{id}/near")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    out.faces.push(Face {
        surface: far,
        outward: off > 0.0,
        loops: copy(1),
        tolerance,
        name: Some(format!("{id}/far")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    out.faces.extend(sides);
    Ok(out)
}

/// How the two faces along edge `edge` of `solid` meet, halfway along
/// it: the angle the body fills between them (less than a half turn at
/// an outside edge, more at an inside one), or `None` where the edge is
/// not between two faces or they run on smoothly into each other.
pub fn opening(solid: &Solid, edge: usize) -> Option<f64> {
    let e = &solid.edges[edge];
    let t = e.middle();
    let along = e.curve.derivative_at(t).1.try_normalize()?;
    let mut sides: Vec<(Point3, Point3)> = Vec::new();
    for face in &solid.faces {
        for step in face.loops.iter().flatten() {
            let Walk::Edge {
                edge: walked,
                reversed,
            } = step.walk
            else {
                continue;
            };
            if walked != edge {
                continue;
            }
            let own = normal(&face.surface, step.pcurve.point_at(t));
            let walk = if reversed { -along } else { along };
            let out = if face.outward { own } else { -own };
            sides.push((own.cross(walk).normalize_or_zero(), out));
        }
    }
    let &[(into_a, _), (into_b, out_b)] = &sides[..] else {
        return None;
    };
    let lean = out_b.dot(into_a);
    let between = into_a.dot(into_b).clamp(-1.0, 1.0).acos();
    if lean.abs() < 1e-6 && between > std::f64::consts::PI - 1e-6 {
        return None;
    }
    Some(if lean < 0.0 {
        between
    } else {
        std::f64::consts::TAU - between
    })
}

/// The tube of radius `radius` around edge `edge` of `solid`: every
/// point within `radius` of the edge, between the planes square to it at
/// its ends. Its faces are called `{id}/side/e0` to `e3`.
pub fn tube(solid: &Solid, edge: usize, radius: f64, id: &str) -> anyhow::Result<Solid> {
    use crate::sweep::{self, Loft};
    let e = &solid.edges[edge];
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    // Around a circle smaller than the tube is wide, it closes over the
    // circle's middle.
    if e.ends[0] == e.ends[1] {
        let (t0, t1) = e.range;
        let at = |k: usize| e.curve.point_at(t0 + (t1 - t0) * k as f64 / 12.0);
        let centre = (0..12).map(at).sum::<Point3>() / 12.0;
        let ring = at(0).distance(centre);
        let axis = (at(0) - centre).cross(at(3) - centre);
        let round = axis.length() > 0.0
            && (0..12).all(|k| {
                let r = at(k) - centre;
                (r.length() - ring).abs() <= size * 1e-9
                    && r.dot(axis).abs() <= size * 1e-9 * axis.length()
            });
        if round && ring < radius * (1.0 - 1e-9) {
            return crate::blend::apple(id, centre, axis, ring, radius);
        }
    }
    // Turned as a face beside the edge is: the tube around a circle on
    // a cylinder then turns with the circle, and is a torus exactly.
    let beside = solid
        .faces
        .iter()
        .flat_map(|face| face.loops.iter().flatten().map(move |step| (face, step)))
        .find(|(_, step)| matches!(step.walk, Walk::Edge { edge: walked, .. } if walked == edge))
        .context("the edge is of no face")?;
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let mut flat = |t: f64| -> anyhow::Result<Vec<[f64; 4]>> {
        let (at, d) = e.curve.derivative_at(t);
        let along = d.try_normalize().context("the edge has no direction")?;
        let (face, step) = beside;
        let x = normal(&face.surface, step.pcurve.point_at(t))
            .reject_from_normalized(along)
            .try_normalize()
            .context("the edge runs into its face")?;
        let y = along.cross(x);
        let corner = |k: usize| match k % 4 {
            0 => x,
            1 => y,
            2 => -x,
            _ => -y,
        };
        let mut ring = Vec::new();
        for k in 0..4 {
            let (a, b) = (corner(k), corner(k + 1));
            let (p, m, q) = (at + a * radius, at + (a + b) * radius, at + b * radius);
            ring.extend([
                [p.x, p.y, p.z, 1.0],
                [m.x, m.y, m.z, w],
                [q.x, q.y, q.z, 1.0],
            ]);
        }
        Ok(ring)
    };
    let (t0, t1) = e.range;
    let (degree, knots, mut rings) =
        sweep::along(&e.curve, (t0, t1), size * 1e-9, size, &mut flat)?;
    let closed = e.ends[0] == e.ends[1] && {
        let (d0, d1) = (e.curve.derivative_at(t0).1, e.curve.derivative_at(t1).1);
        d0.normalize_or_zero().dot(d1.normalize_or_zero()) > 1.0 - 1e-8
    };
    if closed {
        let first = rings[0].clone();
        *rings.last_mut().context("no sections")? = first;
    }
    let arc = || (2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let loft = Loft {
        sides: vec![arc(), arc(), arc(), arc()],
        counts: vec![3; 4],
        degree,
        knots,
        rings,
        closed,
        tolerance: (size * 1e-9).max(1e-9),
    };
    let mut scratch = workpiece_core::world::topo::Topo::new();
    let body = sweep::loft(&mut scratch, id, &loft)?;
    let mut tube = workpiece_boolean::read(&scratch, body)?;
    // Its ends lie in the balls about the edge's ends, which it runs on
    // into smoothly.
    let rims: Vec<usize> = (0..tube.edges.len())
        .filter(|&k| {
            tube.edges[k].name.as_deref().is_some_and(|name| {
                name.starts_with(&format!("{id}/start/e"))
                    || name.starts_with(&format!("{id}/end/e"))
            })
        })
        .collect();
    for face in &mut tube.faces {
        if face
            .name
            .as_deref()
            .is_some_and(|n| n.starts_with(&format!("{id}/side/")))
        {
            face.smooth = rims.clone();
        }
    }
    Ok(tube)
}

/// `solid` grown by `distance` (every point within it of the solid) or,
/// for a distance less than nothing, shrunk by it (every point of the
/// solid at least that far inside it) — or, with faces `open` (none of
/// them, for a closed hollow), its shell: the wall of every point of
/// the solid within the distance of its other faces.
pub fn thickened(
    solid: &Solid,
    distance: f64,
    open: Option<&[usize]>,
    sharp: bool,
) -> anyhow::Result<Solid> {
    thickened_once(solid, distance, open, sharp, false, true)
}

/// The shell of `solid` with walls `thickness` thick inside it, open
/// through its faces `open`, as a part is hollowed: every other face
/// moved in by the thickness, and the moved faces run on or cut back to
/// meet each other — sharply too inside an inside edge of the solid,
/// where [`thickened`] leaves the round that keeps every point of the
/// wall within the thickness of the faces.
pub fn shelled(solid: &Solid, thickness: f64, open: &[usize]) -> anyhow::Result<Solid> {
    let sharp = match thickened_once(solid, -thickness, Some(open), false, true, true) {
        Ok(made) => return Ok(made),
        Err(sharp) => sharp,
    };
    // Where no two walls meet at an inside edge, the walls kept sharp
    // are those of the shell with the rounds: that one is the same.
    // Elsewhere it is not, and it is not made instead.
    let half = std::f64::consts::PI;
    let inside = (0..solid.edges.len()).find(|&e| {
        let mut beside: Vec<usize> = Vec::new();
        for (f, face) in solid.faces.iter().enumerate() {
            for step in face.loops.iter().flatten() {
                if matches!(step.walk, Walk::Edge { edge, .. } if edge == e) && !beside.contains(&f)
                {
                    beside.push(f);
                }
            }
        }
        beside.len() == 2
            && beside.iter().all(|f| !open.contains(f))
            && opening(solid, e).is_some_and(|angle| angle > half + 1e-9)
    });
    if let Some(e) = inside {
        return Err(sharp.context(format!(
            "the walls cannot be brought to meet sharply inside the inside edge {:?}",
            solid.edges[e].name.as_deref().unwrap_or("of the body")
        )));
    }
    thickened(solid, -thickness, Some(open), false).map_err(|_| sharp)
}

/// [`thickened`]; `meeting` says whether walls that leave no hollow are
/// looked at again a hair thinner, to tell those that just meet from
/// those too thick.
fn thickened_once(
    solid: &Solid,
    distance: f64,
    open: Option<&[usize]>,
    sharp: bool,
    meet: bool,
    meeting: bool,
) -> anyhow::Result<Solid> {
    // A wall is run on past an opening as far as the body goes; where
    // that takes it through something else of the body (past a step
    // the next wall leans over), only as far as its inside reaches.
    if distance < 0.0 {
        let size = {
            let (lo, hi) = solid.extent();
            (hi - lo).length()
        };
        if let Some(made) = crate::tweak::shrunk_same(solid, -distance, open, size) {
            return Ok(made);
        }
    }
    let first = match thickened_reaching(solid, distance, open, sharp, meet, meeting, false) {
        Ok(made) => return Ok(made),
        Err(first) => first,
    };
    if open.is_none() || distance > 0.0 {
        return Err(first);
    }
    thickened_reaching(solid, distance, open, sharp, meet, meeting, true).map_err(|_| first)
}

/// [`thickened_once`]; `short` says whether a wall runs on past an
/// opening only as far as its inside reaches past the edge between
/// them (not at all, where they meet at a right angle or less).
fn thickened_reaching(
    solid: &Solid,
    distance: f64,
    open: Option<&[usize]>,
    sharp: bool,
    meet: bool,
    meeting: bool,
    short: bool,
) -> anyhow::Result<Solid> {
    let shell = open.is_some();
    let open = open.unwrap_or(&[]);
    use workpiece_boolean::{Op, combine};
    ensure!(distance != 0.0 && distance.is_finite(), "by some distance");
    let d = distance.abs();
    let grows = distance > 0.0;
    let wall = |f: usize| !open.contains(&f);
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let mut faces_of: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk
                && !faces_of[edge].contains(&f)
            {
                faces_of[edge].push(f);
            }
        }
    }
    let half = std::f64::consts::PI;
    // The edges between two walls at which their slabs part (an outside
    // edge of what grows, an inside one of what shrinks), each with how
    // far a face has to run on past it to meet the other; and, for a
    // shell into the body, those between a wall and an opening where
    // the body opens wider than a right angle (but not about an inside
    // edge: the wall there ends flush with the opening's surface).
    // (Hollowed as a part is, the walls of a shell into the body meet
    // sharply inside an inside edge of the body: each runs on to the
    // other — no round is left there, as there is in a body shrunk to
    // what is a distance inside it.)
    let meet = sharp || (meet && shell && !grows);
    let mut parting: Vec<(usize, f64)> = Vec::new();
    let mut tubed: Vec<usize> = Vec::new();
    for (e, beside) in faces_of.iter().enumerate() {
        let walls = beside.iter().filter(|f| wall(**f)).count();
        let Some(angle) = opening(solid, e) else {
            continue;
        };
        let gap = if grows { half - angle } else { angle - half };
        if walls == 2 && gap > 1e-9 {
            parting.push((e, d * (gap / 2.0).tan()));
            if !meet {
                tubed.push(e);
            }
        } else if walls == 1 && !grows && angle > half / 2.0 + 1e-9 && angle < half {
            tubed.push(e);
        }
    }
    if sharp {
        tubed.clear();
    }
    // A wall that meets an opening at an inside edge ends flush with the
    // opening's surface: it is run on a little past the edge, and cut
    // off by what is under that surface there — under the opening, and
    // beyond it past that edge.
    let mut under: BTreeMap<usize, Solid> = BTreeMap::new();
    for (e, beside) in faces_of.iter().enumerate() {
        let inside = opening(solid, e).is_some_and(|angle| angle > half + 1e-9);
        let (&[a, b], true) = (&beside[..], inside && !grows) else {
            continue;
        };
        let f = match (wall(a), wall(b)) {
            (true, false) => b,
            (false, true) => a,
            _ => continue,
        };
        let deep = -5.0 * d;
        let id = format!("thick/under{e}");
        let made = run_on(solid, f, &[(e, 5.0 * d)]).and_then(|wider| slab(&wider, 0, deep, &id));
        // (Where that is not made, the wall ends at the edge.)
        if let Ok(strip) = made
            && !strip.is_empty()
        {
            under.insert(e, strip);
        }
    }
    let mut pieces: Vec<(String, Solid)> = Vec::new();
    let mut tips: Vec<usize> = Vec::new();
    for f in (0..solid.faces.len()).filter(|f| wall(*f)) {
        let id = format!("thick/f{f}");
        // A wall runs on past the edges it shares with an opening, as
        // far as its surface goes (its inside ends where it meets the
        // opening's surface, not at the opening's edge); and, kept
        // sharp, past the edges it parts from its neighbours at.
        let mut far: Vec<(usize, f64)> = (0..solid.edges.len())
            .filter(|&e| faces_of[e].contains(&f) && faces_of[e].iter().any(|g| !wall(*g)))
            .filter(|_| !grows)
            // (Past an inside edge only a little: it is cut off there
            // at the opening's surface, below.)
            .filter_map(|e| {
                if under.contains_key(&e) {
                    return Some((e, 3.0 * d));
                }
                if !short {
                    return Some((e, size));
                }
                // (As far as what is within the wall's thickness of
                // it lies past the edge, and half as much again.)
                let past = opening(solid, e)? - half / 2.0;
                (past > 1e-9).then(|| (e, size.min(1.5 * d * past.tan())))
            })
            .collect();
        if meet {
            far.extend(parting.iter().filter(|p| faces_of[p.0].contains(&f)));
        }
        let made = if far.is_empty() {
            slab(solid, f, distance, &id)
        } else {
            run_on(solid, f, &far).and_then(|wider| slab(&wider, 0, distance, &id))
        };
        // (A body of one face grown until it closes over its own hole
        // is what is left of its grown face.)
        if made.is_err()
            && grows
            && !shell
            && let Some(closed) = closed_over(solid, distance, &id)
        {
            return Ok(closed);
        }
        let mut piece =
            made.with_context(|| format!("thickening face {:?}", solid.faces[f].name))?;
        for (e, strip) in &under {
            if faces_of[*e].contains(&f) {
                piece = combine(Op::Cut, &piece, strip)
                    .with_context(|| format!("ending the wall {:?}", solid.faces[f].name))?
                    .solid;
            }
        }
        // (A round moved through its axis: what it passes past it too.)
        if let Some(past) = past_axis(solid, f, distance, &format!("{id}/x")) {
            pieces.push((format!("{id}/x"), past));
        }
        // Its far face runs on smoothly into the tubes around its edges.
        let rims: Vec<usize> = (0..piece.edges.len())
            .filter(|&k| {
                let name = piece.edges[k].name.as_deref();
                name.is_some_and(|n| n.starts_with(&format!("{id}/far")))
            })
            .collect();
        for face in &mut piece.faces {
            if face.name.as_deref() == Some(&format!("{id}/far")) {
                face.smooth = rims.clone();
            }
        }
        // Around a point the face comes to (a cone's tip), what has
        // grown is round: a ball about the point.
        if grows && !sharp {
            let apex = format!("{id}/apex");
            if piece
                .edges
                .iter()
                .any(|e| e.name.as_deref().is_some_and(|n| n.starts_with(&apex)))
            {
                for step in solid.faces[f].loops.iter().flatten() {
                    if let Walk::Pole { vertex, .. } = step.walk {
                        tips.push(vertex);
                    }
                }
            }
        }
        pieces.push((id, piece));
    }
    for &e in &tubed {
        let id = format!("thick/e{e}");
        pieces.push((id.clone(), tube(solid, e, d, &id)?));
    }
    // (A tube all the way around an edge that closes on itself has no
    // end to round off.)
    let mut corners: Vec<usize> = tubed
        .iter()
        .filter(|&&e| solid.edges[e].ends[0] != solid.edges[e].ends[1])
        .flat_map(|&e| solid.edges[e].ends)
        .chain(tips)
        .collect();
    corners.sort_unstable();
    corners.dedup();
    for v in corners {
        let id = format!("thick/v{v}");
        // (Turned to no plane of the body: its seam and its poles then
        // lie in none of the body's faces. Turned from the way the
        // body's own edges leave the corner, so that it is the same
        // ball however the body lies.)
        let centre = solid.vertices[v].point;
        let mut ways: Vec<Point3> = Vec::new();
        for edge in solid.edges.iter().filter(|e| e.ends.contains(&v)) {
            let (t0, t1) = edge.range;
            let at = if edge.ends[0] == v { t0 } else { t1 };
            let towards =
                edge.point(at + (if edge.ends[0] == v { t1 - t0 } else { t0 - t1 }) * 1e-3);
            if let Some(way) = (towards - centre).try_normalize()
                && ways.iter().all(|w: &Point3| w.cross(way).length() > 1e-3)
            {
                ways.push(way);
            }
        }
        let [x, y, z] = match ways[..] {
            [a, b, ..] => {
                let z = a.cross(b).normalize();
                [a, z.cross(a), z]
            }
            _ => [Point3::X, Point3::Y, Point3::Z],
        };
        let axis = Point3::new(1.0, 2.0, 3.0).normalize();
        let turned = |p: Point3| {
            let r = p - centre;
            let (sin, cos) = 1.0_f64.sin_cos();
            let r = r * cos + axis.cross(r) * sin + axis * (axis.dot(r) * (1.0 - cos));
            centre + x * r.x + y * r.y + z * r.z
        };
        let ball = crate::blend::ball(&id, centre, d)?.moved(&turned)?;
        pieces.push((id.clone(), ball));
    }
    // Grown, the body with all of them; shrunk, without. One after the
    // other — and, where one cannot be told apart from what the ones
    // before it left (a wedge whose every face lies in theirs), the
    // other way around: what comes of it is the same.
    let taken = |backwards: bool| -> anyhow::Result<Solid> {
        let mut result = solid.clone();
        let mut order: Vec<&(String, Solid)> = pieces.iter().collect();
        if backwards {
            order.reverse();
        }
        for (id, piece) in order {
            let op = if grows { Op::Union } else { Op::Cut };
            result = combine(op, &result, piece)
                .with_context(|| format!("taking {id} in"))?
                .solid;
        }
        Ok(result)
    };
    // (Or all of them put together first, and taken as one.)
    let together = || -> anyhow::Result<Solid> {
        let mut all = Solid::default();
        for (id, piece) in &pieces {
            all = combine(Op::Union, &all, piece)
                .with_context(|| format!("putting {id} to the others"))?
                .solid;
        }
        let op = if grows { Op::Union } else { Op::Cut };
        Ok(combine(op, solid, &all)?.solid)
    };
    let mut result = match taken(false) {
        Ok(result) => result,
        Err(first) => taken(true).or_else(|_| together()).map_err(|_| first)?,
    };
    if !shell {
        return Ok(result);
    }
    if !grows {
        if result.is_empty() {
            // Walls that just meet leave no hollow, and the body as it
            // was; thicker than fits, there is no such shell.
            let too_thick =
                "the wall is thicker than the body takes: nothing is left to hollow out";
            ensure!(meeting, too_thick);
            let thinner = distance * (1.0 - 1e-6);
            ensure!(
                thickened_once(solid, thinner, Some(open), sharp, meet, false).is_ok(),
                too_thick
            );
            return Ok(solid.clone());
        }
        // A shell into the body: what of the body the shrunk one
        // leaves.
        return Ok(combine(Op::Cut, solid, &result)
            .context("hollowing the body")?
            .solid);
    }
    // A shell out of the body: what has grown around it, cut off at
    // each opening's surface, less the body itself.
    for &f in open {
        let all: Vec<(usize, f64)> = (0..solid.edges.len())
            .filter(|&e| faces_of[e].contains(&f))
            .map(|e| (e, 4.0 * d))
            .collect();
        let id = format!("thick/open{f}");
        let beyond = run_on(solid, f, &all)
            .and_then(|wider| slab(&wider, 0, 4.0 * d, &id))
            .with_context(|| format!("closing the opening {:?}", solid.faces[f].name))?;
        result = combine(Op::Cut, &result, &beyond)
            .context("cutting the wall off at an opening")?
            .solid;
    }
    Ok(combine(Op::Cut, &result, solid)
        .context("hollowing the wall")?
        .solid)
}

/// How far past its low (or high) end in u (or, with `v`, in v) a
/// surface that is straight that way comes to a point, if it does.
fn to_a_point(surface: &NurbsSurface, v: bool, low: bool) -> Option<f64> {
    let (p, q) = surface.degrees();
    let (nu, nv) = surface.counts();
    let (degree, along, rows) = if v { (q, nv, nu) } else { (p, nu, nv) };
    if degree != 1 || along != 2 || rows < 2 {
        return None;
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let length = if v { v1 - v0 } else { u1 - u0 };
    let at = |row: usize, end: usize| {
        let (i, j) = if v { (row, end) } else { (end, row) };
        (surface.control_point(i, j), surface.weights()[i * nv + j])
    };
    let (end, other) = if low { (0, 1) } else { (1, 0) };
    let ((e0, w0), (o0, x0)) = (at(0, end), at(0, other));
    let d0 = (e0 - o0) / length;
    let size = (0..rows).fold(0.0_f64, |m, r| m.max(at(r, other).0.distance(o0)));
    let mut found: Option<f64> = None;
    for row in 0..rows {
        let ((e, w), (o, x)) = (at(row, end), at(row, other));
        if (w * x0 - x * w0).abs() > 1e-12 * w0 * x0 {
            return None;
        }
        let (apart, d) = (e - e0, (e - o) / length - d0);
        if d.length() * length <= size * 1e-12 {
            if apart.length() > size * 1e-12 {
                return None;
            }
            continue;
        }
        let here = -apart.dot(d) / d.length_squared();
        if (apart + d * here).length() > size * 1e-10 {
            return None;
        }
        match found {
            Some(x) if (x - here).abs() > 1e-9 * length => return None,
            _ => found = Some(here),
        }
    }
    found.filter(|x| *x > 1e-9 * length)
}

/// Face `face` of `solid` run on across those of its edges that are in
/// `far` (each with how far), or as far as its surface goes: a solid of that
/// one face (not closed), for a slab that reaches past the face's own
/// edges — what a wall's inside is where it runs on to an opening.
pub fn run_on(solid: &Solid, face: usize, far: &[(usize, f64)]) -> anyhow::Result<Solid> {
    let across: Vec<usize> = far.iter().map(|x| x.0).collect();
    let across = &across[..];
    let reach_of = |edge: usize| far.iter().find(|x| x.0 == edge).map_or(0.0, |x| x.1);
    let f = &solid.faces[face];
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let within = size * 1e-9;
    // The surface itself first, where an edge to be crossed is at its
    // rim (a cylinder's end): run on as what it is, as far as asked —
    // unless it closes on itself that way, or comes to a point.
    let mut wider: Arc<NurbsSurface> = f.surface.clone();
    {
        let surface = &f.surface;
        let ((u0, u1), (v0, v1)) = surface.domain();
        let span = Point2::new(u1 - u0, v1 - v0);
        // How much further each side has to be: low u, high u, low v,
        // high v.
        let mut need = [0.0_f64; 4];
        for step in f.loops.iter().flatten() {
            let Walk::Edge { edge, reversed } = step.walk else {
                continue;
            };
            if !across.contains(&edge) {
                continue;
            }
            let e = &solid.edges[edge];
            for k in 0..=16 {
                let t = e.range.0 + (e.range.1 - e.range.0) * f64::from(k) / 16.0;
                let uv = step.pcurve.point_at(t);
                let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
                let along = e.curve.derivative_at(t).1.normalize_or_zero();
                let walk = if reversed { -along } else { along };
                let away = -normal(surface, uv).cross(walk);
                let (a, b, c) = (du.dot(du), du.dot(dv), dv.dot(dv));
                let det = a * c - b * b;
                if det.abs() <= 1e-300 {
                    continue;
                }
                let (p, q) = (du.dot(away), dv.dot(away));
                let g = Point2::new((p * c - q * b) / det, (q * a - p * b) / det);
                let to = uv + g * reach_of(edge);
                need[0] = need[0].max(u0 - to.x);
                need[1] = need[1].max(to.x - u1);
                need[2] = need[2].max(v0 - to.y);
                need[3] = need[3].max(to.y - v1);
            }
        }
        let weights = surface.weights();
        let (nu, nv) = surface.counts();
        for (side, &more) in need.iter().enumerate() {
            let (v, low) = (side >= 2, side % 2 == 0);
            let room = if v { span.y } else { span.x };
            if more <= room * 1e-9 {
                continue;
            }
            // (Not around what closes, nor past a point.)
            let ends = |k: f64| {
                if v {
                    let u = u0 + span.x * k;
                    (surface.point_at(u, v0), surface.point_at(u, v1))
                } else {
                    let w = v0 + span.y * k;
                    (surface.point_at(u0, w), surface.point_at(u1, w))
                }
            };
            let closes = [0.2, 0.7].into_iter().all(|k| {
                let (a, b) = ends(k);
                a.distance(b) <= within
            });
            let side_at = |k: f64| if low { ends(k).0 } else { ends(k).1 };
            let pointed = side_at(0.1).distance(side_at(0.6)) <= within;
            if closes || pointed {
                continue;
            }
            // (A weighted direction — an arc — only half as far again:
            // run on in its parameter, it comes around and breaks.)
            let weighted = (0..nu).any(|i| {
                (0..nv).any(|j| {
                    let other = if v { i * nv } else { j };
                    (weights[i * nv + j] - weights[other]).abs() > 1e-12
                })
            });
            // (A little further than asked: what is run on to stops a
            // little short of the surface's rim.)
            let more = (more * 1.02 + room * 0.02).min(room * if weighted { 0.5 } else { 64.0 });
            // (But no further than where it comes to a point: a ring
            // run on to its middle, a cone to its tip.)
            let more = to_a_point(&wider, v, low)
                .filter(|x| *x <= more * (1.0 + 1e-9))
                .unwrap_or(more);
            let ((a0, a1), (b0, b1)) = wider.domain();
            let to = match (v, low) {
                (false, true) => a0 - more,
                (false, false) => a1 + more,
                (true, true) => b0 - more,
                (true, false) => b1 + more,
            };
            wider = Arc::new(run_on_surface(&wider, v, low, to)?);
        }
    }
    let surface = &wider;
    let ((u0, u1), (v0, v1)) = surface.domain();
    let span = Point2::new(u1 - u0, v1 - v0);
    let mut out = Solid::default();
    let mut vertex_of: BTreeMap<usize, usize> = BTreeMap::new();
    let mut edge_of: BTreeMap<usize, usize> = BTreeMap::new();
    let mut loops = Vec::new();
    let mut take_vertex = |out: &mut Solid, v: usize| {
        *vertex_of.entry(v).or_insert_with(|| {
            out.vertices.push(solid.vertices[v].clone());
            out.vertices.len() - 1
        })
    };
    // A curve through `of` over the knots of `like`.
    let fitted = |like: &Curve3, range: (f64, f64), of: &dyn Fn(f64) -> Point3| {
        pair(like, range, of, within).map(|(_, beside)| beside)
    };
    for walk in &f.loops {
        let mut steps = Vec::new();
        for step in walk {
            let Walk::Edge { edge, reversed } = step.walk else {
                let Walk::Pole { vertex, range } = step.walk else {
                    unreachable!()
                };
                steps.push(Use {
                    walk: Walk::Pole {
                        vertex: take_vertex(&mut out, vertex),
                        range,
                    },
                    pcurve: step.pcurve.clone(),
                });
                continue;
            };
            let e = &solid.edges[edge];
            let ends = [
                take_vertex(&mut out, e.ends[0]),
                take_vertex(&mut out, e.ends[1]),
            ];
            let own = *edge_of.entry(edge).or_insert_with(|| {
                out.edges.push(Edge { ends, ..e.clone() });
                out.edges.len() - 1
            });
            let twice = f
                .loops
                .iter()
                .flatten()
                .filter(|s| matches!(s.walk, Walk::Edge { edge: other, .. } if other == edge))
                .count()
                > 1;
            let kept = Use {
                walk: Walk::Edge {
                    edge: own,
                    reversed,
                },
                pcurve: step.pcurve.clone(),
            };
            if !across.contains(&edge) || twice {
                steps.push(kept);
                continue;
            }
            // Out of the face across the edge, in (u, v): what there is
            // one length of in space, square to the edge.
            let out_at = |t: f64| -> Point2 {
                let uv = step.pcurve.point_at(t);
                let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
                let along = e.curve.derivative_at(t).1.normalize_or_zero();
                let walk = if reversed { -along } else { along };
                let away = -normal(surface, uv).cross(walk);
                // (du, dv)·g = away, as nearly as can be.
                let (a, b, c) = (du.dot(du), du.dot(dv), dv.dot(dv));
                let (p, q) = (du.dot(away), dv.dot(away));
                let det = a * c - b * b;
                if det.abs() <= 1e-300 {
                    return Point2::ZERO;
                }
                Point2::new((p * c - q * b) / det, (q * a - p * b) / det)
            };
            // As far as the surface goes there (a little short of its
            // rim: of where it may come to a point).
            let (t0, t1) = e.range;
            let step_uv = |t: f64| step.pcurve.point_at(t);
            // (Where the surface comes to a point there — a ball's pole
            // — all the way to it; else a little short of its rim.)
            let pointed = |v: bool, low: bool| {
                let at = |k: f64| {
                    if v {
                        surface.point_at(u0 + span.x * k, if low { v0 } else { v1 })
                    } else {
                        surface.point_at(if low { u0 } else { u1 }, v0 + span.y * k)
                    }
                };
                at(0.1).distance(at(0.6)) <= within && at(0.3).distance(at(0.9)) <= within
            };
            let reach = reach_of(edge);
            let mut far = reach;
            let mut pole: Option<(bool, bool)> = None;
            for k in 0..=32 {
                let t = t0 + (t1 - t0) * f64::from(k) / 32.0;
                let (uv, g) = (step.pcurve.point_at(t), out_at(t));
                for (v, at, by, lo, hi, room) in [
                    (false, uv.x, g.x, u0, u1, span.x),
                    (true, uv.y, g.y, v0, v1, span.y),
                ] {
                    // (Not that way at all, but for the last bits of
                    // the arithmetic.)
                    if by.abs() * reach <= room * 1e-12 {
                        continue;
                    }
                    let low = by < 0.0;
                    let short = if pointed(v, low) { 0.0 } else { room * 1e-3 };
                    let edge_of_it = if low { lo + short } else { hi - short };
                    let here = ((edge_of_it - at) / by).max(0.0);
                    if here < far {
                        far = here;
                        pole = (short == 0.0).then_some((v, low));
                    }
                }
            }
            // And no further than the edge can be moved out before it
            // comes to nothing: the rim of a hole, moved into the hole.
            let mut least = f64::INFINITY;
            for k in 0..=32 {
                let t = t0 + (t1 - t0) * (f64::from(k) + 0.5) / 33.0;
                let h = (t1 - t0) * 1e-3;
                let (a, b, c) = (
                    e.curve.point_at((t - h).max(t0)),
                    e.curve.point_at(t),
                    e.curve.point_at((t + h).min(t1)),
                );
                let (ab, bc) = (b - a, c - b);
                let step = (ab.length() + bc.length()) / 2.0;
                if step <= 0.0 {
                    continue;
                }
                let bend = (bc.normalize_or_zero() - ab.normalize_or_zero()) / step;
                let uv = step_uv(t);
                let along = e.curve.derivative_at(t).1.normalize_or_zero();
                let walk = if reversed { -along } else { along };
                let towards = bend.dot(-normal(surface, uv).cross(walk));
                if towards > 1e-12 {
                    least = least.min(1.0 / towards);
                }
            }
            // (Short of that, unless the surface ends first.)
            // (Where the surface comes to a point just there, the edge
            // comes to nothing at it: that is closed, above.)
            if least < far * (1.0 - 1e-6) || (pole.is_none() && least < far) {
                far = 0.9 * least;
                pole = None;
            }
            if far <= reach * 1e-6 || far <= within * 1e3 {
                steps.push(kept);
                continue;
            }
            let moved = |t: f64| step.pcurve.point_at(t) + out_at(t) * far;
            // Out to a point the surface comes to: the face closes
            // there, if the whole edge comes to it together.
            let together = pole.filter(|&(v, low)| {
                let side = match (v, low) {
                    (true, true) => v0,
                    (true, false) => v1,
                    (false, true) => u0,
                    (false, false) => u1,
                };
                e.ends[0] == e.ends[1]
                    && (0..=16).all(|k| {
                        let at = moved(t0 + (t1 - t0) * f64::from(k) / 16.0);
                        let off = if v { at.y - side } else { at.x - side };
                        off.abs() <= 1e-9 * (span.x + span.y)
                    })
            });
            if together.is_some() {
                let on = |uv: Point2| surface.point_at(uv.x, uv.y);
                let (from, to) = (step.pcurve.point_at(t0), moved(t0));
                let tip = out.vertices.len();
                out.vertices.push(Vertex {
                    point: on(to),
                    tolerance: within * 4.0,
                    name: None,
                });
                let cubic = Curve3::new(
                    3,
                    vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                    (0..4)
                        .map(|i| on(from).lerp(on(to), f64::from(i) / 3.0))
                        .collect(),
                    None,
                )?;
                let line = Curve2::line(from, to)?;
                let curve = fitted(&cubic, (0.0, 1.0), &|x| on(line.point_at(x)))?;
                out.edges.push(Edge {
                    curve: Arc::new(curve),
                    range: (0.0, 1.0),
                    ends: [ends[0], tip],
                    tolerance: e.tolerance.max(within * 2.0),
                    name: None,
                    also: Vec::new(),
                });
                let way = out.edges.len() - 1;
                let (first, last) = if reversed { (t1, t0) } else { (t0, t1) };
                let out_line =
                    |t: f64| Curve2::line(step.pcurve.point_at(t), moved(t)).map(Arc::new);
                steps.push(Use {
                    walk: Walk::Edge {
                        edge: way,
                        reversed: false,
                    },
                    pcurve: out_line(first)?,
                });
                steps.push(Use {
                    walk: Walk::Pole {
                        vertex: tip,
                        range: (0.0, 1.0),
                    },
                    pcurve: Arc::new(Curve2::line(moved(first), moved(last))?),
                });
                steps.push(Use {
                    walk: Walk::Edge {
                        edge: way,
                        reversed: true,
                    },
                    pcurve: out_line(last)?,
                });
                continue;
            }
            // The far edge, in (u, v) and in space, over the edge's own
            // parameters.
            let lift = |p: Point2| Point3::new(p.x, p.y, 0.0);
            let flat = Curve3::new(
                step.pcurve.degree(),
                step.pcurve.knots().to_vec(),
                step.pcurve
                    .control_points()
                    .iter()
                    .map(|p| lift(*p))
                    .collect(),
                Some(step.pcurve.weights().to_vec()),
            )?;
            let in_uv = pair(
                &flat,
                e.range,
                &|t| lift(moved(t)),
                1e-12 * (span.x + span.y),
            )?
            .1;
            let far_pcurve = Curve2::new(
                in_uv.degree(),
                in_uv.knots().to_vec(),
                in_uv
                    .control_points()
                    .iter()
                    .map(|p| Point2::new(p.x, p.y))
                    .collect(),
                Some(in_uv.weights().to_vec()),
            )?;
            let on = |uv: Point2| surface.point_at(uv.x, uv.y);
            let far_curve = fitted(&e.curve, e.range, &|t| on(far_pcurve.point_at(t)))?;
            // (An edge that closes on itself, where its face's surface
            // does too, has one way out and back: the seam, run on.)
            let closes = e.ends[0] == e.ends[1]
                && far_curve.point_at(t0).distance(far_curve.point_at(t1)) <= within * 4.0;
            let mut far_ends = [t0, t1].map(|t| {
                out.vertices.push(Vertex {
                    point: far_curve.point_at(t),
                    tolerance: within * 4.0,
                    name: None,
                });
                out.vertices.len() - 1
            });
            if closes {
                out.vertices.pop();
                far_ends[1] = far_ends[0];
            }
            out.edges.push(Edge {
                curve: Arc::new(far_curve),
                range: e.range,
                ends: far_ends,
                tolerance: e.tolerance.max(within * 2.0),
                name: None,
                also: Vec::new(),
            });
            let far_edge = out.edges.len() - 1;
            // The two ways out to it, at the edge's ends: straight in
            // (u, v).
            let mut ways = [0, 0];
            let mut way_pcurves = Vec::new();
            for (k, t) in [t0, t1].into_iter().enumerate() {
                let (from, to) = (step.pcurve.point_at(t), far_pcurve.point_at(t));
                let line = Curve2::line(from, to)?;
                let cubic = Curve3::new(
                    3,
                    vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                    (0..4)
                        .map(|i| on(from).lerp(on(to), f64::from(i) / 3.0))
                        .collect(),
                    None,
                )?;
                let curve = fitted(&cubic, (0.0, 1.0), &|x| on(line.point_at(x)))?;
                way_pcurves.push(Arc::new(line));
                if closes && k == 1 {
                    ways[1] = ways[0];
                    continue;
                }
                out.edges.push(Edge {
                    curve: Arc::new(curve),
                    range: (0.0, 1.0),
                    ends: [ends[k], far_ends[k]],
                    tolerance: e.tolerance.max(within * 2.0),
                    name: Some("thick/way".to_owned()),
                    also: Vec::new(),
                });
                ways[k] = out.edges.len() - 1;
            }
            // Around the edge instead of along it: out at where the
            // walk comes to it, along the far edge, back in.
            let (first, last) = if reversed { (1, 0) } else { (0, 1) };
            steps.push(Use {
                walk: Walk::Edge {
                    edge: ways[first],
                    reversed: false,
                },
                pcurve: way_pcurves[first].clone(),
            });
            steps.push(Use {
                walk: Walk::Edge {
                    edge: far_edge,
                    reversed,
                },
                pcurve: Arc::new(far_pcurve),
            });
            steps.push(Use {
                walk: Walk::Edge {
                    edge: ways[last],
                    reversed: true,
                },
                pcurve: way_pcurves[last].clone(),
            });
        }
        // Where the walk comes back in from one far edge and goes
        // straight out again to the next, it goes around the corner
        // instead: on along the one far edge and back along the other,
        // to where the two meet.
        let on = |uv: Point2| surface.point_at(uv.x, uv.y);
        let is_way = |out: &Solid, step: &Use, back: bool| match step.walk {
            Walk::Edge { edge, reversed } => {
                reversed == back && out.edges[edge].name.as_deref() == Some("thick/way")
            }
            Walk::Pole { .. } => false,
        };
        let mut k = 0;
        while steps.len() >= 4 && k < steps.len() {
            let n = steps.len();
            let (i, j) = (k, (k + 1) % n);
            if !(is_way(&out, &steps[i], true) && is_way(&out, &steps[j], false)) {
                k += 1;
                continue;
            }
            let (before, after) = (&steps[(i + n - 1) % n], &steps[(j + 1) % n]);
            let far_of = |step: &Use| match step.walk {
                Walk::Edge { edge, .. } => out.edges[edge].ends[1],
                Walk::Pole { vertex, .. } => vertex,
            };
            let (from_vertex, to_vertex) = (far_of(&steps[i]), far_of(&steps[j]));
            let (from, to) = (steps[i].pcurve.point_at(1.0), steps[j].pcurve.point_at(1.0));
            // The ways the two far edges run there, as they are walked.
            let heading = |step: &Use, end: bool| -> Point2 {
                let (a, b) = match step.walk {
                    Walk::Edge { edge, reversed } => {
                        let (t0, t1) = out.edges[edge].range;
                        if reversed { (t1, t0) } else { (t0, t1) }
                    }
                    Walk::Pole { range, .. } => range,
                };
                let h = (b - a) * 1e-4;
                if end {
                    step.pcurve.point_at(b) - step.pcurve.point_at(b - h)
                } else {
                    step.pcurve.point_at(a + h) - step.pcurve.point_at(a)
                }
            };
            let (ta, tb) = (heading(before, true), heading(after, false));
            let cross = ta.perp_dot(tb);
            let gap = to - from;
            let meet = (cross.abs() > 1e-9 * ta.length() * tb.length()).then(|| {
                let a = gap.perp_dot(tb) / cross;
                from + ta * a
            });
            let inside = |p: Point2| p.x >= u0 && p.x <= u1 && p.y >= v0 && p.y <= v1;
            let corner =
                meet.filter(|m| inside(*m) && (*m - from).dot(ta) > 0.0 && (to - *m).dot(tb) > 0.0);
            let mut around: Vec<Use> = Vec::new();
            let link =
                |out: &mut Solid, a: (usize, Point2), b: (usize, Point2)| -> anyhow::Result<Use> {
                    let line = Curve2::line(a.1, b.1)?;
                    let cubic = Curve3::new(
                        3,
                        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                        (0..4)
                            .map(|i| on(a.1).lerp(on(b.1), f64::from(i) / 3.0))
                            .collect(),
                        None,
                    )?;
                    let curve = fitted(&cubic, (0.0, 1.0), &|x| on(line.point_at(x)))?;
                    out.edges.push(Edge {
                        curve: Arc::new(curve),
                        range: (0.0, 1.0),
                        ends: [a.0, b.0],
                        tolerance: within * 4.0,
                        name: None,
                        also: Vec::new(),
                    });
                    Ok(Use {
                        walk: Walk::Edge {
                            edge: out.edges.len() - 1,
                            reversed: false,
                        },
                        pcurve: Arc::new(line),
                    })
                };
            // Where the face turns inwards, the two far edges cross
            // instead of parting: each ends where they cross.
            let crossing =
                meet.filter(|m| inside(*m) && (*m - from).dot(ta) < 0.0 && (to - *m).dot(tb) < 0.0);
            let (bi, ai) = ((i + n - 1) % n, (j + 1) % n);
            let mut crossed = false;
            if let (
                Some(m),
                Walk::Edge {
                    edge: eb,
                    reversed: rb,
                },
                Walk::Edge {
                    edge: ea,
                    reversed: ra,
                },
            ) = (crossing, steps[bi].walk, steps[ai].walk)
                && eb != ea
            {
                // Where along an edge its (u, v) curve passes the crossing.
                let passes = |step: &Use, edge: usize, way: Point2| {
                    let (mut lo, mut hi) = out.edges[edge].range;
                    let ahead = |t: f64| (step.pcurve.point_at(t) - m).dot(way);
                    let rising = ahead(hi) > ahead(lo);
                    for _ in 0..80 {
                        let mid = (lo + hi) / 2.0;
                        if (ahead(mid) > 0.0) == rising {
                            hi = mid;
                        } else {
                            lo = mid;
                        }
                    }
                    (lo + hi) / 2.0
                };
                let (tb_at, ta_at) = (passes(&steps[bi], eb, ta), passes(&steps[ai], ea, tb));
                let near_enough = |step: &Use, t: f64| {
                    step.pcurve.point_at(t).distance(m) <= 1e-9 * (span.x + span.y)
                };
                if near_enough(&steps[bi], tb_at) && near_enough(&steps[ai], ta_at) {
                    out.vertices.push(Vertex {
                        point: on(m),
                        tolerance: within * 4.0,
                        name: None,
                    });
                    let at = out.vertices.len() - 1;
                    // (The one walked before ends there; the one after
                    // starts there.)
                    let edge = &mut out.edges[eb];
                    if rb {
                        (edge.range.0, edge.ends[0]) = (tb_at, at);
                    } else {
                        (edge.range.1, edge.ends[1]) = (tb_at, at);
                    }
                    let edge = &mut out.edges[ea];
                    if ra {
                        (edge.range.1, edge.ends[1]) = (ta_at, at);
                    } else {
                        (edge.range.0, edge.ends[0]) = (ta_at, at);
                    }
                    crossed = true;
                }
            }
            match corner {
                _ if crossed => {}
                Some(m) => {
                    out.vertices.push(Vertex {
                        point: on(m),
                        tolerance: within * 4.0,
                        name: None,
                    });
                    let at = out.vertices.len() - 1;
                    around.push(link(&mut out, (from_vertex, from), (at, m))?);
                    around.push(link(&mut out, (at, m), (to_vertex, to))?);
                }
                None if from.distance(to) > 1e-12 * (span.x + span.y) => {
                    around.push(link(&mut out, (from_vertex, from), (to_vertex, to))?);
                }
                // Where the two far edges come to one point (edges that
                // run on into each other smoothly, run on as far), they
                // meet there: the walk goes on from the one into the
                // other, with no slit out and back between them.
                None if from_vertex != to_vertex
                    && matches!(before.walk, Walk::Edge { .. })
                    && matches!(after.walk, Walk::Edge { .. }) =>
                {
                    for edge in &mut out.edges {
                        for end in &mut edge.ends {
                            if *end == to_vertex {
                                *end = from_vertex;
                            }
                        }
                    }
                }
                None => {
                    k += 1;
                    continue;
                }
            }
            // (The later of the two first: the other keeps its place.)
            let (low, high) = (i.min(j), i.max(j));
            steps.remove(high);
            steps.remove(low);
            let at = if j == 0 { steps.len() } else { low };
            for (n, step) in around.into_iter().enumerate() {
                steps.insert(at + n, step);
            }
            k = 0;
        }
        loops.push(steps);
    }
    out.faces.push(Face {
        surface: surface.clone(),
        outward: f.outward,
        loops,
        tolerance: f.tolerance,
        name: f.name.clone(),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    // Only what the face walks.
    let walked: Vec<bool> = (0..out.edges.len())
        .map(|e| {
            out.faces[0]
                .loops
                .iter()
                .flatten()
                .any(|s| matches!(s.walk, Walk::Edge { edge, .. } if edge == e))
        })
        .collect();
    let renumber: Vec<usize> = walked
        .iter()
        .scan(0, |n, w| {
            *n += usize::from(*w);
            Some(n.saturating_sub(1))
        })
        .collect();
    let mut k = 0;
    out.edges.retain(|_| {
        k += 1;
        walked[k - 1]
    });
    for step in out.faces[0].loops.iter_mut().flatten() {
        if let Walk::Edge { edge, .. } = &mut step.walk {
            *edge = renumber[*edge];
        }
    }
    Ok(out)
}

/// A row of weighted control points over `knots` (of `degree`) with its
/// end moved out to `to`: the same curve on what it ran over, run on
/// beyond as the curve itself does (a line straight on, an arc around).
pub(crate) fn run_out(
    degree: usize,
    knots: &[f64],
    row: &[[f64; 4]],
    to: f64,
) -> (Vec<f64>, Vec<[f64; 4]>) {
    let n = row.len();
    // The last piece that is one.
    let mut k = n - 1;
    while k > degree && knots[k] >= knots[k + 1] {
        k -= 1;
    }
    let mut longer = knots.to_vec();
    for knot in longer.iter_mut().skip(n) {
        *knot = to;
    }
    // What the piece is at any `degree` parameters at once (its
    // blossom): at those of a control point's knots, that point.
    let at = |args: &[f64]| -> [f64; 4] {
        let mut d: Vec<[f64; 4]> = row[k - degree..=k].to_vec();
        for r in 1..=degree {
            for j in (r..=degree).rev() {
                let i = k - degree + j;
                let (a, b) = (knots[i], knots[i + degree + 1 - r]);
                let x = (args[r - 1] - a) / (b - a);
                d[j] = std::array::from_fn(|c| d[j - 1][c] * (1.0 - x) + d[j][c] * x);
            }
        }
        d[degree]
    };
    let mut out = row.to_vec();
    for (j, point) in out.iter_mut().enumerate().skip(n.saturating_sub(degree)) {
        *point = at(&longer[j + 1..=j + degree]);
    }
    (longer, out)
}

/// `surface` run on past the end of its u (or, with `v`, its v) — the
/// low end, with `low` — to the parameter `to`.
#[allow(clippy::needless_range_loop)]
pub fn run_on_surface(
    surface: &NurbsSurface,
    v: bool,
    low: bool,
    to: f64,
) -> anyhow::Result<NurbsSurface> {
    let (p, q) = surface.degrees();
    let (nu, nv) = surface.counts();
    let (ku, kv) = surface.knots();
    let at = |i: usize, j: usize| {
        let (point, w) = (surface.control_point(i, j), surface.weights()[i * nv + j]);
        [point.x * w, point.y * w, point.z * w, w]
    };
    // Rows along the direction that is run on, turned about for its low
    // end.
    let (degree, knots, rows, along) = if v { (q, kv, nu, nv) } else { (p, ku, nv, nu) };
    let (mut knots, to) = (knots.to_vec(), if low { -to } else { to });
    if low {
        knots = knots.iter().rev().map(|k| -k).collect();
    }
    let mut made: Vec<Vec<[f64; 4]>> = Vec::new();
    let mut longer = knots.clone();
    for r in 0..rows {
        let mut row: Vec<[f64; 4]> = (0..along)
            .map(|k| if v { at(r, k) } else { at(k, r) })
            .collect();
        if low {
            row.reverse();
        }
        let (k, mut out) = run_out(degree, &knots, &row, to);
        if low {
            out.reverse();
        }
        longer = k;
        made.push(out);
    }
    if low {
        longer = longer.iter().rev().map(|k| -k).collect();
    }
    let (mut points, mut weights) = (Vec::new(), Vec::new());
    for i in 0..nu {
        for j in 0..nv {
            let h = if v { made[i][j] } else { made[j][i] };
            points.push(Point3::new(h[0], h[1], h[2]) / h[3]);
            weights.push(h[3]);
        }
    }
    let (ku, kv) = if v {
        (ku.to_vec(), longer)
    } else {
        (longer, kv.to_vec())
    };
    Ok(NurbsSurface::new(
        p,
        q,
        ku,
        kv,
        nu,
        nv,
        points,
        Some(weights),
    )?)
}

/// `solid` with its faces `faces` moved by `by` without turning.
///
/// Faces that are all of a feature — a hole, a notch, a boss: what is
/// closed over when they are taken off the body ([`crate::heal`]) —
/// are moved as that feature, where it slides along the faces it is cut
/// from or stands on: the hole is filled and drilled anew.
///
/// Any others each alone: a flat face is moved to where its plane goes
/// (sliding in itself, it stays), and the faces beside it run on to
/// meet it where they stand square to it; what any other face sweeps
/// on its way is added to the solid where it moves out of it, and taken
/// from it where it moves in.
pub fn faces_moved(solid: &Solid, faces: &[usize], by: Point3) -> anyhow::Result<Solid> {
    use workpiece_boolean::{Op, combine};
    ensure!(!faces.is_empty(), "pick the faces to move");
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    if let Some(made) = flat_faces_moved(solid, faces, by, size) {
        return Ok(made);
    }
    if let Some(made) = feature_moved(solid, faces, by) {
        return Ok(made);
    }
    let mut swept = Vec::new();
    for (k, &face) in faces.iter().enumerate() {
        let id = format!("moved/f{k}");
        let f = &solid.faces[face];
        // A flat face: by how far its plane goes.
        let flat = f.loops.iter().flatten().next().and_then(|step| {
            let n = normal_out(f, step.pcurve.point_at(solid.span(step).0));
            let flat = f.loops.iter().flatten().all(|step| {
                let (a, b) = solid.span(step);
                (0..=4).all(|k| {
                    let uv = step.pcurve.point_at(a + (b - a) * f64::from(k) / 4.0);
                    (normal_out(f, uv) - n).length() < 1e-9
                })
            });
            flat.then_some(n.dot(by))
        });
        match flat {
            Some(far) if far.abs() <= size * 1e-12 => {}
            Some(far) => swept.push((slab(solid, face, far, &id)?, far > 0.0)),
            None => swept.push(slab_along(solid, face, by, &id)?),
        }
    }
    let mut made = solid.clone();
    // What is added first, then what is taken.
    for out_of in [true, false] {
        for (slab, _) in swept.iter().filter(|s| s.1 == out_of) {
            let op = if out_of { Op::Union } else { Op::Cut };
            made = combine(op, &made, slab).context("moving a face")?.solid;
        }
    }
    Ok(made)
}

/// `solid` with its flat faces `faces` moved by `by`, each by how far
/// its plane goes, made where it is ([`crate::tweak`]): if they are all
/// flat, no two of them that both move share an edge (what becomes of
/// the corner between two such is the slabs' to say), and the body
/// stays made up as it is. `None` if not.
fn flat_faces_moved(solid: &Solid, faces: &[usize], by: Point3, size: f64) -> Option<Solid> {
    let mut far = vec![0.0; solid.faces.len()];
    for &face in faces {
        let f = &solid.faces[face];
        let way = crate::tweak::flat_way(&f.surface, size * 1e-12)?;
        let out = if f.outward { way } else { -way };
        let off = out.dot(by);
        if off.abs() > size * 1e-12 {
            far[face] = off;
        }
    }
    for edge in 0..solid.edges.len() {
        let moving = solid.faces.iter().enumerate().filter(|(f, face)| {
            let mut steps = face.loops.iter().flatten();
            far[*f] != 0.0
                && steps.any(|s| matches!(s.walk, Walk::Edge { edge: e, .. } if e == edge))
        });
        if moving.count() > 1 {
            return None;
        }
    }
    crate::tweak::faces_off_same(solid, faces, &|f| far[f], size)
}

/// `solid` with the feature its faces `faces` are all of moved by `by`,
/// if they are one that slides: what closing the body over them puts
/// there (or takes away), put back (or taken away again) `by` further
/// on. `None` if they are not closed over, or if what closes them over
/// would not slide in itself that way.
fn feature_moved(solid: &Solid, faces: &[usize], by: Point3) -> Option<Solid> {
    use workpiece_boolean::{Op, combine, measure};
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let healed = crate::heal::faces_deleted(solid, faces).ok()?;
    let grew = measure(&healed).volume > measure(solid).volume;
    let plug = if grew {
        combine(Op::Cut, &healed, solid).ok()?.solid
    } else {
        combine(Op::Cut, solid, &healed).ok()?.solid
    };
    if plug.is_empty() {
        return None;
    }
    // Its faces that are not the feature's own lie in the faces it is
    // cut from: they have to slide in themselves.
    let near = solid.tolerance().max(size * 1e-9);
    for face in &plug.faces {
        let mut own = true;
        let mut slides = true;
        for step in face.loops.iter().flatten() {
            let (a, b) = plug.span(step);
            for k in 0..=4 {
                let uv = step
                    .pcurve
                    .point_at(a + (b - a) * (f64::from(k) + 0.5) / 5.0);
                let p = face.surface.point_at(uv.x, uv.y);
                own &= faces
                    .iter()
                    .any(|&g| workpiece_boolean::lies_on(solid, g, p, near));
                slides &= normal(&face.surface, uv).dot(by).abs() <= by.length() * 1e-9;
            }
        }
        if !own && !slides {
            return None;
        }
    }
    let moved = plug.moved(&|p| p + by).ok()?;
    let op = if grew { Op::Cut } else { Op::Union };
    let made = combine(op, &healed, &moved).ok()?.solid;
    (!made.is_empty()).then_some(made)
}

/// `solid` with its faces `faces` moved by `to`, a turn and a shift
/// (which keeps lengths and angles). Without a turn that is
/// [`faces_moved`]. Turned, flat faces of one plane are moved to the
/// plane `to` takes theirs to ([`faces_to_plane`]): a side turned about
/// its lower edge leans. Faces that bend, or of several planes, are
/// not turned yet.
pub fn faces_placed(
    solid: &Solid,
    faces: &[usize],
    to: &dyn Fn(Point3) -> Point3,
) -> anyhow::Result<Solid> {
    ensure!(!faces.is_empty(), "pick the faces to move");
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let by = to(Point3::ZERO);
    let shifted = [Point3::X, Point3::Y, Point3::Z]
        .into_iter()
        .all(|axis| (to(axis) - by - axis).length() < 1e-12);
    if shifted {
        // (Moved nowhere, a face is where it was.)
        if by.length() <= size * 1e-12 {
            return Ok(solid.clone());
        }
        return faces_moved(solid, faces, by);
    }
    // The plane of the first, which all of them have to lie in.
    let mut plane: Option<(Point3, Point3)> = None;
    for &face in faces {
        let f = &solid.faces[face];
        let ((u0, u1), (v0, v1)) = f.surface.domain();
        for (i, j) in [
            (0.0, 0.0),
            (1.0, 0.0),
            (0.0, 1.0),
            (1.0, 1.0),
            (0.5, 0.5),
            (0.3, 0.8),
        ] {
            let uv = Point2::new(u0 + (u1 - u0) * i, v0 + (v1 - v0) * j);
            let (p, n) = (f.surface.point_at(uv.x, uv.y), normal_out(f, uv));
            let (origin, normal) = *plane.get_or_insert((p, n));
            ensure!(
                (n - normal).length() < 1e-9 && (p - origin).dot(normal).abs() < size * 1e-9,
                "only flat faces of one plane are turned: others are not made yet"
            );
        }
    }
    let (origin, normal) = plane.context("pick the faces to move")?;
    faces_to_plane(solid, faces, to(origin), to(origin + normal) - to(origin))
}

/// `solid` with its faces `faces` replaced by the plane through
/// `origin` across `normal`: where the plane is out of the body from a
/// face, the body runs on to it, straight off the face; where it is in
/// the body, what is between the face and the plane is taken off.
pub fn faces_to_plane(
    solid: &Solid,
    faces: &[usize],
    origin: Point3,
    normal: Point3,
) -> anyhow::Result<Solid> {
    use workpiece_boolean::{Op, combine};
    ensure!(!faces.is_empty(), "pick the faces to replace");
    let normal = normal.try_normalize().context("a plane has a normal")?;
    let reach = {
        let (lo, hi) = solid.extent();
        (hi - lo).length() * 2.0 + (lo - origin).length() + (hi - origin).length()
    };
    let mut made = solid.clone();
    let mut beyond = Vec::new();
    for (k, &face) in faces.iter().enumerate() {
        // All that is straight off the face, out of the body and into
        // it; of that, what is past the plane on the face's outside.
        let out = slab(solid, face, reach, &format!("replace/out{k}"))?;
        let into = slab(solid, face, -reach, &format!("replace/in{k}"))?;
        let f = &solid.faces[face];
        let step = f
            .loops
            .iter()
            .flatten()
            .next()
            .context("a face has edges")?;
        let uv = step.pcurve.point_at(solid.span(step).0);
        let facing = normal_out(f, uv).dot(normal);
        ensure!(
            facing.abs() > 1e-6,
            "the plane stands on edge to the face it is to replace"
        );
        let through = combine(Op::Union, &out, &into)
            .context("running a face on")?
            .solid;
        let [front, behind] = crate::split::halves(&through, origin, normal)?;
        beyond.push(if facing > 0.0 { front } else { behind });
        made = combine(Op::Union, &made, &out)
            .context("running the body on to the plane")?
            .solid;
    }
    for past in &beyond {
        made = combine(Op::Cut, &made, past)
            .context("ending the body at the plane")?
            .solid;
    }
    Ok(made)
}

/// The unit normal of `face` at (u, v), out of its body.
fn normal_out(face: &workpiece_boolean::model::Face, uv: Point2) -> Point3 {
    let n = normal(&face.surface, uv);
    if face.outward { n } else { -n }
}

/// Whether face `face` of `solid`, moved off itself by `distance`, still
/// meets the faces beside it: each of its edges, moved with it, has to
/// be in the body or on it yet — but one the body is on the inside of
/// (an outside edge), moved out, which the face beside it runs on to. A
/// face pushed in through the far side of the body, or raised out past
/// the top of a wall that stands on it, has left what it met behind.
fn stays_within(solid: &Solid, face: usize, distance: f64) -> anyhow::Result<()> {
    let f = &solid.faces[face];
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let near = solid.tolerance().max(size * 1e-9);
    let placer = workpiece_boolean::Placer::new(solid);
    for step in f.loops.iter().flatten() {
        let Walk::Edge { edge, .. } = step.walk else {
            continue;
        };
        let inside_edge = opening(solid, edge).is_some_and(|a| a > std::f64::consts::PI);
        if distance > 0.0 && !inside_edge {
            continue;
        }
        let (a, b) = solid.edges[edge].range;
        for share in [0.25, 0.5, 0.75] {
            let t = a + (b - a) * share;
            let uv = step.pcurve.point_at(t);
            let moved = f.surface.point_at(uv.x, uv.y) + normal_out(f, uv) * distance;
            ensure!(
                placer.side(moved, near) != Some(workpiece_boolean::Side::Out),
                "the face {} is moved past the faces it meets: its edge {} would leave the body",
                f.name.as_deref().unwrap_or("picked"),
                solid.edges[edge].name.as_deref().unwrap_or("there")
            );
        }
    }
    Ok(())
}

/// `solid` with its faces `faces` moved off themselves by `distance`
/// along their normals: out of the solid, or (less than nothing) into
/// it — a hole widened, a boss made thinner, a wall pushed out.
pub fn faces_offset(solid: &Solid, faces: &[usize], distance: f64) -> anyhow::Result<Solid> {
    use workpiece_boolean::{Op, combine};
    ensure!(!faces.is_empty(), "pick the faces to offset");
    ensure!(distance != 0.0 && distance.is_finite(), "by some distance");
    for &face in faces {
        stays_within(solid, face, distance)?;
    }
    // Where two of them part at the edge between them as they move (an
    // outside edge, moving out), each runs on until it meets the other.
    let moved_at = |edge: usize| {
        let beside = solid.faces.iter().enumerate().filter(|(_, face)| {
            let mut steps = face.loops.iter().flatten();
            steps.any(|s| matches!(s.walk, Walk::Edge { edge: e, .. } if e == edge))
        });
        beside.filter(|(f, _)| faces.contains(f)).count() == 2
    };
    let half = std::f64::consts::PI;
    let parting = |edge: usize| -> Option<f64> {
        let angle = opening(solid, edge)?;
        let gap = if distance > 0.0 {
            half - angle
        } else {
            angle - half
        };
        (gap > 1e-9 && moved_at(edge)).then(|| distance.abs() * (gap / 2.0).tan())
    };
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    if let Some(made) = crate::tweak::faces_off_same(solid, faces, &|_| distance, size) {
        return Ok(made);
    }
    let mut made = solid.clone();
    for (k, &face) in faces.iter().enumerate() {
        let id = format!("offset/f{k}");
        let mut far: Vec<(usize, f64)> = Vec::new();
        for step in solid.faces[face].loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk
                && let Some(by) = parting(edge)
                && far.iter().all(|x| x.0 != edge)
            {
                far.push((edge, by));
            }
        }
        let slab = if far.is_empty() {
            slab(solid, face, distance, &id)?
        } else {
            let wider = run_on(solid, face, &far)?;
            slab(&wider, 0, distance, &id)?
        };
        let op = if distance > 0.0 { Op::Union } else { Op::Cut };
        made = combine(op, &made, &slab)
            .context("offsetting a face")?
            .solid;
    }
    Ok(made)
}

/// The faces `faces` of `solid`, as a sheet, thickened by `distance`
/// along their normals out of the solid (into it, for less than
/// nothing; `both` ways, with that): every face's slab, and where the
/// slabs of two faces part at the edge between them, the tube around
/// that edge — or, with `sharp`, the faces run on until they meet.
pub fn thicken(
    solid: &Solid,
    faces: &[usize],
    distance: f64,
    both: bool,
    sharp: bool,
) -> anyhow::Result<Solid> {
    use workpiece_boolean::{Op, combine};
    ensure!(distance != 0.0 && distance.is_finite(), "by some distance");
    ensure!(!faces.is_empty(), "pick the faces to thicken");
    let d = distance.abs();
    let mut faces_of: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    for &f in faces {
        for step in solid.faces[f].loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk
                && !faces_of[edge].contains(&f)
            {
                faces_of[edge].push(f);
            }
        }
    }
    let half = std::f64::consts::PI;
    let mut pieces: Vec<(String, Solid)> = Vec::new();
    let sides: Vec<f64> = if both {
        vec![distance, -distance]
    } else {
        vec![distance]
    };
    for (s, &side) in sides.iter().enumerate() {
        // The edges between two of the faces at which their slabs part
        // on this side, each with how far a face has to run on past it
        // to meet the other.
        let parting: Vec<(usize, f64)> = (0..solid.edges.len())
            .filter(|&e| faces_of[e].len() == 2)
            .filter_map(|e| {
                let angle = opening(solid, e)?;
                let gap = if side > 0.0 {
                    half - angle
                } else {
                    angle - half
                };
                (gap > 1e-9).then(|| (e, d * (gap / 2.0).tan()))
            })
            .collect();
        for &f in faces {
            let id = format!("thick/s{s}/f{f}");
            let across: Vec<usize> = parting
                .iter()
                .map(|p| p.0)
                .filter(|e| faces_of[*e].contains(&f))
                .collect();
            // (To both sides: one slab, from the face's far side on
            // the one to its far side on the other.)
            if both && s == 1 {
                continue;
            }
            let through = |of: &Solid, face: usize| -> anyhow::Result<Solid> {
                if !both {
                    return slab(of, face, side, &id);
                }
                let back = slab(of, face, -side, &format!("{id}/back"))?;
                let far = back
                    .faces
                    .iter()
                    .position(|x| x.name.as_deref() == Some(&format!("{id}/back/far")))
                    .context("a slab has a far face")?;
                slab(&back, far, -2.0 * side.abs(), &id)
            };
            let made = if sharp && !across.is_empty() {
                let far: Vec<(usize, f64)> = parting
                    .iter()
                    .filter(|p| across.contains(&p.0))
                    .copied()
                    .collect();
                run_on(solid, f, &far).and_then(|wider| through(&wider, 0))
            } else {
                through(solid, f)
            };
            let mut piece =
                made.with_context(|| format!("thickening face {:?}", solid.faces[f].name))?;
            if !both && let Some(past) = past_axis(solid, f, side, &format!("{id}/x")) {
                pieces.push((format!("{id}/x"), past));
            }
            let far: Vec<usize> = (0..piece.edges.len())
                .filter(|&k| {
                    let name = piece.edges[k].name.as_deref();
                    name.is_some_and(|n| n.starts_with(&format!("{id}/far")))
                })
                .collect();
            for face in &mut piece.faces {
                if face.name.as_deref() == Some(&format!("{id}/far")) {
                    face.smooth = far.clone();
                }
            }
            pieces.push((id, piece));
        }
        if sharp {
            continue;
        }
        // Of a tube only what is between the two slabs: not what is
        // on the other side of the faces, where the sheet has nothing.
        let between = |mut piece: Solid, beside: &[usize], id: &str| -> anyhow::Result<Solid> {
            for &f in beside {
                let behind = slab(solid, f, -side, &format!("{id}/behind{f}"))?;
                piece = combine(Op::Cut, &piece, &behind)
                    .with_context(|| format!("cutting {id} back to the sheet's side"))?
                    .solid;
                // (Nor what the face's own slab has: the tube and the
                // slabs then lie against each other, not in each other.)
                let front = slab(solid, f, side, &format!("{id}/front{f}"))?;
                piece = combine(Op::Cut, &piece, &front)
                    .with_context(|| format!("cutting {id} back to its slabs"))?
                    .solid;
            }
            Ok(piece)
        };
        for &(e, _) in &parting {
            let id = format!("thick/s{s}/e{e}");
            let whole = tube(solid, e, d, &id)?;
            pieces.push((id.clone(), between(whole, &faces_of[e], &id)?));
        }
        // A ball where tubes end inside the sheet (not at its rim,
        // where they end as the sheet does).
        let rim = |v: usize| {
            (0..solid.edges.len())
                .any(|e| faces_of[e].len() == 1 && solid.edges[e].ends.contains(&v))
        };
        let mut corners: Vec<usize> = parting
            .iter()
            .filter(|p| solid.edges[p.0].ends[0] != solid.edges[p.0].ends[1])
            .flat_map(|p| solid.edges[p.0].ends)
            .filter(|v| !rim(*v))
            .collect();
        corners.sort_unstable();
        corners.dedup();
        for v in corners {
            let id = format!("thick/s{s}/v{v}");
            let centre = solid.vertices[v].point;
            let axis = Point3::new(1.0, 2.0, 3.0).normalize();
            let turned = |p: Point3| {
                let r = p - centre;
                let (sin, cos) = 1.0_f64.sin_cos();
                centre + r * cos + axis.cross(r) * sin + axis * (axis.dot(r) * (1.0 - cos))
            };
            let whole = crate::blend::ball(&id, centre, d)?.moved(&turned)?;
            let beside: Vec<usize> = faces
                .iter()
                .copied()
                .filter(|&f| {
                    let steps = solid.faces[f].loops.iter().flatten();
                    steps.map(|step| solid.ends(step).0).any(|w| w == v)
                })
                .collect();
            // Of the ball, only what no tube has: what is past the end
            // of each edge that ends here lies in that edge's tube.
            let mut round = between(whole, &beside, &id)?;
            for &(e, _) in &parting {
                let edge = &solid.edges[e];
                let Some(end) = edge.ends.iter().position(|w| *w == v) else {
                    continue;
                };
                let t = if end == 0 { edge.range.0 } else { edge.range.1 };
                let along = edge.curve.derivative_at(t).1;
                let into = if end == 0 { along } else { -along };
                let [_, behind] = crate::split::halves(&round, centre, into)
                    .with_context(|| format!("ending {id} where a tube starts"))?;
                round = behind;
            }
            pieces.push((id.clone(), round));
        }
    }
    // One after the other, each next to what is there already: a tube
    // right after a slab it lies against (two slabs that meet at an
    // edge only are no body to add to), a ball after its tubes.
    let touches = |id: &str, taken: &[String]| -> bool {
        let Some((side, what)) = id.strip_prefix("thick/").and_then(|x| x.split_once('/')) else {
            return true;
        };
        let has = |x: String| taken.contains(&format!("thick/{side}/{x}"));
        if let Some(e) = what.strip_prefix('e').and_then(|e| e.parse::<usize>().ok()) {
            return faces_of[e].iter().any(|f| has(format!("f{f}")));
        }
        if let Some(v) = what.strip_prefix('v').and_then(|v| v.parse::<usize>().ok()) {
            return (0..solid.edges.len())
                .any(|e| solid.edges[e].ends.contains(&v) && has(format!("e{e}")));
        }
        false
    };
    let mut left = pieces;
    let (first, mut result) = left.remove(0);
    let mut taken = vec![first];
    while !left.is_empty() {
        let next = left
            .iter()
            .position(|(id, _)| touches(id, &taken))
            .unwrap_or(0);
        let (id, piece) = left.remove(next);
        result = combine(Op::Union, &result, &piece)
            .with_context(|| format!("taking {id} in"))?
            .solid;
        taken.push(id);
    }
    Ok(result)
}

/// `solid` with its faces `faces` drafted by `angle` (radians) for a
/// pull along `pull`, about the neutral plane through `neutral` square
/// to the pull: each turns about where it meets that plane until it
/// leans by the angle from the pull, into the body on the side the pull
/// points to and out of it on the other. The faces beside them are
/// trimmed to them or grow to meet them, and drafted faces meet each
/// other where they cross. With `both`, they lean the same way on
/// either side of the neutral plane.
pub fn drafted(
    solid: &Solid,
    faces: &[usize],
    pull: Point3,
    neutral: Point3,
    angle: f64,
    both: bool,
) -> anyhow::Result<Solid> {
    use workpiece_boolean::{Op, combine};
    let pull = pull.try_normalize().context("the pull has no direction")?;
    ensure!(
        angle.abs() > 1e-9 && angle.abs() < 1.5,
        "a draft is by some angle, less than a right one"
    );
    ensure!(!faces.is_empty(), "pick the faces to draft");
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let height = |p: Point3| (p - neutral).dot(pull);
    let lean = angle.tan();
    // Out of the body by this much (into it, where it is less than
    // nothing).
    // (With `both`, the same way on either side of the neutral plane:
    // a part made in a mould of two halves, parted there.)
    let rise = |h: f64| if both { h.abs() } else { h };
    let by = |p: Point3| -lean * rise(height(p));
    let names: Vec<Option<String>> = faces.iter().map(|&f| solid.faces[f].name.clone()).collect();
    let chosen = |part: &Solid| -> Vec<usize> {
        (0..part.faces.len())
            .filter(|&f| {
                let name = part.faces[f]
                    .name
                    .as_deref()
                    .map(|n| n.split('#').next().unwrap_or(n));
                name.is_some() && names.iter().any(|n| n.as_deref() == name)
            })
            .collect()
    };
    ensure!(
        names.iter().all(Option::is_some),
        "a face to draft has no name to know it by"
    );
    // Above the neutral plane and below it the faces move opposite
    // ways: a body the plane goes through is drafted as its two halves.
    let samples = |part: &Solid, f: usize| -> Vec<f64> {
        let face = &part.faces[f];
        let steps = face.loops.iter().flatten();
        steps
            .flat_map(|step| {
                let (a, b) = part.span(step);
                (0..=4).map(move |k| {
                    let uv = step.pcurve.point_at(a + (b - a) * f64::from(k) / 4.0);
                    height(face.surface.point_at(uv.x, uv.y))
                })
            })
            .collect()
    };
    let none = size * 1e-8;
    let crossed = faces.iter().any(|&f| {
        let h = samples(solid, f);
        h.iter().any(|x| *x > none) && h.iter().any(|x| *x < -none)
    });
    let parts: Vec<Solid> = if crossed {
        let (a, b) = pull.any_orthonormal_pair();
        let above = crate::blend::cuboid(
            "draft/above",
            neutral - (a + b) * size * 2.0,
            [a, b, pull],
            [size * 4.0, size * 4.0, size * 2.0],
        )?;
        let upper = combine(Op::Intersect, solid, &above).context("parting the body")?;
        let lower = combine(Op::Cut, solid, &above).context("parting the body")?;
        vec![upper.solid, lower.solid]
    } else {
        vec![solid.clone()]
    };
    let mut whole: Option<Solid> = None;
    for part in parts {
        if part.is_empty() {
            continue;
        }
        let mine = chosen(&part);
        let mut faces_of: Vec<Vec<usize>> = vec![Vec::new(); part.edges.len()];
        for &f in &mine {
            for step in part.faces[f].loops.iter().flatten() {
                if let Walk::Edge { edge, .. } = step.walk
                    && !faces_of[edge].contains(&f)
                {
                    faces_of[edge].push(f);
                }
            }
        }
        // Which way each moves here, and how far at most.
        let moves: BTreeMap<usize, (bool, f64)> = mine
            .iter()
            .filter_map(|&f| {
                let most = samples(&part, f)
                    .into_iter()
                    .map(|h| -lean * rise(h))
                    .fold(0.0_f64, |m, d| if d.abs() > m.abs() { d } else { m });
                (most.abs() > none).then_some((f, (most > 0.0, most.abs())))
            })
            .collect();
        let mut result = part.clone();
        for (k, (&f, &(out_of, _))) in moves.iter().enumerate() {
            let wedge = slab_by(&part, f, &by, &format!("draft/f{k}"))
                .with_context(|| format!("drafting face {:?}", part.faces[f].name))?;
            let op = if out_of { Op::Union } else { Op::Cut };
            result = combine(op, &result, &wedge)
                .with_context(|| format!("drafting face {:?}", part.faces[f].name))?
                .solid;
        }
        // Between two drafted faces that part as they move — into the
        // body at an inside edge, out of it at an outside one — what
        // both would take or add, run on past the edge.
        let half = std::f64::consts::PI;
        for (e, beside) in faces_of.iter().enumerate() {
            let &[a, b] = &beside[..] else {
                continue;
            };
            let (Some(&(out_a, far_a)), Some(&(out_b, far_b))) = (moves.get(&a), moves.get(&b))
            else {
                continue;
            };
            let Some(angle) = opening(&part, e) else {
                continue;
            };
            if out_a != out_b || (out_a == (angle > half)) {
                continue;
            }
            let reach = 4.0 * far_a.max(far_b);
            let on = |f: usize, id: &str| -> anyhow::Result<Solid> {
                run_on(&part, f, &[(e, reach)]).and_then(|wider| slab_by(&wider, 0, &by, id))
            };
            let both = combine(Op::Intersect, &on(a, "draft/a")?, &on(b, "draft/b")?)
                .context("where two drafted faces meet")?
                .solid;
            if both.is_empty() {
                continue;
            }
            let op = if out_a { Op::Union } else { Op::Cut };
            result = combine(op, &result, &both)
                .context("where two drafted faces meet")?
                .solid;
        }
        whole = Some(match whole {
            None => result,
            Some(other) => {
                combine(Op::Union, &other, &result)
                    .context("putting the drafted halves together")?
                    .solid
            }
        });
    }
    whole.context("nothing to draft")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surfaces_run_on_as_what_they_are() -> anyhow::Result<()> {
        // A quarter of a cylinder of radius 2, 3 high: run on around and
        // upwards, it is more of the same cylinder, and what it was
        // where it was.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for (x, y, weight) in [(2.0, 0.0, 1.0), (2.0, 2.0, w), (0.0, 2.0, 1.0)] {
            for z in [0.0, 3.0] {
                points.push(Point3::new(x, y, z));
                weights.push(weight);
            }
        }
        let quarter = NurbsSurface::new(
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            3,
            2,
            points,
            Some(weights),
        )?;
        let more = run_on_surface(&quarter, false, false, 1.5)?;
        let more = run_on_surface(&more, true, true, -1.0)?;
        for (u, v) in [(0.3, 0.2), (0.9, 1.0), (1.4, -0.7), (1.2, 0.5)] {
            let p = more.point_at(u, v);
            assert!((p.x.hypot(p.y) - 2.0).abs() < 1e-12, "{p:?}");
            assert!((p.z - 3.0 * v).abs() < 1e-12, "{p:?}");
            if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
                assert!(p.distance(quarter.point_at(u, v)) < 1e-12);
            }
        }
        Ok(())
    }
}
