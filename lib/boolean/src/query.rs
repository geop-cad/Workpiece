//! Questions asked of a solid: is a point of a surface inside a face, is
//! a point of space on the solid's boundary, inside it or outside.
//!
//! A face is the part of its surface inside its loops, so "inside the
//! face" is asked in (u, v): a ray from the point crosses the loops'
//! curves an odd number of times. The crossings are roots of polynomials
//! in Bernstein form, found by clipping, so no polygon stands in for a
//! curve. A ray that grazes a curve or runs through the end of one tells
//! nothing; another is tried.
//!
//! Inside the solid is the same one dimension up: a ray from the point
//! crosses the faces an odd number of times, the crossings found by the
//! contact search of a segment with each surface.

use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::NurbsSurface;

use crate::Refusal;
use crate::bezier::{Bounds, Piece, bernstein, curve_pieces, roots, surface_pieces};
use crate::model::{Face, Solid, Walk};
use crate::solve::{Along, At, Contact, Over, contacts};

/// Directions of rays in (u, v): none along an axis or a diagonal.
const RAYS: [(f64, f64); 6] = [
    (0.921_060_994, 0.389_418_342),
    (-0.362_357_754, 0.932_039_086),
    (0.540_302_306, -0.841_470_985),
    (-0.989_992_497, -0.141_120_008),
    (0.070_737_202, 0.997_494_987),
    (0.753_902_254, 0.656_986_599),
];

/// Directions of rays in space.
const RAYS3: [[f64; 3]; 8] = [
    [0.267_261_242, 0.534_522_484, 0.801_783_726],
    [-0.842_151_921, 0.336_860_768, 0.421_075_961],
    [0.455_842_306, -0.683_763_459, 0.569_802_882],
    [0.384_900_179, 0.769_800_359, -0.508_068_237],
    [-0.627_963_030, -0.502_370_424, 0.594_651_780],
    [0.811_107_106, -0.324_442_842, -0.486_664_263],
    [-0.196_116_135, 0.588_348_405, -0.784_464_541],
    [-0.512_989_176, -0.769_483_764, -0.380_058_475],
];

/// The loops of a face as Bézier pieces in (u, v).
#[derive(Debug, Clone)]
pub(crate) struct Trim {
    /// The pieces of every loop's curves.
    pub pieces: Vec<Piece>,
    /// The box around them: `(u, v)` ranges.
    pub range: [(f64, f64); 2],
    /// The box around each piece: its least and greatest u, then v.
    /// What a ray passes by is told from these, without the piece.
    boxes: Vec<[f64; 4]>,
}

/// Where a ray from a point crosses the pieces.
enum Crossings {
    /// This many times.
    Count(usize),
    /// It grazes a piece or runs through the end of one.
    Unclear,
    /// The point is on a piece.
    On,
}

impl Trim {
    /// The loops of `pieces`.
    pub(crate) fn new(pieces: Vec<Piece>) -> Self {
        let mut range = [(f64::INFINITY, f64::NEG_INFINITY); 2];
        for h in pieces.iter().flat_map(|p| &p.cp) {
            for k in 0..2 {
                let x = h[k] / h[3];
                range[k] = (range[k].0.min(x), range[k].1.max(x));
            }
        }
        let boxes = pieces
            .iter()
            .map(|p| {
                let mut b = [
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                ];
                for h in &p.cp {
                    let (u, v) = (h[0] / h[3], h[1] / h[3]);
                    b = [b[0].min(u), b[1].max(u), b[2].min(v), b[3].max(v)];
                }
                b
            })
            .collect();
        Self {
            pieces,
            range,
            boxes,
        }
    }

    /// The loops of `face`.
    pub(crate) fn of(solid: &Solid, face: &Face) -> Self {
        let mut pieces = Vec::new();
        for step in face.loops.iter().flatten() {
            let (a, b) = solid.span(step);
            pieces.extend(curve_pieces(&*step.pcurve, a.min(b), a.max(b)));
        }
        Self::new(pieces)
    }

    fn size(&self) -> f64 {
        let [u, v] = self.range;
        libm::hypot(u.1 - u.0, v.1 - v.0).max(f64::MIN_POSITIVE)
    }

    fn crossings(&self, q: Point2, d: Point2) -> Crossings {
        let near = self.size() * 1e-12;
        let mut count = 0;
        for (piece, b) in self.pieces.iter().zip(&self.boxes) {
            // A piece whose box is all to one side of the ray, or all
            // behind where it starts, is not crossed.
            let corners = [(b[0], b[2]), (b[1], b[2]), (b[1], b[3]), (b[0], b[3])];
            let side = corners.map(|(u, v)| (u - q.x) * d.y - (v - q.y) * d.x);
            let ahead = corners.map(|(u, v)| (u - q.x) * d.x + (v - q.y) * d.y);
            if side.iter().all(|&x| x > near)
                || side.iter().all(|&x| x < -near)
                || ahead.iter().all(|&x| x < -near)
            {
                continue;
            }
            // How far each control point is to the side of the ray.
            let c: Vec<f64> = piece
                .cp
                .iter()
                .map(|h| (h[0] - q.x * h[3]) * d.y - (h[1] - q.y * h[3]) * d.x)
                .collect();
            if c.iter().all(|&x| x > 0.0) || c.iter().all(|&x| x < 0.0) {
                continue;
            }
            let Some(found) = roots(&c, near) else {
                // The piece lies along the ray.
                let (a, b) = (piece.at(0, 0), piece.at(piece.n[0] - 1, 0));
                let along = |p: Point3| (Point2::new(p.x, p.y) - q).dot(d);
                if along(a).min(along(b)) <= near && along(a).max(along(b)) >= -near {
                    return Crossings::On;
                }
                return Crossings::Unclear;
            };
            for t in found {
                let p = piece.eval([t, 0.0]);
                let ahead = (Point2::new(p.x, p.y) - q).dot(d);
                if ahead.abs() <= near {
                    return Crossings::On;
                }
                if ahead < 0.0 {
                    continue;
                }
                if !(1e-9..=1.0 - 1e-9).contains(&t) {
                    return Crossings::Unclear;
                }
                let (before, after) = (bernstein(&c, t - 1e-6), bernstein(&c, t + 1e-6));
                if before * after < 0.0 {
                    count += 1;
                } else if before == 0.0 || after == 0.0 {
                    return Crossings::Unclear;
                }
            }
        }
        Crossings::Count(count)
    }

    /// Whether `q` is inside the loops; `None` if it is on one of them
    /// (or too near to tell).
    pub(crate) fn holds(&self, q: Point2) -> Option<bool> {
        if self.pieces.is_empty() {
            return Some(true);
        }
        for (x, y) in RAYS {
            match self.crossings(q, Point2::new(x, y)) {
                Crossings::Count(n) => return Some(n % 2 == 1),
                Crossings::On => return None,
                Crossings::Unclear => {}
            }
        }
        None
    }

    /// A point well inside the loops: the middle of the widest stretch
    /// inside along a few lines of constant v, with half its width.
    pub(crate) fn inner(&self) -> Option<(Point2, f64)> {
        self.inner_points().into_iter().next()
    }

    /// Points inside the loops, the ones farthest from them first: the
    /// middles of the stretches inside along a few lines of constant v,
    /// each with half its stretch's width.
    pub(crate) fn inner_points(&self) -> Vec<(Point2, f64)> {
        let (v0, v1) = self.range[1];
        let mut found: Vec<(Point2, f64)> = Vec::new();
        let mut lines = 0;
        for f in [
            0.5, 0.37, 0.63, 0.24, 0.76, 0.43, 0.57, 0.11, 0.89, 0.31, 0.69,
        ] {
            let v = v0 + (v1 - v0) * f;
            let Some(mut us) = self.along(v) else {
                continue;
            };
            us.sort_by(f64::total_cmp);
            if us.len() % 2 == 1 {
                continue;
            }
            for pair in us.chunks(2) {
                let width = (pair[1] - pair[0]) / 2.0;
                if width > 0.0 {
                    // Off the middle too: a middle is where things
                    // made alike tend to touch.
                    found.push((Point2::new((pair[0] + pair[1]) / 2.0, v), width));
                    found.push((Point2::new(pair[0] + width * 0.618, v), width * 0.6));
                }
            }
            lines += 1;
            if lines >= 5 && !found.is_empty() {
                break;
            }
        }
        found.sort_by(|a, b| b.1.total_cmp(&a.1));
        found
    }

    /// The u at which the line of constant `v` crosses the pieces;
    /// `None` if it grazes one or runs through the end of one.
    fn along(&self, v: f64) -> Option<Vec<f64>> {
        let mut us = Vec::new();
        for piece in &self.pieces {
            let c: Vec<f64> = piece.cp.iter().map(|h| h[1] - v * h[3]).collect();
            if c.iter().all(|&x| x > 0.0) || c.iter().all(|&x| x < 0.0) {
                continue;
            }
            for t in roots(&c, self.size() * 1e-12)? {
                if !(1e-9..=1.0 - 1e-9).contains(&t) {
                    return None;
                }
                if bernstein(&c, t - 1e-6) * bernstein(&c, t + 1e-6) >= 0.0 {
                    return None;
                }
                us.push(piece.eval([t, 0.0]).x);
            }
        }
        Some(us)
    }
}

/// The unit normal `∂u × ∂v` of `surface` at `uv`; where the surface
/// collapses (a pole), the normal just beside it.
pub(crate) fn normal(surface: &NurbsSurface, uv: Point2) -> Point3 {
    let (_, du, dv) = surface.derivatives_at(uv.x, uv.y);
    if let Some(n) = du.cross(dv).try_normalize()
        && du.cross(dv).length() > 1e-9 * du.length().max(dv.length()).powi(2)
    {
        return n;
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let mid = Point2::new((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    for step in [1e-6, 1e-4, 1e-2] {
        let q = uv + (mid - uv) * step;
        let (_, du, dv) = surface.derivatives_at(q.x, q.y);
        if let Some(n) = du.cross(dv).try_normalize() {
            return n;
        }
    }
    Point3::Z
}

/// A face made ready for questions.
#[derive(Debug, Clone)]
pub(crate) struct FaceData {
    /// Its surface's Bézier patches, as far as its loops reach.
    pub patches: Vec<Piece>,
    /// The box around them.
    pub bounds: Bounds,
    /// Its loops.
    pub trim: Trim,
    /// The (u, v) box the loops lie in.
    pub uv: [(f64, f64); 2],
}

impl FaceData {
    /// `face` of `solid`, ready.
    pub(crate) fn new(solid: &Solid, face: &Face) -> Self {
        Self::over(&face.surface, Trim::of(solid, face))
    }

    /// The part of `surface` inside `trim`, ready.
    pub(crate) fn over(surface: &NurbsSurface, trim: Trim) -> Self {
        let (du, dv) = surface.domain();
        let uv = if trim.pieces.is_empty() {
            [du, dv]
        } else {
            let fit = |r: (f64, f64), d: (f64, f64)| {
                let pad = (d.1 - d.0) * 1e-9;
                let (a, b) = ((r.0 - pad).max(d.0), (r.1 + pad).min(d.1));
                if a < b { (a, b) } else { d }
            };
            [fit(trim.range[0], du), fit(trim.range[1], dv)]
        };
        let patches = surface_pieces(surface, uv[0], uv[1]);
        let bounds = patches
            .iter()
            .fold(Bounds::EMPTY, |b, p| b.union(p.bounds()));
        Self {
            patches,
            bounds,
            trim,
            uv,
        }
    }
}

/// A solid made ready for questions.
#[derive(Debug, Clone)]
pub(crate) struct SolidData {
    /// Its faces.
    pub faces: Vec<FaceData>,
    /// Its edges' Bézier pieces.
    pub edges: Vec<Vec<Piece>>,
    /// The box around it.
    pub bounds: Bounds,
}

impl SolidData {
    /// `solid`, ready.
    pub(crate) fn new(solid: &Solid) -> Self {
        let faces: Vec<FaceData> = solid
            .faces
            .iter()
            .map(|f| FaceData::new(solid, f))
            .collect();
        let edges = solid
            .edges
            .iter()
            .map(|e| curve_pieces(&*e.curve, e.range.0, e.range.1))
            .collect();
        let bounds = faces.iter().fold(Bounds::EMPTY, |b, f| b.union(f.bounds));
        Self {
            faces,
            edges,
            bounds,
        }
    }
}

/// Where `p` is on `face` (with its data), within `tol`: its (u, v), if
/// it is inside the loops or on them.
pub(crate) fn on_face(face: &Face, data: &FaceData, p: Point3, tol: f64) -> Option<Point2> {
    if !data.bounds.holds(p, tol) {
        return None;
    }
    let found = contacts(
        &At(p),
        &[Piece::point(p)],
        &Over(&face.surface, data.uv),
        &data.patches,
        tol,
    )
    .ok()?;
    found
        .iter()
        .map(|c| Point2::new(c.b[0], c.b[1]))
        .find(|&uv| data.trim.holds(uv) != Some(false))
}

/// How many times the ray from `origin` along `d` (a unit vector)
/// crosses the faces `among` of `solid` farther than `skip` away;
/// `None` if it grazes one or passes too near an edge to tell.
fn crossings(
    solid: &Solid,
    data: &SolidData,
    among: &[usize],
    origin: Point3,
    d: Point3,
    skip: f64,
) -> Option<usize> {
    let size = data.bounds.size().max(f64::MIN_POSITIVE);
    let reach = (data.bounds.lo - origin)
        .abs()
        .max((data.bounds.hi - origin).abs())
        .length()
        + size * 0.1;
    crossings_within(solid, data, among, origin, d, reach, skip)
}

/// How many times the stretch from `from` to `to` crosses the faces
/// `among` of `solid`; `None` if it grazes one or passes too near an
/// edge to tell, or has no length.
pub(crate) fn crossings_between(
    solid: &Solid,
    data: &SolidData,
    among: &[usize],
    from: Point3,
    to: Point3,
) -> Option<usize> {
    let reach = from.distance(to);
    let d = (to - from).try_normalize()?;
    crossings_within(solid, data, among, from, d, reach, -1.0)
}

/// [`crossings`], of the ray as far as `reach` from its origin.
fn crossings_within(
    solid: &Solid,
    data: &SolidData,
    among: &[usize],
    origin: Point3,
    d: Point3,
    reach: f64,
    skip: f64,
) -> Option<usize> {
    let size = data.bounds.size().max(f64::MIN_POSITIVE);
    let end = origin + d * reach;
    let segment = workpiece_core::world::topo::Curve3::line(origin, end).ok()?;
    let ray = [Piece::segment(origin, end)];
    let ray_bounds = ray[0].bounds();
    let fine = size * 1e-11;
    let mut count = 0;
    for &f in among {
        let (face, fd) = (&solid.faces[f], &data.faces[f]);
        // (Twice the nearness: the ray has to pass no farther from the
        // face's box than what counts as meeting the face.)
        if !fd.bounds.meets(&ray_bounds, fine) || !fd.bounds.meets_stretch(origin, end, 2.0 * fine)
        {
            continue;
        }
        let hits: Vec<Contact> = contacts(
            &Along(&segment, (0.0, 1.0)),
            &ray,
            &Over(&face.surface, fd.uv),
            &fd.patches,
            fine,
        )
        .ok()?;
        for hit in hits {
            let uv = Point2::new(hit.b[0], hit.b[1]);
            let along = hit.a[0] * reach;
            // A long contact: the ray runs in the surface.
            if (hit.box_a[0].1 - hit.box_a[0].0) * reach > size * 1e-6 {
                if fd.trim.holds(uv) == Some(false) {
                    continue;
                }
                return None;
            }
            let inside = fd.trim.holds(uv);
            if inside == Some(false) {
                continue;
            }
            if along <= skip {
                if along.abs() <= skip {
                    continue;
                }
                return None;
            }
            inside?;
            if normal(&face.surface, uv).dot(d).abs() < 1e-4 {
                return None;
            }
            // Near an edge the faces on both sides would each count it,
            // or neither.
            let margin = (face.tolerance * 16.0).max(size * 1e-9);
            let near_edge = face.loops.iter().flatten().any(|step| {
                let Walk::Edge { edge, .. } = step.walk else {
                    return false;
                };
                let e = &solid.edges[edge];
                let pieces = &data.edges[edge];
                let reach = margin + e.tolerance;
                pieces.iter().any(|p| p.bounds().holds(hit.pb, reach))
                    && contacts(
                        &At(hit.pb),
                        &[Piece::point(hit.pb)],
                        &Along(&e.curve, e.range),
                        pieces,
                        reach,
                    )
                    .is_ok_and(|c| !c.is_empty())
            });
            if near_edge {
                return None;
            }
            count += 1;
        }
    }
    Some(count)
}

/// How far along the ray from `origin` along `d` (a unit vector) it
/// meets each face of `solid`: every meeting within a face or on its
/// edge, a point on an edge once for each face there. `None` if a
/// meeting could not be looked for.
pub(crate) fn meetings(
    solid: &Solid,
    data: &SolidData,
    origin: Point3,
    d: Point3,
) -> Option<Vec<f64>> {
    let size = data.bounds.size().max(f64::MIN_POSITIVE);
    let reach = (data.bounds.lo - origin)
        .abs()
        .max((data.bounds.hi - origin).abs())
        .length()
        + size * 0.1;
    let end = origin + d * reach;
    let segment = workpiece_core::world::topo::Curve3::line(origin, end).ok()?;
    let ray = [Piece::segment(origin, end)];
    let ray_bounds = ray[0].bounds();
    let fine = size * 1e-11;
    let mut all = Vec::new();
    for (face, fd) in solid.faces.iter().zip(&data.faces) {
        // (Twice the nearness: the ray has to pass no farther from the
        // face's box than what counts as meeting the face.)
        if !fd.bounds.meets(&ray_bounds, fine) || !fd.bounds.meets_stretch(origin, end, 2.0 * fine)
        {
            continue;
        }
        let hits: Vec<Contact> = contacts(
            &Along(&segment, (0.0, 1.0)),
            &ray,
            &Over(&face.surface, fd.uv),
            &fd.patches,
            fine,
        )
        .ok()?;
        for hit in hits {
            let uv = Point2::new(hit.b[0], hit.b[1]);
            if fd.trim.holds(uv) != Some(false) {
                all.push(hit.a[0] * reach);
            }
        }
    }
    Some(all)
}

/// Whether `p`, a point on none of the faces `among` of `solid`, is
/// inside the shells they make: a ray from it crosses them an odd number
/// of times. `None` if every ray tried grazes a face or passes too near
/// an edge.
pub(crate) fn inside(solid: &Solid, data: &SolidData, among: &[usize], p: Point3) -> Option<bool> {
    if !data.bounds.holds(p, 0.0) {
        return Some(false);
    }
    RAYS3.iter().find_map(|d| {
        let n = crossings(solid, data, among, p, Point3::from_array(*d), -1.0)?;
        Some(n % 2 == 1)
    })
}

/// A point well inside face `f` of `solid`, with the outward normal
/// there.
pub(crate) fn sample(solid: &Solid, data: &SolidData, f: usize) -> Option<(Point3, Point3)> {
    let face = &solid.faces[f];
    let (uv, _) = data.faces[f].trim.inner().or_else(|| {
        let [u, v] = data.faces[f].uv;
        data.faces[f]
            .trim
            .pieces
            .is_empty()
            .then_some((Point2::new((u.0 + u.1) / 2.0, (v.0 + v.1) / 2.0), 0.0))
    })?;
    let n = normal(&face.surface, uv);
    let p = face.surface.point_at(uv.x, uv.y);
    Some((p, if face.outward { n } else { -n }))
}

/// Whether the shell of `faces` has the solid's material around it (a
/// void) and not inside it: leaving one of its faces outwards, a ray
/// crosses it once more to get out of the hollow.
fn is_void(solid: &Solid, data: &SolidData, faces: &[usize]) -> Option<bool> {
    let size = data.bounds.size();
    for &f in faces {
        let Some((p, n)) = sample(solid, data, f) else {
            continue;
        };
        let (x, y) = n.any_orthonormal_pair();
        for (a, b) in [(0.31, 0.17), (-0.23, 0.41), (0.13, -0.37), (-0.45, -0.11)] {
            let d = (n + x * a + y * b).normalize();
            if let Some(n) = crossings(solid, data, faces, p, d, size * 1e-7) {
                return Some(n % 2 == 1);
            }
        }
    }
    None
}

impl Solid {
    /// Its lumps: each connected piece of material as a solid of its
    /// own, with the hollows inside it.
    pub fn lumps(&self) -> Result<Vec<Solid>, Refusal> {
        let lumps = self.lump_shells()?;
        Ok(lumps
            .iter()
            .filter(|shells| !shells.is_empty())
            .map(|shells| self.part(&shells.concat()))
            .collect())
    }

    /// The shells of the solid as lists of faces, each lump's outer
    /// shell first and its hollows after.
    pub(crate) fn lump_shells(&self) -> Result<Vec<Vec<Vec<usize>>>, Refusal> {
        let shells = self.shells();
        if shells.len() <= 1 {
            return Ok(vec![shells]);
        }
        let data = SolidData::new(self);
        let unclear = || Refusal::Degenerate("a shell could not be told inside out".into());
        let mut lumps: Vec<Vec<Vec<usize>>> = Vec::new();
        let mut voids = Vec::new();
        for shell in shells {
            if is_void(self, &data, &shell).ok_or_else(unclear)? {
                voids.push(shell);
            } else {
                lumps.push(vec![shell]);
            }
        }
        for hollow in voids {
            let (p, _) = hollow
                .iter()
                .find_map(|&f| sample(self, &data, f))
                .ok_or_else(unclear)?;
            let mut around: Option<(usize, f64)> = None;
            for (k, lump) in lumps.iter().enumerate() {
                if inside(self, &data, &lump[0], p).ok_or_else(unclear)? {
                    let size = lump[0]
                        .iter()
                        .fold(Bounds::EMPTY, |b, &f| b.union(data.faces[f].bounds))
                        .size();
                    if around.is_none_or(|a| size < a.1) {
                        around = Some((k, size));
                    }
                }
            }
            match around {
                Some((k, _)) => lumps[k].push(hollow),
                None => lumps.push(vec![hollow]),
            }
        }
        Ok(lumps)
    }
}
