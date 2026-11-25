//! Connectors: the frames joints are made between.
//!
//! A connector is a frame on something of a part, told by what it is on
//! and how it sits there ([`Snap`]), so that it follows the part when the
//! part changes. Which frame the pointer offers follows where it is:
//!
//! * on a **vertex**: a frame there, along the part's own axes;
//! * on an **arc** or circle: its centre, z along its axis;
//! * on any other **edge**: its middle, z along it — or, near an end, that
//!   end, z along the edge there;
//! * on a **face**, away from its boundary: its middle, z its normal — or,
//!   on a face around an axis (a cylinder, a cone), a point of the axis,
//!   z along it;
//! * on a face **near an edge**: the edge's frame with z the face's
//!   normal (an arc's own frame, for an arc);
//! * on a face **near a vertex**: that vertex, z the face's normal.

use anyhow::Context;
use workpiece_core::math::Point3;
use workpiece_core::world::topo::{
    CoedgeKind, Curve3, EdgeId, EntityId, FaceId, NurbsSurface, Orientation, Topo, VertexId,
};
use workpiece_core::world::{Datum, World};
use workpiece_core::{Pose, Source};

/// Where on an edge a frame is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EdgeAt {
    /// Its middle.
    Middle,
    /// The centre of the circle it is an arc of.
    Centre,
}

/// How a connector sits on what it is on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Snap {
    /// On a vertex, along the part's axes.
    Vertex,
    /// In the middle of an edge, z along it.
    EdgeMiddle,
    /// At an end of an edge (its start, or its end), z along it there.
    EdgeEnd {
        /// Whether at its start.
        start: bool,
    },
    /// At the centre of an arc, z along its axis.
    ArcCentre,
    /// In the middle of a face, z its normal.
    FaceCentre,
    /// On the axis of a face that goes around one, z along it.
    FaceAxis,
    /// On an edge of a face, z the face's normal there (an arc's own
    /// frame, at its centre).
    FaceEdge {
        /// The edge's name.
        edge: String,
        /// Where on it.
        at: EdgeAt,
    },
    /// On a vertex of a face, z the face's normal there.
    FaceVertex {
        /// The vertex's name.
        vertex: String,
    },
    /// A datum's own frame: a frame's, a plane's, or an axis' with z
    /// along it.
    Datum,
}

/// A frame at `o` with z along `z`: x the part's x turned as little as
/// it takes (its y, if z is along x).
pub fn frame_along(o: Point3, z: Point3) -> Pose {
    let z = z.try_normalize().unwrap_or(Point3::Z);
    let hint = if z.dot(Point3::X).abs() < 0.9 {
        Point3::X
    } else {
        Point3::Y
    };
    let x = hint.reject_from_normalized(z).normalize();
    Pose::from_axes(o, x, z.cross(x))
}

/// A frame at `o` with z along `z` and x as near `x` as it can be.
fn frame_with(o: Point3, z: Point3, x: Point3) -> Pose {
    let z = z.try_normalize().unwrap_or(Point3::Z);
    match x.reject_from_normalized(z).try_normalize() {
        Some(x) => Pose::from_axes(o, x, z.cross(x)),
        None => frame_along(o, z),
    }
}

struct EdgeShape<'a> {
    curve: &'a Curve3,
    range: (f64, f64),
    ends: [Point3; 2],
}

fn edge<'a>(topo: &'a Topo, id: EdgeId) -> Option<EdgeShape<'a>> {
    let e = topo.edge(id)?;
    Some(EdgeShape {
        curve: topo.curve(e.curve)?,
        range: (e.t0, e.t1),
        ends: [topo.vertex(e.start)?.point, topo.vertex(e.end)?.point],
    })
}

impl EdgeShape<'_> {
    fn at(&self, f: f64) -> Point3 {
        self.curve
            .point_at(self.range.0 + (self.range.1 - self.range.0) * f)
    }

    fn tangent(&self, f: f64) -> Point3 {
        let t = self.range.0 + (self.range.1 - self.range.0) * f;
        self.curve.derivative_at(t).1.normalize_or_zero()
    }

    /// The centre, axis and radius of the circle it is an arc of, if it
    /// is one.
    fn arc(&self) -> Option<(Point3, Point3, f64)> {
        let p: Vec<Point3> = (0..5).map(|k| self.at(f64::from(k) / 4.0)).collect();
        // A whole circle starts where it ends: three points apart.
        let (a, b, c) = (p[0], p[1], p[3]);
        let (u, v) = (b - a, c - a);
        let n = u.cross(v);
        let size = u.length().max(v.length());
        if n.length() <= 1e-6 * size * size {
            return None;
        }
        // The circle through the three.
        let centre = a
            + (u.length_squared() * v.cross(n) + v.length_squared() * n.cross(u))
                / (2.0 * n.length_squared());
        let radius = centre.distance(a);
        let axis = n.normalize();
        let on = |q: Point3| {
            (centre.distance(q) - radius).abs() <= 1e-6 * radius
                && (q - centre).dot(axis).abs() <= 1e-6 * radius
        };
        (p.iter().all(|q| on(*q)) && on(self.at(0.37))).then_some((centre, axis, radius))
    }

    /// How far `q` is from it, about.
    fn distance(&self, q: Point3) -> f64 {
        (0..=32)
            .map(|k| self.at(f64::from(k) / 32.0).distance(q))
            .fold(f64::INFINITY, f64::min)
    }
}

/// The (u, v) of the point of `surface` nearest `p`: the nearest of a
/// grid of its points, then stepped towards the foot.
fn foot(surface: &NurbsSurface, p: Point3) -> (f64, f64) {
    let ((u0, u1), (v0, v1)) = surface.domain();
    const GRID: usize = 24;
    let mut best = (f64::INFINITY, u0, v0);
    for i in 0..=GRID {
        for j in 0..=GRID {
            let (u, v) = (
                u0 + (u1 - u0) * i as f64 / GRID as f64,
                v0 + (v1 - v0) * j as f64 / GRID as f64,
            );
            let d = surface.point_at(u, v).distance_squared(p);
            if d < best.0 {
                best = (d, u, v);
            }
        }
    }
    let (mut u, mut v) = (best.1, best.2);
    for _ in 0..20 {
        let (q, du, dv) = surface.derivatives_at(u, v);
        let r = p - q;
        let (a, b, c) = (du.dot(du), du.dot(dv), dv.dot(dv));
        let det = a * c - b * b;
        if det.abs() <= 1e-300 {
            break;
        }
        let (ru, rv) = (r.dot(du), r.dot(dv));
        let (su, sv) = ((c * ru - b * rv) / det, (a * rv - b * ru) / det);
        let (nu, nv) = ((u + su).clamp(u0, u1), (v + sv).clamp(v0, v1));
        if surface.point_at(nu, nv).distance_squared(p)
            >= surface.point_at(u, v).distance_squared(p)
        {
            break;
        }
        (u, v) = (nu, nv);
    }
    (u, v)
}

/// The vertices and edges around `face` (not the seams a face that
/// closes around is cut open along).
fn boundary(topo: &Topo, face: FaceId) -> (Vec<VertexId>, Vec<EdgeId>) {
    let (mut vertices, mut edges, mut seams) = (Vec::new(), Vec::new(), Vec::new());
    for &l in &topo.face(face).map(|f| f.loops.clone()).unwrap_or_default() {
        for &c in topo.loop_(l).map(|l| &l.coedges[..]).unwrap_or_default() {
            let Some(coedge) = topo.coedge(c) else {
                continue;
            };
            match coedge.kind {
                CoedgeKind::Edge { edge, .. } => {
                    if edges.contains(&edge) {
                        // Walked both ways: a seam, no edge of the face.
                        seams.push(edge);
                    } else {
                        edges.push(edge);
                    }
                    if let Some(e) = topo.edge(edge) {
                        for v in [e.start, e.end] {
                            if !vertices.contains(&v) {
                                vertices.push(v);
                            }
                        }
                    }
                }
                CoedgeKind::Pole { vertex, .. } => {
                    if !vertices.contains(&vertex) {
                        vertices.push(vertex);
                    }
                }
            }
        }
    }
    edges.retain(|e| !seams.contains(e));
    (vertices, edges)
}

/// Whether the outside of `face` is against its surface's own normal.
fn reversed(topo: &Topo, face: FaceId) -> bool {
    topo.shells().iter().any(|(_, shell)| {
        shell
            .faces
            .iter()
            .any(|u| u.face == face && u.orientation == Orientation::Reversed)
    })
}

/// The outward normal of `face` at the point of it nearest `p`.
fn normal_at(topo: &Topo, face: FaceId, p: Point3) -> Option<Point3> {
    let surface = topo.surface(topo.face(face)?.surface)?;
    // (A plane's, wherever: also where its surface comes to a point —
    // the middle of a revolved disc.)
    if let Some(workpiece_shape::known::Known::Plane { normal, .. }) =
        workpiece_shape::known::known(surface)
    {
        return Some(if reversed(topo, face) {
            -normal
        } else {
            normal
        });
    }
    let (u, v) = foot(surface, p);
    let (_, du, dv) = surface.derivatives_at(u, v);
    let n = du.cross(dv).try_normalize().or_else(|| {
        // A point the surface collapses to: a little way in.
        let ((u0, u1), (v0, v1)) = surface.domain();
        let (iu, iv) = (
            u + ((u0 + u1) / 2.0 - u) * 1e-3,
            v + ((v0 + v1) / 2.0 - v) * 1e-3,
        );
        let (_, du, dv) = surface.derivatives_at(iu, iv);
        du.cross(dv).try_normalize()
    })?;
    Some(if reversed(topo, face) { -n } else { n })
}

/// A point about the middle of `face`, on it.
fn middle(topo: &Topo, face: FaceId) -> Option<Point3> {
    let surface = topo.surface(topo.face(face)?.surface)?;
    // The middle of its boundary, each piece by its length: of an arc,
    // the arc's own (of a whole circle its centre — of a ring, a disc,
    // the axis); of anything else, its pieces'.
    let (_, edges) = boundary(topo, face);
    let mut sum = Point3::ZERO;
    let mut count = 0.0;
    for e in edges {
        let Some(shape) = edge(topo, e) else {
            continue;
        };
        const PIECES: usize = 64;
        let at = |k: usize| shape.at(k as f64 / PIECES as f64);
        if let Some((centre, axis, radius)) = shape.arc() {
            let swept: f64 = (0..PIECES)
                .map(|k| {
                    let (a, b) = (at(k) - centre, at(k + 1) - centre);
                    axis.dot(a.cross(b)).atan2(a.dot(b))
                })
                .sum();
            let half = swept.abs() / 2.0;
            let start = (at(0) - centre).normalize_or_zero();
            let mid = glam::DQuat::from_axis_angle(axis, swept / 2.0) * start;
            let from = if half > 1e-9 { half.sin() / half } else { 1.0 };
            let length = radius * swept.abs();
            sum += (centre + mid * (radius * from)) * length;
            count += length;
        } else {
            for k in 0..PIECES {
                let (a, b) = (at(k), at(k + 1));
                let length = a.distance(b);
                sum += (a + b) / 2.0 * length;
                count += length;
            }
        }
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let centre = if count > 0.0 {
        sum / count
    } else {
        surface.point_at((u0 + u1) / 2.0, (v0 + v1) / 2.0)
    };
    // On the face: onto its plane where it is flat (the middle of a
    // ring stays on its axis, in the hole), else its nearest point.
    if let Some(workpiece_shape::known::Known::Plane { origin, normal }) =
        workpiece_shape::known::known(surface)
    {
        return Some(centre - normal * (centre - origin).dot(normal));
    }
    let (u, v) = foot(surface, centre);
    Some(surface.point_at(u, v))
}

/// The axis `face` goes around, if it does (a cylinder, a cone): a point
/// of it and its direction.
fn axis_of(topo: &Topo, face: FaceId) -> Option<(Point3, Point3)> {
    let surface = topo.surface(topo.face(face)?.surface)?;
    let ((u0, u1), (v0, v1)) = surface.domain();
    let mut samples = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            let (u, v) = (
                u0 + (u1 - u0) * (f64::from(i) + 0.5) / 5.0,
                v0 + (v1 - v0) * (f64::from(j) + 0.5) / 5.0,
            );
            let (p, du, dv) = surface.derivatives_at(u, v);
            if let Some(n) = du.cross(dv).try_normalize() {
                samples.push((p, n));
            }
        }
    }
    if samples.len() < 9 {
        return None;
    }
    // The normals of a surface around an axis all lean the same way to
    // it: they lie in a plane, and the axis is across it.
    let mean = samples.iter().map(|s| s.1).sum::<Point3>() / samples.len() as f64;
    let mut cov = [[0.0_f64; 3]; 3];
    for (_, n) in &samples {
        let d = (*n - mean).to_array();
        for a in 0..3 {
            for b in 0..3 {
                cov[a][b] += d[a] * d[b];
            }
        }
    }
    let (values, vectors) = eigen(cov);
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| values[*a].total_cmp(&values[*b]));
    let spread = values[order[2]];
    // Flat (no spread at all), or spread every way (a ball): no axis.
    if spread <= 1e-9 || values[order[0]] > 1e-9 * spread || values[order[1]] <= 1e-6 * spread {
        return None;
    }
    let axis = Point3::from_array(vectors[order[0]]).normalize();
    // Every normal's line meets the axis: where, seen along it.
    let (ex, ey) = axis.any_orthonormal_pair();
    let flat = |p: Point3| (p.dot(ex), p.dot(ey));
    let (mut a, mut b, mut c, mut rx, mut ry) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, n) in &samples {
        let (qx, qy) = flat(*p);
        let (mx, my) = flat(*n);
        let len = mx.hypot(my);
        if len <= 1e-9 {
            return None;
        }
        let (mx, my) = (mx / len, my / len);
        // (I − m mᵀ)
        let (pxx, pxy, pyy) = (1.0 - mx * mx, -mx * my, 1.0 - my * my);
        a += pxx;
        b += pxy;
        c += pyy;
        rx += pxx * qx + pxy * qy;
        ry += pxy * qx + pyy * qy;
    }
    let det = a * c - b * b;
    if det.abs() <= 1e-12 {
        return None;
    }
    let (cx, cy) = ((c * rx - b * ry) / det, (a * ry - b * rx) / det);
    let size = samples
        .iter()
        .map(|s| s.0.distance(samples[0].0))
        .fold(0.0, f64::max)
        .max(1e-12);
    let off = samples
        .iter()
        .map(|(p, n)| {
            let (qx, qy) = flat(*p);
            let (mx, my) = flat(*n);
            let len = mx.hypot(my);
            ((cx - qx) * my - (cy - qy) * mx).abs() / len
        })
        .fold(0.0, f64::max);
    if off > 1e-6 * size {
        return None;
    }
    let height = samples.iter().map(|s| s.0.dot(axis)).sum::<f64>() / samples.len() as f64;
    Some((ex * cx + ey * cy + axis * height, axis))
}

/// The eigenvalues and eigenvectors of a symmetric matrix, by Jacobi's
/// turns.
fn eigen(mut m: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        let (mut p, mut q, mut big) = (0, 1, 0.0_f64);
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            if m[a][b].abs() > big {
                (p, q, big) = (a, b, m[a][b].abs());
            }
        }
        if big <= 1e-15 {
            break;
        }
        let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
        let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
        let c = 1.0 / (t * t + 1.0).sqrt();
        let s = t * c;
        for row in &mut m {
            let (a, b) = (row[p], row[q]);
            row[p] = c * a - s * b;
            row[q] = s * a + c * b;
        }
        let (rp, rq) = (m[p], m[q]);
        m[p] = std::array::from_fn(|k| c * rp[k] - s * rq[k]);
        m[q] = std::array::from_fn(|k| s * rp[k] + c * rq[k]);
        for row in &mut v {
            let (a, b) = (row[p], row[q]);
            row[p] = c * a - s * b;
            row[q] = s * a + c * b;
        }
    }
    // Columns are the vectors.
    let column = |k: usize| [v[0][k], v[1][k], v[2][k]];
    (
        [m[0][0], m[1][1], m[2][2]],
        [column(0), column(1), column(2)],
    )
}

fn named<T: TryFrom<EntityId>>(topo: &Topo, name: &str) -> anyhow::Result<T> {
    topo.resolve(name)
        .with_context(|| format!("{name} is gone, or is not what a connector there needs"))
}

/// The frame of the connector on `source` of `world`, sitting there as
/// `snap` says, in the world's coordinates.
pub fn frame(world: &World, source: &Source, snap: &Snap) -> anyhow::Result<Pose> {
    let topo = &world.topo;
    if *snap == Snap::Datum {
        let name = source.name();
        return Ok(match world.datums.get(name) {
            Some(Datum::Frame(frame)) => frame.pose(),
            Some(Datum::Plane(plane)) => plane.pose(),
            Some(Datum::Axis(axis)) => frame_along(axis.origin(), axis.direction()),
            None => anyhow::bail!("no datum {name}"),
        });
    }
    let Source::Topology(name) = source else {
        anyhow::bail!("a connector on {source:?} cannot sit there that way");
    };
    let edge_of = |name: &str| -> anyhow::Result<EdgeShape<'_>> {
        edge(topo, named(topo, name)?).with_context(|| format!("{name} has no curve"))
    };
    let vertex_of = |name: &str| -> anyhow::Result<Point3> {
        let id: VertexId = named(topo, name)?;
        Ok(topo.vertex(id).context("no vertex")?.point)
    };
    let face_normal =
        |face: FaceId, p: Point3| normal_at(topo, face, p).context("the face has no normal there");
    Ok(match snap {
        Snap::Vertex => Pose::from_translation(vertex_of(name)?),
        Snap::EdgeMiddle => {
            let e = edge_of(name)?;
            frame_along(e.at(0.5), e.tangent(0.5))
        }
        Snap::EdgeEnd { start } => {
            let e = edge_of(name)?;
            let f = if *start { 0.0 } else { 1.0 };
            frame_along(e.ends[usize::from(!*start)], e.tangent(f))
        }
        Snap::ArcCentre => {
            let (centre, axis, _) = edge_of(name)?.arc().context("the edge is no arc")?;
            frame_along(centre, axis)
        }
        Snap::FaceCentre => {
            let face: FaceId = named(topo, name)?;
            let p = middle(topo, face).context("the face has no surface")?;
            frame_along(p, face_normal(face, p)?)
        }
        Snap::FaceAxis => {
            let face: FaceId = named(topo, name)?;
            let (o, axis) = axis_of(topo, face).context("the face goes around no axis")?;
            frame_along(o, axis)
        }
        Snap::FaceEdge { edge, at } => {
            let face: FaceId = named(topo, name)?;
            let e = edge_of(edge)?;
            match (at, e.arc()) {
                (EdgeAt::Centre, Some((centre, axis, _))) => {
                    // Along the face's normal, of the arc's two ways.
                    let n = face_normal(face, e.at(0.5))?;
                    frame_along(centre, if axis.dot(n) < 0.0 { -axis } else { axis })
                }
                _ => {
                    let p = e.at(0.5);
                    frame_with(p, face_normal(face, p)?, e.tangent(0.5))
                }
            }
        }
        Snap::FaceVertex { vertex } => {
            let face: FaceId = named(topo, name)?;
            let p = vertex_of(vertex)?;
            frame_along(p, face_normal(face, p)?)
        }
        Snap::Datum => unreachable!("handled above"),
    })
}

/// How a connector on `source` of `world` sits there when the pointer is
/// at `at` on it (in the world's coordinates); `near` is how close to a
/// vertex or an edge counts as on it.
pub fn infer(world: &World, source: &Source, at: Point3, near: f64) -> Option<Snap> {
    let topo = &world.topo;
    let name = match source {
        Source::Datum(_) | Source::FrameAxis { .. } => return Some(Snap::Datum),
        Source::Profile(_) | Source::Placed { .. } => return None,
        Source::Topology(name) => name,
    };
    match topo.referred(name)? {
        EntityId::Vertex(_) => Some(Snap::Vertex),
        EntityId::Edge(id) => {
            let e = edge(topo, id)?;
            if e.arc().is_some() {
                return Some(Snap::ArcCentre);
            }
            let (d0, d1) = (e.ends[0].distance(at), e.ends[1].distance(at));
            Some(if d0.min(d1) <= near {
                Snap::EdgeEnd { start: d0 <= d1 }
            } else {
                Snap::EdgeMiddle
            })
        }
        EntityId::Face(id) => {
            let (vertices, edges) = boundary(topo, id);
            let vertex = vertices
                .iter()
                .filter_map(|&v| Some((topo.vertex(v)?.point.distance(at), topo.name_of(v)?)))
                .filter(|(d, _)| *d <= near)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, vertex)) = vertex {
                return Some(Snap::FaceVertex {
                    vertex: vertex.to_owned(),
                });
            }
            let on_edge = edges
                .iter()
                .filter_map(|&e| {
                    let shape = edge(topo, e)?;
                    Some((shape.distance(at), topo.name_of(e)?, shape.arc().is_some()))
                })
                .filter(|(d, _, _)| *d <= near)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, edge, arc)) = on_edge {
                return Some(Snap::FaceEdge {
                    edge: edge.to_owned(),
                    at: if arc { EdgeAt::Centre } else { EdgeAt::Middle },
                });
            }
            Some(if axis_of(topo, id).is_some() {
                Snap::FaceAxis
            } else {
                Snap::FaceCentre
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::{Axis, Measure, Operation, Standalone};
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs, AddCylinder, AddCylinderArgs};

    /// A box 2 × 4 × 6 around the origin and a cylinder of radius 1 and
    /// height 3 standing on it.
    fn world() -> anyhow::Result<World> {
        let mut b = Standalone::new();
        AddBox {
            id: "box".into(),
            args: AddBoxArgs {
                frame: Some(workpiece_core::Reference::datum(
                    workpiece_core::world::ORIGIN,
                )),
                size: [2.0, 4.0, 6.0].map(Measure::new),
            },
        }
        .apply(&mut b)?;
        AddCylinder {
            id: "rod".into(),
            args: AddCylinderArgs {
                frame: Some(workpiece_core::Reference::datum(
                    workpiece_core::world::ORIGIN,
                )),
                radius: Measure::new(1.0),
                height: Measure::new(3.0),
            },
        }
        .apply(&mut b)?;
        Ok(b.world)
    }

    /// The names of what `world` has of a kind, with a point of each.
    fn faces(world: &World, prefix: &str) -> Vec<(String, Point3)> {
        let topo = &world.topo;
        topo.faces()
            .ids()
            .filter_map(|f| {
                let name = topo.name_of(f)?;
                name.starts_with(prefix)
                    .then(|| Some((name.to_owned(), middle(topo, f)?)))?
            })
            .collect()
    }

    #[test]
    fn the_middle_of_a_round_face_is_on_its_axis() -> anyhow::Result<()> {
        // The rod's ends: discs, their rims whole circles drawn by
        // rational curves (not evenly along them).
        let w = world()?;
        let ends: Vec<_> = faces(&w, "rod")
            .into_iter()
            .filter(|(_, at)| at.z.abs() < 1e-9 || (at.z - 3.0).abs() < 1e-9)
            .collect();
        assert_eq!(ends.len(), 2, "{ends:?}");
        for (name, at) in ends {
            assert!(at.x.abs() < 1e-9 && at.y.abs() < 1e-9, "{name}: {at}");
        }
        Ok(())
    }

    #[test]
    fn faces_offer_their_middle_their_edges_and_their_corners() -> anyhow::Result<()> {
        let w = world()?;
        let (top, at) = faces(&w, "box")
            .into_iter()
            .max_by(|a, b| a.1.z.total_cmp(&b.1.z))
            .expect("a top face");
        let source = Source::Topology(top);
        // In the middle: the face's middle, z up.
        let snap = infer(&w, &source, at, 0.2).expect("a snap");
        assert_eq!(snap, Snap::FaceCentre);
        let f = frame(&w, &source, &snap)?;
        assert!(
            f.translation()
                .abs_diff_eq(Point3::new(0.0, 0.0, 3.0), 1e-9)
        );
        assert!(f.axis(Axis::Z).abs_diff_eq(Point3::Z, 1e-9));
        // Near a corner: the corner, z still the face's normal.
        let corner = Point3::new(1.0, 2.0, 3.0);
        let snap = infer(&w, &source, corner - Point3::new(0.05, 0.05, 0.0), 0.2).expect("a snap");
        assert!(matches!(snap, Snap::FaceVertex { .. }), "{snap:?}");
        let f = frame(&w, &source, &snap)?;
        assert!(f.translation().abs_diff_eq(corner, 1e-9));
        assert!(f.axis(Axis::Z).abs_diff_eq(Point3::Z, 1e-9));
        // Near an edge: the edge's middle, z the normal, x along it.
        let snap = infer(&w, &source, Point3::new(0.95, 0.3, 3.0), 0.2).expect("a snap");
        assert!(
            matches!(
                snap,
                Snap::FaceEdge {
                    at: EdgeAt::Middle,
                    ..
                }
            ),
            "{snap:?}"
        );
        let f = frame(&w, &source, &snap)?;
        assert!(
            f.translation()
                .abs_diff_eq(Point3::new(1.0, 0.0, 3.0), 1e-9)
        );
        assert!(f.axis(Axis::Z).abs_diff_eq(Point3::Z, 1e-9));
        assert!(f.axis(Axis::X).cross(Point3::Y).length() < 1e-9);
        Ok(())
    }

    #[test]
    fn round_faces_and_arcs_offer_their_axis() -> anyhow::Result<()> {
        let w = world()?;
        let topo = &w.topo;
        // The cylinder's wall: the face of it that goes around an axis.
        let wall = topo
            .faces()
            .ids()
            .find(|f| {
                topo.name_of(*f).is_some_and(|n| n.starts_with("rod"))
                    && axis_of(topo, *f).is_some()
            })
            .expect("a wall");
        let source = Source::Topology(topo.name_of(wall).expect("named").to_owned());
        let snap = infer(&w, &source, Point3::new(1.0, 0.0, 1.5), 0.1).expect("a snap");
        assert_eq!(snap, Snap::FaceAxis);
        let f = frame(&w, &source, &snap)?;
        assert!(f.translation().truncate().length() < 1e-9, "{f:?}");
        assert!(f.axis(Axis::Z).cross(Point3::Z).length() < 1e-9);
        // Its rim: the circle's centre.
        let rim = topo
            .edges()
            .ids()
            .find(|e| {
                topo.name_of(*e).is_some_and(|n| n.starts_with("rod"))
                    && edge(topo, *e).and_then(|s| s.arc()).is_some()
            })
            .expect("a rim");
        let source = Source::Topology(topo.name_of(rim).expect("named").to_owned());
        let snap = infer(&w, &source, Point3::new(1.0, 0.0, 0.0), 0.1).expect("a snap");
        assert_eq!(snap, Snap::ArcCentre);
        let f = frame(&w, &source, &snap)?;
        assert!(f.translation().truncate().length() < 1e-9);
        assert!(f.axis(Axis::Z).cross(Point3::Z).length() < 1e-9);
        Ok(())
    }

    #[test]
    fn straight_edges_offer_their_middle_and_their_ends() -> anyhow::Result<()> {
        let w = world()?;
        let topo = &w.topo;
        // An edge of the box along z.
        let (id, shape) = topo
            .edges()
            .ids()
            .filter_map(|e| Some((e, edge(topo, e)?)))
            .find(|(e, s)| {
                topo.name_of(*e).is_some_and(|n| n.starts_with("box"))
                    && (s.ends[0] - s.ends[1]).cross(Point3::Z).length() < 1e-9
            })
            .expect("an edge along z");
        let source = Source::Topology(topo.name_of(id).expect("named").to_owned());
        let mid = (shape.ends[0] + shape.ends[1]) / 2.0;
        let snap = infer(&w, &source, mid, 0.2).expect("a snap");
        assert_eq!(snap, Snap::EdgeMiddle);
        let f = frame(&w, &source, &snap)?;
        assert!(f.translation().abs_diff_eq(mid, 1e-9));
        assert!(f.axis(Axis::Z).cross(Point3::Z).length() < 1e-9);
        let snap = infer(
            &w,
            &source,
            shape.ends[1] * 0.99 + shape.ends[0] * 0.01,
            0.2,
        );
        assert_eq!(snap, Some(Snap::EdgeEnd { start: false }));
        let f = frame(&w, &source, &Snap::EdgeEnd { start: false })?;
        assert!(f.translation().abs_diff_eq(shape.ends[1], 1e-9));
        Ok(())
    }
}
