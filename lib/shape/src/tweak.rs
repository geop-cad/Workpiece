//! Faces moved off themselves where they are, the body's make-up kept:
//! every vertex, edge and face stays what it is between the same
//! neighbours, and only where they are changes — the corners to where
//! the moved surfaces meet, the edges to the lines and circles between
//! them. Nothing is combined. For the bodies this is so of: faces whose
//! edges are straight or run around circles, none of them lost or
//! turned around by the move, and none brought to where another is.
//! What is made is checked for all of that; whatever is not so is left
//! to the slabs and the booleans of [`crate::thick`].

use std::sync::Arc;

use workpiece_boolean::Solid;
use workpiece_boolean::model::{Face, Use, Walk};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve2, Curve3, NurbsSurface, Topo};

use crate::local::Flat;
use crate::thick::{normal, offset_surface};

/// The way a flat surface faces (∂u × ∂v), if it is flat: all of its
/// control points in one plane, within `within`.
pub(crate) fn flat_way(surface: &NurbsSurface, within: f64) -> Option<Point3> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    let middle = Point2::new((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    let n = normal(surface, middle);
    if n.length_squared() < 0.5 {
        return None;
    }
    let at = surface.point_at(middle.x, middle.y);
    surface
        .control_points()
        .iter()
        .all(|p| (*p - at).dot(n).abs() <= within)
        .then_some(n)
}

/// `surface` moved `by` along its normal ∂u × ∂v.
fn moved_off(surface: &NurbsSurface, by: f64, within: f64) -> Option<NurbsSurface> {
    if let Some(n) = flat_way(surface, within * 1e-3) {
        let (p, q) = surface.degrees();
        let (ku, kv) = surface.knots();
        let (nu, nv) = surface.counts();
        let points = surface.control_points().iter().map(|c| *c + n * by);
        return NurbsSurface::new(
            p,
            q,
            ku.to_vec(),
            kv.to_vec(),
            nu,
            nv,
            points.collect(),
            Some(surface.weights().to_vec()),
        )
        .ok();
    }
    if let Some(made) = turned_off(surface, by, within) {
        return Some(made);
    }
    // (Or turned along v and straight along u: the same, with u and v
    // changed over — which turns its normal around.)
    if let Some(made) = swapped(surface).and_then(|other| turned_off(&other, -by, within)) {
        return swapped(&made);
    }
    offset_surface(surface, &|_| by, within).ok()
}

/// `surface` with u for v and v for u.
pub(crate) fn swapped(surface: &NurbsSurface) -> Option<NurbsSurface> {
    let (p, q) = surface.degrees();
    let (nu, nv) = surface.counts();
    let (ku, kv) = surface.knots();
    let weights = surface.weights();
    let mut points = Vec::with_capacity(nu * nv);
    let mut turned = Vec::with_capacity(nu * nv);
    for j in 0..nv {
        for i in 0..nu {
            points.push(surface.control_point(i, j));
            turned.push(weights[i * nv + j]);
        }
    }
    NurbsSurface::new(q, p, kv.to_vec(), ku.to_vec(), nv, nu, points, Some(turned)).ok()
}

/// `surface` moved `by` along its normal, exactly, if it is one turned
/// about an axis and straight across (a cylinder, a cone short of its
/// tip): two rows of control points with the same weights, each around
/// a circle about the one axis. Every row then moves as a whole —
/// away from the axis and along it, as the surface leans.
fn turned_off(surface: &NurbsSurface, by: f64, within: f64) -> Option<NurbsSurface> {
    let (p, q) = surface.degrees();
    let (nu, nv) = surface.counts();
    let (ku, kv) = surface.knots();
    let weights = surface.weights();
    if q != 1 || nv != 2 || p < 2 || (0..nu).any(|i| weights[i * nv] != weights[i * nv + 1]) {
        return None;
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let row = |j: usize| {
        Curve3::new(
            p,
            ku.to_vec(),
            (0..nu).map(|i| surface.control_point(i, j)).collect(),
            Some((0..nu).map(|i| weights[i * nv + j]).collect()),
        )
        .ok()
    };
    let (c0, k0, r0) = circle_of(&row(0)?, (u0, u1), within)?;
    let (c1, k1, r1) = circle_of(&row(1)?, (u0, u1), within)?;
    let scale = r0.max(r1);
    if k0.cross(k1).length() > 1e-9
        || (c1 - c0).cross(k0).length() > within
        || r0.min(r1) <= scale * 1e-9
    {
        return None;
    }
    // How it leans: its normal as so much away from the axis and so
    // much along it, the same all over.
    let uv = Point2::new((u0 + u1) / 2.0, v0);
    let n = normal(surface, uv);
    let away = (surface.point_at(uv.x, uv.y) - c0) / r0;
    let (out, along) = (n.dot(away), n.dot(k0));
    if (out * out + along * along - 1.0).abs() > 1e-9 {
        return None;
    }
    let mut points = Vec::with_capacity(nu * nv);
    for i in 0..nu {
        for (j, (c, r)) in [(c0, r0), (c1, r1)].into_iter().enumerate() {
            let at = surface.control_point(i, j);
            // (A control point is as far along the axis as its row.)
            let flat = at - k0 * (at - c).dot(k0);
            points.push(at + ((flat - c) / r * out + k0 * along) * by);
        }
    }
    let made = NurbsSurface::new(
        p,
        q,
        ku.to_vec(),
        kv.to_vec(),
        nu,
        nv,
        points,
        Some(weights.to_vec()),
    )
    .ok()?;
    for (su, sv) in [(0.13, 0.0), (0.5, 1.0), (0.81, 0.4)] {
        let (u, v) = (u0 + (u1 - u0) * su, v0 + (v1 - v0) * sv);
        let wanted = surface.point_at(u, v) + normal(surface, Point2::new(u, v)) * by;
        if made.point_at(u, v).distance(wanted) > within * 1e-2 {
            return None;
        }
    }
    Some(made)
}

/// The least step that brings what `rows` ask to nothing: each a way
/// and how far along it is still to go. Ways that say nothing new (two
/// faces that touch) are left out.
fn least_step(rows: &[(Point3, f64)]) -> Point3 {
    let mut kept: Vec<(Point3, f64)> = Vec::new();
    for &(n, b) in rows {
        let (mut r, mut rb) = (n, b);
        for (q, c) in &kept {
            let along = n.dot(*q);
            r -= *q * along;
            rb -= c * along;
        }
        let left = r.length();
        if left > 1e-6 {
            kept.push((r / left, rb / left));
        }
    }
    kept.iter().map(|(q, c)| *q * *c).sum()
}

/// The circle `curve` runs around over `range`, if it does: its
/// centre, its axis and its radius.
pub(crate) fn circle_of(
    curve: &Curve3,
    range: (f64, f64),
    within: f64,
) -> Option<(Point3, Point3, f64)> {
    let at = |share: f64| curve.point_at(range.0 + (range.1 - range.0) * share);
    let (a, b, c) = (at(0.0), at(0.31), at(0.67));
    let (ab, ac) = (b - a, c - a);
    let k = ab.cross(ac);
    let area = k.length_squared();
    if area <= 0.0 || !area.is_finite() {
        return None;
    }
    // (The circle through three points.)
    let centre =
        a + (k.cross(ab) * ac.length_squared() + ac.cross(k) * ab.length_squared()) / (2.0 * area);
    let radius = centre.distance(a);
    let k = k / area.sqrt();
    let on = |p: Point3| {
        ((p - centre).length() - radius).abs() <= within && (p - centre).dot(k).abs() <= within
    };
    ((0..=8).all(|i| on(at(f64::from(i) / 8.0)))
        && curve
            .control_points()
            .iter()
            .all(|p| (*p - centre).dot(k).abs() <= within))
    .then_some((centre, k, radius))
}

/// Whether two stretches cross, each between two points of a plane,
/// other than at an end they share.
fn cross(a: [Point2; 2], b: [Point2; 2]) -> bool {
    let side = |p: Point2, q: Point2, r: Point2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(a[0], a[1], b[0]), side(a[0], a[1], b[1]));
    let (d3, d4) = (side(b[0], b[1], a[0]), side(b[0], b[1], a[1]));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// The loops of `face` as lines from point to point in its (u, v), each
/// step a few; and the area each loop goes around, with its sign.
fn outline(solid: &Solid, face: &Face) -> (Vec<Vec<Point2>>, Vec<f64>) {
    const PIECES: usize = 6;
    let mut loops = Vec::new();
    let mut areas = Vec::new();
    for lp in &face.loops {
        let mut points = Vec::new();
        for step in lp {
            let (a, b) = solid.span(step);
            for i in 0..PIECES {
                points.push(step.pcurve.point_at(a + (b - a) * i as f64 / PIECES as f64));
            }
        }
        let n = points.len();
        let area: f64 = (0..n)
            .map(|i| points[i].perp_dot(points[(i + 1) % n]))
            .sum();
        areas.push(area / 2.0);
        loops.push(points);
    }
    (loops, areas)
}

/// Whether the outlines of a face cross themselves or each other.
fn tangled(loops: &[Vec<Point2>]) -> bool {
    let mut all: Vec<([Point2; 2], usize, usize)> = Vec::new();
    for (l, points) in loops.iter().enumerate() {
        let n = points.len();
        for i in 0..n {
            all.push(([points[i], points[(i + 1) % n]], l, i));
        }
    }
    for (i, (a, la, ia)) in all.iter().enumerate() {
        for (b, lb, ib) in &all[i + 1..] {
            if la == lb {
                let n = loops[*la].len();
                let apart = ib - ia;
                if apart <= 1 || apart + 1 >= n {
                    continue;
                }
            }
            if cross(*a, *b) {
                return true;
            }
        }
    }
    false
}

/// Whether `p` is within the outline `around`.
fn within(p: Point2, around: &[Point2]) -> bool {
    let n = around.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (around[i], around[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// The faces along every edge of `solid`.
fn faces_beside(solid: &Solid) -> Vec<Vec<usize>> {
    let mut beside: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                beside[edge].push(f);
            }
        }
    }
    beside
}

/// `solid` shrunk by `d`, or (with faces `open`) its shell that thick
/// into it: made where it is, if the body is one whose every edge
/// between two walls is an outside one or smooth, and whose walls meet
/// its openings at a right angle or less — nothing of what is taken
/// away is round then, and what is left inside is the body with its
/// walls moved in, made up as it is. `None` if it is not such a body.
pub(crate) fn shrunk_same(
    solid: &Solid,
    d: f64,
    open: Option<&[usize]>,
    size: f64,
) -> Option<Solid> {
    use std::f64::consts::PI;
    let wall = |f: usize| open.is_none_or(|open| !open.contains(&f));
    let beside = faces_beside(solid);
    for (e, faces) in beside.iter().enumerate() {
        let &[a, b] = &faces[..] else {
            return None;
        };
        let angle = crate::thick::opening(solid, e);
        match (wall(a), wall(b)) {
            (true, true) => {
                if angle.is_some_and(|angle| angle > PI + 1e-9) {
                    return None;
                }
            }
            (false, false) => return None,
            _ => {
                if angle.is_none_or(|angle| angle > PI / 2.0 + 1e-9) {
                    return None;
                }
            }
        }
    }
    let off: Vec<f64> = (0..solid.faces.len())
        .map(|f| if wall(f) { -d } else { 0.0 })
        .collect();
    let inner = offset_same(solid, &off, size, open.is_none())?;
    let Some(open) = open else {
        return Some(inner);
    };
    // The shell: the body's walls, the walls of what is left inside it
    // seen from the other side, and of every opening what is between
    // the two.
    let (nv, ne) = (solid.vertices.len(), solid.edges.len());
    let mut out = solid.clone();
    out.vertices
        .extend(inner.vertices.iter().cloned().map(|mut v| {
            v.name = None;
            v
        }));
    out.edges.extend(inner.edges.iter().cloned().map(|mut e| {
        e.ends = e.ends.map(|v| v + nv);
        e.name = None;
        e.also.clear();
        e
    }));
    let far = |step: &Use, turned: bool| -> Option<Use> {
        let walk = match step.walk {
            Walk::Edge { edge, reversed } => Walk::Edge {
                edge: edge + ne,
                reversed: reversed != turned,
            },
            // (A point a face comes to, walked the other way around:
            // not one of these.)
            Walk::Pole { .. } if turned => return None,
            Walk::Pole { vertex, range } => Walk::Pole {
                vertex: vertex + nv,
                range,
            },
        };
        Some(Use {
            walk,
            pcurve: step.pcurve.clone(),
        })
    };
    for (f, face) in inner.faces.iter().enumerate() {
        if open.contains(&f) {
            // (No outline of the inside crosses one of the body's.)
            let (mut all, _) = outline(solid, &solid.faces[f]);
            let own = all.len();
            let (inside, _) = outline(&inner, face);
            all.extend(inside);
            if tangled(&all) || (own..all.len()).any(|i| !within(all[i][0], &all[0])) {
                return None;
            }
            for lp in &face.loops {
                let turned: Option<Vec<Use>> = lp.iter().rev().map(|s| far(s, true)).collect();
                out.faces[f].loops.push(turned?);
            }
            continue;
        }
        let loops: Option<Vec<Vec<Use>>> = face
            .loops
            .iter()
            .map(|lp| lp.iter().map(|s| far(s, false)).collect())
            .collect();
        out.faces.push(Face {
            surface: face.surface.clone(),
            outward: !face.outward,
            loops: loops?,
            tolerance: face.tolerance,
            name: Some(format!("thick/f{f}/far")),
            also: Vec::new(),
            smooth: Vec::new(),
        });
    }
    let mut scratch = Topo::new();
    workpiece_boolean::write(&mut scratch, &out, "shell").ok()?;
    scratch.validate().ok()?;
    Some(out)
}

/// `solid` with its faces `faces` each moved off itself by what `by`
/// says of it (out of the body, or into it), made where it is: if the
/// body stays made up as it is, and every edge between a moved face and
/// one that stays moves straight off with its face — the face that
/// stays runs on (or is cut back) in itself there, and no step comes
/// between the two. `None` if not.
pub(crate) fn faces_off_same(
    solid: &Solid,
    faces: &[usize],
    by: &dyn Fn(usize) -> f64,
    size: f64,
) -> Option<Solid> {
    let tol = size * 1e-10;
    let mut off = vec![0.0; solid.faces.len()];
    for &f in faces {
        off[f] = by(f);
    }
    if off.iter().all(|x| *x == 0.0) {
        return None;
    }
    let made = offset_same(solid, &off, size, true)?;
    for (f, face) in solid.faces.iter().enumerate() {
        if off[f] == 0.0 {
            continue;
        }
        for step in face.loops.iter().flatten() {
            let Walk::Edge { edge, .. } = step.walk else {
                return None;
            };
            let other = solid.faces.iter().enumerate().any(|(g, other)| {
                g != f
                    && off[g] == 0.0
                    && other
                        .loops
                        .iter()
                        .flatten()
                        .any(|s| matches!(s.walk, Walk::Edge { edge: e, .. } if e == edge))
            });
            if !other {
                continue;
            }
            let (was, now) = (&solid.edges[edge], &made.edges[edge]);
            for share in [0.0, 0.5, 1.0] {
                let t = was.range.0 + (was.range.1 - was.range.0) * share;
                let n = normal(&face.surface, step.pcurve.point_at(t));
                let out = if face.outward { n } else { -n };
                if now.point(t).distance(was.point(t) + out * off[f]) > tol {
                    return None;
                }
            }
        }
    }
    Some(made)
}

/// `surface` run on along the parameters it goes straight along (two
/// rows of control points with the same weights) until its (u, v)
/// reaches from `lo` to `hi`, kept over the parameters it has: with it,
/// for u and for v, what a parameter of the old surface is multiplied
/// by and what is then added to give that place's parameter in the new
/// one. `None` if it would have to run on along a parameter it bends
/// along.
#[allow(clippy::type_complexity)]
fn stretched(
    surface: &NurbsSurface,
    lo: Point2,
    hi: Point2,
) -> Option<(NurbsSurface, [(f64, f64); 2])> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (nu, nv) = surface.counts();
    let (p, q) = surface.degrees();
    let slack = |a: f64, b: f64| (b - a) * 1e-12;
    let wide = lo.x < u0 - slack(u0, u1) || hi.x > u1 + slack(u0, u1);
    let high = lo.y < v0 - slack(v0, v1) || hi.y > v1 + slack(v0, v1);
    let mut points: Vec<Point3> = surface.control_points().to_vec();
    let weights = surface.weights();
    let mut maps = [(1.0, 0.0); 2];
    if high {
        if q != 1 || nv != 2 || (0..nu).any(|i| weights[i * nv] != weights[i * nv + 1]) {
            return None;
        }
        let (a, b) = (
            (lo.y.min(v0) - v0) / (v1 - v0),
            (hi.y.max(v1) - v0) / (v1 - v0),
        );
        for i in 0..nu {
            let (r0, r1) = (points[i * nv], points[i * nv + 1]);
            points[i * nv] = r0 + (r1 - r0) * a;
            points[i * nv + 1] = r0 + (r1 - r0) * b;
        }
        // (Where the old parameter v is in the new: a at v0, b at v1.)
        maps[1] = (1.0 / (b - a), v0 - (v0 + a * (v1 - v0)) / (b - a));
    }
    if wide {
        if p != 1 || nu != 2 || (0..nv).any(|j| weights[j] != weights[nv + j]) {
            return None;
        }
        let (a, b) = (
            (lo.x.min(u0) - u0) / (u1 - u0),
            (hi.x.max(u1) - u0) / (u1 - u0),
        );
        for j in 0..nv {
            let (r0, r1) = (points[j], points[nv + j]);
            points[j] = r0 + (r1 - r0) * a;
            points[nv + j] = r0 + (r1 - r0) * b;
        }
        maps[0] = (1.0 / (b - a), u0 - (u0 + a * (u1 - u0)) / (b - a));
    }
    if !(wide || high) {
        return None;
    }
    let (ku, kv) = surface.knots();
    let made = NurbsSurface::new(
        p,
        q,
        ku.to_vec(),
        kv.to_vec(),
        nu,
        nv,
        points,
        Some(weights.to_vec()),
    )
    .ok()?;
    Some((made, maps))
}

/// Where `p` is in the (u, v) of `surface`, which may be past its
/// sides along a parameter the surface goes straight along: looked for
/// near `guess`.
fn place(surface: &NurbsSurface, p: Point3, guess: Point2) -> Point2 {
    let (mut uv, off) = workpiece_boolean::geometry::foot(surface, p, guess);
    if off == 0.0 {
        return uv;
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (nu, nv) = surface.counts();
    let (dp, dq) = surface.degrees();
    // (Past a side it goes straight across: as far past it as the
    // point is along the way across.)
    for _ in 0..2 {
        if dq == 1 && nv == 2 {
            let (a, b) = (surface.point_at(uv.x, v0), surface.point_at(uv.x, v1));
            let d = b - a;
            if d.length_squared() > 0.0 {
                uv.y = v0 + (v1 - v0) * (p - a).dot(d) / d.length_squared();
            }
        }
        if dp == 1 && nu == 2 {
            let (a, b) = (
                surface.point_at(u0, uv.y.clamp(v0, v1)),
                surface.point_at(u1, uv.y.clamp(v0, v1)),
            );
            let d = b - a;
            if d.length_squared() > 0.0 {
                uv.x = u0 + (u1 - u0) * (p - a).dot(d) / d.length_squared();
            }
        }
    }
    uv
}

/// `pcurve` where every (u, v) is taken to `maps` of it.
fn remapped(pcurve: &Curve2, maps: &[(f64, f64); 2]) -> Option<Arc<Curve2>> {
    let points = pcurve
        .control_points()
        .iter()
        .map(|c| Point2::new(c.x * maps[0].0 + maps[0].1, c.y * maps[1].0 + maps[1].1));
    Curve2::new(
        pcurve.degree(),
        pcurve.knots().to_vec(),
        points.collect(),
        Some(pcurve.weights().to_vec()),
    )
    .ok()
    .map(Arc::new)
}

/// `solid` with every face `f` moved `off[f]` along its normal out of
/// the body (into it, for less than nothing; not at all, for nothing)
/// and made up as before: `None` if it is not a body this is so of.
/// `size` is how large the body is; `check` says whether the kernel's
/// own checker is asked too (not, where what is made of it is checked
/// as a whole).
pub(crate) fn offset_same(solid: &Solid, off: &[f64], size: f64, check: bool) -> Option<Solid> {
    let tol = size * 1e-10;
    let nf = solid.faces.len();
    let changed = |f: usize| off[f] != 0.0;
    // The faces along every edge, and around every vertex each with
    // where the vertex is in it.
    let mut beside: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    let mut around: Vec<Vec<(usize, Point2)>> = vec![Vec::new(); solid.vertices.len()];
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            let (ends, span) = match step.walk {
                Walk::Edge { edge, .. } => {
                    beside[edge].push(f);
                    (solid.edges[edge].ends, solid.edges[edge].range)
                }
                Walk::Pole { vertex, range } => ([vertex; 2], range),
            };
            for (v, t) in ends.into_iter().zip([span.0, span.1]) {
                if around[v].iter().all(|x| x.0 != f) {
                    around[v].push((f, step.pcurve.point_at(t)));
                }
            }
        }
    }
    if beside.iter().any(|b| b.len() != 2) {
        return None;
    }
    // The surfaces, moved. (One that comes to a point where its
    // outline stays is moved only as a flat one is: point and all.)
    let mut surfaces: Vec<Arc<NurbsSurface>> = Vec::with_capacity(nf);
    for (f, face) in solid.faces.iter().enumerate() {
        if !changed(f) {
            surfaces.push(face.surface.clone());
            continue;
        }
        let pointed = face
            .loops
            .iter()
            .flatten()
            .any(|step| matches!(step.walk, Walk::Pole { .. }));
        if pointed && flat_way(&face.surface, tol * 1e-3).is_none() {
            return None;
        }
        let by = if face.outward { off[f] } else { -off[f] };
        surfaces.push(Arc::new(moved_off(&face.surface, by, tol)?));
    }
    // The vertices: where the surfaces around each now meet — from
    // where it was, the least way that brings it onto them all. (A
    // surface is asked as far as it reaches; past a side it goes
    // straight across, what it says there holds on.)
    let mut points: Vec<Point3> = solid.vertices.iter().map(|v| v.point).collect();
    let mut moved = vec![false; points.len()];
    for (v, faces) in around.iter().enumerate() {
        if !faces.iter().any(|x| changed(x.0)) {
            continue;
        }
        moved[v] = true;
        let mut p = points[v];
        let mut guess: Vec<Point2> = faces.iter().map(|x| x.1).collect();
        let mut settled = false;
        for _ in 0..12 {
            let mut rows = Vec::with_capacity(faces.len());
            let mut worst = 0.0_f64;
            for (k, (f, _)) in faces.iter().enumerate() {
                let s = &surfaces[*f];
                let (uv, _) = workpiece_boolean::geometry::foot(s, p, guess[k]);
                guess[k] = uv;
                let n = normal(s, uv);
                let b = n.dot(s.point_at(uv.x, uv.y) - p);
                worst = worst.max(b.abs());
                rows.push((n, b));
            }
            if worst <= size * 1e-13 {
                settled = true;
                break;
            }
            p += least_step(&rows);
            if !p.is_finite() {
                return None;
            }
        }
        if !settled {
            return None;
        }
        points[v] = p;
    }
    let mut out = solid.clone();
    for (vertex, point) in out.vertices.iter_mut().zip(&points) {
        vertex.point = *point;
    }
    // A surface its corners have left runs on to them, where it goes
    // straight on: its curves in (u, v) with it.
    let mut relaid = vec![false; nf];
    for (f, face) in solid.faces.iter().enumerate() {
        let mut corners: Vec<(usize, Point2)> = Vec::new();
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                let it = &solid.edges[edge];
                for (v, t) in it.ends.into_iter().zip([it.range.0, it.range.1]) {
                    if moved[v] {
                        corners.push((v, step.pcurve.point_at(t)));
                    }
                }
            }
        }
        if corners.is_empty() {
            continue;
        }
        let s = &surfaces[f];
        let ((u0, u1), (v0, v1)) = s.domain();
        let (mut lo, mut hi) = (Point2::new(u0, v0), Point2::new(u1, v1));
        for (v, guess) in &corners {
            let uv = place(s, points[*v], *guess);
            lo = lo.min(uv);
            hi = hi.max(uv);
        }
        if let Some((wider, maps)) = stretched(s, lo, hi) {
            surfaces[f] = Arc::new(wider);
            relaid[f] = true;
            for step in out.faces[f].loops.iter_mut().flatten() {
                step.pcurve = remapped(&step.pcurve, &maps)?;
            }
            for x in around.iter_mut().flatten().filter(|x| x.0 == f) {
                x.1 = Point2::new(x.1.x * maps[0].0 + maps[0].1, x.1.y * maps[1].0 + maps[1].1);
            }
        }
    }
    // (Every corner on every surface around it, as they are now.)
    for (v, faces) in around.iter().enumerate() {
        if !moved[v] {
            continue;
        }
        for (f, guess) in faces {
            let s = &surfaces[*f];
            let (uv, _) = workpiece_boolean::geometry::foot(s, points[v], *guess);
            if s.point_at(uv.x, uv.y).distance(points[v]) > tol {
                return None;
            }
        }
    }
    for (face, surface) in out.faces.iter_mut().zip(&surfaces) {
        face.surface = surface.clone();
    }
    // The edges: a straight one between its ends as they are now, one
    // around a circle around that circle's axis still, as much larger
    // or smaller and as far along it as its ends are.
    let mut redone = vec![false; solid.edges.len()];
    for (e, edge) in solid.edges.iter().enumerate() {
        let [va, vb] = edge.ends;
        if !(moved[va] || moved[vb] || beside[e].iter().any(|f| changed(*f))) {
            continue;
        }
        redone[e] = true;
        let (a, b) = (edge.point(edge.range.0), edge.point(edge.range.1));
        let (na, nb) = (points[va], points[vb]);
        let d = b - a;
        let straight = va != vb
            && d.length() > tol
            && edge
                .curve
                .control_points()
                .iter()
                .all(|p| (*p - a).cross(d).length() <= tol * d.length());
        let to: Box<dyn Fn(Point3) -> Point3> = if straight {
            let nd = nb - na;
            // (Not turned around, nor come to nothing.)
            if nd.dot(d) <= 0.0 || nd.length() <= size * 1e-9 {
                return None;
            }
            Box::new(move |p: Point3| na + nd * ((p - a).dot(d) / d.length_squared()))
        } else {
            let (centre, axis, radius) = circle_of(&edge.curve, edge.range, tol)?;
            let now = centre + axis * (na - centre).dot(axis);
            // (Not through its axis, nor come to nothing.)
            let grown = (na - now).dot(a - centre) / (radius * radius);
            if grown * radius <= size * 1e-9 {
                return None;
            }
            let to = move |p: Point3| now + (p - centre) * grown;
            if to(a).distance(na) > tol || to(b).distance(nb) > tol {
                return None;
            }
            Box::new(to)
        };
        let curve = Curve3::new(
            edge.curve.degree(),
            edge.curve.knots().to_vec(),
            edge.curve.control_points().iter().map(|p| to(*p)).collect(),
            Some(edge.curve.weights().to_vec()),
        )
        .ok()?;
        out.edges[e].curve = Arc::new(curve);
    }
    // Their curves in the faces: exactly, in a flat face laid out
    // evenly; where the curve was a straight line of the face's (u, v)
    // (along a cylinder, around it), the line between its ends as they
    // are now — if the edge is there all along.
    let mut touched = vec![false; nf];
    for f in 0..nf {
        let surface = &surfaces[f];
        let flat = Flat::of(surface, tol);
        for l in 0..out.faces[f].loops.len() {
            for s in 0..out.faces[f].loops[l].len() {
                let step = &out.faces[f].loops[l][s];
                let Walk::Edge { edge, .. } = step.walk else {
                    continue;
                };
                if !redone[edge] {
                    continue;
                }
                touched[f] = true;
                let it = &out.edges[edge];
                let (t0, t1) = it.range;
                let made = match &flat {
                    Some(flat) => flat.curve(&it.curve)?,
                    None => {
                        let old = &step.pcurve;
                        let plain = old.degree() == 1
                            && old.control_points().len() == 2
                            && old.weights().iter().all(|w| *w == 1.0);
                        if !plain || t1 <= t0 {
                            return None;
                        }
                        // (Where the surface comes to a point, the
                        // point is wherever the line was.)
                        let foot = |t: f64| {
                            let was = old.point_at(t);
                            let uv = workpiece_boolean::geometry::foot(surface, it.point(t), was).0;
                            let (_, du, dv) = surface.derivatives_at(was.x, was.y);
                            let ((u0, u1), (v0, v1)) = surface.domain();
                            Point2::new(
                                if du.length() * (u1 - u0) <= tol {
                                    was.x
                                } else {
                                    uv.x
                                },
                                if dv.length() * (v1 - v0) <= tol {
                                    was.y
                                } else {
                                    uv.y
                                },
                            )
                        };
                        let (ua, ub) = (foot(t0), foot(t1));
                        let slope = (ub - ua) / (t1 - t0);
                        let (k0, k1) = old.domain();
                        let line = |t: f64| ua + slope * (t - t0);
                        Arc::new(
                            Curve2::new(1, vec![k0, k0, k1, k1], vec![line(k0), line(k1)], None)
                                .ok()?,
                        )
                    }
                };
                for i in 0..=8 {
                    let t = t0 + (t1 - t0) * f64::from(i) / 8.0;
                    let uv = made.point_at(t);
                    let off = surface.point_at(uv.x, uv.y).distance(it.point(t));
                    if off.is_nan() || off > tol {
                        return None;
                    }
                }
                out.faces[f].loops[l][s].pcurve = made;
            }
        }
        // (A point the surface comes to, moved with it: still where
        // its curve in (u, v) is.)
        for step in out.faces[f].loops.iter().flatten() {
            if let Walk::Pole { vertex, range } = step.walk {
                let uv = step.pcurve.point_at((range.0 + range.1) / 2.0);
                if surface.point_at(uv.x, uv.y).distance(points[vertex]) > tol {
                    return None;
                }
            }
        }
    }
    // Every face as it was made up: its outlines going around the way
    // they did, crossing neither themselves nor each other.
    for (f, face) in out.faces.iter().enumerate() {
        if !(touched[f] || relaid[f]) {
            continue;
        }
        let (was, areas_were) = outline(solid, &solid.faces[f]);
        let (now, areas) = outline(&out, face);
        let scale = |loops: &[Vec<Point2>]| {
            let all = loops.iter().flatten();
            let (lo, hi) = all.fold((Point2::INFINITY, Point2::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(*p), hi.max(*p))
            });
            (hi - lo).length_squared()
        };
        let none = scale(&was).max(scale(&now)) * 1e-12;
        for (a, b) in areas_were.iter().zip(&areas) {
            if a.abs() > none && (b.abs() <= none || a.signum() != b.signum()) {
                return None;
            }
        }
        if tangled(&now) {
            return None;
        }
        // (And each still within, or without, the others it was.)
        for i in 0..now.len() {
            for j in 0..now.len() {
                if i != j && within(was[i][0], &was[j]) != within(now[i][0], &now[j]) {
                    return None;
                }
            }
        }
    }
    // No face brought to where another is that it has no corner in
    // common with: there is a way along which the two are apart — one
    // of the axes, or straight off one of them that is flat.
    let mut hulls: Vec<Vec<Point3>> = Vec::with_capacity(nf);
    let mut corners: Vec<Vec<usize>> = Vec::with_capacity(nf);
    let mut ways: Vec<Option<Point3>> = Vec::with_capacity(nf);
    for (f, face) in out.faces.iter().enumerate() {
        let mut hull = Vec::new();
        let mut own = Vec::new();
        for step in face.loops.iter().flatten() {
            match step.walk {
                Walk::Edge { edge, .. } => {
                    // (Of its curve, the piece that is the edge.)
                    let it = &out.edges[edge];
                    let piece = it.curve.trimmed(it.range.0, it.range.1).ok()?;
                    hull.extend(piece.control_points().iter().copied());
                    own.extend(it.ends);
                }
                Walk::Pole { vertex, .. } => own.push(vertex),
            }
        }
        // (A face that bends both ways reaches past its edges: as far
        // as its whole surface, at most.)
        let s = &surfaces[f];
        let (p, q) = s.degrees();
        let way = flat_way(s, tol);
        if p > 1 && q > 1 && way.is_none() {
            hull.extend(s.control_points().iter().copied());
        }
        own.sort_unstable();
        own.dedup();
        hulls.push(hull);
        corners.push(own);
        ways.push(way);
    }
    let reach = |hull: &[Point3], way: Point3| {
        hull.iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.dot(way)), hi.max(p.dot(way)))
            })
    };
    for f in 0..nf {
        for g in f + 1..nf {
            if !(touched[f] || touched[g]) || corners[f].iter().any(|v| corners[g].contains(v)) {
                continue;
            }
            let middle = |hull: &[Point3]| hull.iter().sum::<Point3>() / hull.len().max(1) as f64;
            let between = (middle(&hulls[g]) - middle(&hulls[f])).try_normalize();
            let along = [
                Some(Point3::X),
                Some(Point3::Y),
                Some(Point3::Z),
                ways[f],
                ways[g],
                between,
            ];
            let apart = along.into_iter().flatten().any(|way| {
                let (a, b) = (reach(&hulls[f], way), reach(&hulls[g], way));
                a.0 - b.1 > tol || b.0 - a.1 > tol
            });
            if !apart {
                return None;
            }
        }
    }
    out.settle();
    if check {
        // And sound, as the kernel's own checker sees it.
        let mut scratch = Topo::new();
        workpiece_boolean::write(&mut scratch, &out, "moved").ok()?;
        scratch.validate().ok()?;
    }
    Some(out)
}
