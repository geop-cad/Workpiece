//! How two solids stand to each other: the volume they share, and how
//! far apart they are where they share none.
//!
//! The least distance between two boundaries is taken between a place
//! on a face, an edge or a vertex of the one and a place on a face, an
//! edge or a vertex of the other. Every such pair whose boxes are not
//! already farther apart than the least found is searched: from the
//! nearest of a few places on each, downhill (Gauss–Newton steps, held
//! within the parameters) to where the two places are nearest. A place
//! on a face counts if it is within the face's loops; where it is not,
//! one of the face's edges holds the nearest place, and is searched as
//! well.

use shapecraft_core::cancel::checkpoint;
use shapecraft_core::math::{Point2, Point3};

use crate::bezier::Bounds;
use crate::model::Solid;
use crate::query::{SolidData, Trim};
use crate::solve::{Along, At, Over, Shape};
use crate::{Op, Refusal, combine, measure};

/// How two solids stand to each other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meeting {
    /// The volume they share.
    pub volume: f64,
    /// The least distance from a point of the one to a point of the
    /// other: nothing where they share volume or touch.
    pub gap: f64,
}

/// The volume `a` and `b` share and the gap between them. Solids that
/// overlap have a volume and no gap, solids that touch have neither,
/// and solids apart have the gap alone.
pub fn interference(a: &Solid, b: &Solid) -> Result<Meeting, Refusal> {
    if a.is_empty() || b.is_empty() {
        return Ok(Meeting {
            volume: 0.0,
            gap: f64::INFINITY,
        });
    }
    let (da, db) = (SolidData::new(a), SolidData::new(b));
    // Boxes apart: nothing is shared, without asking more.
    let shared = if da.bounds.meets(&db.bounds, a.tolerance() + b.tolerance()) {
        measure(&combine(Op::Intersect, a, b)?.solid)
            .volume
            .max(0.0)
    } else {
        0.0
    };
    if shared > 0.0 {
        return Ok(Meeting {
            volume: shared,
            gap: 0.0,
        });
    }
    Ok(Meeting {
        volume: 0.0,
        gap: gap(a, &da, b, &db),
    })
}

/// The least distance from a point of `a` to a point of `b`, their
/// insides included: nothing where they touch, overlap, or one is
/// inside the other.
pub fn distance(a: &Solid, b: &Solid) -> Result<f64, Refusal> {
    if a.is_empty() || b.is_empty() {
        return Ok(f64::INFINITY);
    }
    let (da, db) = (SolidData::new(a), SolidData::new(b));
    let touching = a.tolerance() + b.tolerance();
    if da.bounds.meets(&db.bounds, touching) {
        // Near each other: whether they touch or share anything is
        // told first — where their boundaries cross (looked for where
        // their pieces' boxes meet, and no further once found), by a
        // place of the one inside the other, and for sure by what they
        // share.
        if gap_over(a, &da, b, &db, Some(touching), false) <= touching {
            return Ok(0.0);
        }
        for (x, dx, y) in [(a, &da, b), (b, &db, a)] {
            let place = (0..x.faces.len()).find_map(|f| crate::query::sample(x, dx, f));
            if let Some((p, _)) = place
                && crate::holds(y, p) == Some(true)
            {
                return Ok(0.0);
            }
        }
        if !combine(Op::Intersect, a, b)?.solid.is_empty() {
            return Ok(0.0);
        }
    }
    let between = gap_over(a, &da, b, &db, None, true);
    if between <= touching {
        return Ok(0.0);
    }
    Ok(between)
}

/// A face, an edge or a vertex, as what is searched of it.
struct Element<'a> {
    shape: Box<dyn Shape + 'a>,
    bounds: Bounds,
    /// The loops of a face.
    trim: Option<&'a Trim>,
    /// A few places over it (see [`places`]): found when first asked
    /// for, for every other it is searched against.
    places: std::cell::OnceCell<Vec<([f64; 2], Point3)>>,
}

fn elements<'a>(solid: &'a Solid, data: &'a SolidData) -> Vec<Element<'a>> {
    let mut out = Vec::new();
    for v in &solid.vertices {
        out.push(Element {
            shape: Box::new(At(v.point)),
            bounds: Bounds::of([v.point]),
            trim: None,
            places: std::cell::OnceCell::new(),
        });
    }
    for (edge, pieces) in solid.edges.iter().zip(&data.edges) {
        out.push(Element {
            shape: Box::new(Along(&edge.curve, edge.range)),
            bounds: pieces
                .iter()
                .fold(Bounds::EMPTY, |b, p| b.union(p.bounds())),
            trim: None,
            places: std::cell::OnceCell::new(),
        });
    }
    for (face, fd) in solid.faces.iter().zip(&data.faces) {
        out.push(Element {
            shape: Box::new(Over(&face.surface, fd.uv)),
            bounds: fd.bounds,
            trim: Some(&fd.trim),
            places: std::cell::OnceCell::new(),
        });
    }
    out
}

/// How far apart two boxes are at least.
fn boxes_apart(a: &Bounds, b: &Bounds) -> f64 {
    let d = (a.lo - b.hi).max(b.lo - a.hi).max(Point3::ZERO);
    d.length()
}

/// The least distance between the boundaries of `a` and `b`.
fn gap(a: &Solid, da: &SolidData, b: &Solid, db: &SolidData) -> f64 {
    gap_over(a, da, b, db, None, false)
}

/// [`gap`], looked for no further once it is found to be `enough` or
/// less: they touch, and how nearly does not matter.
fn gap_over(
    a: &Solid,
    da: &SolidData,
    b: &Solid,
    db: &SolidData,
    enough: Option<f64>,
    fine: bool,
) -> f64 {
    let (ea, eb) = (elements(a, da), elements(b, db));
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (i, x) in ea.iter().enumerate() {
        for (j, y) in eb.iter().enumerate() {
            pairs.push((boxes_apart(&x.bounds, &y.bounds), i, j));
        }
    }
    pairs.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut best = f64::INFINITY;
    for (least, i, j) in pairs {
        if least >= best || enough.is_some_and(|e| best <= e || least > 0.0) {
            break;
        }
        checkpoint();
        if let Some(found) = nearest(&ea[i], &eb[j], best, fine) {
            best = best.min(found);
        }
    }
    best
}

/// A few places over the parameters of `shape`, each with its point.
fn places(shape: &dyn Shape) -> Vec<([f64; 2], Point3)> {
    let domain = shape.domain();
    let n = [0, 1].map(|k| if k < shape.dims() { 7 } else { 1 });
    let mut out = Vec::new();
    for i in 0..n[0] {
        for j in 0..n[1] {
            let at = |k: usize, step: usize| {
                let (lo, hi) = domain[k];
                if n[k] == 1 {
                    lo
                } else {
                    lo + (hi - lo) * step as f64 / (n[k] - 1) as f64
                }
            };
            let x = [at(0, i), at(1, j)];
            out.push((x, shape.eval(x).0));
        }
    }
    out
}

/// The least distance between a place on `a` and a place on `b` that
/// are nearest each other, if it is less than `best` and the places are
/// within the loops of what they are on.
fn nearest(a: &Element<'_>, b: &Element<'_>, best: f64, fine: bool) -> Option<f64> {
    let pa = a.places.get_or_init(|| places(&*a.shape));
    let pb = b.places.get_or_init(|| places(&*b.shape));
    let mut starts: Vec<(f64, [f64; 2], [f64; 2])> = Vec::new();
    for (x, p) in pa {
        for (y, q) in pb {
            starts.push((p.distance_squared(*q), *x, *y));
        }
    }
    starts.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut found: Option<f64> = None;
    for &(_, x, y) in starts.iter().take(8) {
        let (x, y, d) = downhill(&*a.shape, &*b.shape, x, y, fine);
        if d >= best || found.is_some_and(|f| d >= f) {
            continue;
        }
        let within = |e: &Element<'_>, at: [f64; 2]| {
            e.trim
                .is_none_or(|t| t.holds(Point2::new(at[0], at[1])) != Some(false))
        };
        if within(a, x) && within(b, y) {
            found = Some(d);
        }
    }
    found
}

/// From `x` on `a` and `y` on `b`, downhill to where the two places
/// are nearest each other within the shapes' parameters: the places,
/// and how far apart they are.
fn downhill(
    a: &dyn Shape,
    b: &dyn Shape,
    mut x: [f64; 2],
    mut y: [f64; 2],
    fine: bool,
) -> ([f64; 2], [f64; 2], f64) {
    let (na, nb) = (a.dims(), b.dims());
    let n = na + nb;
    let (doma, domb) = (a.domain(), b.domain());
    let bound = |k: usize| if k < na { doma[k] } else { domb[k - na] };
    let apart = |x: [f64; 2], y: [f64; 2]| a.eval(x).0.distance_squared(b.eval(y).0);
    let mut f = apart(x, y);
    if n == 0 {
        return (x, y, f.sqrt());
    }
    let mut damp = 1e-6;
    for _ in 0..200 {
        let (p, dp) = a.eval(x);
        let (q, dq) = b.eval(y);
        let r = p - q;
        // The columns of the Jacobian of r.
        let cols: Vec<Point3> = (0..n)
            .map(|k| if k < na { dp[k] } else { -dq[k - na] })
            .collect();
        let at = |k: usize| if k < na { x[k] } else { y[k - na] };
        let grad: Vec<f64> = cols.iter().map(|c| c.dot(r)).collect();
        // What is held at an end of its range and would leave it stays.
        let free: Vec<usize> = (0..n)
            .filter(|&k| {
                let (lo, hi) = bound(k);
                !(at(k) <= lo && grad[k] > 0.0 || at(k) >= hi && grad[k] < 0.0)
            })
            .collect();
        if free.is_empty() {
            break;
        }
        let mut moved = false;
        for _ in 0..40 {
            let m = free.len();
            let mut h = vec![vec![0.0; m + 1]; m];
            for (i, &ki) in free.iter().enumerate() {
                for (j, &kj) in free.iter().enumerate() {
                    h[i][j] = cols[ki].dot(cols[kj]);
                }
                h[i][i] *= 1.0 + damp;
                h[i][i] += f64::MIN_POSITIVE;
                h[i][m] = -grad[ki];
            }
            let Some(step) = solved(h) else {
                damp *= 10.0;
                continue;
            };
            let (mut nx, mut ny) = (x, y);
            for (i, &k) in free.iter().enumerate() {
                let (lo, hi) = bound(k);
                let to = (at(k) + step[i]).clamp(lo, hi);
                if k < na {
                    nx[k] = to;
                } else {
                    ny[k - na] = to;
                }
            }
            let nf = apart(nx, ny);
            if nf < f {
                (x, y, f) = (nx, ny, nf);
                damp = (damp / 4.0).max(1e-12);
                moved = true;
                break;
            }
            if nf == f {
                break;
            }
            damp *= 10.0;
            if damp > 1e12 {
                break;
            }
        }
        if !moved {
            break;
        }
    }
    // Come to rest where the way from the one place to the other is
    // square to every way either can still go: there they are nearest,
    // and nothing is left to look for.
    let settled = {
        let (p, dp) = a.eval(x);
        let (q, dq) = b.eval(y);
        let r = p - q;
        (0..n).all(|k| {
            let (c, at) = if k < na {
                (dp[k], x[k])
            } else {
                (-dq[k - na], y[k - na])
            };
            let (lo, hi) = bound(k);
            let down = c.dot(r);
            // (Held at an end of its range it would leave.)
            at <= lo && down > 0.0
                || at >= hi && down < 0.0
                || down.abs() <= 1e-9 * c.length() * r.length()
        })
    };
    // And by ever smaller steps along each parameter to where none
    // brings them nearer: far apart and bending, the steps before come
    // to the nearest places slowly.
    let mut h = if fine && !settled { 1.0 / 64.0 } else { 0.0 };
    while h > 1e-15 {
        let mut nearer = false;
        for k in 0..n {
            let (lo, hi) = bound(k);
            for way in [h, -h] {
                let (mut nx, mut ny) = (x, y);
                if k < na {
                    nx[k] = (x[k] + way * (hi - lo)).clamp(lo, hi);
                } else {
                    ny[k - na] = (y[k - na] + way * (hi - lo)).clamp(lo, hi);
                }
                let nf = apart(nx, ny);
                if nf < f {
                    (x, y, f) = (nx, ny, nf);
                    nearer = true;
                }
            }
        }
        if !nearer {
            h /= 2.0;
        }
    }
    (x, y, f.sqrt())
}

/// The solution of the linear system whose rows are `h` (the last
/// column its right side), by elimination; `None` if it has none.
fn solved(mut h: Vec<Vec<f64>>) -> Option<Vec<f64>> {
    let m = h.len();
    for c in 0..m {
        let pivot = (c..m).max_by(|&i, &j| h[i][c].abs().total_cmp(&h[j][c].abs()))?;
        if h[pivot][c].abs() < 1e-300 {
            return None;
        }
        h.swap(c, pivot);
        for r in 0..m {
            if r != c {
                let k = h[r][c] / h[c][c];
                let pivot_row = h[c].clone();
                for (x, p) in h[r].iter_mut().zip(&pivot_row).skip(c) {
                    *x -= k * p;
                }
            }
        }
    }
    let out: Vec<f64> = (0..m).map(|i| h[i][m] / h[i][i]).collect();
    out.iter().all(|v| v.is_finite()).then_some(out)
}
