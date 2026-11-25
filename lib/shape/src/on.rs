//! Where on the model an annotation points: a face's middle and the way
//! out of the body there.

use anyhow::Context;
use shapecraft_boolean::model::Walk;
use shapecraft_boolean::{Solid, read, read_sheet};
use shapecraft_core::math::Point3;
use shapecraft_core::world::topo::{BodyKind, Topo};

/// The body that has the face called `name`, read, and which of its
/// faces that is.
pub fn face(topo: &Topo, name: &str) -> anyhow::Result<(Solid, usize)> {
    let id = topo
        .referred(name)
        .with_context(|| format!("there is no face {name:?}"))?;
    let body = topo.owner(id).context("the face is of no body")?;
    let solid = match topo.body(body).map(|b| b.kind) {
        Some(BodyKind::Sheet) => read_sheet(topo, body)?,
        _ => read(topo, body)?,
    };
    let found = solid
        .faces
        .iter()
        .position(|f| f.name.as_deref() == Some(name) || f.also.iter().any(|n| n == name))
        .with_context(|| format!("there is no face {name:?}"))?;
    Ok((solid, found))
}

/// About the middle of face `face` of `solid`, on it, and the way out
/// of the body there: the middle of its border in the surface's own
/// (u, v).
pub fn middle(solid: &Solid, face: usize) -> (Point3, Point3) {
    let it = &solid.faces[face];
    let mut sum = shapecraft_core::math::Point2::ZERO;
    let mut count = 0.0;
    for step in it.loops.iter().flatten() {
        if let Walk::Edge { .. } = step.walk {
            let (a, b) = solid.span(step);
            for k in 0..4 {
                sum += step.pcurve.point_at(a + (b - a) * f64::from(k) / 4.0);
                count += 1.0;
            }
        }
    }
    let ((u0, u1), (v0, v1)) = it.surface.domain();
    let uv = if count > 0.0 {
        sum / count
    } else {
        shapecraft_core::math::Point2::new((u0 + u1) / 2.0, (v0 + v1) / 2.0)
    };
    let (p, du, dv) = it
        .surface
        .derivatives_at(uv.x.clamp(u0, u1), uv.y.clamp(v0, v1));
    let normal = du.cross(dv).normalize_or_zero();
    (p, if it.outward { normal } else { -normal })
}

/// How large `solid` is: what a note stands off its feature by is a
/// share of it.
pub fn size(solid: &Solid) -> f64 {
    let (lo, hi) = solid.extent();
    (hi - lo).length().max(1e-9)
}

/// The size a step holds between face `face` of `solid` and where that
/// is `by` off it, along `way` (a unit vector; the way out of the body
/// there, if none is given): `measure` of the step's arguments, `value`
/// as it is in the part built. Called `{step}/{what}`.
pub fn held(
    step: &str,
    what: &str,
    (solid, face): (&Solid, usize),
    (by, way): (f64, Option<Point3>),
    measure: &shapecraft_core::Measure,
    value: f64,
) -> (String, shapecraft_core::world::Annotation) {
    use shapecraft_core::world::{Annotation, Measured};
    let (on, out) = middle(solid, face);
    let measured = Measured::Length {
        from: on,
        to: on + way.unwrap_or(out) * by,
        along: None,
    };
    let held = Annotation::held(measured, value, measure, 0.0);
    (format!("{step}/{what}"), held)
}

/// An edge between two faces, at a point of it: the point, the way the
/// edge runs there, the ways out of the body on its two faces, the ways
/// into each face from the edge (along the face, square to the edge),
/// and whether the body is convex there (a corner that sticks out, not
/// one that goes in).
#[derive(Debug, Clone, Copy)]
pub struct Corner {
    /// The point of the edge.
    pub at: Point3,
    /// Along the edge (a unit vector).
    pub along: Point3,
    /// Out of the body on each face (unit vectors).
    pub out: [Point3; 2],
    /// Into each face from the edge (unit vectors).
    pub into: [Point3; 2],
    /// Whether the edge sticks out.
    pub convex: bool,
}

impl Corner {
    /// Where the middle of a ball of `radius` is that touches both
    /// faces along the edge: inside the body at an edge that sticks
    /// out, outside at one that goes in.
    pub fn ball(&self, radius: f64) -> Point3 {
        let [a, b] = self.out;
        let off = (a + b) / (1.0 + a.dot(b)).max(1e-9) * radius;
        if self.convex {
            self.at - off
        } else {
            self.at + off
        }
    }
}

/// The corner at the middle of the edge called `name` of `topo`, if it
/// is an edge between two faces of a solid body.
pub fn corner(topo: &Topo, name: &str) -> Option<Corner> {
    corners(topo, name).into_iter().next()
}

/// The corners of the edge called `name` of `topo` where a view along
/// one of the model's axes sees a size at the edge true: first its
/// middle, then each point where the edge, or the way into one of its
/// faces, is square to an axis — of a round rim, where it turns
/// across the view. Nothing if it is no edge between two faces of a
/// solid body.
pub fn corners(topo: &Topo, name: &str) -> Vec<Corner> {
    let found = || {
        let id = topo.referred(name)?;
        let body = topo.owner(id)?;
        let solid = read(topo, body).ok()?;
        let edge = solid
            .edges
            .iter()
            .position(|e| e.name.as_deref() == Some(name) || e.also.iter().any(|n| n == name))?;
        Some((solid, edge))
    };
    let Some((solid, edge)) = found() else {
        return Vec::new();
    };
    corners_of(&solid, edge)
}

/// The corners of edge `edge` of `solid`, as [`corners`] has them.
pub fn corners_of(solid: &Solid, edge: usize) -> Vec<Corner> {
    let at = |share: f64| corner_on(solid, edge, share);
    let Some(middle) = at(0.5) else {
        return Vec::new();
    };
    let mut all = vec![middle];
    // (Of a closed edge, also where it closes: what crosses there is
    // not told by samples on either side of it.)
    if let (Some(start), Some(end)) = (at(0.0), at(1.0))
        && start.at.distance(end.at) <= 1e-9 * (1.0 + start.at.length())
        && start.at.distance(middle.at) > 1e-9 * (1.0 + start.at.length())
    {
        all.push(start);
    }
    // (Each way something at the edge can be square to an axis: where
    // that changes sign along it, found by halves.)
    let ways: [fn(&Corner) -> Point3; 3] = [|c| c.along, |c| c.into[0], |c| c.into[1]];
    const SAMPLES: usize = 48;
    let samples: Vec<Option<Corner>> = (0..=SAMPLES)
        .map(|k| at(k as f64 / SAMPLES as f64))
        .collect();
    for axis in [Point3::X, Point3::Y, Point3::Z] {
        for way in ways {
            let f = |c: &Corner| way(c).dot(axis);
            let mut push = |c: Corner| {
                if all
                    .iter()
                    .all(|o| o.at.distance(c.at) > 1e-9 * (1.0 + c.at.length()))
                {
                    all.push(c);
                }
            };
            // (Each two samples it is on opposite sides of nothing at,
            // with none or only ones right at nothing between them.)
            let values: Vec<(usize, f64)> = samples
                .iter()
                .enumerate()
                .filter_map(|(k, c)| Some((k, f(c.as_ref()?))))
                .collect();
            let off: Vec<(usize, f64)> = values
                .iter()
                .copied()
                .filter(|(_, v)| v.abs() > 1e-12)
                .collect();
            for pair in off.windows(2) {
                let [(i, fp), (j, fq)] = [pair[0], pair[1]];
                // (What stays square to it all along — the way into a
                // flat face — only wavers about nothing: no crossing.)
                if fp * fq >= 0.0 || fp.abs().max(fq.abs()) < 1e-6 {
                    continue;
                }
                if j > i + 1 {
                    // (A sample right at it.)
                    if let Some(c) = samples[i + 1] {
                        push(c);
                    }
                    continue;
                }
                let (mut lo, mut hi) = (i as f64 / SAMPLES as f64, j as f64 / SAMPLES as f64);
                for _ in 0..50 {
                    let mid = (lo + hi) / 2.0;
                    match at(mid) {
                        Some(c) if f(&c) * fp > 0.0 => lo = mid,
                        Some(_) => hi = mid,
                        None => break,
                    }
                }
                if let Some(c) = at((lo + hi) / 2.0) {
                    push(c);
                }
            }
        }
    }
    all
}

/// The corner of edge `edge` of `solid` at `share` of the way along it.
fn corner_on(solid: &Solid, edge: usize, share: f64) -> Option<Corner> {
    let it = &solid.edges[edge];
    let (a, b) = it.range;
    let middle = a + (b - a) * share;
    let at = it.point(middle);
    let step = (b - a) * 1e-3;
    let along = (it.point(middle + step) - it.point(middle - step)).try_normalize()?;
    let mut out = Vec::new();
    let mut into = Vec::new();
    for face in &solid.faces {
        for used in face.loops.iter().flatten() {
            let Walk::Edge { edge: e, reversed } = used.walk else {
                continue;
            };
            if e != edge {
                continue;
            }
            // Where the edge's middle is in the face's (u, v), and the
            // face to the left of the edge as it is walked.
            // (Where on its pcurve the face has the edge's point: its
            // parameter need not run as the edge's does, nor the same
            // way — the nearest of a few, then closer.)
            let (from, to) = used.pcurve.domain();
            let on = |t: f64| {
                let uv = used.pcurve.point_at(t);
                face.surface.point_at(uv.x, uv.y)
            };
            const NEAREST_OF: usize = 32;
            let mut t = (0..=NEAREST_OF)
                .map(|k| from + (to - from) * k as f64 / NEAREST_OF as f64)
                .min_by(|x, y| on(*x).distance(at).total_cmp(&on(*y).distance(at)))?;
            let h = (to - from) * 1e-6;
            for _ in 0..12 {
                let slope = (on(t + h) - on(t - h)) / (2.0 * h);
                let size = slope.length_squared();
                if size <= 0.0 {
                    break;
                }
                let lo = from.min(to);
                let hi = from.max(to);
                t = (t - (on(t) - at).dot(slope) / size).clamp(lo, hi);
            }
            let uv = used.pcurve.point_at(t);
            let run = used.pcurve.point_at(t + (to - from) * 1e-3) - uv;
            let walked = if reversed { -run } else { run };
            let left = shapecraft_core::math::Point2::new(-walked.y, walked.x).try_normalize()?;
            let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
            let normal = du.cross(dv).try_normalize()?;
            let normal = if face.outward { normal } else { -normal };
            let inward = (du * left.x + dv * left.y).try_normalize()?;
            let inward = (inward - along * inward.dot(along)).try_normalize()?;
            out.push(normal);
            into.push(inward);
        }
    }
    let ([a, b], [ia, ib]) = (
        <[Point3; 2]>::try_from(out).ok()?,
        <[Point3; 2]>::try_from(into).ok()?,
    );
    Some(Corner {
        at,
        along,
        out: [a, b],
        into: [ia, ib],
        // (Where it sticks out, each face runs back from the edge
        // against the other's way out.)
        convex: ia.dot(b) < 0.0,
    })
}
