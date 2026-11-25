//! Blends made where they are: the two faces beside an edge cut back
//! to the blend's rails, the blend's face laid between them, and the
//! faces the edge ends on cut by the blend's end sections — nothing is
//! combined. For the blends that touch nothing but those faces: the
//! rails stay inside the two faces, each face is flat (a rail's curve
//! in it is then the rail itself, exactly) or one the rail runs along
//! as the edge does, a way across (a cylinder's rim: the rail's curve
//! in it is the edge's, moved over), and each end is at a corner of
//! three faces whose third is flat and holds the end section — or
//! there is no end: the edge closes on itself, and the blend with it.
//! Whatever is not so is left to the tools and the booleans of
//! [`crate::blend`]; so is whatever comes out unsound here.

use std::sync::Arc;

use shapecraft_boolean::Solid;
use shapecraft_boolean::model::{Edge, Face, Use, Vertex, Walk};
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::{Curve2, Curve3, NurbsSurface, Topo};

use crate::roll::{Rolled, Stretch};

/// A flat surface whose (u, v) is a straight measure of it: where a
/// point of its plane is in (u, v).
pub(crate) struct Flat {
    origin: Point3,
    /// The (u, v) of a point as two rows to multiply its offset by.
    rows: [Point3; 2],
    low: Point2,
    normal: Point3,
    within: f64,
}

impl Flat {
    /// `surface`, if it is such a one (a parallelogram's patch).
    pub(crate) fn of(surface: &NurbsSurface, within: f64) -> Option<Flat> {
        if surface.degrees() != (1, 1)
            || surface.counts() != (2, 2)
            || surface.weights().iter().any(|w| *w != 1.0)
        {
            return None;
        }
        let ((u0, u1), (v0, v1)) = surface.domain();
        let origin = surface.point_at(u0, v0);
        let (a, b) = (
            surface.point_at(u1, v0) - origin,
            surface.point_at(u0, v1) - origin,
        );
        let far = surface.point_at(u1, v1);
        if (origin + a + b).distance(far) > within {
            return None;
        }
        let normal = a.cross(b);
        let area = normal.length_squared();
        if area <= 0.0 || !area.is_finite() {
            return None;
        }
        // (a·x + b·y = p: solved with the two's reciprocal pair.)
        let rows = [
            b.cross(normal) / area * (u1 - u0),
            normal.cross(a) / area * (v1 - v0),
        ];
        Some(Flat {
            origin,
            rows,
            low: Point2::new(u0, v0),
            normal: normal / area.sqrt(),
            within,
        })
    }

    /// Where `p` is in (u, v), if it is in the plane.
    fn at(&self, p: Point3) -> Option<Point2> {
        let d = p - self.origin;
        if d.dot(self.normal).abs() > self.within {
            return None;
        }
        Some(self.low + Point2::new(d.dot(self.rows[0]), d.dot(self.rows[1])))
    }

    /// `curve` in (u, v): its control points there, if all are in the
    /// plane (the curve is then, and this is it exactly).
    pub(crate) fn curve(&self, curve: &Curve3) -> Option<Arc<Curve2>> {
        let points: Option<Vec<Point2>> =
            curve.control_points().iter().map(|p| self.at(*p)).collect();
        Curve2::new(
            curve.degree(),
            curve.knots().to_vec(),
            points?,
            Some(curve.weights().to_vec()),
        )
        .ok()
        .map(Arc::new)
    }
}

/// Whether `pcurve` over `range` stays on `surface`'s patch, and is
/// there where `curve` is, within `within`.
fn lies(
    surface: &NurbsSurface,
    pcurve: &Curve2,
    curve: &Curve3,
    range: (f64, f64),
    within: f64,
) -> bool {
    let ((u0, u1), (v0, v1)) = surface.domain();
    let slack = Point2::new(u1 - u0, v1 - v0) * 1e-9;
    (0..=16).all(|i| {
        let t = range.0 + (range.1 - range.0) * f64::from(i) / 16.0;
        let uv = pcurve.point_at(t);
        uv.x >= u0 - slack.x
            && uv.x <= u1 + slack.x
            && uv.y >= v0 - slack.y
            && uv.y <= v1 + slack.y
            && surface.point_at(uv.x, uv.y).distance(curve.point_at(t)) <= within
    })
}

/// The curve of `rail` (over `range`) in `face`'s (u, v): the rail
/// itself, if the face is flat; else the straight line through where
/// the rail is a quarter, half and three quarters of the way along
/// (`at`: each a parameter and the place) — if that is where the rail
/// is all along: a cylinder's or a cone's rim, as the surface itself
/// runs around.
fn in_face(
    face: &Face,
    rail: &Curve3,
    range: (f64, f64),
    at: [(f64, Point2); 3],
    within: f64,
) -> Option<Arc<Curve2>> {
    let made = match Flat::of(&face.surface, within) {
        Some(flat) => flat.curve(rail)?,
        None => {
            let [(ta, a), (tm, m), (tb, b)] = at;
            let slope = (b - a) / (tb - ta);
            let line = |t: f64| m + slope * (t - tm);
            let (t0, t1) = range;
            Arc::new(Curve2::new(1, vec![t0, t0, t1, t1], vec![line(t0), line(t1)], None).ok()?)
        }
    };
    lies(&face.surface, &made, rail, range, within).then_some(made)
}

/// The step of `solid`'s face `face` along `edge`, if there is one
/// only: its loop and its place there.
fn step_of(solid: &Solid, face: usize, edge: usize) -> Option<(usize, usize)> {
    let mut found = None;
    for (l, lp) in solid.faces[face].loops.iter().enumerate() {
        for (s, step) in lp.iter().enumerate() {
            if matches!(step.walk, Walk::Edge { edge: e, .. } if e == edge) {
                if found.is_some() {
                    return None;
                }
                found = Some((l, s));
            }
        }
    }
    found
}

/// The faces of `solid` that walk `edge`.
fn beside(solid: &Solid, edge: usize) -> Vec<usize> {
    let walks = |f: &Face| -> usize {
        f.loops
            .iter()
            .flatten()
            .filter(|step| matches!(step.walk, Walk::Edge { edge: e, .. } if e == edge))
            .count()
    };
    let mut out = Vec::new();
    for (f, face) in solid.faces.iter().enumerate() {
        out.extend(std::iter::repeat_n(f, walks(face)));
    }
    out
}

/// A corner's patch as it is laid into the body.
struct Patch {
    /// The ball's radius: how far short of the corner the rounds end.
    radius: f64,
    /// Its corners, each with the vertex it is.
    feet: Vec<(Point3, usize)>,
    /// Its sides: the edge each is, and the vertices it runs between.
    sides: Vec<(usize, [usize; 2])>,
}

/// Lays the patch that closes the corner at vertex `v` of `solid`,
/// where three rounds of `found` end, into `work` — its face, its
/// three sides and its three corners — if the corner is one a patch
/// closes ([`crate::blend::patched`]), an outside one.
#[allow(clippy::too_many_arguments)]
fn patch_at(
    solid: &Solid,
    work: &mut Solid,
    id: &str,
    v: usize,
    found: &[(usize, Rolled)],
    shape: &crate::blend::Shape,
    size: f64,
    tol: f64,
) -> Option<Patch> {
    let patch = crate::blend::patched(solid, v, found, shape, size).ok()??;
    if !patch.convex {
        return None;
    }
    let mut scratch = Topo::new();
    let called = format!("{id}/v{v}/corner");
    let kept = crate::sweep::corner(
        &mut scratch,
        &called,
        patch.centre,
        patch.axes,
        &patch.section,
    )
    .ok()?;
    let kept = shapecraft_boolean::read(&scratch, kept).ok()?;
    // The patch itself: the one of its faces that comes to a point.
    let comes_to_a_point = |f: &&Face| {
        let mut steps = f.loops.iter().flatten();
        steps.any(|step| matches!(step.walk, Walk::Pole { .. }))
    };
    let mut patches = kept.faces.iter().filter(comes_to_a_point);
    let (face, None) = (patches.next()?, patches.next()) else {
        return None;
    };
    let [lp] = &face.loops[..] else {
        return None;
    };
    let mut vertex_at: Vec<(usize, usize)> = Vec::new();
    let mut vertex = |work: &mut Solid, k: usize| -> usize {
        if let Some(seen) = vertex_at.iter().find(|s| s.0 == k) {
            return seen.1;
        }
        work.vertices.push(Vertex {
            point: kept.vertices[k].point,
            tolerance: tol * 4.0,
            name: None,
        });
        vertex_at.push((k, work.vertices.len() - 1));
        work.vertices.len() - 1
    };
    let mut sides = Vec::new();
    let mut walk = Vec::new();
    for step in lp {
        let stepped = match step.walk {
            Walk::Pole { vertex: k, range } => Walk::Pole {
                vertex: vertex(work, k),
                range,
            },
            Walk::Edge { edge, reversed } => {
                let it = &kept.edges[edge];
                let ends = [vertex(work, it.ends[0]), vertex(work, it.ends[1])];
                work.edges.push(Edge {
                    curve: it.curve.clone(),
                    range: it.range,
                    ends,
                    tolerance: tol,
                    name: None,
                    also: Vec::new(),
                });
                sides.push((work.edges.len() - 1, ends));
                Walk::Edge {
                    edge: work.edges.len() - 1,
                    reversed,
                }
            }
        };
        walk.push(Use {
            walk: stepped,
            pcurve: step.pcurve.clone(),
        });
    }
    if sides.len() != 3 || vertex_at.len() != 3 {
        return None;
    }
    // Its outside is away from the ball's centre.
    let ((u0, u1), (v0, v1)) = face.surface.domain();
    let (p, du, dv) = face
        .surface
        .derivatives_at((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    let out = du.cross(dv).dot(p - patch.centre);
    if out == 0.0 || !out.is_finite() {
        return None;
    }
    work.faces.push(Face {
        surface: face.surface.clone(),
        outward: out > 0.0,
        loops: vec![walk],
        tolerance: face.tolerance,
        name: face.name.clone(),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    let feet = vertex_at
        .iter()
        .map(|&(_, at)| (work.vertices[at].point, at))
        .collect();
    Some(Patch {
        radius: patch.radius,
        feet,
        sides,
    })
}

/// `solid` with the edges of `found` blended, each with its own tool's
/// blend face (named as the tools of `id` are; the new edges have no
/// names of their own, as those a tool leaves have none), if every one
/// of them
/// can be made where it is (`size` is how large the body is); `None` if not — nothing is wrong then, the
/// blend is only not one of these.
pub(crate) fn blended(
    solid: &Solid,
    id: &str,
    found: &[(usize, Rolled)],
    shape: &crate::blend::Shape,
    margin: f64,
    size: f64,
) -> Option<Solid> {
    let tol = size * 1e-9;
    // Two of them that end together run on into each other there
    // (which is seen where they are made); three are closed by a
    // corner's patch.
    let ending = |v: usize| -> Vec<usize> {
        (0..found.len())
            .filter(|&k| found[k].1.vertices.contains(&v))
            .collect()
    };
    for (_, a) in found {
        if a.closed != (a.vertices[0] == a.vertices[1])
            || a.faces[0] == a.faces[1]
            || a.swell.iter().any(Option::is_some)
            || a.on.iter().any(Option::is_some)
        {
            return None;
        }
        for v in a.vertices {
            let n = ending(v).len();
            if n > 3 || (n >= 2 && a.closed) {
                return None;
            }
        }
    }
    let blended: Vec<usize> = found.iter().map(|f| f.0).collect();
    // Where two of them run on into each other: the rails' ends there
    // (as the first of the two has them), the section between them,
    // and the middle of it.
    let mut joints: std::collections::BTreeMap<usize, ([usize; 2], usize, [Point3; 3])> =
        std::collections::BTreeMap::new();
    let poles = solid
        .faces
        .iter()
        .flat_map(|f| f.loops.iter().flatten())
        .filter_map(|step| match step.walk {
            Walk::Pole { vertex, .. } => Some(vertex),
            Walk::Edge { .. } => None,
        })
        .collect::<Vec<usize>>();
    let mut work = solid.clone();
    // Where three of them end together: the corner's patch, laid there
    // by the first of them — its radius, its three corners (the feet
    // of the ball on the three faces) and its three sides, each with
    // the corners it runs between.
    let mut patches: std::collections::BTreeMap<usize, Patch> = std::collections::BTreeMap::new();
    let mut dead_edges = Vec::new();
    let mut dead_vertices = Vec::new();
    let mut touched: Vec<usize> = Vec::new();
    for (k, (edge, rolled)) in found.iter().enumerate() {
        shapecraft_core::cancel::checkpoint();
        let edge = *edge;
        if rolled.edge != edge || rolled.vertices.iter().any(|v| poles.contains(v)) {
            return None;
        }
        let name = format!("{id}/e{k}");
        let closed = rolled.closed;
        // At a corner of three it ends at the corner's patch, short of
        // the edge's end.
        let mut cornered = [false; 2];
        for (end, cornered) in cornered.iter_mut().enumerate() {
            let v = rolled.vertices[end];
            if ending(v).len() != 3 {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = patches.entry(v) {
                entry.insert(patch_at(solid, &mut work, id, v, found, shape, size, tol)?);
                touched.push(work.faces.len() - 1);
            }
            *cornered = true;
        }
        let at_end = |end: usize| match patches.get(&rolled.vertices[end]) {
            Some(patch) if cornered[end] => rolled.inset(end, patch.radius),
            _ if end == 0 => rolled.range.0,
            _ => rolled.range.1,
        };
        let (t0, t1) = (at_end(0), at_end(1));
        if t1 - t0 <= (rolled.range.1 - rolled.range.0) * 1e-6 {
            return None;
        }
        let tool = rolled.tool(&name, margin, &Stretch::default()).ok()?;
        let side = format!("{name}/side/e0");
        let blend = tool
            .faces
            .iter()
            .find(|f| f.name.as_deref() == Some(side.as_str()))?;
        let stations = [
            rolled.station(t0, None).ok()?,
            rolled.station(t1, None).ok()?,
        ];
        let middle = rolled.station((t0 + t1) / 2.0, None).ok()?;
        // (Rails on the edge's own faces: not rolled off onto others.)
        if !stations.iter().chain([&middle]).all(|s| s.on_own()) {
            return None;
        }
        // The tool's rails: its edges along the blend face, each with
        // its line in that face's (u, v).
        let rail = |j: usize| -> Option<(Arc<Curve3>, &Use)> {
            let called = format!("{name}/along/v{j}");
            let e = tool
                .edges
                .iter()
                .position(|e| e.name.as_deref() == Some(called.as_str()))?;
            let step = blend
                .loops
                .iter()
                .flatten()
                .find(|step| matches!(step.walk, Walk::Edge { edge, .. } if edge == e))?;
            let it = &tool.edges[e];
            let slack = (t1 - t0) * 1e-9;
            if it.range.0 > t0 + slack || it.range.1 < t1 - slack {
                return None;
            }
            let curve = Arc::new(it.curve.trimmed(t0, t1).ok()?);
            Some((curve, step))
        };
        let mut rails = [rail(0)?, rail(1)?];
        let places = [
            (t0, &stations[0]),
            ((t0 + t1) / 2.0, &middle),
            (t1, &stations[1]),
        ];
        let fit = |rails: &[(Arc<Curve3>, &Use); 2]| {
            (0..2).all(|j| {
                places
                    .iter()
                    .all(|(t, station)| rails[j].0.point_at(*t).distance(station.rails[j]) <= tol)
            })
        };
        if !fit(&rails) {
            rails.swap(0, 1);
            if !fit(&rails) {
                return None;
            }
        }
        // How far the blend reaches from its edge, at most: nothing
        // farther from the edge than that is taken by it.
        let mut reach = 0.0_f64;
        for station in [&stations[0], &middle, &stations[1]] {
            for j in 0..2 {
                reach = reach.max(station.rails[j].distance(station.at));
            }
        }
        let reach = reach * 2.0 + tol;
        let (mut lo, mut hi) = (Point3::INFINITY, Point3::NEG_INFINITY);
        for p in solid.edges[edge]
            .curve
            .trimmed(t0, t1)
            .ok()?
            .control_points()
        {
            lo = lo.min(*p - Point3::splat(reach));
            hi = hi.max(*p + Point3::splat(reach));
        }
        let near = |p: Point3| p.cmpge(lo).all() && p.cmple(hi).all();

        // The new vertices: the rails' ends (one each, of a blend that
        // closes on itself).
        let mut corners = [[0usize; 2]; 2];
        // (At an end another blend has made already: its, the other
        // way around if that one's sides are the other way around.)
        let mut flipped = [false; 2];
        let mut joined = [false; 2];
        // (The parameters a section's edge has, where it is not made
        // here.)
        let mut spans = [(0.0, 1.0); 2];
        for end in 0..2 {
            if cornered[end] {
                let patch = &patches[&rolled.vertices[end]];
                let foot = |p: Point3| {
                    let near = patch.feet.iter().find(|f| f.0.distance(p) <= tol * 16.0);
                    near.map(|f| f.1)
                };
                let rails = stations[end].rails;
                corners[end] = [foot(rails[0])?, foot(rails[1])?];
                continue;
            }
            if let Some((theirs, _, at)) = joints.get(&rolled.vertices[end]) {
                let rails = stations[end].rails;
                let same = |a: Point3, b: Point3| a.distance(b) <= tol;
                flipped[end] = if same(rails[0], at[0]) && same(rails[1], at[1]) {
                    false
                } else if same(rails[0], at[1]) && same(rails[1], at[0]) {
                    true
                } else {
                    return None;
                };
                corners[end] = if flipped[end] {
                    [theirs[1], theirs[0]]
                } else {
                    *theirs
                };
                joined[end] = true;
                continue;
            }
            if end == 1 && closed {
                corners[1] = corners[0];
                continue;
            }
            for (j, rail) in stations[end].rails.into_iter().enumerate() {
                work.vertices.push(Vertex {
                    point: rail,
                    tolerance: tol * 4.0,
                    name: None,
                });
                corners[end][j] = work.vertices.len() - 1;
            }
        }
        // The blend's section at `station`, as a curve.
        let section_at = |station: &crate::roll::Station| -> Option<Curve3> {
            let (degree, knots, points) = rolled.section(station).ok()?;
            let section = Curve3::new(
                degree,
                knots,
                points
                    .iter()
                    .map(|h| Point3::new(h[0], h[1], h[2]))
                    .collect(),
                Some(points.iter().map(|h| h[3]).collect()),
            )
            .ok()?;
            (section.point_at(0.0).distance(station.rails[0]) <= tol
                && section.point_at(1.0).distance(station.rails[1]) <= tol)
                .then_some(section)
        };
        // An edge of the body that ends at the blended one, on its side
        // `j`: from now on it ends at the rail there.
        let shorten = |work: &mut Solid, e: usize, v: usize, end: usize, j: usize| {
            let it = &work.edges[e];
            if it.ends[0] == it.ends[1] {
                return None;
            }
            let rail = stations[end].rails[j];
            let at = shapecraft_boolean::geometry::on_curve(&it.curve, it.range, rail, tol);
            let &[at] = &at[..] else {
                return None;
            };
            let slack = (it.range.1 - it.range.0) * 1e-6;
            if at <= it.range.0 + slack
                || at >= it.range.1 - slack
                || it.point(at).distance(rail) > tol
            {
                return None;
            }
            let it = &mut work.edges[e];
            if it.ends[0] == v {
                it.ends[0] = corners[end][j];
                it.range.0 = at;
            } else {
                it.ends[1] = corners[end][j];
                it.range.1 = at;
            }
            Some(())
        };
        let mut sections = [0usize; 2];
        if closed {
            // No end: but where the edge's vertex is, a seam of a face
            // beside it may end, and the blend's own seam is.
            let v = rolled.vertices[0];
            let others: Vec<usize> = (0..solid.edges.len())
                .filter(|&e| !blended.contains(&e) && work.edges[e].ends.contains(&v))
                .collect();
            let mut seamed = [false; 2];
            for e in others {
                let faces = beside(&work, e);
                let j = (0..2).find(|&j| faces == [rolled.faces[j]; 2])?;
                if std::mem::replace(&mut seamed[j], true) {
                    return None;
                }
                shorten(&mut work, e, v, 0, j)?;
            }
            work.edges.push(Edge {
                curve: Arc::new(section_at(&stations[0])?),
                range: (0.0, 1.0),
                ends: corners[0],
                tolerance: tol,
                name: None,
                also: Vec::new(),
            });
            sections = [work.edges.len() - 1; 2];
        } else {
            // Each end: the corner of three faces there, and the third
            // cut by the blend's section.
            for (end, station) in stations.iter().enumerate() {
                let v = rolled.vertices[end];
                if cornered[end] {
                    let side = patches[&v].sides.iter().find(|s| {
                        s.1 == corners[end] || s.1 == [corners[end][1], corners[end][0]]
                    })?;
                    sections[end] = side.0;
                    flipped[end] = side.1 != corners[end];
                    spans[end] = work.edges[side.0].range;
                    continue;
                }
                if joined[end] {
                    // (Made by the blend it runs on into: the same
                    // section, if it is.)
                    let (_, made, at) = &joints[&v];
                    let middle = section_at(station)?.point_at(0.5);
                    if middle.distance(at[2]) > tol {
                        return None;
                    }
                    sections[end] = *made;
                    continue;
                }
                let others: Vec<usize> = (0..solid.edges.len())
                    .filter(|&e| !blended.contains(&e) && work.edges[e].ends.contains(&v))
                    .collect();
                if let &[_, other] = &ending(v)[..] {
                    // Another blend runs on from here: on each side,
                    // in the same face — or in the next, and the edge
                    // between the two faces ends at the rail.
                    let other = if found[other].0 == edge {
                        ending(v)[0]
                    } else {
                        other
                    };
                    let theirs = found[other].1.faces;
                    let mut used = Vec::new();
                    for (j, &own) in rolled.faces.iter().enumerate() {
                        let between: Vec<usize> = others
                            .iter()
                            .copied()
                            .filter(|&e| {
                                let faces = beside(&work, e);
                                faces.len() == 2
                                    && faces.contains(&own)
                                    && faces.iter().all(|f| *f == own || theirs.contains(f))
                                    && (faces[0] != faces[1] || theirs.contains(&own))
                            })
                            .collect();
                        match between[..] {
                            [] if theirs.contains(&own) => {}
                            [e] if !used.contains(&e) => {
                                shorten(&mut work, e, v, end, j)?;
                                used.push(e);
                            }
                            _ => return None,
                        }
                    }
                    if used.len() != others.len() {
                        return None;
                    }
                    let section = section_at(station)?;
                    let middle = section.point_at(0.5);
                    work.edges.push(Edge {
                        curve: Arc::new(section),
                        range: (0.0, 1.0),
                        ends: corners[end],
                        tolerance: tol,
                        name: None,
                        also: Vec::new(),
                    });
                    sections[end] = work.edges.len() - 1;
                    let at = [station.rails[0], station.rails[1], middle];
                    joints.insert(v, (corners[end], sections[end], at));
                    continue;
                }
                let &[x, y] = &others[..] else {
                    return None;
                };
                let (bx, by) = (beside(&work, x), beside(&work, y));
                if bx.len() != 2 || by.len() != 2 {
                    return None;
                }
                let (ea, eb) = if bx.contains(&rolled.faces[0]) && by.contains(&rolled.faces[1]) {
                    (x, y)
                } else if by.contains(&rolled.faces[0]) && bx.contains(&rolled.faces[1]) {
                    (y, x)
                } else {
                    return None;
                };
                let third = |e: usize, own: usize| beside(&work, e).into_iter().find(|f| *f != own);
                let c = third(ea, rolled.faces[0])?;
                if third(eb, rolled.faces[1])? != c || rolled.faces.contains(&c) {
                    return None;
                }
                // The section, in the third face.
                let section = section_at(station)?;
                let flat = Flat::of(&work.faces[c].surface, tol)?;
                let pcurve = flat.curve(&section)?;
                if !lies(&work.faces[c].surface, &pcurve, &section, (0.0, 1.0), tol) {
                    return None;
                }
                // Nothing else of the third face's border is in the
                // corner the section cuts off: the triangle of the
                // corner and the section's ends, which holds the
                // section.
                let corner = solid.vertices[v].point;
                let [a, b] = station.rails;
                let (ea_way, eb_way) = (a - corner, b - corner);
                let normal = ea_way.cross(eb_way);
                let area = normal.length_squared();
                if area <= 0.0 {
                    return None;
                }
                // (How far inside the triangle a point of its plane is:
                // the least of its three weights there.)
                let inside = |p: Point3| {
                    let d = p - corner;
                    let wa = d.cross(eb_way).dot(normal) / area;
                    let wb = ea_way.cross(d).dot(normal) / area;
                    wa.min(wb).min(1.0 - wa - wb)
                };
                if section.control_points().iter().any(|p| inside(*p) < -1e-9) {
                    return None;
                }
                for step in work.faces[c].loops.iter().flatten() {
                    let Walk::Edge { edge: e, .. } = step.walk else {
                        return None;
                    };
                    if e == ea || e == eb {
                        continue;
                    }
                    let it = &work.edges[e];
                    for i in 0..=16 {
                        let t = it.range.0 + (it.range.1 - it.range.0) * f64::from(i) / 16.0;
                        if inside(it.point(t)) > -1e-6 {
                            return None;
                        }
                    }
                }
                // The two edges beside the blended one end at the rails.
                shorten(&mut work, ea, v, end, 0)?;
                shorten(&mut work, eb, v, end, 1)?;
                work.edges.push(Edge {
                    curve: Arc::new(section),
                    range: (0.0, 1.0),
                    ends: corners[end],
                    tolerance: tol,
                    name: None,
                    also: Vec::new(),
                });
                let made = work.edges.len() - 1;
                sections[end] = made;
                // In the third face's loop, between the two.
                let (l, s) = step_of(&work, c, ea)?;
                let (l2, s2) = step_of(&work, c, eb)?;
                let n = work.faces[c].loops[l].len();
                if l != l2 || n < 3 {
                    return None;
                }
                // (From the first rail's edge to the second's, or back.)
                let (after, reversed) = if (s + 1) % n == s2 {
                    (s, false)
                } else if (s2 + 1) % n == s {
                    (s2, true)
                } else {
                    return None;
                };
                let walk = Walk::Edge {
                    edge: made,
                    reversed,
                };
                work.faces[c].loops[l].insert(after + 1, Use { walk, pcurve });
                touched.push(c);
            }
        }
        // The two faces beside the edge: cut back to the rails.
        let mut along = [0usize; 2];
        for j in 0..2 {
            let f = rolled.faces[j];
            let (l, s) = step_of(&work, f, edge)?;
            let curve = rails[j].0.clone();
            let quarter = |i: f64| -> Option<(f64, Point2)> {
                let t = t0 + (t1 - t0) * i / 4.0;
                Some((
                    t,
                    rolled.station(t, None).ok().filter(|s| s.on_own())?.uv()[j],
                ))
            };
            let at = [quarter(1.0)?, quarter(2.0)?, quarter(3.0)?];
            let pcurve = in_face(&work.faces[f], &curve, (t0, t1), at, tol)?;
            // Whether the blend takes the point `p` of the face: by its
            // rail, which is where the blend leaves the face all along
            // (as [`Rolled::takes`] has it, without a station of its
            // own for every point asked about).
            let takes = |p: Point3| -> bool {
                let Some((t, far)) = rolled.beside(p) else {
                    return false;
                };
                if t < t0 || t > t1 {
                    return rolled.takes(j, p);
                }
                curve.point_at(t).distance(rolled.point(t)) - far > size * 1e-7
            };
            // Nothing else of the face's border is in what the blend
            // takes of it.
            for step in work.faces[f].loops.iter().flatten() {
                let e = match step.walk {
                    Walk::Edge { edge: e, .. } => e,
                    // (A point the face comes to: as any other of its
                    // border.)
                    Walk::Pole { vertex, .. } => {
                        let p = work.vertices[vertex].point;
                        if near(p) && takes(p) {
                            return None;
                        }
                        continue;
                    }
                };
                // (Another blend's edge is its own to look at.)
                if blended.contains(&e) {
                    continue;
                }
                let it = &work.edges[e];
                for i in 0..=16 {
                    let t = it.range.0 + (it.range.1 - it.range.0) * f64::from(i) / 16.0;
                    let p = it.point(t);
                    if near(p) && takes(p) {
                        return None;
                    }
                }
            }
            work.edges.push(Edge {
                curve,
                range: (t0, t1),
                ends: [corners[0][j], corners[1][j]],
                tolerance: tol,
                name: None,
                also: Vec::new(),
            });
            along[j] = work.edges.len() - 1;
            let step = &mut work.faces[f].loops[l][s];
            let Walk::Edge { reversed, .. } = step.walk else {
                return None;
            };
            *step = Use {
                walk: Walk::Edge {
                    edge: along[j],
                    reversed,
                },
                pcurve,
            };
            touched.push(f);
        }
        // The blend's face between the rails.
        let surface = blend.surface.clone();
        let uv = |j: usize, t: f64| rails[j].1.pcurve.point_at(t);
        // (A section runs from the first rail to the second: of the
        // blend that made it.)
        let across = |t: f64, flipped: bool, (a, b): (f64, f64)| -> Option<Arc<Curve2>> {
            let ends = if flipped {
                vec![uv(1, t), uv(0, t)]
            } else {
                vec![uv(0, t), uv(1, t)]
            };
            Curve2::new(1, vec![a, a, b, b], ends, None)
                .ok()
                .map(Arc::new)
        };
        let pcurves = [
            across(t0, flipped[0], spans[0])?,
            across(t1, flipped[1], spans[1])?,
        ];
        let step = |edge: usize, reversed: bool, pcurve: &Arc<Curve2>| Use {
            walk: Walk::Edge { edge, reversed },
            pcurve: pcurve.clone(),
        };
        // Around it with the surface's normal up, the face on the left.
        let ring = [uv(0, t0), uv(0, t1), uv(1, t1), uv(1, t0)];
        let turn: f64 = (0..4).map(|i| ring[i].perp_dot(ring[(i + 1) % 4])).sum();
        let (p0, p1) = (&rails[0].1.pcurve, &rails[1].1.pcurve);
        let walk = if turn > 0.0 {
            vec![
                step(along[0], false, p0),
                step(sections[1], flipped[1], &pcurves[1]),
                step(along[1], true, p1),
                step(sections[0], !flipped[0], &pcurves[0]),
            ]
        } else {
            vec![
                step(sections[0], flipped[0], &pcurves[0]),
                step(along[1], false, p1),
                step(sections[1], !flipped[1], &pcurves[1]),
                step(along[0], true, p0),
            ]
        };
        // Its outside is where the body's is at the rails.
        let at = (uv(0, (t0 + t1) / 2.0) + uv(1, (t0 + t1) / 2.0)) / 2.0;
        let (_, du, dv) = surface.derivatives_at(at.x, at.y);
        let out = (middle.outs[0] + middle.outs[1]).dot(du.cross(dv));
        if out == 0.0 || !out.is_finite() {
            return None;
        }
        work.faces.push(Face {
            surface,
            outward: out > 0.0,
            loops: vec![walk],
            tolerance: blend.tolerance,
            name: blend.name.clone(),
            also: Vec::new(),
            smooth: Vec::new(),
        });
        touched.push(work.faces.len() - 1);
        dead_edges.push(edge);
        dead_vertices.extend(rolled.vertices);
    }
    // Every step of what was touched lies in its face where its edge
    // is, and starts where the one before it ends.
    touched.sort_unstable();
    touched.dedup();
    for &f in &touched {
        let face = &work.faces[f];
        for lp in &face.loops {
            for (s, step) in lp.iter().enumerate() {
                if let Walk::Edge { edge, .. } = step.walk {
                    let it = &work.edges[edge];
                    for i in 0..=4 {
                        let t = it.range.0 + (it.range.1 - it.range.0) * f64::from(i) / 4.0;
                        let uv = step.pcurve.point_at(t);
                        let off = face.surface.point_at(uv.x, uv.y).distance(it.point(t));
                        if off.is_nan() || off > tol * 16.0 + it.tolerance + face.tolerance {
                            return None;
                        }
                    }
                }
                let next = &lp[(s + 1) % lp.len()];
                if work.ends(step).1 != work.ends(next).0 {
                    return None;
                }
            }
        }
    }
    // Without what is no more.
    let mut vertex_at = vec![usize::MAX; work.vertices.len()];
    let mut edge_at = vec![usize::MAX; work.edges.len()];
    let mut out = Solid::default();
    for (v, vertex) in work.vertices.iter().enumerate() {
        if !dead_vertices.contains(&v) {
            vertex_at[v] = out.vertices.len();
            out.vertices.push(vertex.clone());
        }
    }
    for (e, edge) in work.edges.iter().enumerate() {
        if dead_edges.contains(&e) {
            continue;
        }
        let mut edge = edge.clone();
        for end in &mut edge.ends {
            *end = vertex_at[*end];
            if *end == usize::MAX {
                return None;
            }
        }
        edge_at[e] = out.edges.len();
        out.edges.push(edge);
    }
    for face in &work.faces {
        let mut face = face.clone();
        for step in face.loops.iter_mut().flatten() {
            match &mut step.walk {
                Walk::Edge { edge, .. } => {
                    *edge = edge_at[*edge];
                    if *edge == usize::MAX {
                        return None;
                    }
                }
                Walk::Pole { vertex, .. } => {
                    *vertex = vertex_at[*vertex];
                    if *vertex == usize::MAX {
                        return None;
                    }
                }
            }
        }
        face.smooth = face
            .smooth
            .iter()
            .map(|e| edge_at[*e])
            .filter(|e| *e != usize::MAX)
            .collect();
        out.faces.push(face);
    }
    // And sound, as the kernel's own checker sees it.
    let mut scratch = Topo::new();
    shapecraft_boolean::write(&mut scratch, &out, "blended").ok()?;
    scratch.validate().ok()?;
    Some(out)
}
