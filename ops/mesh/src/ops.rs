//! What is done to a mesh: each a function of the mesh and of what is
//! picked in it, giving the new mesh and what is picked in that — or
//! why it cannot be done.

use std::collections::{BTreeMap, BTreeSet};

use shapecraft_core::math::Point3;

use crate::poly::{Edge, Item, Poly, edge};

/// What an operation gives: the new mesh and what is picked in it, or
/// why it cannot be done.
pub type Done = Result<(Poly, Vec<Item>), String>;

fn no(why: &str) -> Done {
    Err(why.to_owned())
}

/// The directed edges around the region `faces`: those its faces walk
/// that no other face of it walks back.
fn rim(poly: &Poly, faces: &BTreeSet<usize>) -> Vec<(usize, usize)> {
    let mut walked = BTreeSet::new();
    for &f in faces {
        let ring = &poly.faces[f];
        for k in 0..ring.len() {
            walked.insert((ring[k], ring[(k + 1) % ring.len()]));
        }
    }
    walked
        .iter()
        .filter(|&&(a, b)| !walked.contains(&(b, a)))
        .copied()
        .collect()
}

/// How far a corner moves into its face for the edges on both sides
/// of it to move in by one: `into` and `out of` are the unit
/// directions of the edges walked to the corner and from it, `n` the
/// normal there.
fn inwards(n: Point3, into: Point3, out_of: Point3) -> Point3 {
    let (m0, m1) = (n.cross(into), n.cross(out_of));
    (m0 + m1) / (1.0 + m0.dot(m1)).max(0.2)
}

/// The region `faces` raised off the mesh: the points around it are
/// doubled and the new ones moved in by `inset` and all of the region
/// by `lift` (of a point and the normal there), with a quad from each
/// edge around it to where it was.
fn raised(
    poly: &Poly,
    faces: &BTreeSet<usize>,
    inset: f64,
    lift: &dyn Fn(usize, Point3) -> Point3,
) -> Poly {
    let around = rim(poly, faces);
    let mut normals = vec![Point3::ZERO; poly.points.len()];
    for &f in faces {
        let n = poly.area_normal(f);
        for &v in &poly.faces[f] {
            normals[v] += n;
        }
    }
    let normal = |v: usize| normals[v].normalize_or_zero();
    let mut made = poly.clone();
    // The points around the region, doubled.
    let mut double: BTreeMap<usize, usize> = BTreeMap::new();
    let into: BTreeMap<usize, usize> = around.iter().map(|&(a, b)| (b, a)).collect();
    let out_of: BTreeMap<usize, usize> = around.iter().copied().collect();
    for &(a, _) in &around {
        let n = normal(a);
        let mut at = poly.points[a] + lift(a, n);
        if let (Some(&from), Some(&to)) = (into.get(&a), out_of.get(&a)) {
            let d0 = (poly.points[a] - poly.points[from]).normalize_or_zero();
            let d1 = (poly.points[to] - poly.points[a]).normalize_or_zero();
            at += inwards(n, d0, d1) * inset;
        }
        made.points.push(at);
        double.insert(a, made.points.len() - 1);
    }
    // Those inside it only move.
    for v in poly.points_of(&faces.iter().map(|&f| Item::Face(f)).collect::<Vec<_>>()) {
        if !double.contains_key(&v) {
            made.points[v] += lift(v, normal(v));
        }
    }
    let now = |v: usize| double.get(&v).copied().unwrap_or(v);
    let mut inside: BTreeSet<Edge> = BTreeSet::new();
    for &f in faces {
        let ring = &poly.faces[f];
        for k in 0..ring.len() {
            inside.insert(edge(ring[k], ring[(k + 1) % ring.len()]));
        }
        made.faces[f] = ring.iter().map(|&v| now(v)).collect();
    }
    for &(a, b) in &around {
        inside.remove(&edge(a, b));
        made.faces.push(vec![a, b, now(b), now(a)]);
    }
    made.creases = poly
        .creases
        .iter()
        .map(|&(a, b)| {
            if inside.contains(&(a, b)) {
                edge(now(a), now(b))
            } else {
                (a, b)
            }
        })
        .collect();
    made
}

/// The picked faces pushed out by `distance` along the way they face
/// together, walls left behind them; or the picked edges, each with a
/// new face beside it.
pub fn extrude(poly: &Poly, picked: &[Item], distance: f64) -> Done {
    let faces = poly.faces_of(picked);
    if !faces.is_empty() {
        let along: Point3 = faces.iter().map(|&f| poly.area_normal(f)).sum();
        let along = along.normalize_or_zero();
        let lift = |_: usize, n: Point3| if along == Point3::ZERO { n } else { along } * distance;
        let made = raised(poly, &faces, 0.0, &lift);
        return Ok((made, faces.into_iter().map(Item::Face).collect()));
    }
    let edges = poly.edges_of(picked);
    if edges.is_empty() {
        return no("Pick the faces or edges to extrude");
    }
    let sides = poly.edges();
    // Each edge goes away from the face it is of.
    let mut away: BTreeMap<usize, Point3> = BTreeMap::new();
    for &(a, b) in &edges {
        let side = sides[&(a, b)][0];
        let along = (poly.points[b] - poly.points[a]).normalize_or_zero();
        let n = poly.normal(side.face);
        let mut out = along.cross(n);
        // (Away from the middle of the face.)
        let middle = (poly.points[a] + poly.points[b]) / 2.0;
        if out.dot(middle - poly.center(side.face)) < 0.0 {
            out = -out;
        }
        *away.entry(a).or_default() += out;
        *away.entry(b).or_default() += out;
    }
    let mut made = poly.clone();
    let mut double = BTreeMap::new();
    for (&v, out) in &away {
        made.points
            .push(poly.points[v] + out.normalize_or_zero() * distance);
        double.insert(v, made.points.len() - 1);
    }
    let mut now = Vec::new();
    for &(a, b) in &edges {
        made.add_face(vec![a, b, double[&b], double[&a]]);
        now.push(Item::Edge(
            double[&a].min(double[&b]),
            double[&a].max(double[&b]),
        ));
    }
    Ok((made, now))
}

/// The picked faces with a rim `thickness` wide set into them, and
/// what is left inside pushed out by `depth`: all that are beside each
/// other as one, or (`each`) each by itself.
pub fn inset(poly: &Poly, picked: &[Item], thickness: f64, depth: f64, each: bool) -> Done {
    let faces = poly.faces_of(picked);
    if faces.is_empty() {
        return no("Pick the faces to inset");
    }
    let lift = |_: usize, n: Point3| n * depth;
    let mut made = poly.clone();
    if each {
        for &f in &faces {
            made = raised(&made, &BTreeSet::from([f]), thickness, &lift);
        }
    } else {
        made = raised(&made, &faces, thickness, &lift);
    }
    Ok((made, faces.into_iter().map(Item::Face).collect()))
}

/// The mesh with new points on its edges: on each edge of `cuts`, at
/// the fractions given (from its lower point to its higher), in every
/// face along it. And the new points of each edge, in that order.
pub fn split_edges(
    poly: &Poly,
    cuts: &BTreeMap<Edge, Vec<f64>>,
) -> (Poly, BTreeMap<Edge, Vec<usize>>) {
    let mut made = poly.clone();
    let mut new: BTreeMap<Edge, Vec<usize>> = BTreeMap::new();
    for (&(a, b), at) in cuts {
        let mut points = Vec::new();
        for &t in at {
            made.points.push(poly.points[a].lerp(poly.points[b], t));
            points.push(made.points.len() - 1);
        }
        if made.creases.remove(&(a, b)) {
            let mut along = vec![a];
            along.extend(&points);
            along.push(b);
            made.creases
                .extend(along.windows(2).map(|w| edge(w[0], w[1])));
        }
        new.insert((a, b), points);
    }
    for ring in &mut made.faces {
        let mut with = Vec::with_capacity(ring.len());
        for k in 0..ring.len() {
            let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
            with.push(a);
            if let Some(points) = new.get(&edge(a, b)) {
                if a < b {
                    with.extend(points);
                } else {
                    with.extend(points.iter().rev());
                }
            }
        }
        *ring = with;
    }
    (made, new)
}

/// Cuts the face that has both `a` and `b`, not beside each other, in
/// two along the line between them. Whether there was one.
pub fn connect(poly: &mut Poly, a: usize, b: usize) -> bool {
    for f in 0..poly.faces.len() {
        let ring = &poly.faces[f];
        let (Some(i), Some(j)) = (
            ring.iter().position(|&v| v == a),
            ring.iter().position(|&v| v == b),
        ) else {
            continue;
        };
        let n = ring.len();
        let apart = (i + n - j) % n;
        if apart < 2 || n - apart < 2 {
            continue;
        }
        let (lo, hi) = (i.min(j), i.max(j));
        let one: Vec<usize> = ring[lo..=hi].to_vec();
        let mut other: Vec<usize> = ring[hi..].to_vec();
        other.extend(&ring[..=lo]);
        poly.faces[f] = one;
        poly.faces.push(other);
        return true;
    }
    false
}

/// The edges across the quads from `start`, one after the other, each
/// as its points on the one side and on the other; and whether they
/// come back around to the first.
pub fn ring(poly: &Poly, start: Edge) -> (Vec<(usize, usize)>, bool) {
    let sides = poly.edges();
    let Some(first) = sides.get(&start) else {
        return (Vec::new(), false);
    };
    // From an edge across the quad `face` to the edge over from it.
    let across = |(a, b): (usize, usize), face: usize| -> Option<(usize, usize)> {
        let ring = &poly.faces[face];
        if ring.len() != 4 {
            return None;
        }
        let i = ring.iter().position(|&v| v == a)?;
        if ring[(i + 1) % 4] == b {
            Some((ring[(i + 3) % 4], ring[(i + 2) % 4]))
        } else if ring[(i + 3) % 4] == b {
            Some((ring[(i + 1) % 4], ring[(i + 2) % 4]))
        } else {
            None
        }
    };
    let walk = |mut face: Option<usize>| {
        let mut found = Vec::new();
        let mut at = start;
        while let Some(f) = face {
            let Some(next) = across(at, f) else { break };
            if edge(next.0, next.1) == start {
                return (found, true);
            }
            found.push(next);
            if found.len() > poly.faces.len() {
                break;
            }
            let over = &sides[&edge(next.0, next.1)];
            face = over.iter().map(|s| s.face).find(|&g| g != f);
            at = next;
        }
        (found, false)
    };
    let (forth, closed) = walk(first.first().map(|s| s.face));
    let mut all = vec![start];
    all.extend(forth);
    if !closed {
        let (back, _) = walk(first.get(1).map(|s| s.face));
        let mut whole: Vec<_> = back.into_iter().rev().collect();
        whole.extend(all);
        all = whole;
    }
    (all, closed)
}

/// The edges on from `start` end to end, each going straight on
/// through the points where four edges meet, or along the rim of the
/// mesh.
pub fn edge_loop(poly: &Poly, start: Edge) -> BTreeSet<Edge> {
    let sides = poly.edges();
    let beside = poly.beside();
    let mut found = BTreeSet::from([start]);
    if !sides.contains_key(&start) {
        return found;
    }
    let open = |e: Edge| sides.get(&e).is_some_and(|s| s.len() == 1);
    for (mut from, mut at) in [start, (start.1, start.0)] {
        loop {
            let on = if open(edge(from, at)) {
                // Along the rim.
                let mut next = beside[at]
                    .iter()
                    .filter(|&&n| n != from && open(edge(at, n)));
                match (next.next(), next.next()) {
                    (Some(&n), None) => Some(n),
                    _ => None,
                }
            } else if beside[at].len() == 4 {
                // The edge that is in no face with this one.
                let faces: Vec<usize> = sides[&edge(from, at)].iter().map(|s| s.face).collect();
                beside[at].iter().copied().find(|&n| {
                    n != from && sides[&edge(at, n)].iter().all(|s| !faces.contains(&s.face))
                })
            } else {
                None
            };
            let Some(next) = on else { break };
            if !found.insert(edge(at, next)) {
                break;
            }
            (from, at) = (at, next);
        }
    }
    found
}

/// Cuts around the mesh across the quads from `start`: `cuts` new
/// loops of edges, evenly apart and moved by `slide` (between −1 and
/// 1) of the room they have.
pub fn loop_cut(poly: &Poly, start: Edge, cuts: usize, slide: f64) -> Done {
    let (across, closed) = ring(poly, start);
    if across.len() < 2 {
        return no("The edge is beside no quad to cut across");
    }
    let cuts = cuts.clamp(1, 64);
    let slide = slide.clamp(-0.95, 0.95);
    let at: Vec<f64> = (0..cuts)
        .map(|k| (k as f64 + 1.0 + slide) / (cuts as f64 + 1.0))
        .collect();
    let mut wanted = BTreeMap::new();
    for &(a, b) in &across {
        let fractions: Vec<f64> = if a < b {
            at.clone()
        } else {
            at.iter().rev().map(|t| 1.0 - t).collect()
        };
        wanted.insert(edge(a, b), fractions);
    }
    let (mut made, new) = split_edges(poly, &wanted);
    // The new points of an edge, from its one side to its other.
    let along = |(a, b): (usize, usize)| -> Vec<usize> {
        let points = new[&edge(a, b)].clone();
        if a < b {
            points
        } else {
            points.into_iter().rev().collect()
        }
    };
    let mut picked = Vec::new();
    let n = across.len();
    for i in 0..if closed { n } else { n - 1 } {
        let (here, there) = (along(across[i]), along(across[(i + 1) % n]));
        for k in 0..cuts {
            if connect(&mut made, here[k], there[k]) {
                picked.push(Item::Edge(here[k].min(there[k]), here[k].max(there[k])));
            }
        }
    }
    Ok((made, picked))
}

/// The picked faces cut in quads about their middles, or the picked
/// edges cut in two — and the faces between two of those cut across.
pub fn subdivide(poly: &Poly, picked: &[Item]) -> Done {
    let faces = poly.faces_of(picked);
    if !faces.is_empty() {
        let mut wanted = BTreeMap::new();
        for &f in &faces {
            let ring = &poly.faces[f];
            for k in 0..ring.len() {
                wanted.insert(edge(ring[k], ring[(k + 1) % ring.len()]), vec![0.5]);
            }
        }
        let (mut made, _) = split_edges(poly, &wanted);
        let mut now = Vec::new();
        for &f in &faces {
            let ring = made.faces[f].clone();
            let was = &poly.faces[f];
            // (The ring starts at a point that was there.)
            debug_assert_eq!(ring.len(), 2 * was.len());
            made.points.push(poly.center(f));
            let middle = made.points.len() - 1;
            let n = ring.len();
            for k in (0..n).step_by(2) {
                let quad = vec![middle, ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]];
                if k == 0 {
                    made.faces[f] = quad;
                    now.push(Item::Face(f));
                } else {
                    made.faces.push(quad);
                    now.push(Item::Face(made.faces.len() - 1));
                }
            }
        }
        return Ok((made, now));
    }
    let edges = poly.edges_of(picked);
    if edges.is_empty() {
        return no("Pick the faces or edges to subdivide");
    }
    let wanted = edges.iter().map(|&e| (e, vec![0.5])).collect();
    let (mut made, new) = split_edges(poly, &wanted);
    let middles: BTreeSet<usize> = new.values().flatten().copied().collect();
    let mut now = Vec::new();
    for f in 0..poly.faces.len() {
        let on: Vec<usize> = made.faces[f]
            .iter()
            .copied()
            .filter(|v| middles.contains(v))
            .collect();
        if let [a, b] = on[..]
            && connect(&mut made, a, b)
        {
            now.push(Item::Edge(a.min(b), a.max(b)));
        }
    }
    if now.is_empty() {
        now = middles.into_iter().map(Item::Vertex).collect();
    }
    Ok((made, now))
}

/// The picked points joined by new edges across the faces they share;
/// picked edges are cut in two first, and their middles joined.
pub fn join(poly: &Poly, picked: &[Item]) -> Done {
    if !poly.edges_of(picked).is_empty() {
        return subdivide(poly, picked);
    }
    let points: Vec<usize> = picked
        .iter()
        .filter_map(|i| match *i {
            Item::Vertex(v) => Some(v),
            _ => None,
        })
        .collect();
    if points.len() < 2 {
        return no("Pick two points of a face, or two of its edges, to connect");
    }
    let mut made = poly.clone();
    let mut now = Vec::new();
    for (k, &a) in points.iter().enumerate() {
        for &b in &points[k + 1..] {
            if connect(&mut made, a, b) {
                now.push(Item::Edge(a.min(b), a.max(b)));
            }
        }
    }
    if now.is_empty() {
        return no("The points picked share no face to cut across");
    }
    Ok((made, now))
}

/// The picked edges cut away at a slant `width` wide: a face in place
/// of each, and one at each point where they end.
pub fn bevel(poly: &Poly, picked: &[Item], width: f64) -> Done {
    let sides = poly.edges();
    let edges: BTreeSet<Edge> = poly
        .edges_of(picked)
        .into_iter()
        .filter(|e| sides[e].len() == 2)
        .collect();
    if edges.is_empty() {
        return no("Pick edges between two faces to bevel");
    }
    let cut = |a: usize, b: usize| edges.contains(&edge(a, b));
    let at_cut: BTreeSet<usize> = edges.iter().flat_map(|&(a, b)| [a, b]).collect();
    let mut made = poly.clone();
    // A point slid along an edge that stays, away from a point where
    // edges are cut: asked for by the faces on both sides of it.
    let mut slid: BTreeMap<(usize, usize), (Point3, f64)> = BTreeMap::new();
    for ring in &poly.faces {
        let n = ring.len();
        for k in 0..n {
            let (prev, v, next) = (ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]);
            let to = |x: usize| (poly.points[x] - poly.points[v]).normalize_or_zero();
            let sine = to(prev).cross(to(next)).length().max(0.2);
            for (stays, goes) in [(prev, next), (next, prev)] {
                if cut(v, goes) && !cut(v, stays) {
                    let long = poly.points[v].distance(poly.points[stays]);
                    let far = (width / sine).min(long * 0.9);
                    let entry = slid.entry((v, stays)).or_insert((Point3::ZERO, 0.0));
                    entry.0 += poly.points[v] + to(stays) * far;
                    entry.1 += 1.0;
                }
            }
        }
    }
    let mut slide: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (key, (sum, count)) in slid {
        made.points.push(sum / count);
        slide.insert(key, made.points.len() - 1);
    }
    // The corners of the faces, as they are after: of each face at
    // each point where edges are cut.
    let mut corner: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    for (f, ring) in poly.faces.iter().enumerate() {
        let n = ring.len();
        let mut with = Vec::with_capacity(n);
        for k in 0..n {
            let (prev, v, next) = (ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]);
            if !at_cut.contains(&v) {
                with.push(v);
                continue;
            }
            let now: Vec<usize> = if cut(v, prev) && cut(v, next) {
                let normal = poly.normal(f);
                let d0 = (poly.points[v] - poly.points[prev]).normalize_or_zero();
                let d1 = (poly.points[next] - poly.points[v]).normalize_or_zero();
                made.points
                    .push(poly.points[v] + inwards(normal, d0, d1) * width);
                vec![made.points.len() - 1]
            } else {
                match (slide.get(&(v, prev)), slide.get(&(v, next))) {
                    (Some(&a), Some(&b)) => vec![a, b],
                    (Some(&a), None) if cut(v, next) => vec![a],
                    (None, Some(&b)) if cut(v, prev) => vec![b],
                    (Some(&a), None) => vec![a, v],
                    (None, Some(&b)) => vec![v, b],
                    (None, None) => vec![v],
                }
            };
            with.extend(&now);
            corner.insert((f, v), now);
        }
        made.faces[f] = with;
    }
    // A face in place of each edge.
    let mut now = Vec::new();
    for &(a, b) in &edges {
        let [one, other] = [sides[&(a, b)][0].face, sides[&(a, b)][1].face];
        // The corner of a face at an end of the edge that is on the
        // edge's side: its only one, or the one not slid along another.
        let on = |f: usize, v: usize, far: usize| -> usize {
            let c = &corner[&(f, v)];
            let ring = &poly.faces[f];
            let k = ring.iter().position(|&x| x == v).unwrap_or(0);
            let n = ring.len();
            if c.len() == 1 || ring[(k + 1) % n] != far {
                c[0]
            } else {
                c[c.len() - 1]
            }
        };
        let quad = vec![
            on(one, a, b),
            on(other, a, b),
            on(other, b, a),
            on(one, b, a),
        ];
        let mut ring = quad.clone();
        ring.dedup();
        if ring.len() >= 3 {
            now.push(Item::Face(made.add_face(ring)));
        }
    }
    // A face at each point, where three corners or more are around it.
    let at = poly.faces_at();
    for &v in &at_cut {
        // Around the point: each face after the one that walks to it
        // along the edge this one walks away along.
        let fan = &at[v];
        let next_of = |f: usize| {
            let ring = &poly.faces[f];
            let k = ring.iter().position(|&x| x == v).unwrap_or(0);
            ring[(k + 1) % ring.len()]
        };
        let prev_of = |f: usize| {
            let ring = &poly.faces[f];
            let k = ring.iter().position(|&x| x == v).unwrap_or(0);
            ring[(k + ring.len() - 1) % ring.len()]
        };
        let after = |f: usize| fan.iter().copied().find(|&g| prev_of(g) == next_of(f));
        let before = |f: usize| fan.iter().copied().find(|&g| next_of(g) == prev_of(f));
        let mut first = fan[0];
        for _ in 0..fan.len() {
            match before(first) {
                Some(g) if g != fan[0] => first = g,
                _ => break,
            }
        }
        let mut around: Vec<usize> = Vec::new();
        let mut f = Some(first);
        for _ in 0..fan.len() {
            let Some(g) = f else { break };
            around.extend(&corner[&(g, v)]);
            f = after(g).filter(|&h| h != first);
        }
        around.dedup();
        while around.len() > 1 && around.first() == around.last() {
            around.pop();
        }
        if around.len() >= 3 {
            now.push(Item::Face(made.add_face(around)));
        }
    }
    made.creases = poly
        .creases
        .iter()
        .filter(|e| !edges.contains(e))
        .map(|&(a, b)| {
            let a_now = slide.get(&(a, b)).copied().unwrap_or(a);
            let b_now = slide.get(&(b, a)).copied().unwrap_or(b);
            edge(a_now, b_now)
        })
        .collect();
    let used: BTreeSet<usize> = made.faces.iter().flatten().copied().collect();
    let gone: BTreeSet<usize> = at_cut.into_iter().filter(|v| !used.contains(v)).collect();
    let (made, _) = made.without_points(&gone);
    Ok((made, now))
}

/// The loops around the region `faces`, each the points along it.
fn loops_around(poly: &Poly, faces: &BTreeSet<usize>) -> Option<Vec<Vec<usize>>> {
    let around = rim(poly, faces);
    let mut next: BTreeMap<usize, usize> = BTreeMap::new();
    for &(a, b) in &around {
        if next.insert(a, b).is_some() {
            return None;
        }
    }
    let mut loops = Vec::new();
    while let Some((&start, _)) = next.iter().next() {
        let mut ring = vec![start];
        let mut at = start;
        loop {
            at = next.remove(&at)?;
            if at == start {
                break;
            }
            ring.push(at);
        }
        loops.push(ring);
    }
    Some(loops)
}

/// The faces of `faces` that hang together, group by group.
fn groups(poly: &Poly, faces: &BTreeSet<usize>) -> Vec<BTreeSet<usize>> {
    let sides = poly.edges();
    let mut left = faces.clone();
    let mut all = Vec::new();
    while let Some(&start) = left.iter().next() {
        left.remove(&start);
        let mut group = BTreeSet::from([start]);
        let mut todo = vec![start];
        while let Some(f) = todo.pop() {
            let ring = &poly.faces[f];
            for k in 0..ring.len() {
                for side in &sides[&edge(ring[k], ring[(k + 1) % ring.len()])] {
                    if left.remove(&side.face) {
                        group.insert(side.face);
                        todo.push(side.face);
                    }
                }
            }
        }
        all.push(group);
    }
    all
}

/// The mesh with each of `regions` made one face, where one loop goes
/// around it; and the faces made.
fn merged(poly: &Poly, regions: &[BTreeSet<usize>]) -> (Poly, Vec<usize>) {
    let mut made = poly.clone();
    let mut gone = BTreeSet::new();
    let mut rings = Vec::new();
    for region in regions {
        if region.len() < 2 || region.iter().any(|f| gone.contains(f)) {
            continue;
        }
        if let Some([ring]) = loops_around(poly, region).as_deref() {
            gone.extend(region.iter().copied());
            rings.push(ring.clone());
        }
    }
    let count = poly.faces.len() - gone.len();
    made.faces = poly
        .faces
        .iter()
        .enumerate()
        .filter(|(f, _)| !gone.contains(f))
        .map(|(_, ring)| ring.clone())
        .collect();
    let new: Vec<usize> = (count..count + rings.len()).collect();
    made.faces.extend(rings);
    // The points left between two edges that go straight on are no
    // corners of anything.
    let used: BTreeSet<usize> = made.faces.iter().flatten().copied().collect();
    let orphans: BTreeSet<usize> = poly
        .faces
        .iter()
        .flatten()
        .copied()
        .filter(|v| !used.contains(v))
        .collect();
    let (made, _) = made.without_points(&orphans);
    (made, new)
}

/// The points that only two edges meet at, taken out of them: `among`
/// only.
fn unbent(poly: &Poly, among: &BTreeSet<usize>) -> (Poly, Vec<Option<usize>>) {
    let beside = poly.beside();
    let mut made = poly.clone();
    let mut gone = BTreeSet::new();
    for &v in among {
        if beside[v].len() != 2 {
            continue;
        }
        if made
            .faces
            .iter()
            .any(|ring| ring.contains(&v) && ring.len() <= 3)
        {
            continue;
        }
        for ring in &mut made.faces {
            ring.retain(|&x| x != v);
        }
        gone.insert(v);
    }
    made.without_points(&gone)
}

/// What is picked taken out and the faces around it made one: the
/// faces on both sides of an edge, around a point, or picked beside
/// each other.
pub fn dissolve(poly: &Poly, picked: &[Item]) -> Done {
    let faces = poly.faces_of(picked);
    if !faces.is_empty() {
        let (made, new) = merged(poly, &groups(poly, &faces));
        if new.is_empty() {
            return no("Pick faces beside each other to make one of");
        }
        return Ok((made, new.into_iter().map(Item::Face).collect()));
    }
    let edges = poly.edges_of(picked);
    if !edges.is_empty() {
        let sides = poly.edges();
        // The faces across the edges, as far as they hang together.
        let mut across: BTreeSet<usize> = BTreeSet::new();
        for e in &edges {
            if sides[e].len() == 2 {
                across.extend(sides[e].iter().map(|s| s.face));
            }
        }
        let mut regions: Vec<BTreeSet<usize>> = Vec::new();
        let mut left = across.clone();
        while let Some(&start) = left.iter().next() {
            left.remove(&start);
            let mut group = BTreeSet::from([start]);
            let mut todo = vec![start];
            while let Some(f) = todo.pop() {
                let ring = &poly.faces[f];
                for k in 0..ring.len() {
                    let e = edge(ring[k], ring[(k + 1) % ring.len()]);
                    if !edges.contains(&e) {
                        continue;
                    }
                    for side in &sides[&e] {
                        if left.remove(&side.face) {
                            group.insert(side.face);
                            todo.push(side.face);
                        }
                    }
                }
            }
            regions.push(group);
        }
        let ends: BTreeSet<Point3Key> = edges
            .iter()
            .flat_map(|&(a, b)| [a, b])
            .map(|v| key(poly.points[v]))
            .collect();
        let (made, _) = merged(poly, &regions);
        if made == *poly {
            return no("The edges picked are not between two faces");
        }
        let among = (0..made.points.len())
            .filter(|&v| ends.contains(&key(made.points[v])))
            .collect();
        let (made, _) = unbent(&made, &among);
        return Ok((made, Vec::new()));
    }
    let points = poly.points_of(picked);
    if points.is_empty() {
        return no("Pick what to dissolve");
    }
    let at = poly.faces_at();
    let beside = poly.beside();
    let two: BTreeSet<usize> = points
        .iter()
        .copied()
        .filter(|&v| beside[v].len() == 2)
        .collect();
    let regions: Vec<BTreeSet<usize>> = points
        .iter()
        .filter(|v| !two.contains(v))
        .map(|&v| at[v].iter().copied().collect())
        .collect();
    let marks: BTreeSet<Point3Key> = two.iter().map(|&v| key(poly.points[v])).collect();
    let (made, _) = merged(poly, &regions);
    let among = (0..made.points.len())
        .filter(|&v| marks.contains(&key(made.points[v])))
        .collect();
    let (made, _) = unbent(&made, &among);
    if made == *poly {
        return no("The points picked cannot be taken out of the faces around them");
    }
    Ok((made, Vec::new()))
}

/// A point, as what it can be found again by.
type Point3Key = [u64; 3];

fn key(p: Point3) -> Point3Key {
    p.to_array().map(f64::to_bits)
}

/// What is picked removed, with the faces at it.
pub fn delete(poly: &Poly, picked: &[Item]) -> Done {
    if picked.is_empty() {
        return no("Pick what to delete");
    }
    let mut faces = poly.faces_of(picked);
    let edges = poly.edges_of(picked);
    let sides = poly.edges();
    for e in &edges {
        faces.extend(sides[e].iter().map(|s| s.face));
    }
    let points: BTreeSet<usize> = picked
        .iter()
        .filter_map(|i| match *i {
            Item::Vertex(v) if v < poly.points.len() => Some(v),
            _ => None,
        })
        .collect();
    let (made, _) = poly.without_faces(&faces);
    // (Where the points are now: by where they are, as faces went.)
    let marks: BTreeSet<Point3Key> = points.iter().map(|&v| key(poly.points[v])).collect();
    let gone: BTreeSet<usize> = (0..made.points.len())
        .filter(|&v| marks.contains(&key(made.points[v])))
        .collect();
    let (made, _) = made.without_points(&gone);
    Ok((made, Vec::new()))
}

/// The picked points made one, in their middle: all of them, or
/// (`each`) those of each run of picked edges for themselves.
pub fn merge(poly: &Poly, picked: &[Item], each: bool) -> Done {
    let points = poly.points_of(picked);
    if points.len() < 2 {
        return no("Pick the points to merge");
    }
    let mut groups: Vec<BTreeSet<usize>> = Vec::new();
    let edges = poly.edges_of(picked);
    if each && !edges.is_empty() {
        for (run, _) in Poly::runs(&edges) {
            let run: BTreeSet<usize> = run.into_iter().collect();
            match groups.iter_mut().find(|g| !g.is_disjoint(&run)) {
                Some(group) => group.extend(run),
                None => groups.push(run),
            }
        }
    } else {
        groups.push(points);
    }
    let mut to: BTreeMap<usize, usize> = BTreeMap::new();
    let mut made = poly.clone();
    let mut kept = Vec::new();
    for group in &groups {
        let middle = group.iter().map(|&v| poly.points[v]).sum::<Point3>() / group.len() as f64;
        let first = *group.iter().next().unwrap_or(&0);
        made.points[first] = middle;
        kept.push(key(middle));
        for &v in group {
            to.insert(v, first);
        }
    }
    Ok(welded_by(&made, &to, &kept))
}

/// The mesh with each point of `to` made the point it is sent to; the
/// points at `kept` are what is picked after.
fn welded_by(poly: &Poly, to: &BTreeMap<usize, usize>, kept: &[Point3Key]) -> (Poly, Vec<Item>) {
    let now = |v: usize| to.get(&v).copied().unwrap_or(v);
    let mut made = poly.clone();
    for ring in &mut made.faces {
        for v in ring.iter_mut() {
            *v = now(*v);
        }
    }
    made.creases = poly
        .creases
        .iter()
        .map(|&(a, b)| edge(now(a), now(b)))
        .collect();
    made.creases.retain(|&(a, b)| a != b);
    made.tidy();
    let gone: BTreeSet<usize> = to
        .iter()
        .filter(|(from, to)| from != to)
        .map(|(&from, _)| from)
        .collect();
    let (made, _) = made.without_points(&gone);
    let picked = (0..made.points.len())
        .filter(|&v| kept.contains(&key(made.points[v])))
        .map(Item::Vertex)
        .collect();
    (made, picked)
}

/// The points closer together than `within` made one: those picked,
/// or all of them when none is.
pub fn weld(poly: &Poly, picked: &[Item], within: f64) -> Done {
    let mut among = poly.points_of(picked);
    if among.is_empty() {
        among = (0..poly.points.len()).collect();
    }
    let within = within.max(0.0);
    let cell = |p: Point3| {
        let c = (p / within.max(1e-12)).floor();
        [c.x as i64, c.y as i64, c.z as i64]
    };
    let mut cells: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
    let mut to: BTreeMap<usize, usize> = BTreeMap::new();
    for &v in &among {
        let p = poly.points[v];
        let [x, y, z] = cell(p);
        let mut found = None;
        'near: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &other in cells.get(&[x + dx, y + dy, z + dz]).into_iter().flatten() {
                        if poly.points[other].distance(p) <= within {
                            found = Some(other);
                            break 'near;
                        }
                    }
                }
            }
        }
        match found {
            Some(other) => {
                to.insert(v, other);
            }
            None => cells.entry([x, y, z]).or_default().push(v),
        }
    }
    if to.is_empty() {
        return no("No points are that close together");
    }
    let (made, _) = welded_by(poly, &to, &[]);
    Ok((made, Vec::new()))
}

/// Faces between two runs of picked edges that are as long: quads
/// from each edge of the one to the edge of the other it is nearest.
/// Two picked faces are bridged around, and go.
pub fn bridge(poly: &Poly, picked: &[Item]) -> Done {
    let faces = poly.faces_of(picked);
    let mut made = poly.clone();
    let mut runs: Vec<(Vec<usize>, bool)> = Vec::new();
    if faces.len() == 2 {
        for &f in &faces {
            runs.push((poly.faces[f].clone(), true));
        }
        made.faces = poly
            .faces
            .iter()
            .enumerate()
            .filter(|(f, _)| !faces.contains(f))
            .map(|(_, r)| r.clone())
            .collect();
    } else {
        runs = Poly::runs(&poly.edges_of(picked));
    }
    let [(one, closed), (other, other_closed)] = &runs[..] else {
        return no("Pick two runs of edges, or two faces, to bridge");
    };
    if one.len() != other.len() || closed != other_closed {
        return no("The two runs to bridge have to be as long as each other");
    }
    let n = one.len();
    let at = |v: usize| poly.points[v];
    // The other run, turned and (if closed) started where the two
    // lie nearest: the least way between them.
    let mut best: Option<(f64, Vec<usize>)> = None;
    for reversed in [false, true] {
        let turned: Vec<usize> = if reversed {
            other.iter().rev().copied().collect()
        } else {
            other.clone()
        };
        for shift in 0..if *closed { n } else { 1 } {
            let tried: Vec<usize> = (0..n).map(|k| turned[(k + shift) % n]).collect();
            let far: f64 = (0..n).map(|k| at(one[k]).distance(at(tried[k]))).sum();
            if best.as_ref().is_none_or(|(least, _)| far < *least) {
                best = Some((far, tried));
            }
        }
    }
    let Some((_, other)) = best else {
        return no("There is nothing to bridge");
    };
    let mut now = Vec::new();
    for k in 0..if *closed { n } else { n - 1 } {
        let next = (k + 1) % n;
        now.push(Item::Face(made.add_face(vec![
            one[k],
            one[next],
            other[next],
            other[k],
        ])));
    }
    let (made, _) = made.without_points(&BTreeSet::new());
    Ok((made, now))
}

/// The unit normal of the plane the points lie nearest, and their
/// middle.
pub fn plane_of(points: &[Point3]) -> (Point3, Point3) {
    let middle = points.iter().sum::<Point3>() / points.len().max(1) as f64;
    // The direction the points spread least in: of the matrix of how
    // they spread, turned over, the direction it stretches most.
    let mut c = [[0.0; 3]; 3];
    for p in points {
        let d = (*p - middle).to_array();
        for i in 0..3 {
            for j in 0..3 {
                c[i][j] += d[i] * d[j];
            }
        }
    }
    let trace = c[0][0] + c[1][1] + c[2][2];
    let apply = |v: Point3| {
        let v = v.to_array();
        let row = |i: usize| trace * v[i] - (c[i][0] * v[0] + c[i][1] * v[1] + c[i][2] * v[2]);
        Point3::new(row(0), row(1), row(2))
    };
    let mut n = Point3::new(0.3, 0.5, 0.8).normalize();
    for _ in 0..64 {
        n = apply(n).try_normalize().unwrap_or(Point3::Z);
    }
    (n, middle)
}

/// `points` in order around their middle, in the plane they lie
/// nearest.
fn around(poly: &Poly, points: &BTreeSet<usize>) -> Vec<usize> {
    let at: Vec<Point3> = points.iter().map(|&v| poly.points[v]).collect();
    let (n, middle) = plane_of(&at);
    let x = n.any_orthonormal_vector();
    let y = n.cross(x);
    let mut ordered: Vec<(f64, usize)> = points
        .iter()
        .map(|&v| {
            let d = poly.points[v] - middle;
            (d.dot(y).atan2(d.dot(x)), v)
        })
        .collect();
    ordered.sort_by(|a, b| a.0.total_cmp(&b.0));
    ordered.into_iter().map(|(_, v)| v).collect()
}

/// A face made of what is picked: around each closed run of edges,
/// across an open one, between two edges, or around the picked
/// points.
pub fn fill(poly: &Poly, picked: &[Item]) -> Done {
    let mut made = poly.clone();
    let mut now = Vec::new();
    let runs = Poly::runs(&poly.edges_of(picked));
    match &runs[..] {
        [] => {
            let points = poly.points_of(picked);
            if points.len() < 3 {
                return no("Pick three points or more, or the edges around a hole, to fill");
            }
            now.push(made.add_face(around(poly, &points)));
        }
        [(a, false), (b, false)] if a.len() == 2 && b.len() == 2 => {
            let at = |v: usize| poly.points[v];
            let straight = at(a[1]).distance(at(b[0])) + at(b[1]).distance(at(a[0]));
            let crossed = at(a[1]).distance(at(b[1])) + at(b[0]).distance(at(a[0]));
            let ring = if straight <= crossed {
                vec![a[0], a[1], b[0], b[1]]
            } else {
                vec![a[0], a[1], b[1], b[0]]
            };
            now.push(made.add_face(ring));
        }
        runs => {
            for (run, _) in runs {
                if run.len() >= 3 {
                    now.push(made.add_face(run.clone()));
                }
            }
        }
    }
    if now.is_empty() {
        return no("There is nothing picked a face could be made of");
    }
    Ok((made, now.into_iter().map(Item::Face).collect()))
}

/// The hole a closed run of picked edges goes around filled with a
/// grid of quads, `span` of them along the side the run starts with
/// (turned on by `turn` edges).
#[allow(clippy::needless_range_loop)]
pub fn grid_fill(poly: &Poly, picked: &[Item], span: usize, turn: usize) -> Done {
    let runs = Poly::runs(&poly.edges_of(picked));
    let [(run, true)] = &runs[..] else {
        return no("Pick the edges around one hole to fill with a grid");
    };
    let n = run.len();
    if n < 4 || n % 2 == 1 {
        return no("A grid fills a hole with an even number of edges around it");
    }
    let span = span.clamp(1, n / 2 - 1);
    let other = n / 2 - span;
    let at = |k: usize| run[(k + turn) % n];
    let p = |k: usize| poly.points[at(k)];
    let mut made = poly.clone();
    // The points of the grid: those around it are the run's.
    let mut grid = vec![vec![usize::MAX; span + 1]; other + 1];
    for i in 0..=span {
        grid[0][i] = at(i);
        grid[other][i] = at(2 * span + other - i);
    }
    for j in 0..=other {
        grid[j][span] = at(span + j);
        grid[j][0] = at((n - j) % n);
    }
    for j in 1..other {
        for i in 1..span {
            let (u, v) = (i as f64 / span as f64, j as f64 / other as f64);
            let (below, above) = (p(i), p(2 * span + other - i));
            let (left, right) = (p((n - j) % n), p(span + j));
            let corners = p(0) * (1.0 - u) * (1.0 - v)
                + p(span) * u * (1.0 - v)
                + p(span + other) * u * v
                + p(2 * span + other) * (1.0 - u) * v;
            let point = below * (1.0 - v) + above * v + left * (1.0 - u) + right * u - corners;
            made.points.push(point);
            grid[j][i] = made.points.len() - 1;
        }
    }
    let mut now = Vec::new();
    for j in 0..other {
        for i in 0..span {
            let ring = vec![
                grid[j][i],
                grid[j][i + 1],
                grid[j + 1][i + 1],
                grid[j + 1][i],
            ];
            now.push(Item::Face(made.add_face(ring)));
        }
    }
    Ok((made, now))
}

/// The picked faces there twice: the copies are what is picked after,
/// to be moved away.
pub fn duplicate(poly: &Poly, picked: &[Item]) -> Done {
    let faces = poly.faces_of(picked);
    if faces.is_empty() {
        return no("Pick the faces to duplicate");
    }
    let mut made = poly.clone();
    let mut double = BTreeMap::new();
    let mut now = Vec::new();
    for &f in &faces {
        let ring = poly.faces[f]
            .iter()
            .map(|&v| {
                *double.entry(v).or_insert_with(|| {
                    made.points.push(poly.points[v]);
                    made.points.len() - 1
                })
            })
            .collect();
        made.faces.push(ring);
        now.push(Item::Face(made.faces.len() - 1));
    }
    Ok((made, now))
}

/// The picked faces (all, when none is) turned to face the other way.
pub fn flip(poly: &Poly, picked: &[Item]) -> Done {
    let mut faces = poly.faces_of(picked);
    if faces.is_empty() {
        faces = (0..poly.faces.len()).collect();
    }
    let mut made = poly.clone();
    for &f in &faces {
        made.faces[f].reverse();
    }
    Ok((made, picked.to_vec()))
}

/// The mesh with its faces turned to face one way across the edges
/// they share, and each closed piece of it outwards.
pub fn outwards(poly: &Poly) -> Poly {
    let mut made = poly.clone();
    let sides = poly.edges();
    let mut settled = vec![false; poly.faces.len()];
    for start in 0..poly.faces.len() {
        if settled[start] {
            continue;
        }
        settled[start] = true;
        let mut piece = vec![start];
        let mut todo = vec![start];
        while let Some(f) = todo.pop() {
            let ring = made.faces[f].clone();
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                let beside = &sides[&edge(a, b)];
                if beside.len() != 2 {
                    continue;
                }
                let Some(g) = beside.iter().map(|s| s.face).find(|&g| g != f) else {
                    continue;
                };
                if settled[g] {
                    continue;
                }
                let other = &made.faces[g];
                let same =
                    (0..other.len()).any(|j| other[j] == a && other[(j + 1) % other.len()] == b);
                if same {
                    made.faces[g].reverse();
                }
                settled[g] = true;
                piece.push(g);
                todo.push(g);
            }
        }
        // (What a closed piece encloses is more than nothing; an open
        // one is left as its first face is.)
        let closed = piece.iter().all(|&f| {
            let ring = &made.faces[f];
            (0..ring.len()).all(|k| sides[&edge(ring[k], ring[(k + 1) % ring.len()])].len() == 2)
        });
        let mut inside = 0.0;
        for &f in &piece {
            let ring = &made.faces[f];
            for k in 1..ring.len().saturating_sub(1) {
                let [a, b, c] = [ring[0], ring[k], ring[k + 1]].map(|v| made.points[v]);
                inside += a.dot(b.cross(c));
            }
        }
        if closed && inside < 0.0 {
            for &f in &piece {
                made.faces[f].reverse();
            }
        }
    }
    made
}

/// The picked faces (all, when none is) cut in triangles: quads along
/// their shorter diagonal, larger ones from a corner — or (`poke`)
/// all from their middles.
pub fn triangulate(poly: &Poly, picked: &[Item], poke: bool) -> Done {
    let mut faces = poly.faces_of(picked);
    if faces.is_empty() {
        faces = (0..poly.faces.len()).collect();
    }
    let mut made = poly.clone();
    let mut now = Vec::new();
    for &f in &faces {
        let ring = poly.faces[f].clone();
        let n = ring.len();
        let mut triangles: Vec<Vec<usize>> = Vec::new();
        if poke {
            made.points.push(poly.center(f));
            let middle = made.points.len() - 1;
            for k in 0..n {
                triangles.push(vec![ring[k], ring[(k + 1) % n], middle]);
            }
        } else if n == 4 {
            let at = |k: usize| poly.points[ring[k]];
            if at(0).distance(at(2)) <= at(1).distance(at(3)) {
                triangles.extend([
                    vec![ring[0], ring[1], ring[2]],
                    vec![ring[0], ring[2], ring[3]],
                ]);
            } else {
                triangles.extend([
                    vec![ring[0], ring[1], ring[3]],
                    vec![ring[1], ring[2], ring[3]],
                ]);
            }
        } else if n > 4 {
            for k in 1..n - 1 {
                triangles.push(vec![ring[0], ring[k], ring[k + 1]]);
            }
        } else {
            now.push(Item::Face(f));
            continue;
        }
        for (k, triangle) in triangles.into_iter().enumerate() {
            if k == 0 {
                made.faces[f] = triangle;
                now.push(Item::Face(f));
            } else {
                made.faces.push(triangle);
                now.push(Item::Face(made.faces.len() - 1));
            }
        }
    }
    Ok((made, now))
}

/// The picked triangles (all, when none is picked) made quads, two by
/// two: those first that make the flattest and squarest.
pub fn quads(poly: &Poly, picked: &[Item]) -> Done {
    let mut faces = poly.faces_of(picked);
    if faces.is_empty() {
        faces = (0..poly.faces.len()).collect();
    }
    let sides = poly.edges();
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (e, beside) in &sides {
        let [one, other] = [beside[0], *beside.get(1).unwrap_or(&beside[0])];
        let both = [one.face, other.face];
        if beside.len() != 2
            || both
                .iter()
                .any(|f| !faces.contains(f) || poly.faces[*f].len() != 3)
            || poly.creases.contains(e)
        {
            continue;
        }
        // How far from flat, and how far from square.
        let bend = 1.0 - poly.normal(one.face).dot(poly.normal(other.face));
        let apex = |f: usize| {
            let v = poly.faces[f].iter().find(|&&v| v != e.0 && v != e.1);
            v.map_or(Point3::ZERO, |&v| poly.points[v])
        };
        let across = apex(one.face).distance(apex(other.face));
        let along = poly.points[e.0].distance(poly.points[e.1]);
        let skew = (across - along).abs() / (across + along).max(1e-12);
        pairs.push((bend * 4.0 + skew, one.face, other.face));
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut used = BTreeSet::new();
    let mut regions = Vec::new();
    for (cost, a, b) in pairs {
        if cost > 1.5 || used.contains(&a) || used.contains(&b) {
            continue;
        }
        used.extend([a, b]);
        regions.push(BTreeSet::from([a, b]));
    }
    if regions.is_empty() {
        return no("No two triangles picked make a quad");
    }
    let (made, new) = merged(poly, &regions);
    Ok((made, new.into_iter().map(Item::Face).collect()))
}

/// The picked points (all, when none is) moved `by` of the way to the
/// middle of the points beside them: what is uneven is evened out.
pub fn relax(poly: &Poly, picked: &[Item], by: f64) -> Done {
    let mut points = poly.points_of(picked);
    if points.is_empty() {
        points = (0..poly.points.len()).collect();
    }
    let beside = poly.beside();
    let mut made = poly.clone();
    for &v in &points {
        if beside[v].is_empty() {
            continue;
        }
        let middle =
            beside[v].iter().map(|&n| poly.points[n]).sum::<Point3>() / beside[v].len() as f64;
        made.points[v] = poly.points[v].lerp(middle, by.clamp(0.0, 1.0));
    }
    Ok((made, picked.to_vec()))
}

/// The picked points moved into the plane they lie nearest.
pub fn flatten(poly: &Poly, picked: &[Item]) -> Done {
    let points = poly.points_of(picked);
    if points.len() < 4 {
        return no("Pick four points or more to flatten");
    }
    let at: Vec<Point3> = points.iter().map(|&v| poly.points[v]).collect();
    let (n, middle) = plane_of(&at);
    let mut made = poly.clone();
    for &v in &points {
        let p = poly.points[v];
        made.points[v] = p - n * (p - middle).dot(n);
    }
    Ok((made, picked.to_vec()))
}

/// The picked edges creased — kept sharp when the mesh is subdivided
/// — or, if all of them are, smooth again.
pub fn crease(poly: &Poly, picked: &[Item]) -> Done {
    let mut edges = poly.edges_of(picked);
    for f in poly.faces_of(picked) {
        let ring = &poly.faces[f];
        for k in 0..ring.len() {
            edges.insert(edge(ring[k], ring[(k + 1) % ring.len()]));
        }
    }
    if edges.is_empty() {
        return no("Pick the edges to crease");
    }
    let mut made = poly.clone();
    if edges.iter().all(|e| poly.creases.contains(e)) {
        made.creases.retain(|e| !edges.contains(e));
    } else {
        made.creases.extend(edges);
    }
    Ok((made, picked.to_vec()))
}

/// The picked faces (all, when none is) given a thickness: a second
/// skin `thickness` behind them, and walls around where they end.
pub fn solidify(poly: &Poly, picked: &[Item], thickness: f64) -> Done {
    let mut faces = poly.faces_of(picked);
    if faces.is_empty() {
        faces = (0..poly.faces.len()).collect();
    }
    if faces.is_empty() {
        return no("There are no faces to thicken");
    }
    let normals = poly.point_normals();
    let mut made = poly.clone();
    let mut double: BTreeMap<usize, usize> = BTreeMap::new();
    let mut now: Vec<Item> = faces.iter().map(|&f| Item::Face(f)).collect();
    for &f in &faces {
        let ring: Vec<usize> = poly.faces[f]
            .iter()
            .rev()
            .map(|&v| {
                *double.entry(v).or_insert_with(|| {
                    made.points.push(poly.points[v] - normals[v] * thickness);
                    made.points.len() - 1
                })
            })
            .collect();
        made.faces.push(ring);
        now.push(Item::Face(made.faces.len() - 1));
    }
    for (a, b) in rim(poly, &faces) {
        made.faces.push(vec![b, a, double[&a], double[&b]]);
    }
    for &(a, b) in &poly.creases {
        if let (Some(&a), Some(&b)) = (double.get(&a), double.get(&b)) {
            made.creases.insert(edge(a, b));
        }
    }
    Ok((made, now))
}

/// The mesh and its mirror images in the planes through the origin
/// that `axes` says (across x, y, z), made one where they meet: the
/// points in a plane (within `near` of it) are shared.
pub fn mirrored(poly: &Poly, axes: [bool; 3], near: f64) -> Poly {
    let mut made = poly.clone();
    for axis in 0..3 {
        if !axes[axis] {
            continue;
        }
        let n = made.points.len();
        let mut image = Vec::with_capacity(n);
        for v in 0..n {
            let mut p = made.points[v];
            if p[axis].abs() <= near {
                p[axis] = 0.0;
                made.points[v] = p;
                image.push(v);
            } else {
                p[axis] = -p[axis];
                made.points.push(p);
                image.push(made.points.len() - 1);
            }
        }
        // (A face that lies in the plane is its own image: it goes.)
        let flat = |ring: &Vec<usize>| ring.iter().all(|&v| image[v] == v);
        made.faces.retain(|ring| !flat(ring));
        let images: Vec<Vec<usize>> = made
            .faces
            .iter()
            .map(|ring| ring.iter().rev().map(|&v| image[v]).collect())
            .collect();
        made.faces.extend(images);
        let creases: Vec<Edge> = made
            .creases
            .iter()
            .map(|&(a, b)| edge(image[a], image[b]))
            .collect();
        made.creases.extend(creases);
    }
    made.tidy();
    made
}

/// Everything joined to what is picked, by edges.
pub fn linked(poly: &Poly, picked: &[Item]) -> BTreeSet<usize> {
    let beside = poly.beside();
    let mut found = poly.points_of(picked);
    let mut todo: Vec<usize> = found.iter().copied().collect();
    while let Some(v) = todo.pop() {
        for &n in &beside[v] {
            if found.insert(n) {
                todo.push(n);
            }
        }
    }
    found
}

/// What of the kind of `like` lies on `points`: the points
/// themselves, the edges between two of them, or the faces of them
/// only.
pub fn items_on(poly: &Poly, points: &BTreeSet<usize>, like: Item) -> Vec<Item> {
    match like {
        Item::Vertex(_) => points.iter().map(|&v| Item::Vertex(v)).collect(),
        Item::Edge(..) => poly
            .edges()
            .keys()
            .filter(|(a, b)| points.contains(a) && points.contains(b))
            .map(|&(a, b)| Item::Edge(a, b))
            .collect(),
        Item::Face(_) => (0..poly.faces.len())
            .filter(|&f| poly.faces[f].iter().all(|v| points.contains(v)))
            .map(Item::Face)
            .collect(),
    }
}

/// What is picked and what is beside it (`more`), or what is picked
/// but for what is at its rim.
pub fn grown(poly: &Poly, picked: &[Item], more: bool) -> Vec<Item> {
    let Some(&like) = picked.first() else {
        return Vec::new();
    };
    let points = poly.points_of(picked);
    let beside = poly.beside();
    let next: BTreeSet<usize> = if more {
        points
            .iter()
            .flat_map(|&v| beside[v].iter().copied().chain([v]))
            .collect()
    } else {
        points
            .iter()
            .copied()
            .filter(|&v| beside[v].iter().all(|n| points.contains(n)))
            .collect()
    };
    if more && matches!(like, Item::Face(_)) {
        // (The faces at the picked points, not those of them only.)
        return (0..poly.faces.len())
            .filter(|&f| poly.faces[f].iter().any(|v| points.contains(v)))
            .map(Item::Face)
            .collect();
    }
    items_on(poly, &next, like)
}

/// The picked faces cut loose from the faces beside them: the points
/// around them are doubled, so that they move by themselves.
pub fn detach(poly: &Poly, picked: &[Item]) -> Done {
    let faces = poly.faces_of(picked);
    if faces.is_empty() {
        return no("Pick the faces to split off");
    }
    let mut shared = BTreeSet::new();
    for (f, ring) in poly.faces.iter().enumerate() {
        if !faces.contains(&f) {
            shared.extend(ring.iter().copied());
        }
    }
    let mut made = poly.clone();
    let mut double: BTreeMap<usize, usize> = BTreeMap::new();
    for &f in &faces {
        for k in 0..poly.faces[f].len() {
            let v = poly.faces[f][k];
            if shared.contains(&v) {
                made.faces[f][k] = *double.entry(v).or_insert_with(|| {
                    made.points.push(poly.points[v]);
                    made.points.len() - 1
                });
            }
        }
    }
    if double.is_empty() {
        return no("The faces picked are beside no others");
    }
    Ok((made, faces.into_iter().map(Item::Face).collect()))
}

/// The picked points (all, when none is) moved by `distance` along
/// the way the mesh faces at them: blown up, or let down.
pub fn inflate(poly: &Poly, picked: &[Item], distance: f64) -> Done {
    let mut points = poly.points_of(picked);
    if points.is_empty() {
        points = (0..poly.points.len()).collect();
    }
    let normals = poly.point_normals();
    let mut made = poly.clone();
    for &v in &points {
        made.points[v] += normals[v] * distance;
    }
    Ok((made, picked.to_vec()))
}

/// The half of the mesh on the upper side of the plane through the
/// origin across `axis`: the faces through the plane are cut along
/// it, and what is below goes. What a mirror then makes whole again.
pub fn upper_half(poly: &Poly, axis: usize, near: f64) -> Poly {
    let side = |v: usize, mesh: &Poly| {
        let x = mesh.points[v][axis];
        if x.abs() <= near {
            0
        } else {
            x.signum() as i32
        }
    };
    let mut cuts = BTreeMap::new();
    for &(a, b) in poly.edges().keys() {
        if side(a, poly) * side(b, poly) < 0 {
            let (xa, xb) = (poly.points[a][axis], poly.points[b][axis]);
            cuts.insert((a, b), vec![xa / (xa - xb)]);
        }
    }
    let (mut made, _) = split_edges(poly, &cuts);
    for p in &mut made.points {
        if p[axis].abs() <= near {
            p[axis] = 0.0;
        }
    }
    // Each face through the plane is cut between its two points in it.
    for f in 0..made.faces.len() {
        let ring = made.faces[f].clone();
        let sides: Vec<i32> = ring.iter().map(|&v| side(v, &made)).collect();
        if !(sides.contains(&1) && sides.contains(&-1)) {
            continue;
        }
        let on: Vec<usize> = (0..ring.len()).filter(|&k| sides[k] == 0).collect();
        if let [i, j] = on[..] {
            let n = ring.len();
            if (j - i) >= 2 && (i + n - j) >= 2 {
                made.faces[f] = ring[i..=j].to_vec();
                let mut other = ring[j..].to_vec();
                other.extend(&ring[..=i]);
                made.faces.push(other);
            }
        }
    }
    let below: BTreeSet<usize> = (0..made.faces.len())
        .filter(|&f| made.center(f)[axis] < -near)
        .collect();
    made.without_faces(&below).0
}
