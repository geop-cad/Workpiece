//! Object snaps in the scene, as Rhino has them: what a point moved or
//! placed in three dimensions lands on near the pointer — a point (a
//! vertex, a sketch's point, a datum's origin), the middle of an edge, a
//! centre (of a circle or arc, of a flat face), a quadrant point of a
//! circle, where two edges cross as they are seen, the foot of the
//! perpendicular from where a drag started, or the nearest point of an
//! edge. Each kind is switched on and off by itself ([`SnapKinds`]); the
//! grid ([`workpiece_core::handles::snap`]) is the caller's, as it keeps
//! to the plane or the axis a drag moves in.
//!
//! What is snapped to: the scene's ([`scene`], made once for a world as
//! it was built) and what the edited step offers of its own
//! ([`SnapTargets`] of its space). The sketcher's grid spacing and its
//! badges are the same ([`grid_step`](workpiece_core::handles::snap::grid_step),
//! [`SnapKind::badge`]).

use std::rc::Rc;

use workpiece_core::handles::{Snap, SnapKind, SnapKinds, SnapTargets};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::visibility::shown_topology;
use workpiece_core::world::{Datum, Visibility, World};
use workpiece_core::{Projection, Ray};

/// How near the pointer, in pixels, a point is snapped to.
pub const REACH_PX: f64 = 12.0;

/// How far `p` is from `ray`'s line, in model units; none behind the
/// eye.
fn off(ray: &Ray, p: Point3) -> Option<f64> {
    let d = p - ray.start();
    let along = d.dot(ray.direction());
    if matches!(ray.projection(), Projection::Perspective { .. }) && along <= 0.0 {
        return None;
    }
    Some((d - ray.direction() * along).length())
}

/// The point of the segment from `a` to `b` nearest `ray`'s line.
fn nearest_on(a: Point3, b: Point3, ray: &Ray) -> Point3 {
    let Some(dir) = (b - a).try_normalize() else {
        return a;
    };
    let s = ray.closest_on_line(a, dir).unwrap_or(0.0);
    a + dir * s.clamp(0.0, a.distance(b))
}

/// What `ray` points at among `targets`, of the kinds `kinds` takes,
/// within [`REACH_PX`] of the pointer (`pixel` gives the size of a pixel
/// at a point): the nearest point-like target (a point, a middle, a
/// centre, a quadrant point); else where two edges cross as seen; else
/// the foot of the perpendicular from `from` onto an edge; else the
/// nearest point of an edge. None for nothing near, or with snapping
/// off.
pub fn find(
    targets: &[&SnapTargets],
    ray: &Ray,
    pixel: &dyn Fn(Point3) -> f64,
    kinds: &SnapKinds,
    from: Option<Point3>,
) -> Option<Snap> {
    if !kinds.on {
        return None;
    }
    let px = |p: Point3| Some(off(ray, p)? / pixel(p).max(1e-300));
    let within = |p: Point3, reach: f64| px(p).filter(|d| *d <= reach);
    // A point, a middle, a centre, a quadrant point.
    let mut best: Option<(f64, Snap)> = None;
    for t in targets {
        for &(at, kind) in &t.points {
            if !kinds.takes(kind) {
                continue;
            }
            if let Some(d) = within(at, REACH_PX)
                && best.is_none_or(|(least, _)| d < least)
            {
                best = Some((d, Snap { at, kind }));
            }
        }
    }
    if let Some((_, snap)) = best {
        return Some(snap);
    }
    // The edges near the pointer.
    let near: Vec<[Point3; 2]> = targets
        .iter()
        .flat_map(|t| t.segments.iter().copied())
        .filter(|&[a, b]| within(nearest_on(a, b, ray), 2.0 * REACH_PX).is_some())
        .take(400)
        .collect();
    if kinds.takes(SnapKind::Crossing) {
        let mut best: Option<(f64, Point3)> = None;
        for (i, s) in near.iter().enumerate() {
            for q in &near[i + 1..] {
                let Some(at) = crossing(ray, *s, *q) else {
                    continue;
                };
                if let Some(d) = within(at, REACH_PX)
                    && best.is_none_or(|(least, _)| d < least)
                {
                    best = Some((d, at));
                }
            }
        }
        if let Some((_, at)) = best {
            return Some(Snap {
                at,
                kind: SnapKind::Crossing,
            });
        }
    }
    if let (Some(from), true) = (from, kinds.takes(SnapKind::Perpendicular)) {
        let foot = near
            .iter()
            .filter_map(|&[a, b]| {
                let d = b - a;
                let s = (from - a).dot(d) / d.length_squared().max(1e-300);
                (0.0..=1.0).contains(&s).then(|| a + d * s)
            })
            .filter(|p| p.distance(from) > 1e-9)
            .filter_map(|p| Some((within(p, REACH_PX)?, p)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, at)) = foot {
            return Some(Snap {
                at,
                kind: SnapKind::Perpendicular,
            });
        }
    }
    if kinds.takes(SnapKind::Edge) {
        let on = near
            .iter()
            .map(|&[a, b]| nearest_on(a, b, ray))
            .filter_map(|p| Some((within(p, REACH_PX)?, p)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, at)) = on {
            return Some(Snap {
                at,
                kind: SnapKind::Edge,
            });
        }
    }
    None
}

/// Where the segments `s` and `q` cross as `ray`'s eye sees them, on the
/// one in front; none where they do not, or meet at an end.
fn crossing(ray: &Ray, s: [Point3; 2], q: [Point3; 2]) -> Option<Point3> {
    if s.iter().any(|a| q.iter().any(|b| a.distance(*b) <= 1e-12)) {
        return None;
    }
    let d = ray.direction();
    let u = d.any_orthonormal_vector();
    let v = d.cross(u);
    let perspective = matches!(ray.projection(), Projection::Perspective { .. });
    let eye = if perspective {
        ray.start()
    } else {
        Point3::ZERO
    };
    let depth = |p: Point3| (p - eye).dot(d);
    let flat = |p: Point3| {
        let w = p - eye;
        let k = if perspective {
            1.0 / depth(p).max(1e-12)
        } else {
            1.0
        };
        Point2::new(w.dot(u) * k, w.dot(v) * k)
    };
    let (a, b, c, e) = (flat(s[0]), flat(s[1]), flat(q[0]), flat(q[1]));
    let (r, t) = (b - a, e - c);
    let den = r.perp_dot(t);
    if den.abs() <= 1e-15 * r.length() * t.length() {
        return None;
    }
    let x = (c - a).perp_dot(t) / den;
    let y = (c - a).perp_dot(r) / den;
    if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return None;
    }
    // Back into space: along each segment (seen in perspective, the
    // parameter on the screen is not the one in space).
    let lift = |seg: [Point3; 2], k: f64| {
        let k = if perspective {
            let (da, db) = (depth(seg[0]), depth(seg[1]));
            k * da / (k * da + (1.0 - k) * db)
        } else {
            k
        };
        seg[0].lerp(seg[1], k)
    };
    let (p, o) = (lift(s, x), lift(q, y));
    Some(if depth(p) <= depth(o) { p } else { o })
}

/// The circle through the points of `polyline` (centre, normal,
/// radius), if they lie on one.
fn circle_of(polyline: &[Point3]) -> Option<(Point3, Point3, f64)> {
    if polyline.len() < 6 {
        return None;
    }
    let (a, b, c) = (
        polyline[0],
        polyline[polyline.len() / 2],
        polyline[polyline.len() - 1],
    );
    // (A closed one: through a third and two thirds of the way.)
    let (b, c) = if a.distance(c) <= 1e-9 * a.distance(b).max(1e-300) {
        (
            polyline[polyline.len() / 3],
            polyline[2 * polyline.len() / 3],
        )
    } else {
        (b, c)
    };
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let nn = n.length_squared();
    if nn <= 1e-24 {
        return None;
    }
    let centre =
        a + (n.cross(ab) * ac.length_squared() + ac.cross(n) * ab.length_squared()) / (2.0 * nn);
    let radius = centre.distance(a);
    let normal = n / nn.sqrt();
    let fits = polyline.iter().all(|p| {
        (p.distance(centre) - radius).abs() <= radius * 1e-4
            && (*p - centre).dot(normal).abs() <= radius * 1e-4
    });
    fits.then_some((centre, normal, radius))
}

/// What of `world` (showing what `visibility` does not hide) is snapped
/// to: its corners, the middles of its edges and its edges, the centres
/// and quadrant points of its circles and arcs, the centres of its flat
/// faces, the points and curves of its sketches, the origins of its
/// datums. Made once for a world as it was built and kept.
pub fn scene(world: &World, visibility: &Visibility) -> Rc<SnapTargets> {
    type Kept = Option<((u64, Vec<String>), Rc<SnapTargets>)>;
    thread_local! {
        static KEPT: std::cell::RefCell<Kept> = const { std::cell::RefCell::new(None) };
    }
    let key = (
        world.stamp,
        visibility.bodies.iter().cloned().collect::<Vec<_>>(),
    );
    if world.stamp != 0
        && let Some(found) = KEPT.with_borrow(|k| {
            k.as_ref()
                .filter(|(at, _)| *at == key)
                .map(|(_, t)| t.clone())
        })
    {
        return found;
    }
    let made = Rc::new(targets_of(world, visibility));
    KEPT.set(Some((key, made.clone())));
    made
}

fn targets_of(world: &World, visibility: &Visibility) -> SnapTargets {
    let mut out = SnapTargets::default();
    let topo = &world.topo;
    let shown = shown_topology(topo, visibility);
    for &id in &shown.vertices {
        if let Some(v) = topo.vertex(id) {
            out.points.push((v.point, SnapKind::Point));
        }
    }
    for &id in &shown.edges {
        let Some(edge) = topo.edge(id) else { continue };
        let Some(curve) = topo.curve(edge.curve) else {
            continue;
        };
        let polyline = curve.polyline(edge.t0, edge.t1);
        out.points
            .push((curve.point_at((edge.t0 + edge.t1) / 2.0), SnapKind::Middle));
        for (&p, &q) in polyline.iter().zip(polyline.iter().skip(1)) {
            out.segments.push([p, q]);
        }
        if let Some((centre, normal, radius)) = circle_of(&polyline) {
            out.points.push((centre, SnapKind::Centre));
            let u = (Point3::X - normal * normal.x)
                .try_normalize()
                .unwrap_or_else(|| normal.any_orthonormal_vector());
            let v = normal.cross(u);
            for q in [u, v, -u, -v].map(|w| centre + w * radius) {
                let on = polyline
                    .iter()
                    .zip(polyline.iter().skip(1))
                    .any(|(&a, &b)| q.distance(a.lerp(b, 0.5)) <= a.distance(b));
                if on {
                    out.points.push((q, SnapKind::Quad));
                }
            }
        }
    }
    // The middles of flat faces.
    for &id in &shown.faces {
        let Some(mesh) = workpiece_rasterize::tessellate::face(topo, id) else {
            continue;
        };
        let mut area = 0.0;
        let mut sum = Point3::ZERO;
        let mut normal: Option<Point3> = None;
        let mut flat = true;
        for ([a, b, c], _) in &mesh.triangles {
            let n = (*b - *a).cross(*c - *a);
            let w = n.length() / 2.0;
            if let Some(n) = n.try_normalize() {
                match normal {
                    None => normal = Some(n),
                    Some(m) => flat &= m.dot(n) > 1.0 - 1e-9,
                }
            }
            area += w;
            sum += (*a + *b + *c) / 3.0 * w;
        }
        if flat && area > 0.0 {
            out.points.push((sum / area, SnapKind::Centre));
        }
    }
    for (_, profile) in world.shown_profiles(visibility) {
        let pose = profile.plane.pose();
        for e in &profile.edges {
            let points: Vec<Point3> = e
                .curve
                .polyline(e.t0, e.t1)
                .into_iter()
                .map(|p| pose.transform_point(p.extend(0.0)))
                .collect();
            if let (Some(&first), Some(&last)) = (points.first(), points.last()) {
                out.points.push((first, SnapKind::Point));
                out.points.push((last, SnapKind::Point));
            }
            for (&p, &q) in points.iter().zip(points.iter().skip(1)) {
                out.segments.push([p, q]);
            }
        }
    }
    for (_, datum) in world.shown_datums(visibility) {
        let at = match datum {
            Datum::Frame(frame) => frame.origin(),
            Datum::Plane(plane) => plane.origin(),
            Datum::Axis(axis) => axis.origin(),
        };
        out.points.push((at, SnapKind::Point));
    }
    out
}
