//! Closed curves where two surfaces cross that no edge reaches: a ball
//! through the middle of a plate's face.
//!
//! Two patches whose normals are nowhere parallel cannot cross in a
//! closed curve inside both; whatever they cross in reaches the border
//! of one of them, and is found there, where a border curve meets the
//! other patch. Patches whose normals may be parallel are halved until
//! they are not, or are too small or too nearly the same surface to
//! matter.

use shapecraft_core::math::{Point2, Point3};

use crate::bezier::{Bounds, Piece};
use crate::solve::{At, Over, Shape, apart, contacts, within};

/// A curve of constant u or v on a surface.
struct Iso<'a> {
    surface: &'a Over<'a>,
    /// Which parameter runs along it.
    free: usize,
    /// The other one's value.
    at: f64,
    range: (f64, f64),
}

impl Iso<'_> {
    fn uv(&self, t: f64) -> [f64; 2] {
        if self.free == 0 {
            [t, self.at]
        } else {
            [self.at, t]
        }
    }
}

impl Shape for Iso<'_> {
    fn dims(&self) -> usize {
        1
    }

    fn eval(&self, x: [f64; 2]) -> (Point3, [Point3; 2]) {
        let (p, d) = self.surface.eval(self.uv(x[0]));
        (p, [d[self.free], Point3::ZERO])
    }

    fn domain(&self) -> [(f64, f64); 2] {
        [self.range, (0.0, 0.0)]
    }
}

/// The axis and half-angle of a cone that holds every normal of `p`;
/// `None` if no narrow one does.
fn cone(p: &Piece) -> Option<(Point3, f64)> {
    let axis = p.extent(0).cross(p.extent(1)).try_normalize()?;
    let (nu, nv) = (p.n[0], p.n[1]);
    // A normal is a mix of the crosses of the net's steps.
    let mut least = 1.0_f64;
    for iu in 0..nu.saturating_sub(1) {
        for ju in 0..nv {
            let du = p.at(iu + 1, ju) - p.at(iu, ju);
            let lu = du.length();
            for iv in 0..nu {
                for jv in 0..nv.saturating_sub(1) {
                    let dv = p.at(iv, jv + 1) - p.at(iv, jv);
                    let lv = dv.length();
                    let n = du.cross(dv);
                    let ln = n.length();
                    if ln <= 1e-12 * lu * lv {
                        if lu == 0.0 || lv == 0.0 {
                            continue;
                        }
                        return None;
                    }
                    least = least.min(n.dot(axis) / ln);
                }
            }
        }
    }
    (least > 0.05).then(|| (axis, least.clamp(-1.0, 1.0).acos()))
}

/// A cone that holds every normal of a patch, if a narrow one does.
type Cone = Option<(Point3, f64)>;

/// Whether the patches may have parallel normals somewhere.
fn may_align(a: &Cone, b: &Cone) -> bool {
    let (Some((na, wa)), Some((nb, wb))) = (a, b) else {
        return true;
    };
    let between = na.dot(*nb).clamp(-1.0, 1.0).acos();
    let wide = wa + wb + 1e-9;
    between <= wide || std::f64::consts::PI - between <= wide
}

/// A patch with what is asked of it again and again while it is held
/// against halves of another: worked out once.
struct Held {
    piece: Piece,
    bounds: Bounds,
    /// How big its box is.
    size: f64,
    /// From start to end along each parameter.
    extent: [Point3; 2],
    /// Across both: its normal, all in all — with how far it reaches
    /// along that.
    normal: Option<(Point3, f64, f64)>,
    /// The cone of its normals: worked out when first asked for (not
    /// for a patch that is apart from what it is held against).
    cone: std::cell::OnceCell<Cone>,
}

impl Held {
    fn of(piece: Piece) -> Self {
        let bounds = piece.bounds();
        let extent = [piece.extent(0), piece.extent(1)];
        let normal = extent[0].cross(extent[1]).try_normalize().map(|n| {
            let (lo, hi) = piece.reach(n);
            (n, lo, hi)
        });
        Self {
            cone: std::cell::OnceCell::new(),
            size: bounds.size(),
            bounds,
            extent,
            normal,
            piece,
        }
    }

    fn cone(&self) -> &Cone {
        self.cone.get_or_init(|| cone(&self.piece))
    }

    /// How far it is from flat: as [`Piece::bend`] tells it.
    fn bend(&self) -> f64 {
        match self.normal {
            Some((_, lo, hi)) => hi - lo,
            None => self.size,
        }
    }

    /// Its two halves, along the longer of its parameters.
    fn halves(&self) -> [Held; 2] {
        let axis = usize::from(self.extent[1].length() > self.extent[0].length());
        self.piece.halves(axis).map(Held::of)
    }
}

/// Whether some direction shows the patches more than `tol` apart: as
/// [`apart`] tells it of two patches.
fn held_apart(a: &Held, b: &Held, tol: f64) -> bool {
    if a.piece.dims() != 2 || b.piece.dims() != 2 {
        return apart(&a.piece, &b.piece, tol);
    }
    if !a.bounds.meets(&b.bounds, tol) {
        return true;
    }
    if let Some((n, a0, a1)) = a.normal {
        let (b0, b1) = b.piece.reach(n);
        if a0 - b1 > tol || b0 - a1 > tol {
            return true;
        }
    }
    if let Some((n, b0, b1)) = b.normal {
        let (a0, a1) = a.piece.reach(n);
        if a0 - b1 > tol || b0 - a1 > tol {
            return true;
        }
    }
    false
}

/// What a surface over a box of its parameters lies inside, patch by
/// patch: each patch's box, the slab along its normal, and its middle.
/// A point that is not within a tolerance of one of them is not within
/// it of the surface — told without looking for the nearest place on
/// it.
pub(crate) struct Hull(Vec<Around>, Told);

/// The points found to be on the surface already. (The corners of a
/// patch are asked after again with each halving of the patch it is
/// held against.)
type Told = std::cell::RefCell<std::collections::HashSet<[u64; 4]>>;

/// What one patch lies inside: its box, the slab along its normal (the
/// normal, and from where to where along it), and its middle.
type Around = (Bounds, Option<(Point3, f64, f64)>, [f64; 2]);

impl Hull {
    /// What the patches `patches` of a surface lie inside.
    pub(crate) fn of(patches: &[Piece]) -> Self {
        let around = |p: &Piece| {
            let slab = p.extent(0).cross(p.extent(1)).try_normalize().map(|n| {
                let (lo, hi) = p.reach(n);
                (n, lo, hi)
            });
            (p.bounds(), slab, p.middle())
        };
        Self(patches.iter().map(around).collect(), Told::default())
    }

    /// The middles of the patches `p` may be within `tol` of.
    fn near(&self, p: Point3, tol: f64) -> impl Iterator<Item = [f64; 2]> + '_ {
        // (A little more than the tolerance: what is on the surface is
        // inside what bounds it only up to rounding.)
        let pad = tol * (1.0 + 1e-6) + 1e-12 * (1.0 + p.abs().max_element());
        let holds = move |(bounds, slab, _): &&Around| {
            bounds.holds(p, pad)
                && slab.is_none_or(|(n, lo, hi)| {
                    let d = p.dot(n);
                    d >= lo - pad && d <= hi + pad
                })
        };
        self.0.iter().filter(holds).map(|(_, _, middle)| *middle)
    }

    /// Whether `p` is within `tol` of the surface `over` that this
    /// bounds: looked for from `first` — and, where that search ends
    /// held back at the border of the surface's parameters, from the
    /// middle of every patch the point may be on (the nearest place is
    /// come to downhill, which from the far side of a surface that
    /// closes on itself, across its seam, ends at the seam).
    fn holds(&self, p: Point3, over: &Over<'_>, first: [f64; 2], tol: f64) -> bool {
        let mut near = self.near(p, tol).peekable();
        if near.peek().is_none() {
            return false;
        }
        let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), tol.to_bits()];
        // (Only that it is: a search that did not find it from one
        // place may find it from another.)
        if self.1.borrow().contains(&key) {
            return true;
        }
        let (found, held) = within(&At(p), over, [0.0; 2], first, tol);
        let on =
            found || (held && near.any(|middle| within(&At(p), over, [0.0; 2], middle, tol).0));
        if on {
            self.1.borrow_mut().insert(key);
        }
        on
    }
}

/// The border curves of patch `p`: each as a Bézier piece, the
/// parameter that runs along it, and the value of the other.
fn borders(p: &Piece) -> Vec<(Piece, usize, f64)> {
    let (nu, nv) = (p.n[0], p.n[1]);
    let line = |free: usize, last: bool| {
        let cp = if free == 0 {
            let j = if last { nv - 1 } else { 0 };
            (0..nu).map(|i| p.cp[i * nv + j]).collect()
        } else {
            let i = if last { nu - 1 } else { 0 };
            (0..nv).map(|j| p.cp[i * nv + j]).collect()
        };
        let fixed = p.range[1 - free];
        (
            Piece {
                n: [if free == 0 { nu } else { nv }, 1],
                cp,
                range: [p.range[free], (0.0, 0.0)],
            },
            free,
            if last { fixed.1 } else { fixed.0 },
        )
    };
    vec![line(0, false), line(0, true), line(1, false), line(1, true)]
}

/// Whether the patches `patches` of one surface all lie in one plane,
/// to within `within`: the surface is flat.
pub(crate) fn flat(patches: &[Piece], within: f64) -> bool {
    let Some(first) = patches.first() else {
        return false;
    };
    let Some(n) = first.extent(0).cross(first.extent(1)).try_normalize() else {
        return false;
    };
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in patches {
        if p.dims() != 2 {
            return false;
        }
        let (a, b) = p.reach(n);
        (lo, hi) = (lo.min(a), hi.max(b));
    }
    hi - lo <= within
}

/// Where the border curve `border` (of `iso`) goes through the flat
/// patch `flat` of the surface `over`: the parameter on the border,
/// how far along it to either side it stays within `tol` of the plane,
/// and the (u, v) on the surface. `None` where this does not tell it —
/// the patch is not flat, or the border comes within the tolerance of
/// its plane without going through it (it touches, or ends there):
/// that is looked for as between any two shapes.
fn through_flat(
    iso: &Iso<'_>,
    border: &Piece,
    over: &Over<'_>,
    flat: &Held,
    tol: f64,
) -> Option<Vec<(f64, f64, [f64; 2])>> {
    use crate::bezier::{bernstein, roots};
    let (n, lo, hi) = flat.normal?;
    if hi - lo > tol * 1e-3 || border.cp.len() < 2 {
        return None;
    }
    let level = (lo + hi) / 2.0;
    // The border's height over the plane, times its weight.
    let g: Vec<f64> = border
        .cp
        .iter()
        .map(|h| n.dot(Point3::new(h[0], h[1], h[2])) - level * h[3])
        .collect();
    let heavy = border.cp.iter().fold(0.0_f64, |m, h| m.max(h[3]));
    if border.cp.iter().any(|h| h[3] <= 0.0) {
        return None;
    }
    let close = tol * 2.0 * heavy;
    if g[0].abs() <= close || g[g.len() - 1].abs() <= close {
        return None;
    }
    // Where it turns back, it is clear of the plane.
    if g.len() > 2 {
        let d: Vec<f64> = g.windows(2).map(|w| w[1] - w[0]).collect();
        if d.iter().any(|x| *x != 0.0) {
            for s in roots(&d, 0.0)? {
                if bernstein(&g, s).abs() <= close {
                    return None;
                }
            }
        }
    }
    let (from, to) = border.range[0];
    let degree = (g.len() - 1) as f64;
    let d: Vec<f64> = g.windows(2).map(|w| (w[1] - w[0]) * degree).collect();
    let mut out = Vec::new();
    if g.iter().all(|x| *x > 0.0) || g.iter().all(|x| *x < 0.0) {
        return Some(out);
    }
    for s in roots(&g, 0.0)? {
        let t = from + (to - from) * s;
        let p = iso.eval([t, 0.0]).0;
        let (_, uv, _, q) = crate::solve::closest(&At(p), over, [0.0; 2], flat.piece.middle());
        if q.distance(p) > tol {
            continue;
        }
        let inside = (0..2).all(|k| {
            let (a, b) = flat.piece.range[k];
            let pad = (b - a).abs() * 1e-9;
            uv[k] >= a - pad && uv[k] <= b + pad
        });
        if !inside {
            continue;
        }
        // (How far along it stays within the tolerance: by how fast
        // it goes through.)
        let slope = bernstein(&d, s).abs();
        let reach = if slope > 0.0 {
            (tol * heavy / slope).min(1.0) * (to - from)
        } else {
            to - from
        };
        out.push((t, reach, uv));
    }
    Some(out)
}

/// Points of the crossing of patch `a` of surface `sa` with patch `b` of
/// `sb`, at least one on every closed curve of it inside both: each with
/// its (u, v) on the two surfaces.
#[allow(clippy::too_many_arguments)]
pub(crate) fn seeds(
    sa: (&Over<'_>, bool),
    a: &Piece,
    sb: (&Over<'_>, bool),
    b: &Piece,
    hulls: (&Hull, &Hull),
    tol: f64,
    left: &mut usize,
    out: &mut Vec<(Point3, Point2, Point2)>,
) {
    if apart(a, b, tol) {
        return;
    }
    let (a, b) = (Held::of(a.clone()), Held::of(b.clone()));
    seeds_of(sa, &a, sb, &b, hulls, tol, 0, left, out);
}

#[allow(clippy::too_many_arguments)]
fn seeds_of(
    sa: (&Over<'_>, bool),
    held_a: &Held,
    sb: (&Over<'_>, bool),
    held_b: &Held,
    hulls: (&Hull, &Hull),
    tol: f64,
    depth: usize,
    left: &mut usize,
    out: &mut Vec<(Point3, Point2, Point2)>,
) {
    shapecraft_core::cancel::checkpoint();
    if held_apart(held_a, held_b, tol) {
        return;
    }
    let (a, b) = (&held_a.piece, &held_b.piece);
    if !may_align(held_a.cone(), held_b.cone()) {
        for (first, x, sx, y, sy, hy, hy_held) in [
            (true, a, (sa.0, sa.1), b, sb.0, hulls.1, held_b),
            (false, b, (sb.0, sb.1), a, sa.0, hulls.0, held_a),
        ] {
            for (border, free, at) in borders(x) {
                // A border along the rim of what the face's loops lie
                // in: a crossing there is on the loops or outside them,
                // inside no face — nothing a closed curve is found by.
                if sx.1 {
                    let (lo, hi) = sx.0.domain()[1 - free];
                    let near = (hi - lo).abs() * 1e-9;
                    if (at - lo).abs() <= near || (at - hi).abs() <= near {
                        continue;
                    }
                }
                let iso = Iso {
                    surface: sx.0,
                    free,
                    at,
                    range: border.range[0],
                };
                // A border that lies in the other patch from end to end
                // (a rim on a plane) is no crossing of it — and nothing
                // clipping narrows: told beforehand, from a few places
                // along it.
                let (from, to) = iso.range;
                let lies_in = [0.0, 0.21, 0.5, 0.77, 1.0].iter().all(|share| {
                    let p = iso.eval([from + (to - from) * share, 0.0]).0;
                    hy.holds(p, sy, y.middle(), tol)
                });
                if lies_in {
                    continue;
                }
                // Where the other patch is flat, the border meets it
                // where it crosses its plane: the roots of a
                // polynomial, not a search.
                if let Some(found) = through_flat(&iso, &border, sy, hy_held, tol) {
                    for (t, reach, other) in found {
                        let (lo, hi) = (t - reach, t + reach);
                        let (lo, hi) = (lo.max(from), hi.min(to));
                        let long = iso.eval([lo, 0.0]).0.distance(iso.eval([hi, 0.0]).0);
                        let whole = iso.eval([from, 0.0]).0.distance(iso.eval([to, 0.0]).0);
                        if long > 16.0 * tol && long > 0.2 * whole {
                            continue;
                        }
                        let p = iso.eval([t, 0.0]).0;
                        let (own, other) =
                            (Point2::from_array(iso.uv(t)), Point2::from_array(other));
                        out.push(if first {
                            (p, own, other)
                        } else {
                            (p, other, own)
                        });
                    }
                    continue;
                }
                let Ok(found) = contacts(&iso, &[border], sy, std::slice::from_ref(y), tol) else {
                    continue;
                };
                for c in found {
                    // A long contact is a border lying in the other
                    // surface, not a crossing of it. (Long for the
                    // border: one that crosses at a shallow angle is
                    // within the tolerance of the surface for many
                    // times the tolerance, and crosses all the same.)
                    let (lo, hi) = c.box_a[0];
                    let long = iso.eval([lo, 0.0]).0.distance(iso.eval([hi, 0.0]).0);
                    let (from, to) = iso.range;
                    let whole = iso.eval([from, 0.0]).0.distance(iso.eval([to, 0.0]).0);
                    if long > 16.0 * tol && long > 0.2 * whole {
                        continue;
                    }
                    let (own, other) =
                        (Point2::from_array(iso.uv(c.a[0])), Point2::from_array(c.b));
                    out.push(if first {
                        (c.point(), own, other)
                    } else {
                        (c.point(), other, own)
                    });
                }
            }
        }
        return;
    }
    // The same surface, here: flat and together, or every corner of one
    // on the other.
    // (Or as deep as is looked: two surfaces that barely dip into each
    // other face nearly one way all over the small loop they cross in,
    // and are looked at closely — as long as there is looking left for
    // it, which surfaces that touch along a curve would use up.)
    if depth >= 14 && (depth >= 44 || *left == 0) || held_a.bend() + held_b.bend() <= tol * 0.25 {
        return;
    }
    *left = left.saturating_sub(1);
    let on = |x: &Piece, sy: &Over<'_>, y: &Piece, hy: &Hull| {
        let corners = [
            x.at(0, 0),
            x.at(x.n[0] - 1, 0),
            x.at(0, x.n[1] - 1),
            x.at(x.n[0] - 1, x.n[1] - 1),
            x.eval([0.5, 0.5]),
            x.eval([0.31, 0.17]),
            x.eval([0.73, 0.38]),
            x.eval([0.22, 0.81]),
        ];
        corners
            .into_iter()
            .all(|p| hy.holds(p, sy, y.middle(), tol))
    };
    // (Looked for every third time the patches are halved: patches of
    // one surface are halved a few times more before they are told,
    // and those of two are not asked each time.)
    if depth.is_multiple_of(3) && (on(a, sb.0, b, hulls.1) || on(b, sa.0, a, hulls.0)) {
        return;
    }
    if held_a.size >= held_b.size {
        for half in held_a.halves() {
            seeds_of(sa, &half, sb, held_b, hulls, tol, depth + 1, left, out);
        }
    } else {
        for half in held_b.halves() {
            seeds_of(sa, held_a, sb, &half, hulls, tol, depth + 1, left, out);
        }
    }
}

/// Places where patch `a` of `sa` and patch `b` of `sb` may touch — be
/// together with one normal — each as the middles of two small patches
/// to look for the touching from (many of them around each place).
#[allow(clippy::too_many_arguments)]
pub(crate) fn touches(
    sa: &Over<'_>,
    a: &Piece,
    sb: &Over<'_>,
    b: &Piece,
    hulls: (&Hull, &Hull),
    tol: f64,
    out: &mut Vec<(Point2, Point2)>,
) {
    if apart(a, b, tol) {
        return;
    }
    let (a, b) = (Held::of(a.clone()), Held::of(b.clone()));
    touches_of(sa, &a, sb, &b, hulls, tol, 0, out);
}

#[allow(clippy::too_many_arguments)]
fn touches_of(
    sa: &Over<'_>,
    held_a: &Held,
    sb: &Over<'_>,
    held_b: &Held,
    hulls: (&Hull, &Hull),
    tol: f64,
    depth: usize,
    out: &mut Vec<(Point2, Point2)>,
) {
    shapecraft_core::cancel::checkpoint();
    if out.len() > 512
        || held_apart(held_a, held_b, tol)
        || !may_align(held_a.cone(), held_b.cone())
    {
        return;
    }
    let (a, b) = (&held_a.piece, &held_b.piece);
    if held_a.bend() + held_b.bend() <= tol * 0.25 {
        return;
    }
    let on = |x: &Piece, sy: &Over<'_>, y: &Piece, hy: &Hull| {
        // (Its corners and its middle, and places that are neither
        // on its diagonals nor on its middle lines: two vaults crossed
        // over one square share all of the first five.)
        let corners = [
            x.at(0, 0),
            x.at(x.n[0] - 1, 0),
            x.at(0, x.n[1] - 1),
            x.at(x.n[0] - 1, x.n[1] - 1),
            x.eval([0.5, 0.5]),
            x.eval([0.31, 0.17]),
            x.eval([0.73, 0.38]),
            x.eval([0.22, 0.81]),
        ];
        corners
            .into_iter()
            .all(|p| hy.holds(p, sy, y.middle(), tol))
    };
    if depth.is_multiple_of(3) && (on(a, sb, b, hulls.1) || on(b, sa, a, hulls.0)) {
        return;
    }
    if depth >= 9 {
        out.push((
            Point2::from_array(a.middle()),
            Point2::from_array(b.middle()),
        ));
        return;
    }
    if held_a.size >= held_b.size {
        for half in held_a.halves() {
            touches_of(sa, &half, sb, held_b, hulls, tol, depth + 1, out);
        }
    } else {
        for half in held_b.halves() {
            touches_of(sa, held_a, sb, &half, hulls, tol, depth + 1, out);
        }
    }
}
