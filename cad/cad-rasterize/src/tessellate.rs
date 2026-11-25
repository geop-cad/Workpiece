//! Sampling faces into triangles, for drawing and picking.
//!
//! A face is the region its loops' pcurves bound in (u, v): that region
//! is cut into triangles (ear by ear, holes joined to the outline first),
//! and the triangles are cut again along a grid, a fixed number of steps
//! per knot span where the surface curves, so they follow it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::nurbs::SEGMENTS_PER_SPAN;
use workpiece_core::world::topo::{BodyId, CoedgeKind, FaceId, NurbsSurface, Orientation, Topo};

/// The parameters to sample a direction of `surface` at (`along_v`: its
/// v, else its u): every distinct knot, and between two of them as many
/// evenly spaced ones as the surface turns there — a step for every
/// 192nd of a turn, and never more than [`FINEST`] to a span (a circle
/// of four spans gets 96). That is finer than outlines need: the light mirrored in a face
/// (and the stripes of a zebra analysis) follows the normals, which run
/// straight between the corners. (A sweep along a helix has dozens of
/// spans to a turn: a few steps in each are as round as many in a
/// quarter circle's.)
/// The most steps a knot span is sampled in.
const FINEST: usize = 2 * SEGMENTS_PER_SPAN;
/// The fewest steps a direction that bends is sampled in from side to
/// side, shared among its spans (each gets two at least).
const GENTLEST: usize = 48;

fn samples(surface: &NurbsSurface, along_v: bool) -> Vec<f64> {
    let (du, dv) = surface.degrees();
    let (ku, kv) = surface.knots();
    let (degree, knots, across_degree, across) = if along_v {
        (dv, kv, du, ku)
    } else {
        (du, ku, dv, kv)
    };
    let mut distinct: Vec<f64> = knots[degree..knots.len() - degree].to_vec();
    distinct.dedup();
    // Where across the surface the turning is looked at: a few places
    // from side to side.
    let (a0, a1) = (
        across[across_degree],
        across[across.len() - across_degree - 1],
    );
    let places = [0.02, 0.26, 0.5, 0.74, 0.98].map(|f| a0 + (a1 - a0) * f);
    let way = |t: f64, other: f64| {
        let (_, d_u, d_v) = if along_v {
            surface.derivatives_at(other, t)
        } else {
            surface.derivatives_at(t, other)
        };
        if along_v { d_v } else { d_u }
    };
    let mut out = Vec::new();
    for pair in distinct.windows(2) {
        let per_span = if degree <= 1 {
            1
        } else {
            let mut turned = 0.0_f64;
            for other in places {
                let at = |f: f64| way(pair[0] + (pair[1] - pair[0]) * f, other);
                let ways = [0.001, 0.25, 0.5, 0.75, 0.999].map(at);
                let all: f64 = ways.windows(2).map(|w| w[0].angle_between(w[1])).sum();
                if all.is_finite() {
                    turned = turned.max(all);
                }
            }
            let step = std::f64::consts::TAU / (8 * FINEST) as f64;
            // A span that bends at all gets a few steps however little
            // it turns: mirrored stripes show every corner of a nearly
            // flat face.
            let least = if turned > 1e-9 {
                (GENTLEST / (distinct.len() - 1)).max(2)
            } else {
                1
            };
            ((turned / step).ceil() as usize).clamp(least, FINEST.max(least))
        };
        for k in 0..per_span {
            out.push(pair[0] + (pair[1] - pair[0]) * k as f64 / per_span as f64);
        }
    }
    if let Some(&last) = distinct.last() {
        out.push(last);
    }
    out
}

/// A sampled face: grid triangles with a normal at each corner.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceMesh {
    /// The triangles' corners, and the unit normal at each.
    pub triangles: Vec<([Point3; 3], [Point3; 3])>,
}

/// The unit normal `∂u × ∂v` of `surface` at (u, v), by central
/// differences; at a collapsed point (a pole) it is taken a little way in.
pub fn normal(surface: &NurbsSurface, u: f64, v: f64) -> Point3 {
    Normals::of(surface).at(u, v)
}

/// The normals of a surface: what [`normal`] works out of the whole
/// surface, worked out once for all the points asked for (a thread's
/// face has tens of thousands of control points and as many corners).
pub struct Normals<'a> {
    surface: &'a NurbsSurface,
    /// Differences shorter than this are rounding noise: the surface
    /// collapses there. (A real one is about 2e-5 of the surface's size.)
    noise: f64,
}

impl<'a> Normals<'a> {
    /// Those of `surface`.
    pub fn of(surface: &'a NurbsSurface) -> Self {
        let (lo, hi) = surface
            .control_points()
            .iter()
            .fold((Point3::INFINITY, Point3::NEG_INFINITY), |(lo, hi), &p| {
                (lo.min(p), hi.max(p))
            });
        Self {
            surface,
            noise: (hi - lo).length() * 1e-10,
        }
    }

    /// The unit normal at (u, v).
    pub fn at(&self, u: f64, v: f64) -> Point3 {
        let surface = self.surface;
        let ((u0, u1), (v0, v1)) = surface.domain();
        let (hu, hv) = ((u1 - u0) * 1e-5, (v1 - v0) * 1e-5);
        let at = |u: f64, v: f64| surface.point_at(u.clamp(u0, u1), v.clamp(v0, v1));
        let n = |u: f64, v: f64| {
            let du = at(u + hu, v) - at(u - hu, v);
            let dv = at(u, v + hv) - at(u, v - hv);
            if du.length() <= self.noise || dv.length() <= self.noise {
                return None;
            }
            du.cross(dv).try_normalize()
        };
        let inward = |x: f64, lo: f64, hi: f64| x + (0.5 * (lo + hi) - x) * 1e-3;
        n(u, v)
            .or_else(|| n(inward(u, u0, u1), inward(v, v0, v1)))
            .unwrap_or(Point3::Z)
    }
}

/// Whether `p` is inside the polygons, by the even-odd rule.
fn inside(polygons: &[Vec<Point2>], p: Point2) -> bool {
    let mut crossings = 0;
    for polygon in polygons {
        for (k, &a) in polygon.iter().enumerate() {
            let b = polygon[(k + 1) % polygon.len()];
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if x > p.x {
                    crossings += 1;
                }
            }
        }
    }
    crossings % 2 == 1
}

/// The loops of `face` as polygons in (u, v), from their sampled pcurves.
/// Coedges without a pcurve leave their loop out.
fn loop_polygons(topo: &Topo, face: FaceId) -> Vec<Vec<Point2>> {
    let Some(face) = topo.face(face) else {
        return Vec::new();
    };
    face.loops
        .iter()
        .filter_map(|&l| {
            let mut polygon: Vec<Point2> = Vec::new();
            for &c in &topo.loop_(l)?.coedges {
                let coedge = topo.coedge(c)?;
                let pcurve = topo.pcurve(coedge.pcurve?)?;
                let (t0, t1) = topo.coedge_range(coedge);
                // Pcurves run along the edge; walk them the loop's way.
                let reversed = matches!(
                    coedge.kind,
                    CoedgeKind::Edge {
                        orientation: Orientation::Reversed,
                        ..
                    }
                );
                let mut points = pcurve.polyline(t0, t1);
                if reversed {
                    points.reverse();
                }
                // Each coedge ends where the next starts.
                points.pop();
                polygon.extend(points);
            }
            polygon.dedup_by(|b, a| a.distance_squared(*b) == 0.0);
            let polygon = straightened(polygon);
            (polygon.len() >= 3).then_some(polygon)
        })
        .collect()
}

/// `polygon` without the corners that lie on the straight line between
/// their neighbours: a curve that is straight in (u, v) — the edges of a
/// thread's flank, hundreds of thousands of samples long — bounds the
/// same region as its two ends, and the grid cuts it where the surface
/// bends.
fn straightened(polygon: Vec<Point2>) -> Vec<Point2> {
    let n = polygon.len();
    if n < 4 {
        return polygon;
    }
    let (lo, hi) = polygon
        .iter()
        .fold((polygon[0], polygon[0]), |(lo, hi), &p| {
            (lo.min(p), hi.max(p))
        });
    let slack = (hi - lo).length() * 1e-7;
    let mut out: Vec<Point2> = Vec::with_capacity(n);
    for k in 0..n {
        let (b, c) = (polygon[k], polygon[(k + 1) % n]);
        let a = out.last().copied().unwrap_or(polygon[n - 1]);
        let along = c - a;
        // Between its neighbours, and on their line.
        let straight =
            (b - a).perp_dot(along).abs() <= slack * along.length() && (b - a).dot(c - b) > 0.0;
        if !straight {
            out.push(b);
        }
    }
    out
}

/// Twice the signed area of `polygon`: positive when counter-clockwise.
fn area2(polygon: &[Point2]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|k| polygon[k].perp_dot(polygon[(k + 1) % n]))
        .sum()
}

/// Whether the open segments `a`–`b` and `c`–`d` cross.
fn crosses(a: Point2, b: Point2, c: Point2, d: Point2) -> bool {
    let side = |p: Point2, q: Point2, r: Point2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(a, b, c), side(a, b, d));
    let (d3, d4) = (side(c, d, a), side(c, d, b));
    (d1 > 0.0) != (d2 > 0.0)
        && (d3 > 0.0) != (d4 > 0.0)
        && d1 != 0.0
        && d2 != 0.0
        && d3 != 0.0
        && d4 != 0.0
}

/// The corners of a polygon by where they are: a grid over its extent.
struct Grid {
    low: Point2,
    /// Cells per unit along each axis.
    per: Point2,
    counts: [usize; 2],
    cells: Vec<Vec<usize>>,
}

impl Grid {
    fn new(polygon: &[Point2]) -> Self {
        let mut low = Point2::splat(f64::INFINITY);
        let mut high = Point2::splat(f64::NEG_INFINITY);
        for &p in polygon {
            low = low.min(p);
            high = high.max(p);
        }
        let size = (high - low).max(Point2::splat(1e-12));
        // About one corner to a cell, the cells as square as the
        // extent allows.
        let n = polygon.len().max(1) as f64;
        let across = (n * size.x / size.y).sqrt().clamp(1.0, n).round();
        let counts = [across as usize, (n / across).ceil().max(1.0) as usize];
        let per = Point2::new(counts[0] as f64 / size.x, counts[1] as f64 / size.y);
        let mut grid = Self {
            low,
            per,
            counts,
            cells: vec![Vec::new(); counts[0] * counts[1]],
        };
        for (i, &p) in polygon.iter().enumerate() {
            let [x, y] = grid.cell(p);
            grid.cells[y * counts[0] + x].push(i);
        }
        grid
    }

    fn cell(&self, p: Point2) -> [usize; 2] {
        let at = (p - self.low) * self.per;
        [
            (at.x.max(0.0) as usize).min(self.counts[0] - 1),
            (at.y.max(0.0) as usize).min(self.counts[1] - 1),
        ]
    }

    /// Whether `wanted` takes a corner of the cells the triangle lies
    /// over.
    fn any_in(&self, a: Point2, b: Point2, c: Point2, wanted: &dyn Fn(usize) -> bool) -> bool {
        let [x0, y0] = self.cell(a.min(b).min(c));
        let [x1, y1] = self.cell(a.max(b).max(c));
        (y0..=y1).any(|y| {
            (x0..=x1).any(|x| {
                self.cells[y * self.counts[0] + x]
                    .iter()
                    .any(|&i| wanted(i))
            })
        })
    }
}

/// Cuts a counter-clockwise polygon without holes (but maybe with the
/// doubled edges of bridges to former holes) into triangles, ear by ear.
///
/// The corners are kept as a ring; only the corners that turn inwards
/// can lie in an ear, so only they are looked at; and the search for
/// the next ear goes on around the ring from where the last was cut. A long outline (a
/// thread's flank has thousands of corners) is cut in about as many
/// steps as it has corners times those that turn inwards.
fn ears(polygon: &[Point2]) -> Vec<[Point2; 3]> {
    let n = polygon.len();
    let mut out = Vec::new();
    if n < 3 {
        return out;
    }
    let mut before: Vec<usize> = (0..n).map(|k| (k + n - 1) % n).collect();
    let mut after: Vec<usize> = (0..n).map(|k| (k + 1) % n).collect();
    let turn = |before: &[usize], after: &[usize], k: usize| {
        let (a, b, c) = (polygon[before[k]], polygon[k], polygon[after[k]]);
        (b - a).perp_dot(c - b)
    };
    // The corners that do not turn outwards, among those left.
    let mut inward: Vec<bool> = (0..n).map(|k| turn(&before, &after, k) <= 0.0).collect();
    // Every corner in a grid of about as many cells as there are
    // corners: an ear is checked against the corners of the cells it
    // lies over, not against all (a thread's face has tens of thousands,
    // and its ears are small).
    let grid = Grid::new(polygon);
    let mut left = n;
    let mut at = 0;
    let mut tried = 0;
    while left > 3 {
        let (a, b, c) = (polygon[before[at]], polygon[at], polygon[after[at]]);
        let ear = !inward[at]
            // No other corner that turns inwards inside it.
            && !grid.any_in(a, b, c, &|i| {
                let p = polygon[i];
                inward[i]
                    && p != a
                    && p != b
                    && p != c
                    && (b - a).perp_dot(p - a) >= 0.0
                    && (c - b).perp_dot(p - b) >= 0.0
                    && (a - c).perp_dot(p - c) >= 0.0
            });
        let cut = if ear {
            at
        } else if tried >= left {
            // Without an ear (rounding), the flattest corner goes.
            let mut flattest = (f64::INFINITY, at);
            let mut k = at;
            for _ in 0..left {
                let flat = turn(&before, &after, k).abs();
                if flat < flattest.0 {
                    flattest = (flat, k);
                }
                k = after[k];
            }
            flattest.1
        } else {
            tried += 1;
            at = after[at];
            continue;
        };
        let (p, q) = (before[cut], after[cut]);
        if turn(&before, &after, cut) > 0.0 {
            out.push([polygon[p], polygon[cut], polygon[q]]);
        }
        after[p] = q;
        before[q] = p;
        inward[cut] = false;
        left -= 1;
        for k in [p, q] {
            inward[k] = turn(&before, &after, k) <= 0.0;
        }
        // On from the corner after the next: ears cut one beside the
        // other fan out from one corner into slivers as long as the
        // outline, each to be cut again at every step of the grid.
        at = after[q];
        tried = 0;
    }
    let (a, b, c) = (before[at], at, after[at]);
    out.push([polygon[a], polygon[b], polygon[c]]);
    out
}

/// Cuts the region the rings bound (by the even-odd rule) into triangles,
/// counter-clockwise.
fn triangulate(rings: &[Vec<Point2>]) -> Vec<[Point2; 3]> {
    // A ring inside an even number of others bounds material from
    // outside; inside an odd number, it is a hole of the smallest
    // around it.
    let depth: Vec<Vec<usize>> = (0..rings.len())
        .map(|i| {
            (0..rings.len())
                .filter(|&j| j != i && inside(std::slice::from_ref(&rings[j]), rings[i][0]))
                .collect()
        })
        .collect();
    let mut out = Vec::new();
    for (i, ring) in rings.iter().enumerate() {
        if depth[i].len() % 2 == 1 {
            continue;
        }
        let mut outer = ring.clone();
        if area2(&outer) < 0.0 {
            outer.reverse();
        }
        let mut holes: Vec<Vec<Point2>> = (0..rings.len())
            .filter(|&j| depth[j].len() == depth[i].len() + 1 && depth[j].contains(&i))
            .map(|j| {
                let mut hole = rings[j].clone();
                if area2(&hole) > 0.0 {
                    hole.reverse();
                }
                hole
            })
            .collect();
        // Each hole is joined to the outline by a bridge: there and back
        // along one segment that crosses nothing. Rightmost holes first.
        let rightmost = |hole: &Vec<Point2>| hole.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        holes.sort_by(|a, b| rightmost(b).total_cmp(&rightmost(a)));
        for h in 0..holes.len() {
            let hole = &holes[h];
            let start = (0..hole.len())
                .max_by(|&a, &b| hole[a].x.total_cmp(&hole[b].x))
                .unwrap_or(0);
            let from = hole[start];
            let blocked = |to: Point2| {
                let ring_blocks = |ring: &[Point2]| {
                    (0..ring.len()).any(|k| crosses(from, to, ring[k], ring[(k + 1) % ring.len()]))
                };
                ring_blocks(&outer) || holes[h..].iter().any(|r| ring_blocks(r))
            };
            let mut order: Vec<usize> = (0..outer.len()).collect();
            order.sort_by(|&a, &b| {
                outer[a]
                    .distance_squared(from)
                    .total_cmp(&outer[b].distance_squared(from))
            });
            let to = order
                .iter()
                .copied()
                .find(|&k| !blocked(outer[k]))
                .unwrap_or(order[0]);
            let mut joined = outer[..=to].to_vec();
            joined.extend((0..=hole.len()).map(|k| hole[(start + k) % hole.len()]));
            joined.extend_from_slice(&outer[to..]);
            outer = joined;
        }
        out.extend(ears(&outer));
    }
    out
}

/// The part of the convex polygon `polygon` where `axis` (0 for u, 1 for
/// v) is at least `lo` and at most `hi`.
fn strip(polygon: &[Point2], axis: usize, lo: f64, hi: f64) -> Vec<Point2> {
    let clip = |polygon: &[Point2], bound: f64, keep_above: bool| {
        let mut out = Vec::new();
        for (k, &a) in polygon.iter().enumerate() {
            let b = polygon[(k + 1) % polygon.len()];
            let (xa, xb) = (a[axis], b[axis]);
            let (ia, ib) = (
                (xa >= bound) == keep_above || xa == bound,
                (xb >= bound) == keep_above || xb == bound,
            );
            if ia {
                out.push(a);
            }
            if ia != ib {
                out.push(a + (b - a) * ((bound - xa) / (xb - xa)));
            }
        }
        out
    };
    clip(&clip(polygon, lo, true), hi, false)
}

/// Cuts `triangles` along the lines where `axis` takes one of `values`
/// (ascending), so no triangle spans more than one step.
fn slice(triangles: Vec<[Point2; 3]>, axis: usize, values: &[f64]) -> Vec<[Point2; 3]> {
    if values.len() <= 2 {
        return triangles;
    }
    let mut out = Vec::new();
    for triangle in triangles {
        let lo = triangle.iter().map(|p| p[axis]).fold(f64::MAX, f64::min);
        let hi = triangle.iter().map(|p| p[axis]).fold(f64::MIN, f64::max);
        // (Only the steps the triangle reaches into.)
        let first = values.partition_point(|v| *v <= lo).saturating_sub(1);
        for pair in values[first..].windows(2) {
            if pair[0] >= hi {
                break;
            }
            if pair[1] <= lo {
                continue;
            }
            let piece = strip(&triangle, axis, pair[0], pair[1]);
            for k in 1..piece.len().saturating_sub(1) {
                out.push([piece[0], piece[k], piece[k + 1]]);
            }
        }
    }
    out
}

/// Samples `face` of `topo`: the region its loops bound in (u, v) (all of
/// the domain without loops), cut into triangles no wider than a step of
/// the grid where the surface curves.
///
/// A face is sampled once: what it is made of (its surface, and the
/// curves of its loops in (u, v)) is what its triangles are kept under,
/// so the same face in another world — the bodies a step did not touch,
/// an imported model while a fillet beside it is edited — is not sampled
/// again.
pub fn face(topo: &Topo, id: FaceId) -> Option<Arc<FaceMesh>> {
    static KEPT: Mutex<Kept> = Mutex::new(Kept {
        meshes: None,
        triangles: 0,
    });
    let key = made_of(topo, id)?;
    {
        let kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mesh) = kept.meshes.as_ref().and_then(|m| m.get(&key)) {
            return Some(mesh.clone());
        }
    }
    let mesh = Arc::new(sample(topo, id)?);
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    // (No more than some tens of millions of triangles are held.)
    if kept.triangles > 40_000_000 {
        kept.meshes = None;
        kept.triangles = 0;
    }
    kept.triangles += mesh.triangles.len();
    kept.meshes
        .get_or_insert_with(HashMap::new)
        .insert(key, mesh.clone());
    Some(mesh)
}

/// The faces sampled so far, by what they are made of.
struct Kept {
    meshes: Option<HashMap<u128, Arc<FaceMesh>>>,
    triangles: usize,
}

/// What `face` of `topo` is made of, hashed: its surface and the curves
/// of its loops, with the stretch of each that is walked and which way.
fn made_of(topo: &Topo, id: FaceId) -> Option<u128> {
    use std::hash::{Hash, Hasher};
    let face = topo.face(id)?;
    let surface = topo.surface(face.surface)?;
    let hash = |seed: u8| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        seed.hash(&mut h);
        let floats = |h: &mut std::collections::hash_map::DefaultHasher, xs: &[f64]| {
            xs.len().hash(h);
            for x in xs {
                x.to_bits().hash(h);
            }
        };
        surface.degrees().hash(&mut h);
        surface.counts().hash(&mut h);
        let (ku, kv) = surface.knots();
        floats(&mut h, ku);
        floats(&mut h, kv);
        floats(&mut h, surface.weights());
        for p in surface.control_points() {
            floats(&mut h, &p.to_array());
        }
        for &l in &face.loops {
            let Some(lp) = topo.loop_(l) else {
                continue;
            };
            lp.coedges.len().hash(&mut h);
            for &c in &lp.coedges {
                let Some(coedge) = topo.coedge(c) else {
                    continue;
                };
                let (t0, t1) = topo.coedge_range(coedge);
                floats(&mut h, &[t0, t1]);
                matches!(
                    coedge.kind,
                    CoedgeKind::Edge {
                        orientation: Orientation::Reversed,
                        ..
                    }
                )
                .hash(&mut h);
                if let Some(pcurve) = coedge.pcurve.and_then(|p| topo.pcurve(p)) {
                    pcurve.degree().hash(&mut h);
                    floats(&mut h, pcurve.knots());
                    floats(&mut h, pcurve.weights());
                    for p in pcurve.control_points() {
                        floats(&mut h, &p.to_array());
                    }
                }
            }
        }
        h.finish()
    };
    Some((u128::from(hash(1)) << 64) | u128::from(hash(2)))
}

fn sample(topo: &Topo, id: FaceId) -> Option<FaceMesh> {
    let face = topo.face(id)?;
    let surface = topo.surface(face.surface)?;
    let (us, vs) = (samples(surface, false), samples(surface, true));
    let mut rings = loop_polygons(topo, id);
    if rings.is_empty() {
        let ((u0, u1), (v0, v1)) = surface.domain();
        rings.push(vec![
            Point2::new(u0, v0),
            Point2::new(u1, v0),
            Point2::new(u1, v1),
            Point2::new(u0, v1),
        ]);
    }
    let flat = slice(slice(triangulate(&rings), 0, &us), 1, &vs);
    let mut triangles = Vec::new();
    let normals_of = Normals::of(surface);
    // A corner of the grid is a corner of some six triangles: where it
    // is and its normal are worked out once.
    let mut corners_at: HashMap<[u64; 2], (Point3, Point3)> = HashMap::new();
    let mut corner = |c: Point2| {
        *corners_at
            .entry([c.x.to_bits(), c.y.to_bits()])
            .or_insert_with(|| (surface.point_at(c.x, c.y), normals_of.at(c.x, c.y)))
    };
    for corners in flat {
        workpiece_core::cancel::checkpoint();
        let at = corners.map(&mut corner);
        let positions = at.map(|c| c.0);
        let [a, b, c] = positions;
        if (b - a).cross(c - a).length_squared() <= f64::EPSILON * f64::EPSILON {
            continue;
        }
        triangles.push((positions, at.map(|c| c.1)));
    }
    Some(FaceMesh { triangles })
}

/// The volume the sampled faces of `body` enclose: as exact as its
/// triangles are flat.
pub fn volume(topo: &Topo, body: BodyId) -> f64 {
    let Some(source) = topo.body(body) else {
        return 0.0;
    };
    let uses = source
        .lumps
        .iter()
        .filter_map(|&l| topo.lump(l))
        .flat_map(|l| &l.shells)
        .filter_map(|&s| topo.shell(s))
        .flat_map(|s| &s.faces);
    let mut total = 0.0;
    for face_use in uses {
        let Some(mesh) = face(topo, face_use.face) else {
            continue;
        };
        let sign = match face_use.orientation {
            Orientation::Forward => 1.0,
            Orientation::Reversed => -1.0,
        };
        for ([a, b, c], _) in mesh.triangles.iter().copied() {
            total += sign * a.dot(b.cross(c)) / 6.0;
        }
    }
    total
}

/// The solid bodies of `topo`, each by name with its sampled faces'
/// triangles, wound to face out of it.
fn solids(topo: &Topo) -> Vec<(String, Vec<[Point3; 3]>)> {
    let mut all = Vec::new();
    for (id, body) in topo.bodies().iter() {
        if body.kind != workpiece_core::world::topo::BodyKind::Solid {
            continue;
        }
        let mut triangles = Vec::new();
        let uses = body
            .lumps
            .iter()
            .filter_map(|&l| topo.lump(l))
            .flat_map(|l| &l.shells)
            .filter_map(|&s| topo.shell(s))
            .flat_map(|s| &s.faces);
        for face_use in uses {
            let Some(mesh) = face(topo, face_use.face) else {
                continue;
            };
            let forward = matches!(face_use.orientation, Orientation::Forward);
            for ([a, b, c], _) in mesh.triangles.iter().copied() {
                triangles.push(if forward { [a, b, c] } else { [a, c, b] });
            }
        }
        all.push((topo.name_of(id).unwrap_or("body").to_owned(), triangles));
    }
    all
}

/// The check sum a zip file wants of `bytes`.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (!(crc & 1)).wrapping_add(1));
        }
    }
    !crc
}

/// A zip file of `files` (each a name and its bytes), stored as they
/// are.
fn zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut directory: Vec<u8> = Vec::new();
    for (name, bytes) in files {
        let (at, sum, size) = (out.len() as u32, crc32(bytes), bytes.len() as u32);
        // Version, flags, method (stored), time, date; then the sums.
        let mut about: Vec<u8> = vec![20, 0, 0, 0, 0, 0, 0, 0, 0x21, 0];
        about.extend(sum.to_le_bytes());
        about.extend(size.to_le_bytes());
        about.extend(size.to_le_bytes());
        about.extend((name.len() as u16).to_le_bytes());
        about.extend([0, 0]);
        out.extend(b"PK\x03\x04");
        out.extend(&about);
        out.extend(name.as_bytes());
        out.extend(bytes);
        directory.extend(b"PK\x01\x02");
        directory.extend([20, 0]);
        directory.extend(&about);
        directory.extend([0_u8; 10]);
        directory.extend(at.to_le_bytes());
        directory.extend(name.as_bytes());
    }
    let (at, size) = (out.len() as u32, directory.len() as u32);
    out.extend(directory);
    out.extend(b"PK\x05\x06");
    out.extend([0_u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend(size.to_le_bytes());
    out.extend(at.to_le_bytes());
    out.extend([0, 0]);
    out
}

/// The solid bodies of `topo` as a 3MF file, its numbers in `unit`: each an
/// object of its own, a mesh of its sampled faces' triangles sharing
/// their corners.
pub fn three_mf(topo: &Topo, unit: workpiece_core::Unit) -> Vec<u8> {
    use std::fmt::Write;
    let unit = match unit {
        workpiece_core::Unit::Millimetre => "millimeter",
        workpiece_core::Unit::Centimetre => "centimeter",
        workpiece_core::Unit::Metre => "meter",
        workpiece_core::Unit::Inch => "inch",
    };
    let mut model = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"{unit}\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n <resources>\n",
    );
    let bodies = solids(topo);
    for (k, (name, triangles)) in bodies.iter().enumerate() {
        // Corners that are one place are one vertex: by their numbers
        // as they are written.
        let mut index = std::collections::HashMap::new();
        let mut vertices = String::new();
        let mut faces = String::new();
        for triangle in triangles {
            let ids = triangle.map(|p| {
                let key = format!("x=\"{:.6}\" y=\"{:.6}\" z=\"{:.6}\"", p.x, p.y, p.z);
                let next = index.len();
                *index.entry(key.clone()).or_insert_with(|| {
                    let _ = writeln!(vertices, "     <vertex {key}/>");
                    next
                })
            });
            if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
                let _ = writeln!(
                    faces,
                    "     <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>",
                    ids[0], ids[1], ids[2]
                );
            }
        }
        let name = name
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;");
        let _ = write!(
            model,
            "  <object id=\"{}\" type=\"model\" name=\"{name}\">\n   <mesh>\n    <vertices>\n{vertices}    </vertices>\n    <triangles>\n{faces}    </triangles>\n   </mesh>\n  </object>\n",
            k + 1
        );
    }
    model.push_str(" </resources>\n <build>\n");
    for k in 0..bodies.len() {
        let _ = writeln!(model, "  <item objectid=\"{}\"/>", k + 1);
    }
    model.push_str(" </build>\n</model>\n");
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>\n";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>\n";
    zip(&[
        ("[Content_Types].xml", types.into()),
        ("_rels/.rels", rels.into()),
        ("3D/3dmodel.model", model.into_bytes()),
    ])
}

/// The solid bodies of `topo` as a binary STL file: their sampled
/// faces' triangles, each wound to face out of its body.
pub fn stl(topo: &Topo) -> Vec<u8> {
    let triangles: Vec<[Point3; 3]> = solids(topo).into_iter().flat_map(|(_, t)| t).collect();
    let mut out = vec![0_u8; 80];
    out[..13].copy_from_slice(b"workpiece stl");
    out.extend((triangles.len() as u32).to_le_bytes());
    for [a, b, c] in triangles {
        let normal = (b - a).cross(c - a).normalize_or_zero();
        for p in [normal, a, b, c] {
            for x in p.to_array() {
                out.extend((x as f32).to_le_bytes());
            }
        }
        out.extend(0_u16.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_cover_each_span_as_it_turns() -> anyhow::Result<()> {
        // Half a cylinder: two quarter turns around, straight along.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let around = [
            (1.0, 0.0, 1.0),
            (1.0, 1.0, w),
            (0.0, 1.0, 1.0),
            (-1.0, 1.0, w),
            (-1.0, 0.0, 1.0),
        ];
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for (x, y, weight) in around {
            for z in [0.0, 2.0] {
                points.push(Point3::new(x, y, z));
                weights.push(weight);
            }
        }
        let surface = NurbsSurface::new(
            2,
            1,
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            5,
            2,
            points,
            Some(weights),
        )?;
        assert_eq!(samples(&surface, true), [0.0, 1.0]);
        let round = samples(&surface, false);
        assert_eq!(round.len(), 2 * FINEST + 1);
        assert_eq!(round.first(), Some(&0.0));
        assert_eq!(round.last(), Some(&1.0));
        Ok(())
    }

    #[test]
    fn regions_with_holes_are_cut_into_triangles_of_their_area() {
        let square = |lo: f64, hi: f64| {
            vec![
                Point2::new(lo, lo),
                Point2::new(hi, lo),
                Point2::new(hi, hi),
                Point2::new(lo, hi),
            ]
        };
        let area = |triangles: &[[Point2; 3]]| -> f64 {
            triangles
                .iter()
                .map(|[a, b, c]| (*b - *a).perp_dot(*c - *a) / 2.0)
                .sum()
        };
        // A square with two holes, one of them the other way around.
        let mut second = square(0.6, 0.8);
        second.reverse();
        let rings = [square(0.0, 1.0), square(0.2, 0.4), second];
        let triangles = triangulate(&rings);
        assert!((area(&triangles) - (1.0 - 0.04 - 0.04)).abs() < 1e-12);
        // Cutting along a grid keeps the area and the turning sense.
        let cut = slice(triangles, 0, &[0.0, 0.25, 0.5, 0.75, 1.0]);
        assert!((area(&cut) - 0.92).abs() < 1e-12);
        assert!(
            cut.iter()
                .all(|[a, b, c]| (*b - *a).perp_dot(*c - *a) > 0.0)
        );
        assert!(cut.iter().all(|t| {
            let lo = t.iter().map(|p| p.x).fold(f64::MAX, f64::min);
            let hi = t.iter().map(|p| p.x).fold(f64::MIN, f64::max);
            hi - lo <= 0.25 + 1e-12
        }));
    }

    #[test]
    fn even_odd_trims_holes() {
        let square = |lo: f64, hi: f64| {
            vec![
                Point2::new(lo, lo),
                Point2::new(hi, lo),
                Point2::new(hi, hi),
                Point2::new(lo, hi),
            ]
        };
        let polygons = [square(0.0, 1.0), square(0.4, 0.6)];
        assert!(inside(&polygons, Point2::new(0.2, 0.2)));
        assert!(!inside(&polygons, Point2::new(0.5, 0.5)));
        assert!(!inside(&polygons, Point2::new(1.5, 0.5)));
    }

    #[test]
    fn zip_files_hold_what_is_put_in_them() {
        // (The check sum of "123456789" is the one everybody quotes.)
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let file = zip(&[("a.txt", b"hello".to_vec()), ("b/c.txt", b"more".to_vec())]);
        assert!(file.starts_with(b"PK\x03\x04"));
        // Its end tells of two files and where their directory is.
        let end = &file[file.len() - 22..];
        assert_eq!(&end[..4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([end[10], end[11]]), 2);
        let at = u32::from_le_bytes([end[16], end[17], end[18], end[19]]) as usize;
        assert_eq!(&file[at..at + 4], b"PK\x01\x02");
        let model = three_mf(&Topo::new(), workpiece_core::Unit::Inch);
        assert!(model.windows(16).any(|w| w == b"3D/3dmodel.model"));
    }
}
