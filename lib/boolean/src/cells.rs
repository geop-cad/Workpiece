//! Cutting each face along what was drawn on it, sorting the pieces by
//! where they are — inside the other solid, outside it, or on its
//! boundary — and closing up those an operation keeps.
//!
//! On a face, its boundary and the parts drawn on it meet only at
//! vertices, so in (u, v) they are a map: walking it with the face on
//! the left, turning as sharply left as there is a way at every vertex,
//! goes around one piece of the face.

use std::collections::BTreeMap;
use std::sync::Arc;

use shapecraft_core::math::Point2;

use crate::bezier::curve_pieces;
use crate::fuse::{Fuse, Half, Outcome};
use crate::model::{Edge, Face, Solid, Use, Vertex, Walk};
use crate::query::{Trim, inside, normal, on_face};
use crate::{Op, Refusal};

/// A piece of a face.
struct Cell {
    /// The face, among all faces of both solids.
    fid: usize,
    /// Its loops: the outer one, then its holes.
    loops: Vec<Vec<Half>>,
    /// Points well inside it, in (u, v): the best first.
    samples: Vec<Point2>,
}

/// Where a piece of a face is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Inside the other solid.
    In,
    /// Outside it.
    Out,
    /// On its boundary, the two outsides the same way.
    OnSame,
    /// On its boundary, the two outsides opposite.
    OnOpposite,
}

fn degenerate(what: impl Into<String>) -> Refusal {
    Refusal::Degenerate(what.into())
}

/// About how long `half` is in (u, v).
fn length(half: &Half) -> f64 {
    (0..8)
        .map(|k| {
            half.uv(f64::from(k) / 8.0)
                .distance(half.uv(f64::from(k + 1) / 8.0))
        })
        .sum()
}

/// From the tail of `half` (`from_head`: from its head, backwards) to
/// where it is `far` away from there in (u, v) — or to its other end,
/// if that is nearer.
fn away(half: &Half, from_head: bool, far: f64) -> Point2 {
    let at = |g: f64| {
        if from_head {
            half.uv(1.0 - g) - half.uv(1.0)
        } else {
            half.uv(g) - half.uv(0.0)
        }
    };
    let whole = length(half);
    if whole <= 0.0 || far <= 0.0 {
        return at(1.0);
    }
    // (As far along as that is on a straight way; then nearer or
    // farther by what is still missing.)
    let mut g = (far / whole).min(1.0);
    for _ in 0..8 {
        let now = at(g).length();
        if now <= 0.0 || (now - far).abs() <= far * 1e-9 {
            break;
        }
        let next = (g * far / now).min(1.0);
        if next == g {
            break;
        }
        g = next;
    }
    at(g)
}

/// The area a walk goes around in (u, v), counter-clockwise positive:
/// by Green's theorem along each curve, span by span of its knots (a
/// sliver between two curves that all but lie on each other has an
/// area smaller than any chords along them would tell), and straight
/// across from where one ends to where the next starts.
fn area(walk: &[usize], halves: &[Half]) -> f64 {
    let mut sum = 0.0;
    for (k, &h) in walk.iter().enumerate() {
        let (half, next) = (&halves[h], &halves[walk[(k + 1) % walk.len()]]);
        sum += crate::geometry::swept(&half.pcurve, half.at(0.0), half.at(1.0))
            + half.uv(1.0).perp_dot(next.uv(0.0)) / 2.0;
    }
    sum
}

/// The Bézier pieces of each of a face's halves, taken apart once:
/// the same halves bound piece after piece that is asked about.
type Taken = std::cell::RefCell<Vec<Option<Vec<crate::bezier::Piece>>>>;

fn trim_of(walks: &[&[usize]], halves: &[Half], taken: &Taken) -> Trim {
    let mut taken = taken.borrow_mut();
    taken.resize(halves.len(), None);
    let mut pieces = Vec::new();
    for &h in walks.iter().flat_map(|w| w.iter()) {
        let half = &halves[h];
        let of =
            taken[h].get_or_insert_with(|| curve_pieces(&*half.pcurve, half.range.0, half.range.1));
        pieces.extend(of.iter().cloned());
    }
    Trim::new(pieces)
}

/// The pieces face `fid` falls into.
fn cells_of(fuse: &Fuse<'_>, fid: usize) -> Result<Vec<Cell>, Refusal> {
    let work = &fuse.works[fid];
    let size = {
        let [u, v] = fuse.data[work.solid].faces[work.face].uv;
        (u.1 - u.0).hypot(v.1 - v.0)
    };
    // A part drawn up to a pole ends somewhere along the collapsed
    // side: the walk around the pole is cut there, so that it has a
    // corner to end at.
    let mut halves: Vec<Half> = Vec::with_capacity(work.halves.len());
    let mut walks: Vec<Vec<usize>> = Vec::with_capacity(work.loops.len());
    for walk in &work.loops {
        let mut new = Vec::with_capacity(walk.len());
        for &h in walk {
            let half = &work.halves[h];
            let mut cuts: Vec<f64> = Vec::new();
            if half.part.is_none() {
                let (a, b) = half.range;
                for (part, pcurve) in &fuse.extra[fid] {
                    let p = &fuse.parts[*part];
                    for (end, t) in [(p.ends[0], p.range.0), (p.ends[1], p.range.1)] {
                        if end != half.tail {
                            continue;
                        }
                        let q = pcurve.point_at(t);
                        // The nearest point of the side: by halving
                        // around the best of a few.
                        let off = |s: f64| half.pcurve.point_at(s).distance(q);
                        let mut best = (0..=64)
                            .map(|k| a + (b - a) * f64::from(k) / 64.0)
                            .min_by(|x, y| off(*x).total_cmp(&off(*y)))
                            .unwrap_or(a);
                        let mut step = (b - a) / 64.0;
                        for _ in 0..50 {
                            step /= 2.0;
                            for s in [best - step, best + step] {
                                let s = s.clamp(a, b);
                                if off(s) < off(best) {
                                    best = s;
                                }
                            }
                        }
                        let inside = best > a + (b - a) * 1e-9 && best < b - (b - a) * 1e-9;
                        if inside && off(best) <= size * 1e-6 {
                            cuts.push(best);
                        }
                    }
                }
                cuts.sort_by(f64::total_cmp);
                cuts.dedup_by(|x, y| (*x - *y).abs() <= (b - a) * 1e-9);
            }
            let mut from = half.range.0;
            for to in cuts.into_iter().chain([half.range.1]) {
                new.push(halves.len());
                halves.push(Half {
                    range: (from, to),
                    ..half.clone()
                });
                from = to;
            }
        }
        walks.push(new);
    }
    let mut twin: Vec<Option<usize>> = vec![None; halves.len()];
    // Where each walk starts and ends: a corner of the boundary, or a
    // vertex inside the face.
    let mut tail = vec![usize::MAX; halves.len()];
    let mut head = vec![usize::MAX; halves.len()];
    let mut corners: Vec<(usize, usize, Point2)> = Vec::new();
    let mut nodes = 0;
    for walk in &walks {
        for (k, &h) in walk.iter().enumerate() {
            let next = walk[(k + 1) % walk.len()];
            head[h] = nodes;
            tail[next] = nodes;
            corners.push((halves[h].head, nodes, halves[next].uv(0.0)));
            nodes += 1;
        }
    }
    let mut inner: BTreeMap<usize, usize> = BTreeMap::new();
    let mut node_of = |v: usize, uv: Point2, nodes: &mut usize| {
        let nearest = corners
            .iter()
            .filter(|c| c.0 == v)
            .min_by(|a, b| a.2.distance(uv).total_cmp(&b.2.distance(uv)));
        match nearest {
            Some(c) => c.1,
            None => *inner.entry(v).or_insert_with(|| {
                *nodes += 1;
                *nodes - 1
            }),
        }
    };
    for (part, pcurve) in &fuse.extra[fid] {
        let p = &fuse.parts[*part];
        let forward = Half {
            part: Some(*part),
            forward: true,
            tail: p.ends[0],
            head: p.ends[1],
            pcurve: pcurve.clone(),
            range: p.range,
        };
        let backward = Half {
            forward: false,
            tail: p.ends[1],
            head: p.ends[0],
            ..forward.clone()
        };
        let (a, b) = (
            node_of(p.ends[0], forward.uv(0.0), &mut nodes),
            node_of(p.ends[1], forward.uv(1.0), &mut nodes),
        );
        let k = halves.len();
        halves.extend([forward, backward]);
        twin.extend([Some(k + 1), Some(k)]);
        tail.extend([a, b]);
        head.extend([b, a]);
    }
    let mut dead = vec![false; halves.len()];
    let angle = |d: Point2| d.y.atan2(d.x);
    let loops = loop {
        let mut leaving: Vec<Vec<usize>> = vec![Vec::new(); nodes];
        for h in 0..halves.len() {
            if !dead[h] {
                leaving[tail[h]].push(h);
            }
        }
        // The walk after `h`: the first way out of its head clockwise
        // from the way back, seen from nearer and then from farther
        // where two ways leave together.
        let next = |h: usize| -> Option<usize> {
            let out = &leaving[head[h]];
            let mut choice = None;
            // (All seen from as far off: two ways that leave together
            // are told apart by where they are a little on, and one
            // looked at farther along its bend than the other would
            // seem to have turned away from it.)
            let shortest = out
                .iter()
                .chain([&h])
                .map(|&x| length(&halves[x]))
                .fold(f64::INFINITY, f64::min);
            for f in [1e-3, 1e-2, 1e-1] {
                let far = shortest * f;
                let back = angle(away(&halves[h], true, far));
                let mut turns: Vec<(f64, usize)> = out
                    .iter()
                    .filter(|&&x| Some(x) != twin[h])
                    .map(|&x| {
                        let a = (back - angle(away(&halves[x], false, far)))
                            .rem_euclid(std::f64::consts::TAU);
                        (if a < 1e-9 { std::f64::consts::TAU } else { a }, x)
                    })
                    .collect();
                turns.sort_by(|a, b| a.0.total_cmp(&b.0));
                match turns[..] {
                    [] => return twin[h].filter(|t| !dead[*t]),
                    [(_, x)] => return Some(x),
                    [(a, x), (b, _), ..] => {
                        choice = Some(x);
                        // (One that leaves along the way back — a
                        // curve that touches the edge it starts on —
                        // is to one side of it only seen from farther.)
                        let back =
                            |t: &(f64, usize)| t.0 < 1e-4 || t.0 > std::f64::consts::TAU - 1e-4;
                        if b - a > 1e-4 && !turns.iter().any(back) {
                            return choice;
                        }
                    }
                }
            }
            choice
        };
        let mut seen = vec![false; halves.len()];
        let mut loops: Vec<Vec<usize>> = Vec::new();
        for start in 0..halves.len() {
            if dead[start] || seen[start] {
                continue;
            }
            let mut walk = Vec::new();
            let mut h = start;
            loop {
                if seen[h] {
                    if h != start {
                        return Err(degenerate("a face's pieces do not close up"));
                    }
                    break;
                }
                seen[h] = true;
                walk.push(h);
                h = next(h).ok_or_else(|| degenerate("a face's boundary ends nowhere"))?;
            }
            loops.push(walk);
        }
        // A part walked both ways by one loop parts nothing: it hangs
        // into a piece or bridges it. Without it the piece is the same.
        let mut hanging = false;
        for walk in &loops {
            for &h in walk {
                if twin[h].is_some_and(|t| walk.contains(&t)) {
                    dead[h] = true;
                    hanging = true;
                }
            }
        }
        if !hanging {
            break loops;
        }
    };
    // Loops that turn left bound a piece; the others are holes in the
    // smallest piece around them.
    let mut outer: Vec<(f64, Vec<usize>, Vec<Vec<usize>>)> = Vec::new();
    let mut holes: Vec<Vec<usize>> = Vec::new();
    for walk in loops {
        let size = area(&walk, &halves);
        if size > 0.0 {
            outer.push((size, walk, Vec::new()));
        } else {
            holes.push(walk);
        }
    }
    outer.sort_by(|a, b| a.0.total_cmp(&b.0));
    let taken = Taken::default();
    for hole in holes {
        let at = halves[hole[0]].uv(0.5);
        let around = outer
            .iter_mut()
            .find(|o| trim_of(&[&o.1], &halves, &taken).holds(at) == Some(true))
            .ok_or_else(|| degenerate("a hole of a face is in none of its pieces"))?;
        around.2.push(hole);
    }
    Ok(outer
        .into_iter()
        .map(|(_, walk, holes)| {
            let mut walks: Vec<&[usize]> = vec![&walk];
            walks.extend(holes.iter().map(Vec::as_slice));
            let samples = trim_of(&walks, &halves, &taken)
                .inner_points()
                .into_iter()
                .take(6)
                .map(|x| x.0)
                .collect();
            let pick = |w: &[usize]| w.iter().map(|&h| halves[h].clone()).collect();
            Cell {
                fid,
                loops: walks.iter().map(|w| pick(w)).collect(),
                samples,
            }
        })
        .collect())
}

/// Groups of cells of one solid that hang together through edges the
/// other solid's boundary does not run along: all of a group is on one
/// side of it.
fn groups(fuse: &Fuse<'_>, cells: &[Cell]) -> Vec<usize> {
    fn root(of: &mut [usize], mut k: usize) -> usize {
        while of[k] != k {
            of[k] = of[of[k]];
            k = of[k];
        }
        k
    }
    let mut of: Vec<usize> = (0..cells.len()).collect();
    let mut first: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (k, cell) in cells.iter().enumerate() {
        let solid = fuse.works[cell.fid].solid;
        for half in cell.loops.iter().flatten() {
            let Some(part) = half.part.filter(|&p| !fuse.parts[p].shared) else {
                continue;
            };
            match first.get(&(solid, part)) {
                Some(&other) => {
                    let (a, b) = (root(&mut of, k), root(&mut of, other));
                    of[a] = b;
                }
                None => {
                    first.insert((solid, part), k);
                }
            }
        }
    }
    (0..cells.len()).map(|k| root(&mut of, k)).collect()
}

/// Where the cell is against the other solid, if one of its samples
/// tells.
fn classify(fuse: &Fuse<'_>, cell: &Cell) -> Option<Class> {
    let work = &fuse.works[cell.fid];
    let (s, other) = (work.solid, 1 - work.solid);
    let face = &fuse.solids[s].faces[work.face];
    let (solid, data) = (fuse.solids[other], &fuse.data[other]);
    // A piece of a face lies on the other solid's boundary only if
    // every edge around it does.
    let may_lie_on = cell
        .loops
        .iter()
        .flatten()
        .all(|h| h.part.is_none_or(|p| fuse.parts[p].shared));
    let all: Vec<usize> = (0..solid.faces.len()).collect();
    // How far the samples that only touch a face of the other solid are
    // in front of it: the farthest, behind it or before.
    let mut beside: Option<f64> = None;
    // What the samples that lie on a face of the other solid say the
    // piece is: so it is, unless one of them is off that solid's faces.
    let mut lies: Option<Class> = None;
    'samples: for &uv in &cell.samples {
        let p = face.surface.point_at(uv.x, uv.y);
        let n = normal(&face.surface, uv) * if face.outward { 1.0 } else { -1.0 };
        for (f, there) in solid.faces.iter().enumerate() {
            let tol = face.tolerance + there.tolerance + fuse.target * 4.0;
            let Some(at) = on_face(there, &data.faces[f], p, tol) else {
                continue;
            };
            let m = normal(&there.surface, at) * if there.outward { 1.0 } else { -1.0 };
            // On a face only where the two face the same way or
            // opposite ways; a sliver thinner than the tolerances is
            // near a face across it without lying on it.
            let facing = n.dot(m);
            if may_lie_on && facing.abs() > 0.99 {
                // (One place where the two lie on each other may be
                // where a round face only touches a flat one, along a
                // line that several samples can be on: a piece lies on
                // the other solid if every sample of it does.)
                let found = if facing > 0.0 {
                    Class::OnSame
                } else {
                    Class::OnOpposite
                };
                if lies.is_some_and(|l| l != found) {
                    return Some(found);
                }
                lies = Some(found);
                continue 'samples;
            }
            if facing.abs() > 0.99 || !may_lie_on {
                // Just a place where the two touch: it tells nothing
                // of the piece. Another sample will.
                let before = (p - there.surface.point_at(at.x, at.y)).dot(m);
                if beside.is_none_or(|b| before.abs() > b.abs()) {
                    beside = Some(before);
                }
                continue 'samples;
            }
        }
        if let Some(inside) = inside(solid, data, &all, p) {
            return Some(if inside { Class::In } else { Class::Out });
        }
    }
    // A piece all within the tolerances of a face of the other solid,
    // without lying on it (it crosses it along its own boundary): a
    // shallow dip through that face, in or out by which side of the
    // face it is on.
    // (Every sample that told anything lay on the other solid: so does
    // the piece.)
    if lies.is_some() {
        return lies;
    }
    beside
        .filter(|b| !may_lie_on && b.abs() > fuse.target * 0.1)
        .map(|b| if b < 0.0 { Class::In } else { Class::Out })
}

/// The solid `op` makes of the two, from the pieces of their faces.
pub(crate) fn assemble(fuse: &mut Fuse<'_>, op: Op) -> Result<Outcome, Refusal> {
    let mut cells = Vec::new();
    for fid in 0..fuse.works.len() {
        cells.extend(cells_of(fuse, fid)?);
    }
    let group = groups(fuse, &cells);
    let mut class: BTreeMap<usize, Class> = BTreeMap::new();
    for (k, cell) in cells.iter().enumerate() {
        if class.contains_key(&group[k]) {
            continue;
        }
        if let Some(c) = classify(fuse, cell) {
            class.insert(group[k], c);
        }
    }
    let mut kept: Vec<(usize, bool)> = Vec::new();
    for (k, cell) in cells.iter().enumerate() {
        let c = *class
            .get(&group[k])
            .ok_or_else(|| degenerate("a piece of a face could not be placed"))?;
        if c != Class::Out {
            fuse.touched = true;
        }
        let first = fuse.works[cell.fid].solid == 0;
        let keep = match (op, first, c) {
            (Op::Union, _, Class::Out) | (Op::Union, true, Class::OnSame) => Some(false),
            (Op::Intersect, _, Class::In) | (Op::Intersect, true, Class::OnSame) => Some(false),
            (Op::Cut, true, Class::Out | Class::OnOpposite) => Some(false),
            // What of the second is inside the first bounds the cut,
            // turned inside out.
            (Op::Cut, false, Class::In) => Some(true),
            _ => None,
        };
        if let Some(flipped) = keep {
            kept.push((k, flipped));
        }
    }
    // Every edge of a closed solid is walked twice, once each way. Where
    // four faces come to one edge (two solids touching along it, a hole
    // whose wall touches a side), each two that bound the same stretch
    // of material around it get a copy of the edge of their own.
    let mut walked: BTreeMap<usize, Vec<(bool, [usize; 3])>> = BTreeMap::new();
    for (n, &(k, flipped)) in kept.iter().enumerate() {
        let work = &fuse.works[cells[k].fid];
        let outward = fuse.solids[work.solid].faces[work.face].outward != flipped;
        for (l, walk) in cells[k].loops.iter().enumerate() {
            for (h, half) in walk.iter().enumerate() {
                if let Some(part) = half.part {
                    walked
                        .entry(part)
                        .or_default()
                        .push((half.forward == outward, [n, l, h]));
                }
            }
        }
    }
    let mut copy: BTreeMap<[usize; 3], usize> = BTreeMap::new();
    for (part, uses) in &walked {
        let open = || {
            let at = fuse.parts[*part].curve.point_at(fuse.parts[*part].middle());
            Refusal::Open(format!(
                "an edge near ({:.6}, {:.6}, {:.6}) would bound {} face(s)",
                at.x,
                at.y,
                at.z,
                uses.len()
            ))
        };
        match uses[..] {
            [(a, _), (b, _)] if a != b => {}
            [_, _, _, _] => {
                let pairs = around(fuse, &cells, &kept, *part, uses).ok_or_else(open)?;
                for (n, pair) in pairs.into_iter().enumerate() {
                    for at in pair {
                        copy.insert(at, n);
                    }
                }
            }
            _ => return Err(open()),
        }
    }
    let mut solid = Solid::default();
    let mut vertex: BTreeMap<usize, usize> = BTreeMap::new();
    let mut edge: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    let mut take = |solid: &mut Solid, v: usize| {
        *vertex.entry(v).or_insert_with(|| {
            let vert = &fuse.verts[v];
            solid.vertices.push(Vertex {
                point: vert.point,
                tolerance: vert.tol,
                name: vert.name.clone(),
            });
            solid.vertices.len() - 1
        })
    };
    let mut named: Vec<bool> = vec![false; fuse.works.len()];
    for (n, &(k, flipped)) in kept.iter().enumerate() {
        let cell = &cells[k];
        let work = &fuse.works[cell.fid];
        let face = &fuse.solids[work.solid].faces[work.face];
        let mut loops = Vec::with_capacity(cell.loops.len());
        // (What the face was known to run on smoothly along, its piece
        // still is.)
        let mut smooth: Vec<usize> = Vec::new();
        for (l, walk) in cell.loops.iter().enumerate() {
            let mut steps = Vec::with_capacity(walk.len());
            for (h, half) in walk.iter().enumerate() {
                let walk = match half.part {
                    Some(p) => {
                        let part = &fuse.parts[p];
                        let ends = [
                            take(&mut solid, part.ends[0]),
                            take(&mut solid, part.ends[1]),
                        ];
                        let which = copy.get(&[n, l, h]).copied().unwrap_or(0);
                        let e = *edge.entry((p, which)).or_insert_with(|| {
                            solid.edges.push(Edge {
                                curve: part.curve.clone(),
                                range: part.range,
                                ends,
                                tolerance: part.tol,
                                name: part.name.clone(),
                                also: part.owner.map_or_else(Vec::new, |(s, e)| {
                                    fuse.solids[s].edges[e].also.clone()
                                }),
                            });
                            solid.edges.len() - 1
                        });
                        let known = part
                            .owner
                            .is_some_and(|(s, of)| s == work.solid && face.smooth.contains(&of));
                        if known && !smooth.contains(&e) {
                            smooth.push(e);
                        }
                        Walk::Edge {
                            edge: e,
                            reversed: !half.forward,
                        }
                    }
                    None => Walk::Pole {
                        vertex: take(&mut solid, half.tail),
                        range: half.range,
                    },
                };
                steps.push(Use {
                    walk,
                    pcurve: Arc::clone(&half.pcurve),
                });
            }
            loops.push(steps);
        }
        // The first piece of a face keeps its name.
        let name = if std::mem::replace(&mut named[cell.fid], true) {
            face.name.as_ref().map(|n| format!("{n}#"))
        } else {
            face.name.clone()
        };
        solid.faces.push(Face {
            surface: face.surface.clone(),
            outward: face.outward != flipped,
            loops,
            tolerance: face.tolerance,
            name,
            also: face.also.clone(),
            smooth,
        });
    }
    crate::tidy::tidy(&mut solid);
    number(&mut solid);
    Ok(Outcome {
        solid,
        touched: fuse.touched,
    })
}

/// The four uses of `part` by kept pieces of faces, in the two pairs
/// that each bound one stretch of material around it — if they do: each
/// pair one use each way.
fn around(
    fuse: &Fuse<'_>,
    cells: &[Cell],
    kept: &[(usize, bool)],
    part: usize,
    uses: &[(bool, [usize; 3])],
) -> Option<[[[usize; 3]; 2]; 2]> {
    let edge = &fuse.parts[part];
    let t = edge.middle();
    let (_, along) = edge.curve.derivative_at(t);
    let along = along.try_normalize()?;
    let e1 = along.any_orthonormal_vector();
    let e2 = along.cross(e1);
    // Of each use: the angle around the edge its face leaves at (seen a
    // short way into the face: two faces that touch along the edge leave
    // it the same way), and whether its material is on from there, the
    // way the angle rises.
    let mut faces: Vec<(f64, bool, bool, [usize; 3])> = Vec::with_capacity(4);
    for &(way, [n, l, h]) in uses {
        let (k, flipped) = kept[n];
        let half = &cells[k].loops[l][h];
        let work = &fuse.works[cells[k].fid];
        let face = &fuse.solids[work.solid].faces[work.face];
        let (uv, d) = half.pcurve.derivative_at(t);
        let walk = if half.forward { d } else { -d };
        // Into the face: to the left of the way it is walked.
        let into = Point2::new(-walk.y, walk.x);
        let (p, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
        let step = du * into.x + dv * into.y;
        let reach = fuse.scale * 1e-3 / step.length().max(1e-300);
        let q = uv + into * reach;
        let leaving = face.surface.point_at(q.x, q.y) - p;
        let angle = leaving.dot(e2).atan2(leaving.dot(e1));
        let out = du.cross(dv) * if face.outward != flipped { 1.0 } else { -1.0 };
        let rising = along.cross(leaving);
        faces.push((angle, -out.dot(rising) > 0.0, way, [n, l, h]));
    }
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut pairs = Vec::with_capacity(2);
    for i in 0..4 {
        let (a, b) = (faces[i], faces[(i + 1) % 4]);
        if a.1 {
            // Material from this one on to the next, which closes it.
            if b.1 || a.2 == b.2 {
                return None;
            }
            pairs.push([a.3, b.3]);
        }
    }
    match pairs[..] {
        [a, b] => Some([a, b]),
        _ => None,
    }
}

/// Gives the pieces of a face after its first numbers: `name#2`,
/// `name#3`, …
fn number(solid: &mut Solid) {
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for face in &mut solid.faces {
        let Some(base) = face.name.as_ref().and_then(|n| n.strip_suffix('#')) else {
            continue;
        };
        let base = base.to_owned();
        let n = count.entry(base.clone()).or_insert(1);
        *n += 1;
        face.name = Some(format!("{base}#{n}"));
    }
}
