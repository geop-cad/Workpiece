//! What a result is left with that it does not need: faces side by side
//! in one surface — the pieces of a face that lie side by side again,
//! and flat faces in one plane, as of two blocks set flush — are made
//! one face, and edges end to end along one curve one edge.

use std::collections::BTreeMap;
use std::sync::Arc;

use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::{Curve2, Curve3, NurbsSurface};

use crate::model::{Face, Solid, Use, Walk};

/// Makes faces of `solid` that lie side by side in one surface one
/// face, and edges that run on along one curve one edge.
pub(crate) fn tidy(solid: &mut Solid) {
    let mut gone_faces = vec![false; solid.faces.len()];
    let mut gone_edges = vec![false; solid.edges.len()];
    // Face by face with its neighbours, until nothing more joins.
    let mut again = true;
    while again {
        again = false;
        let mut beside: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
        for (f, face) in solid.faces.iter().enumerate() {
            if gone_faces[f] {
                continue;
            }
            for step in face.loops.iter().flatten() {
                if let Walk::Edge { edge, .. } = step.walk {
                    beside[edge].push(f);
                }
            }
        }
        // (A face joined to another in this round is that other.)
        let mut now: Vec<usize> = (0..solid.faces.len()).collect();
        for (e, faces) in beside.iter().enumerate() {
            let &[a, b] = &faces[..] else {
                continue;
            };
            let stands = |mut f: usize| {
                while now[f] != f {
                    f = now[f];
                }
                f
            };
            let (a, b) = (stands(a), stands(b));
            if gone_edges[e] || a == b {
                continue;
            }
            if join_faces(solid, &mut gone_faces, &mut gone_edges, a, b) {
                now[a.max(b)] = a.min(b);
                again = true;
            }
        }
    }
    join_edges(solid, &gone_faces, &mut gone_edges);
    compact(solid, &gone_faces, &gone_edges);
}

/// Whether the two surfaces are one and the same, point for point.
fn identical(a: &Arc<NurbsSurface>, b: &Arc<NurbsSurface>) -> bool {
    Arc::ptr_eq(a, b)
        || (a.degrees() == b.degrees()
            && a.knots() == b.knots()
            && a.control_points() == b.control_points()
            && a.weights() == b.weights())
}

/// Makes faces `f` and `g` one, if they lie in one surface: all the
/// edges between them go.
fn join_faces(
    solid: &mut Solid,
    gone_faces: &mut [bool],
    gone_edges: &mut [bool],
    f: usize,
    g: usize,
) -> bool {
    let (f, g) = (f.min(g), f.max(g));
    let (face_f, face_g) = (&solid.faces[f], &solid.faces[g]);
    // The two as they are, on the surface they share; or two flat ones
    // in one plane, both brought onto one patch.
    let same = identical(&face_f.surface, &face_g.surface) && face_f.outward == face_g.outward;
    let (surface, outward, loops_f, loops_g, near) = if same {
        let ((u0, u1), (v0, v1)) = face_f.surface.domain();
        (
            face_f.surface.clone(),
            face_f.outward,
            face_f.loops.clone(),
            face_g.loops.clone(),
            1e-9 * (1.0 + (u1 - u0).abs() + (v1 - v0).abs()),
        )
    } else {
        let Some(flat) = flat_together(solid, face_f, face_g) else {
            return false;
        };
        flat
    };
    // The steps of both, and after each the one that follows it once
    // the edges between the two are gone: around such an edge, on along
    // the other face.
    let steps: Vec<Use> = loops_f.iter().chain(&loops_g).flatten().cloned().collect();
    let mut next = vec![0; steps.len()];
    let mut from_f = vec![false; steps.len()];
    let mut at = 0;
    for (which, loops) in [&loops_f, &loops_g].into_iter().enumerate() {
        for walk in loops {
            for k in 0..walk.len() {
                next[at + k] = at + (k + 1) % walk.len();
                from_f[at + k] = which == 0;
            }
            at += walk.len();
        }
    }
    let edge_of = |k: usize| match steps[k].walk {
        Walk::Edge { edge, .. } => Some(edge),
        Walk::Pole { .. } => None,
    };
    // The edges between the two: walked by both, at one place of the
    // surface. (An edge one face walks twice is a seam of its own; and
    // one the two walk at different places is where the surface closes
    // on itself, a seam of the two together.)
    let at_one_place = |a: usize, b: usize| {
        let Some(edge) = edge_of(a) else {
            return false;
        };
        let (t0, t1) = solid.edges[edge].range;
        [t0, t1].into_iter().all(|t| {
            steps[a]
                .pcurve
                .point_at(t)
                .distance(steps[b].pcurve.point_at(t))
                <= near
        })
    };
    let mut twin: Vec<Option<usize>> = vec![None; steps.len()];
    let mut by_edge: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for k in 0..steps.len() {
        if let Some(edge) = edge_of(k) {
            by_edge.entry(edge).or_default().push(k);
        }
    }
    for walked in by_edge.values() {
        for (i, &a) in walked.iter().enumerate() {
            for &b in &walked[i + 1..] {
                if from_f[a] == from_f[b]
                    || twin[a].is_some()
                    || twin[b].is_some()
                    || !at_one_place(a, b)
                {
                    continue;
                }
                twin[a] = Some(b);
                twin[b] = Some(a);
            }
        }
    }
    // They are walked opposite ways, as the two sides of one edge are.
    let opposite = |a: usize, b: usize| match (steps[a].walk, steps[b].walk) {
        (Walk::Edge { reversed: x, .. }, Walk::Edge { reversed: y, .. }) => x != y,
        _ => false,
    };
    if (0..steps.len()).any(|a| twin[a].is_some_and(|b| !opposite(a, b))) {
        return false;
    }
    if twin.iter().all(Option::is_none) {
        return false;
    }
    let follow = |mut k: usize| -> Option<usize> {
        for _ in 0..=steps.len() {
            k = next[k];
            match twin[k] {
                Some(other) => k = other,
                None => return Some(k),
            }
        }
        None
    };
    let mut seen = vec![false; steps.len()];
    let mut loops: Vec<Vec<Use>> = Vec::new();
    for start in 0..steps.len() {
        if seen[start] || twin[start].is_some() {
            continue;
        }
        let mut walk = Vec::new();
        let mut k = start;
        while !seen[k] {
            seen[k] = true;
            walk.push(steps[k].clone());
            let Some(on) = follow(k) else {
                return false;
            };
            k = on;
        }
        if k != start {
            return false;
        }
        loops.push(walk);
    }
    // (A surface that closes on itself with nothing left to bound it
    // is no face.)
    if loops.is_empty() {
        return false;
    }
    for (k, twin) in twin.iter().enumerate() {
        if let (Some(_), Some(edge)) = (twin, edge_of(k)) {
            gone_edges[edge] = true;
        }
    }
    let name = match (&solid.faces[f].name, &solid.faces[g].name) {
        // (The first piece of a face is the one called as the face.)
        (Some(a), Some(b)) if a.ends_with('#') && !b.ends_with('#') => Some(b.clone()),
        (None, b) => b.clone(),
        (a, _) => a.clone(),
    };
    let tolerance = solid.faces[f].tolerance.max(solid.faces[g].tolerance);
    // What either was called or known by, the one face is known by.
    let mut also: Vec<String> = Vec::new();
    for face in [&solid.faces[f], &solid.faces[g]] {
        let names = face.name.iter().chain(&face.also);
        for known in names.map(|n| n.trim_end_matches('#').to_owned()) {
            let own = name.as_deref().map(|n| n.trim_end_matches('#'));
            if Some(known.as_str()) != own && !also.contains(&known) {
                also.push(known);
            }
        }
    }
    let mut smooth = solid.faces[f].smooth.clone();
    for &e in &solid.faces[g].smooth {
        if !smooth.contains(&e) {
            smooth.push(e);
        }
    }
    solid.faces[f] = Face {
        surface,
        outward,
        loops,
        tolerance,
        name,
        also,
        smooth,
    };
    gone_faces[g] = true;
    true
}

/// The plane a surface lies in, if it is flat to within `within`: a
/// point of it and its unit normal, the way ∂u × ∂v points.
fn plane_of(surface: &NurbsSurface, within: f64) -> Option<(Point3, Point3)> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    // (Away from its rim: there it may come to a point.)
    let (at, du, dv) = surface.derivatives_at(u0 + (u1 - u0) * 0.37, v0 + (v1 - v0) * 0.41);
    let normal = du.cross(dv).try_normalize()?;
    surface
        .control_points()
        .iter()
        .all(|p| (*p - at).dot(normal).abs() <= within)
        .then_some((at, normal))
}

/// Two flat faces in one plane, facing the same way, on one patch that
/// holds both: the patch, whether it faces outwards, the loops of the
/// two on it, and how near two places on it are one.
#[allow(clippy::type_complexity)]
fn flat_together(
    solid: &Solid,
    f: &Face,
    g: &Face,
) -> Option<(Arc<NurbsSurface>, bool, Vec<Vec<Use>>, Vec<Vec<Use>>, f64)> {
    let within = f.tolerance + g.tolerance;
    let (origin, normal) = plane_of(&f.surface, within)?;
    let (other, other_normal) = plane_of(&g.surface, within)?;
    let out = |n: Point3, face: &Face| if face.outward { n } else { -n };
    if out(normal, f).dot(out(other_normal, g)) < 1.0 - 1e-12 {
        return None;
    }
    // (All of the second, not a point of it alone: two planes a hair's
    // turn apart are one only as far as they are.)
    let level = |p: &Point3| (*p - origin).dot(normal).abs() <= within;
    if !level(&other) || !g.surface.control_points().iter().all(level) {
        return None;
    }
    // The patch: measured in lengths from a point of the first face,
    // its normal the first face's own.
    let x = normal.any_orthonormal_vector();
    let y = normal.cross(x);
    let onto = |p: Point3| Point2::new((p - origin).dot(x), (p - origin).dot(y));
    // The second face's loops run the other way around if its surface
    // faces the other way.
    let turned = f.outward != g.outward;
    let (mut lo, mut hi) = (Point2::INFINITY, Point2::NEG_INFINITY);
    let mut moved = |face: &Face, turned: bool| -> Option<Vec<Vec<Use>>> {
        let walked = |edge: usize| {
            let steps = face.loops.iter().flatten();
            steps
                .filter(|s| matches!(s.walk, Walk::Edge { edge: other, .. } if other == edge))
                .count()
        };
        let mut loops = Vec::new();
        for walk in &face.loops {
            let mut steps = Vec::new();
            for step in walk {
                // (Where its surface came to a point, or closed on
                // itself, the plane does neither: a disk's centre and
                // its seam bound nothing.)
                let Walk::Edge { edge, reversed } = step.walk else {
                    continue;
                };
                // (A face whose surface closes on itself — a disk made
                // by turning a line about its end — is left as it is.)
                if walked(edge) > 1 {
                    return None;
                }
                // The edge's own curve, seen in the plane.
                let curve = &solid.edges[edge].curve;
                let points: Vec<Point2> = curve.control_points().iter().map(|p| onto(*p)).collect();
                for p in &points {
                    (lo, hi) = (lo.min(*p), hi.max(*p));
                }
                let pcurve = Curve2::new(
                    curve.degree(),
                    curve.knots().to_vec(),
                    points,
                    Some(curve.weights().to_vec()),
                )
                .ok()?;
                steps.push(Use {
                    walk: Walk::Edge {
                        edge,
                        reversed: reversed != turned,
                    },
                    pcurve: Arc::new(pcurve),
                });
            }
            if turned {
                steps.reverse();
            }
            if !steps.is_empty() {
                loops.push(steps);
            }
        }
        Some(loops)
    };
    let loops_f = moved(f, false)?;
    let loops_g = moved(g, turned)?;
    if loops_f.is_empty() || loops_g.is_empty() {
        return None;
    }
    let pad = (hi - lo).length() * 0.01 + 1e-9;
    let (lo, hi) = (lo - Point2::splat(pad), hi + Point2::splat(pad));
    let place = |p: Point2| origin + x * p.x + y * p.y;
    let surface = NurbsSurface::new(
        1,
        1,
        vec![lo.x, lo.x, hi.x, hi.x],
        vec![lo.y, lo.y, hi.y, hi.y],
        2,
        2,
        vec![
            place(lo),
            place(Point2::new(lo.x, hi.y)),
            place(Point2::new(hi.x, lo.y)),
            place(hi),
        ],
        None,
    )
    .ok()?;
    let size = (hi - lo).length();
    Some((
        Arc::new(surface),
        f.outward,
        loops_f,
        loops_g,
        within * 2.0 + 1e-9 * size,
    ))
}

/// Makes two edges that meet at a vertex nothing else comes to, and run
/// on along one curve there, one edge.
fn join_edges(solid: &mut Solid, gone_faces: &[bool], gone_edges: &mut [bool]) {
    let poles: Vec<usize> = solid
        .faces
        .iter()
        .enumerate()
        .filter(|(f, _)| !gone_faces[*f])
        .flat_map(|(_, face)| face.loops.iter().flatten())
        .filter_map(|step| match step.walk {
            Walk::Pole { vertex, .. } => Some(vertex),
            Walk::Edge { .. } => None,
        })
        .collect();
    let mut again = true;
    while again {
        again = false;
        let mut at: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (e, edge) in solid.edges.iter().enumerate() {
            if !gone_edges[e] {
                for v in edge.ends {
                    at.entry(v).or_default().push(e);
                }
            }
        }
        for (v, edges) in at {
            let &[a, b] = &edges[..] else {
                continue;
            };
            if a == b || poles.contains(&v) || gone_edges[a] || gone_edges[b] {
                continue;
            }
            if join_two(solid, gone_faces, a, b, v) {
                gone_edges[b] = true;
                again = true;
            }
        }
    }
}

/// Whether the piece of `curve` over `range` is straight: its control
/// points in one line, one after the other, to within `within`.
fn straight<P: shapecraft_core::world::topo::ControlPoint>(
    curve: &shapecraft_core::world::topo::NurbsCurve<P>,
    range: (f64, f64),
    within: f64,
) -> bool {
    if curve.degree() == 1 && curve.control_points().len() == 2 {
        return true;
    }
    let Ok(piece) = curve.trimmed(range.0.min(range.1), range.0.max(range.1)) else {
        return false;
    };
    let points = piece.control_points();
    let (from, to) = (points[0], points[points.len() - 1]);
    let apart = |a: P, b: P| {
        let d = a + b * -1.0;
        d.dot(d).sqrt()
    };
    let length = apart(from, to);
    if length <= within {
        return false;
    }
    let mut before = 0.0;
    points.iter().all(|&p| {
        // (Where along the chord it is, and how far off it.)
        let (da, db) = (apart(p, from), apart(p, to));
        let along = (da * da - db * db + length * length) / (2.0 * length);
        let off = (da * da - along * along).max(0.0).sqrt();
        let onward = along >= before - within;
        before = before.max(along);
        off <= within && onward
    })
}

/// `first` and then `second` as one curve, of the higher of their
/// degrees; `second`'s parameters `stretch` times as long.
fn end_to_end<P: shapecraft_core::world::topo::ControlPoint>(
    first: &shapecraft_core::world::topo::NurbsCurve<P>,
    second: &shapecraft_core::world::topo::NurbsCurve<P>,
    stretch: f64,
) -> Option<shapecraft_core::world::topo::NurbsCurve<P>> {
    let degree = first.degree().max(second.degree());
    first
        .elevated(degree)
        .ok()?
        .joined(&second.elevated(degree).ok()?, stretch)
        .ok()
}

/// Makes edge `a` what it and edge `b` are together, if the two run on
/// through vertex `v` in one direction: pieces of one curve that follow
/// one another, two straight pieces, or curves of one degree that meet
/// there without a bend.
fn join_two(solid: &mut Solid, gone_faces: &[bool], a: usize, b: usize, v: usize) -> bool {
    let (ea, eb) = (solid.edges[a].clone(), solid.edges[b].clone());
    if ea.ends[0] == ea.ends[1] || eb.ends[0] == eb.ends[1] {
        return false;
    }
    // Which end of each is at the vertex.
    let (end_a, end_b) = (usize::from(ea.ends[1] == v), usize::from(eb.ends[1] == v));
    let (far_a, far_b) = (ea.ends[1 - end_a], eb.ends[1 - end_b]);
    if far_a == far_b {
        return false;
    }
    let t = |range: (f64, f64), end: usize| if end == 0 { range.0 } else { range.1 };
    let span = (ea.range.1 - ea.range.0).abs() + (eb.range.1 - eb.range.0).abs();
    let within = ea.tolerance.max(eb.tolerance);
    // A piece of a curve over a range, the way it goes on from the one
    // edge into the other: the first edge's towards the vertex, the
    // second's away from it.
    fn piece<P: shapecraft_core::world::topo::ControlPoint>(
        curve: &shapecraft_core::world::topo::NurbsCurve<P>,
        range: (f64, f64),
        turned: bool,
    ) -> Option<shapecraft_core::world::topo::NurbsCurve<P>> {
        let piece = curve.trimmed(range.0, range.1).ok()?;
        Some(if turned { piece.reversed() } else { piece })
    }
    // The one curve: theirs, where they are pieces of one that follow
    // one another; the line both are of; or the two one after the
    // other, where they do not bend at the vertex.
    let same = Arc::ptr_eq(&ea.curve, &eb.curve)
        && end_a != end_b
        && (t(ea.range, end_a) - t(eb.range, end_b)).abs() <= 1e-12 * span;
    let lines = straight(&ea.curve, ea.range, within) && straight(&eb.curve, eb.range, within);
    // How much longer the second edge's parameters get, joined on.
    let mut stretch = 1.0;
    let (curve, range, flip_a, flip_b) = if same {
        let range = (ea.range.0.min(eb.range.0), ea.range.1.max(eb.range.1));
        (ea.curve.clone(), range, false, false)
    } else if lines {
        let (from, through, to) = (
            ea.point(t(ea.range, 1 - end_a)),
            ea.point(t(ea.range, end_a)),
            eb.point(t(eb.range, 1 - end_b)),
        );
        let way = (to - from).try_normalize();
        let Some(way) = way else {
            return false;
        };
        let straight = (through - from).reject_from_normalized(way).length() <= within
            && (through - from).dot(way) > 0.0
            && (to - through).dot(way) > 0.0;
        let Ok(curve) = Curve3::line(from, to) else {
            return false;
        };
        if !straight {
            return false;
        }
        // (It runs from the first edge's far end to the second's.)
        (Arc::new(curve), (0.0, 1.0), end_a == 0, end_b == 1)
    } else {
        let (Some(first), Some(second)) = (
            piece(&ea.curve, ea.range, end_a == 0),
            piece(&eb.curve, eb.range, end_b == 1),
        ) else {
            return false;
        };
        let (_, before) = first.derivative_at(first.domain().1);
        let (_, after) = second.derivative_at(second.domain().0);
        let (Some(way), Some(on)) = (before.try_normalize(), after.try_normalize()) else {
            return false;
        };
        // (No bend to speak of: less than a thousandth of a radian.)
        if way.dot(on) < 1.0 - 5e-7 {
            return false;
        }
        stretch = after.length() / before.length();
        let Some(curve) = end_to_end(&first, &second, stretch) else {
            return false;
        };
        let range = curve.domain();
        (Arc::new(curve), range, end_a == 0, end_b == 1)
    };
    // Every face that walks the one walks the other next: its two
    // curves there become one.
    let mut changes: Vec<(usize, usize, usize, usize, Use)> = Vec::new();
    for (f, face) in solid.faces.iter().enumerate() {
        if gone_faces[f] {
            continue;
        }
        for (l, walk) in face.loops.iter().enumerate() {
            for k in 0..walk.len() {
                let Walk::Edge { edge, reversed } = walk[k].walk else {
                    continue;
                };
                if edge != a {
                    continue;
                }
                // The step with the other edge: after this one if it
                // is walked towards the vertex, before it if away.
                let towards = (end_a == 1) != reversed;
                let other = if towards {
                    (k + 1) % walk.len()
                } else {
                    (k + walk.len() - 1) % walk.len()
                };
                if !matches!(walk[other].walk, Walk::Edge { edge, .. } if edge == b) || other == k {
                    return false;
                }
                let (pa, pb) = (&walk[k].pcurve, &walk[other].pcurve);
                let pcurve = if same && Arc::ptr_eq(pa, pb) {
                    pa.clone()
                } else if !same && !lines {
                    // The two one after the other, as the edges' curves.
                    let (Some(first), Some(second)) = (
                        piece(pa, ea.range, end_a == 0),
                        piece(pb, eb.range, end_b == 1),
                    ) else {
                        return false;
                    };
                    let Some(made) = end_to_end(&first, &second, stretch) else {
                        return false;
                    };
                    Arc::new(made)
                } else {
                    // Two straight pieces that are one straight piece.
                    let size = |c: &Curve2| {
                        let (lo, hi) = c.domain();
                        c.point_at(lo).distance(c.point_at(hi))
                    };
                    let tiny = 1e-9 * (size(pa) + size(pb));
                    if !straight(pa, ea.range, tiny) || !straight(pb, eb.range, tiny) {
                        return false;
                    }
                    let (from, to) = (
                        pa.point_at(t(ea.range, 1 - end_a)),
                        pb.point_at(t(eb.range, 1 - end_b)),
                    );
                    // (From the new curve's start to its end.)
                    let starts_at_a = if same { ea.range.0 < eb.range.0 } else { true };
                    let (from, to) = if starts_at_a { (from, to) } else { (to, from) };
                    let Ok(made) = Curve2::new(
                        1,
                        vec![range.0, range.0, range.1, range.1],
                        vec![from, to],
                        None,
                    ) else {
                        return false;
                    };
                    let within = within.max(face.tolerance) * 2.0 + 1e-9;
                    let fits = [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().all(|x| {
                        let at = range.0 + (range.1 - range.0) * x;
                        let uv = made.point_at(at);
                        face.surface
                            .point_at(uv.x, uv.y)
                            .distance(curve.point_at(at))
                            <= within
                    });
                    if !fits {
                        return false;
                    }
                    Arc::new(made)
                };
                let step = Use {
                    walk: Walk::Edge {
                        edge: a,
                        reversed: reversed != flip_a,
                    },
                    pcurve,
                };
                changes.push((f, l, k, other, step));
            }
        }
    }
    let _ = flip_b;
    // (Both are walked by the same faces, as often.)
    let count = |e: usize| {
        solid
            .faces
            .iter()
            .enumerate()
            .filter(|(f, _)| !gone_faces[*f])
            .flat_map(|(_, face)| face.loops.iter().flatten())
            .filter(|step| matches!(step.walk, Walk::Edge { edge, .. } if edge == e))
            .count()
    };
    if changes.len() != count(a) || changes.len() != count(b) {
        return false;
    }
    // The later steps of a loop first, so that the earlier keep their
    // places.
    changes.sort_by_key(|x| std::cmp::Reverse((x.0, x.1, x.3)));
    for (f, l, k, _, step) in &changes {
        solid.faces[*f].loops[*l][*k] = step.clone();
    }
    for (f, l, _, other, _) in &changes {
        solid.faces[*f].loops[*l].remove(*other);
    }
    for face in &mut solid.faces {
        if face.smooth.contains(&b) {
            face.smooth.retain(|e| *e != b);
            if !face.smooth.contains(&a) {
                face.smooth.push(a);
            }
        }
    }
    let edge = &mut solid.edges[a];
    edge.curve = curve;
    edge.range = range;
    edge.ends = if same {
        if ea.range.0 < eb.range.0 {
            [ea.ends[0], eb.ends[1]]
        } else {
            [eb.ends[0], ea.ends[1]]
        }
    } else {
        [far_a, far_b]
    };
    edge.tolerance = ea.tolerance.max(eb.tolerance);
    if edge.name.is_none() {
        edge.name = eb.name.clone();
    }
    for known in eb.name.iter().chain(&eb.also) {
        if Some(known) != edge.name.as_ref() && !edge.also.contains(known) {
            edge.also.push(known.clone());
        }
    }
    true
}

/// Drops the faces and edges that are gone and the vertices nothing
/// ends at any more.
fn compact(solid: &mut Solid, gone_faces: &[bool], gone_edges: &[bool]) {
    if !gone_faces.contains(&true) && !gone_edges.contains(&true) {
        return;
    }
    let mut used = vec![false; solid.vertices.len()];
    for (e, edge) in solid.edges.iter().enumerate() {
        if !gone_edges[e] {
            for v in edge.ends {
                used[v] = true;
            }
        }
    }
    for (f, face) in solid.faces.iter().enumerate() {
        if !gone_faces[f] {
            for step in face.loops.iter().flatten() {
                if let Walk::Pole { vertex, .. } = step.walk {
                    used[vertex] = true;
                }
            }
        }
    }
    let renumber = |keep: &[bool]| -> Vec<usize> {
        let mut at = 0;
        keep.iter()
            .map(|k| {
                at += usize::from(*k);
                at.saturating_sub(1)
            })
            .collect()
    };
    let vertex = renumber(&used);
    let kept_edges: Vec<bool> = gone_edges.iter().map(|g| !g).collect();
    let edge = renumber(&kept_edges);
    let mut k = 0;
    solid.vertices.retain(|_| {
        k += 1;
        used[k - 1]
    });
    let mut k = 0;
    solid.edges.retain(|_| {
        k += 1;
        kept_edges[k - 1]
    });
    for e in &mut solid.edges {
        e.ends = e.ends.map(|v| vertex[v]);
    }
    let mut k = 0;
    solid.faces.retain(|_| {
        k += 1;
        !gone_faces[k - 1]
    });
    for face in &mut solid.faces {
        face.smooth = face
            .smooth
            .iter()
            .filter(|&&e| kept_edges[e])
            .map(|&e| edge[e])
            .collect();
        for step in face.loops.iter_mut().flatten() {
            step.walk = match step.walk {
                Walk::Edge { edge: e, reversed } => Walk::Edge {
                    edge: edge[e],
                    reversed,
                },
                Walk::Pole { vertex: v, range } => Walk::Pole {
                    vertex: vertex[v],
                    range,
                },
            };
        }
    }
}
