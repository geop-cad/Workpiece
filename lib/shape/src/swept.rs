//! Sweeping an outline along a path: the solid a section makes carried
//! along a curve, square to it, growing, shrinking or changing as it
//! goes.

use anyhow::Context;
use workpiece_boolean::Solid;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve3, Topo};

use crate::sweep::{self, Loft};

/// The outline that is swept: its sides one after the other around it
/// (each a curve of its own degree and knots, ending where the next
/// starts), and what its control points are at each place along the
/// path.
pub struct Outline<'a> {
    /// Of each side, its degree, its knots and how many control points
    /// it has.
    pub sides: Vec<(usize, Vec<f64>, usize)>,
    /// The control points of all the sides one after the other, at the
    /// share `along` of the path's length (from 0 at its start to 1):
    /// each (x, y) in the section's plane, and its weight.
    pub at: &'a dyn Fn(f64) -> Vec<(Point2, f64)>,
}

/// An outline that is the same all along: `points` (x, y and weight)
/// of sides as [`Outline::sides`] gives them.
pub fn steady(points: &[(Point2, f64)]) -> impl Fn(f64) -> Vec<(Point2, f64)> + '_ {
    move |_| points.to_vec()
}

/// The solid `outline` sweeps along `path` from `range.0` to `range.1`,
/// with flat ends. The section stays square to the path, its y along
/// `up` as nearly as that allows and its x to the right of that, seen
/// along the path; `size` is about how big all of it is (what it is
/// made exact against).
pub fn swept(
    id: &str,
    outline: &Outline<'_>,
    path: &Curve3,
    range: (f64, f64),
    up: Point3,
    size: f64,
) -> anyhow::Result<Solid> {
    let at = |share: f64| -> Vec<(Point3, f64)> {
        (outline.at)(share)
            .into_iter()
            .map(|(p, w)| (Point3::new(p.x, p.y, 0.0), w))
            .collect()
    };
    carried(
        id,
        &outline.sides,
        &at,
        path,
        range,
        (0.0, 1.0),
        up,
        size,
        false,
    )
}

/// How a section is kept upright as it is carried along a path.
#[derive(Debug, Clone, Copy)]
pub enum Upright {
    /// Its y stays along this as nearly as the path allows, all the way.
    Fixed(Point3),
    /// Its y starts along this as nearly as the path allows and then
    /// never turns about the path: the rotation-minimizing frame.
    Least(Point3),
}

impl From<Point3> for Upright {
    fn from(up: Point3) -> Self {
        Upright::Fixed(up)
    }
}

/// The frames of a section along a path: at each place the section's
/// x, y and the way the path goes.
struct Frames<'a> {
    path: &'a Curve3,
    range: (f64, f64),
    up: Point3,
    /// For the frame that never turns about the path: its y at evenly
    /// spaced places of the range, each got from the one before by
    /// double reflection (Wang, Jüttler, Zheng and Liu, 2008), which is
    /// exact to the fourth order of the step.
    least: Vec<Point3>,
}

impl<'a> Frames<'a> {
    const STEPS: usize = 2048;

    fn new(path: &'a Curve3, range: (f64, f64), up: Upright) -> anyhow::Result<Self> {
        let (up, least) = match up {
            Upright::Fixed(up) => (up, false),
            Upright::Least(up) => (up, true),
        };
        let mut frames = Frames {
            path,
            range,
            up,
            least: Vec::new(),
        };
        if least {
            let (t0, t1) = range;
            let (mut place, mut along) = frames.way(t0)?;
            let mut y = up
                .reject_from_normalized(along)
                .try_normalize()
                .context("the path runs along the way that is to stay up")?;
            let mut all = vec![y];
            for k in 1..=Self::STEPS {
                let t = t0 + (t1 - t0) * k as f64 / Self::STEPS as f64;
                let (next, onward) = frames.way(t)?;
                y = reflected(y, place, along, next, onward);
                (place, along) = (next, onward);
                all.push(y);
            }
            frames.least = all;
        }
        Ok(frames)
    }

    /// Where the path is at `t`, and which way it goes.
    fn way(&self, t: f64) -> anyhow::Result<(Point3, Point3)> {
        let (place, d) = self.path.derivative_at(t);
        let along = d.try_normalize().context("the path has no direction")?;
        Ok((place, along))
    }

    /// The place on the path at `t` and the section's x, y and onward
    /// there.
    fn at(&self, t: f64) -> anyhow::Result<(Point3, [Point3; 3])> {
        let (place, along) = self.way(t)?;
        let y = if self.least.is_empty() {
            self.up
        } else {
            let (t0, t1) = self.range;
            let x = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0) * Self::STEPS as f64;
            let k = (x.round() as usize).min(Self::STEPS);
            let from = t0 + (t1 - t0) * k as f64 / Self::STEPS as f64;
            let (before, was) = self.way(from)?;
            reflected(self.least[k], before, was, place, along)
        };
        let y = y
            .reject_from_normalized(along)
            .try_normalize()
            .context("the path runs along the way that is to stay up")?;
        Ok((place, [y.cross(along), y, along]))
    }

    /// The section's y where the range ends.
    fn last(&self) -> Point3 {
        self.least.last().copied().unwrap_or(self.up)
    }
}

/// `y` of a frame at `from` going `was`, taken to `to` going `along`
/// without turning about the path: reflected in the plane between the
/// two places, then in the one that takes the reflected direction to
/// the new one.
fn reflected(y: Point3, from: Point3, was: Point3, to: Point3, along: Point3) -> Point3 {
    let v1 = to - from;
    let c1 = v1.length_squared();
    if c1 <= 0.0 {
        return y;
    }
    let y1 = y - v1 * (2.0 / c1 * v1.dot(y));
    let t1 = was - v1 * (2.0 / c1 * v1.dot(was));
    let v2 = along - t1;
    let c2 = v2.length_squared();
    if c2 <= 0.0 {
        return y1;
    }
    y1 - v2 * (2.0 / c2 * v2.dot(y1))
}

/// As [`swept`], of a section that need not lie square to the path:
/// each of its control points is given by how far it is to the right
/// (x), up (y) and on along the path (z) from the place on the path.
/// `shares` is what `at` is asked for at the two ends of the range (by
/// length between); `around` says that the path closes on itself, and
/// the solid with it.
#[allow(clippy::too_many_arguments)]
pub fn carried(
    id: &str,
    sides: &[(usize, Vec<f64>, usize)],
    at: &dyn Fn(f64) -> Vec<(Point3, f64)>,
    path: &Curve3,
    range: (f64, f64),
    shares: (f64, f64),
    up: impl Into<Upright>,
    size: f64,
    around: bool,
) -> anyhow::Result<Solid> {
    let frames = Frames::new(path, range, up.into())?;
    let (t0, t1) = range;
    anyhow::ensure!(t1 > t0, "a path has some length");
    // How far along the path each place is, by length: the speed
    // summed by Gauss's rule over each of many steps, and from the
    // nearest step's start to the place.
    const STEPS: usize = 512;
    let run = |a: f64, b: f64| -> f64 {
        const NODES: [(f64, f64); 3] = [
            (-0.774_596_669_241_483_4, 5.0 / 9.0),
            (0.0, 8.0 / 9.0),
            (0.774_596_669_241_483_4, 5.0 / 9.0),
        ];
        let (middle, half) = ((a + b) / 2.0, (b - a) / 2.0);
        NODES
            .iter()
            .map(|(x, w)| path.derivative_at(middle + half * x).1.length() * w * half)
            .sum()
    };
    let step = |k: usize| t0 + (t1 - t0) * k as f64 / STEPS as f64;
    let mut lengths = vec![0.0; STEPS + 1];
    for k in 0..STEPS {
        lengths[k + 1] = lengths[k] + run(step(k), step(k + 1));
    }
    let whole = lengths[STEPS];
    anyhow::ensure!(whole > 0.0, "a path has some length");
    let share = |t: f64| {
        let t = t.clamp(t0, t1);
        let x = (t - t0) / (t1 - t0) * STEPS as f64;
        let k = (x.floor() as usize).min(STEPS - 1);
        let own = (lengths[k] + run(step(k), t)) / whole;
        shares.0 + (shares.1 - shares.0) * own
    };
    let mut flat = |t: f64| -> anyhow::Result<Vec<[f64; 4]>> {
        let (place, [x, y, along]) = frames.at(t)?;
        Ok(at(share(t))
            .into_iter()
            .map(|(p, w)| {
                let q = place + x * p.x + y * p.y + along * p.z;
                [q.x, q.y, q.z, w]
            })
            .collect())
    };
    let (degree, knots, mut rings) = sweep::along(path, range, size * 1e-9, size, &mut flat)?;
    if around {
        let first = rings[0].clone();
        *rings.last_mut().context("a sweep has sections")? = first;
    }
    let loft = Loft {
        sides: sides.iter().map(|s| (s.0, s.1.clone())).collect(),
        counts: sides.iter().map(|s| s.2).collect(),
        degree,
        knots,
        rings,
        closed: around,
        tolerance: (size * 1e-9).max(1e-9),
    };
    let mut scratch = Topo::new();
    let body = sweep::loft(&mut scratch, id, &loft)?;
    Ok(workpiece_boolean::read(&scratch, body)?)
}

/// The sides of an outline, as an [`Outline`] takes them: of each, its
/// degree, its knots and how many control points it has.
pub type Sides = Vec<(usize, Vec<f64>, usize)>;

/// The control points of all the sides of an outline, one side after
/// the other, each with its weight.
pub type Weighted = Vec<(Point2, f64)>;

/// The sides of a polygon through `corners`, and its control points, as
/// an [`Outline`] takes them.
pub fn polygon(corners: &[Point2]) -> (Sides, Weighted) {
    let n = corners.len();
    let sides = vec![(1, vec![0.0, 0.0, 1.0, 1.0], 2); n];
    let points = (0..n)
        .flat_map(|k| [(corners[k], 1.0), (corners[(k + 1) % n], 1.0)])
        .collect();
    (sides, points)
}

/// The sides of a circle of `radius` about `centre` (four quarter
/// arcs), and its control points, as an [`Outline`] takes them.
pub fn circle(centre: Point2, radius: f64) -> (Sides, Weighted) {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let corner = |k: usize| match k % 4 {
        0 => Point2::X,
        1 => Point2::Y,
        2 => -Point2::X,
        _ => -Point2::Y,
    };
    let sides = vec![(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 3); 4];
    let points = (0..4)
        .flat_map(|k| {
            let (a, b) = (corner(k), corner(k + 1));
            [
                (centre + a * radius, 1.0),
                (centre + (a + b) * radius, w),
                (centre + b * radius, 1.0),
            ]
        })
        .collect();
    (sides, points)
}

/// The solid `outline` (the same all along) sweeps along the straight
/// runs from each of `corners` to the next (and, with `closed`, from the
/// last back to the first), mitered where two runs meet: each run is
/// cut off at the plane that halves the turn. The section's y is along
/// `up` (or, on a run that goes along `up` itself, along whatever is
/// square to it).
pub fn swept_runs(
    id: &str,
    sides: &[(usize, Vec<f64>, usize)],
    points: &[(Point2, f64)],
    corners: &[Point3],
    closed: bool,
    up: Point3,
    size: f64,
) -> anyhow::Result<Solid> {
    use workpiece_boolean::{Op, combine};
    let n = corners.len();
    anyhow::ensure!(n >= 2, "a path has two ends");
    let runs = if closed { n } else { n - 1 };
    let way = |k: usize| (corners[(k + 1) % n] - corners[k % n]).normalize_or_zero();
    // How far the section reaches from the path: a run goes that much
    // further past a corner than the corner, to be cut back there.
    let reach = points.iter().map(|p| p.0.length()).fold(0.0, f64::max) * 4.0 + size * 0.01;
    let at = steady(points);
    let outline = Outline {
        sides: sides.to_vec(),
        at: &at,
    };
    let mut whole: Option<Solid> = None;
    for k in 0..runs {
        let d = way(k);
        anyhow::ensure!(d != Point3::ZERO, "two corners of the path are one");
        let before = (closed || k > 0).then(|| way((k + n - 1) % n));
        let after = (closed || k + 1 < runs).then(|| way(k + 1));
        let (from, to) = (corners[k], corners[(k + 1) % n]);
        let line = Curve3::line(
            from - d * if before.is_some() { reach } else { 0.0 },
            to + d * if after.is_some() { reach } else { 0.0 },
        )?;
        let upright = if up.cross(d).length() > 1e-6 {
            up
        } else {
            d.any_orthonormal_vector()
        };
        let mut run = swept(
            &format!("{id}/run{k}"),
            &outline,
            &line,
            line.domain(),
            upright,
            size,
        )?;
        // Cut back at each corner to the plane that halves the turn.
        for (corner, other, keep_before) in [(from, before, false), (to, after, true)] {
            let Some(other) = other else {
                continue;
            };
            let across = (d + other)
                .try_normalize()
                .context("the path turns right back")?;
            // (Past the plane, for the run that comes to the corner;
            // short of it, for the one that leaves.)
            let out = if keep_before { across } else { -across };
            let (a, b) = out.any_orthonormal_pair();
            let far = reach * 4.0 + size;
            let beyond = crate::blend::cuboid(
                &format!("{id}/beyond{k}"),
                corner - (a + b) * far,
                [out, a, b],
                [far, far * 2.0, far * 2.0],
            )?;
            run = combine(Op::Cut, &run, &beyond)
                .context("mitering a corner of the path")?
                .solid;
        }
        whole = Some(match whole {
            None => run,
            Some(sofar) => {
                combine(Op::Union, &sofar, &run)
                    .context("joining two runs of the path")?
                    .solid
            }
        });
    }
    whole.context("nothing to sweep")
}

/// How long `curve` is, about.
fn length(curve: &Curve3) -> f64 {
    let (a, b) = curve.domain();
    let points = curve.polyline(a, b);
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Makes the outlines `loops` have as many sides each: each is cut
/// where another has a corner, as far along.
fn same_count(loops: &mut [Vec<Curve3>]) -> anyhow::Result<()> {
    if loops.iter().any(|sides| sides.len() != loops[0].len()) {
        let shares = |sides: &[Curve3]| -> Vec<f64> {
            let lengths: Vec<f64> = sides.iter().map(length).collect();
            let whole: f64 = lengths.iter().sum();
            let mut at = 0.0;
            let mut all = vec![0.0];
            for l in &lengths {
                at += l / whole;
                all.push(at);
            }
            all
        };
        let own: Vec<Vec<f64>> = loops.iter().map(|sides| shares(sides)).collect();
        let mut cuts: Vec<f64> = own
            .iter()
            .flatten()
            .copied()
            .filter(|c| *c < 1.0 - 1e-6)
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        cuts.push(1.0);
        for (sides, own) in loops.iter_mut().zip(&own) {
            let mut pieces = Vec::new();
            for pair in cuts.windows(2) {
                let middle = (pair[0] + pair[1]) / 2.0;
                let k = (0..sides.len())
                    .find(|k| own[k + 1] >= middle)
                    .unwrap_or(sides.len() - 1);
                let side = &sides[k];
                let (a, b) = side.domain();
                // Where on the side a share of the way around is: by
                // length along it.
                let param = |share: f64| {
                    let f = (share - own[k]) / (own[k + 1] - own[k]);
                    if f <= 1e-6 {
                        return a;
                    }
                    if f >= 1.0 - 1e-6 {
                        return b;
                    }
                    const STEPS: usize = 256;
                    let at = |i: usize| side.point_at(a + (b - a) * i as f64 / STEPS as f64);
                    let mut lengths = vec![0.0; STEPS + 1];
                    for i in 0..STEPS {
                        lengths[i + 1] = lengths[i] + at(i).distance(at(i + 1));
                    }
                    let wanted = f * lengths[STEPS];
                    let i = (0..STEPS)
                        .find(|i| lengths[i + 1] >= wanted)
                        .unwrap_or(STEPS - 1);
                    let within = (wanted - lengths[i]) / (lengths[i + 1] - lengths[i]).max(1e-300);
                    a + (b - a) * (i as f64 + within) / STEPS as f64
                };
                pieces.push(side.trimmed(param(pair[0]), param(pair[1]))?);
            }
            *sides = pieces;
        }
    }
    Ok(())
}

/// A closed outline of at least three sides: `outline`, its sides
/// halved until there are that many (a circle becomes four quarters).
fn at_least_three(outline: &[Curve3]) -> anyhow::Result<Vec<Curve3>> {
    let mut sides = outline.to_vec();
    anyhow::ensure!(!sides.is_empty(), "an outline has sides");
    while sides.len() < 3 {
        let mut halves = Vec::new();
        for side in &sides {
            let (a, b) = side.domain();
            let middle = (a + b) / 2.0;
            halves.push(side.trimmed(a, middle)?);
            halves.push(side.trimmed(middle, b)?);
        }
        sides = halves;
    }
    Ok(sides)
}

/// `curve` over the range from 0 to 1: its degree, knots, and control
/// points with their weights.
fn unit(curve: &Curve3) -> (usize, Vec<f64>, Vec<(Point3, f64)>) {
    let (a, b) = curve.domain();
    let knots = curve.knots().iter().map(|k| (k - a) / (b - a)).collect();
    let points = curve
        .control_points()
        .iter()
        .copied()
        .zip(curve.weights().iter().copied())
        .collect();
    (curve.degree(), knots, points)
}

/// What is beyond the plane through `corner` that faces along `out`,
/// taken off `solid`.
fn cut_beyond(
    id: &str,
    solid: &Solid,
    corner: Point3,
    out: Point3,
    far: f64,
) -> anyhow::Result<Solid> {
    let (a, b) = out.any_orthonormal_pair();
    let beyond = crate::blend::cuboid(
        id,
        corner - (a + b) * far,
        [out, a, b],
        [far, far * 2.0, far * 2.0],
    )?;
    Ok(
        workpiece_boolean::combine(workpiece_boolean::Op::Cut, solid, &beyond)
            .context("mitering a corner of the path")?
            .solid,
    )
}

/// The solid the closed `outline` (its sides one after the other, where
/// they are in space) makes carried along `path`: curves each starting
/// where the one before ends, the first where the outline is carried
/// from. The outline keeps its place relative to the path, which it is
/// turned with; `up` stays up (for a flat path, the normal of its
/// plane). Where two curves of the path meet at an angle the solid is
/// mitered. On the way it turns about the path by `twist` in all and
/// grows to `scale` times its size. A path that `closes` comes back to
/// its start.
pub fn along_path(
    id: &str,
    outline: &[Curve3],
    path: &[Curve3],
    closes: bool,
    up: impl Into<Upright>,
    twist: f64,
    scale: f64,
) -> anyhow::Result<Solid> {
    use workpiece_boolean::{Op, combine};
    let upright = up.into();
    let (Upright::Fixed(up) | Upright::Least(up)) = upright;
    // What keeps the section upright along each curve of the path: one
    // that does not turn about the path goes on from curve to curve.
    let mut held = upright;
    let outline = at_least_three(outline)?;
    anyhow::ensure!(!path.is_empty(), "pick a path");
    anyhow::ensure!(scale > 0.0, "the scale at the end is more than nothing");
    anyhow::ensure!(
        !closes || (twist == 0.0 && scale == 1.0),
        "what goes all the way around a closed path comes back as it left: no twist, no scale"
    );
    let ends = |curve: &Curve3| -> anyhow::Result<[(Point3, Point3); 2]> {
        let (a, b) = curve.domain();
        let way = |t: f64| {
            let (p, d) = curve.derivative_at(t);
            anyhow::Ok((p, d.try_normalize().context("the path has no direction")?))
        };
        anyhow::Ok([way(a)?, way(b)?])
    };
    // The outline, seen from where the path starts.
    let (origin, along) = ends(&path[0])?[0];
    let y = up
        .reject_from_normalized(along)
        .try_normalize()
        .context("the path runs along the way that is to stay up")?;
    let x = y.cross(along);
    let mut sides = Vec::new();
    let mut local = Vec::new();
    for side in &outline {
        let (degree, knots, points) = unit(side);
        sides.push((degree, knots, points.len()));
        for (p, w) in points {
            let d = p - origin;
            local.push((Point3::new(d.dot(x), d.dot(y), d.dot(along)), w));
        }
    }
    let reach = local.iter().map(|p| p.0.length()).fold(0.0, f64::max);
    let lengths: Vec<f64> = path.iter().map(length).collect();
    let whole: f64 = lengths.iter().sum();
    anyhow::ensure!(whole > 0.0, "a path has some length");
    let size = whole + reach * 2.0;
    let section = |share: f64| -> Vec<(Point3, f64)> {
        let (sin, cos) = (twist * share).sin_cos();
        let by = 1.0 + (scale - 1.0) * share;
        local
            .iter()
            .map(|(p, w)| {
                let turned = Point3::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos, p.z);
                (turned * by, *w)
            })
            .collect()
    };
    if closes && path.len() == 1 {
        let [a, b] = ends(&path[0])?;
        if a.1.angle_between(b.1) < 1e-6 {
            let range = path[0].domain();
            return carried(
                id,
                &sides,
                &section,
                &path[0],
                range,
                (0.0, 1.0),
                up,
                size,
                true,
            );
        }
    }
    let n = path.len();
    let mut whole_solid: Option<Solid> = None;
    let mut done = 0.0;
    for (k, piece) in path.iter().enumerate() {
        let shares = (done / whole, (done + lengths[k]) / whole);
        done += lengths[k];
        let [(from, d0), (to, d1)] = ends(piece)?;
        let name = format!("{id}/run{k}");
        let here = held;
        let mut run = carried(
            &name,
            &sides,
            &section,
            piece,
            piece.domain(),
            shares,
            here,
            size,
            false,
        )?;
        // (Up where the curve starts and where it ends, for what runs
        // on straight past its corners.)
        let mut ups = [up, up];
        if let Upright::Least(from) = here {
            let last = Frames::new(piece, piece.domain(), here)?.last();
            ups = [from, last];
            held = Upright::Least(last);
        }
        // Where the path turns a corner, the run goes on straight past
        // it and is cut back at the plane that halves the turn.
        let before = (closes || k > 0).then(|| ends(&path[(k + n - 1) % n]).map(|e| e[1].1));
        let after = (closes || k + 1 < n).then(|| ends(&path[(k + 1) % n]).map(|e| e[0].1));
        let corners = [
            (from, d0, before.transpose()?, shares.0, false),
            (to, d1, after.transpose()?, shares.1, true),
        ];
        for (corner, way, other, share, leaving) in corners {
            let Some(other) = other else {
                continue;
            };
            let turn = way.angle_between(other);
            if turn < 1e-6 {
                continue;
            }
            anyhow::ensure!(turn < 3.0, "the path turns right back on itself");
            let further = reach * (turn / 2.0).tan() * 1.5 + size * 1e-3;
            let line = if leaving {
                Curve3::line(corner, corner + way * further)?
            } else {
                Curve3::line(corner - way * further, corner)?
            };
            let fixed = |_: f64| section(share);
            let more = carried(
                &format!("{name}/past{}", u8::from(leaving)),
                &sides,
                &fixed,
                &line,
                line.domain(),
                (0.0, 1.0),
                ups[usize::from(leaving)],
                size,
                false,
            )?;
            run = combine(Op::Union, &run, &more)
                .context("running on past a corner of the path")?
                .solid;
            let across = (way + other)
                .try_normalize()
                .context("the path turns right back on itself")?;
            let out = if leaving { across } else { -across };
            let far = (reach + further) * 4.0 + size;
            run = cut_beyond(&format!("{name}/beyond"), &run, corner, out, far)?;
        }
        whole_solid = Some(match whole_solid {
            None => run,
            Some(sofar) => {
                combine(Op::Union, &sofar, &run)
                    .context("joining two runs of the path")?
                    .solid
            }
        });
    }
    whole_solid.context("nothing to sweep")
}

/// What the area within `loops` sweeps along `path` (as
/// [`along_path`] sweeps one outline): each loop its sides, and whether
/// it is around a hole of the area. What the outlines around the area
/// sweep, joined, less what those around its holes do. The first loop's
/// faces are called after `id`, those of loop `k` after `{id}/l{k}`.
pub fn area_along_path(
    id: &str,
    loops: &[(Vec<Curve3>, bool)],
    path: &[Curve3],
    closes: bool,
    up: impl Into<Upright>,
    twist: f64,
    scale: f64,
) -> anyhow::Result<Solid> {
    use workpiece_boolean::{Op, combine};
    let up = up.into();
    let mut whole: Option<Solid> = None;
    for holes in [false, true] {
        for (k, (sides, hole)) in loops.iter().enumerate() {
            if *hole != holes {
                continue;
            }
            let id = if k == 0 {
                id.to_owned()
            } else {
                format!("{id}/l{k}")
            };
            let swept = along_path(&id, sides, path, closes, up, twist, scale)?;
            whole = Some(match whole {
                None => swept,
                Some(sofar) => {
                    let op = if holes { Op::Cut } else { Op::Union };
                    combine(op, &sofar, &swept)
                        .context("putting the swept outlines together")?
                        .solid
                }
            });
        }
    }
    whole.context("nothing to sweep")
}

/// The solid through `sections`: closed outlines (the sides of each one
/// after the other, where they are in space), one after the other. With
/// two it runs straight from one to the other; with more, smoothly
/// through them all. Outlines of as many sides are joined side to side,
/// from the corners nearest each other; others are first cut where
/// another has a corner, as far around.
pub fn through_sections(
    id: &str,
    sections: &[Vec<Curve3>],
    how: &Shaping<'_>,
) -> anyhow::Result<Solid> {
    lofted(id, sections, false, how)
}

/// How a loft leaves its first section or arrives at its last: at a
/// place of the section, the way out of what it runs on from — into
/// the loft — and how it bends there (the curvature vector the loft
/// takes across the section, nothing for none asked).
pub struct Held<'a> {
    /// The way, and the bend, at a place of the section.
    pub at: &'a dyn Fn(Point3) -> (Point3, Option<Point3>),
}

/// What shapes a loft besides its sections: how it is held at its
/// first and last section, and the guide curves it runs along (each a
/// chain of curves, from the first section to the last, touching each
/// section on its way).
#[derive(Default)]
pub struct Shaping<'a> {
    /// At the first section, and at the last.
    pub ends: [Option<Held<'a>>; 2],
    /// The guides.
    pub guides: Vec<Vec<Curve3>>,
}

impl Shaping<'_> {
    /// Whether it shapes nothing: a loft as its sections make it.
    fn is_plain(&self) -> bool {
        self.ends.iter().all(Option::is_none) && self.guides.is_empty()
    }
}

/// The point of the side `k` of the ring `ring` (control points of the
/// sides of a section, one after the other: `sides` say their degrees,
/// knots and counts) at `t` (0 to 1 along it).
fn ring_point(sides: &[(usize, Vec<f64>, usize)], ring: &[[f64; 4]], k: usize, t: f64) -> Point3 {
    let offset: usize = sides[..k].iter().map(|s| s.2).sum();
    let (degree, knots, _) = &sides[k];
    let (first, values) = sweep::basis(*degree, knots, t.clamp(0.0, 1.0));
    let mut sum = [0.0; 4];
    for (value, c) in values.iter().zip(&ring[offset + first..]) {
        for i in 0..4 {
            sum[i] += value * c[i];
        }
    }
    Point3::new(sum[0], sum[1], sum[2]) / sum[3]
}

/// Points along a chain of curves, evenly by parameter, and how far
/// along it each is.
fn along_chain(chain: &[Curve3]) -> Vec<(Point3, f64)> {
    let mut out: Vec<(Point3, f64)> = Vec::new();
    for curve in chain {
        let (a, b) = curve.domain();
        for k in 0..=256 {
            let p = curve.point_at(a + (b - a) * f64::from(k) / 256.0);
            let at = out.last().map_or(0.0, |(q, l)| l + q.distance(p));
            out.push((p, at));
        }
    }
    out
}

/// The point `at` along a chain sampled by [`along_chain`].
fn chain_at(samples: &[(Point3, f64)], at: f64) -> Point3 {
    let k = samples
        .partition_point(|s| s.1 < at)
        .clamp(1, samples.len() - 1);
    let ((p, a), (q, b)) = (samples[k - 1], samples[k]);
    if b <= a {
        return q;
    }
    p + (q - p) * ((at - a) / (b - a)).clamp(0.0, 1.0)
}

/// The rings of a loft with `how` shaping it: the sections' rings at
/// `places` (rising from 0 to 1) held at the ends, and — for guides —
/// with rings put in between, each moved so that the loft runs through
/// the guides there. The loft's degree, knots along, and rings.
fn shaped(
    sides: &[(usize, Vec<f64>, usize)],
    rings: Vec<Vec<[f64; 4]>>,
    places: &[f64],
    centres: &[Point3],
    how: &Shaping<'_>,
) -> anyhow::Result<crate::interpolate::Spline> {
    use crate::interpolate::{Held as Ends, interpolate};
    let m = rings.len();
    let whole: f64 = (1..m).map(|j| centres[j].distance(centres[j - 1])).sum();
    // At each end, of each control point, the way the loft goes (as
    // long as it is long) and its bend.
    let ends: Vec<Ends> = (0..2)
        .map(|e| {
            let Some(held) = &how.ends[e] else {
                return Ends::default();
            };
            let (j, onward) = if e == 0 {
                (0, centres[1] - centres[0])
            } else {
                (m - 1, centres[m - 1] - centres[m - 2])
            };
            let mut first = Vec::new();
            let mut second = Vec::new();
            let mut bends = false;
            for c in &rings[j] {
                let p = Point3::new(c[0], c[1], c[2]) / c[3];
                let (way, bend) = (held.at)(p);
                // (Into the loft: on from the first, on into the last.)
                let way = way.normalize_or_zero();
                let way = if way.dot(onward) < 0.0 { -way } else { way };
                let d = way * whole;
                first.push([d.x * c[3], d.y * c[3], d.z * c[3], 0.0]);
                if let Some(bend) = bend {
                    bends = true;
                    let k = bend * whole * whole;
                    second.push([k.x * c[3], k.y * c[3], k.z * c[3], 0.0]);
                } else {
                    second.push([0.0; 4]);
                }
            }
            Ends {
                first: Some(first),
                second: bends.then_some(second),
            }
        })
        .collect();
    let (degree, knots, control) = interpolate(places, rings.clone(), [&ends[0], &ends[1]])?;
    if how.guides.is_empty() {
        return Ok((degree, knots, control));
    }
    // The ring of the loft at `v`.
    let ring_at = |v: f64| -> Vec<[f64; 4]> {
        let (first, values) = sweep::basis(degree, &knots, v);
        let mut out = vec![[0.0; 4]; control[0].len()];
        for (value, row) in values.iter().zip(&control[first..]) {
            for (o, c) in out.iter_mut().zip(row) {
                for i in 0..4 {
                    o[i] += value * c[i];
                }
            }
        }
        out
    };
    // Where each guide touches each section: the side and the place
    // along it; and how far along the guide that is.
    let guides: Vec<Vec<(Point3, f64)>> = how.guides.iter().map(|g| along_chain(g)).collect();
    let mut anchors: Vec<Vec<(f64, f64)>> = Vec::new();
    for samples in &guides {
        let mut of_guide = Vec::new();
        for ring in &rings {
            let mut best = (f64::INFINITY, 0.0, 0.0);
            for k in 0..sides.len() {
                for q in 0..=48 {
                    let t = f64::from(q) / 48.0;
                    let p = ring_point(sides, ring, k, t);
                    for (g, along) in samples {
                        let far = p.distance(*g);
                        if far < best.0 {
                            best = (far, k as f64 + t, *along);
                        }
                    }
                }
            }
            // Closer: the place on the section nearest the guide's
            // place, and the guide's place nearest that, in turn.
            let (mut place, mut along) = (best.1, best.2);
            for _ in 0..4 {
                let g = chain_at(samples, along);
                let k = (place.floor() as usize).min(sides.len() - 1);
                let (mut lo, mut hi) = (
                    (place - k as f64 - 0.05).max(0.0),
                    (place - k as f64 + 0.05).min(1.0),
                );
                for _ in 0..60 {
                    let (x, y) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                    if ring_point(sides, ring, k, x).distance(g)
                        < ring_point(sides, ring, k, y).distance(g)
                    {
                        hi = y;
                    } else {
                        lo = x;
                    }
                }
                place = k as f64 + (lo + hi) / 2.0;
                let p = ring_point(sides, ring, k, place - k as f64);
                along = samples
                    .iter()
                    .min_by(|x, y| x.0.distance(p).total_cmp(&y.0.distance(p)))
                    .map_or(along, |s| s.1);
            }
            of_guide.push((place, along));
        }
        anchors.push(of_guide);
    }
    let size = centres
        .iter()
        .zip(&rings)
        .map(|(c, ring)| {
            ring.iter()
                .map(|r| (Point3::new(r[0], r[1], r[2]) / r[3]).distance(*c))
                .fold(0.0, f64::max)
        })
        .fold(whole, f64::max);
    for (g, samples) in guides.iter().enumerate() {
        let _ = samples;
        for j in 0..m {
            let (place, _) = anchors[g][j];
            let k = (place.floor() as usize).min(sides.len() - 1);
            let p = ring_point(sides, &rings[j], k, place - k as f64);
            let g_point = chain_at(&guides[g], anchors[g][j].1);
            anyhow::ensure!(
                p.distance(g_point) <= size * 1e-3,
                "guide {} does not touch section {}: a guide runs from the first section to the last through each",
                g + 1,
                j + 1
            );
        }
    }
    // Rings in between, a few in each span, moved onto the guides.
    const BETWEEN: usize = 8;
    let mut all_places = Vec::new();
    let mut all_rings = Vec::new();
    for j in 0..m {
        all_places.push(places[j]);
        all_rings.push(rings[j].clone());
        if j + 1 == m {
            break;
        }
        for q in 1..=BETWEEN {
            let f = q as f64 / (BETWEEN + 1) as f64;
            let v = places[j] + (places[j + 1] - places[j]) * f;
            let mut ring = ring_at(v);
            // Pulled onto the guides there: each place of the ring moved
            // as the places of the guides near it are (by a bell of
            // the ring's size about each), again until they are on them.
            let centre = ring
                .iter()
                .map(|c| Point3::new(c[0], c[1], c[2]) / c[3])
                .sum::<Point3>()
                / ring.len() as f64;
            let reach = ring
                .iter()
                .map(|c| (Point3::new(c[0], c[1], c[2]) / c[3]).distance(centre))
                .sum::<f64>()
                / ring.len() as f64;
            let reach = reach.max(size * 1e-3);
            let bell = |d: f64| (-(d / reach).powi(2)).exp();
            for _ in 0..6 {
                let pairs: Vec<(Point3, Point3)> = (0..guides.len())
                    .map(|g| {
                        let (a, b) = (anchors[g][j], anchors[g][j + 1]);
                        // (The short way around the ring from one to
                        // the other.)
                        let n = sides.len() as f64;
                        let mut d = b.0 - a.0;
                        if d > n / 2.0 {
                            d -= n;
                        } else if d < -n / 2.0 {
                            d += n;
                        }
                        let place = (a.0 + d * f).rem_euclid(n);
                        let k = (place.floor() as usize).min(sides.len() - 1);
                        let here = ring_point(sides, &ring, k, place - k as f64);
                        let wanted = chain_at(&guides[g], a.1 + (b.1 - a.1) * f);
                        (here, wanted)
                    })
                    .collect();
                let off = pairs
                    .iter()
                    .map(|(h, w)| h.distance(*w))
                    .fold(0.0, f64::max);
                if off <= size * 1e-12 {
                    break;
                }
                // The bells' heights: so that each guide's place is moved
                // onto it.
                let n = pairs.len();
                let mut matrix: Vec<Vec<f64>> = (0..n)
                    .map(|i| {
                        (0..n)
                            .map(|k| {
                                bell(pairs[i].0.distance(pairs[k].0))
                                    + if i == k { 1e-9 } else { 0.0 }
                            })
                            .collect()
                    })
                    .collect();
                let mut heights: Vec<Point3> = pairs.iter().map(|(h, w)| *w - *h).collect();
                for c in 0..n {
                    let pivot = (c..n)
                        .max_by(|x, y| matrix[*x][c].abs().total_cmp(&matrix[*y][c].abs()))
                        .unwrap_or(c);
                    matrix.swap(c, pivot);
                    heights.swap(c, pivot);
                    for r in 0..n {
                        if r != c && matrix[c][c] != 0.0 {
                            let m = matrix[r][c] / matrix[c][c];
                            let top = matrix[c].clone();
                            for (x, y) in matrix[r].iter_mut().zip(&top) {
                                *x -= m * y;
                            }
                            let h = heights[c];
                            heights[r] -= h * m;
                        }
                    }
                }
                let heights: Vec<Point3> = (0..n).map(|i| heights[i] / matrix[i][i]).collect();
                for c in &mut ring {
                    let p = Point3::new(c[0], c[1], c[2]) / c[3];
                    let by: Point3 = pairs
                        .iter()
                        .zip(&heights)
                        .map(|((h, _), height)| *height * bell(p.distance(*h)))
                        .sum();
                    let p = p + by;
                    *c = [p.x * c[3], p.y * c[3], p.z * c[3], c[3]];
                }
            }
            all_places.push(v);
            all_rings.push(ring);
        }
    }
    interpolate(&all_places, all_rings, [&ends[0], &ends[1]])
}

/// The surface through `runs`: outlines left open (the curves of each
/// one after the other, and the normal of the plane they are in), one
/// after the other. Each is taken from the end nearer where the one
/// before starts.
pub fn through_open_sections(
    id: &str,
    runs: &[(Vec<Curve3>, Point3)],
    how: &Shaping<'_>,
) -> anyhow::Result<Solid> {
    anyhow::ensure!(runs.len() >= 2, "a loft goes through two sections or more");
    let ends = |run: &[Curve3]| {
        let last = &run[run.len() - 1];
        (
            run[0].point_at(run[0].domain().0),
            last.point_at(last.domain().1),
        )
    };
    // The same way along, each as the one before.
    let mut open: Vec<Vec<Curve3>> = Vec::new();
    for (run, _) in runs {
        anyhow::ensure!(!run.is_empty(), "an outline has sides");
        let mut run = run.clone();
        if let Some(before) = open.last() {
            let (a, b) = ends(before);
            let (c, d) = ends(&run);
            if a.distance(d) + b.distance(c) < a.distance(c) + b.distance(d) {
                run.reverse();
                for side in &mut run {
                    *side = side.reversed();
                }
            }
        }
        open.push(run);
    }
    same_count(&mut open)?;
    let own = open[0].len();
    // Closed by two straight sides, out to where they go around the
    // way the loft goes: what sweeps solids can loft it.
    let middles: Vec<Point3> = open
        .iter()
        .map(|run| {
            let (a, b) = ends(run);
            (a + b) / 2.0
        })
        .collect();
    let m = open.len();
    for j in 0..m {
        let onward = if j + 1 < m {
            middles[j + 1] - middles[j]
        } else {
            middles[j] - middles[j - 1]
        };
        let (start, end) = ends(&open[j]);
        let chord = start - end;
        anyhow::ensure!(chord.length() > 0.0, "an open outline ends where it starts");
        let across = runs[j]
            .1
            .cross(chord)
            .try_normalize()
            .context("an open outline lies along the normal of its plane")?;
        let mut reach = 0.0_f64;
        let mut all = Vec::new();
        for curve in &open[j] {
            let (a, b) = curve.domain();
            for k in 0..16 {
                let p = curve.point_at(a + (b - a) * f64::from(k) / 16.0);
                reach = reach.max((p - middles[j]).dot(across).abs());
                all.push(p);
            }
        }
        all.push(end);
        let far = 2.0 * reach + chord.length();
        let turn = |apex: Point3| -> f64 {
            let mut points = all.clone();
            points.push(apex);
            let centre = points.iter().sum::<Point3>() / points.len() as f64;
            (0..points.len())
                .map(|k| (points[k] - centre).cross(points[(k + 1) % points.len()] - centre))
                .sum::<Point3>()
                .dot(onward)
        };
        let apex = [1.0, -1.0]
            .map(|side| middles[j] + across * (side * far))
            .into_iter()
            .max_by(|a, b| turn(*a).total_cmp(&turn(*b)))
            .unwrap_or(middles[j]);
        open[j].push(Curve3::line(end, apex)?);
        open[j].push(Curve3::line(apex, start)?);
    }
    let solid = lofted(id, &open, true, how)?;
    // But the sides that closed it, and its flat ends.
    let faces: Vec<usize> = (0..solid.faces.len())
        .filter(|&f| {
            let name = solid.faces[f].name.as_deref().unwrap_or("");
            name.rsplit_once("/side/e")
                .and_then(|(_, k)| k.parse::<usize>().ok())
                .is_some_and(|k| k < own)
        })
        .collect();
    Ok(crate::surface::faces_of(&solid, &faces))
}

/// The solid through the closed outlines `sections`; `aligned` if they
/// go around the same way already, each from where the one before
/// starts.
fn lofted(
    id: &str,
    sections: &[Vec<Curve3>],
    aligned: bool,
    how: &Shaping<'_>,
) -> anyhow::Result<Solid> {
    let m = sections.len();
    anyhow::ensure!(m >= 2, "a loft goes through two sections or more");
    let mut loops = Vec::new();
    for section in sections {
        loops.push(at_least_three(section)?);
    }
    let start = |curve: &Curve3| curve.point_at(curve.domain().0);
    let points = |sides: &[Curve3]| -> Vec<Point3> {
        let mut all = Vec::new();
        for side in sides {
            let (a, b) = side.domain();
            let mut piece = side.polyline(a, b);
            piece.pop();
            all.extend(piece);
        }
        all
    };
    let centres: Vec<Point3> = loops
        .iter()
        .map(|sides| {
            let all = points(sides);
            all.iter().sum::<Point3>() / all.len() as f64
        })
        .collect();
    // The loft goes on through each section: one that the next lies
    // back behind, on the side the last came from, folds it over.
    for j in 1..m - 1 {
        let all = points(&loops[j]);
        let across: Point3 = (0..all.len())
            .map(|k| (all[k] - centres[j]).cross(all[(k + 1) % all.len()] - centres[j]))
            .sum();
        let came = (centres[j] - centres[j - 1]).dot(across);
        let goes = (centres[j + 1] - centres[j]).dot(across);
        anyhow::ensure!(
            came * goes > 0.0,
            "section {} of the loft lies back behind section {}: each comes after the one before",
            j + 2,
            j + 1
        );
    }
    // All of them around the way the loft goes, the same way.
    for j in (0..m).filter(|_| !aligned) {
        let onward = if j + 1 < m {
            centres[j + 1] - centres[j]
        } else {
            centres[j] - centres[j - 1]
        };
        let all = points(&loops[j]);
        let turn: Point3 = (0..all.len())
            .map(|k| (all[k] - centres[j]).cross(all[(k + 1) % all.len()] - centres[j]))
            .sum();
        if turn.dot(onward) < 0.0 {
            loops[j].reverse();
            for side in &mut loops[j] {
                *side = side.reversed();
            }
        }
    }
    // Each from the corner nearest where the one before starts — as
    // far out from its middle for its size: a section that is a
    // smaller copy of the one before starts at the copy of its corner,
    // not at whichever of its corners lies farthest that way.
    let reach: Vec<f64> = (0..m)
        .map(|j| {
            let all = points(&loops[j]);
            all.iter().map(|p| p.distance(centres[j])).sum::<f64>() / all.len() as f64
        })
        .collect();
    for j in (1..m).filter(|_| !aligned) {
        let (before, here) = (reach[j - 1], reach[j]);
        let grown = if before > 0.0 && here > 0.0 {
            here / before
        } else {
            1.0
        };
        let wanted = centres[j] + (start(&loops[j - 1][0]) - centres[j - 1]) * grown;
        let near = |k: &usize| start(&loops[j][*k]).distance(wanted);
        let first = (0..loops[j].len())
            .min_by(|a, b| near(a).total_cmp(&near(b)))
            .unwrap_or(0);
        loops[j].rotate_left(first);
    }
    let mut size = 0.0_f64;
    for sides in &loops {
        for side in sides {
            size = size.max(start(side).distance(centres[0]));
        }
    }
    let size = size * 2.0;
    anyhow::ensure!(size > 0.0, "the sections have some size");
    same_count(&mut loops)?;
    // Each side as curves of one kind in every section: as they are, if
    // they are of one kind already; else cubics that are them closely.
    let count = loops[0].len();
    let tolerance = size * 1e-8;
    let mut sides = Vec::new();
    let mut rings: Vec<Vec<[f64; 4]>> = vec![Vec::new(); m];
    for k in 0..count {
        let units: Vec<_> = loops.iter().map(|sides| unit(&sides[k])).collect();
        let alike = units.iter().all(|u| {
            u.0 == units[0].0
                && u.1.len() == units[0].1.len()
                && u.1
                    .iter()
                    .zip(&units[0].1)
                    .all(|(a, b)| (a - b).abs() < 1e-9)
        });
        if alike {
            sides.push((units[0].0, units[0].1.clone(), units[0].2.len()));
            for (ring, unit) in rings.iter_mut().zip(&units) {
                ring.extend(unit.2.iter().map(|(p, w)| [p.x * w, p.y * w, p.z * w, *w]));
            }
            continue;
        }
        let at = |j: usize, t: f64| {
            let (a, b) = loops[j][k].domain();
            loops[j][k].point_at(a + (b - a) * t)
        };
        let mut knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let mut fitted: Vec<Vec<[f64; 4]>> = Vec::new();
        for _ in 0..12 {
            let n = knots.len() - 4;
            let mut matrix = vec![vec![0.0; n]; n];
            let mut wanted = Vec::new();
            for (i, row) in matrix.iter_mut().enumerate() {
                let t = knots[i + 1..=i + 3].iter().sum::<f64>() / 3.0;
                let (first, values) = sweep::basis(3, &knots, t);
                row[first..first + values.len()].copy_from_slice(&values);
                wanted.push(
                    (0..m)
                        .map(|j| at(j, t))
                        .map(|p| [p.x, p.y, p.z, 1.0])
                        .collect(),
                );
            }
            fitted = sweep::through(matrix, wanted).context("a side could not be fitted")?;
            let mut more = Vec::new();
            for span in 3..n {
                let (a, b) = (knots[span], knots[span + 1]);
                if b <= a {
                    continue;
                }
                let mut worst = 0.0_f64;
                for f in [0.21, 0.5, 0.79] {
                    let t = a + (b - a) * f;
                    let (first, values) = sweep::basis(3, &knots, t);
                    for j in 0..m {
                        let mut made = Point3::ZERO;
                        for (value, row) in values.iter().zip(&fitted[first..]) {
                            made += Point3::new(row[j][0], row[j][1], row[j][2]) * *value;
                        }
                        worst = worst.max(made.distance(at(j, t)));
                    }
                }
                if worst > tolerance {
                    more.push((a + b) / 2.0);
                }
            }
            if more.is_empty() || n > 200 {
                break;
            }
            knots.extend(more);
            knots.sort_by(f64::total_cmp);
        }
        sides.push((3, knots, fitted.len()));
        for (j, ring) in rings.iter_mut().enumerate() {
            ring.extend(fitted.iter().map(|row| row[j]));
        }
    }
    // Along: straight between two, smoothly through more — or as `how`
    // shapes it.
    let (degree, knots, rings) = if !how.is_plain() {
        let mut places = vec![0.0];
        for j in 1..m {
            places.push(places[j - 1] + centres[j].distance(centres[j - 1]));
        }
        let whole = places[m - 1];
        anyhow::ensure!(whole > 0.0, "the sections lie in one another");
        for place in &mut places {
            *place /= whole;
        }
        let shape: Vec<(usize, Vec<f64>, usize)> = sides.clone();
        shaped(&shape, rings, &places, &centres, how)?
    } else if m == 2 {
        (1, vec![0.0, 0.0, 1.0, 1.0], rings)
    } else {
        let degree = (m - 1).min(3);
        let mut places = vec![0.0];
        for j in 1..m {
            places.push(places[j - 1] + centres[j].distance(centres[j - 1]));
        }
        let whole = places[m - 1];
        anyhow::ensure!(whole > 0.0, "the sections lie in one another");
        for place in &mut places {
            *place /= whole;
        }
        let mut knots = vec![0.0; degree + 1];
        for j in 1..m - degree {
            knots.push(places[j..j + degree].iter().sum::<f64>() / degree as f64);
        }
        knots.extend(std::iter::repeat_n(1.0, degree + 1));
        let mut matrix = vec![vec![0.0; m]; m];
        for (row, place) in matrix.iter_mut().zip(&places) {
            let (first, values) = sweep::basis(degree, &knots, *place);
            row[first..first + values.len()].copy_from_slice(&values);
        }
        let through = sweep::through(matrix, rings).context("the sections could not be joined")?;
        (degree, knots, through)
    };
    let loft = Loft {
        counts: sides.iter().map(|s| s.2).collect(),
        sides: sides.into_iter().map(|s| (s.0, s.1)).collect(),
        degree,
        knots,
        rings,
        closed: false,
        tolerance: tolerance.max(1e-9),
    };
    let mut scratch = Topo::new();
    let body = sweep::loft(&mut scratch, id, &loft)?;
    Ok(workpiece_boolean::read(&scratch, body)?)
}

/// The helix about the axis through `origin` along `axis` that starts
/// at `from` and advances `pitch` along the axis in each of `turns`
/// turns (right-handed about the axis, or `left`-handed), its radius
/// growing by `grow` in each turn (a spiral; flat, without a pitch):
/// cubics a thirty-second of a turn each: within a billionth of its
/// radius of it, and of its direction closely enough that a section
/// carried along it is not tilted by more than that across its own
/// width (a sixteenth of a turn each left what a screw's section
/// sweeps a millionth off at its rim).
pub fn helix(
    origin: Point3,
    axis: Point3,
    from: Point3,
    pitch: f64,
    turns: f64,
    grow: f64,
    left: bool,
) -> anyhow::Result<Curve3> {
    use std::f64::consts::TAU;
    let axis = axis.try_normalize().context("a helix has an axis")?;
    anyhow::ensure!(turns > 0.0 && turns.is_finite(), "a helix makes some turns");
    let out = (from - origin).reject_from_normalized(axis);
    let radius = out.length();
    anyhow::ensure!(radius > 0.0, "a helix starts off its axis");
    let e1 = out / radius;
    let e2 = axis.cross(e1) * if left { -1.0 } else { 1.0 };
    let foot = from - out;
    let pieces = (turns * 32.0).ceil().max(1.0) as usize;
    let step = turns * TAU / pieces as f64;
    let rise = pitch / TAU;
    let wider = grow / TAU;
    anyhow::ensure!(
        radius + grow * turns > 0.0,
        "the spiral shrinks to nothing before its last turn"
    );
    let at =
        |a: f64| foot + (e1 * a.cos() + e2 * a.sin()) * (radius + wider * a) + axis * (rise * a);
    let way = |a: f64| {
        (e2 * a.cos() - e1 * a.sin()) * (radius + wider * a)
            + (e1 * a.cos() + e2 * a.sin()) * wider
            + axis * rise
    };
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    let mut points = vec![at(0.0)];
    let mut knots = vec![0.0; 4];
    for k in 0..pieces {
        let (a, b) = (step * k as f64, step * (k + 1) as f64);
        points.extend([at(a) + way(a) * handle, at(b) - way(b) * handle, at(b)]);
        knots.extend([(k + 1) as f64; 3]);
    }
    knots.push(pieces as f64);
    Ok(Curve3::new(3, knots, points, None)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_that_does_not_turn_about_its_path_keeps_to_it() {
        let (x, y, z) = (Point3::X, Point3::Y, Point3::Z);
        // Along a flat path, what is up across its plane stays up.
        let flat = crate::arcs::ellipse(x, -x, z, 2.0);
        let frames = Frames::new(&flat, flat.domain(), Upright::Least(y)).expect("frames");
        assert!(frames.last().distance(y) < 1e-12);
        // Along a path that leaves its plane it stays square to the
        // path, and is the same found from either neighbour of a place.
        let bent = Curve3::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![
                Point3::ZERO,
                z * 2.0,
                z * 4.0 + x * 2.0,
                z * 5.0 + x * 3.0 + y * 3.0,
            ],
            None,
        )
        .expect("a curve");
        let frames = Frames::new(&bent, (0.0, 1.0), Upright::Least(y)).expect("frames");
        let step = 1.0 / Frames::STEPS as f64;
        let (_, [_, before, _]) = frames.at(0.5 - step * 0.499).expect("a frame");
        let (_, [_, after, along]) = frames.at(0.5 - step * 0.501).expect("a frame");
        assert!(before.distance(after) < 1e-5);
        assert!(after.dot(along).abs() < 1e-12);
        assert!(frames.last().distance(y) > 1e-3);
    }
}
