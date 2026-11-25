//! The views of a sheet: what each sees of the model, and how that is
//! drawn (ISO 128).

use std::sync::Arc;

use crate::Sheet;
use crate::sheet::{Beside, HalfSide, Projection, SheetView, Style, TangentEdges, ViewKind};
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::{Cutting, World};

use super::notes::Outline;
use super::{Fill, Kept, Laid, LaidView, Pen, Subject, millimetres, scale_text};
use crate::quick::{Shape, hidden, shape};

/// What a view sees of a model, in the view's own coordinates: the
/// model's lengths, across and up from where the model's middle is
/// seen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    /// Its lines — the edges, and the outlines of round faces — each
    /// with whether the model hides it, and whether it is a smooth
    /// (tangent) edge, where faces go on into each other.
    pub lines: Vec<(Point2, Point2, bool, bool)>,
    /// Its faces as triangles, the farthest first, each with how
    /// brightly it is lit (0 to 1).
    pub faces: Vec<([Point2; 3], f64)>,
    /// Where a section cuts the model: triangles, to be hatched, each
    /// with the how manyth body it is of (each body is hatched its own
    /// way).
    pub cut: Vec<([Point2; 3], usize)>,
    /// The circles seen square on: middle and radius.
    pub rounds: Vec<(Point2, f64)>,
    /// The axes of what is round and seen from the side: from one end
    /// to the other.
    pub axes: Vec<(Point2, Point2)>,
    /// The threads that are said of the model, not cut into it.
    pub threads: super::threads::Threads,
    /// Of a section: how many bodies reach on into what the cut takes
    /// away — bodies the cut could not be made in, drawn whole.
    pub uncut: usize,
    /// The lower left and the upper right of all of it.
    pub lo: Point2,
    /// See `lo`.
    pub hi: Point2,
}

/// Of each view (by its name), the traces of datums it could draw.
type Traces = std::collections::BTreeMap<String, Vec<(Point2, Point2, super::Pen)>>;

thread_local! {
    /// Of the sheet being laid out, the traces of datums each view (by
    /// its name) could draw, on the sheet: what a dimension's extension
    /// line may start on (see `lines::traces`).
    pub(crate) static TRACES: std::cell::RefCell<Traces> = Default::default();
}

/// A view where it stands: what it sees, how it looks at the model,
/// and how what it sees comes onto the sheet.
pub(crate) struct Placed<'a> {
    pub(crate) view: &'a SheetView,
    pub(crate) seen: Arc<Seen>,
    /// Across, up, and towards who looks: unit vectors of the model.
    pub(crate) basis: [Point3; 3],
    /// The model's middle: what the view's own coordinates are from.
    pub(crate) middle: Point3,
    /// Where the point `origin` of the view's own coordinates is on the
    /// sheet.
    pub(crate) centre: Point2,
    /// See `centre`.
    pub(crate) origin: Point2,
    /// How many millimetres of the sheet one length of the model is.
    pub(crate) scale: f64,
    /// Of a detail: how far around `origin` it shows.
    pub(crate) reach: Option<f64>,
    /// Of a section: where it cuts.
    pub(crate) cutting: Option<Cutting>,
    /// How the sheet draws smooth (tangent) edges.
    pub(crate) tangent: TangentEdges,
    /// Of a half section, what the view sees of the model uncut: what
    /// its other half shows.
    pub(crate) outside: Option<Arc<Seen>>,
}

impl Placed<'_> {
    /// Where the point `p` of the view's own coordinates is on the
    /// sheet.
    pub(crate) fn sheet(&self, p: Point2) -> Point2 {
        self.centre + (p - self.origin) * self.scale
    }

    /// Where the point `p` of the model is in the view's own
    /// coordinates.
    pub(crate) fn flat(&self, p: Point3) -> Point2 {
        let d = p - self.middle;
        Point2::new(d.dot(self.basis[0]), d.dot(self.basis[1]))
    }

    /// Where the point `p` of the model is on the sheet.
    pub(crate) fn place(&self, p: Point3) -> Point2 {
        self.sheet(self.flat(p))
    }

    /// The direction `d` of the model as the view sees it: across and
    /// up, in the model's lengths.
    pub(crate) fn way(&self, d: Point3) -> Point2 {
        Point2::new(d.dot(self.basis[0]), d.dot(self.basis[1]))
    }

    /// The lower left and upper right of what it shows, on the sheet.
    pub(crate) fn bounds(&self) -> (Point2, Point2) {
        match self.reach {
            Some(reach) => {
                let r = Point2::splat(reach * self.scale);
                (self.centre - r, self.centre + r)
            }
            None => {
                let (lo, hi) = self.shown();
                (self.sheet(lo), self.sheet(hi))
            }
        }
    }

    /// Where an extension line from `p` (on the sheet) running on to
    /// `foot` starts on what the view draws (see [`Outline`]). Of the
    /// lines along the extension line — a face seen edge on — those
    /// that run on into one another make one piece of outline: of the
    /// piece `p` is on (or the nearest), the point of what is drawn of
    /// it nearest `foot`; hidden, if none of it is drawn (an edge that
    /// lines up with a line drawn farther on is not that line). With no
    /// such lines, `p` itself if a drawn line runs through it.
    pub(crate) fn outline(&self, p: Point2, foot: Point2) -> Outline {
        let style = self.view.style;
        let near = 0.02;
        let reach = p.distance(foot);
        let way = (foot - p).try_normalize();
        let tangent = self.tangent;
        // (Of a section of only its cut, the cut's outline.)
        let cut_outline: Vec<(Point2, Point2, bool, bool)> = if self.only_cut() {
            self.cut_outline()
                .into_iter()
                .map(|(a, b)| (a, b, false, false))
                .collect()
        } else {
            Vec::new()
        };
        let own: &[(Point2, Point2, bool, bool)] = if self.only_cut() {
            &cut_outline
        } else {
            &self.seen.lines
        };
        let lines = own.iter().copied().chain(
            // (The centre lines, which are drawn.)
            self.seen.axes.iter().map(|(a, b)| (*a, *b, false, false)),
        );
        // (Along the extension line: from, to, whether drawn.)
        let mut along: Vec<(f64, f64, bool)> = Vec::new();
        let (mut drawn_through, mut hidden_through) = (false, false);
        for (a, b, behind, smooth) in lines {
            let drawn = (!behind || matches!(style, Style::HiddenDashed | Style::Wireframe))
                && !(smooth && (behind || tangent == TangentEdges::Hidden));
            let clipped = match self.reach {
                Some(r) => within(a, b, self.origin, r),
                None => Some((a, b)),
            };
            let Some((a, b)) = clipped else {
                continue;
            };
            let (a, b) = (self.sheet(a), self.sheet(b));
            if let Some(w) = way
                && (a - p).perp_dot(w).abs() <= near
                && (b - p).perp_dot(w).abs() <= near
            {
                let (s, t) = ((a - p).dot(w), (b - p).dot(w));
                along.push((s.min(t), s.max(t), drawn));
                continue;
            }
            let d = b - a;
            let t = ((p - a).dot(d) / d.length_squared().max(1e-300)).clamp(0.0, 1.0);
            if (a + d * t).distance(p) <= near {
                if drawn {
                    drawn_through = true;
                } else {
                    hidden_through = true;
                }
            }
        }
        // (The traces of datums the view draws where dimensions end on
        // them: an extension line runs on from a trace's end.)
        if let Some(w) = way {
            TRACES.with(|t| {
                for (a, b, _) in t.borrow().get(&self.view.id).into_iter().flatten() {
                    if (*a - p).perp_dot(w).abs() <= near && (*b - p).perp_dot(w).abs() <= near {
                        let (s, t) = ((*a - p).dot(w), (*b - p).dot(w));
                        along.push((s.min(t), s.max(t), true));
                    }
                }
            });
        }
        if let (Some(w), false) = (way, along.is_empty()) {
            // The pieces they make, and the one `p` is on or nearest.
            along.sort_by(|x, y| x.0.total_cmp(&y.0));
            let join = 0.3;
            let mut pieces: Vec<Vec<(f64, f64, bool)>> = Vec::new();
            for line in along {
                match pieces.last_mut() {
                    Some(piece)
                        if line.0 <= piece.iter().map(|l| l.1).fold(f64::MIN, f64::max) + join =>
                    {
                        piece.push(line)
                    }
                    _ => pieces.push(vec![line]),
                }
            }
            let off = |piece: &Vec<(f64, f64, bool)>| {
                let lo = piece.iter().map(|l| l.0).fold(f64::MAX, f64::min);
                let hi = piece.iter().map(|l| l.1).fold(f64::MIN, f64::max);
                if lo <= near && hi >= -near {
                    0.0
                } else {
                    lo.abs().min(hi.abs())
                }
            };
            let piece = pieces
                .iter()
                .min_by(|x, y| off(x).total_cmp(&off(y)))
                .expect("not empty");
            let best = piece
                .iter()
                .filter(|l| l.2)
                .map(|l| reach.clamp(l.0, l.1))
                .min_by(|x, y| (reach - x).abs().total_cmp(&(reach - y).abs()));
            return match best {
                Some(t) => Outline::Drawn(p + w * t),
                None => Outline::Hidden,
            };
        }
        // (The middle of something round, drawn with its centre lines.)
        let centre = self
            .seen
            .rounds
            .iter()
            .any(|(c, _)| self.sheet(*c).distance(p) <= near)
            || self.round_about(p).is_some();
        if drawn_through || centre {
            Outline::Drawn(p)
        } else if hidden_through {
            Outline::Hidden
        } else {
            Outline::Free
        }
    }

    /// Whether the view draws something round about `p` (on the sheet):
    /// an arc — a bend, a round's quarter — seen square on, drawn as
    /// pieces of one radius about it; the largest such radius, on the
    /// sheet. (A whole circle seen square on is in `seen.rounds`.)
    pub(crate) fn round_about(&self, p: Point2) -> Option<f64> {
        use crate::sheet::{Style, TangentEdges};
        let style = self.view.style;
        // (Of each radius, how far around the pieces at it go.)
        let mut radii: Vec<(f64, f64)> = Vec::new();
        for (a, b, behind, smooth) in &self.seen.lines {
            let drawn = (!behind || matches!(style, Style::HiddenDashed | Style::Wireframe))
                && !(*smooth && (*behind || self.tangent == TangentEdges::Hidden));
            if !drawn {
                continue;
            }
            let (a, b) = (self.sheet(*a), self.sheet(*b));
            let (ra, rb) = (a.distance(p), b.distance(p));
            let r = (ra + rb) / 2.0;
            let length = a.distance(b);
            // (A short piece, both its ends as far from `p`, its middle
            // within its sag of that: a chord of the circle.)
            if r < 0.2 || length < 1e-9 || length > r * 0.6 || (ra - rb).abs() > r * 2e-3 {
                continue;
            }
            let sag = r - (r * r - length * length / 4.0).max(0.0).sqrt();
            if (a.lerp(b, 0.5).distance(p) - (r - sag)).abs() > r * 2e-3 + 1e-6 {
                continue;
            }
            match radii.iter_mut().find(|(x, _)| (*x - r).abs() <= r * 2e-3) {
                Some(at) => at.1 += length / r,
                None => radii.push((r, length / r)),
            }
        }
        // (Round enough to be an arc: some tens of degrees of it.)
        radii
            .into_iter()
            .filter(|(_, around)| *around >= 0.35)
            .map(|(r, _)| r)
            .reduce(f64::max)
    }

    /// Of a half section (or half view): a point of the line that
    /// divides its halves, on the sheet; the way (a unit vector) into
    /// the half not cut — drawn uncut, or not at all; and whether it
    /// is not drawn.
    pub(crate) fn halved(&self) -> Option<(Point2, Point2, bool)> {
        let (Some(half), Some(_)) = (self.view.half, &self.cutting) else {
            return None;
        };
        let out = match half.cut {
            HalfSide::Left => Point2::X,
            HalfSide::Right => -Point2::X,
            HalfSide::Above => -Point2::Y,
            HalfSide::Below => Point2::Y,
        };
        Some((self.sheet(self.divide()), out, half.only))
    }

    /// Of a half section, the point (of its own coordinates) on the
    /// line that divides its halves (ISO 128-3: a centre line): the
    /// axis of what is round and is cut through its axis, running the
    /// way the halves divide — of those, the nearest the view's middle
    /// — else the section's own origin.
    pub(crate) fn divide(&self) -> Point2 {
        let (Some(half), Some(cut)) = (self.view.half, &self.cutting) else {
            return Point2::ZERO;
        };
        let upright = matches!(half.cut, HalfSide::Left | HalfSide::Right);
        let reach = (self.seen.hi - self.seen.lo).length().max(1e-9);
        // (An axis of the cut model or, where the half is drawn uncut,
        // of the model whole: the same axes.)
        let axes = self
            .seen
            .axes
            .iter()
            .chain(self.outside.iter().flat_map(|o| o.axes.iter()));
        let mut best: Option<(f64, Point2)> = None;
        for (a, b) in axes {
            let d = (*b - *a).normalize_or_zero();
            let along = if upright { d.x.abs() } else { d.y.abs() };
            if along > 1e-6 || a.distance(*b) < reach * 0.05 {
                continue;
            }
            // (Where it is across the view, and how far that is from
            // the middle.)
            let off = if upright { a.x } else { a.y };
            if best.is_none_or(|(o, _)| off.abs() < o.abs() - reach * 1e-6) {
                best = Some((off, *a));
            }
        }
        if let Some((_, a)) = best {
            return a;
        }
        let [right, up, _] = self.basis;
        // (The section's origin, or where it is nearest on the first
        // piece of its line: the plane's origin is on its line.)
        let foot = match cut.path[..] {
            [p0, p1, ..] => {
                let d = (p1 - p0).normalize_or_zero();
                p0 + d * (-p0).dot(d)
            }
            _ => Point2::ZERO,
        };
        let d = cut.origin + cut.x * foot.x + cut.y * foot.y - self.middle;
        Point2::new(d.dot(right), d.dot(up))
    }

    /// The outline of its cut, in its own coordinates: the sides of the
    /// cut's triangles that only one of them has.
    pub(crate) fn cut_outline(&self) -> Vec<(Point2, Point2)> {
        let key = |p: Point2| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64);
        let mut sides: std::collections::BTreeMap<_, (Point2, Point2, usize)> = Default::default();
        for (corners, _) in &self.seen.cut {
            for k in 0..3 {
                let (a, b) = (corners[k], corners[(k + 1) % 3]);
                let (ka, kb) = (key(a), key(b));
                let side = if ka <= kb { (ka, kb) } else { (kb, ka) };
                sides.entry(side).or_insert((a, b, 0)).2 += 1;
            }
        }
        sides
            .into_values()
            .filter(|s| s.2 == 1)
            .map(|s| (s.0, s.1))
            .collect()
    }

    /// Whether it shows only what its cut meets.
    pub(crate) fn only_cut(&self) -> bool {
        self.view.cut_only && self.cutting.is_some() && !self.seen.cut.is_empty()
    }

    /// The lower left and upper right of what it shows, in its own
    /// coordinates: of a section that shows only its cut, the cut.
    pub(crate) fn shown(&self) -> (Point2, Point2) {
        if !self.only_cut() {
            // (Of a half section drawn as its cut half only, that half.)
            let (mut lo, mut hi) = (self.seen.lo, self.seen.hi);
            if let (Some(half), Some(_)) = (self.view.half.filter(|h| h.only), &self.cutting) {
                let at = self.divide();
                match half.cut {
                    HalfSide::Left => hi.x = hi.x.min(at.x),
                    HalfSide::Right => lo.x = lo.x.max(at.x),
                    HalfSide::Above => lo.y = lo.y.max(at.y),
                    HalfSide::Below => hi.y = hi.y.min(at.y),
                }
            }
            return (lo, hi);
        }
        self.seen.cut.iter().flat_map(|(corners, _)| corners).fold(
            (
                Point2::splat(f64::INFINITY),
                Point2::splat(f64::NEG_INFINITY),
            ),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    }
}

/// How near who looks the nearest of some triangles is, over a raster
/// of the view: a depth buffer.
struct Nearest {
    lo: Point2,
    /// Squares to a length of the model.
    per: f64,
    size: usize,
    depth: Vec<f64>,
}

impl Nearest {
    /// Squares across the larger side of the view.
    const SIZE: usize = 1024;

    /// Of `triangles`, as `flat` lays them out, nearness along `towards`.
    fn of(
        triangles: &[([Point3; 3], [Point3; 3])],
        flat: &dyn Fn(Point3) -> Point2,
        towards: Point3,
    ) -> Self {
        let (mut lo, mut hi) = (
            Point2::splat(f64::INFINITY),
            Point2::splat(f64::NEG_INFINITY),
        );
        for (p, _) in triangles {
            for q in p {
                let f = flat(*q);
                lo = lo.min(f);
                hi = hi.max(f);
            }
        }
        let size = Self::SIZE;
        let span = (hi - lo).max_element().max(1e-12);
        let per = (size - 1) as f64 / span;
        let mut depth = vec![f64::NEG_INFINITY; size * size];
        for (p, _) in triangles {
            let f = p.map(|q| (flat(q) - lo) * per);
            let z = p.map(|q| q.dot(towards));
            let area = (f[1] - f[0]).perp_dot(f[2] - f[0]);
            if area.abs() < 1e-12 {
                continue;
            }
            let (bl, bh) = (f[0].min(f[1]).min(f[2]), f[0].max(f[1]).max(f[2]));
            let (x0, x1) = (
                bl.x.floor().max(0.0) as usize,
                (bh.x.ceil() as usize).min(size - 1),
            );
            let (y0, y1) = (
                bl.y.floor().max(0.0) as usize,
                (bh.y.ceil() as usize).min(size - 1),
            );
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = Point2::new(x as f64, y as f64);
                    let w0 = (f[2] - f[1]).perp_dot(c - f[1]) / area;
                    let w1 = (f[0] - f[2]).perp_dot(c - f[2]) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                        continue;
                    }
                    let d = w0 * z[0] + w1 * z[1] + w2 * z[2];
                    let at = &mut depth[y * size + x];
                    if d > *at {
                        *at = d;
                    }
                }
            }
        }
        Self {
            lo,
            per,
            size,
            depth,
        }
    }

    /// Whether a point at `p` of the view, `z` near, is as near as the
    /// nearest there (within `slack`): in view. (About the nearest
    /// square, and those beside it: a raster's square is not a point.)
    fn in_front(&self, p: Point2, z: f64, slack: f64) -> bool {
        let q = (p - self.lo) * self.per;
        let (x, y) = (q.x.round() as i64, q.y.round() as i64);
        let mut nearest = f64::NEG_INFINITY;
        for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (x, y) = (x + dx, y + dy);
            if x < 0 || y < 0 || x >= self.size as i64 || y >= self.size as i64 {
                continue;
            }
            let d = self.depth[y as usize * self.size + x as usize];
            if d.is_finite() {
                nearest = if nearest.is_finite() {
                    nearest.min(d)
                } else {
                    d
                };
            }
        }
        !nearest.is_finite() || z >= nearest - slack
    }
}

/// What is seen of `world` (cut open along `cut`) looking with `basis`
/// from the middle `middle`.
/// How far off the line of the view it is of a section view may be put
/// and still be drawn in line with it, in millimetres on the sheet:
/// farther off, it stands where it is put.
const IN_LINE: f64 = 15.0;

/// A side of a triangle, by its two corners (rounded), whichever way.
type EdgeKey = ([i64; 3], [i64; 3]);

/// A side of a triangle as the triangle has it: its corners, the
/// normals there (in the order of its key), and whether the triangle
/// looks at who looks beside it (1) or away (-1).
type SideSeen = (Point3, Point3, [Point3; 2], i32);

/// How little a face may look at who looks, or away, and still be
/// seen edge on there: the noise of a normal square to the view.
const GRAZED: f64 = 1e-9;

/// How far apart (in radians) the normals of two triangles may be where
/// they meet for the face to go on smoothly there: the triangles of a
/// seam, which meet with normals a hair apart, not those of an edge.
const SMOOTH: f64 = 1e-2;

/// Draws round what of `lines` runs along a circle in short straight
/// pieces — the outline of a round face (a ring seen along its axis),
/// found where its flat triangles turn away, a polygon of some tens of
/// sides — each piece as the arc between its ends: where four or more
/// pieces run on into one another, all their corners on one circle.
fn round_off(lines: &mut Vec<(Point2, Point2, bool, bool)>, reach: f64) {
    let near = reach * 1e-7;
    let key = |p: Point2| ((p.x / near).round() as i64, (p.y / near).round() as i64);
    let mut ends: std::collections::HashMap<(i64, i64), Vec<usize>> = Default::default();
    for (k, (a, b, _, _)) in lines.iter().enumerate() {
        ends.entry(key(*a)).or_default().push(k);
        ends.entry(key(*b)).or_default().push(k);
    }
    // (Each run: pieces joined end to end, two at each corner.)
    let mut used = vec![false; lines.len()];
    let mut arcs: Vec<(usize, Point2, f64)> = Vec::new();
    for first in 0..lines.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut run = vec![first];
        let mut corners = vec![lines[first].0, lines[first].1];
        for forward in [true, false] {
            loop {
                let tip = if forward {
                    corners[corners.len() - 1]
                } else {
                    corners[0]
                };
                let at = ends.get(&key(tip)).map_or(&[][..], Vec::as_slice);
                if at.len() != 2 {
                    break;
                }
                let Some(&next) = at.iter().find(|k| !used[**k]) else {
                    break;
                };
                if lines[next].2 != lines[first].2 {
                    break;
                }
                let (a, b, _, _) = lines[next];
                let far = if key(a) == key(tip) { b } else { a };
                // (A polygon of a circle turns a little at each corner:
                // a line that meets it at a sharp turn — a straight
                // side running from rim to rim, a chord with both ends
                // on the circle — is not of it.)
                let before = if forward {
                    tip - corners[corners.len() - 2]
                } else {
                    corners[1] - tip
                };
                let after = if forward { far - tip } else { tip - far };
                if before.angle_to(after).abs() > 0.6 {
                    break;
                }
                used[next] = true;
                if forward {
                    run.push(next);
                    corners.push(far);
                } else {
                    run.insert(0, next);
                    corners.insert(0, far);
                }
            }
        }
        if run.len() < 4 {
            continue;
        }
        // The circle through its first, middle and last corner.
        let (p, q, r) = (
            corners[0],
            corners[corners.len() / 2],
            corners[corners.len() - 1],
        );
        let (b, c) = (q - p, r - p);
        let d = 2.0 * b.perp_dot(c);
        if d.abs() < 1e-300 {
            continue;
        }
        let centre = p + Point2::new(
            c.y * b.length_squared() - b.y * c.length_squared(),
            b.x * c.length_squared() - c.x * b.length_squared(),
        ) / d;
        let radius = centre.distance(p);
        if radius > reach * 10.0 || radius < reach * 1e-4 {
            continue;
        }
        let tol = radius * 2e-3 + reach * 1e-6;
        // (Each piece a short chord of it: a long one — a straight edge
        // from rim to rim, a flat cut across a round top — is no side
        // of a polygon drawn for the circle.)
        if corners
            .iter()
            .all(|x| (x.distance(centre) - radius).abs() <= tol)
            && corners
                .windows(2)
                .all(|w| w[0].distance(w[1]) <= radius * 0.6)
        {
            arcs.extend(run.into_iter().map(|k| (k, centre, radius)));
        }
    }
    if arcs.is_empty() {
        return;
    }
    // (The pieces of those circles in runs too short to tell by
    // themselves — a ring's outline broken where the wires across it
    // meet it, a few sides between each two: each a short chord of one
    // of them, its ends on it.)
    let mut circles: Vec<(Point2, f64)> = Vec::new();
    for (_, c, r) in &arcs {
        if !circles
            .iter()
            .any(|(d, s)| d.distance(*c) <= r * 1e-3 && (s - r).abs() <= r * 1e-3)
        {
            circles.push((*c, *r));
        }
    }
    let rounded: std::collections::HashSet<usize> = arcs.iter().map(|(k, _, _)| *k).collect();
    for (k, (a, b, _, _)) in lines.iter().enumerate() {
        if rounded.contains(&k) {
            continue;
        }
        let length = a.distance(*b);
        if let Some((c, r)) = circles.iter().copied().find(|(c, r)| {
            let tol = r * 2e-3 + reach * 1e-6;
            length > 0.0
                && length <= r * 0.3
                && [a, b].iter().all(|p| (p.distance(*c) - r).abs() <= tol)
        }) {
            arcs.push((k, c, r));
        }
    }
    let mut out = Vec::with_capacity(lines.len() + arcs.len() * 4);
    let mut of: std::collections::HashMap<usize, (Point2, f64)> =
        arcs.into_iter().map(|(k, c, r)| (k, (c, r))).collect();
    for (k, line) in lines.iter().enumerate() {
        let Some((c, r)) = of.remove(&k) else {
            out.push(*line);
            continue;
        };
        let (a, b, behind, smooth) = *line;
        let (s, mut e) = ((a - c).y.atan2((a - c).x), (b - c).y.atan2((b - c).x));
        while e - s > std::f64::consts::PI {
            e -= std::f64::consts::TAU;
        }
        while s - e > std::f64::consts::PI {
            e += std::f64::consts::TAU;
        }
        // (Pieces of about a degree.)
        let n = (((e - s).abs() / 0.017).ceil() as usize).clamp(1, 32);
        let at = |t: usize| match t {
            0 => a,
            t if t == n => b,
            t => c + Point2::from_angle(s + (e - s) * t as f64 / n as f64) * r,
        };
        for t in 0..n {
            out.push((at(t), at(t + 1), behind, smooth));
        }
    }
    *lines = out;
}

/// Takes out of `lines` the specks: a bit of a line in view, shorter
/// all told than a few thousandths of `reach`, that touches no other line
/// in view — what the faces' flat pieces let through of an edge that is
/// hidden all but there (an edge far behind, seen past the rim of a
/// bore), drawn as a dot.
fn specks_out(lines: &mut Vec<(Point2, Point2, bool, bool)>, reach: f64) {
    let near = reach * 1e-6;
    let short = reach * 3e-3;
    let shown: Vec<usize> = (0..lines.len()).filter(|k| !lines[*k].2).collect();
    let key = |p: Point2| ((p.x / near).round() as i64, (p.y / near).round() as i64);
    let mut ends: std::collections::HashMap<(i64, i64), Vec<usize>> = Default::default();
    for &k in &shown {
        ends.entry(key(lines[k].0)).or_default().push(k);
        ends.entry(key(lines[k].1)).or_default().push(k);
    }
    let mut group: std::collections::HashMap<usize, usize> = Default::default();
    let mut gone = vec![false; lines.len()];
    for &first in &shown {
        if group.contains_key(&first) {
            continue;
        }
        // (The lines joined to it end to end.)
        let mut members = vec![first];
        group.insert(first, first);
        let mut k = 0;
        while k < members.len() {
            let (a, b, _, _) = lines[members[k]];
            for end in [a, b] {
                for &other in ends.get(&key(end)).into_iter().flatten() {
                    if group.insert(other, first).is_none() {
                        members.push(other);
                    }
                }
            }
            k += 1;
        }
        let length: f64 = members
            .iter()
            .map(|k| lines[*k].0.distance(lines[*k].1))
            .sum();
        if length >= short {
            continue;
        }
        // (Nor where it ends on another line in view.)
        let touches = members.iter().any(|m| {
            let (a, b, _, _) = lines[*m];
            [a, b].iter().any(|p| {
                shown.iter().any(|o| {
                    !members.contains(o) && {
                        let (c, d, _, _) = lines[*o];
                        let cd = d - c;
                        let t =
                            ((*p - c).dot(cd) / cd.length_squared().max(1e-300)).clamp(0.0, 1.0);
                        p.distance(c + cd * t) <= near * 10.0
                    }
                })
            })
        });
        if !touches {
            for m in members {
                gone[m] = true;
            }
        }
    }
    let mut k = 0;
    lines.retain(|_| {
        k += 1;
        !gone[k - 1]
    });
}

/// `lines` (each with whether it is hidden), each stretch of them drawn
/// once: where lines lie over each other — the near and the far edge
/// of a face seen edge on — what is in view is drawn, and of what is
/// hidden only what is not under that. Lines are over each other when
/// they are within `near` of each other.
fn once(lines: Vec<(Point2, Point2, bool, bool)>, near: f64) -> Vec<(Point2, Point2, bool, bool)> {
    if lines.is_empty() {
        return lines;
    }
    let (mut lo, mut hi) = (
        Point2::splat(f64::INFINITY),
        Point2::splat(f64::NEG_INFINITY),
    );
    for (a, b, _, _) in &lines {
        lo = lo.min(a.min(*b));
        hi = hi.max(a.max(*b));
    }
    // (What is drawn so far, by where it is: in squares.)
    const SQUARES: usize = 64;
    let span = (hi - lo).max(Point2::splat(near.max(1e-12)));
    let square = |p: Point2| {
        let at = (p - lo) / span * SQUARES as f64;
        let clamp = |x: f64| (x.max(0.0) as usize).min(SQUARES - 1);
        (clamp(at.x), clamp(at.y))
    };
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); SQUARES * SQUARES];
    let mut kept: Vec<(Point2, Point2, bool, bool)> = Vec::with_capacity(lines.len());
    // In view first, then hidden; edges before smooth edges (an outline
    // over a tangent edge is the outline); each in the order it came.
    let order = [(false, false), (false, true), (true, false), (true, true)]
        .into_iter()
        .flat_map(|kind| lines.iter().filter(move |l| (l.2, l.3) == kind));
    for &(a, b, behind, smooth) in order {
        let length = a.distance(b);
        if length <= near {
            kept.push((a, b, behind, smooth));
            continue;
        }
        let way = (b - a) / length;
        // The shares of it that lie over what is drawn already.
        let mut over: Vec<(f64, f64)> = Vec::new();
        let (sa, sb) = (
            square(a.min(b) - Point2::splat(near)),
            square(a.max(b) + Point2::splat(near)),
        );
        let mut seen_before = std::collections::BTreeSet::new();
        for i in sa.0..=sb.0 {
            for j in sa.1..=sb.1 {
                for &k in &grid[i * SQUARES + j] {
                    if !seen_before.insert(k) {
                        continue;
                    }
                    let (p, q, _, _) = kept[k];
                    // (Over it only along it: both its ends on this
                    // line.)
                    let off = |x: Point2| (x - a).perp_dot(way).abs();
                    if off(p) > near || off(q) > near {
                        continue;
                    }
                    let (s, t) = ((p - a).dot(way), (q - a).dot(way));
                    let (s, t) = (s.min(t).max(0.0), s.max(t).min(length));
                    if t - s > near {
                        over.push((s, t));
                    }
                }
            }
        }
        over.sort_by(|x, y| x.0.total_cmp(&y.0));
        // What is left of it between those.
        let mut from = 0.0;
        let mut left = Vec::new();
        for (s, t) in over {
            if s - from > near {
                left.push((from, s));
            }
            from = f64::max(from, t);
        }
        if length - from > near {
            left.push((from, length));
        }
        for (s, t) in left {
            let (p, q) = (a + way * s, a + way * t);
            let k = kept.len();
            kept.push((p, q, behind, smooth));
            let (lo, hi) = (square(p.min(q)), square(p.max(q)));
            for i in lo.0..=hi.0 {
                for j in lo.1..=hi.1 {
                    grid[i * SQUARES + j].push(k);
                }
            }
        }
    }
    kept
}

/// Of `lines` (each with whether it is hidden), in the order they run,
/// what is hidden or in view for a short stretch only — less than
/// `short` long — between pieces of one line that are all the other
/// way, taken to be that way too: the noise of telling what hides a
/// line that grazes a face (a rim along a face seen edge on), which
/// would break it into dashes or leave gaps in it.
fn steady(lines: &mut [(Point2, Point2, bool, bool)], join: f64, short: f64) {
    let n = lines.len();
    let mut k = 0;
    while k < n {
        // (A stretch of pieces joined end to end, the same way.)
        let mut end = k + 1;
        while end < n
            && lines[end].2 == lines[k].2
            && lines[end - 1].1.distance(lines[end].0) <= join
        {
            end += 1;
        }
        let length: f64 = lines[k..end].iter().map(|l| l.0.distance(l.1)).sum();
        let before = k > 0 && lines[k - 1].1.distance(lines[k].0) <= join;
        let after = end < n && lines[end - 1].1.distance(lines[end].0) <= join;
        if before && after && length < short && lines[k - 1].2 == lines[end].2 {
            let way = lines[end].2;
            for line in &mut lines[k..end] {
                line.2 = way;
            }
        }
        k = end;
    }
}

/// Of `sides` (each of a triangle, its corners, its normals there,
/// and which way the triangle looks), where two that lie along each
/// other — not end to end, but one over a part of the other — look
/// different ways and have the face go on smoothly between them: what
/// they share, as a piece of an outline (its ends, and from where it is
/// looked at for what hides it).
fn turns_along(
    sides: &[SideSeen],
    reach: f64,
    smooth: &dyn Fn(Point3, Point3) -> bool,
) -> Vec<(Point3, Point3, Point3)> {
    let mut out = Vec::new();
    if sides.len() < 2 {
        return out;
    }
    // (By where they are: in cubes as big as the longest of them.)
    let cube = sides
        .iter()
        .map(|s| s.0.distance(s.1))
        .fold(reach * 1e-6, f64::max);
    let at = |p: Point3| (p / cube).floor().to_array().map(|x| x as i64);
    let mut cubes: std::collections::HashMap<[i64; 3], Vec<usize>> = Default::default();
    for (k, s) in sides.iter().enumerate() {
        cubes.entry(at((s.0 + s.1) / 2.0)).or_default().push(k);
    }
    let near = reach * 1e-7;
    for (k, u) in sides.iter().enumerate() {
        let length = u.0.distance(u.1);
        if length <= near {
            continue;
        }
        let way = (u.1 - u.0) / length;
        let normal_at = |s: f64| u.2[0].lerp(u.2[1], s);
        let [x, y, z] = at((u.0 + u.1) / 2.0);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(those) = cubes.get(&[x + dx, y + dy, z + dz]) else {
                        continue;
                    };
                    for &j in those {
                        let v = &sides[j];
                        // (Each pair once: the one that looks at who
                        // looks first.)
                        if j == k || u.3 != 1 || v.3 != -1 {
                            continue;
                        }
                        // (Along it as nearly as the faces are flat:
                        // two faces that meet there without an edge
                        // are each sampled at their own points, and
                        // their sides are chords of the one outline,
                        // a little apart.)
                        let within = near.max(length.max(v.0.distance(v.1)) * 3e-2);
                        let off = |p: Point3| (p - u.0).reject_from_normalized(way).length();
                        if off(v.0) > within || off(v.1) > within {
                            continue;
                        }
                        let (s, t) = ((v.0 - u.0).dot(way) / length, (v.1 - u.0).dot(way) / length);
                        let (lo, hi) = (s.min(t).max(0.0), s.max(t).min(1.0));
                        if (hi - lo) * length <= near {
                            continue;
                        }
                        let mid = (lo + hi) / 2.0;
                        let along_v = if (t - s).abs() > 1e-300 {
                            (mid - s) / (t - s)
                        } else {
                            0.5
                        };
                        let normal_v = v.2[0].lerp(v.2[1], along_v);
                        if !smooth(normal_at(mid), normal_v) {
                            continue;
                        }
                        let (a, b) = (u.0 + way * (lo * length), u.0 + way * (hi * length));
                        let outward = (normal_at(mid) + normal_v).normalize_or_zero();
                        out.push((a, b, (a + b) / 2.0 + outward * (reach * 4e-3)));
                    }
                }
            }
        }
    }
    out
}

fn see(
    world: &World,
    basis: [Point3; 3],
    middle: Point3,
    cut: Option<&Cutting>,
    shaded: bool,
) -> Seen {
    let [right, up, towards] = basis;
    let mut shape_of = Shape::default();
    let said = &world.threads;
    let opened;
    let world = match cut {
        Some(cut) => {
            let far = world_reach(world) * 4.0 + cut.origin.length() + 1.0;
            opened = crate::section::opened(world, cut, far);
            &opened
        }
        None => world,
    };
    shape(world, shapecraft_core::Pose::IDENTITY, &mut shape_of);
    let flat = |p: Point3| {
        let d = p - middle;
        Point2::new(d.dot(right), d.dot(up))
    };
    // (Of the faces too: a ball has no edge to measure it by.)
    let reach = shape_of
        .runs
        .iter()
        .flatten()
        .chain(shape_of.triangles.iter().flat_map(|t| &t.0))
        .fold(1e-9_f64, |m, p| m.max(p.distance(middle)));
    // Each piece of a line, and from where it is looked for whether
    // something hides it: its middle — of an outline, a little off the
    // face it is of, which else hides it from itself.
    let mut pieces: Vec<(Point3, Point3, Point3)> = Vec::new();
    // (Of the edges, which are smooth: those first in `pieces`.)
    let mut smooth_pieces: Vec<bool> = Vec::new();
    for (k, run) in shape_of.runs.iter().enumerate() {
        // (Each looked at for what hides it from a little out of the
        // body, off the faces it is the edge of: from on it, those faces
        // — seen edge on, or nearly — would hide it from itself.)
        let outward = shape_of.outward.get(k);
        pieces.extend(run.windows(2).enumerate().map(|(j, w)| {
            let out = outward
                .and_then(|o| Some((*o.get(j)? + *o.get(j + 1)?).normalize_or_zero()))
                .unwrap_or(Point3::ZERO);
            (w[0], w[1], (w[0] + w[1]) / 2.0 + out * (reach * 4e-3))
        }));
        // (Not where a face on it is seen edge on: there it is the
        // outline, where the round turns away.)
        let edge_on = shape_of
            .sides
            .get(k)
            .is_some_and(|n| n.iter().any(|n| n.dot(towards).abs() < 1e-6));
        let smooth = shape_of.smooth.get(k).copied().unwrap_or(false) && !edge_on;
        smooth_pieces.resize(pieces.len(), smooth);
    }
    // Each side of a triangle that could be an outline (one that is
    // nearly seen edge on), as each triangle on it has it.
    let mut sides: std::collections::BTreeMap<EdgeKey, Vec<SideSeen>> = Default::default();
    let key = |q: Point3| (q / (reach * 1e-8)).round().to_array().map(|x| x as i64);
    // The outlines of round faces: where they turn away — between
    // where a face looks at who looks and where it looks away, not
    // where it is only seen edge on (a wall seen from above, which is
    // its edges; the noise of its normals would draw it as a zigzag).
    for (p, n) in &shape_of.triangles {
        let d = n.map(|n| n.dot(towards));
        let side: [i32; 3] = d.map(|d| {
            if d > GRAZED {
                1
            } else if d < -GRAZED {
                -1
            } else {
                0
            }
        });
        // (Where it turns away along a side of the triangles rather
        // than through one — a cylinder meshed with corners right on its
        // outline, or its seam there, the corners of which are twice,
        // with normals a hair apart: which way each triangle beside the
        // side looks.)
        for k in 0..3 {
            let (i, j) = ((k + 1) % 3, (k + 2) % 3);
            if side[i] * side[j] < 0 || d[i].abs() > SMOOTH || d[j].abs() > SMOOTH {
                continue;
            }
            let looks = match side[i] + side[j] {
                0 => side[k],
                s => s.signum(),
            };
            if looks == 0 {
                continue;
            }
            let (ki, kj) = (key(p[i]), key(p[j]));
            let (edge, normals) = if ki <= kj {
                ((ki, kj), [n[i], n[j]])
            } else {
                ((kj, ki), [n[j], n[i]])
            };
            sides
                .entry(edge)
                .or_default()
                .push((p[i], p[j], normals, looks));
        }
        if !(side.contains(&1) && side.contains(&-1)) {
            continue;
        }
        let mut ends = Vec::new();
        let mut out = Point3::ZERO;
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            if side[i] * side[j] < 0 {
                let f = d[i] / (d[i] - d[j]);
                ends.push(p[i].lerp(p[j], f));
                out += n[i].lerp(n[j], f);
            }
        }
        // (Through a corner that is seen edge on: there.)
        if let Some(k) = side.iter().position(|s| *s == 0) {
            ends.push(p[k]);
            out += n[k];
        }
        if let [a, b] = ends[..] {
            let out = out.normalize_or_zero();
            pieces.push((a, b, (a + b) / 2.0 + out * (reach * 4e-3)));
        }
    }
    // A side is an outline where a triangle on it looks at who looks
    // and one looks away, and the face goes on smoothly over it from
    // the one to the other (else it is an edge of the model, drawn as
    // that). Once, however many triangles meet there.
    let smooth = |m: Point3, n: Point3| {
        m.normalize_or_zero().dot(n.normalize_or_zero()) >= (SMOOTH * SMOOTH).mul_add(-0.5, 1.0)
    };
    // (Those of a side that no triangle on the other side has whole —
    // the triangles there split it, or have it as a part of a longer
    // side — each with its normals in the order of its corners.)
    let mut alone: Vec<SideSeen> = Vec::new();
    for beside in sides.into_values() {
        let turn = beside.iter().enumerate().find_map(|(x, u)| {
            beside[x + 1..]
                .iter()
                .find(|v| u.3 != v.3 && smooth(u.2[0], v.2[0]) && smooth(u.2[1], v.2[1]))
                .map(|v| (u, v))
        });
        if let Some((u, v)) = turn {
            let out = (u.2[0] + u.2[1] + v.2[0] + v.2[1]).normalize_or_zero();
            pieces.push((u.0, u.1, (u.0 + u.1) / 2.0 + out * (reach * 4e-3)));
        } else if beside.iter().all(|u| u.3 == beside[0].3) {
            alone.extend(beside.iter().map(|&(a, b, n, looks)| {
                let n = if key(a) <= key(b) { n } else { [n[1], n[0]] };
                (a, b, n, looks)
            }));
        }
    }
    // Where such sides lie along each other, one looking at who looks
    // and the other away, the face turns away over what they share.
    pieces.extend(turns_along(&alone, reach, &smooth));
    let mut seen = Seen {
        lo: Point2::splat(f64::INFINITY),
        hi: Point2::splat(f64::NEG_INFINITY),
        ..Seen::default()
    };
    // The triangles by where they are seen, in a grid: what could hide
    // a point is what is seen in the same square.
    const SQUARES: usize = 96;
    let (mut glo, mut ghi) = (
        Point2::splat(f64::INFINITY),
        Point2::splat(f64::NEG_INFINITY),
    );
    for (p, _) in &shape_of.triangles {
        for corner in p {
            glo = glo.min(flat(*corner));
            ghi = ghi.max(flat(*corner));
        }
    }
    let span = (ghi - glo).max(Point2::splat(1e-12));
    let square = |p: Point2| {
        let at = (p - glo) / span * SQUARES as f64;
        let clamp = |x: f64| (x.max(0.0) as usize).min(SQUARES - 1);
        (clamp(at.x), clamp(at.y))
    };
    let mut grid: Vec<Vec<([Point3; 3], [Point3; 3])>> = vec![Vec::new(); SQUARES * SQUARES];
    for triangle in &shape_of.triangles {
        let corners = triangle.0.map(flat);
        let lo = square(corners[0].min(corners[1]).min(corners[2]));
        let hi = square(corners[0].max(corners[1]).max(corners[2]));
        for i in lo.0..=hi.0 {
            for j in lo.1..=hi.1 {
                grid[i * SQUARES + j].push(*triangle);
            }
        }
    }
    // Of an offset section, what lies where the cut turns or steps
    // aside is not drawn (ISO 128-3): the cut shows as if it were one
    // plane.
    let at_a_turn = |p: Point3| {
        let Some(cut) = cut else {
            return false;
        };
        let near = reach * 1e-6;
        let d = p - cut.origin;
        let p = Point2::new(d.dot(cut.x), d.dot(cut.y));
        let last = cut.path.len() - 1;
        cut.path[1..last].iter().any(|c| c.distance(p) <= near)
            || cut.path.windows(2).enumerate().any(|(k, w)| {
                let along = (w[1] - w[0]).normalize_or_zero();
                let aside = (cut.x * along.x + cut.y * along.y).dot(towards).abs() > 1.0 - 1e-9;
                let s = (p - w[0]).dot(along);
                aside
                    && k > 0
                    && k < last - 1
                    && (p - w[0]).perp_dot(along).abs() <= near
                    && s >= -near
                    && s <= w[0].distance(w[1]) + near
            })
    };
    // (An outline looked at for what hides it a short stretch at a
    // time: a long piece — the side of a post meshed as one strip — is
    // partly behind something and partly not. An edge's pieces are
    // short already.)
    let stretch = reach * 1e-2;
    let pieces = pieces
        .into_iter()
        .enumerate()
        .flat_map(|(k, (a, b, from))| {
            let smooth = smooth_pieces.get(k).copied().unwrap_or(false);
            let off = from - (a + b) / 2.0;
            let n = if off == Point3::ZERO {
                1
            } else {
                ((a.distance(b) / stretch).ceil() as usize).clamp(1, 64)
            };
            (0..n).map(move |k| {
                let (s, t) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
                let (p, q) = (a.lerp(b, s), a.lerp(b, t));
                (p, q, (p + q) / 2.0 + off, smooth)
            })
        });
    let mut looked: Vec<(Point2, Point2, bool, bool)> = Vec::new();
    for (a, b, from, smooth) in pieces {
        if at_a_turn(a) && at_a_turn(b) {
            continue;
        }
        let (fa, fb) = (flat(a), flat(b));
        // (An edge seen end on is a point: not drawn.)
        if fa.distance(fb) <= reach * 1e-9 {
            continue;
        }
        // (What is nearer than the faces are flat to — they are drawn
        // of flat pieces — hides nothing: it is the face the line is
        // on.)
        let at = square(flat(from));
        let behind = hidden(&grid[at.0 * SQUARES + at.1], from, towards, reach * 2e-3);
        looked.push((fa, fb, behind, smooth));
    }
    steady(&mut looked, reach * 1e-9, reach * 2e-2);

    for (fa, fb, behind, smooth) in looked {
        seen.lo = seen.lo.min(fa).min(fb);
        seen.hi = seen.hi.max(fa).max(fb);
        // (A piece that runs straight on from the one before it, hidden
        // or in view as that is, is one line with it.)
        if let Some(last) = seen.lines.last_mut()
            && last.2 == behind
            && last.3 == smooth
            && last.1.distance(fa) <= reach * 1e-12
            && (last.1 - last.0).perp_dot(fb - fa).abs()
                <= 1e-9 * (last.1 - last.0).length() * (fb - fa).length()
            && (last.1 - last.0).dot(fb - fa) > 0.0
        {
            last.1 = fb;
            continue;
        }
        seen.lines.push((fa, fb, behind, smooth));
    }
    seen.lines = once(std::mem::take(&mut seen.lines), reach * 1e-6);

    specks_out(&mut seen.lines, reach);

    round_off(&mut seen.lines, reach);

    if seen.lines.is_empty() {
        seen.lo = Point2::ZERO;
        seen.hi = Point2::ZERO;
    }
    // Of a straight cut, the bodies that reach on into what it takes
    // away: where cutting them failed.
    if let Some(cut) = cut
        && let [p0, p1] = cut.path[..]
    {
        let d = (p1 - p0).normalize_or_zero();
        let mut whole = std::collections::BTreeSet::new();
        for ((corners, _), owner) in shape_of.triangles.iter().zip(&shape_of.owners) {
            let taken = corners.iter().any(|p| {
                let q = *p - cut.origin;
                let q = Point2::new(q.dot(cut.x), q.dot(cut.y));
                let s = (q - p0).dot(d);
                d.perp_dot(q - p0) > reach * 1e-4
                    && (!cut.bounded || (s > reach * 1e-4 && s < p0.distance(p1) - reach * 1e-4))
            });
            // (Not of a part a section leaves whole by intent: a shaft,
            // a screw cut along.)
            let (sectioned, at) = shape_of.sectioned[*owner];
            let back = at.inverse();
            let own = Cutting {
                origin: back.transform_point(cut.origin),
                x: back.transform_vector(cut.x),
                y: back.transform_vector(cut.y),
                ..cut.clone()
            };
            if taken && sectioned.cut_by(&own) {
                whole.insert(*owner);
            }
        }
        seen.uncut = whole.len();
    }
    // The faces that look at who looks, the farthest first: lit from
    // over the shoulder.
    let light = (towards + up * 0.5 - right * 0.3).normalize();
    // (Of a shaded view, how near who looks the nearest face is over
    // the view, at a fine raster: what tells a face behind from one in
    // front.)
    let nearest = shaded.then(|| Nearest::of(&shape_of.triangles, &flat, towards));
    let mut faces: Vec<(f64, [Point2; 3], f64)> = shape_of
        .triangles
        .iter()
        .filter(|(p, _)| shaded && (p[1] - p[0]).cross(p[2] - p[0]).length() > 0.0)
        // (Not one hidden all over — at its corners and its middle — by
        // others: drawn by how far off it is, a sliver of a face behind
        // would show through the face in front.)
        .filter(|(p, _)| {
            let Some(nearest) = &nearest else {
                return true;
            };
            let middle = (p[0] + p[1] + p[2]) / 3.0;
            [
                middle,
                p[0].lerp(middle, 0.1),
                p[1].lerp(middle, 0.1),
                p[2].lerp(middle, 0.1),
            ]
            .iter()
            .any(|q| nearest.in_front(flat(*q), q.dot(towards), reach * 5e-3))
        })
        .map(|(p, n)| {
            let normal = (n[0] + n[1] + n[2]).normalize_or_zero();
            // (Where the normals at its corners are far off how the
            // triangle itself lies — a corner where the face's normal is
            // ill-made, a seam — lit as it lies: else a light sliver on
            // a dark face.)
            let lies = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            let lies = if lies.dot(normal) < 0.0 { -lies } else { lies };
            let normal = if lies.dot(normal) < 0.85 {
                lies
            } else {
                normal
            };
            let normal = if normal.dot(towards) < 0.0 {
                -normal
            } else {
                normal
            };
            let depth = (p[0] + p[1] + p[2]).dot(towards) / 3.0;
            (depth, p.map(flat), 0.35 + 0.65 * normal.dot(light).max(0.0))
        })
        .collect();
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    seen.faces = faces.into_iter().map(|(_, p, lit)| (p, lit)).collect();
    // Where it is cut: the triangles that lie in a piece of the
    // cutting line, between that piece's ends (the first and the last
    // run on).
    if let Some(cut) = cut {
        let near = reach * 1e-7;
        let pieces = cut.path.len() - 1;
        let on = |p: Point3| {
            let d = p - cut.origin;
            let p = Point2::new(d.dot(cut.x), d.dot(cut.y));
            cut.path.windows(2).enumerate().any(|(k, w)| {
                let along = (w[1] - w[0]).normalize_or_zero();
                let s = (p - w[0]).dot(along);
                (p - w[0]).perp_dot(along).abs() <= near
                    && ((k == 0 && !cut.bounded) || s >= -near)
                    && ((k + 1 == pieces && !cut.bounded) || s <= w[0].distance(w[1]) + near)
            })
        };
        seen.cut = shape_of
            .triangles
            .iter()
            .zip(&shape_of.owners)
            .filter(|((p, _), _)| p.iter().all(|p| on(*p)))
            .map(|((p, _), owner)| (p.map(flat), *owner))
            .collect();
    }
    // What is round: square on a circle, from the side an axis. (Whole
    // circles, in one edge or in several: not the arcs of rounded
    // edges.)
    let mut whole: Vec<(Point3, Point3, f64, f64)> = Vec::new();
    for (at, axis, radius, around) in &shape_of.circles {
        let same = whole.iter_mut().find(|w| {
            w.0.distance(*at) <= reach * 1e-7
                && (w.2 - radius).abs() <= reach * 1e-7
                && w.1.cross(*axis).length() <= 1e-7
        });
        match same {
            Some(found) => found.3 += around,
            None => whole.push((*at, *axis, *radius, *around)),
        }
    }
    whole.retain(|w| w.3 >= std::f64::consts::TAU * (1.0 - 1e-6));
    let mut along: Vec<(Point3, Point3, f64, f64)> = Vec::new();
    for (at, axis, radius, _) in &whole {
        let facing = axis.dot(towards).abs();
        if facing > 1.0 - 1e-9 {
            let at = flat(*at);
            let known = seen
                .rounds
                .iter()
                .any(|r| r.0.distance(at) <= reach * 1e-7 && (r.1 - radius).abs() <= reach * 1e-7);
            if !known {
                seen.rounds.push((at, *radius));
            }
        } else if facing < 1e-9 {
            // (Circles about one line are of one axis: from the first
            // to the last of them.)
            let axis = if axis.dot(right + up * 0.5 + towards * 0.25) < 0.0 {
                -*axis
            } else {
                *axis
            };
            let s = (*at - middle).dot(axis);
            let foot = *at - axis * s;
            match along
                .iter_mut()
                .find(|a| a.1.cross(axis).length() <= 1e-9 && a.0.distance(foot) <= reach * 1e-7)
            {
                Some(found) => {
                    found.2 = found.2.min(s);
                    found.3 = found.3.max(s);
                }
                None => along.push((foot, axis, s, s)),
            }
        }
    }
    for (foot, axis, from, to) in along {
        if to - from > reach * 1e-7 {
            seen.axes
                .push((flat(foot + axis * from), flat(foot + axis * to)));
        }
    }
    // The threads that are only said of the model.
    if !said.is_empty() {
        let behind = |p: Point3| {
            let at = square(flat(p));
            hidden(&grid[at.0 * SQUARES + at.1], p, towards, reach * 2e-3)
        };
        seen.threads = super::threads::see(said, towards, &flat, &behind, cut, reach);
    }
    seen
}

/// `world` with only what `names` names: its bodies by their names, the
/// parts placed in it by their names (all of each), and bodies of a
/// placed part as `part/body`.
fn named(world: &World, names: &[String]) -> World {
    let mut only = world.clone();
    let others: Vec<_> = only
        .topo
        .bodies()
        .ids()
        .filter(|b| {
            only.topo
                .name_of(*b)
                .is_none_or(|name| !names.iter().any(|n| n == name))
        })
        .collect();
    for body in others {
        shapecraft_boolean::remove(&mut only.topo, body);
    }
    only.instances = world
        .instances
        .iter()
        .filter_map(|(id, instance)| {
            if names.iter().any(|n| n == id) {
                return Some((id.clone(), instance.clone()));
            }
            let within: Vec<String> = names
                .iter()
                .filter_map(|n| n.strip_prefix(id.as_str())?.strip_prefix('/'))
                .map(str::to_owned)
                .collect();
            (!within.is_empty()).then(|| {
                let mut instance = instance.clone();
                instance.world = Arc::new(named(&instance.world, &within));
                (id.clone(), instance)
            })
        })
        .collect();
    only
}

/// Whether `name` names a body of `world`, a part placed in it, or (as
/// `part/body`) a body of one.
fn names(world: &World, name: &str) -> bool {
    world
        .topo
        .bodies()
        .ids()
        .any(|b| world.topo.name_of(b) == Some(name))
        || world.instances.contains_key(name)
        || world.instances.iter().any(|(id, instance)| {
            name.strip_prefix(id.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .is_some_and(|rest| names(&instance.world, rest))
        })
}

/// How far the solid bodies of `world` reach from its origin.
fn world_reach(world: &World) -> f64 {
    let mut shape_of = Shape::default();
    shape(world, shapecraft_core::Pose::IDENTITY, &mut shape_of);
    shape_of
        .runs
        .iter()
        .flatten()
        .fold(1e-9_f64, |m, p| m.max(p.length()))
}

/// The middle of the box around the solid bodies of `world`.
fn world_middle(world: &World) -> Point3 {
    let mut shape_of = Shape::default();
    shape(world, shapecraft_core::Pose::IDENTITY, &mut shape_of);
    let all = shape_of.runs.iter().flatten();
    let (lo, hi) = all.fold(
        (
            Point3::splat(f64::INFINITY),
            Point3::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    if lo.x.is_finite() {
        (lo + hi) / 2.0
    } else {
        Point3::ZERO
    }
}

/// Every view of `sheet` where it stands, in the order they are
/// drawn; what cannot be drawn is told in `problems`.
/// What a view is seen as, asked for: see [`see_all`].
struct Job {
    key: String,
    bodies: Vec<String>,
    basis: [Point3; 3],
    cut: Option<Cutting>,
    middle: Point3,
    shaded: bool,
}

thread_local! {
    /// While the views of a sheet are only asked for: what they would
    /// be seen as.
    static ASKED: std::cell::RefCell<Option<Vec<Job>>> = const { std::cell::RefCell::new(None) };
}

/// What each view of `sheet` sees of the model, worked out all at once,
/// each on a thread of its own, and kept in `kept` for [`frames`]: what
/// takes long — cutting the model, what hides what, shading — is not
/// done one view after the other.
pub(crate) fn see_all(sheet: &Sheet, subject: &Subject<'_>, kept: &Kept) {
    ASKED.with(|a| *a.borrow_mut() = Some(Vec::new()));
    let _ = frames(sheet, subject, kept, &mut Vec::new());
    let mut jobs = ASKED.with(|a| a.borrow_mut().take()).unwrap_or_default();
    jobs.sort_by(|a, b| a.key.cmp(&b.key));
    jobs.dedup_by(|a, b| a.key == b.key);
    // (Without threads, each view sees for itself as it is laid.)
    if jobs.len() < 2 || !shapecraft_core::HAS_THREADS {
        return;
    }
    let world = subject.world;
    std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|job| {
                scope.spawn(move || {
                    let seen = if job.bodies.is_empty() {
                        see(world, job.basis, job.middle, job.cut.as_ref(), job.shaded)
                    } else {
                        let only = named(world, &job.bodies);
                        see(&only, job.basis, job.middle, job.cut.as_ref(), job.shaded)
                    };
                    (job.key.clone(), seen)
                })
            })
            .collect();
        for handle in handles {
            if let Ok((key, seen)) = handle.join() {
                kept.put(key, seen);
            }
        }
    });
}

pub(crate) fn frames<'a>(
    sheet: &'a Sheet,
    subject: &Subject<'_>,
    kept: &Kept,
    problems: &mut Vec<String>,
) -> Vec<Placed<'a>> {
    // (What each view sees worked out first, all at once — unless that
    // is what this is asked for.)
    if ASKED.with(|a| a.borrow().is_none()) {
        see_all(sheet, subject, kept);
    }
    let world = subject.world;
    let stamp = world.stamp;
    let middle_key = format!("{stamp}/middle");
    // (The model's middle, kept as a view of nothing is.)
    let middle = {
        let seen = kept.seen(middle_key, || {
            let m = world_middle(world);
            Seen {
                lo: Point2::new(m.x, m.y),
                hi: Point2::new(m.z, 0.0),
                ..Seen::default()
            }
        });
        Point3::new(seen.lo.x, seen.lo.y, seen.hi.x)
    };
    let mut placed: Vec<Placed<'a>> = Vec::new();
    let mut left: Vec<&SheetView> = sheet.views.iter().collect();
    // Those that stand by themselves first, then what is of them.
    while !left.is_empty() {
        let before = left.len();
        let mut later = Vec::new();
        for view in left {
            let parent = |of: &str| placed.iter().find(|p| p.view.id == of);
            let own_scale = millimetres(view.scale.unwrap_or(sheet.scale), subject.unit);
            let at = Point2::from_array(view.at);
            // Only the bodies it names, if it names any: the world
            // without the others.
            let only = || named(world, &view.bodies);
            // (What it names that is neither a body nor a placed part.)
            for name in &view.bodies {
                if !names(world, name) {
                    problems.push(format!(
                        "the view {} names {name}, which is no body of the model nor a placed part (nor a \
                         body of one, as `part/body`)",
                        view.id
                    ));
                }
            }
            // What its own coordinates are from: the middle of what it
            // shows — of the bodies it names, where it names some (a
            // view of one body of many stands where it is put, not
            // where that body is off the middle of them all).
            let middle = if view.bodies.is_empty() {
                middle
            } else {
                let seen = kept.seen(format!("{stamp}/middle/{:?}", view.bodies), || {
                    let m = world_middle(&only());
                    Seen {
                        lo: Point2::new(m.x, m.y),
                        hi: Point2::new(m.z, 0.0),
                        ..Seen::default()
                    }
                });
                Point3::new(seen.lo.x, seen.lo.y, seen.hi.x)
            };
            // (Its faces, shaded, only where it is drawn so.)
            let shaded = view.style == Style::Shaded;
            let whole = |basis: [Point3; 3], cut: Option<Cutting>, middle: Point3| {
                let key = format!(
                    "{stamp}/{basis:?}/{cut:?}/{:?}/{middle:?}/{shaded}",
                    view.bodies
                );
                // (Where the views are only asked for, to be seen all at
                // once: asked for, nothing seen yet.)
                let asked = ASKED.with(|a| {
                    let mut a = a.borrow_mut();
                    let jobs = a.as_mut()?;
                    if !kept.has(&key) {
                        jobs.push(Job {
                            key: key.clone(),
                            bodies: view.bodies.clone(),
                            basis,
                            cut: cut.clone(),
                            middle,
                            shaded,
                        });
                    }
                    Some(())
                });
                if asked.is_some() {
                    return Arc::new(Seen::default());
                }
                kept.seen(key, || {
                    if view.bodies.is_empty() {
                        return see(world, basis, middle, cut.as_ref(), shaded);
                    }
                    see(&only(), basis, middle, cut.as_ref(), shaded)
                })
            };
            // What the part has not (a view or a section that was
            // removed from it) cannot be drawn.
            let lost = match &view.kind {
                ViewKind::Named { view } if !world.views.contains_key(view) => {
                    Some(format!("the part has no camera view {view}"))
                }
                ViewKind::Section { section, .. } if !world.sections.contains_key(section) => {
                    Some(format!("the part has no section {section}"))
                }
                _ if view
                    .section
                    .as_ref()
                    .is_some_and(|s| !world.sections.contains_key(s)) =>
                {
                    Some(format!(
                        "the part has no section {}",
                        view.section.as_deref().unwrap_or_default()
                    ))
                }
                _ => None,
            };
            if let Some(lost) = lost {
                problems.push(format!("the view {}: {lost}", view.id));
                continue;
            }
            // (A view from a side cut open by a section of the part,
            // if it says one: a pictorial section.)
            let cut = view
                .section
                .as_ref()
                .and_then(|name| world.sections.get(name).cloned());
            let made = match &view.kind {
                ViewKind::Named { view: named } => world.views.get(named).map(|seen| {
                    let basis = [seen.right, seen.up, seen.towards];
                    Placed {
                        view,
                        seen: whole(basis, cut.clone(), middle),
                        basis,
                        middle,
                        centre: at,
                        origin: Point2::ZERO,
                        scale: own_scale,
                        reach: None,
                        cutting: cut.clone(),
                        tangent: sheet.tangent_edges,
                        outside: None,
                    }
                }),
                ViewKind::Base { side } => {
                    let basis = side.basis();
                    Some(Placed {
                        view,
                        seen: whole(basis, cut.clone(), middle),
                        basis,
                        middle,
                        centre: at,
                        origin: Point2::ZERO,
                        scale: own_scale,
                        reach: None,
                        cutting: cut,
                        tangent: sheet.tangent_edges,
                        outside: None,
                    })
                }
                ViewKind::Projected { of, beside } => parent(of).map(|from| {
                    let [r, u, t] = from.basis;
                    // In first angle, what stands to the right is seen
                    // from the left; in third angle, from the right.
                    let first = sheet.projection == Projection::FirstAngle;
                    let sign = if first { 1.0 } else { -1.0 };
                    let basis = match beside {
                        Beside::Right => [t * sign, u, -r * sign],
                        Beside::Left => [-t * sign, u, r * sign],
                        Beside::Below => [r, -t * sign, u * sign],
                        Beside::Above => [r, t * sign, -u * sign],
                    };
                    // (Of the same bodies as what it is projected from,
                    // unless it names its own: measured from the same
                    // middle, it stands in line with it.)
                    let middle = if view.bodies.is_empty() {
                        from.middle
                    } else {
                        middle
                    };
                    // In line with what it is projected from.
                    let centre = match beside {
                        Beside::Right | Beside::Left => Point2::new(at.x, from.centre.y),
                        Beside::Above | Beside::Below => Point2::new(from.centre.x, at.y),
                    };
                    Placed {
                        view,
                        seen: whole(basis, None, middle),
                        basis,
                        middle,
                        centre,
                        origin: Point2::ZERO,
                        scale: from.scale,
                        reach: None,
                        cutting: None,
                        tangent: sheet.tangent_edges,
                        outside: None,
                    }
                }),
                ViewKind::Section {
                    of, section, turn, ..
                } => 'section: {
                    let from = match of {
                        Some(of) => match parent(of) {
                            Some(from) => Some(from),
                            None => break 'section None,
                        },
                        None => None,
                    };
                    let Some(cut) = world.sections.get(section).cloned() else {
                        break 'section None;
                    };
                    let cut = match from {
                        Some(from) => seen_in(cut, from.basis[2]),
                        None => cut,
                    };
                    let towards = cut.towards();
                    let d = (cut.path[1] - cut.path[0]).normalize_or_zero();
                    let d3 = cut.x * d.x + cut.y * d.y;
                    let normal = cut.x.cross(cut.y);
                    // The view the line shows in, if it looks along the
                    // cut: the section stands upright where the line
                    // does there, and in line with that view where the
                    // line is square to it.
                    let from = from.filter(|from| from.basis[2].dot(normal).abs() > 1.0 - 1e-9);
                    let (basis, centre) = match from {
                        Some(from) => {
                            let d = Point2::new(d3.dot(from.basis[0]), d3.dot(from.basis[1]));
                            if d.y.abs() > d.x.abs() {
                                let up = d3 * d.y.signum();
                                // (In line with the view while it is
                                // put about level with it; anywhere
                                // else it stands by its letters.)
                                let level = (at.y - from.centre.y).abs() < IN_LINE;
                                let centre = match d.x.abs() < 1e-9 && level {
                                    true => Point2::new(at.x, from.centre.y),
                                    false => at,
                                };
                                ([up.cross(towards), up, towards], centre)
                            } else {
                                let across = d3 * d.x.signum();
                                let level = (at.x - from.centre.x).abs() < IN_LINE;
                                let centre = match d.y.abs() < 1e-9 && level {
                                    true => Point2::new(from.centre.x, at.y),
                                    false => at,
                                };
                                ([across, towards.cross(across), towards], centre)
                            }
                        }
                        None => {
                            // By itself: what is nearest the model's z
                            // is up.
                            let up = if normal.z.abs() >= d3.z.abs() {
                                normal * if normal.z < 0.0 { -1.0 } else { 1.0 }
                            } else {
                                d3 * d3.z.signum()
                            };
                            ([up.cross(towards), up, towards], at)
                        }
                    };
                    // Upright, as the model is, where the projection
                    // would lay it on its side or upside down (a cut
                    // in a view from above): then it stands where it
                    // is put, in line with nothing.
                    let upright = (Point3::Z - towards * towards.z).try_normalize();
                    let (basis, centre) = match upright {
                        Some(up) if towards.z.abs() < 0.99 && basis[1].dot(up) < 0.5 => {
                            ([up.cross(towards), up, towards], at)
                        }
                        _ => (basis, centre),
                    };
                    let middle = match from {
                        Some(from) if view.bodies.is_empty() => from.middle,
                        _ => middle,
                    };
                    // Turned on the sheet, if that is asked: about the
                    // way it is looked at (then it stands where it is
                    // put, in line with nothing).
                    let (basis, centre) = if *turn == 0.0 {
                        (basis, centre)
                    } else {
                        let (sin, cos) = turn.to_radians().sin_cos();
                        let [r, u, t] = basis;
                        ([r * cos - u * sin, r * sin + u * cos, t], at)
                    };
                    // (Of a half section that shows its other half,
                    // the model uncut as well.)
                    let outside = view
                        .half
                        .filter(|h| !h.only)
                        .map(|_| whole(basis, None, middle));
                    Some(Placed {
                        view,
                        seen: whole(basis, Some(cut.clone()), middle),
                        basis,
                        middle,
                        centre,
                        origin: Point2::ZERO,
                        // (At the scale of the view it is of, unless
                        // it has one of its own.)
                        scale: match (from, view.scale) {
                            (Some(from), None) => from.scale,
                            _ => own_scale,
                        },
                        reach: None,
                        cutting: Some(cut),
                        tangent: sheet.tangent_edges,
                        outside,
                    })
                }
                ViewKind::Auxiliary { of, angle } => parent(of).map(|from| {
                    // Looking along the sheet in the view it is of: up
                    // stays what was towards who looks there.
                    let [r, u, t] = from.basis;
                    let way = Point2::from_angle(angle.to_radians());
                    let towards = -(r * way.x + u * way.y);
                    let basis = [t.cross(towards), t, towards];
                    let middle = if view.bodies.is_empty() {
                        from.middle
                    } else {
                        middle
                    };
                    Placed {
                        view,
                        seen: whole(basis, None, middle),
                        basis,
                        middle,
                        centre: at,
                        origin: Point2::ZERO,
                        scale: from.scale,
                        reach: None,
                        cutting: None,
                        tangent: sheet.tangent_edges,
                        outside: None,
                    }
                }),
                ViewKind::Detail {
                    of, centre, radius, ..
                } => parent(of).map(|from| Placed {
                    view,
                    seen: from.seen.clone(),
                    basis: from.basis,
                    middle: from.middle,
                    centre: at,
                    origin: Point2::from_array(*centre),
                    scale: own_scale,
                    reach: Some(radius.abs().max(1e-9)),
                    cutting: None,
                    tangent: sheet.tangent_edges,
                    outside: None,
                }),
            };
            if let Some(made) = &made
                && made.seen.uncut > 0
            {
                problems.push(format!(
                    "the section view {}: {} of the bodies it cuts could not be cut, and are drawn whole",
                    view.id, made.seen.uncut
                ));
            }
            match made {
                Some(made) => placed.push(made),
                None => later.push(view),
            }
        }
        left = later;
        if left.len() == before {
            for view in &left {
                problems.push(format!(
                    "the view {} is of a view there is none of",
                    view.id
                ));
            }
            break;
        }
    }
    placed
}

/// `cut` as the view looking along `looking` (towards who looks)
/// shows its line, where it is one straight cut — a plane — that the
/// view sees edge on but that is said in another plane (a section
/// along a plane says it in any): the same cut, said in the plane the
/// view looks onto.
fn seen_in(cut: Cutting, looking: Point3) -> Cutting {
    let towards = cut.towards();
    let shown = cut.x.cross(cut.y).dot(looking).abs() > 1.0 - 1e-9;
    if cut.bounded || cut.path.len() != 2 || shown || towards.dot(looking).abs() > 1e-9 {
        return cut;
    }
    let on = cut.origin + cut.x * cut.path[0].x + cut.y * cut.path[0].y;
    Cutting {
        origin: on,
        x: towards.cross(looking).normalize(),
        y: towards,
        path: vec![Point2::new(-1.0, 0.0), Point2::new(1.0, 0.0)],
        bounded: false,
    }
}

/// Of the line from `a` to `b` (from a view's middle line), what lies in
/// the half `side` of the view (`inside`) or in the other half.
fn in_half(a: Point2, b: Point2, side: HalfSide, inside: bool) -> Option<(Point2, Point2)> {
    // (How far into the half each end is.)
    let depth = |p: Point2| {
        let d = match side {
            HalfSide::Left => -p.x,
            HalfSide::Right => p.x,
            HalfSide::Above => p.y,
            HalfSide::Below => -p.y,
        };
        if inside { d } else { -d }
    };
    let (da, db) = (depth(a), depth(b));
    match (da >= 0.0, db >= 0.0) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        _ => {
            let m = a.lerp(b, da / (da - db));
            let piece = if da >= 0.0 { (a, m) } else { (m, b) };
            (piece.0.distance(piece.1) > 1e-12).then_some(piece)
        }
    }
}

/// Of the line from `a` to `b`, what lies in the box from `lo` to
/// `hi`.
fn within_box(a: Point2, b: Point2, lo: Point2, hi: Point2) -> Option<(Point2, Point2)> {
    let (mut s, mut t) = (0.0_f64, 1.0_f64);
    let d = b - a;
    for (start, way, low, high) in [(a.x, d.x, lo.x, hi.x), (a.y, d.y, lo.y, hi.y)] {
        if way.abs() < 1e-300 {
            if start < low || start > high {
                return None;
            }
            continue;
        }
        let (u, v) = ((low - start) / way, (high - start) / way);
        s = s.max(u.min(v));
        t = t.min(u.max(v));
    }
    (t > s).then(|| (a + d * s, a + d * t))
}

/// The part of the line from `a` to `b` within `reach` of `centre`.
fn within(a: Point2, b: Point2, centre: Point2, reach: f64) -> Option<(Point2, Point2)> {
    let d = b - a;
    let f = a - centre;
    let (qa, qb, qc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - reach * reach);
    if qa <= 0.0 {
        return None;
    }
    let disc = qb * qb - 4.0 * qa * qc;
    if disc <= 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let (t0, t1) = ((-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa));
    let (t0, t1) = (t0.max(0.0), t1.min(1.0));
    (t1 > t0).then(|| (a + d * t0, a + d * t1))
}

/// Of the triangle `corners`, what lies within `reach` of `centre` (a
/// polygon of many sides inside the circle), as triangles.
fn in_circle(corners: [Point2; 3], centre: Point2, reach: f64) -> Vec<[Point2; 3]> {
    const SIDES: usize = 96;
    let mut shape: Vec<Point2> = corners.to_vec();
    for k in 0..SIDES {
        let at = |k: usize| {
            centre + Point2::from_angle(k as f64 * std::f64::consts::TAU / SIDES as f64) * reach
        };
        let (p, q) = (at(k), at(k + 1));
        // (Which side of the polygon's side `p`→`q` a point is: inside
        // when positive.)
        let side = |x: Point2| (q - p).perp_dot(x - p);
        let mut kept = Vec::with_capacity(shape.len() + 1);
        for i in 0..shape.len() {
            let (a, b) = (shape[i], shape[(i + 1) % shape.len()]);
            let (sa, sb) = (side(a), side(b));
            if sa >= 0.0 {
                kept.push(a);
            }
            if (sa >= 0.0) != (sb >= 0.0) {
                kept.push(a.lerp(b, sa / (sa - sb)));
            }
        }
        shape = kept;
        if shape.len() < 3 {
            return Vec::new();
        }
    }
    (1..shape.len() - 1)
        .map(|k| [shape[0], shape[k], shape[k + 1]])
        .collect()
}

/// How the bodies a section cuts are hatched, one after the other:
/// the angle of the lines (45° or 135°) and how far apart they are, in
/// millimetres.
const HATCHES: [(f64, f64); 4] = [(45.0, 2.5), (135.0, 2.5), (45.0, 1.5), (135.0, 1.5)];

/// The narrowest hatching (ISO 128-50: no closer than 0.7 mm).
const CLOSEST_HATCH: f64 = 0.7;

/// How the cut `triangles` (on the sheet) of one body are hatched, from
/// its own way `(angle, gap)`: the angle — of 45° and 135°, else 30°,
/// 60°, 120° or 150° — whose lines run least along its outline (ISO
/// 128-50: hatching is at an angle to the main outlines; lines along a
/// thin wall leave it all but empty), and the gap narrowed to a thin
/// cut, so that a few lines cross even a thin wall.
fn hatching(triangles: &[[Point2; 3]], (angle, gap): (f64, f64)) -> (f64, f64) {
    // Its outline: the sides of its triangles that only one of them has.
    let (outline, area) = outline_of(triangles);
    let around: f64 = outline.iter().map(|(a, b)| a.distance(*b)).sum();
    if around <= 0.0 {
        return (angle, gap);
    }
    // (How much of the outline runs along lines at `angle`: within 20°,
    // the nearer the more.)
    let along = |angle: f64| -> f64 {
        outline
            .iter()
            .map(|(a, b)| {
                let d = *b - *a;
                let off = (d.y.atan2(d.x).to_degrees() - angle).rem_euclid(180.0);
                let off = off.min(180.0 - off);
                a.distance(*b) * (1.0 - off / 20.0).max(0.0)
            })
            .sum()
    };
    let mirror = 180.0 - angle;
    let candidates = [
        angle,
        mirror,
        angle - 15.0,
        angle + 15.0,
        mirror + 15.0,
        mirror - 15.0,
    ];
    // (The first that runs along it little, or else the least.)
    let mut best = (f64::INFINITY, angle);
    for (k, c) in candidates.into_iter().enumerate() {
        let score = along(c) + around * 0.02 * k as f64;
        if score < best.0 {
            best = (score, c);
        }
    }
    // How thick the cut is: of a long strip, twice its area over the
    // way around it is its width.
    let thick = 2.0 * area / around;
    (best.1, gap.min(thick / 2.0).max(CLOSEST_HATCH.min(gap)))
}

/// A cut thinner than this (millimetres on the sheet) is filled black,
/// not hatched (ISO 128-50: narrow areas).
const SOLID: f64 = 1.0;

/// The outline of `triangles` — the sides only one of them has — and
/// their area.
fn outline_of(triangles: &[[Point2; 3]]) -> (Vec<(Point2, Point2)>, f64) {
    let key = |p: Point2| ((p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64);
    let mut sides: std::collections::HashMap<_, (Point2, Point2, usize)> = Default::default();
    let mut area = 0.0;
    for t in triangles {
        area += ((t[1] - t[0]).perp_dot(t[2] - t[0]) / 2.0).abs();
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            let side = if ka <= kb { (ka, kb) } else { (kb, ka) };
            sides.entry(side).or_insert((a, b, 0)).2 += 1;
        }
    }
    let outline = sides
        .into_values()
        .filter(|s| s.2 == 1)
        .map(|s| (s.0, s.1))
        .collect();
    (outline, area)
}

/// How thick the cut `triangles` is: of a long strip, twice its area
/// over the way around it is its width.
fn thickness(triangles: &[[Point2; 3]]) -> f64 {
    let (outline, area) = outline_of(triangles);
    let around: f64 = outline.iter().map(|(a, b)| a.distance(*b)).sum();
    if around > 0.0 {
        2.0 * area / around
    } else {
        0.0
    }
}

/// `triangles` in the pieces that hang together: corner to corner.
fn pieces_of(triangles: &[[Point2; 3]]) -> Vec<Vec<[Point2; 3]>> {
    let key = |p: Point2| ((p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64);
    let mut parent: Vec<usize> = (0..triangles.len()).collect();
    fn root(parent: &mut [usize], mut k: usize) -> usize {
        while parent[k] != k {
            parent[k] = parent[parent[k]];
            k = parent[k];
        }
        k
    }
    let mut at: std::collections::HashMap<(i64, i64), usize> = Default::default();
    for (k, t) in triangles.iter().enumerate() {
        for p in t {
            match at.get(&key(*p)) {
                Some(&j) => {
                    let (a, b) = (root(&mut parent, j), root(&mut parent, k));
                    parent[a] = b;
                }
                None => {
                    at.insert(key(*p), k);
                }
            }
        }
    }
    let mut pieces: std::collections::BTreeMap<usize, Vec<[Point2; 3]>> = Default::default();
    for (k, t) in triangles.iter().enumerate() {
        let r = root(&mut parent, k);
        pieces.entry(r).or_default().push(*t);
    }
    pieces.into_values().collect()
}

/// The lines of a hatching (ISO 128-50: narrow, at `angle` degrees,
/// `gap` apart) over the triangle `corners`.
fn hatch(laid: &mut Laid, corners: [Point2; 3], (angle, gap): (f64, f64)) {
    // Lines of constant distance across them.
    let (sin, cos) = angle.to_radians().sin_cos();
    let of = |p: Point2| p.y * cos - p.x * sin;
    let (lo, hi) = corners
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(of(*p)), hi.max(of(*p)))
        });
    let mut c = (lo / gap).ceil() * gap;
    while c <= hi {
        let mut ends = Vec::new();
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            let (a, b) = (corners[i], corners[j]);
            let (fa, fb) = (of(a) - c, of(b) - c);
            if (fa > 0.0) != (fb > 0.0) {
                ends.push(a.lerp(b, fa / (fa - fb)));
            }
        }
        if let [a, b] = ends[..] {
            laid.line(a, b, Pen::Narrow);
        }
        c += gap;
    }
}

/// The letters of the detail views by their circles in the views they
/// are of (ISO 128-3, as a section's): where the sheet says, else
/// outside the circle, clear of it, of the lines and of what is written
/// — near it, or farther off on a short leader to the circle.
pub(crate) fn detail_letters(laid: &mut Laid, frames: &[Placed<'_>]) {
    let height = super::title_letter();
    for placed in frames {
        let ViewKind::Detail {
            of,
            centre,
            radius,
            letter,
            letter_at,
        } = &placed.view.kind
        else {
            continue;
        };
        let Some(from) = frames.iter().find(|p| p.view.id == *of) else {
            continue;
        };
        let at = from.sheet(Point2::from_array(*centre));
        let r = radius * from.scale;
        let half = Point2::new(super::notes::width(letter, height), height) / 2.0;
        // (Whether the letter's box about `p`, a little larger, is clear
        // of what is drawn and written.)
        let boxes: Vec<[Point2; 2]> = laid
            .texts
            .iter()
            .filter(|t| !t.preview && !t.text.trim().is_empty())
            .map(super::notes::text_box)
            .collect();
        let clear = |p: Point2| {
            let (lo, hi) = (p - half - Point2::splat(0.8), p + half + Point2::splat(0.8));
            let in_box = |a: Point2, b: Point2| within_box(a, b, lo, hi).is_some();
            laid.strokes.iter().all(|s| s.preview || !in_box(s.a, s.b))
                && boxes
                    .iter()
                    .all(|b| b[1].x <= lo.x || hi.x <= b[0].x || b[1].y <= lo.y || hi.y <= b[0].y)
        };
        // (How far out from the middle the letter's box, about a point
        // that way, is clear of the circle by `gap`.)
        let out = |way: Point2, gap: f64| {
            // The box's corner nearest the circle's middle is past it.
            let reach = (way.x.abs() * half.x + way.y.abs() * half.y).max(half.length() * 0.7);
            at + way * (r + gap + reach)
        };
        let ways: Vec<Point2> = (0..16)
            .map(|k| {
                // (Up and right first, as it is drawn by hand.)
                let a = std::f64::consts::FRAC_PI_4 + std::f64::consts::TAU * f64::from(k) / 16.0;
                Point2::from_angle(a)
            })
            .collect();
        let said = letter_at.map(|d| at + Point2::from_array(d));
        let spot = said
            .or_else(|| ways.iter().map(|w| out(*w, 1.5)).find(|p| clear(*p)))
            .or_else(|| {
                [5.0, 9.0, 14.0]
                    .iter()
                    .find_map(|gap| ways.iter().map(|w| out(*w, *gap)).find(|p| clear(*p)))
            })
            .unwrap_or_else(|| out(ways[0], 1.5));
        laid.text(spot, letter.clone(), height, [0.5, 0.5]);
        // A leader to the circle where the letter stands off it.
        let way = (spot - at).normalize_or_zero();
        let edge = {
            // (Where the line from the circle's middle leaves the box.)
            let tx = if way.x.abs() > 1e-9 {
                half.x / way.x.abs()
            } else {
                f64::INFINITY
            };
            let ty = if way.y.abs() > 1e-9 {
                half.y / way.y.abs()
            } else {
                f64::INFINITY
            };
            spot - way * (tx.min(ty) + 0.5)
        };
        if edge.distance(at) > r + 2.5 {
            laid.line(at + way * r, edge, Pen::Narrow);
        }
    }
}

/// Draws the view `placed` of `sheet`, and what it has drawn in the
/// view it is of: where a section cuts, where a detail is.
pub(crate) fn draw(laid: &mut Laid, sheet: &Sheet, placed: &Placed<'_>, all: &[Placed<'_>]) {
    let seen = &placed.seen;
    let style = placed.view.style;
    let clip = |a: Point2, b: Point2| match placed.reach {
        Some(reach) => within(a, b, placed.origin, reach),
        None => Some((a, b)),
    };
    // The faces, shaded.
    if style == Style::Shaded && placed.reach.is_none() {
        for (corners, lit) in &seen.faces {
            let grey = |base: f64| (base * lit).clamp(0.0, 255.0) as u8;
            laid.fills.push(Fill {
                corners: corners.map(|p| placed.sheet(p)),
                colour: [grey(214.0), grey(218.0), grey(226.0)],
            });
        }
    }
    // The cut, hatched: each body cut its own way (ISO 128-50), so
    // that where they meet they are told apart.
    // Of a half section, the half that is cut (`half` of the view's own
    // coordinates, from its middle line).
    let half = placed.view.half.filter(|_| placed.cutting.is_some());
    let hatched = laid.strokes.len();
    let filled = laid.fills.len();
    let mut owners: Vec<usize> = seen.cut.iter().map(|c| c.1).collect();
    owners.sort_unstable();
    owners.dedup();
    for owner in owners {
        let triangles: Vec<[Point2; 3]> = seen
            .cut
            .iter()
            .filter(|c| c.1 == owner)
            .map(|c| c.0.map(|p| placed.sheet(p)))
            .collect();
        let own = HATCHES[owner % HATCHES.len()];
        let (angle, _) = hatching(&triangles, own);
        // Each piece of the cut as thin as it is: a narrow one closer,
        // one too narrow to hatch filled black (ISO 128-50).
        for piece in pieces_of(&triangles) {
            let (_, gap) = hatching(&piece, own);
            if thickness(&piece) < SOLID && half.is_none() {
                for corners in piece {
                    laid.fills.push(Fill {
                        corners,
                        colour: [0, 0, 0],
                    });
                }
                continue;
            }
            for corners in piece {
                hatch(laid, corners, (angle, gap));
            }
        }
    }
    // (Divided where the part's axis is, or the section's origin.)
    let divide = placed.divide();
    if let Some(half) = half {
        let zero = placed.sheet(divide);
        let cut: Vec<_> = laid.strokes.split_off(hatched);
        for stroke in cut {
            if let Some((a, b)) = in_half(stroke.a - zero, stroke.b - zero, half.cut, true) {
                laid.line(a + zero, b + zero, stroke.pen);
            }
        }
    }
    // Of a detail, the hatching and the filled cut only within its
    // circle, like all else it draws.
    if let Some(reach) = placed.reach {
        let (at, r) = (placed.centre, reach * placed.scale);
        let cut: Vec<_> = laid.strokes.split_off(hatched);
        for stroke in cut {
            if let Some((a, b)) = within(stroke.a, stroke.b, at, r) {
                laid.line(a, b, stroke.pen);
            }
        }
        let fills: Vec<_> = laid.fills.split_off(filled);
        for fill in fills {
            for corners in in_circle(fill.corners, at, r) {
                laid.fills.push(Fill { corners, ..fill });
            }
        }
    }
    let mut snaps: Vec<Point2> = Vec::new();
    let mut lines: Vec<(Point2, Point2)> = Vec::new();
    // Of a section that shows only what the cut meets: the outline of
    // the cut faces — the sides of their triangles that only one of
    // them has — and nothing of what lies behind.
    let only_cut = placed.view.cut_only && placed.cutting.is_some();
    if only_cut {
        let key = |p: Point2| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64);
        let mut sides: std::collections::BTreeMap<_, (Point2, Point2, usize)> = Default::default();
        for (corners, _) in &seen.cut {
            for k in 0..3 {
                let (a, b) = (corners[k], corners[(k + 1) % 3]);
                let (ka, kb) = (key(a), key(b));
                let side = if ka <= kb { (ka, kb) } else { (kb, ka) };
                sides.entry(side).or_insert((a, b, 0)).2 += 1;
            }
        }
        for (a, b, times) in sides.into_values() {
            if times != 1 {
                continue;
            }
            let Some((a, b)) = clip(a, b) else {
                continue;
            };
            let (a, b) = (placed.sheet(a), placed.sheet(b));
            laid.line(a, b, Pen::Wide);
            snaps.extend([a, b]);
            lines.push((a, b));
        }
    }
    // (The lines of the view, each with whether it is of the uncut
    // model: the other half of a half section.)
    let uncut = match (half, &placed.outside) {
        (Some(_), Some(outside)) => outside.lines.as_slice(),
        _ => &[],
    };
    let every = seen
        .lines
        .iter()
        .map(|l| (l, false))
        .chain(uncut.iter().map(|l| (l, true)));
    for ((a, b, behind, smooth), whole) in every.filter(|_| !only_cut) {
        let (a, b) = match half {
            Some(half) => match in_half(*a - divide, *b - divide, half.cut, !whole)
                .map(|(a, b)| (a + divide, b + divide))
            {
                Some(piece) => piece,
                None => continue,
            },
            None => (*a, *b),
        };
        let (a, b) = (&a, &b);
        let pen = match (style, behind, smooth, sheet.tangent_edges) {
            // A smooth edge (where a round goes on into a face) thin,
            // if at all; and not where it is hidden.
            (_, _, true, TangentEdges::Hidden) => continue,
            (Style::Wireframe, _, true, TangentEdges::Thin)
            | (_, false, true, TangentEdges::Thin) => Pen::Narrow,
            (Style::Wireframe, _, true, _) | (_, false, true, _) => Pen::Wide,
            (_, true, true, _) => continue,
            (Style::Wireframe, _, false, _) | (_, false, false, _) => Pen::Wide,
            (Style::HiddenDashed, true, false, _) => Pen::Dashed,
            (_, true, false, _) => continue,
        };
        let Some((a, b)) = clip(*a, *b) else {
            continue;
        };
        let (a, b) = (placed.sheet(a), placed.sheet(b));
        laid.line(a, b, pen);
        if pen == Pen::Wide {
            snaps.extend([a, b]);
            lines.push((a, b));
        }
    }
    super::threads::draw(
        laid,
        &seen.threads,
        style,
        placed.scale,
        &|p| placed.sheet(p),
        &clip,
    );
    // Of a section that shows only its cut, the centre lines of what is
    // round in the cut: not of what lies behind it, which is not drawn.
    let (cut_lo, cut_hi) = placed.shown();
    let in_cut = |p: Point2| {
        let near = (cut_hi - cut_lo).length() * 1e-6;
        !placed.only_cut()
            || (p.x >= cut_lo.x - near
                && p.y >= cut_lo.y - near
                && p.x <= cut_hi.x + near
                && p.y <= cut_hi.y + near)
    };
    let round_rounds: Vec<&(Point2, f64)> =
        seen.rounds.iter().filter(|(at, _)| in_cut(*at)).collect();
    // (An axis: as much of it as crosses the cut.)
    let axes: Vec<(Point2, Point2)> = seen
        .axes
        .iter()
        .filter_map(|&(from, to)| {
            if !placed.only_cut() {
                return Some((from, to));
            }
            within_box(from, to, cut_lo, cut_hi)
        })
        .collect();
    let rounds: Vec<(Point2, f64)> = round_rounds
        .iter()
        .copied()
        .filter(|(at, _)| clip(*at, *at + Point2::X * 1e-9).is_some() || placed.reach.is_none())
        .map(|(at, radius)| (placed.sheet(*at), radius * placed.scale))
        .collect();
    snaps.extend(rounds.iter().map(|r| r.0));
    snaps.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    snaps.dedup_by(|a, b| a.distance(*b) < 0.05);
    // Centre lines: a cross through what is round, a line along what
    // is seen from its side; each a little past what it is of — where
    // the sheet draws every centre line by itself (else see
    // `lines::draw`).
    let past = sheet.drafting.past;
    if sheet.centres == crate::sheet::Centres::All {
        let round_rounds: Vec<(Point2, f64)> = round_rounds.iter().map(|r| **r).collect();
        super::lines::seen_centres(laid, placed, &sheet.drafting, &round_rounds, &axes, &clip);
    }
    let (lo, hi) = placed.bounds();
    // A half section's halves divided by a centre line (ISO 128-3),
    // a little past the view.
    if let Some(half) = half {
        let zero = placed.sheet(divide);
        let (a, b) = match half.cut {
            HalfSide::Left | HalfSide::Right => (
                Point2::new(zero.x, lo.y - past),
                Point2::new(zero.x, hi.y + past),
            ),
            HalfSide::Above | HalfSide::Below => (
                Point2::new(lo.x - past, zero.y),
                Point2::new(hi.x + past, zero.y),
            ),
        };
        laid.line(a, b, Pen::Chain);
    }
    let above = Point2::new((lo.x + hi.x) / 2.0, hi.y + 6.0);
    let parent = |of: &str| all.iter().find(|p| p.view.id == of);
    let mut caption = None;
    match &placed.view.kind {
        ViewKind::Section { of, letter, .. } => {
            // Its name over it; and in the view it is of, where it
            // cuts (ISO 128-3): a thin chain line, wide at its ends
            // and where it turns, with arrows the way it is looked at
            // and its letter at each end.
            // (With its scale, where that is its own.)
            let name = match placed.view.scale.filter(|s| *s != sheet.scale) {
                Some(scale) => format!("{letter}–{letter} ({})", scale_text(scale)),
                None => format!("{letter}–{letter}"),
            };
            caption = Some(captioned(laid, placed, above, name, [0.5, 0.0]));
            let from = of.as_deref().and_then(parent);
            if let (Some(from), Some(cut)) = (from, &placed.cutting)
                && from.basis[2].dot(cut.x.cross(cut.y)).abs() > 1.0 - 1e-9
            {
                let mut path: Vec<Point2> = cut
                    .path
                    .iter()
                    .map(|p| from.place(cut.origin + cut.x * p.x + cut.y * p.y))
                    .collect();
                // (As the line is gone along in this view with what is
                // taken away to its left: seen from its back, the
                // other way.)
                let mut gone_along = path.clone();
                if from.basis[2].dot(cut.x.cross(cut.y)) < 0.0 {
                    gone_along.reverse();
                }
                // (One straight cut goes all across the view, and a
                // little past it; one along a sketched line as far as
                // that goes.)
                // (Of a half section, the end at the axis: no arrow there.)
                let mut inner: Option<usize> = None;
                if let ([a, b], false) = (&path[..], cut.bounded) {
                    let (a, b) = (*a, *b);
                    let (lo, hi) = from.bounds();
                    let along = (b - a).normalize_or_zero();
                    let spans = [lo, hi, Point2::new(lo.x, hi.y), Point2::new(hi.x, lo.y)]
                        .map(|c| (c - a).dot(along));
                    let (s0, s1) = spans
                        .iter()
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), s| {
                            (lo.min(*s), hi.max(*s))
                        });
                    // (And on to where its corners are, where they lie
                    // farther out: its arrows go where they are dragged.)
                    let (s0, s1) = (
                        (s0 - 8.0).min(0.0) + 8.0,
                        (s1 + 8.0).max(a.distance(b)) - 8.0,
                    );
                    path = vec![a + along * (s0 - 8.0), a + along * (s1 + 8.0)];
                    // (Of a half section, from where it is cut — the
                    // part's axis — out over the half that is cut.)
                    if let Some(half) = placed.view.half {
                        let d = placed.divide();
                        let [right, up, _] = placed.basis;
                        let axis = from.place(placed.middle + right * d.x + up * d.y);
                        let out = match half.cut {
                            HalfSide::Left => -right,
                            HalfSide::Right => right,
                            HalfSide::Above => up,
                            HalfSide::Below => -up,
                        };
                        let out = from.way(out).normalize_or_zero();
                        let at = (axis - a).dot(along);
                        let (s0, s1) = (s0 - 8.0, s1 + 8.0);
                        if out.dot(along) > 1e-6 && at < s1 {
                            path = vec![a + along * at.max(s0), a + along * s1];
                            inner = Some(0);
                        } else if out.dot(along) < -1e-6 && at > s0 {
                            path = vec![a + along * s0, a + along * at.min(s1)];
                            inner = Some(1);
                        }
                    }
                }
                let last = path.len().saturating_sub(1);
                let mut arrows = Vec::new();
                for (k, w) in path.windows(2).enumerate() {
                    let (a, b) = (w[0], w[1]);
                    laid.line(a, b, Pen::Chain);
                    let along = (b - a).normalize_or_zero();
                    let wide = |at: Point2, way: Point2, end: bool| {
                        let long: f64 = if end { 6.0 } else { 3.0 };
                        (at, at + way * long.min(a.distance(b) / 2.0))
                    };
                    for (p, q) in [wide(a, along, k == 0), wide(b, -along, k + 1 == last)] {
                        laid.line(p, q, Pen::ChainWide);
                    }
                    // Who looks at the cut looks from the side taken
                    // away: as the section view does.
                    let looking = -from.way(placed.basis[2]).normalize_or_zero();
                    for end in [
                        (k == 0 && inner != Some(0)).then_some(a),
                        (k + 1 == last && inner != Some(1)).then_some(b),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        arrows.push(end);
                        let tail = end - looking * 9.0;
                        laid.line(tail, end - looking * 4.0, Pen::Narrow);
                        laid.arrow(end, looking, 5.0);
                        laid.text(
                            tail - looking * 3.5,
                            letter.clone(),
                            super::title_letter(),
                            [0.5, 0.5],
                        );
                    }
                }
                laid.cuts.push(super::LaidCut {
                    view: placed.view.id.clone(),
                    of: from.view.id.clone(),
                    path: gone_along,
                    drawn: path,
                    arrows,
                });
            }
        }
        ViewKind::Auxiliary { of, angle } => {
            // In the view it is of: an arrow the way it looks, with a
            // letter; the same letter over it (ISO 128-3).
            if let Some(from) = parent(of) {
                let way = Point2::from_angle(angle.to_radians());
                let (flo, fhi) = from.bounds();
                let reach = ((fhi - flo) / 2.0).length() + 8.0;
                let tip = (flo + fhi) / 2.0 - way * reach;
                laid.line(tip - way * 12.0, tip - way * 4.0, Pen::Narrow);
                laid.arrow(tip, way, 5.0);
                let mark = super::view_mark(sheet, &placed.view.id);
                laid.text(
                    tip - way * 16.0,
                    mark.clone(),
                    super::title_letter(),
                    [0.5, 0.5],
                );
                caption = Some(captioned(laid, placed, above, mark, [0.5, 0.0]));
            }
        }
        ViewKind::Detail {
            of,
            centre,
            radius,
            letter,
            ..
        } => {
            let scale = placed.view.scale.unwrap_or(sheet.scale);
            caption = Some(captioned(
                laid,
                placed,
                above,
                format!("{letter} ({})", scale_text(scale)),
                [0.5, 0.0],
            ));
            laid.circle(placed.centre, radius * placed.scale, Pen::Narrow);
            if let Some(from) = parent(of) {
                let at = from.sheet(Point2::from_array(*centre));
                let r = radius * from.scale;
                laid.circle(at, r, Pen::Narrow);
                // (Its letter by the circle once all else is drawn: see
                // `detail_letters`.)
            }
        }
        // A view at a scale of its own says so.
        ViewKind::Base { .. } | ViewKind::Named { .. } | ViewKind::Projected { .. } => {
            if let Some(scale) = placed.view.scale.filter(|s| *s != sheet.scale) {
                let at = Point2::new((lo.x + hi.x) / 2.0, lo.y - 6.0);
                let (at, anchor) = match placed.view.caption_at {
                    Some(off) => (placed.centre + Point2::from_array(off), [0.5, 0.5]),
                    None => (at, [0.5, 1.0]),
                };
                let text = format!("({})", scale_text(scale));
                laid.text(at, text, super::lettering(), anchor);
                caption = laid.texts.last().map(super::notes::text_box);
            }
        }
    }
    laid.views.push(LaidView {
        id: placed.view.id.clone(),
        centre: (lo + hi) / 2.0,
        half: (hi - lo) / 2.0,
        scale: placed.scale,
        anchor: placed.centre,
        origin: placed.origin,
        detail: placed.reach.is_some(),
        snaps,
        lines,
        rounds,
        label: super::view_label(sheet, &placed.view.id),
        caption,
        cut_shows_in: cut_shows_in(sheet, placed, all),
    });
}

/// Writes the caption `text` of the view `placed` (in title letters):
/// at `at` (anchored by `anchor`), or where the view says it stands.
/// Gives its box.
fn captioned(
    laid: &mut Laid,
    placed: &Placed<'_>,
    at: Point2,
    text: String,
    anchor: [f64; 2],
) -> [Point2; 2] {
    let (at, anchor) = match placed.view.caption_at {
        Some(off) => (placed.centre + Point2::from_array(off), [0.5, 0.5]),
        None => (at, anchor),
    };
    laid.text(at, text, super::title_letter(), anchor);
    laid.texts
        .last()
        .map(super::notes::text_box)
        .unwrap_or([at, at])
}

/// Of the section view `placed`, the other views of `all` that see its
/// cut edge on, where its cutting line can show: none of a detail, and
/// none that is drawn from the section itself (that would go round).
fn cut_shows_in(sheet: &Sheet, placed: &Placed<'_>, all: &[Placed<'_>]) -> Vec<String> {
    let (ViewKind::Section { .. }, Some(cut)) = (&placed.view.kind, &placed.cutting) else {
        return Vec::new();
    };
    // (Whether the view `id` is drawn from the section, at some remove.)
    let from_it = |id: &str| {
        let mut at = sheet.view(id);
        for _ in 0..sheet.views.len() {
            let Some(view) = at else {
                return false;
            };
            if view.id == placed.view.id {
                return true;
            }
            let of = match &view.kind {
                ViewKind::Projected { of, .. }
                | ViewKind::Auxiliary { of, .. }
                | ViewKind::Detail { of, .. } => Some(of.as_str()),
                ViewKind::Section { of, .. } => of.as_deref(),
                ViewKind::Base { .. } | ViewKind::Named { .. } => None,
            };
            at = of.and_then(|of| sheet.view(of));
        }
        false
    };
    all.iter()
        .filter(|p| p.reach.is_none() && !from_it(&p.view.id))
        .filter(|p| {
            let seen = seen_in(cut.clone(), p.basis[2]);
            p.basis[2].dot(seen.x.cross(seen.y)).abs() > 1.0 - 1e-9
        })
        .map(|p| p.view.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_polygon_of_a_circle_is_drawn_round_and_its_chords_are_not() {
        // A circle of 120 drawn as 40 sides, broken (as wires crossing a
        // ring break its outline) into runs of 2, 3 and the rest; a
        // straight line from rim to rim (a wire's side, a flat cut),
        // meeting it at a sharp turn; and a short chord near the rim
        // joined to nothing.
        let r = 120.0;
        let at = |k: f64| Point2::from_angle(k * std::f64::consts::TAU / 40.0) * r;
        let mut lines: Vec<(Point2, Point2, bool, bool)> = Vec::new();
        for k in 0..40 {
            // (Broken at corners 0, 2 and 5: a gap of a hair there.)
            let (mut a, mut b) = (at(f64::from(k)), at(f64::from(k + 1)));
            if [0, 2, 5].contains(&k) {
                a += (b - a) * 1e-3;
            }
            if [39, 1, 4].contains(&k) {
                b -= (b - a) * 1e-3;
            }
            lines.push((a, b, false, false));
        }
        let chord = (at(12.0), at(28.0));
        lines.push((chord.0, chord.1, false, false));
        round_off(&mut lines, r);
        // The rim: no piece sags off the circle by more than a hundredth.
        for (a, b, _, _) in &lines {
            let middle = (*a + *b) / 2.0;
            let on_rim = (a.length() - r).abs() < 0.5 && (b.length() - r).abs() < 0.5;
            if on_rim && a.distance(*b) < r * 0.5 {
                assert!(r - middle.length() < 0.01, "{a:?} {b:?}");
            }
        }
        // The chord stays straight, whole.
        assert!(
            lines
                .iter()
                .any(|(a, b, _, _)| a.distance(chord.0) < 1e-9 && b.distance(chord.1) < 1e-9),
            "the chord is gone"
        );
    }

    #[test]
    fn a_short_flicker_of_hidden_along_a_line_is_steadied() {
        let p = |x: f64| Point2::new(x, 0.0);
        // In view, a short stretch "hidden", in view again; then a long
        // hidden stretch, which stays.
        let mut lines = vec![
            (p(0.0), p(5.0), false, false),
            (p(5.0), p(5.1), true, false),
            (p(5.1), p(10.0), false, false),
            (p(10.0), p(20.0), true, false),
            (p(20.0), p(30.0), false, false),
        ];
        steady(&mut lines, 1e-9, 0.5);
        let hidden: Vec<bool> = lines.iter().map(|l| l.2).collect();
        assert_eq!(hidden, [false, false, false, true, false]);
    }

    #[test]
    fn lines_over_each_other_are_drawn_once() {
        let p = |x: f64, y: f64| Point2::new(x, y);
        // The near and the far edge of a wall seen edge on, the one in
        // view, the other hidden and longer; and one in view half over
        // the first, the other way.
        let lines = vec![
            (p(0.0, 0.0), p(4.0, 0.0), false, false),
            (p(-1.0, 0.0), p(5.0, 0.0), true, false),
            (p(6.0, 0.0), p(2.0, 0.0), false, false),
            (p(0.0, 1.0), p(4.0, 1.0), false, false),
        ];
        let once = once(lines, 1e-9);
        let length = |behind: bool| -> f64 {
            once.iter()
                .filter(|l| l.2 == behind)
                .map(|l| l.0.distance(l.1))
                .sum()
        };
        // In view: 0 to 6 along y = 0, and the other line.
        assert!((length(false) - 10.0).abs() < 1e-9, "{once:?}");
        // Hidden: only what is not under that, from −1 to 0.
        assert!((length(true) - 1.0).abs() < 1e-9, "{once:?}");
    }

    #[test]
    fn a_thin_wall_along_the_hatching_is_hatched_across() {
        // A wall 40 long and 2.5 thick, running at 45°: two triangles.
        let along = Point2::new(1.0, 1.0).normalize();
        let across = along.perp();
        let p = |s: f64, t: f64| along * s + across * t;
        let wall = [
            [p(0.0, 0.0), p(40.0, 0.0), p(40.0, 2.5)],
            [p(0.0, 0.0), p(40.0, 2.5), p(0.0, 2.5)],
        ];
        let (angle, gap) = hatching(&wall, (45.0, 2.5));
        let off = (angle - 45.0).rem_euclid(180.0);
        assert!(off.min(180.0 - off) >= 15.0, "{angle}");
        assert!((CLOSEST_HATCH..=1.25 + 1e-9).contains(&gap), "{gap}");
        // Lines across it, many of them.
        let mut laid = Laid::default();
        for t in wall {
            hatch(&mut laid, t, (angle, gap));
        }
        assert!(laid.strokes.len() >= 20, "{}", laid.strokes.len());
        // A square stays at 45°, 2.5 apart.
        let square = [
            [
                Point2::ZERO,
                Point2::new(30.0, 0.0),
                Point2::new(30.0, 30.0),
            ],
            [
                Point2::ZERO,
                Point2::new(30.0, 30.0),
                Point2::new(0.0, 30.0),
            ],
        ];
        assert_eq!(hatching(&square, (45.0, 2.5)), (45.0, 2.5));
    }

    #[test]
    fn a_speck_of_a_line_on_its_own_goes() {
        let p = |x: f64, y: f64| Point2::new(x, y);
        let mut lines = vec![
            // An outline, with a short edge on its corner, which stays.
            (p(0.0, 0.0), p(50.0, 0.0), false, false),
            (p(50.0, 0.0), p(50.1, 0.1), false, false),
            // A speck on its own, in two bits; and a hidden line.
            (p(20.0, 20.0), p(20.1, 20.0), false, false),
            (p(20.1, 20.0), p(20.2, 20.0), false, false),
            (p(30.0, 30.0), p(30.1, 30.0), true, false),
        ];
        specks_out(&mut lines, 100.0);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines.iter().all(|l| l.0.y < 1.0 || l.2));
    }

    #[test]
    fn a_cut_too_thin_to_hatch_is_told_and_pieces_are_apart() {
        let p = Point2::new;
        let strip = |x: f64, w: f64| {
            [
                [p(x, 0.0), p(x + 40.0, 0.0), p(x + 40.0, w)],
                [p(x, 0.0), p(x + 40.0, w), p(x, w)],
            ]
        };
        let thin = strip(0.0, 0.5);
        assert!(thickness(&thin) < SOLID, "{}", thickness(&thin));
        assert!(thickness(&strip(0.0, 5.0)) > SOLID);
        let both: Vec<[Point2; 3]> = thin.into_iter().chain(strip(100.0, 5.0)).collect();
        assert_eq!(pieces_of(&both).len(), 2);
    }

    #[test]
    fn an_outline_drawn_as_a_polygon_on_a_circle_is_drawn_round() {
        // A circle of 10 as 40 straight pieces (the outline of a ring's
        // flat triangles), and a square beside it, which stays.
        let at = |k: usize| Point2::from_angle(std::f64::consts::TAU * k as f64 / 40.0) * 10.0;
        let mut lines: Vec<_> = (0..40).map(|k| (at(k), at(k + 1), false, false)).collect();
        let p = |x: f64, y: f64| Point2::new(x + 30.0, y);
        lines.extend([
            (p(0.0, 0.0), p(5.0, 0.0), false, false),
            (p(5.0, 0.0), p(5.0, 5.0), false, false),
            (p(5.0, 5.0), p(0.0, 5.0), false, false),
            (p(0.0, 5.0), p(0.0, 0.0), false, false),
        ]);
        round_off(&mut lines, 50.0);
        let round: Vec<_> = lines.iter().filter(|l| l.0.x < 20.0).collect();
        assert!(round.len() > 200, "{}", round.len());
        for (a, b, _, _) in round {
            assert!((((*a + *b) / 2.0).length() - 10.0).abs() < 0.01);
        }
        assert_eq!(lines.iter().filter(|l| l.0.x >= 20.0).count(), 4);
    }
}
