//! The meshes to start from: a box, a plane, a cylinder, a cone, a
//! sphere of quads and a torus.

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use workpiece_core::math::Point3;

use crate::poly::Poly;

/// A mesh to start from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    /// A box of quads.
    Box,
    /// A flat square of quads.
    Plane,
    /// A cylinder: quads around, a face at each end.
    Cylinder,
    /// A cone: triangles up to its tip, a face below.
    Cone,
    /// A sphere of quads: a box blown up round.
    Sphere,
    /// A torus of quads.
    Torus,
}

impl Primitive {
    /// All of them.
    pub const ALL: [Primitive; 6] = [
        Primitive::Box,
        Primitive::Plane,
        Primitive::Cylinder,
        Primitive::Cone,
        Primitive::Sphere,
        Primitive::Torus,
    ];

    /// What it is called.
    pub fn label(self) -> &'static str {
        match self {
            Primitive::Box => "Box",
            Primitive::Plane => "Plane",
            Primitive::Cylinder => "Cylinder",
            Primitive::Cone => "Cone",
            Primitive::Sphere => "Sphere",
            Primitive::Torus => "Torus",
        }
    }

    /// How many pieces it is cut in when nothing is said.
    pub fn pieces(self) -> usize {
        match self {
            Primitive::Box => 1,
            Primitive::Plane => 4,
            Primitive::Cylinder | Primitive::Cone => 8,
            Primitive::Sphere => 2,
            Primitive::Torus => 8,
        }
    }

    /// The mesh, `size` across and about `at`, cut in `pieces`: along
    /// each side of a box, a grid or a sphere's faces, around a
    /// cylinder, a cone or a torus.
    pub fn mesh(self, size: f64, pieces: usize, at: Point3) -> Poly {
        let half = size / 2.0;
        let mut made = match self {
            Primitive::Box => cube(pieces.max(1), half),
            Primitive::Plane => grid(pieces.max(1), half),
            Primitive::Cylinder => cylinder(pieces.max(3), half, false),
            Primitive::Cone => cylinder(pieces.max(3), half, true),
            Primitive::Sphere => {
                let mut ball = cube(pieces.max(1), 1.0);
                for p in &mut ball.points {
                    *p = p.normalize_or_zero() * half;
                }
                ball
            }
            Primitive::Torus => torus(pieces.max(3), half),
        };
        for p in &mut made.points {
            *p += at;
        }
        made
    }
}

/// A grid in the plane z = 0, `n` quads along each side.
fn grid(n: usize, half: f64) -> Poly {
    let mut made = Poly::default();
    for j in 0..=n {
        for i in 0..=n {
            // (Of whole numbers first: the coordinates read back as written.)
            let at = |k: usize| half * (2.0 * k as f64 - n as f64) / n as f64;
            made.points.push(Point3::new(at(i), at(j), 0.0));
        }
    }
    let index = |i: usize, j: usize| j * (n + 1) + i;
    for j in 0..n {
        for i in 0..n {
            made.faces.push(vec![
                index(i, j),
                index(i + 1, j),
                index(i + 1, j + 1),
                index(i, j + 1),
            ]);
        }
    }
    made
}

/// A box, `n` quads along each side: the points on its sides are
/// shared between them.
fn cube(n: usize, half: f64) -> Poly {
    let mut made = Poly::default();
    let mut found: BTreeMap<[usize; 3], usize> = BTreeMap::new();
    let mut point = |made: &mut Poly, at: [usize; 3]| {
        *found.entry(at).or_insert_with(|| {
            let c = at.map(|k| -half + 2.0 * half * k as f64 / n as f64);
            made.points.push(Point3::from_array(c));
            made.points.len() - 1
        })
    };
    // Each side: the axis it faces along, whether up it, and its two
    // axes turned so that it is walked counter-clockwise from outside.
    for (axis, up) in [
        (0, false),
        (0, true),
        (1, false),
        (1, true),
        (2, false),
        (2, true),
    ] {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for j in 0..n {
            for i in 0..n {
                let at = |i: usize, j: usize| {
                    let mut c = [0; 3];
                    c[axis] = if up { n } else { 0 };
                    c[u] = i;
                    c[v] = j;
                    c
                };
                let mut ring = vec![at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
                if !up {
                    ring.reverse();
                }
                let ring = ring.into_iter().map(|c| point(&mut made, c)).collect();
                made.faces.push(ring);
            }
        }
    }
    made
}

/// A cylinder along z with `n` sides, or a cone up to a tip.
fn cylinder(n: usize, half: f64, cone: bool) -> Poly {
    let mut made = Poly::default();
    let around = |k: usize, z: f64| {
        let a = TAU * k as f64 / n as f64;
        Point3::new(half * libm::cos(a), half * libm::sin(a), z)
    };
    made.points.extend((0..n).map(|k| around(k, -half)));
    made.faces.push((0..n).rev().collect());
    if cone {
        made.points.push(Point3::new(0.0, 0.0, half));
        for k in 0..n {
            made.faces.push(vec![k, (k + 1) % n, n]);
        }
    } else {
        made.points.extend((0..n).map(|k| around(k, half)));
        made.faces.push((n..2 * n).collect());
        for k in 0..n {
            let next = (k + 1) % n;
            made.faces.push(vec![k, next, n + next, n + k]);
        }
    }
    made
}

/// A torus about z, `n` quads around it and `n / 2` (at least three)
/// around its tube.
fn torus(n: usize, half: f64) -> Poly {
    let m = (n / 2).max(3);
    let (tube, ring) = (half / 4.0, half * 3.0 / 4.0);
    let mut made = Poly::default();
    for i in 0..n {
        let a = TAU * i as f64 / n as f64;
        for j in 0..m {
            let b = TAU * j as f64 / m as f64;
            let r = ring + tube * libm::cos(b);
            made.points.push(Point3::new(
                r * libm::cos(a),
                r * libm::sin(a),
                tube * libm::sin(b),
            ));
        }
    }
    let index = |i: usize, j: usize| (i % n) * m + j % m;
    for i in 0..n {
        for j in 0..m {
            made.faces.push(vec![
                index(i, j),
                index(i + 1, j),
                index(i + 1, j + 1),
                index(i, j + 1),
            ]);
        }
    }
    made
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six times the volume a closed mesh encloses.
    fn volume(mesh: &Poly) -> f64 {
        let mut sum = 0.0;
        for ring in &mesh.faces {
            for k in 1..ring.len() - 1 {
                let [a, b, c] = [ring[0], ring[k], ring[k + 1]].map(|v| mesh.points[v]);
                sum += a.dot(b.cross(c));
            }
        }
        sum
    }

    #[test]
    fn the_closed_ones_are_closed_and_face_outwards() {
        for kind in Primitive::ALL {
            let mesh = kind.mesh(2.0, kind.pieces(), Point3::ZERO);
            let open = mesh.edges().values().filter(|s| s.len() != 2).count();
            if kind == Primitive::Plane {
                assert_eq!(open, 16);
                continue;
            }
            assert_eq!(open, 0, "{kind:?} is open");
            assert!(volume(&mesh) > 0.0, "{kind:?} faces inwards");
        }
    }
}
