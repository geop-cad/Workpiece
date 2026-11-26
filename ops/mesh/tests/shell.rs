//! Smooth subdivision bodies shelled and offset: the faces moved off a
//! network of patches stay one smooth closed wall.

use workpiece_boolean::Solid;
use workpiece_boolean::model::Walk;
use workpiece_core::math::Point3;
use workpiece_ops_mesh::poly::Poly;
use workpiece_ops_mesh::{Primitive, body, ops};
use workpiece_ops_testing::sound;
use workpiece_shape::thick;

/// A body with a spike, a five-sided face and so points where three
/// and five faces meet, mirrored across y = 0: about 4 × 10 × 2.
fn spiked() -> Poly {
    let points = [
        [-1.0, 1.0, 1.0],
        [-1.0, 1.0, -1.0],
        [1.0, 1.0, -1.0],
        [1.0, 5.06, 1.0],
        [3.0, 1.0, -1.0],
        [3.0, 1.0, 1.0],
        [-1.0, 0.0, 1.0],
        [-1.0, 0.0, -1.0],
        [1.0, 0.0, -1.0],
        [3.0, 0.0, -1.0],
        [3.0, 0.0, 1.0],
    ];
    let half = Poly {
        points: points.iter().map(|p| Point3::from_array(*p)).collect(),
        faces: vec![
            vec![6, 0, 1, 7],
            vec![9, 4, 5, 10],
            vec![1, 0, 3, 2],
            vec![8, 2, 4, 9],
            vec![2, 3, 5, 4],
            vec![10, 5, 3, 0, 6],
            vec![7, 1, 2, 8],
        ],
        ..Poly::default()
    };
    ops::mirrored(&half, [false, true, false], half.size() * 1e-6)
}

fn smooth(poly: &Poly) -> Solid {
    let mut made = body::bodies(poly, true, 0, "subd").unwrap();
    assert_eq!(made.len(), 1);
    let piece = made.remove(0);
    assert!(piece.closed);
    piece.solid
}

fn face(solid: &Solid, name: &str) -> usize {
    solid
        .faces
        .iter()
        .position(|f| f.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no face {name}"))
}

/// The largest angle (radians) between the outward normals of two
/// faces of `solid` meeting at an edge, of the faces `which` picks.
fn worst_seam(solid: &Solid, which: &dyn Fn(&str) -> bool) -> f64 {
    let mut worst: f64 = 0.0;
    for (e, edge) in solid.edges.iter().enumerate() {
        let mut normals: Vec<Vec<Point3>> = Vec::new();
        for f in &solid.faces {
            if !which(f.name.as_deref().unwrap_or("")) {
                continue;
            }
            for step in f.loops.iter().flatten() {
                if matches!(step.walk, Walk::Edge { edge: w, .. } if w == e) {
                    let (a, b) = edge.range;
                    let ns = (0..9).map(|k| {
                        let uv = step
                            .pcurve
                            .point_at(a + (b - a) * (f64::from(k) + 0.5) / 9.0);
                        let n = thick::normal(&f.surface, uv);
                        if f.outward { n } else { -n }
                    });
                    normals.push(ns.collect());
                }
            }
        }
        if let [a, b] = &normals[..] {
            for (x, y) in a.iter().zip(b) {
                worst = worst.max(libm::acos(x.dot(*y).clamp(-1.0, 1.0)));
            }
        }
    }
    worst
}

/// The area of the faces of `solid` that `which` picks, all whole
/// patches.
fn area(solid: &Solid, which: &dyn Fn(&str) -> bool) -> f64 {
    let n = 24;
    let mut sum = 0.0;
    for f in solid
        .faces
        .iter()
        .filter(|f| which(f.name.as_deref().unwrap_or("")))
    {
        let s = &f.surface;
        let ((u0, u1), (v0, v1)) = s.domain();
        let (hu, hv) = ((u1 - u0) / f64::from(n), (v1 - v0) / f64::from(n));
        for i in 0..n {
            for j in 0..n {
                let (u, v) = (
                    u0 + hu * (f64::from(i) + 0.5),
                    v0 + hv * (f64::from(j) + 0.5),
                );
                let (_, du, dv) = s.derivatives_at(u, v);
                sum += du.cross(dv).length() * hu * hv;
            }
        }
    }
    sum
}

/// A wall `t` thick between `outer` and `inner` of area: its volume, to
/// within what goes with the cube of `t` (and the rims' share).
fn wall(outer: f64, inner: f64, t: f64) -> f64 {
    t * (outer + inner) / 2.0
}

#[test]
fn a_subdivision_body_is_shelled() {
    let solid = smooth(&spiked());
    let open = face(&solid, "subd/f7");
    let t = 0.05;
    let hollow = thick::shelled(&solid, t, &[open]).unwrap();
    let (volume, _) = sound(&hollow).unwrap();
    let outer = area(&hollow, &|n| n.starts_with("subd/"));
    let inner = area(&hollow, &|n| n.ends_with("/far"));
    let wanted = wall(outer, inner, t);
    assert!(
        (volume - wanted).abs() < wanted * 2e-3,
        "{volume} is not {wanted}"
    );
    // (The outer faces less the opening, and the wall about as much
    // less again inside as it is curved.)
    assert!(inner < outer && inner > outer * 0.9, "{inner} of {outer}");
    // As smooth inside as out: the patches meet at a slight angle about
    // the points where not four meet, and the faces moved in off them
    // at the angle the faces moved in off two such ever more bent
    // patches do (as much more as the wall is thick for how round they
    // are there: half as much again).
    let outside = worst_seam(&solid, &|_| true);
    let inside = worst_seam(&hollow, &|n| n.ends_with("/far"));
    assert!(inside < outside * 1.6, "{inside} inside, {outside} out");

    // Its mirror image: the mirror image of the shell.
    let flip = |p: Point3| Point3::new(p.x, -p.y, p.z);
    let mirrored = solid.mapped(&flip, true, 1.0).unwrap();
    let other = thick::shelled(&mirrored, t, &[open]).unwrap();
    let (again, _) = sound(&other).unwrap();
    assert!(
        (again - volume).abs() < volume * 1e-9,
        "{again} is not {volume}"
    );
    for v in &other.vertices {
        let seen = hollow
            .vertices
            .iter()
            .any(|w| flip(w.point).distance(v.point) < 1e-6);
        assert!(seen, "{v:?} is in no mirror image");
    }

    // Thicker than it is round at the spike: refused, saying where.
    let error = thick::shelled(&solid, 0.2, &[open]).unwrap_err();
    let said = format!("{error:#}");
    assert!(
        said.contains("is bent more tightly than the wall is thick")
            && said.contains("walls up to 0.10"),
        "{said}"
    );
}

#[test]
fn a_subdivision_cube_is_shelled_and_offset() {
    let cube = smooth(&Primitive::Box.mesh(2.0, 1, Point3::ZERO));
    let (whole, _) = sound(&cube).unwrap();
    let t = 0.1;
    let open = 0;
    let hollow = thick::shelled(&cube, t, &[open]).unwrap();
    let (volume, _) = sound(&hollow).unwrap();
    let outer = area(&hollow, &|n| n.starts_with("subd/"));
    let inner = area(&hollow, &|n| n.ends_with("/far"));
    let wanted = wall(outer, inner, t);
    assert!(
        (volume - wanted).abs() < wanted * 2e-3,
        "{volume} is not {wanted}"
    );
    let outside = worst_seam(&cube, &|_| true);
    let inside = worst_seam(&hollow, &|n| n.ends_with("/far"));
    assert!(inside < outside * 1.6, "{inside} inside, {outside} out");

    // Offset all over: the body grown by that much.
    let all: Vec<usize> = (0..cube.faces.len()).collect();
    let grown = thick::faces_offset(&cube, &all, t).unwrap();
    let (volume, _) = sound(&grown).unwrap();
    let wanted = whole + wall(area(&cube, &|_| true), area(&grown, &|_| true), t);
    assert!(
        (volume - wanted).abs() < (wanted - whole) * 2e-3,
        "{volume} is not {wanted}"
    );
}
