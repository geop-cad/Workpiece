//! The geometry of a STEP file as NURBS: points and placements, curves
//! as pieces between two points, and surfaces as patches that cover the
//! face on them.
//!
//! Circles, cylinders, cones, spheres and tori are exact as rational
//! NURBS. A surface that turns around an axis is built over just the
//! angles its face needs, starting where the face does, so that a face
//! is one region of the patch's (u, v) and never runs over its edge.

use std::f64::consts::{FRAC_PI_2, TAU};

use anyhow::{Context, bail, ensure};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve3, NurbsSurface};

use crate::part21::{Arguments, File, Instance, Value};

/// A placement: an origin and three axes, orthonormal.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    pub o: Point3,
    pub x: Point3,
    pub y: Point3,
    pub z: Point3,
}

impl Frame {
    fn new(o: Point3, z: Point3, x: Point3) -> Self {
        let z = z.try_normalize().unwrap_or(Point3::Z);
        let x = x
            .reject_from_normalized(z)
            .try_normalize()
            .unwrap_or_else(|| z.any_orthonormal_vector());
        Self {
            o,
            x,
            y: z.cross(x),
            z,
        }
    }

    /// The angle of `p` around z from x, how far it is from the axis,
    /// and how far along it.
    pub(crate) fn around(&self, p: Point3) -> (f64, f64, f64) {
        let d = p - self.o;
        let (a, b) = (d.dot(self.x), d.dot(self.y));
        (b.atan2(a), a.hypot(b), d.dot(self.z))
    }
}

/// The control points (x, y of the unit circle), weights and knots on
/// `[0, 1]` of the arc over `angle` from 0, counter-clockwise.
pub(crate) fn arc_form(angle: f64) -> (Vec<(Point2, f64)>, Vec<f64>) {
    let n = ((angle / FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize;
    let step = angle / n as f64;
    let w = (step / 2.0).cos();
    let mut points = vec![(Point2::X, 1.0)];
    let mut knots = vec![0.0; 3];
    for k in 0..n {
        let (mid, end) = (step * (k as f64 + 0.5), step * (k as f64 + 1.0));
        points.push((Point2::from_angle(mid) / w, w));
        points.push((Point2::from_angle(end), 1.0));
        let knot = (k + 1) as f64 / n as f64;
        knots.extend([knot, knot]);
    }
    knots.push(1.0);
    (points, knots)
}

/// The arc around `center` from `center + x` towards `center + y`
/// (half-axes: a circle if they are as long and square) over `angle`.
pub(crate) fn arc(center: Point3, x: Point3, y: Point3, angle: f64) -> anyhow::Result<Curve3> {
    let (points, knots) = arc_form(angle);
    let (points, weights) = points
        .into_iter()
        .map(|(p, w)| (center + x * p.x + y * p.y, w))
        .unzip();
    Ok(Curve3::new(2, knots, points, Some(weights))?)
}

/// A ring without a hole: a circle of radius `minor` whose centre is
/// `major` (less than that) from the frame's z, turned around it. The
/// circle goes through the axis, so the surface has two parts: the
/// outer, swept by the arc on the circle's own side of the axis (an
/// apple), and the inner, swept by the rest (a lemon).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Spindle {
    pub(crate) frame: Frame,
    pub(crate) major: f64,
    pub(crate) minor: f64,
    /// Whether the file's surface faces into the tube on its outer
    /// part, not out of it.
    pub(crate) inward: bool,
}

impl Spindle {
    /// How far `p` is from the outer part, or from the inner.
    pub(crate) fn off(&self, p: Point3, inner: bool) -> f64 {
        let (_, rho, h) = self.frame.around(p);
        let centre = if inner { -self.major } else { self.major };
        ((rho - centre).hypot(h) - self.minor).abs()
    }

    /// The outer part, or the inner, as what its arc sweeps. The file's
    /// surface faces the other way on the inner part than on the outer
    /// (there the circle is beyond the axis), so the arc runs the other
    /// way.
    pub(crate) fn part(&self, inner: bool) -> anyhow::Result<SurfaceKind> {
        let (f, r) = (self.frame, self.minor);
        let centre = if inner { -self.major } else { self.major };
        let reach = (-centre / r).clamp(-1.0, 1.0).acos();
        let (s, c) = reach.sin_cos();
        let profile = if inner == self.inward {
            // From below around the outside to above.
            arc(
                f.o + f.x * centre,
                (f.x * c - f.z * s) * r,
                (f.x * s + f.z * c) * r,
                2.0 * reach,
            )?
        } else {
            arc(
                f.o + f.x * centre,
                (f.x * c + f.z * s) * r,
                (f.x * s - f.z * c) * r,
                2.0 * reach,
            )?
        };
        Ok(SurfaceKind::Turned { frame: f, profile })
    }
}

/// A curve of the file.
pub(crate) enum CurveKind {
    /// The line through `p` along `d`.
    Line { p: Point3, d: Point3 },
    /// The ellipse in the frame's xy plane with half-axes `a` along x
    /// and `b` along y (a circle if they are equal), run from x towards
    /// y.
    Ellipse { frame: Frame, a: f64, b: f64 },
    /// The branch of a hyperbola in the frame's xy plane around its x:
    /// `a·cosh(u)` along x and `b·sinh(u)` along y.
    Hyperbola { frame: Frame, a: f64, b: f64 },
    /// The parabola in the frame's xy plane open along its x, with its
    /// focus `f` from its vertex: `f·u²` along x and `2f·u` along y.
    Parabola { frame: Frame, f: f64 },
    /// A NURBS curve.
    Spline(Curve3),
}

/// A surface of the file.
pub(crate) enum SurfaceKind {
    /// The frame's xy plane.
    Plane(Frame),
    /// What `profile` (a curve) sweeps turning around the frame's z:
    /// cylinders, cones, spheres, tori, and surfaces of revolution. The
    /// first parameter is the angle, the second the profile's.
    Turned { frame: Frame, profile: Curve3 },
    /// A torus around the frame's z: the circle of radius `minor`
    /// whose centre is `major` from the axis, turned. The second
    /// parameter is the angle around that circle, from its outside
    /// towards z.
    Ring {
        frame: Frame,
        major: f64,
        minor: f64,
        /// Whether the file's surface faces into the tube (a circle
        /// turned around an axis can), not out of it.
        inward: bool,
    },
    /// The same torus the other way around, for a face that goes all
    /// around its tube: the first parameter is the angle around the
    /// tube, the second the angle around the frame's z, backwards.
    Tube {
        frame: Frame,
        major: f64,
        minor: f64,
        inward: bool,
    },
    /// A NURBS surface: given as one, or a curve swept along a line.
    Spline(NurbsSurface),
    /// A NURBS surface that closes around its first parameter (its two
    /// sides there are one curve, its seam): points within `reach` are
    /// on it; `inward` if its parameters were swapped to make the first
    /// the one that closes.
    Closed {
        surface: NurbsSurface,
        reach: f64,
        inward: bool,
        /// Whether it closes around its second parameter too (a ring):
        /// a face on it is then followed around both, and may lie
        /// between both seams.
        both: bool,
        /// The points it comes to where its second parameter starts
        /// and where it ends, if it does (as a ball does at its poles):
        /// there the angle around is none.
        poles: [Option<Point3>; 2],
    },
}

/// Where `p` is on `surface`, if its foot from the nearest of a net of
/// the surface's points is within `reach` of it: a quick look, before
/// the search that finds every place. The net is made once for a
/// surface and kept while the file is read.
pub(crate) fn near(surface: &NurbsSurface, p: Point3, reach: f64) -> Option<Point2> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    type Net = Rc<Vec<(Point2, Point3)>>;
    type Nets = HashMap<(usize, usize, [u64; 3]), Net>;
    thread_local! {
        static NETS: RefCell<Nets> = RefCell::new(HashMap::new());
    }
    let first = surface
        .control_points()
        .first()?
        .to_array()
        .map(f64::to_bits);
    let key = (
        std::ptr::from_ref(surface) as usize,
        surface.control_points().len(),
        first,
    );
    let net = NETS.with(|nets| {
        let mut nets = nets.borrow_mut();
        if nets.len() > 256 {
            nets.clear();
        }
        nets.entry(key)
            .or_insert_with(|| {
                let ((u0, u1), (v0, v1)) = surface.domain();
                let (nu, nv) = surface.counts();
                let (across, along) = ((nu * 3).clamp(8, 192), (nv * 3).clamp(8, 192));
                let mut net = Vec::with_capacity((across + 1) * (along + 1));
                for i in 0..=across {
                    for j in 0..=along {
                        let q = Point2::new(
                            u0 + (u1 - u0) * i as f64 / across as f64,
                            v0 + (v1 - v0) * j as f64 / along as f64,
                        );
                        net.push((q, surface.point_at(q.x, q.y)));
                    }
                }
                Rc::new(net)
            })
            .clone()
    });
    let seed = net
        .iter()
        .min_by(|a, b| a.1.distance_squared(p).total_cmp(&b.1.distance_squared(p)))?;
    let (uv, off) = workpiece_boolean::geometry::foot(surface, p, seed.0);
    (off <= reach).then_some(uv)
}

/// Where `p` is on `surface`, within `reach`: every place found by the
/// search for it, or else the foot of the nearest of a grid of its
/// points, if that is near enough.
pub(crate) fn locate(surface: &NurbsSurface, p: Point3, reach: f64) -> Vec<Point2> {
    let mut found = workpiece_boolean::geometry::on_surface(surface, p, reach);
    if !found.is_empty() {
        // The nearest first: a surface that comes by itself (a coil)
        // has places near the point that are not where it is.
        let off = |q: &Point2| surface.point_at(q.x, q.y).distance(p);
        found.sort_by(|x, y| off(x).total_cmp(&off(y)));
        return found;
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    const GRID: usize = 32;
    let mut best = (f64::INFINITY, Point2::new(u0, v0));
    for i in 0..=GRID {
        for j in 0..=GRID {
            let q = Point2::new(
                u0 + (u1 - u0) * i as f64 / GRID as f64,
                v0 + (v1 - v0) * j as f64 / GRID as f64,
            );
            let d = surface.point_at(q.x, q.y).distance(p);
            if d < best.0 {
                best = (d, q);
            }
        }
    }
    let (uv, off) = workpiece_boolean::geometry::foot(surface, p, best.1);
    if off <= reach { vec![uv] } else { Vec::new() }
}

/// Whether `surface` closes around one of its parameters, its sides
/// there within `tol` of each other: the first (`false`) or the second
/// (`true`).
pub(crate) fn closes(surface: &NurbsSurface, tol: f64) -> Option<bool> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    let at = |k: usize, lo: f64, hi: f64| lo + (hi - lo) * k as f64 / 6.0;
    let around_u = (0..=6).all(|k| {
        let v = at(k, v0, v1);
        surface.point_at(u0, v).distance(surface.point_at(u1, v)) <= tol
    }) && (0..=6).any(|k| {
        let v = at(k, v0, v1);
        surface
            .point_at(u0, v)
            .distance(surface.point_at((u0 + u1) / 2.0, v))
            > tol * 16.0
    });
    if around_u {
        return Some(false);
    }
    let around_v = (0..=6).all(|k| {
        let u = at(k, u0, u1);
        surface.point_at(u, v0).distance(surface.point_at(u, v1)) <= tol
    }) && (0..=6).any(|k| {
        let u = at(k, u0, u1);
        surface
            .point_at(u, v0)
            .distance(surface.point_at(u, (v0 + v1) / 2.0))
            > tol * 16.0
    });
    around_v.then_some(true)
}

/// The points `surface` comes to along its sides where its second
/// parameter starts and where it ends: each a point if that whole side
/// is within `within` of one.
pub(crate) fn collapses(surface: &NurbsSurface, within: f64) -> [Option<Point3>; 2] {
    let ((u0, u1), (v0, v1)) = surface.domain();
    [v0, v1].map(|v| {
        let tip = surface.point_at(u0, v);
        (1..=16)
            .all(|i| {
                let u = u0 + (u1 - u0) * f64::from(i) / 16.0;
                surface.point_at(u, v).distance(tip) <= within
            })
            .then_some(tip)
    })
}

/// Whether `surface` closes around its second parameter too: its sides
/// there within `tol` of each other, as [`closes`] has it of the first.
pub(crate) fn closes_across(surface: &NurbsSurface, tol: f64) -> bool {
    swapped(surface).is_ok_and(|turned| closes(&turned, tol) == Some(false))
}

/// `surface` with its two parameters swapped.
pub(crate) fn swapped(surface: &NurbsSurface) -> anyhow::Result<NurbsSurface> {
    let (nu, nv) = surface.counts();
    let (du, dv) = surface.degrees();
    let (ku, kv) = surface.knots();
    let mut points = Vec::with_capacity(nu * nv);
    let mut weights = Vec::with_capacity(nu * nv);
    for j in 0..nv {
        for i in 0..nu {
            points.push(surface.control_point(i, j));
            weights.push(surface.weights()[i * nv + j]);
        }
    }
    Ok(NurbsSurface::new(
        dv,
        du,
        kv.to_vec(),
        ku.to_vec(),
        nv,
        nu,
        points,
        Some(weights),
    )?)
}

/// A ring's patch around its tube: frame, major radius, the tube angle
/// it starts at and the one it covers, the angles around the axis it
/// lies between.
type Tube = (Frame, f64, f64, f64, (f64, f64));

/// A surface built for a face: the patch, and where on it a point of
/// space is, roughly (to start a search from).
pub(crate) struct Patch {
    pub surface: NurbsSurface,
    /// For a turned surface: its frame, the angle its (u, v) starts at
    /// and the angle it covers.
    turned: Option<(Frame, f64, f64, Curve3)>,
    /// For a ring around its tube: its frame and major radius, the
    /// tube angle its (u, v) starts at and the one it covers, and the
    /// angles around the axis it lies between.
    tube: Option<Tube>,
    /// For a surface closed around its first parameter: the angle its
    /// seam is at, and how near a point on it is; and, closed around
    /// its second too, the angle of its seam there.
    closed: Option<(f64, f64, Option<f64>)>,
    plane: Option<(Frame, [f64; 4])>,
}

impl Patch {
    /// The patch of a surface closed both ways, its seam around the
    /// second parameter at the angle `base` of the face's loops.
    pub(crate) fn across(mut self, base: f64) -> Self {
        if let Some(closed) = &mut self.closed {
            closed.2 = Some(base);
        }
        self
    }

    /// Roughly the (u, v) of `p`; `angle` is its angle around the axis,
    /// unwrapped the way the face's loops are, and `across` its angle
    /// around the second parameter of a surface closed both ways.
    pub(crate) fn guess(
        &self,
        p: Point3,
        angle: Option<f64>,
        across: Option<f64>,
    ) -> Option<Point2> {
        if let Some((frame, [x0, x1, y0, y1])) = self.plane {
            let d = p - frame.o;
            return Some(Point2::new(
                (d.dot(frame.x) - x0) / (x1 - x0),
                (d.dot(frame.y) - y0) / (y1 - y0),
            ));
        }
        if let Some((base, reach, seam)) = self.closed {
            let ((u0, u1), (v0, v1)) = self.surface.domain();
            let wanted = angle.map(|a| (u0 + (a - base) / TAU * (u1 - u0)).clamp(u0, u1));
            // Closed both ways, the loop says where it is both ways.
            if let (Some(u), Some(a), Some(seam)) = (wanted, across, seam) {
                let v = (v0 + (a - seam) / TAU * (v1 - v0)).clamp(v0, v1);
                return Some(Point2::new(u, v));
            }
            // Of the places it is on the surface (two, on the seam),
            // the one nearest around to where the loop is.
            let around = |u: f64| {
                let d = (u - wanted.unwrap_or(u)).abs();
                d.min((u1 - u0) - d)
            };
            let found = locate(&self.surface, p, reach)
                .into_iter()
                .min_by(|x, y| around(x.x).total_cmp(&around(y.x)))?;
            return Some(Point2::new(wanted.unwrap_or(found.x), found.y));
        }
        if let Some((frame, major, from, over, part)) = self.tube {
            let (around, rho, h) = frame.around(p);
            let mut phi = angle.unwrap_or_else(|| h.atan2(rho - major));
            if angle.is_none() {
                while phi < from - 1e-9 {
                    phi += TAU;
                }
            }
            let middle = (part.0 + part.1) / 2.0;
            let theta = around + TAU * ((middle - around) / TAU).round();
            return Some(Point2::new(
                ((phi - from) / over).clamp(0.0, 1.0),
                ((part.1 - theta) / (part.1 - part.0)).clamp(0.0, 1.0),
            ));
        }
        let (frame, from, over, profile) = self.turned.as_ref()?;
        let (raw, rho, h) = frame.around(p);
        let mut theta = angle.unwrap_or(raw);
        if angle.is_none() {
            while theta < *from - 1e-9 {
                theta += TAU;
            }
        }
        // The profile's parameter: the nearest of a few of its points,
        // in the half plane through the axis.
        let (lo, hi) = profile.domain();
        let at = |k: usize| lo + (hi - lo) * k as f64 / 64.0;
        let off = |t: f64| {
            let (_, r, z) = frame.around(profile.point_at(t));
            (r - rho).hypot(z - h)
        };
        let v = (0..=64)
            .map(at)
            .min_by(|a, b| off(*a).total_cmp(&off(*b)))
            .unwrap_or(lo);
        Some(Point2::new(((theta - from) / over).clamp(0.0, 1.0), v))
    }
}

impl SurfaceKind {
    /// The frame it turns around, if it does.
    pub(crate) fn axis(&self) -> Option<&Frame> {
        match self {
            SurfaceKind::Turned { frame, .. }
            | SurfaceKind::Ring { frame, .. }
            | SurfaceKind::Tube { frame, .. } => Some(frame),
            _ => None,
        }
    }

    /// Whether the patch built for it faces the other way than the
    /// file's surface does.
    pub(crate) fn inward(&self) -> bool {
        matches!(
            self,
            SurfaceKind::Ring { inward: true, .. }
                | SurfaceKind::Tube { inward: true, .. }
                | SurfaceKind::Closed { inward: true, .. }
        )
    }

    /// The angle of `p` the way the surface's first parameter goes
    /// around, and how far it is from where that angle is none (the
    /// axis; a ring's tube has no such place).
    pub(crate) fn turn(&self, p: Point3) -> Option<(f64, f64)> {
        match self {
            SurfaceKind::Turned { frame, .. } | SurfaceKind::Ring { frame, .. } => {
                let (angle, rho, _) = frame.around(p);
                Some((angle, rho))
            }
            SurfaceKind::Tube { frame, major, .. } => {
                let (_, rho, h) = frame.around(p);
                Some((h.atan2(rho - major), f64::INFINITY))
            }
            SurfaceKind::Closed { poles, .. } => {
                let off = poles
                    .iter()
                    .flatten()
                    .map(|pole| pole.distance(p))
                    .fold(f64::INFINITY, f64::min);
                Some((self.around_both(p)?.0, off))
            }
            _ => None,
        }
    }

    /// The angles of `p` around both parameters of a surface that
    /// closes around its first: each a whole turn from side to side.
    pub(crate) fn around_both(&self, p: Point3) -> Option<(f64, f64)> {
        let SurfaceKind::Closed { surface, reach, .. } = self else {
            return None;
        };
        let ((u0, u1), (v0, v1)) = surface.domain();
        let at = *locate(surface, p, *reach).first()?;
        Some((TAU * (at.x - u0) / (u1 - u0), TAU * (at.y - v0) / (v1 - v0)))
    }

    /// Whether it is a surface that closes around both its parameters.
    pub(crate) fn both(&self) -> bool {
        matches!(self, SurfaceKind::Closed { both: true, .. })
    }

    /// The patch of a plane that covers `[x0, x1] × [y0, y1]` of its
    /// frame.
    pub(crate) fn flat(&self, [x0, x1, y0, y1]: [f64; 4]) -> anyhow::Result<Patch> {
        let SurfaceKind::Plane(frame) = self else {
            bail!("not a plane");
        };
        let at = |x: f64, y: f64| frame.o + frame.x * x + frame.y * y;
        Ok(Patch {
            surface: NurbsSurface::bilinear(at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1))?,
            turned: None,
            tube: None,
            closed: None,
            plane: Some((*frame, [x0, x1, y0, y1])),
        })
    }

    /// The angle of `p` around the tube of a ring, from its outside
    /// towards z.
    pub(crate) fn around_tube(&self, p: Point3) -> Option<f64> {
        let (SurfaceKind::Ring { frame, major, .. } | SurfaceKind::Tube { frame, major, .. }) =
            self
        else {
            return None;
        };
        let (_, rho, h) = frame.around(p);
        Some(h.atan2(rho - major))
    }

    /// The parameter of the profile of a turned surface nearest to `p`
    /// (turned back into the profile's half plane).
    pub(crate) fn along_profile(&self, p: Point3) -> Option<f64> {
        let SurfaceKind::Turned { frame, profile } = self else {
            return None;
        };
        let (_, rho, h) = frame.around(p);
        let (lo, hi) = profile.domain();
        // A straight profile (a cylinder's, a cone's): the point's foot
        // on it, at once.
        if profile.degree() == 1 && profile.control_points().len() == 2 {
            let at = |q: Point3| {
                let (_, r, z) = frame.around(q);
                Point2::new(r, z)
            };
            let (a, b) = (at(profile.point_at(lo)), at(profile.point_at(hi)));
            let along = (Point2::new(rho, h) - a).dot(b - a) / (b - a).length_squared().max(1e-300);
            return Some(lo + (hi - lo) * along.clamp(0.0, 1.0));
        }
        // (How far out and how high a point is: its place in the half
        // plane.)
        let flat = |q: Point3| {
            let d = q - frame.o;
            Point2::new(d.dot(frame.x).hypot(d.dot(frame.y)), d.dot(frame.z))
        };
        let off = |t: f64| flat(profile.point_at(t)).distance(Point2::new(rho, h));
        // From the nearest of a few places along it — the more, the
        // more pieces it is of —
        let pieces = profile.knots().windows(2).filter(|w| w[1] > w[0]).count();
        let places = (pieces * 16).clamp(32, 128) as u32;
        let mut best = (0..=places)
            .map(|k| lo + (hi - lo) * f64::from(k) / f64::from(places))
            .min_by(|a, b| off(*a).total_cmp(&off(*b)))?;
        // down the slope to where it rests (a point of the surface is
        // on its profile, turned), if it does well inside the profile:
        let mut t = best;
        for _ in 0..12 {
            let (at, d) = profile.derivative_at(t);
            let e = at - frame.o;
            let (a, b) = (e.dot(frame.x), e.dot(frame.y));
            let r = a.hypot(b);
            if r <= 0.0 {
                break;
            }
            let slope = Point2::new(
                (a * d.dot(frame.x) + b * d.dot(frame.y)) / r,
                d.dot(frame.z),
            );
            let speed = slope.length_squared();
            if speed <= 0.0 {
                break;
            }
            let step = (Point2::new(rho, h) - Point2::new(r, e.dot(frame.z))).dot(slope) / speed;
            let next = t + step;
            if !(next > lo && next < hi) {
                break;
            }
            let rests = step.abs() <= (hi - lo) * 1e-15;
            t = next;
            if rests {
                return Some(t);
            }
        }
        // else by halves, from the place.
        let mut step = (hi - lo) / f64::from(places);
        for _ in 0..40 {
            step /= 2.0;
            for t in [best - step, best + step] {
                let t = t.clamp(lo, hi);
                if off(t) < off(best) {
                    best = t;
                }
            }
        }
        Some(best)
    }

    /// The patch of a turned surface from angle `from` over `over`, of
    /// the profile between `lo` and `hi` (a ring: of the tube between
    /// those angles).
    pub(crate) fn turned(&self, from: f64, over: f64, part: (f64, f64)) -> anyhow::Result<Patch> {
        if let SurfaceKind::Closed { surface, reach, .. } = self {
            return Ok(Patch {
                surface: surface.clone(),
                turned: None,
                tube: None,
                closed: Some((from, *reach, None)),
                plane: None,
            });
        }
        if let SurfaceKind::Tube {
            frame,
            major,
            minor,
            ..
        } = self
        {
            // The tube's circle from the angle `from`, times the arc
            // around the axis between the angles of `part`.
            let (circle, knots_u) = arc_form(over);
            let (arc, knots_v) = arc_form(part.1 - part.0);
            // Backwards around the axis: the two parameters then turn
            // the way that faces out of the ring, as its own do.
            let (sin, cos) = part.1.sin_cos();
            let x = frame.x * cos + frame.y * sin;
            let y = -frame.z.cross(x);
            let mut points = Vec::with_capacity(circle.len() * arc.len());
            let mut weights = Vec::with_capacity(points.capacity());
            for (c, wc) in &circle {
                let c = Point2::from_angle(from).rotate(*c);
                for (d, wd) in &arc {
                    points.push(
                        frame.o
                            + frame.z * (minor * c.y)
                            + (x * d.x + y * d.y) * (major + minor * c.x),
                    );
                    weights.push(wc * wd);
                }
            }
            let surface = NurbsSurface::new(
                2,
                2,
                knots_u,
                knots_v,
                circle.len(),
                arc.len(),
                points,
                Some(weights),
            )?;
            return Ok(Patch {
                surface,
                turned: None,
                tube: Some((*frame, *major, from, over, part)),
                closed: None,
                plane: None,
            });
        }
        let (frame, profile) = match self {
            SurfaceKind::Turned { frame, profile } => (frame, profile.trimmed(part.0, part.1)?),
            SurfaceKind::Ring {
                frame,
                major,
                minor,
                ..
            } => {
                let (s, c) = part.0.sin_cos();
                let out = (frame.x * c + frame.z * s) * *minor;
                let up = (frame.z * c - frame.x * s) * *minor;
                (
                    frame,
                    arc(frame.o + frame.x * *major, out, up, part.1 - part.0)?,
                )
            }
            _ => bail!("not a surface around an axis"),
        };
        let (ring, knots_u) = arc_form(over);
        let (sin, cos) = from.sin_cos();
        let x = frame.x * cos + frame.y * sin;
        let y = frame.z.cross(x);
        let mut points = Vec::with_capacity(ring.len() * profile.control_points().len());
        let mut weights = Vec::with_capacity(points.capacity());
        for (c, wc) in &ring {
            for (p, wp) in profile.control_points().iter().zip(profile.weights()) {
                // The profile's point, turned: its height stays, its
                // distance from the axis goes around.
                let (theta, rho, h) = frame.around(*p);
                let (s, k) = theta.sin_cos();
                // A profile off the frame's xz half plane starts
                // turned by its own angle.
                let (px, py) = (x * k + y * s, y * k - x * s);
                points.push(frame.o + frame.z * h + (px * c.x + py * c.y) * rho);
                weights.push(wc * wp);
            }
        }
        let surface = NurbsSurface::new(
            2,
            profile.degree(),
            knots_u,
            profile.knots().to_vec(),
            ring.len(),
            profile.control_points().len(),
            points,
            Some(weights),
        )?;
        Ok(Patch {
            surface,
            turned: Some((*frame, from, over, profile)),
            tube: None,
            closed: None,
            plane: None,
        })
    }

    /// A NURBS surface as its own patch.
    pub(crate) fn whole(&self) -> anyhow::Result<Patch> {
        let SurfaceKind::Spline(surface) = self else {
            bail!("not a NURBS surface");
        };
        Ok(Patch {
            surface: surface.clone(),
            turned: None,
            tube: None,
            closed: None,
            plane: None,
        })
    }
}

/// How far the `corners` of a body reach along `d` from `o`, and a
/// little beyond: what an endless line or surface is cut down to.
fn span(corners: &[Point3], o: Point3, d: Point3) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for c in corners {
        let at = (*c - o).dot(d);
        (lo, hi) = (lo.min(at), hi.max(at));
    }
    if lo > hi {
        return (-1.0, 1.0);
    }
    let more = (hi - lo) * 0.25 + 1e-6;
    (lo - more, hi + more)
}

/// The knots of a spline, each as often as its multiplicity says.
fn knots(multiplicities: Arguments<'_>, values: Arguments<'_>) -> Vec<f64> {
    let mut out = Vec::new();
    for (m, k) in multiplicities.values().zip(values.values()) {
        let (Value::Int(m), Some(k)) = (m, k.real()) else {
            continue;
        };
        out.extend(std::iter::repeat_n(k, m.max(0) as usize));
    }
    out
}

/// A file with its units: what one unit of length is in millimetres, and
/// one of angle in radians.
pub(crate) struct Reader<'a> {
    pub file: &'a File<'a>,
    pub length: f64,
    pub angle: f64,
    /// How near two points of the file are one, as it declares (its
    /// uncertainty of length, in millimetres), or nothing.
    pub closeness: f64,
}

impl<'a> Reader<'a> {
    /// `file` with the units it declares (millimetres and radians if it
    /// declares none).
    pub(crate) fn new(file: &'a File<'a>) -> Self {
        let mut reader = Self {
            file,
            length: 1.0,
            angle: 1.0,
            closeness: 0.0,
        };
        let prefix = |si: Arguments<'_>| match si.at(0) {
            Some(Value::Enum("MILLI")) => 1e-3,
            Some(Value::Enum("CENTI")) => 1e-2,
            Some(Value::Enum("DECI")) => 1e-1,
            Some(Value::Enum("MICRO")) => 1e-6,
            Some(Value::Enum("KILO")) => 1e3,
            _ => 1.0,
        };
        // What a unit named by a conversion is, in its base unit.
        let converted = |unit: &Instance<'_>| -> Option<f64> {
            let based = unit.arguments("CONVERSION_BASED_UNIT")?;
            let measure = file.get(based.values().find_map(|v| v.reference())?)?;
            let args = measure.parts().next()?.1;
            let value = args.real(0)?;
            let base = file.get(args.reference(1)?)?;
            Some(value * base.arguments("SI_UNIT").map_or(1.0, prefix))
        };
        let mut seen = [false; 2];
        for context in file.of_type("GLOBAL_UNIT_ASSIGNED_CONTEXT") {
            let Some(units) = context
                .arguments("GLOBAL_UNIT_ASSIGNED_CONTEXT")
                .and_then(|a| {
                    a.values().find_map(|v| match v {
                        Value::List(l) => Some(l),
                        _ => None,
                    })
                })
            else {
                continue;
            };
            for unit in units.values().filter_map(|v| file.get(v.reference()?)) {
                let si = unit.arguments("SI_UNIT");
                if unit.is("LENGTH_UNIT") && !seen[0] {
                    // In metres, then in millimetres.
                    let metres = converted(&unit).or_else(|| si.map(prefix));
                    if let Some(m) = metres {
                        reader.length = m * 1e3;
                        seen[0] = true;
                    }
                }
                if unit.is("PLANE_ANGLE_UNIT")
                    && !seen[1]
                    && let Some(radians) = converted(&unit).or(si.map(|_| 1.0))
                {
                    reader.angle = radians;
                    seen[1] = true;
                }
            }
        }
        // The least of what the file says of how near is the same.
        let declared = file
            .of_type("UNCERTAINTY_MEASURE_WITH_UNIT")
            .filter_map(|i| i.own().real(0))
            .filter(|x| x.is_finite() && *x > 0.0)
            .fold(f64::INFINITY, f64::min);
        if declared.is_finite() {
            reader.closeness = declared * reader.length;
        }
        reader
    }

    pub(crate) fn get(&self, id: u32) -> anyhow::Result<Instance<'a>> {
        self.file
            .get(id)
            .with_context(|| format!("#{id} is missing"))
    }

    /// The arguments of `instance` that are `name`'s own: those of its
    /// part of that type, or, for a simple instance of a type under it,
    /// those after `skip` inherited ones.
    fn own(instance: &Instance<'a>, name: &str, skip: usize) -> Option<(Arguments<'a>, usize)> {
        match instance.arguments(name) {
            Some(a) if instance.name() != name || instance.parts().count() > 1 => Some((a, 0)),
            Some(a) => Some((a, skip)),
            None => Some((instance.own(), skip)),
        }
    }

    fn triple(&self, id: u32) -> anyhow::Result<Point3> {
        let instance = self.get(id)?;
        let (args, skip) = Self::own(&instance, instance.name(), 1).context("no coordinates")?;
        let list = args
            .list(skip)
            .with_context(|| format!("#{id} has no coordinates"))?;
        let mut c = list.values().filter_map(|v| v.real());
        Ok(Point3::new(
            c.next().unwrap_or(0.0),
            c.next().unwrap_or(0.0),
            c.next().unwrap_or(0.0),
        ))
    }

    /// The point `#id`, in millimetres.
    pub(crate) fn point(&self, id: u32) -> anyhow::Result<Point3> {
        Ok(self.triple(id)? * self.length)
    }

    fn direction(&self, id: u32) -> anyhow::Result<Point3> {
        self.triple(id)?
            .try_normalize()
            .with_context(|| format!("direction #{id} has no length"))
    }

    /// The placement `#id` (an `AXIS2_PLACEMENT_3D`).
    pub(crate) fn frame(&self, id: u32) -> anyhow::Result<Frame> {
        let args = self.get(id)?.own();
        let o = self.point(args.reference(1).context("a placement has no location")?)?;
        let z = match args.reference(2) {
            Some(d) => self.direction(d)?,
            None => Point3::Z,
        };
        let x = match args.reference(3) {
            Some(d) => self.direction(d)?,
            None => Point3::X,
        };
        Ok(Frame::new(o, z, x))
    }

    /// The control points, degree, knots and weights of the spline
    /// curve `instance`.
    fn spline_curve(&self, instance: &Instance<'a>) -> anyhow::Result<Curve3> {
        let simple = instance.parts().count() == 1;
        let base = if simple {
            (instance.own(), 1)
        } else {
            (
                instance.arguments("B_SPLINE_CURVE").context("no spline")?,
                0,
            )
        };
        let degree = match base.0.at(base.1) {
            Some(Value::Int(d)) if d > 0 => d as usize,
            _ => bail!("#{} has no degree", instance.id),
        };
        let points: Vec<Point3> = base
            .0
            .list(base.1 + 1)
            .context("no control points")?
            .values()
            .filter_map(|v| v.reference())
            .map(|p| self.point(p))
            .collect::<Result<_, _>>()?;
        let n = points.len();
        let with = if simple {
            instance.own()
        } else {
            instance
                .arguments("B_SPLINE_CURVE_WITH_KNOTS")
                .unwrap_or(base.0)
        };
        let knots = if instance.is("B_SPLINE_CURVE_WITH_KNOTS") {
            let at = if simple { 6 } else { 0 };
            knots(
                with.list(at).context("no multiplicities")?,
                with.list(at + 1).context("no knots")?,
            )
        } else if instance.is("BEZIER_CURVE") || instance.is("QUASI_UNIFORM_CURVE") {
            // Clamped at the ends, single knots evenly between.
            let inner = n - degree - 1;
            let mut k = vec![0.0; degree + 1];
            k.extend((1..=inner).map(|i| i as f64));
            k.extend(std::iter::repeat_n(inner as f64 + 1.0, degree + 1));
            k
        } else {
            bail!("#{} is a spline of a kind not read", instance.id);
        };
        let weights = instance
            .arguments("RATIONAL_B_SPLINE_CURVE")
            .and_then(|a| a.list(0))
            .map(|l| l.values().filter_map(|v| v.real()).collect::<Vec<_>>());
        let curve = Curve3::new(degree, knots, points, weights)?;
        if clamps(curve.knots(), degree) {
            return Ok(curve);
        }
        // Knots that run on past the curve's ends: cut down to them.
        let (lo, hi) = curve.domain();
        Ok(curve.trimmed(lo, hi)?)
    }

    /// The curve `#id`.
    pub(crate) fn curve(&self, id: u32) -> anyhow::Result<CurveKind> {
        let instance = self.get(id)?;
        let args = instance.own();
        Ok(match instance.name() {
            "LINE" => {
                let p = self.point(args.reference(1).context("a line has no point")?)?;
                let vector = self.get(args.reference(2).context("a line has no direction")?)?;
                let d = self.direction(
                    vector
                        .own()
                        .reference(1)
                        .context("a vector has no direction")?,
                )?;
                CurveKind::Line { p, d }
            }
            "CIRCLE" | "ELLIPSE" => {
                let frame = self.frame(args.reference(1).context("no placement")?)?;
                let a = args.real(2).context("no radius")? * self.length;
                let b = if instance.name() == "ELLIPSE" {
                    args.real(3).context("no second radius")? * self.length
                } else {
                    a
                };
                ensure!(a > 0.0 && b > 0.0, "#{id} has no size");
                CurveKind::Ellipse { frame, a, b }
            }
            "HYPERBOLA" => {
                let frame = self.frame(args.reference(1).context("no placement")?)?;
                let a = args.real(2).context("no half-axis")? * self.length;
                let b = args.real(3).context("no second half-axis")? * self.length;
                ensure!(a > 0.0 && b > 0.0, "#{id} has no size");
                CurveKind::Hyperbola { frame, a, b }
            }
            "PARABOLA" => {
                let frame = self.frame(args.reference(1).context("no placement")?)?;
                let f = args.real(2).context("no focal distance")? * self.length;
                ensure!(f > 0.0, "#{id} has no size");
                CurveKind::Parabola { frame, f }
            }
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" => {
                return self.curve(args.reference(1).context("no curve in space")?);
            }
            _ if instance.is("B_SPLINE_CURVE") || instance.is("B_SPLINE_CURVE_WITH_KNOTS") => {
                CurveKind::Spline(self.spline_curve(&instance)?)
            }
            other => bail!("#{id} is a {other}: a curve not read"),
        })
    }

    /// The curve `#id` as one NURBS over all of it; a line as far as
    /// the body with `corners` reaches.
    fn whole_curve(&self, id: u32, corners: &[Point3]) -> anyhow::Result<Curve3> {
        Ok(match self.curve(id)? {
            CurveKind::Line { p, d } => {
                let (lo, hi) = span(corners, p, d);
                Curve3::line(p + d * lo, p + d * hi)?
            }
            CurveKind::Ellipse { frame, a, b } => arc(frame.o, frame.x * a, frame.y * b, TAU)?,
            CurveKind::Hyperbola { .. } | CurveKind::Parabola { .. } => {
                bail!("#{id} has no ends: a curve not swept")
            }
            CurveKind::Spline(c) => c,
        })
    }

    /// The surface `#id`; what has no end is cut down to as far as the
    /// body with `corners` reaches.
    /// The surface `id` as a ring without a hole, if it is one.
    pub(crate) fn spindle(&self, id: u32) -> anyhow::Result<Option<Spindle>> {
        let entity = self.get(id)?;
        let args = entity.own();
        match entity.name() {
            "TOROIDAL_SURFACE" => {
                let frame = self.frame(args.reference(1).context("no placement")?)?;
                let major = args.real(2).context("no radius")? * self.length;
                let minor = args.real(3).context("no radius")? * self.length;
                Ok(
                    (minor > 0.0 && major >= 0.0 && major <= minor * (1.0 + 1e-9)).then_some(
                        Spindle {
                            frame,
                            major,
                            minor,
                            inward: false,
                        },
                    ),
                )
            }
            "SURFACE_OF_REVOLUTION" => {
                let curve = args.reference(1).context("no curve")?;
                let axis = self.get(args.reference(2).context("no axis")?)?.own();
                let o = self.point(axis.reference(1).context("no axis point")?)?;
                let z = self.direction(axis.reference(2).context("no axis direction")?)?;
                let CurveKind::Ellipse { frame: c, a, b } = self.curve(curve)? else {
                    return Ok(None);
                };
                if (a - b).abs() > a * 1e-12 {
                    return Ok(None);
                }
                let out = (c.o - o).reject_from_normalized(z);
                let major = out.length();
                let flat = c.z.dot(z).abs() <= 1e-9 && c.z.dot(out).abs() <= 1e-9 * a;
                if !flat || major > a * (1.0 + 1e-9) {
                    return Ok(None);
                }
                // Centred on the axis (a ball), any way out across the
                // circle's plane will do.
                let out = if major > a * 1e-9 { out } else { z.cross(c.z) };
                Ok(Some(Spindle {
                    frame: Frame::new(o + z * (c.o - o).dot(z), z, out),
                    major,
                    minor: a,
                    // As a ring's: the circle runs from the outside
                    // towards z if it turns against the way the surface
                    // goes around.
                    inward: c.z.dot(z.cross(out)) > 0.0,
                }))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn surface(&self, id: u32, corners: &[Point3]) -> anyhow::Result<SurfaceKind> {
        let instance = self.get(id)?;
        let args = instance.own();
        let frame = |at: usize| self.frame(args.reference(at).context("no placement")?);
        let length =
            |at: usize| Ok::<_, anyhow::Error>(args.real(at).context("no size")? * self.length);
        Ok(match instance.name() {
            "PLANE" => SurfaceKind::Plane(frame(1)?),
            "CYLINDRICAL_SURFACE" => {
                let (f, r) = (frame(1)?, length(2)?);
                ensure!(r > 0.0, "#{id} has no radius");
                let at = |h: f64| f.o + f.x * r + f.z * h;
                let (lo, hi) = span(corners, f.o, f.z);
                SurfaceKind::Turned {
                    frame: f,
                    profile: Curve3::line(at(lo), at(hi))?,
                }
            }
            "CONICAL_SURFACE" => {
                let (f, r) = (frame(1)?, length(2)?);
                let slope = (args.real(3).context("no angle")? * self.angle).tan();
                ensure!(slope.is_finite() && slope > 0.0, "#{id} has no opening");
                // From the tip (kept just off it: the surface collapses
                // there) outwards, as far as the body reaches. But a
                // cone that opens by next to nothing has its tip far
                // beyond anything: its profile from there would have
                // the body in its last fraction, so it starts where the
                // body does.
                let tip = -r / slope;
                let at = |h: f64| f.o + f.x * (r + h * slope) + f.z * h;
                let (lo, hi) = span(corners, f.o, f.z);
                ensure!(hi > tip, "#{id} is a cone the body is not on");
                let far = lo - tip > 1e3 * (hi - lo).max(r);
                SurfaceKind::Turned {
                    frame: f,
                    profile: Curve3::line(at(if far { lo } else { tip }), at(hi))?,
                }
            }
            "SPHERICAL_SURFACE" => {
                let (f, r) = (frame(1)?, length(2)?);
                ensure!(r > 0.0, "#{id} has no radius");
                // The meridian from the south pole to the north.
                SurfaceKind::Turned {
                    frame: f,
                    profile: arc(f.o, f.z * -r, f.x * r, std::f64::consts::PI)?,
                }
            }
            "TOROIDAL_SURFACE" => {
                let (f, major, minor) = (frame(1)?, length(2)?, length(3)?);
                ensure!(minor > 0.0 && major >= 0.0, "#{id} has no size");
                if major <= minor * (1.0 + 1e-9) {
                    // A ring without a hole, its tube reaching the axis
                    // or through it: its outer part, until a face says
                    // it is on the inner.
                    Spindle {
                        frame: f,
                        major,
                        minor,
                        inward: false,
                    }
                    .part(false)?
                } else {
                    SurfaceKind::Ring {
                        frame: f,
                        major,
                        minor,
                        inward: false,
                    }
                }
            }
            "DEGENERATE_TOROIDAL_SURFACE" => {
                // A ring whose tube goes through the axis, with which of
                // its two parts is meant: the outer (an apple) or the
                // inner (a lemon).
                let (f, major, minor) = (frame(1)?, length(2)?, length(3)?);
                ensure!(
                    minor > major && major >= 0.0,
                    "#{id} is no ring through its axis"
                );
                let outer = args.flag(4).unwrap_or(true);
                let reach = (-major / minor).acos();
                let (from, over) = if outer {
                    (-reach, 2.0 * reach)
                } else {
                    (reach, TAU - 2.0 * reach)
                };
                let (s, c) = from.sin_cos();
                SurfaceKind::Turned {
                    frame: f,
                    profile: arc(
                        f.o + f.x * major,
                        (f.x * c + f.z * s) * minor,
                        (f.z * c - f.x * s) * minor,
                        over,
                    )?,
                }
            }
            "SURFACE_OF_REVOLUTION" => {
                let curve = args.reference(1).context("no curve")?;
                let axis = self.get(args.reference(2).context("no axis")?)?.own();
                let o = self.point(axis.reference(1).context("no axis point")?)?;
                let z = self.direction(axis.reference(2).context("no axis direction")?)?;
                // A circle in a plane through the axis and clear of it,
                // turned: a ring.
                if let CurveKind::Ellipse { frame: c, a, b } = self.curve(curve)?
                    && (a - b).abs() <= a * 1e-12
                {
                    let out = (c.o - o).reject_from_normalized(z);
                    let major = out.length();
                    let in_plane = c.z.dot(z).abs() <= 1e-9 && c.z.dot(out).abs() <= 1e-9 * major;
                    if in_plane && major > a * (1.0 + 1e-9) {
                        return Ok(SurfaceKind::Ring {
                            frame: Frame::new(o + z * (c.o - o).dot(z), z, out),
                            major,
                            minor: a,
                            // The circle runs from the outside towards
                            // z, as a ring's tube does, if it turns
                            // against the way the surface goes around.
                            inward: c.z.dot(z.cross(out)) > 0.0,
                        });
                    }
                    // A circle through the axis, in a plane through it:
                    // a ring without a hole.
                    if let Some(spindle) = self.spindle(id)? {
                        return spindle.part(false);
                    }
                }
                let profile = self.whole_curve(curve, corners)?;
                // The angle is counted from where the profile is.
                let (lo, hi) = profile.domain();
                let towards = profile.point_at((lo + hi) / 2.0) - o;
                SurfaceKind::Turned {
                    frame: Frame::new(o, z, towards),
                    profile,
                }
            }
            "SURFACE_OF_LINEAR_EXTRUSION" => {
                let profile = self.whole_curve(args.reference(1).context("no curve")?, corners)?;
                let vector = self.get(args.reference(2).context("no direction")?)?;
                let d = self.direction(vector.own().reference(1).context("no direction")?)?;
                // As far along the direction as the body reaches from
                // any point of the profile.
                let (body_lo, body_hi) = span(corners, Point3::ZERO, d);
                let (own_lo, own_hi) = span(profile.control_points(), Point3::ZERO, d);
                let (lo, hi) = (body_lo - own_hi, body_hi - own_lo);
                let points = profile
                    .control_points()
                    .iter()
                    .flat_map(|p| [*p + d * lo, *p + d * hi])
                    .collect();
                let weights = profile.weights().iter().flat_map(|w| [*w, *w]).collect();
                SurfaceKind::Spline(NurbsSurface::new(
                    profile.degree(),
                    1,
                    profile.knots().to_vec(),
                    vec![lo, lo, hi, hi],
                    profile.control_points().len(),
                    2,
                    points,
                    Some(weights),
                )?)
            }
            _ if instance.is("B_SPLINE_SURFACE") || instance.is("B_SPLINE_SURFACE_WITH_KNOTS") => {
                SurfaceKind::Spline(self.spline_surface(&instance)?)
            }
            other => bail!("#{id} is a {other}: a surface not read"),
        })
    }

    fn spline_surface(&self, instance: &Instance<'a>) -> anyhow::Result<NurbsSurface> {
        let simple = instance.parts().count() == 1;
        let (base, skip) = if simple {
            (instance.own(), 1)
        } else {
            (
                instance
                    .arguments("B_SPLINE_SURFACE")
                    .context("no spline")?,
                0,
            )
        };
        let degree = |at: usize| match base.at(skip + at) {
            Some(Value::Int(d)) if d > 0 => Ok(d as usize),
            _ => bail!("#{} has no degree", instance.id),
        };
        let (pu, pv) = (degree(0)?, degree(1)?);
        let rows = base.list(skip + 2).context("no control points")?;
        let mut points = Vec::new();
        let mut nu = 0;
        for row in rows.values() {
            let Value::List(row) = row else {
                continue;
            };
            nu += 1;
            for p in row.values().filter_map(|v| v.reference()) {
                points.push(self.point(p)?);
            }
        }
        ensure!(
            nu > 0 && points.len() % nu == 0,
            "#{} has a ragged net",
            instance.id
        );
        let nv = points.len() / nu;
        ensure!(
            instance.is("B_SPLINE_SURFACE_WITH_KNOTS"),
            "#{} is a spline surface of a kind not read",
            instance.id
        );
        let (with, at) = if simple {
            (instance.own(), 8)
        } else {
            (
                instance
                    .arguments("B_SPLINE_SURFACE_WITH_KNOTS")
                    .context("no knots")?,
                0,
            )
        };
        let list = |k: usize| with.list(at + k).context("no knots");
        let (ku, kv) = (knots(list(0)?, list(2)?), knots(list(1)?, list(3)?));
        let weights = instance
            .arguments("RATIONAL_B_SPLINE_SURFACE")
            .and_then(|a| a.list(0))
            .map(|rows| {
                rows.values()
                    .filter_map(|r| match r {
                        Value::List(r) => Some(r),
                        _ => None,
                    })
                    .flat_map(|r| r.values().filter_map(|v| v.real()).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            });
        clamped(&NurbsSurface::new(pu, pv, ku, kv, nu, nv, points, weights)?)
    }
}

/// Whether the knots of a spline of `degree` start and end as many
/// times over as its curve needs to start and end at its first and last
/// control points.
fn clamps(knots: &[f64], degree: usize) -> bool {
    let n = knots.len();
    n > 2 * degree + 1 && knots[0] == knots[degree] && knots[n - 1] == knots[n - 1 - degree]
}

/// `surface` over the parameters it is defined on, with knots that
/// start and end there (a file may give a surface that closes around
/// with knots that run on past its ends).
fn clamped(surface: &NurbsSurface) -> anyhow::Result<NurbsSurface> {
    let (du, dv) = surface.degrees();
    let (ku, kv) = surface.knots();
    if clamps(ku, du) && clamps(kv, dv) {
        return Ok(surface.clone());
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (nu, nv) = surface.counts();
    // Row by row along the second parameter, then column by column
    // along the first: each a curve, cut down to the surface's ends.
    let mut rows: Vec<Curve3> = Vec::with_capacity(nu);
    for i in 0..nu {
        let points = (0..nv).map(|j| surface.control_point(i, j)).collect();
        let weights = (0..nv).map(|j| surface.weights()[i * nv + j]).collect();
        rows.push(Curve3::new(dv, kv.to_vec(), points, Some(weights))?.trimmed(v0, v1)?);
    }
    let kv = rows[0].knots().to_vec();
    let nv = rows[0].control_points().len();
    let mut columns: Vec<Curve3> = Vec::with_capacity(nv);
    for j in 0..nv {
        let points = rows.iter().map(|r| r.control_points()[j]).collect();
        let weights = rows.iter().map(|r| r.weights()[j]).collect();
        columns.push(Curve3::new(du, ku.to_vec(), points, Some(weights))?.trimmed(u0, u1)?);
    }
    let ku = columns[0].knots().to_vec();
    let nu = columns[0].control_points().len();
    let mut points = Vec::with_capacity(nu * nv);
    let mut weights = Vec::with_capacity(nu * nv);
    for i in 0..nu {
        for column in &columns {
            points.push(column.control_points()[i]);
            weights.push(column.weights()[i]);
        }
    }
    Ok(NurbsSurface::new(
        du,
        dv,
        ku,
        kv,
        nu,
        nv,
        points,
        Some(weights),
    )?)
}

/// The piece of `kind` from the point `from` to the point `to` (the
/// whole of a closed curve if `closed`), run the curve's own way if
/// `same` and against it otherwise: a curve that starts at `from`.
pub(crate) fn edge_curve(
    kind: &CurveKind,
    from: Point3,
    to: Point3,
    closed: bool,
    same: bool,
    tol: f64,
) -> anyhow::Result<(Curve3, (f64, f64))> {
    match kind {
        CurveKind::Line { p, d } => {
            let on = |q: Point3| *p + *d * (q - *p).dot(*d);
            let curve = Curve3::line(on(from), on(to))?;
            Ok((curve, (0.0, 1.0)))
        }
        CurveKind::Ellipse { frame, a, b } => {
            let angle = |q: Point3| {
                let d = q - frame.o;
                (d.dot(frame.y) / b).atan2(d.dot(frame.x) / a)
            };
            let (start, end) = (angle(from), angle(to));
            let turn = if same { end - start } else { start - end };
            let mut over = turn.rem_euclid(TAU);
            if closed || over < 1e-9 {
                over = TAU;
            }
            // From the start, the way the edge runs.
            let (s, c) = start.sin_cos();
            let x = frame.x * (a * c) + frame.y * (b * s);
            let y = (frame.y * (b * c) - frame.x * (a * s)) * if same { 1.0 } else { -1.0 };
            Ok((arc(frame.o, x, y, over)?, (0.0, 1.0)))
        }
        // A stretch of a conic is one rational parabola: its ends, and
        // where the tangents at them meet.
        CurveKind::Hyperbola { frame, a, b } => {
            let at = |q: Point3| ((q - frame.o).dot(frame.y) / b).asinh();
            let (u0, u1) = (at(from), at(to));
            ensure!(u0 != u1, "an edge of no length on a hyperbola");
            let on = |u: f64| frame.x * (a * u.cosh()) + frame.y * (b * u.sinh());
            let w = ((u1 - u0) / 2.0).cosh();
            let points = vec![
                frame.o + on(u0),
                frame.o + on((u0 + u1) / 2.0) / w,
                frame.o + on(u1),
            ];
            let curve = Curve3::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                points,
                Some(vec![1.0, w, 1.0]),
            )?;
            Ok((curve, (0.0, 1.0)))
        }
        CurveKind::Parabola { frame, f } => {
            let at = |q: Point3| (q - frame.o).dot(frame.y) / (2.0 * f);
            let (u0, u1) = (at(from), at(to));
            ensure!(u0 != u1, "an edge of no length on a parabola");
            let on = |uu: f64, u: f64| frame.o + frame.x * (f * uu) + frame.y * (2.0 * f * u);
            let points = vec![
                on(u0 * u0, u0),
                on(u0 * u1, (u0 + u1) / 2.0),
                on(u1 * u1, u1),
            ];
            let curve = Curve3::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], points, None)?;
            Ok((curve, (0.0, 1.0)))
        }
        CurveKind::Spline(curve) => {
            let curve = if same {
                curve.clone()
            } else {
                curve.reversed()
            };
            let (lo, hi) = curve.domain();
            if closed {
                return Ok((curve, (lo, hi)));
            }
            // Where an end is on the curve: within the tolerance, or
            // else the nearest of its points, if that is near.
            let extent = {
                let c = curve.control_points();
                c.iter().fold(0.0_f64, |m, p| m.max(p.distance(c[0])))
            };
            let at = |q: Point3| {
                let exact = workpiece_boolean::geometry::on_curve(&curve, (lo, hi), q, tol);
                if let Some(&t) = exact.first() {
                    return Some(t);
                }
                let off = |t: f64| curve.point_at(t).distance(q);
                let mut best = (0..=256)
                    .map(|k| lo + (hi - lo) * f64::from(k) / 256.0)
                    .min_by(|a, b| off(*a).total_cmp(&off(*b)))?;
                let mut step = (hi - lo) / 256.0;
                for _ in 0..50 {
                    step /= 2.0;
                    for t in [best - step, best + step] {
                        let t = t.clamp(lo, hi);
                        if off(t) < off(best) {
                            best = t;
                        }
                    }
                }
                // (Near: a small part of the curve's size — or, on a
                // curve that is itself next to nothing, the tolerance.)
                (off(best) <= (extent * 5e-2).max(tol)).then_some(best)
            };
            let (Some(a), Some(b)) = (at(from), at(to)) else {
                bail!("an edge's ends are not on its curve");
            };
            // An end at the start of a closed curve is at its end too.
            let b = if b <= a + (hi - lo) * 1e-12 { hi } else { b };
            ensure!(b > a, "an edge runs over the end of its curve");
            Ok((curve, (a, b)))
        }
    }
}
