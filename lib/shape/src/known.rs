//! What kind of surface a NURBS surface is, where it is one of the
//! usual: a plane, a cylinder, a cone — whichever way it was made (a
//! plane by revolving a line square to the axis is rational of degree
//! two, a cylinder by extruding an arc, a cone by revolving a slanted
//! line).

use workpiece_core::math::Point3;
use workpiece_core::world::topo::NurbsSurface;

/// A surface of a usual kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Known {
    /// A plane: a point of it and its normal (a unit vector, as the
    /// surface's own normal points).
    Plane {
        /// A point of it.
        origin: Point3,
        /// Its normal.
        normal: Point3,
    },
    /// A cylinder: a point of its axis, the axis (a unit vector) and its
    /// radius.
    Cylinder {
        /// A point of its axis.
        origin: Point3,
        /// Its axis.
        axis: Point3,
        /// Its radius.
        radius: f64,
    },
    /// A cone: its apex, its axis (a unit vector, from the apex into
    /// the cone) and the angle between the axis and its side, in
    /// radians.
    Cone {
        /// Its apex.
        apex: Point3,
        /// Its axis.
        axis: Point3,
        /// Half its opening angle.
        half_angle: f64,
    },
}

/// Points of `surface` on a grid over its domain, each with its unit
/// normal (where it has one).
fn samples(surface: &NurbsSurface) -> Vec<(Point3, Point3)> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    const GRID: usize = 7;
    let mut out = Vec::new();
    for i in 0..GRID {
        for j in 0..GRID {
            let u = u0 + (u1 - u0) * (i as f64 + 0.5) / GRID as f64;
            let v = v0 + (v1 - v0) * (j as f64 + 0.5) / GRID as f64;
            let (p, du, dv) = surface.derivatives_at(u, v);
            if let Some(n) = du.cross(dv).try_normalize() {
                out.push((p, n));
            }
        }
    }
    out
}

/// The point nearest both lines `p + s·a` and `q + t·b`, if they are not
/// parallel: the middle of their common perpendicular.
fn nearest(p: Point3, a: Point3, q: Point3, b: Point3) -> Option<Point3> {
    let n = a.cross(b);
    let nn = n.length_squared();
    if nn < 1e-12 {
        return None;
    }
    let d = q - p;
    let s = d.cross(b).dot(n) / nn;
    let t = d.cross(a).dot(n) / nn;
    Some((p + a * s + q + b * t) / 2.0)
}

/// What kind `surface` is, if it is a plane, a cylinder or a cone to
/// within a millionth of its size.
pub fn known(surface: &NurbsSurface) -> Option<Known> {
    let points = surface.control_points();
    let (lo, hi) = points.iter().fold(
        (
            Point3::splat(f64::INFINITY),
            Point3::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let size = (hi - lo).length().max(1e-12);
    let tol = size * 1e-6;
    let all = samples(surface);
    let (&(p0, n0), rest) = all.split_first()?;
    // A plane: every control point on the plane of the first sample
    // (a rational surface lies within the hull of its control points).
    if points.iter().all(|p| (*p - p0).dot(n0).abs() <= tol) {
        return Some(Known::Plane {
            origin: p0,
            normal: n0,
        });
    }
    // Around an axis, the normals' tips lie on a circle of the unit
    // sphere: the axis is square to its plane.
    let far = rest
        .iter()
        .max_by(|a, b| a.1.distance(n0).total_cmp(&b.1.distance(n0)))?
        .1;
    let third = rest.iter().map(|s| s.1).max_by(|a, b| {
        let area = |n: &Point3| (far - n0).cross(*n - n0).length();
        area(a).total_cmp(&area(b))
    })?;
    let axis = (far - n0).cross(third - n0).try_normalize()?;
    let lean = n0.dot(axis);
    if all.iter().any(|(_, n)| (n.dot(axis) - lean).abs() > 1e-6) {
        return None;
    }
    // Every normal line meets the axis, those of a circle about it at
    // one point: where the first and one well across it about as far
    // along the axis meet is on it.
    let (q, m) = *rest
        .iter()
        .filter(|s| n0.cross(s.1).length() > 0.5)
        .min_by(|a, b| {
            let along = |p: &Point3| (*p - p0).dot(axis).abs();
            along(&a.0).total_cmp(&along(&b.0))
        })?;
    let on = nearest(p0, n0, q, m)?;
    // Each point's distance from the axis, and how far along it is.
    let apart = |p: Point3| {
        let d = p - on;
        let along = d.dot(axis);
        ((d - axis * along).length(), along)
    };
    if lean.abs() < 1e-6 {
        let radius = apart(p0).0;
        return all
            .iter()
            .all(|(p, _)| (apart(*p).0 - radius).abs() <= tol)
            .then_some(Known::Cylinder {
                origin: on,
                axis,
                radius,
            });
    }
    // A cone: the distance from the axis grows evenly along it, at
    // the slope the normals lean by.
    let (r0, h0) = apart(p0);
    let slope = (lean / (1.0 - lean * lean).max(1e-300).sqrt()).abs();
    // (The sign of the slope: which way along the axis it widens.)
    let (r1, h1) = all
        .iter()
        .map(|(p, _)| apart(*p))
        .max_by(|a, b| (a.1 - h0).abs().total_cmp(&(b.1 - h0).abs()))?;
    if (h1 - h0).abs() <= tol {
        return None;
    }
    let widening = (r1 - r0) / (h1 - h0);
    if (widening.abs() - slope).abs() > 1e-6 * slope.max(1.0) {
        return None;
    }
    if all.iter().any(|(p, _)| {
        let (r, h) = apart(*p);
        (r - (r0 + widening * (h - h0))).abs() > tol
    }) {
        return None;
    }
    // The apex: where the radius comes to nothing; the axis into the
    // cone, the way it widens.
    let apex_along = h0 - r0 / widening;
    let axis = if widening > 0.0 { axis } else { -axis };
    Some(Known::Cone {
        apex: on + axis * (apex_along * widening.signum()),
        axis,
        half_angle: widening.abs().atan(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The surface the line from `from` to `to` (in the xz plane)
    /// sweeps turning about z: rational, a circle of nine control points
    /// (degree two) along u, the line along v.
    fn revolved(from: Point3, to: Point3) -> NurbsSurface {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for k in 0..9 {
            let (sin, cos) = (f64::from(k) * std::f64::consts::FRAC_PI_4).sin_cos();
            let (out, weight) = if k % 2 == 1 { (1.0 / w, w) } else { (1.0, 1.0) };
            for p in [from, to] {
                let (x, y) = (p.x * out, p.y * out);
                points.push(Point3::new(x * cos - y * sin, x * sin + y * cos, p.z));
                weights.push(weight);
            }
        }
        let knots_u = vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ];
        let knots_v = vec![0.0, 0.0, 1.0, 1.0];
        NurbsSurface::new(2, 1, knots_u, knots_v, 9, 2, points, Some(weights)).expect("a surface")
    }

    #[test]
    fn revolved_lines_are_planes_cylinders_and_cones() {
        // Square to the axis: an annulus, a plane.
        let ring = revolved(Point3::new(10.0, 0.0, 5.0), Point3::new(13.0, 0.0, 5.0));
        assert_eq!(ring.degrees().0.max(ring.degrees().1), 2);
        let Some(Known::Plane { origin, normal }) = known(&ring) else {
            panic!("{:?}", known(&ring));
        };
        assert!((origin.z - 5.0).abs() < 1e-9 && normal.z.abs() > 1.0 - 1e-9);
        // Along it: a cylinder.
        let side = revolved(Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 0.0, 8.0));
        let Some(Known::Cylinder {
            origin,
            axis,
            radius,
        }) = known(&side)
        else {
            panic!("{:?}", known(&side));
        };
        assert!((radius - 10.0).abs() < 1e-9 && axis.z.abs() > 1.0 - 1e-9);
        assert!(origin.x.abs() < 1e-9 && origin.y.abs() < 1e-9);
        // Slanted: a cone with its apex on the axis at z = -10.
        let slant = revolved(Point3::new(10.0, 0.0, 0.0), Point3::new(20.0, 0.0, 10.0));
        let Some(Known::Cone {
            apex,
            axis,
            half_angle,
        }) = known(&slant)
        else {
            panic!("{:?}", known(&slant));
        };
        assert!(apex.distance(Point3::new(0.0, 0.0, -10.0)) < 1e-6, "{apex}");
        assert!(axis.dot(Point3::Z) > 1.0 - 1e-9);
        assert!((half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-9);
    }
}
