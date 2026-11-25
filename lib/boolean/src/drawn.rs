//! A hidden-line drawing of a solid: its edges, and its outlines where
//! the line of sight grazes a curved face, each cut into the stretches
//! a viewer sees and the stretches the solid hides.
//!
//! What is seen of a curve changes only where, in the view, it crosses
//! another edge or outline (or touches one, as a cylinder's rim does
//! its outline). So every curve is cut at every such place — found on
//! the chords of both and then followed onto the curves themselves —
//! and each stretch between two cuts is seen or hidden as a whole: by
//! whether the ray from its middle toward the viewer meets the solid
//! again.

use workpiece_core::cancel::checkpoint;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve3, NurbsSurface};

use crate::Refusal;
use crate::bezier::Piece;
use crate::model::{Solid, Walk};
use crate::query::{self, FaceData, SolidData};
use crate::shown::{grid_lines, stops};
use crate::solve::{Along, Over, contacts};

/// A hidden-line drawing: polylines on the solid, to be laid flat in
/// the view.
#[derive(Debug, Clone, Default)]
pub struct Drawn {
    /// The stretches a viewer sees.
    pub visible: Vec<Vec<Point3>>,
    /// The stretches the solid hides.
    pub hidden: Vec<Vec<Point3>>,
}

/// A curve of the drawing: an edge, or an outline of a face.
struct Run<'a> {
    at: Box<dyn Fn(f64) -> Point3 + 'a>,
    /// Its parameters from end to end, with where it is not smooth.
    breaks: Vec<f64>,
    straight: bool,
}

/// How much `surface` faces along `d` at `uv`: nothing where the line
/// of sight grazes it.
fn facing(surface: &NurbsSurface, uv: Point2, d: Point3) -> f64 {
    query::normal(surface, uv).dot(d)
}

/// The outlines of a face seen along `d`: where its surface is grazed,
/// as chains of places in (u, v), each exactly on the outline. Lines
/// of constant u and v, as close as keeps the surface between them
/// within `tolerance`, are searched for where the facing changes sign,
/// and those places are joined cell by cell.
fn outlines(surface: &NurbsSurface, fd: &FaceData, d: Point3, tolerance: f64) -> Vec<Vec<Point2>> {
    let lines = [0, 1].map(|axis| {
        let coarse = grid_lines(surface, fd.uv, axis, tolerance * 0.25);
        // (At least a few to a side, whatever the tolerance.)
        let mut fine = Vec::new();
        for pair in coarse.windows(2) {
            let parts = ((8.0 / (coarse.len() - 1) as f64).ceil() as usize).max(1);
            for k in 0..parts {
                fine.push(pair[0] + (pair[1] - pair[0]) * k as f64 / parts as f64);
            }
        }
        fine.extend(coarse.last());
        fine
    });
    let (nu, nv) = (lines[0].len(), lines[1].len());
    let node = |i: usize, j: usize| Point2::new(lines[0][i], lines[1][j]);
    let sign: Vec<bool> = (0..nu * nv)
        .map(|k| facing(surface, node(k / nv, k % nv), d) > 0.0)
        .collect();
    let positive = |i: usize, j: usize| sign[i * nv + j];
    // Where the facing changes along a side of a cell: found once for
    // each side. A side is named by its lower node and its direction.
    let mut roots: std::collections::HashMap<(usize, usize, usize), Point2> =
        std::collections::HashMap::new();
    let mut root = |i: usize, j: usize, axis: usize| -> (usize, usize, usize) {
        let key = (i, j, axis);
        roots.entry(key).or_insert_with(|| {
            let (mut a, mut b) = (
                node(i, j),
                if axis == 0 {
                    node(i + 1, j)
                } else {
                    node(i, j + 1)
                },
            );
            let start = positive(i, j);
            for _ in 0..80 {
                let mid = (a + b) / 2.0;
                if (facing(surface, mid, d) > 0.0) == start {
                    a = mid;
                } else {
                    b = mid;
                }
            }
            (a + b) / 2.0
        });
        key
    };
    type Side = (usize, usize, usize);
    let mut pieces: Vec<(Side, Side)> = Vec::new();
    for i in 0..nu - 1 {
        checkpoint();
        for j in 0..nv - 1 {
            // The sides of the cell, around it, each with the signs at
            // its ends.
            let around = [
                ((i, j, 0), positive(i, j), positive(i + 1, j)),
                ((i + 1, j, 1), positive(i + 1, j), positive(i + 1, j + 1)),
                ((i, j + 1, 0), positive(i + 1, j + 1), positive(i, j + 1)),
                ((i, j, 1), positive(i, j + 1), positive(i, j)),
            ];
            let cut: Vec<usize> = (0..4).filter(|&k| around[k].1 != around[k].2).collect();
            let mut join =
                |a: usize, b: usize, root: &mut dyn FnMut(usize, usize, usize) -> Side| {
                    let (p, q) = (around[a].0, around[b].0);
                    pieces.push((root(p.0, p.1, p.2), root(q.0, q.1, q.2)));
                };
            match cut[..] {
                [a, b] => join(a, b, &mut root),
                [a, b, c, e] => {
                    // Two outlines through one cell: joined so that
                    // the middle is on the side its sign says.
                    let middle = (node(i, j) + node(i + 1, j + 1)) / 2.0;
                    if (facing(surface, middle, d) > 0.0) == around[a].1 {
                        join(a, e, &mut root);
                        join(b, c, &mut root);
                    } else {
                        join(a, b, &mut root);
                        join(c, e, &mut root);
                    }
                }
                _ => {}
            }
        }
    }
    // The pieces end to end.
    let mut at: std::collections::HashMap<Side, Vec<usize>> = std::collections::HashMap::new();
    for (k, (a, b)) in pieces.iter().enumerate() {
        at.entry(*a).or_default().push(k);
        at.entry(*b).or_default().push(k);
    }
    let mut used = vec![false; pieces.len()];
    let mut chains = Vec::new();
    let mut starts: Vec<Side> = at
        .iter()
        .filter(|(_, v)| v.len() == 1)
        .map(|(k, _)| *k)
        .collect();
    starts.sort_unstable();
    let mut rest: Vec<Side> = at.keys().copied().collect();
    rest.sort_unstable();
    for start in starts.into_iter().chain(rest) {
        let mut chain = vec![start];
        let mut here = start;
        while let Some(&k) = at[&here].iter().find(|&&k| !used[k]) {
            used[k] = true;
            here = if pieces[k].0 == here {
                pieces[k].1
            } else {
                pieces[k].0
            };
            chain.push(here);
        }
        if chain.len() > 1 {
            chains.push(chain.into_iter().map(|side| roots[&side]).collect());
        }
    }
    chains
}

/// The place in (u, v) at `t` along the outline through `chain`: on
/// the outline itself, between the two places of the chain around it.
fn along_outline(surface: &NurbsSurface, chain: &[Point2], d: Point3, t: f64) -> Point2 {
    let k = (t.floor().max(0.0) as usize).min(chain.len() - 2);
    let (a, b) = (chain[k], chain[k + 1]);
    let guess = a.lerp(b, (t - k as f64).clamp(0.0, 1.0));
    let across = (b - a).perp();
    if across.length_squared() == 0.0 {
        return guess;
    }
    // Onto the outline, across the chord.
    let value = |s: f64| facing(surface, guess + across * s, d);
    let (mut s, mut here) = (0.0, value(0.0));
    let step = 1e-6;
    for _ in 0..8 {
        if here == 0.0 {
            break;
        }
        let slope = (value(s + step) - here) / step;
        if slope == 0.0 || !slope.is_finite() {
            break;
        }
        let next = s - here / slope;
        let there = value(next);
        if !there.is_finite() || there.abs() >= here.abs() {
            break;
        }
        (s, here) = (next, there);
    }
    guess + across * s
}

/// Whether the ray from `p` along `toward` (a unit vector) meets the
/// boundary of `solid` again, farther than `skip` from `p`.
fn covered(solid: &Solid, data: &SolidData, p: Point3, toward: Point3, skip: f64) -> bool {
    let size = data.bounds.size().max(f64::MIN_POSITIVE);
    let reach = (data.bounds.lo - p)
        .abs()
        .max((data.bounds.hi - p).abs())
        .length()
        + size * 0.1;
    let end = p + toward * reach;
    let Ok(segment) = Curve3::line(p, end) else {
        return false;
    };
    let ray = [Piece::segment(p, end)];
    let ray_bounds = ray[0].bounds();
    let fine = size * 1e-11;
    solid.faces.iter().zip(&data.faces).any(|(face, fd)| {
        if !fd.bounds.meets(&ray_bounds, fine) {
            return false;
        }
        let hits = contacts(
            &Along(&segment, (0.0, 1.0)),
            &ray,
            &Over(&face.surface, fd.uv),
            &fd.patches,
            fine,
        );
        hits.is_ok_and(|hits| {
            hits.iter().any(|hit| {
                hit.box_a[0].0.min(hit.a[0]) * reach > skip
                    && fd.trim.holds(Point2::new(hit.b[0], hit.b[1])) != Some(false)
            })
        })
    })
}

/// The hidden-line drawing of `solid` seen looking along `direction`:
/// every edge between faces that meet at an angle, and every outline
/// of a curved face, as polylines whose chords are within `tolerance`
/// of them.
pub fn project(solid: &Solid, direction: Point3, tolerance: f64) -> Result<Drawn, Refusal> {
    let d = direction
        .try_normalize()
        .ok_or_else(|| Refusal::Degenerate("the view has no direction".into()))?;
    let data = SolidData::new(solid);
    let size = data.bounds.size().max(f64::MIN_POSITIVE);
    let (right, up) = d.any_orthonormal_pair();
    let flat = |p: Point3| Point2::new(p.dot(right), p.dot(up));

    // The edges that are drawn: between two faces that meet at an
    // angle somewhere along them.
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
    // The outlines, as far as they are within their faces' loops.
    let chains: Vec<(usize, Vec<Point2>)> = solid
        .faces
        .iter()
        .zip(&data.faces)
        .enumerate()
        .flat_map(|(f, (face, fd))| {
            let chains = outlines(&face.surface, fd, d, tolerance);
            chains.into_iter().map(move |c| (f, c))
        })
        .collect();
    let mut runs: Vec<Run<'_>> = Vec::new();
    for (e, edge) in solid.edges.iter().enumerate() {
        let (lo, hi) = edge.range;
        if let [first, second] = uses[e][..] {
            if first.0 == second.0 {
                continue;
            }
            let normal = |(f, l, s): (usize, usize, usize), t: f64| {
                let face = &solid.faces[f];
                let n = query::normal(&face.surface, face.loops[l][s].pcurve.point_at(t));
                if face.outward { n } else { -n }
            };
            let smooth = [0.1, 0.3, 0.5, 0.7, 0.9].into_iter().all(|f| {
                let t = lo + (hi - lo) * f;
                normal(first, t).cross(normal(second, t)).length() < 1e-6
            });
            if smooth {
                continue;
            }
        }
        let mut breaks = vec![lo];
        for &k in edge.curve.knots() {
            if k > lo && k < hi && breaks.last().is_none_or(|&l| k > l) {
                breaks.push(k);
            }
        }
        breaks.push(hi);
        let length: f64 = breaks
            .windows(2)
            .map(|w| edge.point(w[0]).distance(edge.point((w[0] + w[1]) / 2.0)))
            .sum();
        if uses[e].is_empty() || length <= size * 1e-12 {
            continue;
        }
        runs.push(Run {
            at: Box::new(move |t| edge.point(t)),
            breaks,
            straight: edge.curve.degree() <= 1 && !edge.curve.is_rational(),
        });
    }
    for (f, chain) in &chains {
        let (surface, fd) = (&*solid.faces[*f].surface, &data.faces[*f]);
        let place = move |t: f64| along_outline(surface, chain, d, t);
        let within = |t: f64| fd.trim.holds(place(t)) != Some(false);
        // Looked at between the places of the chain, and cut where it
        // enters or leaves the loops.
        let last = (chain.len() - 1) as f64;
        let mut from = within(0.0).then_some(0.0);
        let run = |a: f64, b: f64| {
            let mut breaks = vec![a];
            breaks.extend((a.floor() as usize + 1..b.ceil() as usize).map(|k| k as f64));
            breaks.push(b);
            (b - a > 1e-9).then(|| Run {
                at: Box::new(move |t| {
                    let uv = place(t);
                    surface.point_at(uv.x, uv.y)
                }),
                breaks,
                straight: false,
            })
        };
        for k in 0..chain.len() - 1 {
            let (a, b) = (k as f64, k as f64 + 1.0);
            let (inside, next) = (within(a + 0.5), within(b));
            for (lo, hi, to) in [(a, a + 0.5, inside), (a + 0.5, b, next)] {
                let was = from.is_some();
                if was == to {
                    continue;
                }
                let (mut lo, mut hi) = (lo, hi);
                for _ in 0..50 {
                    let mid = (lo + hi) / 2.0;
                    if within(mid) == was {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                match from.take() {
                    Some(start) => runs.extend(run(start, lo)),
                    None => from = Some(hi),
                }
            }
        }
        if let Some(start) = from {
            runs.extend(run(start, last));
        }
    }

    // Every run as chords, in space and in the view.
    let sampled: Vec<Vec<f64>> = runs
        .iter()
        .map(|r| stops(&*r.at, &r.breaks, r.straight, tolerance))
        .collect();
    let chords: Vec<Vec<Point2>> = runs
        .iter()
        .zip(&sampled)
        .map(|(r, ts)| ts.iter().map(|&t| flat((r.at)(t))).collect())
        .collect();
    let boxes: Vec<(Point2, Point2)> = chords
        .iter()
        .map(|c| {
            c.iter().fold(
                (
                    Point2::splat(f64::INFINITY),
                    Point2::splat(f64::NEG_INFINITY),
                ),
                |(lo, hi), &p| (lo.min(p), hi.max(p)),
            )
        })
        .collect();
    // Where two runs cross or touch in the view: near where their
    // chords come close, on the runs themselves.
    let close = tolerance * 2.0;
    let exact = size * 1e-10;
    let mut cuts: Vec<Vec<f64>> = sampled
        .iter()
        .map(|ts| vec![ts[0], ts[ts.len() - 1]])
        .collect();
    let between = |p: Point2, q: Point2, r: Point2, s: Point2| -> Option<(f64, f64)> {
        // The nearest places of two segments, as shares along each.
        let (u, v, w) = (q - p, s - r, p - r);
        let (a, b, c, e, f) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w), v.dot(w));
        let det = a * c - b * b;
        let mut best: Option<(f64, f64, f64)> = None;
        let mut take = |x: f64, y: f64| {
            let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
            let gap = (p + u * x).distance(r + v * y);
            if best.is_none_or(|b| gap < b.0) {
                best = Some((gap, x, y));
            }
        };
        if det > 1e-14 * a * c {
            take((b * f - c * e) / det, (a * f - b * e) / det);
        }
        for x in [0.0, 1.0] {
            take(x, if c > 0.0 { (f + b * x) / c } else { 0.0 });
        }
        for y in [0.0, 1.0] {
            take(if a > 0.0 { (b * y - e) / a } else { 0.0 }, y);
        }
        best.filter(|b| b.0 <= close).map(|b| (b.1, b.2))
    };
    for i in 0..runs.len() {
        checkpoint();
        for j in i + 1..runs.len() {
            let apart = (boxes[i].0 - boxes[j].1).max(boxes[j].0 - boxes[i].1);
            if apart.x > close || apart.y > close {
                continue;
            }
            for a in 0..chords[i].len() - 1 {
                let (p, q) = (chords[i][a], chords[i][a + 1]);
                let (lo, hi) = (p.min(q), p.max(q));
                for b in 0..chords[j].len() - 1 {
                    let (r, s) = (chords[j][b], chords[j][b + 1]);
                    let apart = (lo - r.max(s)).max(r.min(s) - hi);
                    if apart.x > close || apart.y > close {
                        continue;
                    }
                    let Some((x, y)) = between(p, q, r, s) else {
                        continue;
                    };
                    // From the chords onto the runs: the places of the
                    // two that lie over each other in the view.
                    let (ri, rj) = (&runs[i], &runs[j]);
                    let (i0, i1) = (sampled[i][0], sampled[i][sampled[i].len() - 1]);
                    let (j0, j1) = (sampled[j][0], sampled[j][sampled[j].len() - 1]);
                    let mut s = sampled[i][a] + (sampled[i][a + 1] - sampled[i][a]) * x;
                    let mut t = sampled[j][b] + (sampled[j][b + 1] - sampled[j][b]) * y;
                    let off = |s: f64, t: f64| flat((ri.at)(s)) - flat((rj.at)(t));
                    let (hs, ht) = ((i1 - i0) * 1e-7, (j1 - j0) * 1e-7);
                    let mut met = false;
                    for _ in 0..30 {
                        let f = off(s, t);
                        if f.length() <= exact {
                            met = true;
                            break;
                        }
                        let ds = (off((s + hs).min(i1), t) - off((s - hs).max(i0), t))
                            / ((s + hs).min(i1) - (s - hs).max(i0));
                        let dt = (off(s, (t + ht).min(j1)) - off(s, (t - ht).max(j0)))
                            / ((t + ht).min(j1) - (t - ht).max(j0));
                        let det = ds.perp_dot(dt);
                        if det.abs() <= 1e-12 * ds.length() * dt.length() {
                            // Side by side: the nearer of the two to
                            // the other, along itself alone.
                            let step = -f.dot(ds) / ds.length_squared().max(f64::MIN_POSITIVE);
                            s = (s + step).clamp(i0, i1);
                            let f = off(s, t);
                            let step = -f.dot(dt) / dt.length_squared().max(f64::MIN_POSITIVE);
                            t = (t + step).clamp(j0, j1);
                            continue;
                        }
                        let (to_s, to_t) = (-f.perp_dot(dt) / det, -ds.perp_dot(f) / det);
                        s = (s + to_s).clamp(i0, i1);
                        t = (t + to_t).clamp(j0, j1);
                    }
                    if met || off(s, t).length() <= exact {
                        cuts[i].push(s);
                        cuts[j].push(t);
                    }
                }
            }
        }
    }

    // Each stretch between two cuts, seen or hidden by its middle.
    let mut drawn = Drawn::default();
    let skip = size * 1e-4;
    for ((run, ts), mut cuts) in runs.iter().zip(&sampled).zip(cuts) {
        checkpoint();
        cuts.sort_by(f64::total_cmp);
        let whole = ts[ts.len() - 1] - ts[0];
        cuts.dedup_by(|b, a| *b - *a <= whole * 1e-9);
        let mut line: Vec<Point3> = Vec::new();
        let mut line_hidden = false;
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let hidden = covered(solid, &data, (run.at)((a + b) / 2.0), -d, skip);
            if hidden != line_hidden && !line.is_empty() {
                let done = std::mem::take(&mut line);
                if line_hidden {
                    drawn.hidden.push(done);
                } else {
                    drawn.visible.push(done);
                }
            }
            line_hidden = hidden;
            if line.is_empty() {
                line.push((run.at)(a));
            }
            line.extend(ts.iter().filter(|&&t| t > a && t < b).map(|&t| (run.at)(t)));
            line.push((run.at)(b));
        }
        if line.len() > 1 {
            if line_hidden {
                drawn.hidden.push(line);
            } else {
                drawn.visible.push(line);
            }
        }
    }
    Ok(drawn)
}
