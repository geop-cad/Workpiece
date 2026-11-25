//! Mass properties: how the volume of bodies is spread — what their
//! mass, their centre of gravity and their moments of inertia follow
//! from, given what each is made of.

use crate::math::Point3;

/// How the volume of one body is spread.
#[derive(Debug, Clone, PartialEq)]
pub struct Spread {
    /// The body's name.
    pub body: String,
    /// Its volume.
    pub volume: f64,
    /// Its centre: its centre of gravity, as it is of one material.
    pub centre: Point3,
    /// Its moments of inertia about its centre for a density of one,
    /// along the axes of space, as the tensor has them (the products
    /// of inertia negative).
    pub inertia: [[f64; 3]; 3],
}

/// The mass properties a step asks for: of these bodies, taken
/// together. What they weigh is told where what they are made of is
/// known.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MassProperties {
    /// The bodies, each with how its volume is spread.
    pub bodies: Vec<Spread>,
}

/// Bodies taken together, each with the density it has (what a unit of
/// its volume weighs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weighed {
    /// What they weigh.
    pub mass: f64,
    /// Their volume.
    pub volume: f64,
    /// Their centre of gravity.
    pub centre: Point3,
    /// Their moments of inertia about that centre, along the axes of
    /// space, as the tensor has them.
    pub inertia: [[f64; 3]; 3],
    /// Their principal moments of inertia, least first.
    pub principal: [f64; 3],
    /// The principal axes those are about (unit vectors).
    pub axes: [Point3; 3],
    /// The half axes of the ellipsoid of one density that weighs as
    /// much and turns as they do, along the principal axes.
    pub half: [f64; 3],
}

impl MassProperties {
    /// The bodies taken together, `density` saying what a unit of
    /// volume of each (by its name) weighs. `None` if they weigh
    /// nothing.
    pub fn weighed(&self, density: impl Fn(&str) -> f64) -> Option<Weighed> {
        let (mut mass, mut volume, mut moment) = (0.0, 0.0, Point3::ZERO);
        for b in &self.bodies {
            let m = b.volume * density(&b.body);
            mass += m;
            volume += b.volume;
            moment += b.centre * m;
        }
        if mass.abs() < 1e-300 {
            return None;
        }
        let centre = moment / mass;
        let mut inertia = [[0.0; 3]; 3];
        for b in &self.bodies {
            let d = density(&b.body);
            let m = b.volume * d;
            let off = (b.centre - centre).to_array();
            let square: f64 = off.iter().map(|x| x * x).sum();
            for r in 0..3 {
                for c in 0..3 {
                    // (Each about its own centre, moved to theirs.)
                    let own = if r == c { square } else { 0.0 };
                    inertia[r][c] += d * b.inertia[r][c] + m * (own - off[r] * off[c]);
                }
            }
        }
        let (principal, axes) = principal(inertia);
        let [a, b, c] = principal;
        let half = [b + c - a, a + c - b, a + b - c].map(|x| (2.5 * x.max(0.0) / mass).sqrt());
        Some(Weighed {
            mass,
            volume,
            centre,
            inertia,
            principal,
            axes,
            half,
        })
    }
}

/// The principal moments of the symmetric tensor `m`, least first, and
/// the axes they are about: by Jacobi's turns.
fn principal(mut m: [[f64; 3]; 3]) -> ([f64; 3], [Point3; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..60 {
        let off = m[0][1].abs() + m[0][2].abs() + m[1][2].abs();
        let scale = m[0][0].abs() + m[1][1].abs() + m[2][2].abs();
        if off <= 1e-15 * scale {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if m[p][q].abs() < 1e-300 {
                continue;
            }
            let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for row in &mut m {
                let (a, b) = (row[p], row[q]);
                row[p] = c * a - s * b;
                row[q] = s * a + c * b;
            }
            let (top, bottom) = (m[p], m[q]);
            for k in 0..3 {
                m[p][k] = c * top[k] - s * bottom[k];
                m[q][k] = s * top[k] + c * bottom[k];
            }
            for row in &mut v {
                let (a, b) = (row[p], row[q]);
                row[p] = c * a - s * b;
                row[q] = s * a + c * b;
            }
        }
    }
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| m[*a][*a].total_cmp(&m[*b][*b]));
    let axis = |k: usize| Point3::new(v[0][k], v[1][k], v[2][k]).normalize_or_zero();
    (order.map(|k| m[k][k]), order.map(axis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_blocks_weigh_and_turn_as_one() {
        // Two unit cubes of density 2 side by side along x: a 2 × 1 × 1 block.
        let cube = |x: f64, name: &str| Spread {
            body: name.to_owned(),
            volume: 1.0,
            centre: Point3::new(x, 0.5, 0.5),
            inertia: [
                [1.0 / 6.0, 0.0, 0.0],
                [0.0, 1.0 / 6.0, 0.0],
                [0.0, 0.0, 1.0 / 6.0],
            ],
        };
        let both = MassProperties {
            bodies: vec![cube(0.5, "a"), cube(1.5, "b")],
        };
        let w = both.weighed(|_| 2.0).expect("they weigh");
        assert!((w.mass - 4.0).abs() < 1e-12);
        assert!((w.centre - Point3::new(1.0, 0.5, 0.5)).length() < 1e-12);
        // m (b² + c²) / 12 about x, m (a² + c²) / 12 about y and z.
        let wanted = [4.0 * 2.0 / 12.0, 4.0 * 5.0 / 12.0, 4.0 * 5.0 / 12.0];
        for (found, wanted) in w.principal.iter().zip(wanted) {
            assert!((found - wanted).abs() < 1e-12, "{:?}", w.principal);
        }
        assert!(w.axes[0].cross(Point3::X).length() < 1e-9);
        // The ellipsoid that turns as they do is longest along x.
        assert!(w.half[0] > w.half[1] && (w.half[1] - w.half[2]).abs() < 1e-9);
    }
}
