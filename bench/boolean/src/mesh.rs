//! Triangles of a solid that close up: what brep-bench's harness checks
//! a result by.
//!
//! Every edge is sampled once, and both faces along it use those same
//! points, so the triangles of neighbouring faces meet exactly. A face
//! is its loops as polygons in (u, v), cut into triangles ear by ear;
//! triangles that stray from the surface are then split along their
//! inner sides, never along an edge of the face.

use std::collections::{BTreeMap, BTreeSet};

use workpiece_boolean::Solid;
use workpiece_boolean::model::{Face, Walk};
use workpiece_core::math::{Point2, Point3};

/// A corner of a face's triangles: where it is in (u, v), and which
/// point of the solid's it is.
#[derive(Clone, Copy)]
struct Corner {
    uv: Point2,
    id: usize,
}

fn cross(a: Point2, b: Point2, c: Point2) -> f64 {
    (b - a).perp_dot(c - a)
}

fn area(polygon: &[usize], corners: &[Corner]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|k| {
            corners[polygon[k]]
                .uv
                .perp_dot(corners[polygon[(k + 1) % n]].uv)
        })
        .sum::<f64>()
        / 2.0
}

/// Whether `p` is in the triangle or on its sides — or as good as on
/// them: a corner a hair outside a side still lies on it.
fn inside(p: Point2, a: Point2, b: Point2, c: Point2) -> bool {
    let left = |from: Point2, to: Point2| {
        cross(from, to, p) >= -1e-9 * (to - from).length() * (p - from).length()
    };
    left(a, b) && left(b, c) && left(c, a)
}

/// Whether the cut from `m` to `p` crosses or touches the side from `a`
/// to `b` (other than at an end the two share).
fn blocked(m: Point2, p: Point2, a: Point2, b: Point2) -> bool {
    let near = 1e-9 * (p - m).length();
    // How far `q` is to the left of the line from `x` to `y`.
    let side = |x: Point2, y: Point2, q: Point2| cross(x, y, q) / (y - x).length().max(1e-300);
    let on_cut = |q: Point2| {
        q != m
            && q != p
            && side(m, p, q).abs() <= near
            && (q - m).dot(p - m) > 0.0
            && (q - p).dot(m - p) > 0.0
    };
    if on_cut(a) || on_cut(b) {
        return true;
    }
    if a == m || a == p || b == m || b == p {
        return false;
    }
    let (s1, s2) = (side(m, p, a), side(m, p, b));
    let (t1, t2) = (side(a, b, m), side(a, b, p));
    s1 * s2 < 0.0 && t1 * t2 < 0.0
}

/// How far right `p` is, seen a little askew.
fn askew(p: Point2) -> f64 {
    p.x + 0.3713 * p.y
}

/// Joins `hole` (clockwise) into `outer` (counter-clockwise) by a cut
/// from its rightmost corner to the nearest corner of the outline that
/// it can see: one the cut reaches without crossing a side — of the
/// outline, of the hole, or of the holes not joined yet (`rest`) — or
/// running through another corner.
fn join(outer: &mut Vec<usize>, hole: &[usize], rest: &[&Vec<usize>], corners: &[Corner]) {
    let at = |k: usize| corners[k].uv;
    // (Rightmost a little askew: holes in rows would otherwise all be
    // cut along one line, through each other's corners.)
    let Some(start) =
        (0..hole.len()).max_by(|&a, &b| askew(at(hole[a])).total_cmp(&askew(at(hole[b]))))
    else {
        return;
    };
    let m = at(hole[start]);
    let mut order: Vec<usize> = (0..outer.len()).collect();
    order.sort_by(|&a, &b| {
        at(outer[a])
            .distance(m)
            .total_cmp(&at(outer[b]).distance(m))
    });
    let sides =
        |polygon: &[usize], k: usize| (at(polygon[k]), at(polygon[(k + 1) % polygon.len()]));
    let pick = order.into_iter().find(|&k| {
        let p = at(outer[k]);
        p != m
            && !(0..outer.len()).any(|e| {
                let (a, b) = sides(outer, e);
                blocked(m, p, a, b)
            })
            && !std::iter::once(hole)
                .chain(rest.iter().map(|h| h.as_slice()))
                .any(|other| {
                    (0..other.len()).any(|e| {
                        let (a, b) = sides(other, e);
                        blocked(m, p, a, b)
                    })
                })
    });
    let Some(pick) = pick else {
        return;
    };
    let mut joined = Vec::with_capacity(outer.len() + hole.len() + 2);
    joined.extend_from_slice(&outer[..=pick]);
    joined.extend((0..=hole.len()).map(|k| hole[(start + k) % hole.len()]));
    joined.extend_from_slice(&outer[pick..]);
    *outer = joined;
}

/// Cuts the polygon (counter-clockwise) into triangles, ear by ear:
/// always the ear cut off by the shortest side there is (in space), so
/// that a band between two rims is cut across, not along.
fn ears(polygon: Vec<usize>, corners: &[Corner], points: &[Point3]) -> Vec<[usize; 3]> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let at = |k: usize| corners[k].uv;
    let n = polygon.len();
    let mut out = Vec::with_capacity(n);
    if n < 3 {
        return out;
    }
    // The corners still there, each with the one before and after it,
    // and how often its neighbours have changed.
    let mut before: Vec<usize> = (0..n).map(|k| (k + n - 1) % n).collect();
    let mut after: Vec<usize> = (0..n).map(|k| (k + 1) % n).collect();
    let mut there = vec![true; n];
    let mut stamp = vec![0u32; n];
    let mut left = n;
    // The length of the side that cuts corner `k` off: never negative,
    // so its bits are in its order.
    let side = |k: usize, before: &[usize], after: &[usize]| {
        let (a, c) = (
            corners[polygon[before[k]]].id,
            corners[polygon[after[k]]].id,
        );
        points[a].distance_squared(points[c]).to_bits()
    };
    let mut heap: BinaryHeap<Reverse<(u64, usize, u32)>> = (0..n)
        .map(|k| Reverse((side(k, &before, &after), k, 0)))
        .collect();
    // The corners by where they are, in squares: only those in the
    // squares a triangle reaches can be in it.
    let (mut lo, mut hi) = (Point2::INFINITY, Point2::NEG_INFINITY);
    for &k in &polygon {
        (lo, hi) = (lo.min(at(k)), hi.max(at(k)));
    }
    let across = ((n as f64).sqrt().ceil() as usize).clamp(1, 512);
    let size = ((hi - lo) / across as f64).max(Point2::splat(1e-300));
    let square = |p: Point2| {
        let c = ((p - lo) / size).floor();
        (
            (c.x.max(0.0) as usize).min(across - 1),
            (c.y.max(0.0) as usize).min(across - 1),
        )
    };
    let mut squares: Vec<Vec<usize>> = vec![Vec::new(); across * across];
    for (k, &corner) in polygon.iter().enumerate() {
        let (x, y) = square(at(corner));
        squares[y * across + x].push(k);
    }
    // Corners that were no ears when they came up: looked at again
    // once every other has been.
    let mut waiting: Vec<usize> = Vec::new();
    let mut cut = false;
    let mut slack = 1e-6;
    while left > 3 {
        let Some(Reverse((_, k, seen))) = heap.pop() else {
            if waiting.is_empty() || (!cut && slack == 0.0) {
                break;
            }
            // Nothing left that is clearly an ear: corners in a line
            // are cut off too, as triangles of no area, so that what
            // is beyond them can be reached. (A fan over what is left
            // would lie outside an outline that bends inwards.)
            if !cut {
                slack = 0.0;
            }
            cut = false;
            for m in waiting.drain(..) {
                if there[m] {
                    heap.push(Reverse((side(m, &before, &after), m, stamp[m])));
                }
            }
            continue;
        };
        if !there[k] || stamp[k] != seen {
            continue;
        }
        let (i, j, l) = (polygon[before[k]], polygon[k], polygon[after[k]]);
        let (a, b, c) = (at(i), at(j), at(l));
        // Corners in a line are no ears.
        let mut ear = if slack > 0.0 {
            cross(a, b, c) > slack * (b - a).length() * (c - b).length()
        } else {
            cross(a, b, c) >= -1e-12 * (b - a).length() * (c - b).length()
                && (b - a).dot(c - b) >= 0.0
        };
        if ear {
            let (from, to) = (square(a.min(b).min(c)), square(a.max(b).max(c)));
            'squares: for y in from.1..=to.1 {
                for x in from.0..=to.0 {
                    for &q in &squares[y * across + x] {
                        let p = at(polygon[q]);
                        if there[q] && p != a && p != b && p != c && inside(p, a, b, c) {
                            ear = false;
                            break 'squares;
                        }
                    }
                }
            }
        }
        if !ear {
            waiting.push(k);
            continue;
        }
        out.push([i, j, l]);
        there[k] = false;
        left -= 1;
        let (p, q) = (before[k], after[k]);
        after[p] = q;
        before[q] = p;
        cut = true;
        for m in [p, q] {
            stamp[m] += 1;
            heap.push(Reverse((side(m, &before, &after), m, stamp[m])));
        }
    }
    // What is left when no ear is found (corners in a line): a fan.
    let Some(first) = (0..n).find(|&k| there[k]) else {
        return out;
    };
    let mut rest = vec![first];
    let mut k = after[first];
    while k != first {
        rest.push(k);
        k = after[k];
    }
    for w in 1..rest.len().saturating_sub(1) {
        out.push([polygon[rest[0]], polygon[rest[w]], polygon[rest[w + 1]]]);
    }
    out
}

/// The triangles of `solid`, within about `sag` of its faces.
pub fn triangles(solid: &Solid, sag: f64) -> Vec<[Point3; 3]> {
    by_face(solid, sag).concat()
}

/// The triangles of each face of `solid`, in the order of its faces.
pub fn by_face(solid: &Solid, sag: f64) -> Vec<Vec<[Point3; 3]>> {
    let mut points: Vec<Point3> = solid.vertices.iter().map(|v| v.point).collect();
    // Each edge's points, once: parameter and point, from its start to
    // its end.
    let mut along: Vec<Vec<(f64, usize)>> = Vec::with_capacity(solid.edges.len());
    for e in &solid.edges {
        let (a, b) = e.range;
        let mut stops = vec![a];
        // (Not a knot a hair from an end: that would be the end's own
        // point over again, by another name.)
        let hair = (b - a) * 1e-9;
        let inside = |k: &f64| *k > a + hair && *k < b - hair;
        stops.extend(e.curve.knots().iter().copied().filter(inside));
        stops.push(b);
        stops.dedup();
        let mut samples = vec![(a, e.ends[0])];
        for pair in stops.windows(2) {
            // As many steps as the span turns, and as its bulge asks.
            let d = |f: f64| e.curve.derivative_at(pair[0] + (pair[1] - pair[0]) * f).1;
            let turn = |x: Point3, y: Point3| x.angle_between(y);
            let turned = turn(d(1e-3), d(0.5)) + turn(d(0.5), d(1.0 - 1e-3));
            let chord = e.point(pair[0]).distance(e.point(pair[1]));
            // (A chord of length c under an arc that turns by θ is
            // cθ/8 from it; in n steps, n² times nearer.)
            let by_sag = (turned * chord / (8.0 * sag.max(1e-12))).sqrt().ceil();
            let steps = ((turned / 0.2).ceil().max(by_sag) as usize).clamp(1, 256);
            // The stops of those steps, and more wherever the curve
            // between two is further than the sag from their chord (a
            // bend tighter than its span's ends tell of).
            let mut at: Vec<f64> = (0..=steps)
                .map(|k| pair[0] + (pair[1] - pair[0]) * k as f64 / steps as f64)
                .collect();
            for _ in 0..8 {
                let mut more = Vec::with_capacity(at.len() * 2);
                let mut split = false;
                for w in at.windows(2) {
                    more.push(w[0]);
                    let (p, q) = (e.point(w[0]), e.point(w[1]));
                    let strays = [0.25, 0.5, 0.75].into_iter().any(|f| {
                        let on = e.point(w[0] + (w[1] - w[0]) * f);
                        let along = ((on - p).dot(q - p) / (q - p).length_squared().max(1e-300))
                            .clamp(0.0, 1.0);
                        on.distance(p + (q - p) * along) > sag
                    });
                    if strays {
                        more.push((w[0] + w[1]) / 2.0);
                        split = true;
                    }
                }
                more.push(pair[1]);
                at = more;
                if !split {
                    break;
                }
            }
            for &t in &at[1..] {
                if t == pair[1] && pair[1] == b {
                    samples.push((b, e.ends[1]));
                } else {
                    points.push(e.point(t));
                    samples.push((t, points.len() - 1));
                }
            }
        }
        along.push(samples);
    }
    let mut used = BTreeSet::new();
    solid
        .faces
        .iter()
        .map(|face| {
            let mut out = Vec::new();
            mesh_face(face, &along, &mut points, sag, &mut used, &mut out);
            out
        })
        .collect()
}

/// The triangles of one face. `used` holds the inner sides of the faces
/// before it, by their points: where two faces touch between the same
/// edges, both would draw the same side between the edges' points, and
/// four triangles would meet along it; the later face splits its own.
fn mesh_face(
    face: &Face,
    along: &[Vec<(f64, usize)>],
    points: &mut Vec<Point3>,
    sag: f64,
    used: &mut BTreeSet<(usize, usize)>,
    out: &mut Vec<[Point3; 3]>,
) {
    let mut corners: Vec<Corner> = Vec::new();
    let mut loops: Vec<Vec<usize>> = Vec::new();
    for steps in &face.loops {
        let mut polygon = Vec::new();
        for step in steps {
            match step.walk {
                Walk::Edge { edge, reversed } => {
                    let samples = &along[edge];
                    let n = samples.len();
                    // All but the last: the next step starts there.
                    for k in 0..n - 1 {
                        let (t, id) = if reversed {
                            samples[n - 1 - k]
                        } else {
                            samples[k]
                        };
                        polygon.push(corners.len());
                        corners.push(Corner {
                            uv: step.pcurve.point_at(t),
                            id,
                        });
                    }
                }
                Walk::Pole { vertex, range } => {
                    polygon.push(corners.len());
                    corners.push(Corner {
                        uv: step.pcurve.point_at(range.0),
                        id: vertex,
                    });
                }
            }
        }
        if polygon.len() >= 3 {
            loops.push(polygon);
        }
    }
    // The outline is the loop that goes around the most; the others are
    // holes in it.
    loops.sort_by(|a, b| area(b, &corners).total_cmp(&area(a, &corners)));
    let Some(mut outer) = loops.first().cloned() else {
        return;
    };
    let mut sides: BTreeSet<(usize, usize)> = BTreeSet::new();
    for polygon in &loops {
        for k in 0..polygon.len() {
            let (a, b) = (polygon[k], polygon[(k + 1) % polygon.len()]);
            sides.insert((a.min(b), a.max(b)));
        }
    }
    let mut holes: Vec<&Vec<usize>> = loops[1..].iter().collect();
    holes.sort_by(|a, b| {
        let right = |h: &Vec<usize>| {
            h.iter()
                .map(|&k| askew(corners[k].uv))
                .fold(f64::NEG_INFINITY, f64::max)
        };
        right(b).total_cmp(&right(a))
    });
    for (k, hole) in holes.iter().enumerate() {
        join(&mut outer, hole, &holes[k + 1..], &corners);
    }
    let mut triangles = ears(outer, &corners, points);
    // Split inner sides whose middle on the surface is off the side.
    let surface = &face.surface;
    let flat = surface.degrees() == (1, 1);
    // (Twelve rounds for the surface's sake; then as many again that
    // split only sides drawn twice, which a last round may have left.)
    for round in 0..24 {
        let late = round >= 12;
        let mut split: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        // A triangle whose middle is off the surface has its longest
        // inner side split, whatever its sides' own points say.
        let mut forced: BTreeSet<(usize, usize)> = BTreeSet::new();
        if !flat && !late {
            for t in &triangles {
                let uv = (corners[t[0]].uv + corners[t[1]].uv + corners[t[2]].uv) / 3.0;
                let (on, du, dv) = surface.derivatives_at(uv.x, uv.y);
                let p = t.map(|k| points[corners[k].id]);
                let middle = (p[0] + p[1] + p[2]) / 3.0;
                if du.cross(dv).normalize_or_zero().dot(on - middle).abs() <= sag {
                    continue;
                }
                let longest = (0..3)
                    .map(|k| (t[k], t[(k + 1) % 3]))
                    .filter(|&(a, b)| !sides.contains(&(a.min(b), a.max(b))))
                    .max_by(|x, y| {
                        let far = |&(a, b): &(usize, usize)| {
                            points[corners[a].id].distance(points[corners[b].id])
                        };
                        far(x).total_cmp(&far(y))
                    });
                if let Some((a, b)) = longest {
                    forced.insert((a.min(b), a.max(b)));
                }
            }
        }
        // Inner sides between the same two points of the solid twice
        // over — to a point on a seam from both sides of it, around a
        // face that closes on itself — would be one side of four
        // triangles: each is split.
        // (Points by where they are, to a part in a million millions
        // of how far they reach: where a surface comes to a point
        // inside a face, the corners along it are all at one place.)
        let reach = points
            .iter()
            .fold(0.0_f64, |m, p| m.max(p.abs().max_element()));
        let grain = (reach * 1e-12).max(f64::MIN_POSITIVE);
        let place = |id: usize| points[id].to_array().map(|x| (x / grain).round() as i64);
        type Places = ([i64; 3], [i64; 3]);
        // (Counted by the triangles that are triangles: one with two
        // corners at one place is none, and is left out in the end —
        // its two long sides are then one, between its neighbours.)
        let mut between: BTreeMap<Places, (usize, BTreeSet<(usize, usize)>)> = BTreeMap::new();
        for t in &triangles {
            let at = t.map(|k| place(corners[k].id));
            if at[0] == at[1] || at[1] == at[2] || at[0] == at[2] {
                continue;
            }
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = (a.min(b), a.max(b));
                let (i, j) = (at[k], at[(k + 1) % 3]);
                let entry = between.entry((i.min(j), i.max(j))).or_default();
                entry.0 += 1;
                if !sides.contains(&key) {
                    entry.1.insert(key);
                }
            }
        }
        let twice: BTreeSet<(usize, usize)> = between
            .into_values()
            .filter(|(count, _)| *count > 2)
            .flat_map(|(_, keys)| keys)
            .collect();
        for t in &triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = (a.min(b), a.max(b));
                if sides.contains(&key) || split.contains_key(&key) {
                    continue;
                }
                let uv = (corners[a].uv + corners[b].uv) / 2.0;
                let (on, du, dv) = surface.derivatives_at(uv.x, uv.y);
                let chord = (points[corners[a].id] + points[corners[b].id]) / 2.0;
                // Off the surface, not along it: a flat face is never
                // split, however its (u, v) run. (Looked at a quarter
                // and three quarters along too: a side across a wave
                // of the surface has its middle on it.)
                let mut off = du.cross(dv).normalize_or_zero().dot(on - chord).abs();
                if !flat {
                    for f in [0.25, 0.75] {
                        let uv = corners[a].uv.lerp(corners[b].uv, f);
                        let (on, du, dv) = surface.derivatives_at(uv.x, uv.y);
                        let chord = points[corners[a].id].lerp(points[corners[b].id], f);
                        off = off.max(du.cross(dv).normalize_or_zero().dot(on - chord).abs());
                    }
                }
                // Nor may a side be far from where its middle is on
                // the surface: across a face that goes around an axis
                // it would cut the corner.
                let side = points[corners[a].id].distance(points[corners[b].id]);
                let (i, j) = (corners[a].id, corners[b].id);
                let taken = used.contains(&(i.min(j), i.max(j))) || twice.contains(&key);
                let strays =
                    forced.contains(&key) || off > sag || on.distance(chord) > side * 0.1 + sag;
                if taken || (!late && strays) {
                    points.push(on);
                    corners.push(Corner {
                        uv,
                        id: points.len() - 1,
                    });
                    split.insert(key, corners.len() - 1);
                }
            }
        }
        if split.is_empty() {
            break;
        }
        let mut next = Vec::with_capacity(triangles.len() * 2);
        for t in &triangles {
            let mid = |k: usize| {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                split.get(&(a.min(b), a.max(b))).copied()
            };
            match [mid(0), mid(1), mid(2)] {
                [None, None, None] => next.push(*t),
                [Some(m), None, None] => next.extend([[t[0], m, t[2]], [m, t[1], t[2]]]),
                [None, Some(m), None] => next.extend([[t[0], t[1], m], [t[0], m, t[2]]]),
                [None, None, Some(m)] => next.extend([[t[0], t[1], m], [m, t[1], t[2]]]),
                [Some(m), Some(n), None] => {
                    next.extend([[t[0], m, t[2]], [m, n, t[2]], [m, t[1], n]])
                }
                [None, Some(m), Some(n)] => {
                    next.extend([[t[0], t[1], n], [t[1], m, n], [n, m, t[2]]])
                }
                [Some(m), None, Some(n)] => {
                    next.extend([[t[0], m, n], [m, t[1], n], [t[1], t[2], n]])
                }
                [Some(a), Some(b), Some(c)] => {
                    next.extend([[t[0], a, c], [a, t[1], b], [c, b, t[2]], [a, b, c]]);
                }
            }
        }
        triangles = next;
    }
    // Points at one place are one point: where a surface comes to a
    // point inside a face (a torus without a hole, at its middle), the
    // corners found along it are all there.
    // (To a part in a million millions of how far the points reach: the
    // corners along such a place differ in their last bits.)
    let reach = points
        .iter()
        .fold(0.0_f64, |m, p| m.max(p.abs().max_element()));
    let grain = (reach * 1e-12).max(f64::MIN_POSITIVE);
    let mut first_at: BTreeMap<[i64; 3], usize> = BTreeMap::new();
    let mut same = |id: usize| {
        let key = points[id].to_array().map(|x| (x / grain).round() as i64);
        *first_at.entry(key).or_insert(id)
    };
    for t in triangles {
        let ids = t.map(|k| same(corners[k].id));
        if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if !sides.contains(&(a.min(b), a.max(b))) {
                let (i, j) = (ids[k], ids[(k + 1) % 3]);
                used.insert((i.min(j), i.max(j)));
            }
        }
        let p = ids.map(|k| points[k]);
        out.push(if face.outward { p } else { [p[0], p[2], p[1]] });
    }
}

/// Writes `triangles` as an ASCII STL file, with every digit of their
/// points: the single precision of a binary one would join the two
/// sides of a solid thinner than it can tell apart.
pub fn write_stl(path: &std::path::Path, triangles: &[[Point3; 3]]) -> std::io::Result<()> {
    use std::fmt::Write;
    let mut text = String::from("solid workpiece\n");
    for t in triangles {
        let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
        let _ = writeln!(
            text,
            "facet normal {:e} {:e} {:e}\nouter loop",
            n.x, n.y, n.z
        );
        for p in t {
            let _ = writeln!(text, "vertex {:e} {:e} {:e}", p.x, p.y, p.z);
        }
        text.push_str("endloop\nendfacet\n");
    }
    text.push_str("endsolid workpiece\n");
    std::fs::write(path, text)
}
