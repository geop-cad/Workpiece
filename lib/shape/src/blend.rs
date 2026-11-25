//! Blends: rounding and chamfering the edges of a body.
//!
//! An edge is blended by a tool, a solid that is taken from the body
//! along a convex edge and added to it along a concave one, with the
//! booleans. The tool is the blend's section, swept along the edge:
//!
//! * The **section** is worked out in the plane square to the edge, at
//!   as many places along it as it takes, whatever the edge and its two
//!   faces are ([`crate::roll`]): the blend runs from a rail on one face
//!   to a rail on the other (an arc tangent to both, a conic, a curve
//!   that meets them with their curvature, or the straight line of a
//!   chamfer). The tool's section is what lies between the blend and the
//!   corner — and, so that none of its sides lies along a face of the
//!   body, a margin beyond the two faces.
//! * It is **swept** through those sections. Where an edge ends on a
//!   corner the body turns away from, the tool runs on past it; the
//!   blends of the edges that meet there then cut each other off (a
//!   mitered corner), and a corner tool closes the corner: a patch
//!   between three rounds that meet squarely, a flat for chamfers.

use std::collections::BTreeSet;

use anyhow::{Context, ensure};
use workpiece_boolean::model::Walk;
use workpiece_boolean::{Op, Solid, combine, measure, read};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{BodyId, Curve2, Topo};
use workpiece_core::world::{DatumPlane, Profile, ProfileEdge, ProfileRegion, ProfileVertex};

use crate::roll::{EndCut, Rolled, Station, Stretch};
use crate::sweep::{self, Make, Turn};

/// How a round's section runs from one face to the other.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum Section {
    /// An arc of a circle, tangent to both faces.
    #[default]
    Circular,
    /// A conic tangent to both faces where the circle would be: flatter
    /// or fuller than it by its `rho` (a half is a parabola).
    Conic,
    /// A curve that leaves each face with the face's own curvature, and
    /// takes off what the circle would.
    Curvature,
    /// As [`Section::Curvature`], but as full as bends by the radius
    /// `rho` at its middle.
    Centred,
}

/// What a round's size is the size of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum By {
    /// The radius of the ball rolled along the edge.
    #[default]
    Radius,
    /// The width of the round: from one rail straight to the other.
    Width,
    /// The distance from the edge to each rail, across the faces.
    Distance,
}

/// The shape of a blend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// A round.
    Fillet {
        /// What `size` is.
        by: By,
        /// The size.
        size: f64,
        /// The size at the edge's end, where it is not the same all
        /// along: it grows evenly from `size` at the edge's start.
        /// Only along straight edges between flat faces.
        until: Option<f64>,
        /// How the section runs.
        section: Section,
        /// A conic's fullness, between 0 and 1.
        rho: f64,
    },
    /// A flat cut: the distance to its rail across the first face, and
    /// across the second.
    Chamfer {
        /// Across the first face.
        first: f64,
        /// Across the second.
        second: f64,
    },
}

/// A square block: from the plane through `origin` across `normal`, a
/// distance `depth` along the normal, and `reach` to every side.
fn block(
    id: &str,
    origin: Point3,
    normal: Point3,
    reach: f64,
    depth: f64,
) -> anyhow::Result<Solid> {
    let normal = normal.try_normalize().context("a plane without a normal")?;
    let x = normal.any_orthonormal_vector();
    let plane = DatumPlane::new(origin, normal, x)?;
    let mut profile = Profile::new(plane);
    let corners = [
        Point2::new(-reach, -reach),
        Point2::new(reach, -reach),
        Point2::new(reach, reach),
        Point2::new(-reach, reach),
    ];
    for (k, point) in corners.iter().enumerate() {
        profile.vertices.push(ProfileVertex {
            name: format!("v{k}"),
            point: *point,
            tolerance: 1e-9,
        });
        profile.edges.push(ProfileEdge {
            name: format!("e{k}"),
            entity: format!("e{k}"),
            span: (0.0, 1.0),
            curve: Curve2::line(*point, corners[(k + 1) % 4])?,
            t0: 0.0,
            t1: 1.0,
            start: k,
            end: (k + 1) % 4,
            tolerance: 1e-9,
        });
    }
    let walk: Vec<(usize, bool)> = (0..4).map(|k| (k, false)).collect();
    profile.regions.push(ProfileRegion {
        outer: walk.clone(),
        holes: Vec::new(),
        area: 4.0 * reach * reach,
    });
    let mut scratch = Topo::new();
    let body = sweep::extrude(
        &mut scratch,
        id,
        &profile,
        &[walk],
        true,
        (0.0, depth),
        Make::Solid,
    )?;
    Ok(read(&scratch, body)?)
}

/// The block from `corner` as far as `sides` along each of `axes` (unit,
/// square to each other).
pub fn cuboid(
    id: &str,
    corner: Point3,
    axes: [Point3; 3],
    sides: [f64; 3],
) -> anyhow::Result<Solid> {
    let plane = DatumPlane::new(corner, axes[2], axes[0])?;
    // Which way the plane's own y runs against the second axis.
    let way = plane.y_dir().dot(axes[1]).signum();
    let mut corners = [
        Point2::ZERO,
        Point2::new(sides[0], 0.0),
        Point2::new(sides[0], sides[1] * way),
        Point2::new(0.0, sides[1] * way),
    ];
    if way < 0.0 {
        corners.reverse();
    }
    let mut profile = Profile::new(plane);
    for (k, point) in corners.iter().enumerate() {
        profile.vertices.push(ProfileVertex {
            name: format!("v{k}"),
            point: *point,
            tolerance: 1e-7,
        });
        profile.edges.push(ProfileEdge {
            name: format!("e{k}"),
            entity: format!("e{k}"),
            span: (0.0, 1.0),
            curve: Curve2::line(*point, corners[(k + 1) % 4])?,
            t0: 0.0,
            t1: 1.0,
            start: k,
            end: (k + 1) % 4,
            tolerance: 1e-7,
        });
    }
    let walk: Vec<(usize, bool)> = (0..4).map(|k| (k, false)).collect();
    profile.regions.push(ProfileRegion {
        outer: walk.clone(),
        holes: Vec::new(),
        area: sides[0] * sides[1],
    });
    let mut scratch = Topo::new();
    let body = sweep::extrude(
        &mut scratch,
        id,
        &profile,
        &[walk],
        true,
        (0.0, sides[2]),
        Make::Solid,
    )?;
    Ok(read(&scratch, body)?)
}

/// What lies past the face `face` of `solid` near the point `at` of it,
/// on the side away from `keep` (the way, at `at`, out of the face that
/// is kept): the block a piece of its surface (run on past its rims)
/// sweeps as it is moved that way by `far`, the piece around `at` as
/// far as `far` and as long as it faces that way — what a blend's tool
/// is cut back to where it ends on a curved face.
fn past_face(
    solid: &Solid,
    face: usize,
    at: Point3,
    keep: Point3,
    far: f64,
    id: &str,
) -> anyhow::Result<Solid> {
    use std::sync::Arc;
    use workpiece_boolean::model::{Edge, Face, Use, Vertex};
    use workpiece_core::world::topo::Curve3;
    let f = &solid.faces[face];
    // (Run on past each rim by half of itself, where it goes on as
    // itself so far: not where it closes on itself or comes to a point.)
    let surface = {
        let mut out = (*f.surface).clone();
        let ((u0, u1), (v0, v1)) = f.surface.domain();
        let near = far * 1e-9;
        for v in [false, true] {
            let (a, b) = if v { (v0, v1) } else { (u0, u1) };
            let at = |t: f64, s: f64| {
                if v {
                    f.surface.point_at(s, t)
                } else {
                    f.surface.point_at(t, s)
                }
            };
            let (c0, c1) = if v { (u0, u1) } else { (v0, v1) };
            let across = |t: f64| (0..=8).map(move |k| at(t, c0 + (c1 - c0) * f64::from(k) / 8.0));
            let closes = across(a).zip(across(b)).all(|(p, q)| p.distance(q) <= near);
            if closes {
                continue;
            }
            for low in [true, false] {
                let t = if low { a } else { b };
                let first = at(t, c0);
                if across(t).all(|p| p.distance(first) <= near) {
                    continue;
                }
                for share in [0.5, 0.25, 0.1] {
                    let ((p0, p1), (q0, q1)) = out.domain();
                    let to = match (v, low) {
                        (false, true) => p0 - (b - a) * share,
                        (false, false) => p1 + (b - a) * share,
                        (true, true) => q0 - (b - a) * share,
                        (true, false) => q1 + (b - a) * share,
                    };
                    if let Ok(longer) = crate::thick::run_on_surface(&out, v, low, to)
                        && longer.weights().iter().all(|w| *w > 0.0)
                    {
                        out = longer;
                        break;
                    }
                }
            }
        }
        out
    };
    let away = -keep.normalize_or_zero();
    ensure!(away != Point3::ZERO, "a face is cut back to along a way");
    // Where `at` is on it.
    let start = (0..solid.faces[face].loops.len())
        .flat_map(|l| solid.faces[face].loops[l].iter())
        .map(|step| step.pcurve.point_at(solid.span(step).0))
        .min_by(|a, b| {
            let d = |uv: &Point2| f.surface.point_at(uv.x, uv.y).distance(at);
            d(a).total_cmp(&d(b))
        })
        .context("a face has an outline")?;
    let uv = workpiece_boolean::geometry::foot(&surface, at, start).0;
    let ((u0, u1), (v0, v1)) = surface.domain();
    let facing = |p: Point2| crate::thick::normal(&surface, p).dot(away);
    let sense = facing(uv).signum();
    ensure!(
        facing(uv).abs() > 0.5,
        "the face is not across the way the blend is cut back"
    );
    let holds = |p: Point2| {
        p.x >= u0
            && p.x <= u1
            && p.y >= v0
            && p.y <= v1
            && facing(p) * sense >= 0.3
            && surface.point_at(p.x, p.y).distance(at) <= far
    };
    // How far it goes each way from `at`, in (u, v), staying so.
    let reach = |way: Point2, span: f64| -> f64 {
        const N: usize = 96;
        let mut got = 0.0;
        for k in 1..=N {
            let by = span * k as f64 / N as f64;
            if !holds(uv + way * by) {
                break;
            }
            got = by;
        }
        got
    };
    let (su, sv) = (u1 - u0, v1 - v0);
    let mut sides = [
        reach(Point2::new(-1.0, 0.0), su),
        reach(Point2::new(1.0, 0.0), su),
        reach(Point2::new(0.0, -1.0), sv),
        reach(Point2::new(0.0, 1.0), sv),
    ];
    ensure!(
        sides.iter().all(|s| *s > 0.0),
        "the face cannot be run on past where the blend ends on it"
    );
    // (The whole piece faces that way.)
    for _ in 0..12 {
        let all = (0..=8).all(|i| {
            (0..=8).all(|j| {
                let (a, b) = (f64::from(i) / 8.0, f64::from(j) / 8.0);
                let p = Point2::new(
                    uv.x - sides[0] + (sides[0] + sides[1]) * a,
                    uv.y - sides[2] + (sides[2] + sides[3]) * b,
                );
                facing(p) * sense >= 0.2
            })
        });
        if all {
            break;
        }
        sides = sides.map(|s| s * 0.75);
    }
    let within = far * 1e-10;
    let piece = crate::thick::stretch(&surface, false, (uv.x - sides[0], uv.x + sides[1]), within)
        .and_then(|s| crate::thick::stretch(&s, true, (uv.y - sides[2], uv.y + sides[3]), within))
        .context("the face could not be run on past where the blend ends on it")?;
    let ((a0, a1), (b0, b1)) = piece.domain();
    let (nu, nv) = piece.counts();
    let (ku, kv) = piece.knots();
    // Its sides: along u at the first and last v, along v at the first
    // and last u.
    let row = |v: bool, last: bool| -> anyhow::Result<Curve3> {
        let (points, weights): (Vec<Point3>, Vec<f64>) = if v {
            let j = if last { nv - 1 } else { 0 };
            (0..nu)
                .map(|i| (piece.control_point(i, j), piece.weights()[i * nv + j]))
                .unzip()
        } else {
            let i = if last { nu - 1 } else { 0 };
            (0..nv)
                .map(|j| (piece.control_point(i, j), piece.weights()[i * nv + j]))
                .unzip()
        };
        let (degree, knots) = if v {
            (piece.degrees().0, ku.to_vec())
        } else {
            (piece.degrees().1, kv.to_vec())
        };
        Ok(Curve3::new(degree, knots, points, Some(weights))?)
    };
    let tolerance = within * 10.0;
    let corners = [(a0, b0), (a1, b0), (a1, b1), (a0, b1)];
    let mut sheet = Solid::default();
    for (u, v) in corners {
        sheet.vertices.push(Vertex {
            point: piece.point_at(u, v),
            tolerance,
            name: None,
        });
    }
    // Around it, the piece on the left: the first v, the last u, the
    // last v back, the first u back.
    let sides_of = [
        (row(true, false)?, [0, 1], (a0, a1)),
        (row(false, true)?, [1, 2], (b0, b1)),
        (row(true, true)?, [3, 2], (a0, a1)),
        (row(false, false)?, [0, 3], (b0, b1)),
    ];
    let mut walk = Vec::new();
    for (k, (curve, ends, range)) in sides_of.into_iter().enumerate() {
        sheet.edges.push(Edge {
            curve: Arc::new(curve),
            range,
            ends,
            tolerance,
            name: Some(format!("{id}/rim{k}")),
            also: Vec::new(),
        });
        let (from, to) = match k {
            0 => (Point2::new(a0, b0), Point2::new(a1, b0)),
            1 => (Point2::new(a1, b0), Point2::new(a1, b1)),
            2 => (Point2::new(a0, b1), Point2::new(a1, b1)),
            _ => (Point2::new(a0, b0), Point2::new(a0, b1)),
        };
        walk.push(Use {
            walk: Walk::Edge {
                edge: k,
                reversed: k >= 2,
            },
            pcurve: Arc::new(Curve2::new(
                1,
                vec![range.0, range.0, range.1, range.1],
                vec![from, to],
                None,
            )?),
        });
    }
    sheet.faces.push(Face {
        surface: Arc::new(piece),
        outward: true,
        loops: vec![walk],
        tolerance,
        name: Some(format!("{id}/near")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    let (block, _) = crate::thick::slab_along(&sheet, 0, away * far, id)?;
    Ok(block)
}

/// `solid` with the edge that passes through `p` (within `within`, not
/// at either of its ends) cut in two there: a vertex at `p`.
fn split_at(solid: &mut Solid, p: Point3, within: f64) {
    use workpiece_boolean::model::{Edge, Use, Vertex};
    let found = (0..solid.edges.len()).find_map(|e| {
        let edge = &solid.edges[e];
        if edge
            .ends
            .iter()
            .any(|&v| solid.vertices[v].point.distance(p) <= within)
        {
            return None;
        }
        let t = crate::roll::curve_foot(&edge.curve, edge.range, p);
        (edge.point(t).distance(p) <= within).then_some((e, t))
    });
    let Some((e, t)) = found else {
        return;
    };
    let v = solid.vertices.len();
    solid.vertices.push(Vertex {
        point: solid.edges[e].point(t),
        tolerance: solid.edges[e].tolerance,
        name: None,
    });
    let (a, b) = solid.edges[e].range;
    let [v0, v1] = solid.edges[e].ends;
    let second = Edge {
        range: (t, b),
        ends: [v, v1],
        ..solid.edges[e].clone()
    };
    solid.edges[e].range = (a, t);
    solid.edges[e].ends = [v0, v];
    let n = solid.edges.len();
    solid.edges.push(second);
    for face in &mut solid.faces {
        if face.smooth.contains(&e) {
            face.smooth.push(n);
        }
        for walk in &mut face.loops {
            let mut out: Vec<Use> = Vec::with_capacity(walk.len() + 1);
            for step in walk.drain(..) {
                match step.walk {
                    Walk::Edge { edge, reversed } if edge == e => {
                        let other = Use {
                            walk: Walk::Edge { edge: n, reversed },
                            pcurve: step.pcurve.clone(),
                        };
                        if reversed {
                            out.push(other);
                            out.push(step);
                        } else {
                            out.push(step);
                            out.push(other);
                        }
                    }
                    _ => out.push(step),
                }
            }
            *walk = out;
        }
    }
}

/// A ball of `radius` around `centre`: half a disk turned around.
pub fn ball(id: &str, centre: Point3, radius: f64) -> anyhow::Result<Solid> {
    let plane = DatumPlane::new(centre, Point3::Y, Point3::X)?;
    // In the plane: x across, y along the axis the half disk turns
    // around (the plane's y).
    let mut profile = Profile::new(plane);
    let (south, north) = (Point2::new(0.0, -radius), Point2::new(0.0, radius));
    for (k, point) in [south, north].iter().enumerate() {
        profile.vertices.push(ProfileVertex {
            name: format!("v{k}"),
            point: *point,
            tolerance: 1e-9,
        });
    }
    let (points, knots) = crate::arcs::arc_form(std::f64::consts::PI);
    let (controls, weights): (Vec<Point2>, Vec<f64>) = points
        .into_iter()
        // From the south, around by the east.
        .map(|(p, w)| (Point2::new(p.y, -p.x) * radius, w))
        .unzip();
    let arc = Curve2::new(2, knots, controls, Some(weights))?;
    for (k, curve) in [arc, Curve2::line(north, south)?].into_iter().enumerate() {
        let (t0, t1) = curve.domain();
        profile.edges.push(ProfileEdge {
            name: format!("e{k}"),
            entity: format!("e{k}"),
            span: (0.0, 1.0),
            curve,
            t0,
            t1,
            start: k,
            end: 1 - k,
            tolerance: 1e-9,
        });
    }
    let walk = vec![(0, false), (1, false)];
    profile.regions.push(ProfileRegion {
        outer: walk.clone(),
        holes: Vec::new(),
        area: std::f64::consts::PI * radius * radius / 2.0,
    });
    let mut scratch = Topo::new();
    let turn = Turn {
        origin: centre,
        axis: plane.y_dir(),
    };
    let range = (0.0, std::f64::consts::TAU);
    let body = sweep::revolve(
        &mut scratch,
        id,
        &profile,
        &[walk],
        true,
        turn,
        range,
        Make::Solid,
    )?;
    Ok(read(&scratch, body)?)
}

/// Every point within `radius` of the circle of radius `ring` around
/// `centre` across `axis`, where that is more than the circle's own
/// radius: the tube around the circle closes over its middle, and is
/// the arc of a disk about a point of the circle — from the axis, around
/// its outside, back to the axis — turned around.
pub fn apple(
    id: &str,
    centre: Point3,
    axis: Point3,
    ring: f64,
    radius: f64,
) -> anyhow::Result<Solid> {
    ensure!(
        ring >= 0.0 && ring < radius,
        "the tube is not wider than its circle"
    );
    let axis = axis.try_normalize().context("the circle has an axis")?;
    let across = axis.any_orthonormal_vector();
    let plane = DatumPlane::new(centre, across.cross(axis), across)?;
    let mut profile = Profile::new(plane);
    let high = (radius * radius - ring * ring).sqrt();
    let (south, north) = (Point2::new(0.0, -high), Point2::new(0.0, high));
    for (k, point) in [south, north].iter().enumerate() {
        profile.vertices.push(ProfileVertex {
            name: format!("v{k}"),
            point: *point,
            tolerance: 1e-9,
        });
    }
    // From the south, around by the east, about (ring, 0).
    let turn = high.atan2(-ring);
    let (points, knots) = crate::arcs::arc_form(2.0 * turn);
    let (sin, cos) = (-turn).sin_cos();
    let (mut controls, weights): (Vec<Point2>, Vec<f64>) = points
        .into_iter()
        .map(|(p, w)| {
            let turned = Point2::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
            (Point2::new(ring, 0.0) + turned * radius, w)
        })
        .unzip();
    let last = controls.len() - 1;
    (controls[0], controls[last]) = (south, north);
    let arc = Curve2::new(2, knots, controls, Some(weights))?;
    for (k, curve) in [arc, Curve2::line(north, south)?].into_iter().enumerate() {
        let (t0, t1) = curve.domain();
        profile.edges.push(ProfileEdge {
            name: format!("e{k}"),
            entity: format!("e{k}"),
            span: (0.0, 1.0),
            curve,
            t0,
            t1,
            start: k,
            end: 1 - k,
            tolerance: 1e-9,
        });
    }
    let walk = vec![(0, false), (1, false)];
    profile.regions.push(ProfileRegion {
        outer: walk.clone(),
        holes: Vec::new(),
        area: radius * radius * turn + ring * high,
    });
    let mut scratch = Topo::new();
    let turn = Turn {
        origin: centre,
        axis: plane.y_dir(),
    };
    let range = (0.0, std::f64::consts::TAU);
    let body = sweep::revolve(
        &mut scratch,
        id,
        &profile,
        &[walk],
        true,
        turn,
        range,
        Make::Solid,
    )?;
    Ok(read(&scratch, body)?)
}

/// Whether the edge `edge` of `solid` is one to blend: between two
/// faces that meet at an angle.
fn sharp(solid: &Solid, edge: usize) -> bool {
    let e = &solid.edges[edge];
    let t = e.middle();
    let (_, d) = e.curve.derivative_at(t);
    let mut normals = Vec::new();
    for face in &solid.faces {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge: walked, .. } = step.walk
                && walked == edge
            {
                let uv = step.pcurve.point_at(t);
                let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
                normals
                    .push(du.cross(dv).normalize_or_zero() * if face.outward { 1.0 } else { -1.0 });
            }
        }
    }
    normals.len() == 2 && d.length() > 0.0 && normals[0].cross(normals[1]).length() > 1e-4
}

/// Blends what is `picked` of the solid bodies of `topo` to `shape`:
/// edges by their names, every edge of a face or of a body by its name.
/// With `corners`, a corner where every edge is blended is closed (by a
/// ball between rounds, a flat between chamfers); without, the blends
/// run on until they cut each other off. What was blended: each body
/// as it was, and its edges blended (see [`blended`]).
pub fn blend(
    topo: &Topo,
    out: &mut crate::bodies::Out<'_>,
    id: &str,
    picked: &[String],
    shape: &Shape,
    corners: bool,
) -> anyhow::Result<Vec<(Solid, Vec<usize>)>> {
    let mut was = Vec::new();
    for (body, name, solid, edges) in blended(topo, picked)? {
        let made = blend_solid(&solid, id, &edges, shape, corners)?;
        out.remove(topo, body);
        out.write(&made, &name);
        was.push((solid, edges));
    }
    Ok(was)
}

/// A body to blend: it, its name, it read, and its edges blended.
type Blended = (BodyId, String, Solid, Vec<usize>);

/// The edges what is `picked` of the solid bodies of `topo` stands for
/// (see [`blend`]): of each body that has some, the body, its name, it
/// read, and those edges of it, in order.
fn blended(topo: &Topo, picked: &[String]) -> anyhow::Result<Vec<Blended>> {
    ensure!(!picked.is_empty(), "pick the edges to blend");
    let mut found: BTreeSet<&str> = BTreeSet::new();
    let mut all = Vec::new();
    for (body, name) in crate::bodies::solids_with(topo, picked) {
        let solid = read(topo, body)?;
        let mut edges: BTreeSet<usize> = BTreeSet::new();
        let is = |name: &Option<String>| {
            picked
                .iter()
                .find(|p| name.as_deref() == Some(p.as_str()))
                .map(String::as_str)
        };
        for (e, edge) in solid.edges.iter().enumerate() {
            // (By its name, or one it is also known by.)
            let known = || edge.also.iter().find_map(|n| is(&Some(n.clone())));
            if let Some(p) = is(&edge.name).or_else(known) {
                found.insert(p);
                edges.insert(e);
            }
        }
        // Of a face or the body: its edges that are sharp.
        let whole = is(&Some(name.clone()));
        found.extend(whole);
        for face in &solid.faces {
            let named =
                is(&face.name).or_else(|| face.also.iter().find_map(|n| is(&Some(n.clone()))));
            found.extend(named);
            if named.is_none() && whole.is_none() {
                continue;
            }
            for step in face.loops.iter().flatten() {
                if let Walk::Edge { edge, .. } = step.walk
                    && sharp(&solid, edge)
                {
                    edges.insert(edge);
                }
            }
        }
        if !edges.is_empty() {
            all.push((body, name, solid, edges.into_iter().collect()));
        }
    }
    if let Some(missing) = picked.iter().find(|p| !found.contains(p.as_str())) {
        anyhow::bail!("there is no edge, face or body {missing:?} to blend");
    }
    Ok(all)
}

/// The corner between three rounds, where it is closed by a patch.
pub(crate) struct Patched {
    /// Where the three rounds' axes meet.
    pub(crate) centre: Point3,
    /// How far that is from each of the three faces.
    pub(crate) radius: f64,
    /// From the centre towards each face, square to it.
    pub(crate) axes: [Point3; 3],
    /// A round's section as it ends at the corner: (distance from the
    /// third axis, height along it), from that axis's foot outwards.
    pub(crate) section: Curve2,
    pub(crate) convex: bool,
    /// The three edges (as they are numbered in the solid), in the
    /// order of the axes they run against, and how long each is.
    pub(crate) edges: [usize; 3],
    pub(crate) lengths: [f64; 3],
    /// Of an inside corner: how much of each round the corner brings
    /// along with it.
    pub(crate) stubs: [f64; 3],
}

/// The patch that closes the corner at vertex `v` of `solid`, if it is
/// one a patch closes: three rounds of one size that meet squarely, all
/// of outside edges or all of inside ones.
pub(crate) fn patched(
    solid: &Solid,
    v: usize,
    found: &[(usize, Rolled)],
    shape: &Shape,
    size: f64,
) -> anyhow::Result<Option<Patched>> {
    let vertex = &solid.vertices[v];
    let at: Vec<usize> = (0..solid.edges.len())
        .filter(|&e| solid.edges[e].ends.contains(&v))
        .collect();
    let blended: Vec<&(usize, Rolled)> = at
        .iter()
        .filter_map(|e| found.iter().find(|f| f.0 == *e))
        .collect();
    let all = |convex: bool| blended.iter().all(|b| b.1.convex == convex);
    if at.len() != 3 || blended.len() != 3 || !(all(true) || all(false)) {
        return Ok(None);
    }
    let ends: Vec<usize> = blended
        .iter()
        .map(|b| usize::from(b.1.vertices[0] != v))
        .collect();
    let away: Vec<Point3> = (0..3).map(|k| blended[k].1.away(ends[k])).collect();
    let straight = blended.iter().all(|b| b.1.straight);
    let square = (0..3).all(|i| (i + 1..3).all(|j| away[i].dot(away[j]).abs() <= 1e-9));
    let whole = matches!(shape, Shape::Fillet { until: None, .. });
    if !(whole && straight && square) {
        return Ok(None);
    }
    let mut stations = Vec::new();
    for k in 0..3 {
        stations.push(blended[k].1.end(ends[k])?);
    }
    let centres: Vec<Point3> = stations.iter().filter_map(|s| s.centre).collect();
    if centres.len() != 3 {
        return Ok(None);
    }
    let Some(centre) = meeting(centres[0], away[0], centres[1], away[1]) else {
        return Ok(None);
    };
    let reach: Vec<f64> = away
        .iter()
        .map(|a| (centre - vertex.point).dot(*a))
        .collect();
    let r = reach[0];
    let off = |k: usize| (centre - centres[k]).reject_from(away[k]).length();
    // (And every edge has to be long enough to end at the patch.)
    let long = blended.iter().all(|b| b.1.length > r * 1.2);
    if r <= 0.0
        || !long
        || reach.iter().any(|x| (x - r).abs() > 1e-7 * size)
        || (0..3).any(|k| off(k) > 1e-7 * size)
    {
        return Ok(None);
    }
    let axes = [-away[0], -away[1], -away[2]];
    // The first round's section, where it ends at the corner: from the
    // third axis's foot to the second's.
    let (degree, mut knots, points) = blended[0].1.section(&stations[0])?;
    let mut section: Vec<(Point2, f64)> = points
        .iter()
        .map(|h| {
            let p = Point3::new(h[0], h[1], h[2]) + away[0] * r - centre;
            (Point2::new(p.dot(axes[1]), p.dot(axes[2])), h[3])
        })
        .collect();
    if section[0].0.y.abs() < r * 0.5 {
        section.reverse();
        let (lo, hi) = (knots[0], knots[knots.len() - 1]);
        knots = knots.iter().rev().map(|k| lo + hi - k).collect();
    }
    let section = Curve2::new(
        degree,
        knots,
        section.iter().map(|s| s.0).collect(),
        Some(section.iter().map(|s| s.1).collect()),
    )?;
    let lengths = [0, 1, 2].map(|k| blended[k].1.length);
    Ok(Some(Patched {
        centre,
        radius: r,
        axes,
        section,
        convex: all(true),
        edges: [blended[0].0, blended[1].0, blended[2].0],
        lengths,
        stubs: lengths,
    }))
}

/// What a ball takes as it turns about the end of an inside edge that
/// is not rounded, between two rounds of outside edges that end there:
/// `a` and `b`, each a round and which end of it is at the vertex `v`.
/// Both rounds lie in one face; where their other faces meet in the
/// inside edge, the ball rests against that edge and turns about it
/// from the one round to the other — which is a turn in space where
/// the edge is straight there and the common face is the same all
/// around it. `None` where it is not: the two rounds then run on and
/// cut each other off.
fn turned(
    solid: &Solid,
    id: &str,
    v: usize,
    a: (&Rolled, usize),
    b: (&Rolled, usize),
    margin: f64,
    size: f64,
) -> anyhow::Result<Option<Solid>> {
    let within = size * 1e-9;
    // The face both lie in, and each one's other face.
    let Some((ka, kb)) = (0..2)
        .flat_map(|i| (0..2).map(move |j| (i, j)))
        .find(|&(i, j)| a.0.faces[i] == b.0.faces[j])
    else {
        return Ok(None);
    };
    let (common, fa, fb) = (a.0.faces[ka], a.0.faces[1 - ka], b.0.faces[1 - kb]);
    if fa == fb {
        return Ok(None);
    }
    let (Ok(sa), Ok(sb)) = (a.0.end(a.1), b.0.end(b.1)) else {
        return Ok(None);
    };
    let (Some(ca), Some(cb)) = (sa.centre, sb.centre) else {
        return Ok(None);
    };
    // Where the ball rests against the inside edge: where both rounds
    // leave their other faces.
    let rest = sa.rails[1 - ka];
    let corner = solid.vertices[v].point;
    let Some(axis) = (corner - rest).try_normalize() else {
        return Ok(None);
    };
    if sb.rails[1 - kb].distance(rest) > within {
        return Ok(None);
    }
    // The inside edge, straight from the corner to there.
    let straight = (0..solid.edges.len()).any(|e| {
        let edge = &solid.edges[e];
        if !edge.ends.contains(&v) {
            return false;
        }
        let between: Vec<usize> = (0..solid.faces.len())
            .filter(|&f| {
                solid.faces[f]
                    .loops
                    .iter()
                    .flatten()
                    .any(|step| matches!(step.walk, Walk::Edge { edge, .. } if edge == e))
            })
            .collect();
        if !(between.contains(&fa) && between.contains(&fb)) {
            return false;
        }
        let reach = corner.distance(rest);
        let mut far = 0.0_f64;
        for k in 0..=32 {
            let p = edge.point(edge.range.0 + (edge.range.1 - edge.range.0) * f64::from(k) / 32.0);
            far = far.max(p.distance(corner));
            if p.distance(corner) <= reach
                && (p - corner).reject_from_normalized(axis).length() > within
            {
                return false;
            }
        }
        far >= reach
    });
    if !straight {
        return Ok(None);
    }
    let turn = Turn {
        origin: corner,
        axis,
    };
    // The one round's end, turned, is the other's.
    let out = |p: Point3| p - turn.foot(p);
    let up = |p: Point3| (p - corner).dot(axis);
    let same = |p: Point3, q: Point3| {
        (out(p).length() - out(q).length()).abs() <= within && (up(p) - up(q)).abs() <= within
    };
    let (top_a, top_b) = (sa.rails[ka], sb.rails[kb]);
    if !same(ca, cb) || !same(top_a, top_b) || (sa.radius - sb.radius).abs() > within {
        return Ok(None);
    }
    let (Some(from), Some(to)) = (out(ca).try_normalize(), out(cb).try_normalize()) else {
        return Ok(None);
    };
    let angle = from.angle_between(to);
    if !(1e-6..std::f64::consts::PI - 1e-6).contains(&angle) {
        return Ok(None);
    }
    let axis = if axis.dot(from.cross(to)) < 0.0 {
        -axis
    } else {
        axis
    };
    let turn = Turn {
        origin: corner,
        axis,
    };
    // The common face all around: the ball touches it wherever it is
    // on its way.
    let around = |p: Point3, share: f64| {
        let foot = turn.foot(p);
        let across = p - foot;
        let by = angle * share;
        foot + across * by.cos() + axis.cross(across) * by.sin()
    };
    if (1..4).any(|k| {
        !workpiece_boolean::lies_on(
            solid,
            common,
            around(top_a, f64::from(k) / 4.0),
            size * 1e-7,
        )
    }) {
        return Ok(None);
    }
    // In the plane of the first round's end: the round's section from
    // the common face to the edge, on along the edge to a margin beyond
    // the common face, and back above that face.
    let normal = axis.cross(from);
    let plane = DatumPlane::new(corner, normal, from)?;
    let flat = |p: Point3| {
        Point2::new(
            (p - corner).dot(plane.x_dir()),
            (p - corner).dot(plane.y_dir()),
        )
    };
    let above = (corner - rest).normalize_or_zero();
    let lid = (top_a - corner).dot(above).max(0.0) + margin;
    let (degree, knots, points) = a.0.section(&sa)?;
    let arc = Curve2::new(
        degree,
        knots,
        points
            .iter()
            .map(|h| flat(Point3::new(h[0], h[1], h[2])))
            .collect(),
        Some(points.iter().map(|h| h[3]).collect()),
    )?;
    // (The section runs from the round's first rail to its second.)
    let backwards = ka != 0;
    let corners = [
        flat(top_a),
        flat(rest),
        flat(corner + above * lid),
        flat(top_a + above * (lid - (top_a - corner).dot(above))),
    ];
    let mut profile = Profile::new(plane);
    for (k, point) in corners.iter().enumerate() {
        profile.vertices.push(ProfileVertex {
            name: format!("v{k}"),
            point: *point,
            tolerance: 1e-9,
        });
    }
    let (t0, t1) = arc.domain();
    profile.edges.push(ProfileEdge {
        name: "e0".into(),
        entity: "e0".into(),
        span: (0.0, 1.0),
        curve: arc,
        t0,
        t1,
        start: usize::from(backwards),
        end: usize::from(!backwards),
        tolerance: 1e-9,
    });
    for k in 1..4 {
        profile.edges.push(ProfileEdge {
            name: format!("e{k}"),
            entity: format!("e{k}"),
            span: (0.0, 1.0),
            curve: Curve2::line(corners[k], corners[(k + 1) % 4])?,
            t0: 0.0,
            t1: 1.0,
            start: k,
            end: (k + 1) % 4,
            tolerance: 1e-9,
        });
    }
    let mut walk: Vec<(usize, bool)> = vec![(0, backwards), (1, false), (2, false), (3, false)];
    // (Around what is swept, it on the left.)
    let area: f64 = (0..4)
        .map(|k| corners[k].perp_dot(corners[(k + 1) % 4]))
        .sum();
    if area < 0.0 {
        walk.reverse();
        for step in &mut walk {
            step.1 = !step.1;
        }
    }
    profile.regions.push(ProfileRegion {
        outer: walk.clone(),
        holes: Vec::new(),
        area: area.abs() / 2.0,
    });
    let mut scratch = Topo::new();
    let body = sweep::revolve(
        &mut scratch,
        id,
        &profile,
        &[walk],
        true,
        turn,
        (0.0, angle),
        Make::Solid,
    )?;
    let mut tool = read(&scratch, body)?;
    // It runs on smoothly into the common face.
    let rails: Vec<usize> = (0..tool.edges.len())
        .filter(|&e| tool.edges[e].name.as_deref() == Some(&format!("{id}/ring/v0")))
        .collect();
    for face in &mut tool.faces {
        if face.name.as_deref() == Some(&format!("{id}/side/e0")) {
            face.smooth = rails.clone();
        }
    }
    Ok(Some(tool))
}

/// `tool` without what of it is in `other` — and what the tool knew of
/// its faces (which run on smoothly into the body's along which edges),
/// it still does.
fn less(tool: &Solid, other: &Solid) -> anyhow::Result<Solid> {
    let mut cut = combine(Op::Cut, tool, other)?.solid;
    for face in &mut cut.faces {
        let Some(was) = tool
            .faces
            .iter()
            .find(|f| f.name.is_some() && f.name == face.name)
        else {
            continue;
        };
        // (Its edges along those it ran on smoothly along: by name, or
        // — a piece of one cut in several, which only the first keeps
        // the name of — by lying along one.)
        let known: Vec<&workpiece_boolean::model::Edge> =
            was.smooth.iter().map(|&e| &tool.edges[e]).collect();
        let used: Vec<usize> = face
            .loops
            .iter()
            .flatten()
            .filter_map(|step| match step.walk {
                Walk::Edge { edge, .. } => Some(edge),
                Walk::Pole { .. } => None,
            })
            .collect();
        face.smooth = used
            .into_iter()
            .filter(|&e| {
                let edge = &cut.edges[e];
                let named = edge.name.is_some() && known.iter().any(|k| k.name == edge.name);
                named
                    || known.iter().any(|k| {
                        let within = (k.tolerance + edge.tolerance) * 4.0;
                        [0.25, 0.5, 0.75].into_iter().all(|f| {
                            let t = edge.range.0 + (edge.range.1 - edge.range.0) * f;
                            !workpiece_boolean::geometry::on_curve(
                                &k.curve,
                                k.range,
                                edge.point(t),
                                within,
                            )
                            .is_empty()
                        })
                    })
            })
            .collect();
    }
    Ok(cut)
}

/// The tangent chains of `edges` in `solid`: those edges, and every
/// edge reached from one of them through a vertex where it leaves the
/// way the last one arrived — the next piece of the same smooth run (a
/// rounded outline's lines and arcs, the halves of a rim that was cut
/// in two). An edge that is only a face's seam is no part of a chain.
pub fn chain(solid: &Solid, edges: &[usize]) -> Vec<usize> {
    // The faces along each edge: a seam has one, twice.
    let mut faces: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                faces[edge].push(f);
            }
        }
    }
    let between = |e: usize| faces[e].len() == 2 && faces[e][0] != faces[e][1];
    // The way an edge leaves each of its ends.
    let leaves = |e: usize, end: usize| {
        let edge = &solid.edges[e];
        let t = if end == 0 { edge.range.0 } else { edge.range.1 };
        let (_, d) = edge.curve.derivative_at(t);
        d.normalize_or_zero() * if end == 0 { 1.0 } else { -1.0 }
    };
    let mut all: Vec<usize> = Vec::new();
    for &e in edges {
        if !all.contains(&e) {
            all.push(e);
        }
    }
    let mut next = 0;
    while next < all.len() {
        let e = all[next];
        next += 1;
        if !between(e) {
            continue;
        }
        for end in 0..2 {
            let vertex = solid.edges[e].ends[end];
            // (Arriving is leaving, turned round.)
            let arrives = -leaves(e, end);
            for (other, candidate) in solid.edges.iter().enumerate() {
                if all.contains(&other) || !between(other) {
                    continue;
                }
                for (far, &their) in candidate.ends.iter().enumerate() {
                    if their == vertex
                        && arrives.dot(leaves(other, far)) > 1.0 - 1e-8
                        && !all.contains(&other)
                    {
                        all.push(other);
                    }
                }
            }
        }
    }
    all
}

/// Of `edges`, those to blend before the rest: at a corner where
/// outside and inside edges are blended together, the fewer of the two
/// kinds (the inside ones, of as many of each). Blended first, they leave
/// the others one smooth run past the corner — over the round of an
/// inside edge, or under that of an outside one.
fn leads(solid: &Solid, edges: &[usize], shape: &Shape) -> Vec<usize> {
    // (One edge leads no other.)
    if edges.len() < 2 {
        return Vec::new();
    }
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let convex: Vec<Option<bool>> = edges
        .iter()
        .map(|&e| {
            Rolled::of_in(solid, e, shape, None, size)
                .ok()
                .map(|r| r.convex)
        })
        .collect();
    let mut first: Vec<usize> = Vec::new();
    for v in 0..solid.vertices.len() {
        let at = |kind: bool| -> Vec<usize> {
            (0..edges.len())
                .filter(|&k| solid.edges[edges[k]].ends.contains(&v) && convex[k] == Some(kind))
                .collect()
        };
        let (outside, inside) = (at(true), at(false));
        if outside.is_empty() || inside.is_empty() {
            continue;
        }
        for k in if outside.len() < inside.len() {
            outside
        } else {
            inside
        } {
            if !first.contains(&edges[k]) {
                first.push(edges[k]);
            }
        }
    }
    first
}

/// As [`blend_solid`], along the whole tangent chain ([`chain`]) of
/// each of `edges`. Where outside and inside edges meet at a corner,
/// those of the one kind are blended first ([`leads`]) and the others
/// then along what that made of them: their chains run on over the
/// first ones' blends.
pub fn blend_chains(
    solid: &Solid,
    id: &str,
    edges: &[usize],
    shape: &Shape,
    corners: bool,
) -> anyhow::Result<Solid> {
    let all = chain(solid, edges);
    let first = leads(solid, &all, shape);
    if first.is_empty() || first.len() == all.len() {
        return blend_with(solid, id, &all, shape, corners, true);
    }
    let staged = blend_with(solid, &format!("{id}/first"), &first, shape, corners, true)?;
    // What is left of the other edges: the edges that lie along them.
    let within = {
        let (lo, hi) = solid.extent();
        (hi - lo).length() * 1e-7
    };
    let rest: Vec<&workpiece_boolean::model::Edge> = all
        .iter()
        .filter(|e| !first.contains(e))
        .map(|&e| &solid.edges[e])
        .collect();
    let left: Vec<usize> = (0..staged.edges.len())
        .filter(|&e| {
            let edge = &staged.edges[e];
            let middle = edge.point(edge.middle());
            let along = |was: &&workpiece_boolean::model::Edge| {
                !workpiece_boolean::geometry::on_curve(&was.curve, was.range, middle, within)
                    .is_empty()
            };
            rest.iter().any(along) && sharp(&staged, e)
        })
        .collect();
    ensure!(
        !left.is_empty(),
        "nothing is left of the other edges to blend"
    );
    blend_with(&staged, id, &chain(&staged, &left), shape, corners, true)
}

/// `solid` with its edges `edges` blended to `shape`. With `corners`, a
/// corner where every edge is blended is closed (by a ball between
/// rounds, a flat between chamfers); without, the blends run on until
/// they cut each other off.
pub fn blend_solid(
    solid: &Solid,
    id: &str,
    edges: &[usize],
    shape: &Shape,
    corners: bool,
) -> anyhow::Result<Solid> {
    blend_with(solid, id, edges, shape, corners, false)
}

/// `solid` with each of `blends` — an edge, with all of its tangent
/// chain, and its own shape — made as [`blend_chains`] makes it alone,
/// all in one: every blend of outside edges takes away what it takes
/// alone, every blend of inside edges adds what it adds alone. Where
/// blends of different sizes meet at a corner they run on until they
/// cut each other off: three flat cuts end in the point their planes
/// share.
pub fn blend_each(solid: &Solid, id: &str, blends: &[(usize, Shape)]) -> anyhow::Result<Solid> {
    use workpiece_boolean::{Op, combine};
    ensure!(!blends.is_empty(), "pick the edges to blend");
    // Rounds that differ in size only, all in one: where three of them
    // meet at a corner, they are brought to one size there and closed
    // by a ball ([`blend_back`]).
    let round = |shape: &Shape| match *shape {
        Shape::Fillet {
            by,
            size,
            until: None,
            section,
            rho,
        } => Some((by, size, section, rho)),
        _ => None,
    };
    let rounds: Vec<_> = blends.iter().filter_map(|b| round(&b.1)).collect();
    let alike = rounds
        .iter()
        .all(|r| (r.0, r.2, r.3) == (rounds[0].0, rounds[0].2, rounds[0].3));
    if rounds.len() == blends.len() && blends.len() > 1 && alike {
        let most = rounds.iter().map(|r| r.1).fold(0.0, f64::max);
        let (mut all, mut sizes) = (Vec::new(), Vec::new());
        for ((edge, _), round) in blends.iter().zip(&rounds) {
            for e in chain(solid, &[*edge]) {
                if !all.contains(&e) {
                    all.push(e);
                    sizes.push((e, round.1));
                }
            }
        }
        let shape = Shape::Fillet {
            by: rounds[0].0,
            size: most,
            until: None,
            section: rounds[0].2,
            rho: rounds[0].3,
        };
        let meeting = Meeting {
            setback: None,
            sizes: &sizes,
        };
        if let Ok(made) = blend_back(solid, id, &all, &shape, true, true, &meeting) {
            return Ok(made);
        }
    }
    let mut cut: Option<Solid> = None;
    let mut added = Vec::new();
    for (k, (edge, shape)) in blends.iter().enumerate() {
        let alone = blend_chains(solid, &format!("{id}/b{k}"), &[*edge], shape, true)?;
        let size = {
            let (lo, hi) = solid.extent();
            (hi - lo).length()
        };
        if Rolled::of_in(solid, *edge, shape, None, size)?.convex {
            cut = Some(match cut {
                None => alone,
                Some(sofar) => {
                    combine(Op::Intersect, &sofar, &alone)
                        .context("putting two blends together")?
                        .solid
                }
            });
        } else {
            let more = combine(Op::Cut, &alone, solid).context("taking what a blend adds")?;
            added.push(more.solid);
        }
    }
    let mut made = cut.unwrap_or_else(|| solid.clone());
    for more in &added {
        made = combine(Op::Union, &made, more)
            .context("adding what a blend adds")?
            .solid;
    }
    Ok(made)
}

/// As [`blend_solid`]; with `turns`, where two rounds of outside edges
/// end at an inside edge that is not rounded, the ball turns about that
/// edge's end from the one to the other ([`turned`]) instead of the two
/// running on until they cut each other off.
fn blend_with(
    solid: &Solid,
    id: &str,
    edges: &[usize],
    shape: &Shape,
    corners: bool,
    turns: bool,
) -> anyhow::Result<Solid> {
    blend_back(solid, id, edges, shape, corners, turns, &Meeting::default())
}

/// `solid` with its edges `edges` (each with its tangent chain) rounded
/// to `shape`, and every outside corner where three of them meet *set
/// back*: each round stops `setback` short of the corner, and from
/// there swells, with no break in how it bends, to the larger corner
/// that closes the three — one that leaves each face before the rails
/// of its two rounds would meet.
pub fn blend_set_back(
    solid: &Solid,
    id: &str,
    edges: &[usize],
    shape: &Shape,
    setback: f64,
) -> anyhow::Result<Solid> {
    let Shape::Fillet { size, .. } = *shape else {
        anyhow::bail!("only rounds are set back from a corner");
    };
    ensure!(
        setback > size,
        "a corner is set back by more than its rounds' size"
    );
    let all = chain(solid, edges);
    // The corner a quarter of the way from the rounds' size to how far
    // it is set back: the rounds then grow gently enough towards it
    // that their rails, which turn away from the edge as they grow, do
    // not bend them much tighter than they are — or, where that is
    // not made, a little smaller or larger.
    let mut why = None;
    for share in [0.25, 0.2, 0.3, 0.4, 0.5] {
        let near = size + (setback - size) * share;
        let meeting = Meeting {
            setback: Some((setback, near)),
            sizes: &[],
        };
        match blend_back(solid, id, &all, shape, true, true, &meeting) {
            Ok(made) => return Ok(made),
            Err(e) => why = why.or(Some(e)),
        }
    }
    Err(why.unwrap_or_else(|| anyhow::anyhow!("the corner could not be set back")))
}

/// How rounds meet at the corners where three of them end.
#[derive(Default)]
struct Meeting<'a> {
    /// How far each stops short of the corner, and the size they have
    /// grown to where they meet ([`blend_set_back`]).
    setback: Option<(f64, f64)>,
    /// The edges that have a size of their own, each with it: at a
    /// corner they grow or shrink to the largest of the three.
    sizes: &'a [(usize, f64)],
}

/// As [`blend_with`], with how the rounds meet at their corners.
fn blend_back(
    solid: &Solid,
    id: &str,
    edges: &[usize],
    shape: &Shape,
    corners: bool,
    turns: bool,
    meeting: &Meeting<'_>,
) -> anyhow::Result<Solid> {
    let (setback, sizes) = (meeting.setback, meeting.sizes);
    // Each edge's own shape: `shape`, at its own size where it has one.
    let own = |edge: usize| -> Shape {
        match (sizes.iter().find(|s| s.0 == edge), *shape) {
            (
                Some(&(_, size)),
                Shape::Fillet {
                    by,
                    until,
                    section,
                    rho,
                    ..
                },
            ) => Shape::Fillet {
                by,
                size,
                until,
                section,
                rho,
            },
            _ => *shape,
        }
    };
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    let reach = match *shape {
        Shape::Fillet { size, until, .. } => size.max(until.unwrap_or(size)),
        Shape::Chamfer { first, second } => first.max(second),
    };
    ensure!(reach > 0.0 && reach.is_finite(), "a blend needs a size");
    let called = |edge: usize| {
        let name = solid.edges[edge].name.as_deref();
        format!("edge {}", name.unwrap_or("without a name"))
    };
    // Each edge's blend, or why it has none.
    let fits = |rolled: anyhow::Result<Rolled>| -> anyhow::Result<(Rolled, Station)> {
        let rolled = rolled?;
        // The rails run on the faces: halfway along, each is on its.
        let middle = rolled.middle()?;
        // (Or on a face it runs on into smoothly, where the ball rolls
        // onto that.)
        let faces = middle.faces(&rolled);
        for (side, (&face, next)) in faces.iter().zip(&rolled.next_faces).enumerate() {
            let on =
                |f: usize| workpiece_boolean::lies_on(solid, f, middle.rails[side], size * 1e-7);
            ensure!(
                on(face) || next.iter().any(|f| on(*f)),
                "the blend is wider than a face beside the edge"
            );
        }
        Ok((rolled, middle))
    };
    // (What is inside the solid asked of one made ready for it once.)
    let holder = std::cell::OnceCell::new();
    let holds = |p: Point3| {
        holder
            .get_or_init(|| workpiece_boolean::Holder::new(solid))
            .holds(p)
    };
    let mut rolls: Vec<Option<anyhow::Result<(Rolled, Station)>>> = edges
        .iter()
        .map(|&edge| {
            Some(fits(Rolled::of_in(
                solid,
                edge,
                &own(edge),
                Some(&holds),
                size,
            )))
        })
        .collect();
    // A face too narrow for the blends of two of its edges, both
    // blended: the ball does not touch it at all, but rests against
    // the faces either side of it and bridges over it. The two blends
    // are one, between those two faces, and nothing is left of the
    // narrow one.
    let mut beside: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                beside[edge].push(f);
            }
        }
    }
    for i in 0..edges.len() {
        for j in i + 1..edges.len() {
            let (ei, ej) = (edges[i], edges[j]);
            let narrow: Vec<usize> = beside[ei]
                .iter()
                .copied()
                .filter(|f| beside[ej].contains(f))
                .collect();
            let other = |e: usize| beside[e].iter().copied().find(|f| !narrow.contains(f));
            let (&[narrow], Some(a), Some(b)) = (&narrow[..], other(ei), other(ej)) else {
                continue;
            };
            // (Not two edges that meet at a corner of the face.)
            let meet = |v: &usize| solid.edges[ej].ends.contains(v);
            if solid.edges[ei].ends.iter().any(meet) {
                continue;
            }
            let touched = match (&rolls[i], &rolls[j]) {
                (Some(Ok((ri, mi))), Some(Ok((rj, mj)))) => {
                    let side = |r: &Rolled| usize::from(r.faces[0] != narrow);
                    !(rj.takes(side(rj), mi.rails[side(ri)])
                        || ri.takes(side(ri), mj.rails[side(rj)]))
                }
                (None, _) | (_, None) => true,
                _ => false,
            };
            if touched || a == b {
                continue;
            }
            let Ok((bridge, middle)) = fits(Rolled::between(solid, [a, b], shape)) else {
                continue;
            };
            // (All of the narrow face is under the bridge.)
            let middle_of = |e: usize| solid.edges[e].point(solid.edges[e].middle());
            if bridge.faces == [a, b]
                && bridge.takes(0, middle_of(ei))
                && bridge.takes(1, middle_of(ej))
            {
                rolls[i] = Some(Ok((bridge, middle)));
                rolls[j] = None;
            }
        }
    }
    // An edge the ball is too large to roll along (a round tighter
    // than the ball, in a chain of edges) gets a smaller one that does
    // roll along it, and the blends either side of it shrink to that
    // towards it, with no break: the round squeezes through the tight
    // stretch. Where no smaller ball rolls there either, the edge is
    // not blended, and the blends either side of it run on straight
    // into it, and end where they cut each other — or the body — off.
    let tight: Vec<usize> = (0..edges.len())
        .filter(|&i| matches!(&rolls[i], Some(Ok((rolled, _))) if rolled.convex && rolled.tight()))
        .collect();
    if !tight.is_empty() && tight.len() < rolls.iter().flatten().count() {
        for &i in &tight {
            let (ends, long) = match &rolls[i] {
                Some(Ok((rolled, _))) => (rolled.vertices, rolled.length + 2.0 * reach),
                _ => continue,
            };
            let smaller = match *shape {
                Shape::Fillet {
                    by,
                    size,
                    until: None,
                    section,
                    rho,
                } => [0.5, 0.25, 0.125].into_iter().find_map(|share: f64| {
                    let of = |share: f64| Shape::Fillet {
                        by,
                        size: size * share,
                        until: None,
                        section,
                        rho,
                    };
                    // (With room to spare: not the largest that rolls.)
                    let rolls = |share: f64| {
                        Rolled::of_in(solid, edges[i], &of(share), Some(&holds), size)
                            .is_ok_and(|r| !r.tight())
                    };
                    (rolls(share * 1.5) && rolls(share))
                        .then(|| {
                            fits(Rolled::of_in(
                                solid,
                                edges[i],
                                &of(share),
                                Some(&holds),
                                size,
                            ))
                            .ok()
                        })
                        .flatten()
                        .map(|fit| (share, fit))
                }),
                _ => None,
            };
            let share = smaller.as_ref().map(|s| s.0);
            rolls[i] = smaller.map(|s| Ok(s.1));
            for (j, roll) in rolls.iter_mut().enumerate() {
                let Some(Ok((rolled, _))) = roll else {
                    continue;
                };
                if tight.contains(&j) || rolled.vertices[0] == rolled.vertices[1] {
                    continue;
                }
                for end in 0..2 {
                    if !ends.contains(&rolled.vertices[end]) {
                        continue;
                    }
                    match share {
                        Some(by) => {
                            rolled.swell[end] = Some(crate::roll::Swell {
                                near: 0.0,
                                far: (1.5 * reach).min(0.45 * rolled.length),
                                by,
                            });
                        }
                        None => {
                            rolled.on[end] = Some(long.max(rolled.on[end].unwrap_or(0.0)));
                        }
                    }
                }
            }
        }
    }
    let mut found: Vec<(usize, Rolled)> = Vec::new();
    let mut middles = Vec::new();
    for (&edge, roll) in edges.iter().zip(rolls) {
        let Some(roll) = roll else {
            continue;
        };
        let (rolled, middle) = roll.with_context(|| called(edge))?;
        middles.push(middle);
        found.push((edge, rolled));
    }
    // Three straight outside rounds of different sizes that end
    // together: each grows (or shrinks) towards the corner to the size
    // of the largest, and a ball of that size closes the three.
    if !sizes.is_empty() {
        for v in 0..solid.vertices.len() {
            let at = (0..solid.edges.len()).filter(|&e| solid.edges[e].ends.contains(&v));
            let its: Vec<usize> = (0..found.len())
                .filter(|&k| found[k].1.vertices.contains(&v))
                .collect();
            let size_of = |k: &usize| match own(found[*k].0) {
                Shape::Fillet { size, .. } => size,
                Shape::Chamfer { .. } => 0.0,
            };
            let most = its.iter().map(size_of).fold(0.0, f64::max);
            let least = its.iter().map(size_of).fold(f64::INFINITY, f64::min);
            let far = 2.5 * most;
            let fit = |k: &usize| {
                let rolled = &found[*k].1;
                rolled.convex
                    && rolled.straight
                    && rolled.vertices[0] != rolled.vertices[1]
                    && rolled.length > far * 1.2
            };
            if at.count() != 3
                || its.len() != 3
                || !its.iter().all(fit)
                || most - least <= most * 1e-9
            {
                continue;
            }
            let grow: Vec<(usize, f64)> = its.iter().map(|k| (*k, most / size_of(k))).collect();
            for (k, by) in grow {
                let rolled = &mut found[k].1;
                let end = usize::from(rolled.vertices[0] != v);
                rolled.swell[end] = Some(crate::roll::Swell {
                    near: most,
                    far,
                    by,
                });
            }
        }
    }
    // A corner set back: where three straight outside rounds end
    // together, each grows towards it from where it is set back to.
    if let (Some((setback, near)), Shape::Fillet { size: radius, .. }) = (setback, *shape) {
        for v in 0..solid.vertices.len() {
            let at = (0..solid.edges.len()).filter(|&e| solid.edges[e].ends.contains(&v));
            let its: Vec<usize> = (0..found.len())
                .filter(|&k| found[k].1.vertices.contains(&v))
                .collect();
            let fit = |k: &usize| {
                let rolled = &found[*k].1;
                rolled.convex
                    && rolled.straight
                    && rolled.vertices[0] != rolled.vertices[1]
                    && rolled.length > setback * 1.2
            };
            if at.count() != 3 || its.len() != 3 || !its.iter().all(fit) {
                continue;
            }
            for k in its {
                let rolled = &mut found[k].1;
                let end = usize::from(rolled.vertices[0] != v);
                rolled.swell[end] = Some(crate::roll::Swell {
                    near,
                    far: setback,
                    by: near / radius,
                });
            }
        }
    }
    blend_found(solid, id, found, &middles, shape, corners, turns, size)
}

/// `solid` with two of its faces that do not share an edge, but lie
/// either side of one face, joined by a blend of `shape`: it runs along
/// where the two would meet, and takes the face between them with it
/// (a round between the two sides of a chamfer takes the chamfer). Two
/// faces that do share an edge are blended along it.
pub fn blend_faces(
    solid: &Solid,
    id: &str,
    faces: [usize; 2],
    shape: &Shape,
) -> anyhow::Result<Solid> {
    let rolled = Rolled::between(solid, faces, shape)?;
    let middle = rolled.middle()?;
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    for side in 0..2 {
        ensure!(
            workpiece_boolean::lies_on(solid, rolled.faces[side], middle.rails[side], size * 1e-7),
            "the blend is wider than one of the two faces"
        );
    }
    // What is between the two faces lies under the blend.
    let edge = &solid.edges[rolled.edge];
    ensure!(
        rolled.faces != faces || rolled.takes(0, edge.point(edge.middle())),
        "the blend does not reach across the face between the two"
    );
    let found = vec![(rolled.edge, rolled)];
    blend_found(solid, id, found, &[middle], shape, true, false, size)
}

/// `solid` with its face `middle` made one round from the face
/// `sides[0]` to the face `sides[1]`, both of which it shares an edge
/// with: the round touches all three. It is the round of both edges by
/// the radius at which the two rounds meet on the middle face, and
/// leave nothing of it.
pub fn full_round(
    solid: &Solid,
    id: &str,
    sides: [usize; 2],
    middle: usize,
) -> anyhow::Result<Solid> {
    ensure!(
        sides[0] != sides[1] && !sides.contains(&middle),
        "a full round is of one face between two others"
    );
    let size = {
        let (lo, hi) = solid.extent();
        (hi - lo).length()
    };
    // The edges the middle face shares with each side.
    let shared = |side: usize| -> Vec<usize> {
        (0..solid.edges.len())
            .filter(|&e| {
                let on = |f: usize| {
                    solid.faces[f]
                        .loops
                        .iter()
                        .flatten()
                        .any(|step| matches!(step.walk, Walk::Edge { edge, .. } if edge == e))
                };
                on(side) && on(middle)
            })
            .collect()
    };
    let edges = [shared(sides[0]), shared(sides[1])];
    ensure!(
        !edges[0].is_empty() && !edges[1].is_empty(),
        "the middle face shares an edge with each of the other two"
    );
    let round = |radius: f64| Shape::Fillet {
        by: By::Radius,
        size: radius,
        until: None,
        section: Section::Circular,
        rho: 0.5,
    };
    // How far the round of the first edge reaches past the round of the
    // second on the middle face, both of `radius`, where the first
    // edge's place `share` of the way along it is.
    let past = |radius: f64, share: f64| -> Option<f64> {
        let shape = round(radius);
        let first = Rolled::of(solid, edges[0][0], &shape).ok()?;
        let second = Rolled::of(solid, edges[1][0], &shape).ok()?;
        let (t0, t1) = first.range;
        let station = first.station(t0 + (t1 - t0) * share, None).ok()?;
        let side = |r: &Rolled| usize::from(r.faces[0] != middle);
        second.inside(side(&second), station.rails[side(&first)])
    };
    // The radius at which the two just meet, halfway along.
    let (mut lo, mut hi) = (0.0, size * 1e-3);
    while past(hi, 0.5).is_some_and(|by| by < 0.0) && hi < size * 10.0 {
        (lo, hi) = (hi, hi * 2.0);
    }
    for _ in 0..200 {
        workpiece_core::cancel::checkpoint();
        let mid = (lo + hi) / 2.0;
        if mid <= lo || mid >= hi {
            break;
        }
        match past(mid, 0.5) {
            Some(by) if by < 0.0 => lo = mid,
            _ => hi = mid,
        }
    }
    let radius = lo;
    ensure!(radius > 0.0, "no round touches all three faces");
    // (And all along: a round whose radius changes along the face is
    // not made yet.)
    for share in [0.1, 0.9] {
        let off = past(radius, share).context("no round touches all three faces")?;
        ensure!(
            off.abs() <= size * 1e-7,
            "the round between these faces is not of one radius all along: that is not made yet"
        );
    }
    let all: Vec<usize> = edges.concat();
    blend_solid(solid, id, &all, &round(radius), true)
}

/// As [`blend_with`], of blends already found: each with the edge it
/// runs along and the blend halfway along it.
#[allow(clippy::too_many_arguments)]
fn blend_found(
    solid: &Solid,
    id: &str,
    found: Vec<(usize, Rolled)>,
    middles: &[Station],
    shape: &Shape,
    corners: bool,
    turns: bool,
    size: f64,
) -> anyhow::Result<Solid> {
    let reach = match *shape {
        Shape::Fillet { size, until, .. } => size.max(until.unwrap_or(size)),
        Shape::Chamfer { first, second } => first.max(second),
    };
    // Whether a round is not of one size all along.
    let grows = matches!(shape, Shape::Fillet { until: Some(_), .. });
    // Small beside the blend, so that the tool stays by its edge.
    let margin = (reach * 0.25).min(size * 0.01);
    let called = |edge: usize| {
        let name = solid.edges[edge].name.as_deref();
        format!("edge {}", name.unwrap_or("without a name"))
    };
    // No blend reaches into what another takes of a face they share:
    // together they would be wider than the face. (But for two that
    // meet at a corner: there each runs into the other's.)
    for (i, middle) in middles.iter().enumerate() {
        for (j, (other, rolled)) in found.iter().enumerate() {
            let meet = |v: &usize| found[i].1.vertices.contains(v);
            if rolled.vertices.iter().any(meet) {
                continue;
            }
            for (side, face) in found[i].1.faces.iter().enumerate() {
                let Some(theirs) = rolled.faces.iter().position(|f| f == face) else {
                    continue;
                };
                ensure!(
                    i == j || !rolled.takes(theirs, middle.rails[side]),
                    "the blends of {} and {} are wider together than the face between them",
                    called(found[i].0),
                    called(*other)
                );
            }
        }
    }
    // Blends that touch nothing but the two faces beside their edge
    // and the faces it ends on are made where they are: nothing is
    // combined.
    if let Some(made) = crate::local::blended(solid, id, &found, shape, margin, size) {
        return Ok(made);
    }
    // The corners a patch closes: the rounds end there at the patch's
    // sides, not at the corner.
    let mut patches: std::collections::BTreeMap<usize, Patched> = std::collections::BTreeMap::new();
    if corners && !grows {
        for v in 0..solid.vertices.len() {
            if let Some(patch) = patched(solid, v, &found, shape, size)? {
                patches.insert(v, patch);
            }
        }
    }
    // Of an inside corner, how much of each round comes with it: all of
    // it (the round is then one face, from the corner to its end) — but
    // less than half of one that has such a corner at its other end
    // too, which has to be three radii long for that; the rest of it is
    // laid between the two.
    loop {
        let short = patches.iter().find_map(|(v, patch)| {
            (!patch.convex
                && (0..3).any(|k| {
                    let far = solid.edges[patch.edges[k]].ends.iter().find(|w| *w != v);
                    far.is_some_and(|w| patches.get(w).is_some_and(|p| !p.convex))
                        && patch.lengths[k] < 3.0 * patch.radius
                }))
            .then_some(*v)
        });
        match short {
            Some(v) => patches.remove(&v),
            None => break,
        };
    }
    let inside: Vec<usize> = patches
        .iter()
        .filter(|p| !p.1.convex)
        .map(|p| *p.0)
        .collect();
    for v in inside {
        let patch = &patches[&v];
        let stubs = [0, 1, 2].map(|k| {
            let far = solid.edges[patch.edges[k]].ends.iter().find(|w| **w != v);
            let both = far.is_some_and(|w| patches.get(w).is_some_and(|p| !p.convex));
            if both {
                (patch.lengths[k] / 2.0 - 0.25 * patch.radius).min(2.5 * patch.radius)
            } else {
                patch.lengths[k]
            }
        });
        if let Some(patch) = patches.get_mut(&v) {
            patch.stubs = stubs;
        }
    }
    // Where two blends of a kind meet at an angle with no patch
    // between them, and neither runs on past the other (two outside
    // ones around an inside corner, two inside ones around an outside
    // corner): what both would be, run on straight, lies between them.
    let mut meets: std::collections::BTreeMap<usize, Vec<(usize, usize)>> =
        std::collections::BTreeMap::new();
    for (k, (_, rolled)) in found.iter().enumerate() {
        if !rolled.closed {
            for (end, &vertex) in rolled.vertices.iter().enumerate().take(2) {
                meets.entry(vertex).or_default().push((k, end));
            }
        }
    }
    let kinks: Vec<(usize, [(usize, usize); 2])> = meets
        .into_iter()
        .filter_map(|(v, at)| {
            let &[a, b] = &at[..] else {
                return None;
            };
            let (ra, rb) = (&found[a.0].1, &found[b.0].1);
            let alike = ra.convex == rb.convex && ra.air[a.1] == rb.air[b.1];
            let open = ra.convex != ra.air[a.1];
            let bent = ra.away(a.1).dot(rb.away(b.1)) > -1.0 + 1e-6;
            (!patches.contains_key(&v) && alike && open && bent).then_some((v, [a, b]))
        })
        .collect();
    // Of those, where the ball turns about the end of an inside edge:
    // what it takes there.
    let mut turnings: Vec<(usize, Solid)> = Vec::new();
    let round = matches!(
        shape,
        Shape::Fillet {
            until: None,
            section: Section::Circular,
            ..
        }
    );
    if turns && round {
        for (v, [a, b]) in &kinks {
            let (ra, rb) = (&found[a.0].1, &found[b.0].1);
            if !(ra.convex && rb.convex) {
                continue;
            }
            let name = format!("{id}/v{v}/turn");
            if let Some(tool) = turned(solid, &name, *v, (ra, a.1), (rb, b.1), margin, size)? {
                turnings.push((*v, tool));
            }
        }
    }
    let turning = |v: usize| turnings.iter().any(|t| t.0 == v);
    // Where an edge ends on a flat face at a slant: its tool runs on
    // past the end and is cut back to that face's plane.
    let mut slants: Vec<[Option<EndCut>; 2]> = Vec::new();
    for (k, (edge, rolled)) in found.iter().enumerate() {
        let mut slant = [None, None];
        for (end, slant) in slant.iter_mut().enumerate() {
            if !kinks.iter().any(|kink| kink.1.contains(&(k, end))) {
                *slant = rolled
                    .check_end(solid, end, margin)
                    .with_context(|| called(*edge))?;
            }
        }
        slants.push(slant);
    }
    // What is taken away first, then what is added.
    let mut result = solid.clone();
    // What `result` holds, where it was measured last: every step is
    // checked against the one before.
    let mut held: Option<f64> = None;
    // An inside blend is added with a little of what is behind its
    // faces — which, where an outside blend beside it has taken that
    // away, is not behind anything any more: what the outside blends
    // took is left out of what the inside ones add.
    let mixed = found.iter().any(|f| f.1.convex) && found.iter().any(|f| !f.1.convex);
    let mut taken: Option<Solid> = None;
    for convex in [true, false] {
        if !convex && mixed {
            taken = combine(Op::Cut, solid, &result)
                .ok()
                .map(|outcome| outcome.solid)
                .filter(|taken| !taken.is_empty());
        }
        let fit = |tool: Solid| -> Solid {
            match &taken {
                Some(taken) if !convex => less(&tool, taken).unwrap_or(tool),
                _ => tool,
            }
        };
        // The corners first: each round then ends on its corner's side,
        // face to face, not edge to edge with the round beside it.
        for (v, patch) in patches.iter().filter(|p| p.1.convex == convex) {
            let r = patch.radius;
            let vertex = solid.vertices[*v].point;
            let tool = if convex {
                let mut scratch = Topo::new();
                let kept = sweep::corner(
                    &mut scratch,
                    &format!("{id}/v{v}/corner"),
                    patch.centre,
                    patch.axes,
                    &patch.section,
                )?;
                let kept = read(&scratch, kept)?;
                // What is between the patch and the corner: of the
                // block from the centre to a little past the corner,
                // all but what the patch keeps.
                let cell = cuboid(
                    &format!("{id}/v{v}"),
                    patch.centre,
                    patch.axes,
                    [r + reach * 0.25; 3],
                )?;
                combine(Op::Cut, &cell, &kept)
                    .context("making a corner between the blends")?
                    .solid
            } else {
                // An inside corner is an outside corner of what is not
                // there: a block standing for the empty corner (with a
                // stretch of each round, or all of it where the edge
                // is short) is rounded as any block is, and what its
                // box holds besides it is what is added.
                let away = patch.axes.map(|a| -a);
                let sides = patch.stubs;
                let empty = cuboid(&format!("{id}/v{v}/empty"), vertex, away, sides)?;
                let its: Vec<usize> = (0..empty.edges.len())
                    .filter(|&e| {
                        empty.edges[e]
                            .ends
                            .iter()
                            .any(|&w| empty.vertices[w].point.distance(vertex) <= 1e-9 * size)
                    })
                    .collect();
                let rounded = blend_solid(&empty, &format!("{id}/v{v}"), &its, shape, true)
                    .context("making an inside corner between the blends")?;
                let around = cuboid(
                    &format!("{id}/v{v}/around"),
                    vertex - (away[0] + away[1] + away[2]) * (reach * 0.25),
                    away,
                    sides.map(|s| s + reach * 0.25),
                )?;
                let mut tool = combine(Op::Cut, &around, &rounded)
                    .context("making an inside corner between the blends")?
                    .solid;
                // Its rounds run on smoothly into the faces their rails
                // lie in, as any round's do.
                if matches!(shape, Shape::Fillet { .. }) {
                    let base = |name: &str, part: &str| -> Option<String> {
                        name.find(part).map(|at| name[..at].to_owned())
                    };
                    let rails: Vec<(usize, String)> = (0..tool.edges.len())
                        .filter_map(|e| {
                            let name = tool.edges[e].name.as_deref()?;
                            let of = base(name, "/along/v0").or_else(|| base(name, "/along/v1"))?;
                            Some((e, of))
                        })
                        .collect();
                    for face in &mut tool.faces {
                        let Some(of) = face.name.as_deref().and_then(|n| base(n, "/side/e0"))
                        else {
                            continue;
                        };
                        face.smooth = rails.iter().filter(|r| r.1 == of).map(|r| r.0).collect();
                    }
                }
                tool
            };
            let tool = fit(tool);
            let op = if convex { Op::Cut } else { Op::Union };
            let before = held.unwrap_or_else(|| measure(&result).volume);
            result = combine(op, &result, &tool)
                .context("closing a corner between the blends")?
                .solid;
            // The corner takes something away, or adds something.
            let after = measure(&result).volume;
            held = Some(after);
            let change = after - before;
            ensure!(
                if convex { change < 0.0 } else { change > 0.0 },
                "closing a corner between the blends came to nothing"
            );
        }
        for (k, (_, corner)) in found.iter().enumerate() {
            if corner.convex != convex {
                continue;
            }
            // (An inside round starts within the stretch of it that
            // its corner brought along: laid end to end with the
            // corner, the two would share a whole face and a tangent
            // seam.)
            let edge = found[k].0;
            let inset = corner.vertices.map(|v| match patches.get(&v) {
                Some(p) if p.convex => p.radius,
                Some(p) => {
                    let at = p.edges.iter().position(|e| *e == edge).unwrap_or(0);
                    // (All of the round came with the corner.)
                    if p.stubs[at] >= p.lengths[at] {
                        f64::INFINITY
                    } else {
                        p.stubs[at] - 0.5 * p.radius
                    }
                }
                None => 0.0,
            });
            if inset.iter().any(|i| i.is_infinite()) {
                continue;
            }
            // Where it meets another blend at an angle (or itself, an
            // edge that comes back to its start at one), it runs on
            // past the corner and is cut back to the plane halfway
            // between the two: they end there face to face.
            let cuts: Vec<(usize, Point3, Point3, f64)> = (0..2)
                .filter_map(|end| {
                    let (v, [a, b]) = kinks
                        .iter()
                        .find(|kink| kink.1.contains(&(k, end)) && !turning(kink.0))?;
                    let other = if *a == (k, end) { b } else { a };
                    let (own, their) = (-corner.away(end), -found[other.0].1.away(other.1));
                    let half = own.angle_between(their) / 2.0;
                    let long = (reach + margin) / half.tan().max(0.05) * 1.5 + margin * 4.0;
                    let across = (own - their).try_normalize()?;
                    Some((end, solid.vertices[*v].point, across, long))
                })
                .collect();
            let whole = corner.vertices[0] != corner.vertices[1] || cuts.len() < 2;
            let halves: Vec<Option<usize>> = if whole {
                vec![None]
            } else {
                vec![Some(0), Some(1)]
            };
            for half in halves {
                let mut stretch = Stretch {
                    inset,
                    half,
                    ..Stretch::default()
                };
                stretch.on = corner.on;
                for (end, _, _, long) in &cuts {
                    stretch.on[*end] = Some(*long);
                }
                let name = match half {
                    Some(half) => format!("{id}/e{k}/h{half}"),
                    None => format!("{id}/e{k}"),
                };
                // The tool, cut back where it is to end, and what the body
                // is with it.
                let attempt = |stretch: &Stretch,
                               slant: &[Option<EndCut>; 2]|
                 -> anyhow::Result<(Solid, f64)> {
                    let mut tool = corner
                        .tool(&name, margin, stretch)
                        .with_context(|| called(edge))?;
                    for (end, at, across, long) in &cuts {
                        if half.is_some_and(|half| half != *end) {
                            continue;
                        }
                        let far = long + (reach + margin) * 4.0;
                        let (b, c) = across.any_orthonormal_pair();
                        let beyond = cuboid(
                            &format!("{name}/beyond{end}"),
                            *at - (b + c) * far,
                            [*across, b, c],
                            [far, far * 2.0, far * 2.0],
                        )?;
                        tool = less(&tool, &beyond)
                            .with_context(|| format!("ending the blend of {}", called(edge)))?;
                    }
                    for (end, slant) in slant.iter().enumerate() {
                        let Some(slant) = slant else {
                            continue;
                        };
                        if half.is_some_and(|half| half != end) || slant.over.is_some() {
                            continue;
                        }
                        let far = slant.long.unwrap_or(0.0) + (reach + margin) * 4.0;
                        for (j, (face, keep)) in slant.curved.iter().enumerate() {
                            let beyond = past_face(
                                solid,
                                *face,
                                slant.at,
                                *keep,
                                far,
                                &format!("{name}/past{end}/c{j}"),
                            )
                            .with_context(|| format!("ending the blend of {}", called(edge)))?;
                            tool = less(&tool, &beyond)
                                .with_context(|| format!("ending the blend of {}", called(edge)))?;
                        }
                        for (j, keep) in slant.keep.iter().enumerate() {
                            let (b, c) = keep.any_orthonormal_pair();
                            let beyond = cuboid(
                                &format!("{name}/past{end}/{j}"),
                                slant.at - (b + c) * far,
                                [-*keep, b, c],
                                [far, far * 2.0, far * 2.0],
                            )?;
                            tool = less(&tool, &beyond)
                                .with_context(|| format!("ending the blend of {}", called(edge)))?;
                        }
                    }
                    let mut tool = fit(tool);
                    // Where the ball rolls over a rim, its rail runs along
                    // the rim: the two are one edge there, from where it
                    // reaches the rim to the end.
                    let mut body = result.clone();
                    for (end, slant) in slant.iter().enumerate() {
                        let Some(from) = slant.as_ref().and_then(|s| s.over) else {
                            continue;
                        };
                        if half.is_some_and(|half| half != end) {
                            continue;
                        }
                        for solid in [&mut body, &mut tool] {
                            split_at(solid, from, size * 1e-7);
                        }
                    }
                    let op = if convex { Op::Cut } else { Op::Union };
                    let made = combine(op, &body, &tool)
                        .with_context(|| format!("blending {}", called(edge)))?
                        .solid;
                    Ok((made, measure(&tool).volume))
                };
                // Rolled on past an end it is cut back at; or, where that
                // does not come out, run on straight.
                let tried = |slant: &[Option<EndCut>; 2]| {
                    let mut rolled = stretch;
                    for (end, slant) in slant.iter().enumerate() {
                        if let Some(long) = slant.as_ref().and_then(|s| s.long) {
                            rolled.rolled_on[end] = Some(long);
                        }
                    }
                    let straight = {
                        let mut straight = stretch;
                        for (end, slant) in slant.iter().enumerate() {
                            if let Some(long) = slant.as_ref().and_then(|s| s.long) {
                                straight.on[end] = Some(long.max(straight.on[end].unwrap_or(0.0)));
                            }
                        }
                        straight
                    };
                    if slant.iter().any(Option::is_some) {
                        attempt(&rolled, slant).or_else(|_| attempt(&straight, slant))
                    } else {
                        attempt(&stretch, slant)
                    }
                };
                let before = held.unwrap_or_else(|| measure(&result).volume);
                // (Where the ball is to roll over a rim and that does not
                // come out, the blend is not made: cut back to the face it
                // ends on instead, it would leave more than it is to.)
                let (made, holds) = tried(&slants[k])?;
                result = made;
                // The blend takes something away, or adds something —
                // and no more than its tool holds.
                let after = measure(&result).volume;
                held = Some(after);
                let change = after - before;
                ensure!(
                    if convex { change < 0.0 } else { change > 0.0 },
                    "blending {} came to nothing",
                    called(edge)
                );
                ensure!(
                    holds > 0.0 && change.abs() <= holds * (1.0 + 1e-6),
                    "blending {} did not come out as a blend: it would change the body by more than the blend is",
                    called(edge)
                );
            }
        }
    }
    // Then what the ball takes turning about the ends of inside edges:
    // between two rounds that end there, face to face with both.
    for (v, tool) in &turnings {
        let before = held.unwrap_or_else(|| measure(&result).volume);
        result = combine(Op::Cut, &result, tool)
            .with_context(|| format!("turning about the corner {v}"))?
            .solid;
        let after = measure(&result).volume;
        held = Some(after);
        ensure!(
            after < before,
            "turning about a corner between the blends came to nothing"
        );
    }
    if !corners || grows {
        return Ok(result);
    }
    // The corners where every edge is blended, and all are convex.
    for (v, vertex) in solid.vertices.iter().enumerate() {
        let at: Vec<usize> = (0..solid.edges.len())
            .filter(|&e| solid.edges[e].ends.contains(&v))
            .collect();
        let blended: Vec<&(usize, Rolled)> = at
            .iter()
            .filter_map(|e| found.iter().find(|f| f.0 == *e))
            .collect();
        let all = |convex: bool| blended.iter().all(|b| b.1.convex == convex);
        if at.len() < 3
            || blended.len() != at.len()
            || !all(true)
            || patches.contains_key(&v)
            || blended.iter().any(|b| !b.1.straight)
        {
            continue;
        }
        // Of each edge here: which end of it this is, the way along it
        // away from the corner, and its blend there.
        let ends: Vec<usize> = blended
            .iter()
            .map(|b| usize::from(b.1.vertices[0] != v))
            .collect();
        let away: Vec<Point3> = (0..blended.len())
            .map(|k| blended[k].1.away(ends[k]))
            .collect();
        let mut stations = Vec::new();
        for k in 0..blended.len() {
            stations.push(blended[k].1.end(ends[k])?);
        }
        let tool = match *shape {
            Shape::Fillet {
                section: Section::Circular,
                ..
            } => {
                // The ball that touches the faces here: its centre is
                // on each round's axis.
                let centres: Vec<Point3> = stations.iter().filter_map(|s| s.centre).collect();
                // The rounds' axes run from there along the edges, and
                // meet where the ball's centre is.
                if centres.len() != away.len() {
                    continue;
                }
                let Some(centre) = meeting(centres[0], away[0], centres[1], away[1]) else {
                    continue;
                };
                let off = |k: usize| (centre - centres[k]).reject_from(away[k]).length();
                if (0..centres.len()).any(|k| off(k) > 1e-6 * size) {
                    continue;
                }
                let radius = stations[0].radius;
                // The cell beyond the planes through the centre across
                // each edge, towards the corner; less the ball.
                let reach = centre.distance(vertex.point) * 4.0 + margin * 4.0;
                // Behind the first plane, less what is beyond the
                // others.
                let mut cell = block(&format!("{id}/v{v}"), centre, -away[0], reach, reach)?;
                for (k, direction) in away.iter().enumerate().skip(1) {
                    let half = block(
                        &format!("{id}/v{v}/h{k}"),
                        centre,
                        *direction,
                        reach * 4.0,
                        reach * 4.0,
                    )?;
                    cell = combine(Op::Cut, &cell, &half)?.solid;
                }
                let round = ball(&format!("{id}/v{v}/ball"), centre, radius)?;
                combine(Op::Cut, &cell, &round)?.solid
            }
            Shape::Chamfer { .. } => {
                // The plane through the points where the rails of the
                // edges here meet on each face.
                let mut points: Vec<Point3> = Vec::new();
                for i in 0..blended.len() {
                    for j in i + 1..blended.len() {
                        let (a, b) = (&blended[i].1, &blended[j].1);
                        for (sa, fa) in a.faces.iter().enumerate() {
                            for (sb, fb) in b.faces.iter().enumerate() {
                                if fa != fb {
                                    continue;
                                }
                                // On their common face: the corner, in
                                // by each one's rail.
                                if let Some(p) = meeting(
                                    stations[i].rails[sa],
                                    away[i],
                                    stations[j].rails[sb],
                                    away[j],
                                ) {
                                    points.push(p);
                                }
                            }
                        }
                    }
                }
                if points.len() < 3 {
                    continue;
                }
                let normal = (points[1] - points[0]).cross(points[2] - points[0]);
                let Some(mut normal) = normal.try_normalize() else {
                    continue;
                };
                if normal.dot(vertex.point - points[0]) < 0.0 {
                    normal = -normal;
                }
                if points
                    .iter()
                    .any(|p| (*p - points[0]).dot(normal).abs() > 1e-6 * size)
                {
                    continue;
                }
                let reach = points[0].distance(vertex.point) * 4.0 + margin * 4.0;
                block(&format!("{id}/v{v}"), points[0], normal, reach, reach)?
            }
            _ => continue,
        };
        result = combine(Op::Cut, &result, &tool)
            .context("closing a corner between the blends")?
            .solid;
    }
    Ok(result)
}

/// Where the line through `p` along `d` and the line through `q` along
/// `e` (unit directions) meet, or come nearest, on the first.
fn meeting(p: Point3, d: Point3, q: Point3, e: Point3) -> Option<Point3> {
    let (de, between) = (d.dot(e), q - p);
    let det = 1.0 - de * de;
    (det > 1e-9).then(|| p + d * ((between.dot(d) - between.dot(e) * de) / det))
}

impl By {
    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            By::Radius => "Radius",
            By::Width => "Width",
            By::Distance => "Distance",
        }
    }
}

impl Section {
    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Section::Circular => "Circular",
            Section::Conic => "Conic",
            Section::Curvature | Section::Centred => "Curvature",
        }
    }
}

impl workpiece_core::Choices for By {
    const ALL: &'static [Self] = &[By::Radius, By::Width, By::Distance];

    fn label(self) -> &'static str {
        Self::label(self)
    }
}

impl workpiece_core::Choices for Section {
    const ALL: &'static [Self] = &[Section::Circular, Section::Conic, Section::Curvature];

    fn label(self) -> &'static str {
        Self::label(self)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{PI, TAU};

    use super::*;

    /// The prism over the outline `points` (anticlockwise, in the plane
    /// z = 0), `height` tall.
    fn prism(points: &[Point2], height: f64) -> Solid {
        prism_arcs(points, &[], height)
    }

    /// As [`prism`], with the sides from the points in `arcs` on to the
    /// next arcs about their centres, clockwise.
    fn prism_arcs(points: &[Point2], arcs: &[(usize, Point2)], height: f64) -> Solid {
        let arcs: Vec<(usize, Point2, bool)> = arcs.iter().map(|&(k, c)| (k, c, false)).collect();
        prism_turns(points, &arcs, (0.0, height))
    }

    /// As [`prism_arcs`], from `heights.0` to `heights.1`, with the
    /// arcs anticlockwise where they say so.
    fn prism_turns(
        points: &[Point2],
        arcs: &[(usize, Point2, bool)],
        heights: (f64, f64),
    ) -> Solid {
        let plane = DatumPlane::new(Point3::ZERO, Point3::Z, Point3::X).expect("a plane");
        let mut profile = Profile::new(plane);
        let n = points.len();
        for (k, point) in points.iter().enumerate() {
            profile.vertices.push(ProfileVertex {
                name: format!("v{k}"),
                point: *point,
                tolerance: 1e-9,
            });
            profile.edges.push(ProfileEdge {
                name: format!("e{k}"),
                entity: format!("e{k}"),
                span: (0.0, 1.0),
                curve: match arcs.iter().find(|a| a.0 == k) {
                    Some(&(_, centre, true)) => {
                        let (from, to) = (*point - centre, points[(k + 1) % n] - centre);
                        let turn = (from.perp_dot(to)).atan2(from.dot(to)).rem_euclid(TAU);
                        let (form, knots) = crate::arcs::arc_form(turn);
                        let (controls, weights): (Vec<Point2>, Vec<f64>) = form
                            .into_iter()
                            .map(|(q, w)| (centre + from.rotate(q), w))
                            .unzip();
                        Curve2::new(2, knots, controls, Some(weights)).expect("an arc")
                    }
                    Some(&(_, centre, false)) => {
                        // Anticlockwise from the end back to the start,
                        // turned about.
                        let (from, to) = (points[(k + 1) % n] - centre, *point - centre);
                        let turn = (from.perp_dot(to)).atan2(from.dot(to)).rem_euclid(TAU);
                        let (form, knots) = crate::arcs::arc_form(turn);
                        let (controls, weights): (Vec<Point2>, Vec<f64>) = form
                            .into_iter()
                            .map(|(q, w)| (centre + from.rotate(q), w))
                            .unzip();
                        Curve2::new(2, knots, controls, Some(weights))
                            .expect("an arc")
                            .reversed()
                    }
                    None => Curve2::line(*point, points[(k + 1) % n]).expect("a line"),
                },
                t0: 0.0,
                t1: 1.0,
                start: k,
                end: (k + 1) % n,
                tolerance: 1e-9,
            });
        }
        let walk: Vec<(usize, bool)> = (0..n).map(|k| (k, false)).collect();
        profile.regions.push(ProfileRegion {
            outer: walk.clone(),
            holes: Vec::new(),
            area: 1.0,
        });
        let mut scratch = Topo::new();
        let body = sweep::extrude(
            &mut scratch,
            "prism",
            &profile,
            &[walk],
            true,
            heights,
            Make::Solid,
        )
        .expect("a prism");
        read(&scratch, body).expect("a solid")
    }

    /// The edges of `solid` through `points`.
    fn edges_at(solid: &Solid, points: &[Point3]) -> Vec<usize> {
        points
            .iter()
            .map(|p| {
                (0..solid.edges.len())
                    .find(|&e| {
                        let edge = &solid.edges[e];
                        !workpiece_boolean::geometry::on_curve(&edge.curve, edge.range, *p, 1e-7)
                            .is_empty()
                    })
                    .expect("an edge there")
            })
            .collect()
    }

    fn round(radius: f64) -> Shape {
        Shape::Fillet {
            by: By::Radius,
            size: radius,
            until: None,
            section: Section::Circular,
            rho: 0.5,
        }
    }

    /// An L: arms `t` wide, `a` long in x and `b` long in y.
    fn ell(a: f64, b: f64, t: f64, h: f64) -> Solid {
        let p = Point2::new;
        prism(
            &[p(0.0, 0.0), p(a, 0.0), p(a, t), p(t, t), p(t, b), p(0.0, b)],
            h,
        )
    }

    #[test]
    fn a_ball_turns_about_the_end_of_an_inside_edge() -> anyhow::Result<()> {
        let (a, b, t, h, r) = (6.0, 9.0, 3.0, 2.5, 0.6);
        let solid = ell(a, b, t, h);
        let edges = edges_at(&solid, &[Point3::new(4.5, t, h), Point3::new(t, 6.0, h)]);
        let made = blend_chains(&solid, "fillet", &edges, &round(r), true)?;
        // Each round as long as its edge, and between them what is
        // outside a ball turned a quarter about the inside edge's end.
        let whole = (a * t + t * (b - t)) * h;
        let rounds = (1.0 - PI / 4.0) * r * r * ((a - t) + (b - t));
        let turn = PI / 2.0 * r.powi(3) * (5.0 / 6.0 - PI / 4.0);
        let volume = measure(&made).volume;
        assert!(
            (volume - (whole - rounds - turn)).abs() < 1e-9,
            "{volume} against {}",
            whole - rounds - turn
        );
        Ok(())
    }

    /// The face of `solid` through `p`.
    fn face_at(solid: &Solid, p: Point3) -> usize {
        (0..solid.faces.len())
            .find(|&f| workpiece_boolean::lies_on(solid, f, p, 1e-7))
            .expect("a face there")
    }

    #[test]
    fn blends_wider_together_than_their_face_are_refused() -> anyhow::Result<()> {
        // Both upright edges of a plate's end, 3 wide.
        let p = Point2::new;
        let solid = prism(&[p(0.0, 0.0), p(3.0, 0.0), p(3.0, 9.0), p(0.0, 9.0)], 3.5);
        let edges = edges_at(
            &solid,
            &[Point3::new(3.0, 9.0, 2.0), Point3::new(0.0, 9.0, 2.0)],
        );
        assert!(blend_chains(&solid, "fillet", &edges, &round(1.4), true).is_ok());
        let wide = blend_chains(&solid, "fillet", &edges, &round(1.51), true);
        assert!(wide.is_err_and(|e| format!("{e:#}").contains("wider together")));
        Ok(())
    }

    #[test]
    fn an_inside_edge_is_rounded_before_the_outside_ones_it_meets() -> anyhow::Result<()> {
        let (a, b, t, h, r) = (10.0, 12.0, 4.0, 5.0, 0.5);
        let solid = ell(a, b, t, h);
        let edges = edges_at(
            &solid,
            &[
                Point3::new(7.0, t, h),
                Point3::new(t, 9.0, h),
                Point3::new(t, t, 2.0),
            ],
        );
        let made = blend_chains(&solid, "fillet", &edges, &round(r), true)?;
        // Away from the corner each edge is rounded as it is alone; at
        // it, the top's round runs over the inside edge's.
        let strip = (1.0 - PI / 4.0) * r * r;
        let about = (a * t + t * (b - t)) * h - strip * ((a - t) + (b - t)) + strip * h;
        let volume = measure(&made).volume;
        assert!(
            (volume - about).abs() < r.powi(3),
            "{volume} against {about}"
        );
        let holds = |x: f64, y: f64, z: f64| workpiece_boolean::holds(&made, Point3::new(x, y, z));
        // In the inside edge's round, below the top's.
        assert_eq!(holds(t + 0.2 * r, t + 0.2 * r, h - 1.5 * r), Some(true));
        // Over it, what the top's round takes all the way around.
        assert_eq!(holds(t + 0.2 * r, t + 0.2 * r, h - 0.01 * r), Some(false));
        assert_eq!(holds(7.0, t - 0.05 * r, h - 0.05 * r), Some(false));
        Ok(())
    }

    /// A 10 by 4 block, 5 tall, with its corner at (0, 4) cut off `c`
    /// back along both sides.
    fn cut_corner(c: f64) -> Solid {
        let p = Point2::new;
        prism(
            &[
                p(0.0, 0.0),
                p(10.0, 0.0),
                p(10.0, 4.0),
                p(c, 4.0),
                p(0.0, 4.0 - c),
            ],
            5.0,
        )
    }

    #[test]
    fn a_round_takes_a_face_too_narrow_for_it() -> anyhow::Result<()> {
        // Both edges of a corner cut off by a hair: the corner, rounded.
        let (c, r) = (0.0026, 0.58);
        let solid = cut_corner(c);
        let edges = edges_at(
            &solid,
            &[Point3::new(c, 4.0, 2.0), Point3::new(0.0, 4.0 - c, 2.0)],
        );
        let made = blend_chains(&solid, "fillet", &edges, &round(r), true)?;
        let sharp = 10.0 * 4.0 * 5.0 - (1.0 - PI / 4.0) * r * r * 5.0;
        let volume = measure(&made).volume;
        assert!((volume - sharp).abs() < 1e-9, "{volume} against {sharp}");
        // A cut too wide for the ball to bridge keeps a strip between
        // two rounds.
        let solid = cut_corner(1.0);
        let edges = edges_at(
            &solid,
            &[Point3::new(1.0, 4.0, 2.0), Point3::new(0.0, 3.0, 2.0)],
        );
        let made = blend_chains(&solid, "fillet", &edges, &round(0.2), true)?;
        assert!(measure(&made).volume < 10.0 * 4.0 * 5.0 - 0.5 * 5.0);
        Ok(())
    }

    #[test]
    fn a_round_between_two_faces_takes_the_face_between() -> anyhow::Result<()> {
        let (c, r) = (0.1, 0.58);
        let solid = cut_corner(c);
        let faces = [
            face_at(&solid, Point3::new(5.0, 4.0, 2.0)),
            face_at(&solid, Point3::new(0.0, 2.0, 2.0)),
        ];
        let made = blend_faces(&solid, "fillet", faces, &round(r))?;
        let sharp = 10.0 * 4.0 * 5.0 - (1.0 - PI / 4.0) * r * r * 5.0;
        let volume = measure(&made).volume;
        assert!((volume - sharp).abs() < 1e-9, "{volume} against {sharp}");
        // A ball too small to reach across is refused.
        assert!(blend_faces(&solid, "fillet", faces, &round(0.1)).is_err());
        Ok(())
    }

    #[test]
    fn a_full_round_touches_three_faces() -> anyhow::Result<()> {
        // A rib narrowing upward: 2·b wide below, 2·t above, h tall.
        let (b, t, h, long) = (1.5_f64, 1.0_f64, 4.0_f64, 6.0);
        let p = Point2::new;
        let solid = prism(&[p(-b, 0.0), p(b, 0.0), p(t, h), p(-t, h)], long);
        let side = |s: f64| face_at(&solid, Point3::new(s * (b + t) / 2.0, h / 2.0, 3.0));
        let top = face_at(&solid, Point3::new(0.0, h, 3.0));
        let made = full_round(&solid, "round", [side(1.0), side(-1.0)], top)?;
        // The circle that touches the top in its middle and both sides.
        let lean = b - t;
        let rho = h * t / (lean.hypot(h) - lean);
        let cut = t * rho - rho * rho * (t / rho).atan();
        let volume = measure(&made).volume;
        let wanted = ((b + t) * h - 2.0 * cut) * long;
        assert!((volume - wanted).abs() < 1e-6, "{volume} against {wanted}");
        Ok(())
    }

    #[test]
    fn a_face_taken_off_is_closed_over_and_a_notch_moved() -> anyhow::Result<()> {
        // A corner cut off, put back: the faces beside the cut run on.
        let solid = cut_corner(1.0);
        let cut = face_at(&solid, Point3::new(0.5, 3.5, 2.5));
        let whole = crate::heal::faces_deleted(&solid, &[cut])?;
        let volume = measure(&whole).volume;
        assert!((volume - 200.0).abs() < 1e-9, "{volume}");
        // A notch through a side, moved along it by more than it is
        // wide: where it was is whole again.
        let p = Point2::new;
        let notched = prism(
            &[
                p(0.0, 0.0),
                p(10.0, 0.0),
                p(10.0, 4.0),
                p(4.0, 4.0),
                p(4.0, 3.0),
                p(2.0, 3.0),
                p(2.0, 4.0),
                p(0.0, 4.0),
            ],
            5.0,
        );
        let faces = [(2.0, 3.5), (4.0, 3.5), (3.0, 3.0)]
            .map(|(x, y)| face_at(&notched, Point3::new(x, y, 2.5)));
        let moved = crate::thick::faces_moved(&notched, &faces, Point3::X * 4.5)?;
        let placer = workpiece_boolean::Placer::new(&moved);
        let side = |x: f64| placer.side(Point3::new(x, 3.5, 2.5), 1e-9);
        assert_eq!(side(3.0), Some(workpiece_boolean::Side::In));
        assert_eq!(side(7.5), Some(workpiece_boolean::Side::Out));
        let volume = measure(&moved).volume;
        assert!((volume - 190.0).abs() < 1e-9, "{volume}");
        Ok(())
    }

    #[test]
    fn a_corner_is_set_back_and_rounds_of_three_sizes_meet_in_a_ball() -> anyhow::Result<()> {
        let sides = [12.0, 13.0, 14.0];
        let solid = cuboid(
            "box",
            Point3::ZERO,
            [Point3::X, Point3::Y, Point3::Z],
            sides,
        )?;
        let corner = Point3::from_array(sides);
        let its: Vec<Point3> = (0..3)
            .map(|axis| {
                let mut middle = corner;
                middle[axis] /= 2.0;
                middle
            })
            .collect();
        let edges = edges_at(&solid, &its);
        let full: f64 = sides.iter().product();
        let along = |r: f64, from: f64| -> f64 {
            sides
                .iter()
                .map(|l| (1.0 - PI / 4.0) * r * r * (l - from))
                .sum()
        };
        // Set back: outside the corner's cube the rounds as they are
        // alone; on each face, the flat stops short of where their two
        // rails would cross.
        let (r, back) = (1.0, 3.0);
        let made = blend_set_back(&solid, "fillet", &edges, &round(r), back)?;
        let volume = measure(&made).volume;
        let least = full - along(r, back) - back.powi(3);
        assert!(least < volume && volume < full - along(r, back), "{volume}");
        let placer = workpiece_boolean::Placer::new(&made);
        let at = |p: Point3| placer.side(p, 1e-9);
        assert_eq!(
            at(corner - Point3::new(r, r, 1e-4)),
            Some(workpiece_boolean::Side::Out)
        );
        assert_eq!(
            at(corner - Point3::new(4.0, 0.2, 0.2)),
            Some(workpiece_boolean::Side::Out)
        );
        assert_eq!(
            at(corner - Point3::new(4.0, 0.3, 0.3)),
            Some(workpiece_boolean::Side::In)
        );
        // Of three sizes: between what the largest leaves all round and
        // what the smallest does.
        let sizes = [1.0, 1.3, 2.0];
        let blends: Vec<(usize, Shape)> = (0..3).map(|k| (edges[k], round(sizes[k]))).collect();
        let made = blend_each(&solid, "fillet", &blends)?;
        let all_at = |r: f64| full - along(r, r) - r.powi(3) * (1.0 - PI / 6.0);
        let volume = measure(&made).volume;
        assert!(all_at(2.0) < volume && volume < all_at(1.0), "{volume}");
        // (And no crease where they meet: a ball closes the corner.)
        let placer = workpiece_boolean::Placer::new(&made);
        let on_ball = corner - Point3::splat(2.0) + Point3::splat(2.0 / 3.0_f64.sqrt());
        assert_eq!(
            placer.side(on_ball, 1e-7),
            Some(workpiece_boolean::Side::On)
        );
        Ok(())
    }

    #[test]
    fn a_knife_edge_is_rounded() -> anyhow::Result<()> {
        // A wedge that closes at 2.5°: the ball rests far from its edge.
        let (long, thick, h, r) = (8.0_f64, 0.35_f64, 4.0, 0.05_f64);
        let p = Point2::new;
        let solid = prism(&[p(0.0, 0.0), p(long, 0.0), p(long, thick)], h);
        let edges = edges_at(&solid, &[Point3::new(0.0, 0.0, 1.5)]);
        let made = blend_chains(&solid, "fillet", &edges, &round(r), true)?;
        let angle = (thick / long).atan();
        let taken = r * r * (1.0 / (angle / 2.0).tan() - (PI - angle) / 2.0) * h;
        let volume = measure(&made).volume;
        let wanted = long * thick / 2.0 * h - taken;
        assert!((volume - wanted).abs() < 1e-9, "{volume} against {wanted}");
        Ok(())
    }

    #[test]
    fn an_edge_all_but_flat_is_rounded_as_closely_as_any() -> anyhow::Result<()> {
        // A roof that rises 0.02 over 4.7 to each side, rounded by a
        // ball of 392: the round is 3 wide and takes next to nothing.
        let (wide, low, high, tall, r) = (9.45, 6.827, 6.8455, 3.0, 392.0_f64);
        let p = Point2::new;
        let outline = [
            p(0.0, 0.0),
            p(wide, 0.0),
            p(wide, low),
            p(wide / 2.0, high),
            p(0.0, low),
        ];
        let solid = prism(&outline, tall);
        let edges = edges_at(&solid, &[Point3::new(wide / 2.0, high, 0.8)]);
        let made = blend_chains(&solid, "fillet", &edges, &round(r), true)?;
        let turn = 2.0 * ((high - low) / (wide / 2.0)).atan();
        let taken = r * r * ((turn / 2.0).tan() - turn / 2.0) * tall;
        let got = measure(&solid).volume - measure(&made).volume;
        assert!((got - taken).abs() < 1e-9, "{got} against {taken}");
        Ok(())
    }

    /// Of `solid` made into a body: whether it is sound, and its volume.
    fn sound(solid: &Solid) -> f64 {
        let mut topo = Topo::new();
        workpiece_boolean::write(&mut topo, solid, "it").expect("written");
        assert_eq!(topo.validate(), Ok(()));
        measure(solid).volume
    }

    #[test]
    fn a_round_ends_on_faces_at_a_slant() -> anyhow::Result<()> {
        // A block leaning far over (its ends at 72° to its long edges):
        // rounding a prism's edge takes (1 - π/4) r² along it, and a
        // parallelogram's ends, cut at a slant, keep it as long as the
        // edge.
        let (long, wide, lean, h, r) = (10.0, 3.0, 9.0, 4.0, 0.7);
        let p = Point2::new;
        let outline = |w: f64| {
            [
                p(0.0, 0.0),
                p(long, 0.0),
                p(long + lean * w / wide, w),
                p(lean * w / wide, w),
            ]
        };
        let solid = prism(&outline(wide), h);
        let edges = edges_at(&solid, &[Point3::new(long / 2.0, 0.0, h)]);
        let made = blend_chains(&solid, "fillet", &edges, &round(r), true)?;
        let strip = (1.0 - PI / 4.0) * r * r * long;
        let volume = sound(&made);
        let wanted = long * wide * h - strip;
        assert!((volume - wanted).abs() < 1e-7, "{volume} against {wanted}");
        // An inside edge along a step of it, ending on the same slanted
        // ends: as much is added.
        let (w, high) = (1.2, 6.5);
        let step = combine(Op::Union, &solid, &prism(&outline(w), high))?.solid;
        let edges = edges_at(&step, &[Point3::new(lean * w / wide + long / 2.0, w, h)]);
        let made = blend_chains(&step, "fillet", &edges, &round(r), true)?;
        let volume = sound(&made);
        let wanted = long * wide * h + long * w * (high - h) + strip;
        assert!((volume - wanted).abs() < 1e-7, "{volume} against {wanted}");
        Ok(())
    }

    #[test]
    fn a_ball_set_in_a_hollow_wall_is_rounded_in() -> anyhow::Result<()> {
        // A block whose inside is a hollow wall (an arc about (0, 155.95),
        // run along z), with a ball on its top edge; the edge where the
        // ball comes out of the wall rounded. It ends on the block's
        // sides: at a slant, the block 32 thick; where the ball touches
        // them, 40 thick (its round fades out there). What is added is
        // the rolling ball's, worked out by sampling it (from its
        // centre's path: on a ball r larger than the ball's, and on a
        // cylinder r smaller than the wall's; ten million points, good
        // to about 1%).
        let p = Point2::new;
        let centre = p(0.0, 155.95);
        let low = 151.8;
        let wall = (37.5_f64).hypot(188.0 - centre.y);
        let a = p(
            centre.x + (wall * wall - (low - centre.y).powi(2)).sqrt(),
            low,
        );
        for (thick, r, sampled) in [(32.0, 3.0, 86.5), (40.0, 3.0, 120.1), (32.0, 12.0, 1060.8)] {
            let block = prism_arcs(
                &[a, p(57.5, low), p(57.5, 188.0), p(37.5, 188.0)],
                &[(3, centre)],
                thick,
            );
            // (Its poles turned away from the block's sides: up and
            // down.)
            let middle = Point3::new(37.5, 188.0, thick / 2.0);
            let sphere = ball("ball", middle, 20.0)?.moved(&|p: Point3| {
                let d = p - middle;
                middle + Point3::new(d.x, -d.z, d.y)
            })?;
            let solid = combine(Op::Union, &block, &sphere)?.solid;
            // The edge between the ball and the wall: through its lowest
            // point, halfway across.
            let near = Point3::new(47.2, 170.45, thick / 2.0);
            let edge = (0..solid.edges.len())
                .min_by(|&i, &j| {
                    let off = |e: usize| {
                        let edge = &solid.edges[e];
                        (0..=32)
                            .map(|k| {
                                let t = edge.range.0
                                    + (edge.range.1 - edge.range.0) * f64::from(k) / 32.0;
                                edge.point(t).distance(near)
                            })
                            .fold(f64::INFINITY, f64::min)
                    };
                    off(i).total_cmp(&off(j))
                })
                .expect("an edge");
            let made = blend_chains(&solid, "fillet", &[edge], &round(r), true)
                .with_context(|| format!("{thick} thick, r {r}"))?;
            let added = sound(&made) - measure(&solid).volume;
            assert!(
                (added - sampled).abs() < sampled * 0.02,
                "{thick} thick, r {r}: {added} against {sampled}"
            );
        }
        Ok(())
    }

    /// A grip: a rod of `radius` along x from `from` to `to`, with a
    /// half ball on its end at `to` (its centre on the line through
    /// (·, y, z) along x).
    fn grip(
        id: &str,
        (y, z): (f64, f64),
        from: f64,
        to: f64,
        radius: f64,
    ) -> anyhow::Result<Solid> {
        let centre = Point3::new(to, y, z);
        // The plane's y is along -x: the turn is about it.
        let plane = DatumPlane::new(centre, Point3::Z, Point3::Y)?;
        let mut profile = Profile::new(plane);
        let long = to - from;
        let pts = [
            Point2::new(0.0, -radius),
            Point2::new(radius, 0.0),
            Point2::new(radius, long),
            Point2::new(0.0, long),
        ];
        for (k, point) in pts.iter().enumerate() {
            profile.vertices.push(ProfileVertex {
                name: format!("v{k}"),
                point: *point,
                tolerance: 1e-9,
            });
        }
        let (form, knots) = crate::arcs::arc_form(PI / 2.0);
        let (controls, weights): (Vec<Point2>, Vec<f64>) = form
            .into_iter()
            .map(|(p, w)| (Point2::new(p.y, -p.x) * radius, w))
            .unzip();
        let curves = [
            Curve2::new(2, knots, controls, Some(weights))?,
            Curve2::line(pts[1], pts[2])?,
            Curve2::line(pts[2], pts[3])?,
            Curve2::line(pts[3], pts[0])?,
        ];
        for (k, curve) in curves.into_iter().enumerate() {
            let (t0, t1) = curve.domain();
            profile.edges.push(ProfileEdge {
                name: format!("e{k}"),
                entity: format!("e{k}"),
                span: (0.0, 1.0),
                curve,
                t0,
                t1,
                start: k,
                end: (k + 1) % 4,
                tolerance: 1e-9,
            });
        }
        let walk = vec![(0, false), (1, false), (2, false), (3, false)];
        profile.regions.push(ProfileRegion {
            outer: walk.clone(),
            holes: Vec::new(),
            area: PI * radius * radius / 4.0 + radius * long,
        });
        let mut scratch = Topo::new();
        let turn = Turn {
            origin: centre,
            axis: plane.y_dir(),
        };
        let body = sweep::revolve(
            &mut scratch,
            id,
            &profile,
            &[walk],
            true,
            turn,
            (0.0, TAU),
            Make::Solid,
        )?;
        Ok(read(&scratch, body)?)
    }

    /// The T handle's corner: a block whose inside is a hollow wall
    /// (an arc about (0, 155.95), run along z, 40 thick) fully rounded
    /// on its outer side, and a grip (a rod R20 along x ending in a half
    /// ball) on its top edge; and the edge where the ball meets the
    /// wall.
    fn t_corner() -> anyhow::Result<(Solid, usize)> {
        t_corner_by(0.0, 0.0, false)
    }

    /// [`t_corner`] moved by `shift` along x and y, with its grip moved
    /// by `off` more (what is tangent at the edge's ends then all but
    /// tangent), and if `mirrored`, mirrored in the plane x = 0.
    fn t_corner_by(shift: f64, off: f64, mirrored: bool) -> anyhow::Result<(Solid, usize)> {
        let p = |x: f64, y: f64| Point2::new(x + shift, y + shift);
        let centre = p(0.0, 155.95);
        let low = 151.8;
        let wall = (37.5_f64).hypot(188.0 - 155.95);
        let a = Point2::new(
            centre.x + (wall * wall - (low - 155.95_f64).powi(2)).sqrt(),
            low + shift,
        );
        let thick = 40.0;
        // (Rounded first, then cut to the wall: as wide as the round.)
        let slab = prism(
            &[p(20.0, low), p(57.5, low), p(57.5, 188.0), p(20.0, 188.0)],
            thick,
        );
        let outer = edges_at(
            &slab,
            &[
                Point3::new(57.5 + shift, 170.0 + shift, 0.0),
                Point3::new(57.5 + shift, 170.0 + shift, thick),
            ],
        );
        let slab = blend_solid(&slab, "round", &outer, &round(thick / 2.0), true)?;
        let top = 200.0;
        let b = Point2::new(
            centre.x + (wall * wall - (top - 155.95_f64).powi(2)).sqrt(),
            top + shift,
        );
        let past = prism_arcs(&[a, p(70.0, low), p(70.0, top), b], &[(3, centre)], thick);
        let block = combine(Op::Intersect, &slab, &past)?.solid;
        let rod = grip(
            "grip",
            (188.0 + shift + off, thick / 2.0),
            -20.0 + shift,
            37.5 + shift + off,
            20.0,
        )?;
        let solid = combine(Op::Union, &block, &rod)?.solid;
        let flip = |q: Point3| {
            if mirrored {
                Point3::new(-q.x, q.y, q.z)
            } else {
                q
            }
        };
        let solid = if mirrored {
            solid.mapped(&flip, true, 1.0)?
        } else {
            solid
        };
        let near = flip(Point3::new(47.2 + shift, 170.45 + shift, thick / 2.0));
        let edge = (0..solid.edges.len())
            .min_by(|&i, &j| {
                let off = |e: usize| {
                    let edge = &solid.edges[e];
                    (0..=32)
                        .map(|k| {
                            let t =
                                edge.range.0 + (edge.range.1 - edge.range.0) * f64::from(k) / 32.0;
                            edge.point(t).distance(near)
                        })
                        .fold(f64::INFINITY, f64::min)
                };
                off(i).total_cmp(&off(j))
            })
            .expect("an edge");
        Ok((solid, edge))
    }

    /// ∫ `f` from `a` to `b`, by Simpson's rule over `n` (even) steps.
    fn integral(f: &dyn Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
        let h = (b - a) / n as f64;
        let mut sum = f(a) + f(b);
        for k in 1..n {
            sum += f(a + h * k as f64) * if k % 2 == 1 { 4.0 } else { 2.0 };
        }
        sum * h / 3.0
    }

    #[test]
    fn a_round_ends_on_a_curved_face_at_a_slant() -> anyhow::Result<()> {
        // A block ending in a round face (a cylinder about z, of radius
        // `rc` around (`lc`, `cy`)), which its long edges meet at a
        // slant. Rounding the edge along y = 0 at the top takes, at each
        // y across it, its section's height there all the way from the
        // square end at x = 0 to the round end at that y.
        let p = Point2::new;
        let (lc, cy, rc, wide, h, r): (f64, f64, f64, f64, f64, f64) =
            (14.0, 5.0, 12.0, 8.0, 6.0, 1.5);
        let end = |y: f64| lc + (rc * rc - (y - cy).powi(2)).sqrt();
        let outline = |from: f64| {
            [
                p(0.0, from),
                p(end(from), from),
                p(end(wide), wide),
                p(0.0, wide),
            ]
        };
        let block = prism_turns(&outline(0.0), &[(1, p(lc, cy), true)], (0.0, h));
        let edges = edges_at(&block, &[Point3::new(5.0, 0.0, h)]);
        let made = blend_chains(&block, "fillet", &edges, &round(r), true)?;
        // (Over the angle the section's arc turns through: y = r - r sin a.)
        let quarter = std::f64::consts::FRAC_PI_2;
        let taken = integral(
            &|a| end(r - r * a.sin()) * (r - r * a.cos()) * r * a.cos(),
            0.0,
            quarter,
            2000,
        );
        let volume = sound(&made);
        let wanted = measure(&block).volume - taken;
        assert!((volume - wanted).abs() < 1e-6, "{volume} against {wanted}");
        // An inside edge along a step of it, ending on the same round end:
        // as much as the round's section is high is added, from the
        // square end to the round one.
        let (w, high) = (3.0, 10.0);
        let step = combine(
            Op::Union,
            &block,
            &prism_turns(&outline(w), &[(1, p(lc, cy), true)], (0.0, high)),
        )?
        .solid;
        let edges = edges_at(&step, &[Point3::new(5.0, w, h)]);
        let made = blend_chains(&step, "fillet", &edges, &round(r), true)?;
        let added = integral(
            &|a| end(w - r + r * a.sin()) * (r - r * a.cos()) * r * a.cos(),
            0.0,
            quarter,
            2000,
        );
        let volume = sound(&made);
        let wanted = measure(&step).volume + added;
        assert!((volume - wanted).abs() < 1e-6, "{volume} against {wanted}");
        Ok(())
    }

    #[test]
    fn a_round_fades_out_where_a_curved_face_touches_its_face() -> anyhow::Result<()> {
        // A boss (radius `rb`) on a lens (two disks of radius `rp`
        // overlapping), whose two round sides each touch the boss where
        // it stands on the lens' top: the boss' edge there is in two
        // halves, each ending where the boss and a side are tangent. A
        // half rounded takes what a ball rolled around the boss cannot
        // reach: as it nears the side, the ball leaves the lens' top
        // over its rim and rolls on that, touching the boss, until it
        // touches both where they meet — there is nothing to the round
        // there. (What a ball cannot reach, sampled as for REF_T, at
        // the centres of cubes of side 0.00625 and 0.01: as the cubes
        // get smaller it comes to more by a few parts in ten thousand,
        // where the round is thinner than they are, along its rails.)
        let p = Point2::new;
        let (rp, rb, h, high): (f64, f64, f64, f64) = (30.0, 8.0, 5.0, 12.0);
        let d = rp - rb;
        let xl = (rp * rp - d * d).sqrt();
        let lens = prism_turns(
            &[p(-xl, 0.0), p(xl, 0.0)],
            &[(0, p(0.0, d), true), (1, p(0.0, -d), true)],
            (0.0, h),
        );
        let boss = prism_turns(
            &[p(rb, 0.0), p(-rb, 0.0)],
            &[(0, Point2::ZERO, true), (1, Point2::ZERO, true)],
            (h, h + high),
        );
        let solid = combine(Op::Union, &lens, &boss)?.solid;
        // And inside out: the two as a hollow in a block, whose rounds
        // take away what those add (a ball rolled in the block's
        // material, around the hole).
        let block = prism_turns(
            &[
                p(-40.0, -40.0),
                p(40.0, -40.0),
                p(40.0, 40.0),
                p(-40.0, 40.0),
            ],
            &[],
            (-5.0, h + high),
        );
        let hollow = combine(Op::Cut, &block, &solid)?.solid;
        // (The half at x > 0, in the two pieces the boss' seams cut it
        // into.)
        let at = [
            Point3::new(rb * 0.6, rb * 0.8, h),
            Point3::new(rb * 0.6, -rb * 0.8, h),
        ];
        for (r, reached) in [(1.0, 4.2518), (2.0, 15.4152)] {
            let mut changes = Vec::new();
            for body in [&solid, &hollow] {
                let before = sound(body);
                let edges = edges_at(body, &at);
                let made = blend_solid(body, "fillet", &edges, &round(r), true)?;
                changes.push((sound(&made) - before).abs());
            }
            let [added, taken] = changes[..] else {
                unreachable!("two bodies")
            };
            assert!(
                (added - reached).abs() < reached * 1e-3,
                "r {r}: {added} against {reached}"
            );
            assert!(
                (taken - added).abs() < added * 1e-6,
                "r {r}: {taken} taken inside out against {added}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_rail_runs_on_by_a_smooth_seam() -> anyhow::Result<()> {
        // The T handle's corner: the edge where the grip's ball meets the
        // wall runs between the two points where the ball, the grip and
        // the block's full round are tangent. Its round's rail on the
        // ball runs onto the grip (smoothly on from it), across their
        // seam (a small round) or a fraction of a millimetre from it all
        // along (a large one). Towards the edge's ends its rail on the
        // wall reaches the wall's edge with the full round, and the ball
        // rolls on over that edge, touching the ball and the edge, until
        // the round is nothing where the three are tangent. What is
        // added is what a ball rolled anywhere outside the body cannot
        // reach (worked out by sampling it on a grid, the distances to
        // the body exact: REF_T).
        let (solid, edge) = t_corner()?;
        let before = sound(&solid);
        let p = Point2::new;
        for (r, reached, within) in REF_T {
            let made = blend_chains(&solid, "fillet", &[edge], &round(r), true)
                .with_context(|| format!("r {r}"))?;
            let added = sound(&made) - before;
            assert!(
                (added - reached).abs() < within,
                "r {r}: {added} against {reached}"
            );
            // Halfway along, where the ball is square to the plane of
            // symmetry, it adds as much across as a circle of its radius
            // tangent to the ball's and the wall's circles there does.
            if r == 3.0 {
                let dl = 0.02;
                let slab = prism_turns(
                    &[
                        p(0.0, 100.0),
                        p(100.0, 100.0),
                        p(100.0, 250.0),
                        p(0.0, 250.0),
                    ],
                    &[],
                    (20.0 - dl, 20.0 + dl),
                );
                let a = measure(&combine(Op::Intersect, &made, &slab)?.solid).volume;
                let b = measure(&combine(Op::Intersect, &solid, &slab)?.solid).volume;
                let across = (a - b) / (2.0 * dl);
                let wanted = section_t(r);
                assert!(
                    (across - wanted).abs() < wanted * 1e-5,
                    "r {r}: {across} across against {wanted}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_run_out_comes_to_nothing_at_its_end() -> anyhow::Result<()> {
        // The T handle's corner rounded R12: at each end the round comes
        // to nothing — its two rails one point — at the end and within a
        // hair of it (as far in as a sweep's end may be asked for), not
        // as far apart as the ball is found near there.
        let (solid, edge) = t_corner()?;
        let rolled = Rolled::of(&solid, edge, &round(12.0))?;
        for end in 0..2 {
            let cut = rolled.check_end(&solid, end, 1.0)?;
            assert!(
                cut.is_some_and(|c| c.over.is_some()),
                "end {end} rolls over"
            );
        }
        let (t0, t1) = rolled.range;
        for t in [
            t0,
            t0 + t0.abs().max(1.0) * 1e-15,
            t1,
            t1 - t1.abs() * 1e-15,
        ] {
            let station = rolled.station(t, None)?;
            let apart = station.rails[0].distance(station.rails[1]);
            assert!(apart == 0.0, "at {t}: the rails are {apart} apart");
        }
        Ok(())
    }

    #[test]
    fn a_run_out_does_not_hang_on_noise_or_handedness() -> anyhow::Result<()> {
        // The T handle's corner as above, its grip moved by a hair (what
        // is tangent at the edge's ends then all but tangent, as a part's
        // saved values have it), and mirrored: each adds as much as the
        // corner as it is, within a millionth.
        let r = 12.0;
        let added = |shift: f64, off: f64, mirrored: bool| -> anyhow::Result<f64> {
            let (solid, edge) = t_corner_by(shift, off, mirrored)?;
            let made = blend_chains(&solid, "fillet", &[edge], &round(r), true)
                .with_context(|| format!("shift {shift}, off {off}, mirrored {mirrored}"))?;
            Ok(sound(&made) - sound(&solid))
        };
        let wanted = added(0.0, 0.0, false)?;
        for mirrored in [false, true] {
            for (shift, off) in [
                (0.0, 0.0),
                (1e-12, 0.0),
                (-1e-12, 0.0),
                (1e-9, 0.0),
                (-1e-9, 0.0),
                (0.0, 1e-12),
                (0.0, -1e-12),
            ] {
                let got = added(shift, off, mirrored)?;
                assert!(
                    (got - wanted).abs() <= wanted * 1e-6,
                    "shift {shift}, off {off}, mirrored {mirrored}: {got} against {wanted}"
                );
            }
        }
        Ok(())
    }

    /// What a ball of radius `r` cannot reach of the corner of
    /// [`t_corner`] (mm³), and how far that may be off. Sampled on a
    /// grid over the half x > 0 of the corner: a point is added if no
    /// point within `r` of it is `r` from the body or more (the
    /// distances to the body exact, the nearest such point found
    /// exactly on the faces and edges set off by `r` and where two or
    /// three of those meet). r 3: the mean of eight grids of 0.2 set
    /// at random (each one's count is the volume on average), good to
    /// about 0.2; r 12: a grid of 0.2, where one of 0.4 came to 5 less,
    /// good to about 3.
    const REF_T: [(f64, f64, f64); 2] = [(3.0, 107.8, 0.6), (12.0, 1111.4, 4.0)];

    /// The added area across the corner of [`t_corner`] where its plane
    /// of symmetry cuts it: between the ball's circle (radius 20 about
    /// (37.5, 188)), the wall's (about (0, 155.95), through (37.5, 188))
    /// and the circle of radius `r` tangent to both — while that
    /// touches the ball's circle where it is the ball (x of 37.5 and
    /// more).
    fn section_t(r: f64) -> f64 {
        let (ball, b) = (Point2::new(37.5, 188.0), 20.0);
        let (wall, w) = (Point2::new(0.0, 155.95), (37.5_f64).hypot(188.0 - 155.95));
        // The two circles' points where a third's centre is `p` from the
        // first and `q` from the second: the one lower down.
        let meet = |p: f64, q: f64| {
            let d = ball.distance(wall);
            let along = (p * p - q * q + d * d) / (2.0 * d);
            let off = (p * p - along * along).sqrt();
            let u = (wall - ball) / d;
            let v = Point2::new(-u.y, u.x);
            let (x, y) = (ball + u * along + v * off, ball + u * along - v * off);
            if x.y < y.y { x } else { y }
        };
        let centre = meet(b + r, w - r);
        let corner = meet(b, w);
        let on = |c: Point2, rad: f64, q: Point2| c + (q - c).normalize() * rad;
        let (a, c) = (on(ball, b, centre), on(wall, w, centre));
        // The area of the outline: the ball's arc from the corner to the
        // round, the round's arc, the wall's arc back — each as the
        // triangle from the origin to its chord and the circle's
        // segment beyond the chord, with its sign.
        let piece = |centre: Point2, rad: f64, from: Point2, to: Point2| {
            let turn = (from - centre)
                .perp_dot(to - centre)
                .atan2((from - centre).dot(to - centre));
            from.perp_dot(to) / 2.0 + rad * rad * (turn - turn.sin()) / 2.0
        };
        (piece(ball, b, corner, a) + piece(centre, r, a, c) + piece(wall, w, c, corner)).abs()
    }

    #[test]
    fn a_rail_on_a_smooth_seam_is_rounded_as_on_its_face() -> anyhow::Result<()> {
        // A block with a round along its top's far edge, and a wall along
        // its near one: the inside edge at the wall's foot rounded so
        // that its rail on the top lands on the seam where the top runs
        // on into the round — a little short of it, on it, a hair to
        // either side of it, a little past it (where the top is all but
        // flat still). Each takes (1 - π/4) r² along the edge: the round
        // between two flat faces at a right angle.
        let p = Point2::new;
        let (long, wide, h, r0) = (10.0, 5.0, 3.0, 1.0);
        let block = prism(&[p(0.0, 0.0), p(long, 0.0), p(long, wide), p(0.0, wide)], h);
        let outer = edges_at(&block, &[Point3::new(long / 2.0, wide, h)]);
        let block = blend_solid(&block, "round", &outer, &round(r0), true)?;
        let wall = prism(
            &[p(0.0, -2.0), p(long, -2.0), p(long, 0.0), p(0.0, 0.0)],
            h + 6.0,
        );
        let solid = combine(Op::Union, &block, &wall)?.solid;
        let before = sound(&solid);
        let edges = edges_at(&solid, &[Point3::new(long / 2.0, 0.0, h)]);
        let seam = wide - r0;
        for r in [seam - 0.5, seam - 1e-3, seam - 1e-7, seam, seam + 1e-7] {
            let made = blend_chains(&solid, "fillet", &edges, &round(r), true)
                .with_context(|| format!("r {r}"))?;
            let added = sound(&made) - before;
            let wanted = (1.0 - PI / 4.0) * r * r * long;
            assert!(
                (added - wanted).abs() < 1e-6,
                "r {r}: {added} against {wanted}"
            );
        }
        Ok(())
    }
}
