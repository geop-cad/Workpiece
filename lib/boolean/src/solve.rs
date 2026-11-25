//! Where two shapes — a point, a curve, a surface — come within a
//! tolerance of each other.
//!
//! The search is Bézier clipping, the same for every pair: each shape's
//! Bézier pieces are bounded by slabs (a fat line around a curve, a fat
//! plane around a patch), and the other's parameters are clipped to
//! where it can be inside them. A clip that narrows little means several
//! contacts or a tangency, and the piece is halved. What is left when
//! the pieces are small, or straight and flat and still together, is a
//! contact: its closest approach is polished by Newton's method on the
//! shapes themselves and kept if it is within the tolerance.
//!
//! Nothing here knows lines from circles or planes from cylinders.

use workpiece_core::math::Point3;
use workpiece_core::world::topo::{Curve3, NurbsSurface};

use crate::bezier::Piece;

/// Something with up to two parameters and a point at each.
pub(crate) trait Shape {
    /// How many parameters.
    fn dims(&self) -> usize;
    /// The point at `x` and its derivative along each parameter.
    fn eval(&self, x: [f64; 2]) -> (Point3, [Point3; 2]);
    /// The parameters it covers.
    fn domain(&self) -> [(f64, f64); 2];
}

/// A point.
pub(crate) struct At(pub Point3);

impl Shape for At {
    fn dims(&self) -> usize {
        0
    }

    fn eval(&self, _: [f64; 2]) -> (Point3, [Point3; 2]) {
        (self.0, [Point3::ZERO; 2])
    }

    fn domain(&self) -> [(f64, f64); 2] {
        [(0.0, 0.0); 2]
    }
}

/// A curve between two parameters.
pub(crate) struct Along<'a>(pub &'a Curve3, pub (f64, f64));

impl Shape for Along<'_> {
    fn dims(&self) -> usize {
        1
    }

    fn eval(&self, x: [f64; 2]) -> (Point3, [Point3; 2]) {
        let (p, d) = self.0.derivative_at(x[0]);
        (p, [d, Point3::ZERO])
    }

    fn domain(&self) -> [(f64, f64); 2] {
        [self.1, (0.0, 0.0)]
    }
}

/// A surface over a box of its parameters.
pub(crate) struct Over<'a>(pub &'a NurbsSurface, pub [(f64, f64); 2]);

impl Shape for Over<'_> {
    fn dims(&self) -> usize {
        2
    }

    fn eval(&self, x: [f64; 2]) -> (Point3, [Point3; 2]) {
        let (p, du, dv) = self.0.derivatives_at(x[0], x[1]);
        (p, [du, dv])
    }

    fn domain(&self) -> [(f64, f64); 2] {
        self.1
    }
}

/// A place where two shapes are within the tolerance of each other.
#[derive(Debug, Clone)]
pub(crate) struct Contact {
    /// The parameters on the first shape, at the closest approach.
    pub a: [f64; 2],
    /// The parameters on the second.
    pub b: [f64; 2],
    /// The first shape's point there.
    pub pa: Point3,
    /// The second's.
    pub pb: Point3,
    /// How far apart those are.
    pub distance: f64,
    /// The parameters of the first shape the contact was found in: a
    /// small box for a crossing, a long one where the shapes run
    /// together.
    pub box_a: [(f64, f64); 2],
    /// The same on the second shape.
    pub box_b: [(f64, f64); 2],
}

impl Contact {
    /// The middle of the two points.
    pub(crate) fn point(&self) -> Point3 {
        (self.pa + self.pb) / 2.0
    }
}

/// The search gave up: the shapes run together over more places than a
/// contact search should meet.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TooMany;

/// How many unsettled places a search may meet before it gives up.
const BUDGET: usize = 4096;
/// How many contacts it may find.
const MOST: usize = 256;

/// Solves the `m`-by-`m` system `a·x = b` by elimination; `None` if it
/// is singular.
pub(crate) fn solve(mut a: [[f64; 4]; 4], mut b: [f64; 4], m: usize) -> Option<[f64; 4]> {
    for c in 0..m {
        let p = (c..m).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-300 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..m {
            let f = a[r][c] / a[c][c];
            let pivot = a[c];
            for (x, p) in a[r].iter_mut().zip(pivot).take(m).skip(c) {
                *x -= f * p;
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = [0.0; 4];
    for c in (0..m).rev() {
        let s: f64 = (c + 1..m).map(|k| a[c][k] * x[k]).sum();
        x[c] = (b[c] - s) / a[c][c];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// The parameters at which `a` and `b` are closest, near `x` and `y`
/// and inside their domains (damped Gauss–Newton), with the two points.
pub(crate) fn closest(
    a: &dyn Shape,
    b: &dyn Shape,
    x: [f64; 2],
    y: [f64; 2],
) -> ([f64; 2], [f64; 2], Point3, Point3) {
    closest_until(a, b, x, y, -1.0)
}

/// Whether `a` and `b` come within `tol` of each other near `x` and
/// `y`: what [`closest`] would say of how far apart they are there,
/// told as soon as they are that near (the search only ever comes
/// nearer).
/// With it, whether the search ended on the border of `b`'s
/// parameters: held back there, it may not have come to the nearest
/// place.
pub(crate) fn within(
    a: &dyn Shape,
    b: &dyn Shape,
    x: [f64; 2],
    y: [f64; 2],
    tol: f64,
) -> (bool, bool) {
    let (_, at, pa, pb) = closest_until(a, b, x, y, tol);
    let domain = b.domain();
    let held = (0..b.dims()).any(|k| at[k] <= domain[k].0 || at[k] >= domain[k].1);
    (pa.distance(pb) <= tol, held)
}

/// [`closest`], stopped once the two are within `enough` of each other.
fn closest_until(
    a: &dyn Shape,
    b: &dyn Shape,
    x: [f64; 2],
    y: [f64; 2],
    enough: f64,
) -> ([f64; 2], [f64; 2], Point3, Point3) {
    let (da, db) = (a.dims(), b.dims());
    let m = da + db;
    let (dom_a, dom_b) = (a.domain(), b.domain());
    let clamp = |z: [f64; 4]| {
        let mut z = z;
        for k in 0..m {
            let (lo, hi) = if k < da { dom_a[k] } else { dom_b[k - da] };
            z[k] = z[k].clamp(lo, hi);
        }
        z
    };
    let split = |z: [f64; 4]| {
        let mut x = [0.0; 2];
        let mut y = [0.0; 2];
        x[..da].copy_from_slice(&z[..da]);
        y[..db].copy_from_slice(&z[da..m]);
        (x, y)
    };
    let mut z = [0.0; 4];
    z[..da].copy_from_slice(&x[..da]);
    z[da..m].copy_from_slice(&y[..db]);
    z = clamp(z);
    let at = |z: [f64; 4]| {
        let (x, y) = split(z);
        let ((pa, ja), (pb, jb)) = (a.eval(x), b.eval(y));
        let mut j = [Point3::ZERO; 4];
        j[..da].copy_from_slice(&ja[..da]);
        for k in 0..db {
            j[da + k] = -jb[k];
        }
        (pa, pb, j)
    };
    let (mut pa, mut pb, mut j) = at(z);
    let mut damping = 1e-12;
    for _ in 0..60 {
        if m == 0 {
            break;
        }
        let r = pa - pb;
        let f = r.length_squared();
        if r.length() <= enough {
            break;
        }
        let mut h = [[0.0; 4]; 4];
        let mut g = [0.0; 4];
        for i in 0..m {
            g[i] = -j[i].dot(r);
            for k in 0..m {
                h[i][k] = j[i].dot(j[k]);
            }
        }
        let trace: f64 = (0..m).map(|i| h[i][i]).sum();
        if trace == 0.0 {
            break;
        }
        let mut moved = false;
        let mut settled = false;
        let mut step = 0.0_f64;
        while damping < 1e12 {
            let mut hd = h;
            for (i, row) in hd.iter_mut().enumerate().take(m) {
                row[i] += damping * (row[i] + trace * 1e-9);
            }
            if let Some(d) = solve(hd, g, m) {
                // Asked only whether they come within `enough`: a full
                // step that promises next to nothing of what is still
                // missing to that says they do not — the search has all
                // but settled, farther apart. (Not a step held short:
                // that one promises little wherever it is.)
                if enough > 0.0 && damping <= 1e-6 {
                    let gain: f64 = (0..m).map(|k| g[k] * d[k]).sum();
                    if gain <= 1e-3 * (f - enough * enough) {
                        settled = true;
                        break;
                    }
                }
                let mut next = z;
                for k in 0..m {
                    next[k] += d[k];
                }
                let next = clamp(next);
                let (na, nb, nj) = at(next);
                if (na - nb).length_squared() <= f {
                    step = (0..m)
                        .map(|k| (j[k] * (next[k] - z[k])).length())
                        .fold(0.0, f64::max);
                    (z, pa, pb, j) = (next, na, nb, nj);
                    damping = (damping * 0.1).max(1e-14);
                    moved = true;
                    break;
                }
            }
            damping *= 10.0;
        }
        if settled || !moved || step <= 1e-15 * (1.0 + pa.abs().max_element()) {
            break;
        }
    }
    let (x, y) = split(z);
    (x, y, pa, pb)
}

/// The direction to clip parameter `axis` of `x` against `y` along: one
/// across everything else that moves — the other parameter of `x`, and
/// the stretch of `y` — so that the slab around `y` is thin and the
/// parameter is pinned well.
fn across(x: &Piece, axis: usize, y: &Piece) -> Option<Point3> {
    let own = x.extent(axis);
    // The directions everything else moves in, as far as they differ.
    let mut others: Vec<Point3> = Vec::with_capacity(2);
    let mut add = |v: Point3| {
        if others.len() == 2 {
            return;
        }
        let rest = others
            .iter()
            .fold(v, |rest, &o| rest.reject_from_normalized(o));
        if rest.length() > 1e-9 * v.length() && rest.length() > 0.0 {
            others.push(rest.normalize());
        }
    };
    for k in 0..y.dims() {
        add(y.extent(k));
    }
    if x.dims() == 2 {
        add(x.extent(1 - axis));
    }
    match others[..] {
        [] => own.try_normalize(),
        [v] => own.reject_from_normalized(v).try_normalize(),
        _ => Some(others[0].cross(others[1]).normalize()),
    }
}

/// Whether some direction shows `a` and `b` more than `tol` apart.
pub(crate) fn apart(a: &Piece, b: &Piece, tol: f64) -> bool {
    if !a.bounds().meets(&b.bounds(), tol) {
        return true;
    }
    let mut directions = Vec::with_capacity(4);
    for p in [a, b] {
        if p.dims() == 2 {
            directions.push(p.extent(0).cross(p.extent(1)));
        }
    }
    if a.dims() == 1 && b.dims() == 1 {
        directions.push(a.extent(0).cross(b.extent(0)));
    }
    // A curve beside a patch: across the curve and each way the patch
    // runs — what shows a line passing a flat patch outside its sides,
    // which the patch's own normal does not.
    for (curve, patch) in [(a, b), (b, a)] {
        if curve.dims() == 1 && patch.dims() == 2 {
            let along = curve.extent(0);
            directions.push(along.cross(patch.extent(0)));
            directions.push(along.cross(patch.extent(1)));
        }
    }
    directions.into_iter().any(|d| {
        let Some(n) = d.try_normalize() else {
            return false;
        };
        let ((a0, a1), (b0, b1)) = (a.reach(n), b.reach(n));
        a0 - b1 > tol || b0 - a1 > tol
    })
}

/// One clipping of each parameter of `x` against `y`: `None` if `x`
/// cannot be within `tol` of `y`, else whether it was narrowed.
fn clip(x: &mut Piece, y: &Piece, tol: f64) -> Option<bool> {
    let mut narrowed = false;
    for axis in 0..2 {
        if x.n[axis] < 2 {
            continue;
        }
        // Across everything else, where the slab is thin; and along the
        // parameter itself, which bounds where the two run together.
        let pinned = across(x, axis, y);
        let own = x
            .extent(axis)
            .try_normalize()
            .filter(|o| pinned.is_none_or(|n| n.dot(*o).abs() < 0.999));
        for n in pinned.into_iter().chain(own) {
            let (lo, hi) = y.reach(n);
            let (x0, x1) = x.clip(axis, n, lo - tol, hi + tol)?;
            if x1 - x0 < 0.8 {
                // Never to nothing: rounding must not lose the contact.
                let (x0, x1) = ((x0 - 1e-9).max(0.0), (x1 + 1e-9).min(1.0));
                let (a, b) = x.range[axis];
                if (b - a) * (x1 - x0) > 1e-14 * (1.0 + a.abs().max(b.abs())) {
                    x.restrict(axis, x0, x1);
                    narrowed = true;
                }
            }
        }
    }
    Some(narrowed)
}

/// The places where the pieces `a` and `b` can be within `tol`: boxes
/// of their parameters, small or straight enough to hold one contact.
fn search(
    a: &Piece,
    b: &Piece,
    tol: f64,
    leaves: &mut Vec<(Piece, Piece)>,
    budget: &mut usize,
) -> Result<(), TooMany> {
    let fine = tol.max(1e-13 * (1.0 + a.bounds().hi.abs().max_element()));
    let mut stack = vec![(a.clone(), b.clone())];
    while let Some((mut a, mut b)) = stack.pop() {
        *budget = budget.checked_sub(1).ok_or(TooMany)?;
        let mut alive = true;
        for _ in 0..64 {
            if apart(&a, &b, tol) {
                alive = false;
                break;
            }
            let (Some(p), Some(q)) = (clip(&mut a, &b, tol), clip(&mut b, &a, tol)) else {
                alive = false;
                break;
            };
            if !(p || q) {
                break;
            }
        }
        if !alive {
            continue;
        }
        let (sa, sb) = (a.bounds().size(), b.bounds().size());
        let small = sa <= fine && sb <= fine;
        // Straight and flat and still together: they run along each
        // other, and clipping has already cut them to where they do.
        let together = a.bend() + b.bend() <= tol * 0.25;
        if small || together {
            leaves.push((a, b));
            if leaves.len() > BUDGET {
                return Err(TooMany);
            }
            continue;
        }
        // Halve the bigger, along its longer parameter.
        let (big, a_is_big) = if (sa >= sb && a.dims() > 0) || b.dims() == 0 {
            (&a, true)
        } else {
            (&b, false)
        };
        let axis = if big.dims() == 2 && big.span(1) > big.span(0) {
            1
        } else if big.n[0] > 1 {
            0
        } else {
            1
        };
        for half in big.halves(axis) {
            stack.push(if a_is_big {
                (half, b.clone())
            } else {
                (a.clone(), half)
            });
        }
    }
    Ok(())
}

fn boxes_meet(p: &[(f64, f64); 2], q: &[(f64, f64); 2]) -> bool {
    (0..2).all(|k| {
        let pad = 1e-9 * (1.0 + p[k].1.abs().max(q[k].1.abs()));
        p[k].0 <= q[k].1 + pad && q[k].0 <= p[k].1 + pad
    })
}

fn box_union(p: &[(f64, f64); 2], q: &[(f64, f64); 2]) -> [(f64, f64); 2] {
    std::array::from_fn(|k| (p[k].0.min(q[k].0), p[k].1.max(q[k].1)))
}

/// Every place where `a` and `b` (with their Bézier pieces) are within
/// `tol` of each other, each once.
pub(crate) fn contacts(
    a: &dyn Shape,
    a_pieces: &[Piece],
    b: &dyn Shape,
    b_pieces: &[Piece],
    tol: f64,
) -> Result<Vec<Contact>, TooMany> {
    let mut leaves = Vec::new();
    let mut budget = BUDGET * 8;
    for pa in a_pieces {
        let bounds = pa.bounds();
        for pb in b_pieces {
            if bounds.meets(&pb.bounds(), tol) {
                search(pa, pb, tol, &mut leaves, &mut budget)?;
            }
        }
    }
    let mut found: Vec<Contact> = Vec::new();
    for (la, lb) in leaves {
        let (x, y, pa, pb) = closest(a, b, la.middle(), lb.middle());
        let distance = pa.distance(pb);
        if distance > tol * (1.0 + 1e-9) + 1e-14 {
            continue;
        }
        found.push(Contact {
            a: x,
            b: y,
            pa,
            pb,
            distance,
            box_a: la.range,
            box_b: lb.range,
        });
    }
    // One contact found in neighbouring boxes, or polished to the same
    // parameters from two, is one contact.
    let near = |p: &[f64; 2], q: &[f64; 2], domain: [(f64, f64); 2]| {
        (0..2).all(|k| (p[k] - q[k]).abs() <= 1e-7 * (domain[k].1 - domain[k].0).abs())
    };
    let (dom_a, dom_b) = (a.domain(), b.domain());
    let same = |c: &Contact, d: &Contact| {
        (boxes_meet(&c.box_a, &d.box_a) && boxes_meet(&c.box_b, &d.box_b))
            || (near(&c.a, &d.a, dom_a) && near(&c.b, &d.b, dom_b))
    };
    let mut merged = true;
    while merged {
        merged = false;
        'pairs: for i in 0..found.len() {
            for k in i + 1..found.len() {
                if same(&found[i], &found[k]) {
                    let other = found.swap_remove(k);
                    let c = &mut found[i];
                    let (box_a, box_b) = (
                        box_union(&c.box_a, &other.box_a),
                        box_union(&c.box_b, &other.box_b),
                    );
                    if other.distance < c.distance {
                        *c = other;
                    }
                    (c.box_a, c.box_b) = (box_a, box_b);
                    merged = true;
                    break 'pairs;
                }
            }
        }
    }
    if found.len() > MOST {
        return Err(TooMany);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bezier::{curve_pieces, surface_pieces};

    fn circle(radius: f64, z: f64) -> Curve3 {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let xy = [
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (-1.0, 1.0),
            (-1.0, 0.0),
            (-1.0, -1.0),
            (0.0, -1.0),
            (1.0, -1.0),
            (1.0, 0.0),
        ];
        Curve3::new(
            2,
            vec![
                0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
            ],
            xy.iter()
                .map(|&(x, y)| Point3::new(x * radius, y * radius, z))
                .collect(),
            Some(vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0]),
        )
        .expect("a circle")
    }

    /// The cylinder of `radius` around z from 0 to `height`.
    fn cylinder(radius: f64, height: f64) -> NurbsSurface {
        let c = circle(radius, 0.0);
        let points = c
            .control_points()
            .iter()
            .flat_map(|p| [*p, *p + Point3::Z * height])
            .collect();
        let weights = c.weights().iter().flat_map(|&w| [w, w]).collect();
        NurbsSurface::new(
            2,
            1,
            c.knots().to_vec(),
            vec![0.0, 0.0, 1.0, 1.0],
            9,
            2,
            points,
            Some(weights),
        )
        .expect("a cylinder")
    }

    fn curve_surface(c: &Curve3, s: &NurbsSurface, tol: f64) -> Vec<Contact> {
        let (t0, t1) = c.domain();
        let (u, v) = s.domain();
        contacts(
            &Along(c, (t0, t1)),
            &curve_pieces(c, t0, t1),
            &Over(s, [u, v]),
            &surface_pieces(s, u, v),
            tol,
        )
        .expect("few contacts")
    }

    #[test]
    fn lines_pierce_and_touch_cylinders() -> anyhow::Result<()> {
        let s = cylinder(1.0, 2.0);
        // Through the axis: in and out.
        let line = Curve3::line(Point3::new(-3.0, 0.2, 1.0), Point3::new(3.0, 0.2, 1.0))?;
        let mut hits = curve_surface(&line, &s, 1e-9);
        hits.sort_by(|a, b| a.a[0].total_cmp(&b.a[0]));
        assert_eq!(hits.len(), 2);
        let x = (1.0_f64 - 0.04).sqrt();
        assert!(hits[0].pa.abs_diff_eq(Point3::new(-x, 0.2, 1.0), 1e-12));
        assert!(hits[1].pb.abs_diff_eq(Point3::new(x, 0.2, 1.0), 1e-12));
        // Past it: nothing. Touching it: once, where it touches.
        let past = Curve3::line(Point3::new(-3.0, 1.1, 1.0), Point3::new(3.0, 1.1, 1.0))?;
        assert!(curve_surface(&past, &s, 1e-9).is_empty());
        let touch = Curve3::line(Point3::new(-3.0, 1.0, 1.0), Point3::new(3.0, 1.0, 1.0))?;
        let hits = curve_surface(&touch, &s, 1e-9);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].pa.abs_diff_eq(Point3::new(0.0, 1.0, 1.0), 1e-6));
        // Along it: one contact as long as the cylinder.
        let along = Curve3::line(Point3::new(0.6, 0.8, -1.0), Point3::new(0.6, 0.8, 3.0))?;
        let hits = curve_surface(&along, &s, 1e-9);
        assert_eq!(hits.len(), 1);
        let (a, b) = hits[0].box_a[0];
        assert!(
            (a - 0.25).abs() < 1e-6 && (b - 0.75).abs() < 1e-6,
            "{a} {b}"
        );
        Ok(())
    }

    #[test]
    fn curves_cross_and_points_lie_on_shapes() -> anyhow::Result<()> {
        let c = circle(1.0, 0.5);
        let line = Curve3::line(Point3::new(0.0, 0.0, 0.5), Point3::new(2.0, 2.0, 0.5))?;
        let pieces = |c: &Curve3| curve_pieces(c, c.domain().0, c.domain().1);
        let hits = contacts(
            &Along(&c, c.domain()),
            &pieces(&c),
            &Along(&line, line.domain()),
            &pieces(&line),
            1e-9,
        )
        .expect("few");
        assert_eq!(hits.len(), 1);
        let h = std::f64::consts::FRAC_1_SQRT_2;
        assert!(hits[0].pa.abs_diff_eq(Point3::new(h, h, 0.5), 1e-12));
        assert!((hits[0].a[0] - 0.125).abs() < 1e-12);
        // A point on the cylinder, and one off it.
        let s = cylinder(1.0, 2.0);
        let (u, v) = s.domain();
        let on = |p: Point3| {
            contacts(
                &At(p),
                &[Piece::point(p)],
                &Over(&s, [u, v]),
                &surface_pieces(&s, u, v),
                1e-7,
            )
            .expect("few")
        };
        let hits = on(Point3::new(-h, -h, 1.5));
        assert_eq!(hits.len(), 1);
        assert!((hits[0].b[0] - 0.625).abs() < 1e-9 && (hits[0].b[1] - 0.75).abs() < 1e-9);
        assert!(on(Point3::new(-h, -h - 1e-5, 1.5)).is_empty());
        // The seam is met from both sides.
        assert_eq!(on(Point3::new(1.0, 0.0, 1.0)).len(), 2);
        Ok(())
    }
}
