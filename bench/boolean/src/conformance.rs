//! The parts of [step-conformance](https://github.com/dan335/step-conformance),
//! a published benchmark of STEP importers (its parts are CC0), read
//! with this kernel at its four tolerances: 0.0125, 0.05, 0.1 and 0.5 mm.
//! Its results for other readers are in its `results/RESULTS.md`; its
//! scorer is not run here, its rule is applied again:
//!
//! * every part has to import as solids that are valid and closed, with
//!   as many faces as the file has, and be meshed into triangles that
//!   close up, within 60 seconds;
//! * a part of known shape (those with a `.truth.json`: solids of
//!   revolution and plates with holes) has every vertex and every
//!   triangle's middle within four tolerances of the true surface, and
//!   every one of some 200,000 points of the true surface within four
//!   tolerances of the mesh. Stricter than the benchmark asks, the
//!   solid's own volume has to be the true one to a millionth.
//! * the freeform panel's truth is its surfaces: the same both ways,
//!   against its walls between its flat ends, its floor inside the
//!   inner wall, and the flat ends between the walls;
//! * the shop parts have no truth here (the benchmark's is another
//!   kernel's mesh): they are held to the first rule, and to the size
//!   the benchmark gives for them.
//!
//! `boolean-bench step-read OUT TOLERANCE FILE` is this kernel as a
//! reader for the benchmark's own scorer, as its README asks of one.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use serde::Deserialize;
use shapecraft_boolean::Solid;
use shapecraft_boolean::geometry::foot;
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::NurbsSurface;

use crate::mesh::triangles;
use crate::scene::sound;
use crate::{Case, Grade};

/// The set's name among the datasets.
pub const SET: &str = "step-conformance";

const TOLERANCES: [f64; 4] = [0.0125, 0.05, 0.1, 0.5];

/// About how many points of a true surface are looked at from the
/// surface to the mesh.
const SAMPLES: f64 = 200_000.0;

/// The parts without a truth file, with the size (in mm) the benchmark
/// gives for each, where it gives one.
const SHOP: [(&str, Option<[f64; 3]>); 8] = [
    ("hinge_jig", Some([25.4, 31.8, 33.3])),
    ("lego_4x2_01", Some([100.6, 49.8, 38.1])),
    ("door_handle_guide", Some([57.1, 215.9, 33.0])),
    ("rail_guide", Some([237.0, 40.6, 12.7])),
    ("lego_3_pins", Some([135.9, 164.1, 47.6])),
    ("sled_base", Some([237.0, 227.0, 76.2])),
    ("bowl_01", Some([380.0, 380.0, 20.0])),
    ("freeform_panel", None),
];

const KNOWN: [&str; 19] = [
    "cylinder",
    "cylinder_seam",
    "cylinder_nurbs",
    "cone_point",
    "cone_point_vertex",
    "dome",
    "dome_nurbs",
    "washer",
    "torus_ring",
    "torus_ring_nurbs",
    "cylinder_inch",
    "sheet_disc",
    "tiny_pin",
    "far_cylinder",
    "cylinder_placed",
    "cylinder_twice",
    "plate_100_holes",
    "plate_900_holes",
    "grooved_shaft",
];

/// A piece of an outline: a straight line or an arc of a circle.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Piece {
    Segment {
        from: [f64; 2],
        to: [f64; 2],
    },
    Arc {
        center: [f64; 2],
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

impl Piece {
    /// Its points, closely enough to tell what is inside it.
    fn points(&self) -> Vec<Point2> {
        const N: u32 = 256;
        match self {
            Piece::Segment { from, to } => vec![Point2::from_array(*from), Point2::from_array(*to)],
            Piece::Arc {
                center,
                radius,
                start,
                sweep,
            } => (0..=N)
                .map(|k| {
                    let a = start + sweep * f64::from(k) / f64::from(N);
                    Point2::from_array(*center) + Point2::from_angle(a) * *radius
                })
                .collect(),
        }
    }

    /// As a piece of a turned solid's outline (height, distance from
    /// the axis): its part of ∮ r² dh and of ∮ r ds, exactly.
    fn turned(&self) -> (f64, f64) {
        match self {
            Piece::Segment { from, to } => {
                let (p, q) = (Point2::from_array(*from), Point2::from_array(*to));
                (
                    (q.x - p.x) * (p.y * p.y + p.y * q.y + q.y * q.y) / 3.0,
                    p.distance(q) * (p.y + q.y) / 2.0,
                )
            }
            Piece::Arc {
                center,
                radius: r,
                start,
                sweep,
            } => {
                let (c, end) = (center[1], start + sweep);
                let between = |f: &dyn Fn(f64) -> f64| f(end) - f(*start);
                let sin1 = between(&|a| -a.cos());
                let sin2 = between(&|a| a / 2.0 - (2.0 * a).sin() / 4.0);
                let sin3 = between(&|a| -a.cos() + a.cos().powi(3) / 3.0);
                (
                    -r * (c * c * sin1 + 2.0 * c * r * sin2 + r * r * sin3),
                    (r * (c * sweep + r * sin1)).abs(),
                )
            }
        }
    }

    /// As a piece of a flat outline: its part of the area it goes
    /// around (signed), and its length, exactly.
    fn flat(&self) -> (f64, f64) {
        match self {
            Piece::Segment { from, to } => {
                let (p, q) = (Point2::from_array(*from), Point2::from_array(*to));
                (p.perp_dot(q) / 2.0, p.distance(q))
            }
            Piece::Arc {
                center,
                radius: r,
                start,
                sweep,
            } => {
                let end = start + sweep;
                let turn =
                    center[0] * (end.sin() - start.sin()) - center[1] * (end.cos() - start.cos());
                (r * turn / 2.0 + r * r * sweep / 2.0, (r * sweep).abs())
            }
        }
    }

    fn distance(&self, p: Point2) -> f64 {
        match self {
            Piece::Segment { from, to } => {
                let (a, b) = (Point2::from_array(*from), Point2::from_array(*to));
                let along =
                    ((p - a).dot(b - a) / (b - a).length_squared().max(1e-300)).clamp(0.0, 1.0);
                p.distance(a + (b - a) * along)
            }
            Piece::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let c = Point2::from_array(*center);
                // The angle of `p` from the arc's start, the way it goes.
                let way = sweep.signum();
                let turned = ((p - c).to_angle() - start) * way;
                let turned = turned.rem_euclid(TAU);
                if turned <= sweep.abs() + 1e-12 {
                    (p.distance(c) - radius).abs()
                } else {
                    let end = |a: f64| c + Point2::from_angle(a) * *radius;
                    p.distance(end(*start)).min(p.distance(end(start + sweep)))
                }
            }
        }
    }
}

/// A solid of revolution: its outline as (height along the axis,
/// distance from it).
#[derive(Deserialize)]
struct Turned {
    origin: [f64; 3],
    axis: [f64; 3],
    profile: Vec<Piece>,
}

/// A plate: outlines in the plane through `origin` across `axis` (the
/// first around it, the others its holes), `height` thick.
#[derive(Deserialize)]
struct Prism {
    origin: [f64; 3],
    axis: [f64; 3],
    x: [f64; 3],
    height: f64,
    loops: Vec<Vec<Piece>>,
}

/// A NURBS surface as the truth writes it.
#[derive(Deserialize)]
struct Patch {
    degree: [usize; 2],
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    points: Vec<Vec<[f64; 3]>>,
}

/// A flat end, across z at a height.
#[derive(Deserialize)]
struct Cap {
    z: f64,
}

/// A freeform part: the surfaces its walls and floor are on, and the
/// heights of its flat ends.
#[derive(Deserialize)]
struct Freeform {
    surfaces: Vec<Patch>,
    caps: Vec<Cap>,
    #[serde(default)]
    graphs: Vec<Graph>,
}

/// A surface that is a height over a rectangle of x and y.
#[derive(Deserialize)]
struct Graph {
    x: [f64; 2],
    y: [f64; 2],
}

#[derive(Deserialize, Default)]
struct Truth {
    #[serde(default)]
    solids: Vec<Turned>,
    #[serde(default)]
    prisms: Vec<Prism>,
    freeform: Option<Freeform>,
}

/// A freeform truth made ready: each surface with points of it to look
/// for a foot from.
struct Shaped {
    surfaces: Vec<(NurbsSurface, Vec<(Point2, Point3)>)>,
    caps: Vec<f64>,
    /// Each flat end: its height, the outer wall's outline there, and
    /// the inner wall's (empty where it does not reach it).
    ends: Vec<(f64, Vec<Point2>, Vec<Point2>)>,
}

/// How far `q` is from the closed polygon `points` (far, of none).
fn beside(points: &[Point2], q: Point2) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| {
            let along =
                ((q - *a).dot(*b - *a) / (*b - *a).length_squared().max(1e-300)).clamp(0.0, 1.0);
            q.distance(*a + (*b - *a) * along)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Where the wall `surface` is at the height `z`: its outline there,
/// around the axis.
fn level_outline(surface: &NurbsSurface, z: f64) -> Vec<Point2> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    (0..8192)
        .filter_map(|k| {
            let u = u0 + (u1 - u0) * f64::from(k) / 8192.0;
            let over = |v: f64| surface.point_at(u, v).z - z;
            let (mut lo, mut hi) = (v0, v1);
            if over(lo) > 0.0 || over(hi) < 0.0 {
                return None;
            }
            for _ in 0..60 {
                let mid = (lo + hi) / 2.0;
                if over(mid) < 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let p = surface.point_at(u, (lo + hi) / 2.0);
            Some(Point2::new(p.x, p.y))
        })
        .collect()
}

impl Shaped {
    fn of(truth: &Freeform) -> anyhow::Result<Self> {
        let mut surfaces = Vec::new();
        for patch in &truth.surfaces {
            let (nu, nv) = (patch.points.len(), patch.points.first().map_or(0, Vec::len));
            let surface = NurbsSurface::new(
                patch.degree[0],
                patch.degree[1],
                patch.u_knots.clone(),
                patch.v_knots.clone(),
                nu,
                nv,
                patch
                    .points
                    .iter()
                    .flatten()
                    .map(|p| Point3::from_array(*p))
                    .collect(),
                None,
            )?;
            let ((u0, u1), (v0, v1)) = surface.domain();
            let (across, along) = (4 * nu, 4 * nv);
            let mut seeds = Vec::with_capacity((across + 1) * (along + 1));
            for i in 0..=across {
                for j in 0..=along {
                    let uv = Point2::new(
                        u0 + (u1 - u0) * i as f64 / across as f64,
                        v0 + (v1 - v0) * j as f64 / along as f64,
                    );
                    seeds.push((uv, surface.point_at(uv.x, uv.y)));
                }
            }
            surfaces.push((surface, seeds));
        }
        let caps: Vec<f64> = truth.caps.iter().map(|c| c.z).collect();
        let ends = caps
            .iter()
            .map(|&z| {
                let at = |k: usize| {
                    surfaces
                        .get(k)
                        .map_or_else(Vec::new, |s: &(NurbsSurface, _)| level_outline(&s.0, z))
                };
                (z, at(0), at(1))
            })
            .collect();
        Ok(Shaped {
            surfaces,
            caps,
            ends,
        })
    }

    /// How far `p` is from the surfaces and the planes of the flat
    /// ends, or a distance within `limit` if one is found. `last` holds
    /// where on each surface the point before was found.
    fn distance(&self, p: Point3, limit: f64, last: &mut [Point2]) -> f64 {
        // A flat end is the plane within the outer wall's rim there,
        // and outside the inner wall's where that comes up to it.
        let mut least = self
            .ends
            .iter()
            .map(|(z, rim, hole)| {
                // (Too far from its plane to be within the limit.)
                if (p.z - z).abs() > limit {
                    return (p.z - z).abs();
                }
                let q = Point2::new(p.x, p.y);
                let on = inside(rim, q) && !(hole.len() > 2 && inside(hole, q));
                let off = if on {
                    0.0
                } else {
                    beside(rim, q).min(beside(hole, q))
                };
                off.hypot(p.z - z)
            })
            .fold(f64::INFINITY, f64::min);
        if least <= limit {
            return least;
        }
        for (k, (surface, _)) in self.surfaces.iter().enumerate() {
            let (uv, d) = foot(surface, p, last[k]);
            if d <= limit {
                last[k] = uv;
                return d;
            }
            least = least.min(d);
        }
        // Not near where the point before was: from the nearest of the
        // points kept of each surface.
        for (k, (surface, seeds)) in self.surfaces.iter().enumerate() {
            let Some(seed) = seeds
                .iter()
                .min_by(|a, b| a.1.distance_squared(p).total_cmp(&b.1.distance_squared(p)))
            else {
                continue;
            };
            let (mut uv, mut d) = foot(surface, p, seed.0);
            if d > limit {
                // Where that search stalls (a wall that turns tightly):
                // the nearest of a net of the surface's points around
                // the place, drawn ever closer.
                let ((u0, u1), (v0, v1)) = surface.domain();
                let same = |a: Point3, b: Point3| a.distance(b) <= 1e-9 * (1.0 + a.length());
                let closed = (
                    same(
                        surface.point_at(u0, (v0 + v1) / 2.0),
                        surface.point_at(u1, (v0 + v1) / 2.0),
                    ),
                    same(
                        surface.point_at((u0 + u1) / 2.0, v0),
                        surface.point_at((u0 + u1) / 2.0, v1),
                    ),
                );
                let mut reach = Point2::new(u1 - u0, v1 - v0) / 32.0;
                let (mut at, mut best) = (seed.0, seed.1.distance(p));
                for _ in 0..24 {
                    let centre = at;
                    for i in -6..=6 {
                        for j in -6..=6 {
                            // (Around, where the surface closes.)
                            let within = |x: f64, lo: f64, hi: f64, closed: bool| {
                                if closed {
                                    lo + (x - lo).rem_euclid(hi - lo)
                                } else {
                                    x.clamp(lo, hi)
                                }
                            };
                            let q = Point2::new(
                                within(centre.x + reach.x * f64::from(i) / 6.0, u0, u1, closed.0),
                                within(centre.y + reach.y * f64::from(j) / 6.0, v0, v1, closed.1),
                            );
                            let far = surface.point_at(q.x, q.y).distance(p);
                            if far < best {
                                (at, best) = (q, far);
                            }
                        }
                    }
                    reach *= 0.5;
                }
                if best < d {
                    (uv, d) = (at, best);
                }
            }
            if d < least {
                (least, last[k]) = (d, uv);
            }
            if least <= limit {
                break;
            }
        }
        least
    }
}

/// The box around points, and how far `p` is from it at least.
#[derive(Clone, Copy)]
struct Box2(Point2, Point2);

impl Box2 {
    fn of(points: &[Point2]) -> Self {
        let mut b = Box2(Point2::INFINITY, Point2::NEG_INFINITY);
        for p in points {
            b = Box2(b.0.min(*p), b.1.max(*p));
        }
        b
    }

    fn away(&self, p: Point2) -> f64 {
        (self.0 - p).max(p - self.1).max(Point2::ZERO).length()
    }

    fn holds(&self, p: Point2) -> bool {
        self.away(p) == 0.0
    }
}

/// An outline made ready: each piece with its box, and the whole as a
/// polygon with its.
struct Outline {
    pieces: Vec<Box2>,
    polygon: Vec<Point2>,
    around: Box2,
}

impl Outline {
    fn of(pieces: &[Piece]) -> Self {
        let polygon = outline(pieces);
        Outline {
            pieces: pieces.iter().map(|p| Box2::of(&p.points())).collect(),
            around: Box2::of(&polygon),
            polygon,
        }
    }

    /// How far `p` is from `pieces` (this outline's), if nearer than
    /// `least`.
    fn distance(&self, pieces: &[Piece], p: Point2, mut least: f64, turned: bool) -> f64 {
        if self.around.away(p) >= least {
            return least;
        }
        for (piece, b) in pieces.iter().zip(&self.pieces) {
            // (Of a turned solid's outline, what lies along the axis
            // is no surface.)
            if !(turned && piece.on_axis()) && b.away(p) < least {
                least = least.min(piece.distance(p));
            }
        }
        least
    }
}

fn outline(pieces: &[Piece]) -> Vec<Point2> {
    pieces.iter().flat_map(Piece::points).collect()
}

/// Whether `p` is inside the closed polygon `points`.
fn inside(points: &[Point2], p: Point2) -> bool {
    let mut odd = false;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            odd = !odd;
        }
    }
    odd
}

impl Truth {
    /// The volume and the area of the surface.
    fn measures(&self) -> (f64, f64) {
        let (mut volume, mut area) = (0.0, 0.0);
        for solid in &self.solids {
            let (mut v, mut a) = (0.0, 0.0);
            for piece in &solid.profile {
                let (dv, da) = piece.turned();
                (v, a) = (v + dv, a + da);
            }
            volume += PI * v.abs();
            area += TAU * a;
        }
        for prism in &self.prisms {
            let (mut inner, mut around) = (0.0, 0.0);
            for (k, lp) in prism.loops.iter().enumerate() {
                let (mut signed, mut length) = (0.0, 0.0);
                for piece in lp {
                    let (ds, dl) = piece.flat();
                    (signed, length) = (signed + ds, length + dl);
                }
                // The first is the outside, the others are holes.
                inner += if k == 0 { signed.abs() } else { -signed.abs() };
                around += length;
            }
            volume += inner * prism.height;
            area += 2.0 * inner + around * prism.height;
        }
        (volume, area)
    }

    /// The outlines made ready: of the solids of revolution, then of
    /// the plates.
    fn outlines(&self) -> (Vec<Outline>, Vec<Vec<Outline>>) {
        (
            self.solids
                .iter()
                .map(|s| Outline::of(&s.profile))
                .collect(),
            self.prisms
                .iter()
                .map(|p| p.loops.iter().map(|lp| Outline::of(lp)).collect())
                .collect(),
        )
    }

    /// How far `p` is from the surface.
    fn distance(&self, p: Point3, (turned, plates): &(Vec<Outline>, Vec<Vec<Outline>>)) -> f64 {
        let mut least = f64::INFINITY;
        for (solid, ready) in self.solids.iter().zip(turned) {
            let (o, z) = (
                Point3::from_array(solid.origin),
                Point3::from_array(solid.axis).normalize(),
            );
            let h = (p - o).dot(z);
            let r = (p - o - z * h).length();
            least = ready.distance(&solid.profile, Point2::new(h, r), least, true);
        }
        for (prism, ready) in self.prisms.iter().zip(plates) {
            let (o, z, x) = (
                Point3::from_array(prism.origin),
                Point3::from_array(prism.axis).normalize(),
                Point3::from_array(prism.x).normalize(),
            );
            let y = z.cross(x);
            let (at, h) = (Point2::new((p - o).dot(x), (p - o).dot(y)), (p - o).dot(z));
            let mut wall = f64::INFINITY;
            let mut around = 0;
            for (lp, ready) in prism.loops.iter().zip(ready) {
                wall = ready.distance(lp, at, wall, false);
                if ready.around.holds(at) && inside(&ready.polygon, at) {
                    around += 1;
                }
            }
            let within = around == 1;
            let beyond = (-h).max(h - prism.height).max(0.0);
            let d = match (within, beyond > 0.0) {
                (true, true) => beyond,
                (true, false) => wall.min(h).min(prism.height - h),
                (false, true) => wall.hypot(beyond),
                (false, false) => wall,
            };
            least = least.min(d);
        }
        least
    }
}

/// A box of a [`Tree`], and what is in it: two boxes, or a run of the
/// triangles.
type Node = (Point3, Point3, Result<[usize; 2], (usize, usize)>);

/// The triangles of a mesh in boxes within boxes, to ask whether a
/// point is near one of them.
struct Tree {
    nodes: Vec<Node>,
    triangles: Vec<[Point3; 3]>,
}

impl Tree {
    fn new(mut triangles: Vec<[Point3; 3]>) -> Self {
        let mut nodes = Vec::new();
        let n = triangles.len();
        Self::grow(&mut nodes, &mut triangles, 0, n);
        Tree { nodes, triangles }
    }

    fn grow(nodes: &mut Vec<Node>, triangles: &mut [[Point3; 3]], from: usize, to: usize) -> usize {
        let (mut lo, mut hi) = (Point3::INFINITY, Point3::NEG_INFINITY);
        for p in triangles[from..to].iter().flatten() {
            (lo, hi) = (lo.min(*p), hi.max(*p));
        }
        let at = nodes.len();
        nodes.push((lo, hi, Err((from, to))));
        if to - from > 8 {
            // Halved across the box's longest way, by the triangles'
            // middles.
            let size = (hi - lo).to_array();
            let axis = (0..3)
                .max_by(|a, b| size[*a].total_cmp(&size[*b]))
                .unwrap_or(0);
            let middle = |t: &[Point3; 3]| (t[0] + t[1] + t[2]).to_array()[axis];
            triangles[from..to].sort_by(|a, b| middle(a).total_cmp(&middle(b)));
            let half = (from + to) / 2;
            let left = Self::grow(nodes, triangles, from, half);
            let right = Self::grow(nodes, triangles, half, to);
            nodes[at].2 = Ok([left, right]);
        }
        at
    }

    /// Whether a triangle is within `limit` of `p`.
    fn near(&self, p: Point3, limit: f64) -> bool {
        let mut open = vec![0];
        while let Some(k) = open.pop() {
            let (lo, hi, inside) = &self.nodes[k];
            let away = (*lo - p).max(p - *hi).max(Point3::ZERO).length();
            if away > limit {
                continue;
            }
            match inside {
                Ok(both) => open.extend(both),
                Err((from, to)) => {
                    if self.triangles[*from..*to]
                        .iter()
                        .any(|t| nearest(p, t).distance(p) <= limit)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// The point of the triangle nearest to `p`.
fn nearest(p: Point3, t: &[Point3; 3]) -> Point3 {
    let [a, b, c] = *t;
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let sum = va + vb + vc;
    if sum.abs() < 1e-300 {
        return a;
    }
    a + ab * (vb / sum) + ac * (vc / sum)
}

impl Piece {
    /// Whether, as a piece of a turned solid's outline, it lies along
    /// the axis: the outline closes there, and sweeps no surface.
    fn on_axis(&self) -> bool {
        matches!(self, Piece::Segment { from, to } if from[1].abs() <= 1e-12 && to[1].abs() <= 1e-12)
    }

    /// Points along it, no further than `step` apart.
    fn along(&self, step: f64) -> Vec<Point2> {
        match self {
            Piece::Segment { from, to } => {
                let (a, b) = (Point2::from_array(*from), Point2::from_array(*to));
                let n = (a.distance(b) / step).ceil().max(1.0) as usize;
                (0..=n).map(|k| a.lerp(b, k as f64 / n as f64)).collect()
            }
            Piece::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let n = ((radius * sweep).abs() / step).ceil().max(1.0) as usize;
                (0..=n)
                    .map(|k| {
                        let a = start + sweep * k as f64 / n as f64;
                        Point2::from_array(*center) + Point2::from_angle(a) * *radius
                    })
                    .collect()
            }
        }
    }
}

impl Truth {
    /// Points of the true surface, no further than about `step` apart.
    fn samples(&self, step: f64, (_, plates): &(Vec<Outline>, Vec<Vec<Outline>>)) -> Vec<Point3> {
        let mut out = Vec::new();
        for solid in &self.solids {
            let (o, z) = (
                Point3::from_array(solid.origin),
                Point3::from_array(solid.axis).normalize(),
            );
            let (x, y) = z.any_orthonormal_pair();
            let swept = solid.profile.iter().filter(|piece| !piece.on_axis());
            for q in swept.flat_map(|piece| piece.along(step)) {
                let around = ((TAU * q.y / step).ceil() as usize).max(1);
                for k in 0..around {
                    let (sin, cos) = (TAU * k as f64 / around as f64).sin_cos();
                    out.push(o + z * q.x + (x * cos + y * sin) * q.y);
                }
            }
        }
        for (prism, ready) in self.prisms.iter().zip(plates) {
            let (o, z, x) = (
                Point3::from_array(prism.origin),
                Point3::from_array(prism.axis).normalize(),
                Point3::from_array(prism.x).normalize(),
            );
            let y = z.cross(x);
            let place = |q: Point2, h: f64| o + x * q.x + y * q.y + z * h;
            let up = (prism.height / step).ceil().max(1.0) as usize;
            for q in prism
                .loops
                .iter()
                .flatten()
                .flat_map(|piece| piece.along(step))
            {
                for k in 0..=up {
                    out.push(place(q, prism.height * k as f64 / up as f64));
                }
            }
            // The two flat sides: what is inside the first outline and
            // in no hole.
            let Some(outer) = ready.first() else {
                continue;
            };
            let (lo, hi) = (outer.around.0, outer.around.1);
            let across = ((hi - lo) / step).ceil().max(Point2::ONE);
            for i in 0..=across.x as usize {
                for j in 0..=across.y as usize {
                    let q = lo + (hi - lo) * Point2::new(i as f64, j as f64) / across;
                    let around = ready
                        .iter()
                        .filter(|lp| lp.around.holds(q) && inside(&lp.polygon, q))
                        .count();
                    if around == 1 {
                        out.extend([place(q, 0.0), place(q, prism.height)]);
                    }
                }
            }
        }
        out
    }
}

impl Shaped {
    /// Points of the panel's true faces, about `step` apart: of its
    /// outer wall between its flat ends, of its inner wall from its
    /// floor up, of the floor inside that wall, and of the two flat
    /// ends between the walls. (As the truth has it: the first surface
    /// is the outer wall, the second the inner, the third the floor, a
    /// height over the rectangle `floor`.)
    fn samples(&self, step: f64, truth: &Freeform) -> Vec<Point3> {
        let mut out = Vec::new();
        let ([outer, inner, floor], Some(graph)) = (&self.surfaces[..], truth.graphs.first())
        else {
            return out;
        };
        let (bottom, top) = self
            .caps
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |m, z| {
                (m.0.min(*z), m.1.max(*z))
            });
        // The floor's height under (or over) a point.
        let ((fu0, fu1), (fv0, fv1)) = floor.0.domain();
        let height = |p: Point3| {
            let u = (p.x - graph.x[0]) / (graph.x[1] - graph.x[0]);
            let v = (p.y - graph.y[0]) / (graph.y[1] - graph.y[0]);
            ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then(|| {
                floor
                    .0
                    .point_at(fu0 + (fu1 - fu0) * u, fv0 + (fv1 - fv0) * v)
                    .z
            })
        };
        // How long a surface is each way, roughly.
        let lengths = |surface: &NurbsSurface| {
            let ((u0, u1), (v0, v1)) = surface.domain();
            let run = |f: &dyn Fn(f64) -> Point3| {
                (0..256)
                    .map(|k| f(f64::from(k) / 256.0).distance(f(f64::from(k + 1) / 256.0)))
                    .sum::<f64>()
            };
            (
                run(&|f| surface.point_at(u0 + (u1 - u0) * f, (v0 + v1) / 2.0)),
                run(&|f| surface.point_at((u0 + u1) / 2.0, v0 + (v1 - v0) * f)),
            )
        };
        let net = |surface: &NurbsSurface, keep: &dyn Fn(Point3) -> bool, out: &mut Vec<Point3>| {
            let ((u0, u1), (v0, v1)) = surface.domain();
            let (long, high) = lengths(surface);
            let (nu, nv) = (
                (long * 1.5 / step).ceil().max(1.0) as usize,
                (high * 1.5 / step).ceil().max(1.0) as usize,
            );
            for i in 0..=nu {
                for j in 0..=nv {
                    let p = surface.point_at(
                        u0 + (u1 - u0) * i as f64 / nu as f64,
                        v0 + (v1 - v0) * j as f64 / nv as f64,
                    );
                    if keep(p) {
                        out.push(p);
                    }
                }
            }
        };
        net(&outer.0, &|p| p.z >= bottom && p.z <= top, &mut out);
        net(
            &inner.0,
            &|p| p.z <= top && height(p).is_some_and(|h| p.z >= h),
            &mut out,
        );
        // Where a wall is at a height (or at the floor): its outline
        // there, around the axis.
        let outline = |surface: &NurbsSurface, level: &dyn Fn(Point3) -> Option<f64>| {
            let ((u0, u1), (v0, v1)) = surface.domain();
            (0..2048)
                .filter_map(|k| {
                    let u = u0 + (u1 - u0) * f64::from(k) / 2048.0;
                    let over = |v: f64| {
                        let p = surface.point_at(u, v);
                        level(p).map(|z| p.z - z)
                    };
                    let (mut lo, mut hi) = (v0, v1);
                    if over(lo)? > 0.0 || over(hi)? < 0.0 {
                        return None;
                    }
                    for _ in 0..60 {
                        let mid = (lo + hi) / 2.0;
                        if over(mid)? < 0.0 {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    let p = surface.point_at(u, (lo + hi) / 2.0);
                    Some(Point2::new(p.x, p.y))
                })
                .collect::<Vec<Point2>>()
        };
        let pit = outline(&inner.0, &height);
        net(&floor.0, &|p| inside(&pit, Point2::new(p.x, p.y)), &mut out);
        // The flat ends: inside the outer wall there, and outside the
        // inner where it comes up to them.
        for &z in &self.caps {
            let rim = outline(&outer.0, &|_| Some(z));
            let hole = outline(&inner.0, &|_| Some(z));
            let b = Box2::of(&rim);
            let across = ((b.1 - b.0) / step).ceil().max(Point2::ONE);
            for i in 0..=across.x as usize {
                for j in 0..=across.y as usize {
                    let q = b.0 + (b.1 - b.0) * Point2::new(i as f64, j as f64) / across;
                    if inside(&rim, q) && !(hole.len() > 2 && inside(&hole, q)) {
                        out.push(Point3::new(q.x, q.y, z));
                    }
                }
            }
        }
        out
    }
}

fn dir() -> PathBuf {
    crate::datasets::data().join(SET)
}

/// A part as read: its solids and their triangles' closing up is all
/// looked at once per tolerance, the import once.
type Read = Result<(Vec<Solid>, usize), String>;

fn read(path: &Path) -> Read {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // What the file calls a face.
    let text = String::from_utf8_lossy(&bytes);
    let faces = text.matches("ADVANCED_FACE").count();
    let found = shapecraft_ops_step::import(&bytes).map_err(|e| format!("{e:#}"))?;
    if let Some(why) = found.refused.first() {
        return Err(why.clone());
    }
    if found.bodies.is_empty() {
        return Err("no solids".to_owned());
    }
    Ok((found.bodies.into_iter().map(|b| b.solid).collect(), faces))
}

/// Whether every side of every triangle is the side of one other, the
/// other way.
fn closes(mesh: &[[Point3; 3]]) -> bool {
    let key = |p: Point3| p.to_array().map(|x| (x + 0.0).to_bits());
    let mut sides: HashMap<([u64; 3], [u64; 3]), i64> = HashMap::new();
    for t in mesh {
        for k in 0..3 {
            let (a, b) = (key(t[k]), key(t[(k + 1) % 3]));
            if a < b {
                *sides.entry((a, b)).or_default() += 1;
            } else {
                *sides.entry((b, a)).or_default() -= 1;
            }
        }
    }
    sides.values().all(|n| *n == 0)
}

fn grade(name: &str, tolerance: f64, read: &Read, size: Option<[f64; 3]>) -> Grade {
    let clock = Instant::now();
    let (solids, faces) = match read {
        Ok(read) => read,
        Err(why) => return Grade::Refused(why.clone()),
    };
    let mut volume = 0.0;
    for solid in solids {
        match sound(solid) {
            Err(why) => return Grade::Wrong(why),
            Ok((v, _)) if v <= 0.0 => return Grade::Wrong(format!("a solid of volume {v}")),
            Ok((v, _)) => volume += v,
        }
    }
    // A file's solid placed twice is twice its faces.
    let got: usize = solids.iter().map(|s| s.faces.len()).sum();
    if got < *faces {
        return Grade::Wrong(format!("{got} faces of the file's {faces}"));
    }
    let mesh: Vec<[Point3; 3]> = solids
        .iter()
        .flat_map(|s| triangles(s, tolerance))
        .collect();
    if mesh.is_empty() || !closes(&mesh) {
        return Grade::Wrong("its triangles do not close up".to_owned());
    }
    if let Some(size) = size {
        let (mut lo, mut hi) = (Point3::INFINITY, Point3::NEG_INFINITY);
        for p in mesh.iter().flatten() {
            (lo, hi) = (lo.min(*p), hi.max(*p));
        }
        let got = (hi - lo).to_array();
        // (The benchmark gives the sizes to a tenth.)
        if (0..3).any(|k| (got[k] - size[k]).abs() > 0.06 + tolerance) {
            return Grade::Wrong(format!("{got:.1?} across, not {size:?}"));
        }
    }
    let truth = dir().join(format!("{name}.truth.json"));
    if let Ok(text) = std::fs::read_to_string(&truth) {
        let truth: Truth = match serde_json::from_str(&text) {
            Ok(truth) => truth,
            Err(e) => return Grade::Crashed(format!("setup: {}: {e}", truth.display())),
        };
        if let Some(freeform) = &truth.freeform {
            // No measures are known of it: every point of the mesh has
            // to be on one of its surfaces, or in the plane of one of
            // its flat ends.
            let shaped = match Shaped::of(freeform) {
                Ok(shaped) => shaped,
                Err(e) => return Grade::Crashed(format!("setup: {e:#}")),
            };
            let mut last = vec![Point2::ZERO; shaped.surfaces.len()];
            let mut worst = 0.0_f64;
            for t in &mesh {
                for p in [t[0], t[1], t[2], (t[0] + t[1] + t[2]) / 3.0] {
                    worst = worst.max(shaped.distance(p, 4.0 * tolerance, &mut last));
                }
            }
            if worst > 4.0 * tolerance {
                return Grade::Wrong(format!("{worst:.4} mm off the true surface"));
            }
            // And the other way: every point of the true faces has the
            // mesh within four tolerances.
            let area: f64 = mesh
                .iter()
                .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).length() / 2.0)
                .sum();
            let step = (2.0 * tolerance).max((area / SAMPLES).sqrt());
            let tree = Tree::new(mesh.clone());
            let samples = shaped.samples(step, freeform);
            if samples.is_empty() {
                return Grade::Crashed("setup: no points of the true faces".to_owned());
            }
            let missed = samples
                .iter()
                .filter(|p| !tree.near(**p, 4.0 * tolerance))
                .count();
            if missed > 0 {
                return Grade::Wrong(format!(
                    "{missed} of {} points of the true surface are further than that from the mesh",
                    samples.len()
                ));
            }
        }
        if truth.solids.is_empty() && truth.prisms.is_empty() {
            return Grade::Pass;
        }
        let (wanted, area) = truth.measures();
        if ((volume - wanted) / wanted).abs() > 1e-6 {
            return Grade::Wrong(format!("volume {volume} is not {wanted}"));
        }
        let outlines = truth.outlines();
        let mut worst = 0.0_f64;
        let mut meshed = 0.0;
        for t in &mesh {
            meshed += t[0].dot(t[1].cross(t[2])) / 6.0;
            for p in [t[0], t[1], t[2], (t[0] + t[1] + t[2]) / 3.0] {
                worst = worst.max(truth.distance(p, &outlines));
            }
        }
        if worst > 4.0 * tolerance {
            return Grade::Wrong(format!("{worst:.4} mm off the true surface"));
        }
        if (meshed - wanted).abs() > area * 4.0 * tolerance {
            return Grade::Wrong(format!("the mesh holds {meshed}, not {wanted}"));
        }
        // And the other way: every point of the true surface has the
        // mesh within four tolerances.
        let step = (2.0 * tolerance).max((area / SAMPLES).sqrt());
        let tree = Tree::new(mesh.clone());
        let samples = truth.samples(step, &outlines);
        let missed = samples
            .iter()
            .filter(|p| !tree.near(**p, 4.0 * tolerance))
            .count();
        if missed > 0 {
            return Grade::Wrong(format!(
                "{missed} of {} points of the true surface are further than that from the mesh",
                samples.len()
            ));
        }
    }
    if clock.elapsed().as_secs_f64() > 60.0 {
        return Grade::Wrong("more than 60 seconds".to_owned());
    }
    Grade::Pass
}

/// The benchmark's cells: every part at every tolerance.
pub fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    let parts = SHOP
        .iter()
        .map(|(name, size)| (*name, *size))
        .chain(KNOWN.iter().map(|name| (*name, None)));
    for (name, size) in parts {
        let path = dir().join(format!("{name}.step"));
        if !path.exists() {
            continue;
        }
        let cell: Arc<OnceLock<Read>> = Arc::new(OnceLock::new());
        for tolerance in TOLERANCES {
            let (cell, path) = (cell.clone(), path.clone());
            out.push(Case {
                category: SET,
                name: format!("{name}@{tolerance}"),
                run: Box::new(move || {
                    let clock = Instant::now();
                    let read = cell.get_or_init(|| read(&path));
                    let grade = grade(name, tolerance, read, size);
                    if clock.elapsed().as_secs_f64() > 60.0 {
                        return Grade::Wrong("more than 60 seconds".to_owned());
                    }
                    grade
                }),
            });
        }
    }
    out
}

/// This kernel as a reader for the benchmark's scorer: reads `file`,
/// meshes it at `tolerance` and writes `<out>/<stem>.bin` (little
/// endian: the number of vertices and each as three f64 in mm; the
/// number of triangles and each as three u32; the number of faces and
/// of each its number of triangles, the triangles being in the order of
/// the faces), with how long reading and meshing took in
/// `<out>/<stem>.time`. What goes wrong is written to `<out>/<stem>.err`.
pub fn adapter(out: &Path, tolerance: f64, file: &Path) -> std::io::Result<()> {
    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
    let clock = Instant::now();
    let solids = match read(file) {
        Ok((solids, _)) => solids,
        Err(why) => return std::fs::write(out.join(format!("{stem}.err")), why),
    };
    let reading = clock.elapsed().as_secs_f64();
    let clock = Instant::now();
    let faces: Vec<Vec<[Point3; 3]>> = solids
        .iter()
        .flat_map(|s| crate::mesh::by_face(s, tolerance))
        .collect();
    let meshing = clock.elapsed().as_secs_f64();
    let mut ids: HashMap<[u64; 3], u32> = HashMap::new();
    let mut vertices: Vec<Point3> = Vec::new();
    let mut corners: Vec<u32> = Vec::new();
    for p in faces.iter().flatten().flatten() {
        let id = *ids
            .entry(p.to_array().map(|x| (x + 0.0).to_bits()))
            .or_insert_with(|| {
                vertices.push(*p);
                (vertices.len() - 1) as u32
            });
        corners.push(id);
    }
    let mut bytes = Vec::new();
    bytes.extend((vertices.len() as u64).to_le_bytes());
    for x in vertices.iter().flat_map(|p| p.to_array()) {
        bytes.extend(x.to_le_bytes());
    }
    bytes.extend((corners.len() as u64 / 3).to_le_bytes());
    for id in corners {
        bytes.extend(id.to_le_bytes());
    }
    bytes.extend((faces.len() as u64).to_le_bytes());
    for face in &faces {
        bytes.extend((face.len() as u64).to_le_bytes());
    }
    std::fs::write(out.join(format!("{stem}.bin")), bytes)?;
    std::fs::write(
        out.join(format!("{stem}.time")),
        format!("read {reading}\nmesh {meshing}\n"),
    )
}
