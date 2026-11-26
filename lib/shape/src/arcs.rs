//! The NURBS form of circular and elliptic arcs: what sweeping around an
//! axis is made of.

use workpiece_core::math::from_angle;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::Curve3;

/// The control points (in the plane of the arc, as (x, y) of the unit
/// circle), weights and knots of the arc over `angle` from 0,
/// counter-clockwise, on `[0, 1]`: as few quadratic pieces as keep each
/// within a quarter turn.
pub fn arc_form(angle: f64) -> (Vec<(Point2, f64)>, Vec<f64>) {
    let n = ((angle / std::f64::consts::FRAC_PI_2) - 1e-9)
        .ceil()
        .max(1.0) as usize;
    let step = angle / n as f64;
    let w = libm::cos(step / 2.0);
    let mut points = vec![(Point2::X, 1.0)];
    let mut knots = vec![0.0; 3];
    for k in 0..n {
        let (mid, end) = (step * (k as f64 + 0.5), step * (k as f64 + 1.0));
        points.push((from_angle(mid) / w, w));
        points.push((from_angle(end), 1.0));
        let knot = (k + 1) as f64 / n as f64;
        knots.extend([knot, knot]);
    }
    knots.push(1.0);
    (points, knots)
}

/// The arc of the ellipse around `center` with half-axes `x` and `y`
/// (a circle if they are as long and square), from `center + x` towards
/// `center + y` over `angle` (2π for all of it), on `[0, 1]`.
pub fn ellipse(center: Point3, x: Point3, y: Point3, angle: f64) -> Curve3 {
    let (points, knots) = arc_form(angle);
    let (points, weights) = points
        .into_iter()
        .map(|(p, w)| (center + x * p.x + y * p.y, w))
        .unzip();
    Curve3::new(2, knots, points, Some(weights)).expect("a valid arc")
}
