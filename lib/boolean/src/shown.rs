//! A solid as a viewer shows it: every face as triangles of its own,
//! which together close the solid, and every edge as a polyline, with
//! the faces it bounds and how sharply they meet along it.
//!
//! An edge is sampled once, as finely as keeps its chords within the
//! tolerance, and every face it bounds starts from those very points:
//! so the faces' triangles meet corner on corner along it. A face is
//! the region its loops bound in (u, v), cut into triangles ear by ear
//! and then cut finer wherever a triangle's side or middle is farther
//! from the surface than the tolerance — a side by a rule that asks
//! only its two ends, so that the two triangles it lies between are cut
//! alike.

use std::collections::{HashMap, HashSet};

use workpiece_core::cancel::checkpoint;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::NurbsSurface;

use crate::model::{Face, Solid, Walk};
use crate::query::{self, FaceData, SolidData};

/// A face as triangles.
#[derive(Debug, Clone, Default)]
pub struct ShownFace {
    /// The corners.
    pub positions: Vec<Point3>,
    /// The outward unit normal of the face's surface at each corner.
    pub normals: Vec<Point3>,
    /// The triangles, by their corners: counter-clockwise seen from
    /// outside the solid.
    pub triangles: Vec<[usize; 3]>,
}

/// A solid as a viewer shows it.
#[derive(Debug, Clone, Default)]
pub struct Shown {
    /// The faces, in the solid's order.
    pub faces: Vec<ShownFace>,
    /// The edges between faces, each as a polyline.
    pub edges: Vec<Vec<Point3>>,
    /// For each of `edges`, the faces it bounds: one for each use.
    pub edge_faces: Vec<Vec<usize>>,
    /// For each of `edges` between two distinct faces, the least and
    /// greatest angle between their outward normals along it: more than
    /// nothing where the edge is convex, less where it is concave.
    pub edge_turns: Vec<Option<[f64; 2]>>,
    /// For each of `edges`, whether its two faces lie on one surface.
    pub edge_split: Vec<bool>,
    /// The seams: where a face meets itself.
    pub seams: Vec<Vec<Point3>>,
}

/// The parameters from `lo` to `hi` at which `at` is sampled so that no
/// chord strays farther than `tolerance` from it: `breaks` (rising,
/// from `lo` to `hi`) among them.
pub(crate) fn stops(
    at: &dyn Fn(f64) -> Point3,
    breaks: &[f64],
    straight: bool,
    tolerance: f64,
) -> Vec<f64> {
    let near = tolerance.max(0.0) * 0.7;
    fn between(
        at: &dyn Fn(f64) -> Point3,
        (t0, p0): (f64, Point3),
        (t1, p1): (f64, Point3),
        near: f64,
        depth: u32,
        out: &mut Vec<f64>,
    ) {
        let chord = p1 - p0;
        let off = |t: f64| {
            let q = at(t) - p0;
            let along = (q.dot(chord) / chord.length_squared().max(1e-300)).clamp(0.0, 1.0);
            (q - chord * along).length()
        };
        let far = [0.25, 0.5, 0.75]
            .into_iter()
            .map(|f| off(t0 + (t1 - t0) * f))
            .fold(0.0, f64::max);
        if depth > 0 && far > near {
            let mid = (t0 + t1) / 2.0;
            let pm = at(mid);
            between(at, (t0, p0), (mid, pm), near, depth - 1, out);
            between(at, (mid, pm), (t1, p1), near, depth - 1, out);
        } else {
            out.push(t1);
        }
    }
    let mut out = vec![breaks[0]];
    for pair in breaks.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if straight {
            out.push(b);
            continue;
        }
        // (In two halves first: a span that swings to both sides of its
        // chord may pass through it at all three places.)
        let mid = (a + b) / 2.0;
        let (pa, pm, pb) = (at(a), at(mid), at(b));
        between(at, (a, pa), (mid, pm), near, 20, &mut out);
        between(at, (mid, pm), (b, pb), near, 20, &mut out);
    }
    out
}

/// A corner of a face's triangles.
#[derive(Clone, Copy)]
struct Corner {
    uv: Point2,
    at: Point3,
    /// Where the surface collapses to a point: the parameter (0 for u,
    /// 1 for v) that runs along the collapsed side.
    pole: Option<usize>,
}

/// How sharply the ring turns left at `b`, coming from `a` and going on
/// to `d`: the sine of the angle, nothing for what is straight but for
/// rounding.
fn turn(c: &[Corner], a: usize, b: usize, d: usize) -> f64 {
    let (to, on) = (c[b].uv - c[a].uv, c[d].uv - c[b].uv);
    let sine = to.perp_dot(on) / (to.length() * on.length()).max(f64::MIN_POSITIVE);
    if sine.abs() < 1e-9 { 0.0 } else { sine }
}

/// Whether the open segments `a`–`b` and `c`–`d` cross.
fn crosses(a: Point2, b: Point2, c: Point2, d: Point2) -> bool {
    let side = |p: Point2, q: Point2, r: Point2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(a, b, c), side(a, b, d));
    let (d3, d4) = (side(c, d, a), side(c, d, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

fn area2(c: &[Corner], ring: &[usize]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|k| c[ring[k]].uv.perp_dot(c[ring[(k + 1) % n]].uv))
        .sum()
}

fn holds(c: &[Corner], ring: &[usize], p: Point2) -> bool {
    let n = ring.len();
    let mut inside = false;
    for k in 0..n {
        let (a, b) = (c[ring[k]].uv, c[ring[(k + 1) % n]].uv);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Cuts a counter-clockwise ring of corners (maybe with the doubled
/// sides of bridges to former holes) into triangles, ear by ear. Every
/// side of the ring is a side of one triangle, and every other side of
/// a triangle is one of two.
fn ears(c: &[Corner], ring: &[usize]) -> Vec<[usize; 3]> {
    let n = ring.len();
    let mut out = Vec::new();
    if n < 3 {
        return out;
    }
    let mut before: Vec<usize> = (0..n).map(|k| (k + n - 1) % n).collect();
    let mut after: Vec<usize> = (0..n).map(|k| (k + 1) % n).collect();
    let bend = |before: &[usize], after: &[usize], k: usize| {
        turn(c, ring[before[k]], ring[k], ring[after[k]])
    };
    let mut inward: Vec<bool> = (0..n).map(|k| bend(&before, &after, k) <= 0.0).collect();
    let mut gone = vec![false; n];
    let (mut left, mut at, mut tried) = (n, 0, 0);
    while left > 3 {
        checkpoint();
        let (a, b, d) = (
            c[ring[before[at]]].uv,
            c[ring[at]].uv,
            c[ring[after[at]]].uv,
        );
        let ear = !inward[at]
            && !(0..n).any(|i| {
                let p = c[ring[i]].uv;
                !gone[i]
                    && inward[i]
                    && p != a
                    && p != b
                    && p != d
                    && (b - a).perp_dot(p - a) >= 0.0
                    && (d - b).perp_dot(p - b) >= 0.0
                    && (a - d).perp_dot(p - d) >= 0.0
            });
        let cut = if ear {
            at
        } else if tried >= left {
            // Without an ear (rounding), the flattest corner goes.
            let mut flattest = (f64::INFINITY, at);
            let mut k = at;
            for _ in 0..left {
                let flat = bend(&before, &after, k).abs();
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
        out.push([ring[p], ring[cut], ring[q]]);
        after[p] = q;
        before[q] = p;
        gone[cut] = true;
        inward[cut] = false;
        left -= 1;
        for k in [p, q] {
            inward[k] = bend(&before, &after, k) <= 0.0;
        }
        // (On from the corner after the next: ears cut one beside the
        // other fan out from one corner into slivers.)
        at = after[q];
        tried = 0;
    }
    out.push([ring[before[at]], ring[at], ring[after[at]]]);
    out
}

/// Cuts the region the rings bound into triangles: the rings that run
/// counter-clockwise are outlines, the others holes of the outline
/// around them.
fn triangulate(c: &[Corner], rings: &[Vec<usize>]) -> Vec<[usize; 3]> {
    let (outlines, holes): (Vec<&Vec<usize>>, Vec<&Vec<usize>>) =
        rings.iter().partition(|r| area2(c, r) > 0.0);
    let mut out = Vec::new();
    for (k, outline) in outlines.iter().enumerate() {
        let mut outer: Vec<usize> = (*outline).clone();
        let mut mine: Vec<&Vec<usize>> = holes
            .iter()
            .copied()
            .filter(|h| {
                let first = outlines.iter().position(|o| holds(c, o, c[h[0]].uv));
                first.unwrap_or(0) == k
            })
            .collect();
        // Each hole is joined to the outline by a bridge: there and
        // back along one segment that crosses nothing. Rightmost first.
        let rightmost = |h: &Vec<usize>| h.iter().map(|&i| c[i].uv.x).fold(f64::MIN, f64::max);
        mine.sort_by(|a, b| rightmost(b).total_cmp(&rightmost(a)));
        for h in 0..mine.len() {
            let hole = mine[h];
            let start = (0..hole.len())
                .max_by(|&a, &b| c[hole[a]].uv.x.total_cmp(&c[hole[b]].uv.x))
                .unwrap_or(0);
            let from = c[hole[start]].uv;
            let blocked = |to: Point2| {
                let ring_blocks = |ring: &[usize]| {
                    (0..ring.len()).any(|k| {
                        let (p, q) = (c[ring[k]].uv, c[ring[(k + 1) % ring.len()]].uv);
                        crosses(from, to, p, q)
                    })
                };
                ring_blocks(&outer) || mine[h..].iter().any(|r| ring_blocks(r))
            };
            let mut order: Vec<usize> = (0..outer.len()).collect();
            order.sort_by(|&a, &b| {
                let d = |k: usize| c[outer[k]].uv.distance_squared(from);
                d(a).total_cmp(&d(b))
            });
            let to = order
                .iter()
                .copied()
                .find(|&k| !blocked(c[outer[k]].uv))
                .unwrap_or(order[0]);
            let mut joined = outer[..=to].to_vec();
            joined.extend((0..=hole.len()).map(|k| hole[(start + k) % hole.len()]));
            joined.extend_from_slice(&outer[to..]);
            outer = joined;
        }
        out.extend(ears(c, &outer));
    }
    out
}

/// How far `p` is from the segment from `a` to `b`.
fn off_segment(p: Point3, a: Point3, b: Point3) -> f64 {
    let chord = b - a;
    let along = ((p - a).dot(chord) / chord.length_squared().max(1e-300)).clamp(0.0, 1.0);
    p.distance(a + chord * along)
}

/// The triangles of a curved face, being cut finer and turned.
struct Net<'a> {
    surface: &'a NurbsSurface,
    c: Vec<Corner>,
    /// The sides of the face's rings: what no cut may touch.
    sides: HashSet<(usize, usize)>,
    /// What was found of each side looked at: the corner it is cut at.
    cut: HashMap<(usize, usize), Option<usize>>,
    /// How far a chord may stray.
    near: f64,
}

impl Net<'_> {
    /// The corner the side from `a` to `b` is cut at, if the surface
    /// leaves it by more than is allowed: asked of the two ends alone.
    fn split(&mut self, a: usize, b: usize) -> Option<usize> {
        let key = (a.min(b), a.max(b));
        if self.sides.contains(&key) {
            return None;
        }
        if let Some(known) = self.cut.get(&key) {
            return *known;
        }
        let (p, q) = (self.c[key.0], self.c[key.1]);
        let surface = self.surface;
        let far = [0.25, 0.5, 0.75]
            .into_iter()
            .map(|f| {
                let uv = p.uv.lerp(q.uv, f);
                off_segment(surface.point_at(uv.x, uv.y), p.at, q.at)
            })
            .fold(0.0, f64::max);
        let made = (far > self.near).then(|| {
            let uv = p.uv.lerp(q.uv, 0.5);
            self.c.push(Corner {
                uv,
                at: surface.point_at(uv.x, uv.y),
                pole: None,
            });
            self.c.len() - 1
        });
        self.cut.insert(key, made);
        made
    }

    /// `triangles` cut wherever a side or a middle is farther from the
    /// surface than allowed; whether any was.
    fn refined(&mut self, triangles: Vec<[usize; 3]>) -> (Vec<[usize; 3]>, bool) {
        let mut done = Vec::new();
        let mut any = false;
        let mut work: Vec<([usize; 3], u32)> = triangles.into_iter().map(|t| (t, 0)).collect();
        while let Some((t, depth)) = work.pop() {
            if depth >= 24 {
                done.push(t);
                continue;
            }
            checkpoint();
            let [i, j, k] = t;
            let mids = [self.split(i, j), self.split(j, k), self.split(k, i)];
            let next = depth + 1;
            let mut more = |all: &[[usize; 3]]| work.extend(all.iter().map(|&t| (t, next)));
            match mids {
                [None, None, None] => {
                    let c = &self.c;
                    let uv = (c[i].uv + c[j].uv + c[k].uv) / 3.0;
                    let on = self.surface.point_at(uv.x, uv.y);
                    let n = (c[j].at - c[i].at).cross(c[k].at - c[i].at);
                    let off = n
                        .try_normalize()
                        .map_or(0.0, |n| (on - c[i].at).dot(n).abs());
                    if off > self.near {
                        self.c.push(Corner {
                            uv,
                            at: on,
                            pole: None,
                        });
                        let m = self.c.len() - 1;
                        more(&[[i, j, m], [j, k, m], [k, i, m]]);
                    } else {
                        done.push(t);
                        continue;
                    }
                }
                [Some(a), Some(b), Some(d)] => more(&[[i, a, d], [a, j, b], [d, b, k], [a, b, d]]),
                _ => {
                    // Turned so that the first side is cut.
                    let at = mids.iter().position(Option::is_some).unwrap_or(0);
                    let v = [t[at], t[(at + 1) % 3], t[(at + 2) % 3]];
                    let m = [mids[at], mids[(at + 1) % 3], mids[(at + 2) % 3]];
                    let a = m[0].unwrap_or(v[0]);
                    match (m[1], m[2]) {
                        (None, None) => more(&[[v[0], a, v[2]], [a, v[1], v[2]]]),
                        (Some(b), _) => more(&[[a, v[1], b], [v[0], a, b], [v[0], b, v[2]]]),
                        (None, Some(d)) => more(&[[v[0], a, d], [a, v[1], v[2]], [a, v[2], d]]),
                    }
                }
            }
            any = true;
        }
        (done, any)
    }

    /// Puts the corner `m` into the triangle of `triangles` it lies in
    /// (in (u, v)), which becomes three; or onto the side it lies on,
    /// if that is between two triangles, which become four.
    fn insert(&self, triangles: &mut Vec<[usize; 3]>, m: usize) {
        let p = self.c[m].uv;
        let side = |a: usize, b: usize| {
            let (a, b) = (self.c[a].uv, self.c[b].uv);
            let (to, on) = (b - a, p - a);
            let sine = to.perp_dot(on) / (to.length() * on.length()).max(f64::MIN_POSITIVE);
            if sine.abs() < 1e-9 { 0.0 } else { sine }
        };
        let within = |t: &[usize; 3]| {
            (0..3)
                .map(|n| side(t[n], t[(n + 1) % 3]))
                .collect::<Vec<_>>()
        };
        let Some(t) = (0..triangles.len())
            .rev()
            .find(|&t| within(&triangles[t]).iter().all(|&s| s >= 0.0))
        else {
            return;
        };
        let [a, b, c] = triangles[t];
        let sides = within(&triangles[t]);
        match sides.iter().position(|&s| s == 0.0) {
            None => {
                triangles[t] = [a, b, m];
                triangles.extend([[b, c, m], [c, a, m]]);
            }
            Some(n) => {
                let (a, b, c) = (
                    triangles[t][n],
                    triangles[t][(n + 1) % 3],
                    triangles[t][(n + 2) % 3],
                );
                let key = (a.min(b), a.max(b));
                // Its other triangle, if it has one and is no side of
                // the face.
                let other = (0..triangles.len())
                    .find(|&o| o != t && triangles[o].contains(&a) && triangles[o].contains(&b));
                let (Some(o), false) = (other, self.sides.contains(&key)) else {
                    return;
                };
                let Some(&d) = triangles[o].iter().find(|&&i| i != a && i != b) else {
                    return;
                };
                if sides.iter().filter(|&&s| s == 0.0).count() > 1 {
                    return;
                }
                triangles[t] = [a, m, c];
                triangles[o] = [m, b, c];
                triangles.extend([[b, m, d], [m, a, d]]);
            }
        }
    }

    /// The angle of the triangle `a`, `b`, `c` at `c`: in space, or in
    /// (u, v) stretched by `laid`.
    fn angle(&self, a: usize, b: usize, c: usize, laid: Option<Point2>) -> f64 {
        match laid {
            None => {
                let (p, q) = (self.c[a].at - self.c[c].at, self.c[b].at - self.c[c].at);
                libm::atan2(p.cross(q).length(), p.dot(q))
            }
            Some(by) => {
                let (p, q) = (
                    (self.c[a].uv - self.c[c].uv) * by,
                    (self.c[b].uv - self.c[c].uv) * by,
                );
                libm::atan2(p.perp_dot(q).abs(), p.dot(q))
            }
        }
    }

    /// `triangles` with the side between two of them turned to run
    /// between their other two corners wherever that makes them less
    /// thin (the angles across the side are more than half a turn
    /// together), as long as any is. The angles are those in space, or
    /// (for triangles not yet cut to follow the surface) those in
    /// (u, v) stretched by `laid`.
    fn turned(&self, mut triangles: Vec<[usize; 3]>, laid: Option<Point2>) -> Vec<[usize; 3]> {
        // Each side with the triangles on it.
        let key = |a: usize, b: usize| (a.min(b), a.max(b));
        let mut on: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (t, tri) in triangles.iter().enumerate() {
            for n in 0..3 {
                on.entry(key(tri[n], tri[(n + 1) % 3])).or_default().push(t);
            }
        }
        let mut queue: Vec<(usize, usize)> = on.keys().copied().collect();
        queue.sort_unstable();
        let mut budget = queue.len() * 100;
        let collapsed = |t: [usize; 3]| {
            let p = t.map(|i| self.c[i].at);
            laid.is_none() && (p[0] == p[1] || p[1] == p[2] || p[2] == p[0])
        };
        while let Some(side) = queue.pop() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            checkpoint();
            if self.sides.contains(&side) {
                continue;
            }
            let Some(&[s, t]) = on.get(&side).map(Vec::as_slice) else {
                continue;
            };
            // The side as the first triangle walks it, and the corners
            // across it.
            let at = (0..3)
                .find(|&n| key(triangles[s][n], triangles[s][(n + 1) % 3]) == side)
                .unwrap_or(0);
            let (a, b, c) = (
                triangles[s][at],
                triangles[s][(at + 1) % 3],
                triangles[s][(at + 2) % 3],
            );
            let Some(&d) = triangles[t].iter().find(|&&i| i != a && i != b) else {
                continue;
            };
            if collapsed(triangles[s]) || collapsed(triangles[t]) || on.contains_key(&key(c, d)) {
                continue;
            }
            if self.angle(a, b, c, laid) + self.angle(a, b, d, laid) <= std::f64::consts::PI + 1e-9
            {
                continue;
            }
            // Only where the four corners bound a convex region of
            // (u, v): the new triangles lie in it the right way round.
            if turn(&self.c, a, d, c) <= 0.0 || turn(&self.c, d, b, c) <= 0.0 {
                continue;
            }
            let (new_s, new_t) = ([a, d, c], [d, b, c]);
            if collapsed(new_s) || collapsed(new_t) {
                continue;
            }
            triangles[s] = new_s;
            triangles[t] = new_t;
            on.remove(&side);
            on.insert(key(c, d), vec![s, t]);
            for (from, to, other) in [(key(a, d), s, t), (key(b, c), t, s)] {
                if let Some(list) = on.get_mut(&from) {
                    for x in list.iter_mut().filter(|x| **x == other) {
                        *x = to;
                    }
                }
            }
            queue.extend([key(a, c), key(a, d), key(b, c), key(b, d)]);
        }
        triangles
    }
}

/// The values of the parameter `axis` (0 for u, 1 for v) of `surface`
/// over `uv` so close that the surface, followed along that parameter
/// from one to the next, stays within `tolerance` of the chord.
pub(crate) fn grid_lines(
    surface: &NurbsSurface,
    uv: [(f64, f64); 2],
    axis: usize,
    tolerance: f64,
) -> Vec<f64> {
    let (lo, hi) = uv[axis];
    let (degrees, knots) = (surface.degrees(), surface.knots());
    let (degree, knots) = if axis == 0 {
        (degrees.0, knots.0)
    } else {
        (degrees.1, knots.1)
    };
    let mut breaks = vec![lo];
    for &k in knots {
        if k > lo && k < hi && breaks.last().is_none_or(|&l| k > l) {
            breaks.push(k);
        }
    }
    breaks.push(hi);
    let straight = degree <= 1 && surface.weights().windows(2).all(|w| w[0] == w[1]);
    let (a, b) = uv[1 - axis];
    [0.03, 0.5, 0.97]
        .into_iter()
        .map(|f| {
            let other = a + (b - a) * f;
            let at = |t: f64| {
                if axis == 0 {
                    surface.point_at(t, other)
                } else {
                    surface.point_at(other, t)
                }
            };
            stops(&at, &breaks, straight, tolerance)
        })
        .max_by_key(Vec::len)
        .unwrap_or(breaks)
}

/// The triangles of one face.
fn shown_face(
    solid: &Solid,
    face: &Face,
    fd: &FaceData,
    edge_stops: &[Vec<f64>],
    edge_points: &[Vec<Point3>],
    tolerance: f64,
) -> ShownFace {
    let surface: &NurbsSurface = &face.surface;
    // A face on a plane is cut into triangles in the plane itself:
    // however its surface is laid out, they are the face exactly.
    let middle = Point2::new(
        (fd.uv[0].0 + fd.uv[0].1) / 2.0,
        (fd.uv[1].0 + fd.uv[1].1) / 2.0,
    );
    let up = query::normal(surface, middle);
    let first = surface.control_point(0, 0);
    let (mut reach, mut rise) = (0.0_f64, 0.0_f64);
    for p in surface.control_points() {
        reach = reach.max(p.distance(first));
        rise = rise.max((*p - first).dot(up).abs());
    }
    let flat = rise <= reach * 1e-11;
    let across = up.any_orthonormal_vector();
    let plane = |p: Point3| Point2::new((p - first).dot(across), (p - first).dot(up.cross(across)));

    let mut c: Vec<Corner> = Vec::new();
    let mut rings: Vec<Vec<usize>> = Vec::new();
    // The corners that stand at a vertex, and the vertices the surface
    // collapses to.
    let mut stands: Vec<(usize, usize)> = Vec::new();
    let mut poles: HashMap<usize, usize> = HashMap::new();
    for lp in &face.loops {
        let mut ring = Vec::new();
        for step in lp {
            match step.walk {
                Walk::Edge { edge, reversed } => {
                    let n = edge_stops[edge].len();
                    stands.push((c.len(), solid.ends(step).0));
                    for k in 0..n - 1 {
                        let i = if reversed { n - 1 - k } else { k };
                        let at = edge_points[edge][i];
                        ring.push(c.len());
                        c.push(Corner {
                            uv: if flat {
                                plane(at)
                            } else {
                                step.pcurve.point_at(edge_stops[edge][i])
                            },
                            at,
                            pole: None,
                        });
                    }
                }
                Walk::Pole { .. } if flat => {}
                Walk::Pole { vertex, range } => {
                    let (a, b) = range;
                    let (q0, q1) = (step.pcurve.point_at(a), step.pcurve.point_at(b));
                    let axis = usize::from((q1.y - q0.y).abs() > (q1.x - q0.x).abs());
                    poles.insert(vertex, axis);
                    // As finely as the face is round half-way across.
                    let half = (fd.uv[1 - axis].0 + fd.uv[1 - axis].1) / 2.0;
                    let round = |s: f64| {
                        let q = step.pcurve.point_at(s);
                        if axis == 0 {
                            surface.point_at(q.x, half)
                        } else {
                            surface.point_at(half, q.y)
                        }
                    };
                    let breaks: Vec<f64> =
                        (0..=4).map(|k| a + (b - a) * f64::from(k) / 4.0).collect();
                    let along = stops(&round, &breaks, false, tolerance);
                    for &s in &along[..along.len() - 1] {
                        ring.push(c.len());
                        c.push(Corner {
                            uv: step.pcurve.point_at(s),
                            at: solid.vertices[vertex].point,
                            pole: Some(axis),
                        });
                    }
                }
            }
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    for (corner, vertex) in stands {
        if let Some(&axis) = poles.get(&vertex) {
            c[corner].pole = Some(axis);
        }
    }
    let mut triangles = triangulate(&c, &rings);
    if !flat {
        let mut sides: HashSet<(usize, usize)> = HashSet::new();
        for ring in &rings {
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                sides.insert((a.min(b), a.max(b)));
            }
        }
        let mut net = Net {
            surface,
            c,
            sides,
            cut: HashMap::new(),
            near: tolerance * 0.7,
        };
        // Corners within the face too, where lines of constant u and v
        // cross that are as close as keeps the surface between them
        // near; all laid out well in (u, v) stretched as the surface
        // stretches it; then cut finer wherever that was not enough.
        let (_, du, dv) = surface.derivatives_at(middle.x, middle.y);
        let laid = Point2::new(du.length(), dv.length());
        let laid = (laid.x > 0.0 && laid.y > 0.0).then_some(laid);
        if let Some(laid) = laid {
            let lines = [0, 1].map(|axis| grid_lines(surface, fd.uv, axis, tolerance * 0.5));
            let ring_sides: Vec<(Point2, Point2)> = rings
                .iter()
                .flat_map(|r| (0..r.len()).map(|k| (r[k], r[(k + 1) % r.len()])))
                .map(|(a, b)| (net.c[a].uv * laid, net.c[b].uv * laid))
                .collect();
            for i in 1..lines[0].len().saturating_sub(1) {
                for j in 1..lines[1].len().saturating_sub(1) {
                    checkpoint();
                    let uv = Point2::new(lines[0][i], lines[1][j]);
                    let room = ((lines[0][i + 1] - lines[0][i]).min(lines[0][i] - lines[0][i - 1])
                        * laid.x)
                        .min(
                            (lines[1][j + 1] - lines[1][j]).min(lines[1][j] - lines[1][j - 1])
                                * laid.y,
                        )
                        * 0.4;
                    let clear = ring_sides.iter().all(|&(a, b)| {
                        let (p, chord) = (uv * laid - a, b - a);
                        let along =
                            (p.dot(chord) / chord.length_squared().max(1e-300)).clamp(0.0, 1.0);
                        (p - chord * along).length() > room
                    });
                    if clear && fd.trim.holds(uv) == Some(true) {
                        net.c.push(Corner {
                            uv,
                            at: surface.point_at(uv.x, uv.y),
                            pole: None,
                        });
                        let m = net.c.len() - 1;
                        net.insert(&mut triangles, m);
                    }
                }
            }
            triangles = net.turned(triangles, Some(laid));
        }
        for _ in 0..8 {
            let (cut, any) = net.refined(triangles);
            triangles = cut;
            if !any {
                break;
            }
            triangles = net.turned(triangles, laid);
        }
        c = net.c;
    }

    // What collapsed to nothing goes; a corner at a pole is each
    // triangle's own, with the normal the surface has along it there.
    let outward = |n: Point3| if face.outward { n } else { -n };
    let mut shown = ShownFace::default();
    let mut placed: HashMap<usize, usize> = HashMap::new();
    for t in triangles {
        let p = t.map(|i| c[i].at);
        if p[0] == p[1] || p[1] == p[2] || p[2] == p[0] {
            continue;
        }
        let mut corners = [0; 3];
        for n in 0..3 {
            let corner = c[t[n]];
            corners[n] = match corner.pole {
                Some(axis) if !flat => {
                    let (a, b) = (c[t[(n + 1) % 3]].uv, c[t[(n + 2) % 3]].uv);
                    let mut uv = corner.uv;
                    uv[axis] = (a[axis] + b[axis]) / 2.0;
                    shown.positions.push(corner.at);
                    shown.normals.push(outward(query::normal(surface, uv)));
                    shown.positions.len() - 1
                }
                _ => *placed.entry(t[n]).or_insert_with(|| {
                    shown.positions.push(corner.at);
                    let n = if flat {
                        up
                    } else {
                        query::normal(surface, corner.uv)
                    };
                    shown.normals.push(outward(n));
                    shown.positions.len() - 1
                }),
            };
        }
        if !face.outward {
            corners.swap(1, 2);
        }
        shown.triangles.push(corners);
    }
    shown
}

/// How much `surface` bends at `uv` across the direction `across` (a
/// unit vector along it there): the curvature of its cut by the plane
/// through its normal and `across`, measured against the normal `n`.
fn bend(surface: &NurbsSurface, uv: Point2, across: Point3, n: Point3, step: f64) -> Option<f64> {
    let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
    // The step in (u, v) that moves by `across`.
    let (a, b, d) = (du.dot(du), du.dot(dv), dv.dot(dv));
    let det = a * d - b * b;
    if det.abs() <= 1e-12 * a * d {
        return None;
    }
    let (r0, r1) = (du.dot(across), dv.dot(across));
    let w = Point2::new((r0 * d - r1 * b) / det, (r1 * a - r0 * b) / det) * step;
    let ((u0, u1), (v0, v1)) = surface.domain();
    let to = uv + w;
    let to = Point2::new(to.x.clamp(u0, u1), to.y.clamp(v0, v1));
    let moved = surface.point_at(to.x, to.y) - surface.point_at(uv.x, uv.y);
    let along = moved.dot(across);
    if along.abs() < step * 0.1 {
        return None;
    }
    let m = query::normal(surface, to);
    let m = if m.dot(n) < 0.0 { -m } else { m };
    Some(-(m - n).dot(across) / along)
}

/// `solid` as a viewer shows it, no triangle and no chord of an edge
/// farther than `tolerance` from what it stands for.
pub fn display(solid: &Solid, tolerance: f64) -> Shown {
    let data = SolidData::new(solid);
    let size = data.bounds.size().max(f64::MIN_POSITIVE);
    let mut edge_stops = Vec::new();
    let mut edge_points: Vec<Vec<Point3>> = Vec::new();
    for edge in &solid.edges {
        let (lo, hi) = edge.range;
        let mut breaks = vec![lo];
        for &k in edge.curve.knots() {
            if k > lo && k < hi && breaks.last().is_none_or(|&l| k > l) {
                breaks.push(k);
            }
        }
        breaks.push(hi);
        let straight = edge.curve.degree() <= 1 && !edge.curve.is_rational();
        let along = stops(&|t| edge.point(t), &breaks, straight, tolerance);
        let mut points: Vec<Point3> = along.iter().map(|&t| edge.point(t)).collect();
        // (Its ends are its vertices, for every face alike.)
        let n = points.len();
        points[0] = solid.vertices[edge.ends[0]].point;
        points[n - 1] = solid.vertices[edge.ends[1]].point;
        edge_stops.push(along);
        edge_points.push(points);
    }
    let mut shown = Shown::default();
    for (face, fd) in solid.faces.iter().zip(&data.faces) {
        let made = shown_face(solid, face, fd, &edge_stops, &edge_points, tolerance);
        shown.faces.push(made);
    }
    // Each edge with the faces that walk it.
    let mut uses: Vec<Vec<(usize, usize, usize)>> = vec![Vec::new(); solid.edges.len()];
    for (f, face) in solid.faces.iter().enumerate() {
        for (l, lp) in face.loops.iter().enumerate() {
            for (s, step) in lp.iter().enumerate() {
                if let Walk::Edge { edge, .. } = step.walk {
                    uses[edge].push((f, l, s));
                }
            }
        }
    }
    for (e, edge) in solid.edges.iter().enumerate() {
        let line = &edge_points[e];
        let length: f64 = line.windows(2).map(|w| w[0].distance(w[1])).sum();
        if uses[e].is_empty() || length <= size * 1e-12 {
            continue;
        }
        if let [(f, ..), (g, ..)] = uses[e][..]
            && f == g
        {
            shown.seams.push(line.clone());
            continue;
        }
        let mut turns = None;
        let mut split = false;
        if let [first, second] = uses[e][..] {
            let side = |(f, l, s): (usize, usize, usize), t: f64| {
                let face = &solid.faces[f];
                let step = &face.loops[l][s];
                let uv = step.pcurve.point_at(t);
                let n = query::normal(&face.surface, uv);
                let reversed = matches!(step.walk, Walk::Edge { reversed: true, .. });
                let (_, along) = edge.curve.derivative_at(t);
                let walk = if reversed { -along } else { along };
                // Its outward normal, the way into it from the edge,
                // and where on its surface this is.
                let out = if face.outward { n } else { -n };
                (out, n.cross(walk).normalize_or_zero(), uv)
            };
            let (mut least, mut most) = (f64::INFINITY, f64::NEG_INFINITY);
            for &t in &edge_stops[e] {
                let ((n1, into, _), (n2, ..)) = (side(first, t), side(second, t));
                let sharp = libm::atan2(n1.cross(n2).length(), n1.dot(n2));
                let angle = if n2.dot(into) > 0.0 { -sharp } else { sharp };
                (least, most) = (least.min(angle), most.max(angle));
            }
            turns = Some([least, most]);
            // On one surface: no turn, and bent alike across the edge.
            if least.abs().max(most.abs()) < 1e-6 {
                let t = edge.middle();
                let ((n1, into1, uv1), (n2, into2, uv2)) = (side(first, t), side(second, t));
                let step = size * 1e-4;
                let s1 = &solid.faces[first.0].surface;
                let s2 = &solid.faces[second.0].surface;
                let bends = (
                    bend(s1, uv1, into1, n1, step),
                    bend(s2, uv2, into2, n2, step),
                );
                if let (Some(k1), Some(k2)) = bends {
                    split = (k1 - k2).abs() * size < 1e-4;
                }
            }
        }
        shown.edges.push(line.clone());
        shown.edge_faces.push(uses[e].iter().map(|u| u.0).collect());
        shown.edge_turns.push(turns);
        shown.edge_split.push(split);
    }
    shown
}
