//! The curvature-continuous section of a round: Hartmann's blend
//! section of order 2 between two legs (E. Hartmann, "Parametric Gⁿ
//! blending of curves and surfaces", The Visual Computer 17, 2001).
//!
//! From `a` to `b`, with the legs the lines from `apex` to each, it is
//! `apex + α(t)(a − apex) + β(t)(b − apex)`: it leaves each leg as
//! straight as the leg is. Its one number, the *thumbweight* `w` (less
//! than 1), says how full it is; the nearer 1, the sharper its middle.
//! It is a rational curve of degree 5, and is given as that: exactly.

use workpiece_core::math::Point3;

/// The section's order: it meets its legs with their curvature.
const ORDER: i32 = 2;

/// The weights (α, β) of the legs at `t`, and their derivatives.
fn weights(w: f64, t: f64) -> ([f64; 2], [f64; 2]) {
    let m = ORDER + 1;
    let (s, u) = (1.0 - t, t);
    let (big_a, big_b) = (s.powi(m), u.powi(m));
    let (da, db) = (-f64::from(m) * s.powi(m - 1), f64::from(m) * u.powi(m - 1));
    let sum = big_a + big_b;
    let f = big_a / sum;
    let df = (da * big_b - big_a * db) / (sum * sum);
    let (d1, d2) = (1.0 - w + w * t, 1.0 - w * t);
    let (p1, p2) = (t / d1, t * (1.0 - w) / d2);
    let (dp1, dp2) = ((1.0 - w) / (d1 * d1), (1.0 - w) / (d2 * d2));
    (
        [f * (1.0 - p1), (1.0 - f) * p2],
        [df * (1.0 - p1) - f * dp1, -df * p2 + (1.0 - f) * dp2],
    )
}

/// The share of the triangle on its legs that a section of thumbweight
/// `w` takes off.
fn area_share(w: f64) -> f64 {
    const X: [f64; 5] = [
        0.0,
        -0.538_469_310_105_683_1,
        0.538_469_310_105_683_1,
        -0.906_179_845_938_664,
        0.906_179_845_938_664,
    ];
    const W: [f64; 5] = [
        0.568_888_888_888_888_9,
        0.478_628_670_499_366_5,
        0.478_628_670_499_366_5,
        0.236_926_885_056_189_1,
        0.236_926_885_056_189_1,
    ];
    const PANELS: usize = 256;
    let h = 1.0 / PANELS as f64;
    let mut sum = 0.0;
    for k in 0..PANELS {
        let mid = (k as f64 + 0.5) * h;
        for (x, weight) in X.iter().zip(W) {
            let (v, d) = weights(w, mid + 0.5 * h * x);
            sum += weight * 0.5 * h * (v[0] * d[1] - d[0] * v[1]);
        }
    }
    sum
}

/// The thumbweight `f` comes down to `target` at, as it rises to 1.
fn falling_to(f: &dyn Fn(f64) -> f64, target: f64) -> f64 {
    let (mut lo, mut hi) = (-1.0_f64, 1.0 - 1e-9);
    while f(lo) < target && lo > -1e6 {
        hi = lo;
        lo = 2.0 * lo - 1.0;
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if f(mid) > target {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo <= 1e-15 * (1.0 - lo) {
            break;
        }
    }
    0.5 * (lo + hi)
}

/// The thumbweight at which the section between legs that meet at the
/// angle `alpha` takes off what the circle touching both legs where the
/// section leaves them does.
pub fn even(alpha: f64) -> f64 {
    let c = 1.0 / (alpha / 2.0).tan();
    let target = (c - (std::f64::consts::PI - alpha) / 2.0) / (c * (alpha / 2.0).cos().powi(2));
    falling_to(&area_share, target)
}

/// The radius the section from `a` to `b` about `apex` bends by at its
/// middle, at the thumbweight `w`.
pub fn middle_radius(a: Point3, apex: Point3, b: Point3, w: f64) -> f64 {
    let (la, lb) = (a - apex, b - apex);
    let (t, h) = (0.5, 1e-3);
    let (_, d1) = weights(w, t);
    let second = |k: usize| {
        let at = |s: f64| weights(w, t + s * h).1[k];
        (at(-2.0) - 8.0 * at(-1.0) + 8.0 * at(1.0) - at(2.0)) / (12.0 * h)
    };
    let first = la * d1[0] + lb * d1[1];
    let bend = la * second(0) + lb * second(1);
    first.length().powi(3) / first.cross(bend).length()
}

/// The thumbweight at which the section from `a` to `b` about `apex`
/// bends by the radius `centre` at its middle.
pub fn centred(a: Point3, apex: Point3, b: Point3, centre: f64) -> f64 {
    falling_to(&|w| middle_radius(a, apex, b, w), centre)
}

/// The section from `a` to `b` about `apex` at the thumbweight `w`: the
/// control points of a rational curve of degree 5, each with its weight.
pub fn section(a: Point3, apex: Point3, b: Point3, w: f64) -> Vec<[f64; 4]> {
    // By powers of t, to the fifth: what the weights of the legs are
    // over, and what is over it for each leg.
    let times = |p: &[f64], q: &[f64]| {
        let mut out = vec![0.0; p.len() + q.len() - 1];
        for (i, x) in p.iter().enumerate() {
            for (j, y) in q.iter().enumerate() {
                out[i + j] += x * y;
            }
        }
        out
    };
    let (d1, d2) = ([1.0 - w, w], [1.0, -w]);
    let under = times(&times(&[1.0, -3.0, 3.0], &d1), &d2);
    let s4 = [1.0, -4.0, 6.0, -4.0, 1.0];
    let t4 = [0.0, 0.0, 0.0, 0.0, 1.0];
    let scaled = |p: Vec<f64>| -> Vec<f64> { p.iter().map(|x| x * (1.0 - w)).collect() };
    let (alpha, beta) = (scaled(times(&s4, &d2)), scaled(times(&t4, &d1)));
    // The same between its ends (in Bernstein's polynomials).
    let choose = |n: usize, k: usize| (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64);
    let between = |p: &[f64]| -> Vec<f64> {
        (0..=5)
            .map(|i| {
                (0..=i.min(p.len() - 1))
                    .map(|k| choose(i, k) / choose(5, k) * p[k])
                    .sum()
            })
            .collect()
    };
    let (under, alpha, beta) = (between(&under), between(&alpha), between(&beta));
    (0..=5)
        .map(|i| {
            let p = apex + ((a - apex) * alpha[i] + (b - apex) * beta[i]) / under[i];
            [p.x, p.y, p.z, under[i]]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_is_the_curve_it_is_said_to_be() {
        let (a, apex, b) = (Point3::X * 2.0, Point3::ZERO, Point3::new(0.5, 1.5, 0.0));
        for w in [-1.5, 0.2, even(1.2), 0.9] {
            let points = section(a, apex, b, w);
            assert!(points.iter().all(|p| p[3] > 0.0), "weights at {w}");
            for t in [0.0, 0.1, 0.5, 0.83, 1.0] {
                let ([alpha, beta], _) = weights(w, t);
                let want = a * alpha + b * beta;
                // De Casteljau's, with the weights.
                let mut h: Vec<[f64; 4]> = points
                    .iter()
                    .map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
                    .collect();
                while h.len() > 1 {
                    h = h
                        .windows(2)
                        .map(|x| std::array::from_fn(|k| x[0][k] * (1.0 - t) + x[1][k] * t))
                        .collect();
                }
                let got = Point3::new(h[0][0], h[0][1], h[0][2]) / h[0][3];
                assert!(got.distance(want) < 1e-12, "at {t} of {w}");
            }
        }
        // (At a square corner, the middle of the one that takes off
        // what the round does bends by about 1.389 of its radius.)
        let w = even(std::f64::consts::FRAC_PI_2);
        let r = middle_radius(Point3::X, Point3::ZERO, Point3::Y, w);
        assert!((r - 1.389).abs() < 1e-3, "{r}");
        let again = centred(Point3::X, Point3::ZERO, Point3::Y, r);
        assert!((again - w).abs() < 1e-9);
    }
}
