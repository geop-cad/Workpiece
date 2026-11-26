//! Cases of our own for what brep-bench has no cases for: standard
//! parts, surfaces trimmed and rounded, curves in space, meshes read as
//! solids, files written and read back. Each has an answer in closed
//! form, and is graded as brep-bench grades: to a relative 1e-6.

use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use glam::DQuat;
use workpiece_boolean::{Solid, face_area, measure};
use workpiece_core::math::Point3;
use workpiece_core::world::topo::Curve3;
use workpiece_ops_standard::{StandardKind, standard};
use workpiece_ops_surface::{crossing, through};
use workpiece_shape::{faces_of, rounded, trimmed};

use crate::scene::Scene;
use crate::{Case, Grade};

/// What a case found, and what it should have.
type Found = anyhow::Result<(f64, f64)>;

/// The face of `solid` that is best by `score`, as a sheet of its own.
fn face_by(solid: &Solid, score: &dyn Fn(usize) -> f64) -> Solid {
    let best = (0..solid.faces.len())
        .max_by(|a, b| score(*a).total_cmp(&score(*b)))
        .unwrap_or(0);
    faces_of(solid, &[best])
}

/// The face of `solid` furthest along `way`: by the middle of its
/// corners.
fn face_towards(solid: &Solid, way: Point3) -> Solid {
    face_by(solid, &|f| {
        let mut corners = Vec::new();
        for step in solid.faces[f].loops.iter().flatten() {
            if let workpiece_boolean::model::Walk::Edge { edge, .. } = step.walk {
                corners.extend(solid.edges[edge].ends.map(|v| solid.vertices[v].point));
            }
        }
        let n = corners.len().max(1) as f64;
        corners.iter().map(|p| p.dot(way)).sum::<f64>() / n
    })
}

fn area(sheet: &Solid) -> f64 {
    (0..sheet.faces.len()).map(|f| face_area(sheet, f)).sum()
}

/// How long `pieces` are together.
fn long(pieces: &[(Arc<Curve3>, (f64, f64))]) -> f64 {
    let each = pieces.iter().map(|(curve, (from, to))| {
        let at = |k: u32| curve.point_at(from + (to - from) * f64::from(k) / 8192.0);
        (0..8192).map(|k| at(k).distance(at(k + 1))).sum::<f64>()
    });
    each.sum()
}

fn hexagon(flats: f64) -> f64 {
    3.0_f64.sqrt() / 2.0 * flats * flats
}

fn run(name: &str) -> Found {
    let s = &mut Scene::new();
    let turn = DQuat::IDENTITY;
    Ok(match name {
        "nut-m6" => (
            measure(&standard("n", StandardKind::HexNut, "M6", 0.0)?).volume,
            (hexagon(10.0) - PI * 9.0) * 5.2,
        ),
        "screw-m8x30" => (
            measure(&standard("s", StandardKind::HexScrew, "M8", 30.0)?).volume,
            hexagon(13.0) * 5.3 + PI * 16.0 * 30.0,
        ),
        "washer-m10" => (
            measure(&standard("w", StandardKind::Washer, "M10", 0.0)?).volume,
            PI * (100.0 - 5.25 * 5.25) * 2.0,
        ),
        "bearing-6204" => (
            measure(&standard("b", StandardKind::Bearing, "6204", 0.0)?).volume,
            PI * (23.5 * 23.5 - 100.0) * 14.0,
        ),
        "screw-m16-area" => (
            measure(&standard("s", StandardKind::HexScrew, "M16", 60.0)?).area,
            // The head all around, less where the shank stands on it,
            // and the shank's side and end.
            2.0 * hexagon(24.0) + 6.0 * 24.0 / 3.0_f64.sqrt() * 10.0 + TAU * 8.0 * 60.0,
        ),
        // Where a cylinder's side crosses a plate's top: a circle.
        "cylinder-crosses-plane" => {
            let rod = s.cylinder([0.0, 0.0, -5.0], 2.0, 10.0, turn)?;
            let plate = s.cube([0.0, 0.0, 0.0], [10.0, 10.0, 2.0], turn)?;
            let side = face_by(&rod, &|f| face_area(&rod, f));
            let top = face_towards(&plate, Point3::Z);
            (long(&crossing(&side, &top)?), TAU * 2.0)
        }
        // Where a ball crosses a plane off its middle: a smaller circle.
        "ball-crosses-plane" => {
            let ball = s.ball([0.0; 3], 5.0, turn)?;
            let plate = s.cube([0.0, 0.0, 2.0], [20.0, 20.0, 2.0], turn)?;
            let skin = faces_of(&ball, &(0..ball.faces.len()).collect::<Vec<_>>());
            let top = face_towards(&plate, Point3::Z);
            (long(&crossing(&skin, &top)?), TAU * 4.0)
        }
        // Two sides of a block cross in the edge between them.
        "block-sides-cross" => {
            let block = s.cube([0.0; 3], [4.0, 6.0, 8.0], turn)?;
            let (top, front) = (
                face_towards(&block, Point3::Z),
                face_towards(&block, -Point3::Y),
            );
            (long(&crossing(&top, &front)?), 4.0)
        }
        // A cylinder's side cut back to a plane through its axis: half.
        "cylinder-side-trimmed" => {
            let rod = s.cylinder([0.0; 3], 1.5, 4.0, turn)?;
            let side = face_by(&rod, &|f| face_area(&rod, f));
            (
                area(&trimmed(&side, Point3::ZERO, Point3::X)?),
                PI * 1.5 * 4.0,
            )
        }
        // A ball's skin cut back to a plane off its middle: a cap.
        "ball-skin-trimmed" => {
            let ball = s.ball([0.0; 3], 5.0, turn)?;
            let skin = faces_of(&ball, &(0..ball.faces.len()).collect::<Vec<_>>());
            // (What is below z = 3 stays: all but a cap 2 high.)
            let kept = trimmed(&skin, Point3::new(0.0, 0.0, 3.0), Point3::Z)?;
            (area(&kept), 4.0 * PI * 25.0 - TAU * 5.0 * 2.0)
        }
        // Two sides of a block as one surface, the edge between them
        // rounded.
        "sheet-edge-rounded" => {
            let block = s.cube([0.0; 3], [4.0, 4.0, 2.0], turn)?;
            let (top, front) = (
                face_towards(&block, Point3::Z),
                face_towards(&block, -Point3::Y),
            );
            let (sheet, _) = workpiece_boolean::stitched(&[top, front], 1e-6)?;
            let mut walked = vec![0; sheet.edges.len()];
            for step in sheet.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
                if let workpiece_boolean::model::Walk::Edge { edge, .. } = step.walk {
                    walked[edge] += 1;
                }
            }
            let inner: Vec<usize> = (0..walked.len()).filter(|e| walked[*e] == 2).collect();
            let r = 0.5;
            (
                area(&rounded(&sheet, &inner, r, "round")?),
                (4.0 - r) * 4.0 + (2.0 - r) * 4.0 + PI * r / 2.0 * 4.0,
            )
        }
        // A closed mesh read as a solid: a tetrahedron.
        "mesh-tetrahedron" => {
            let p = [
                Point3::ZERO,
                Point3::X * 3.0,
                Point3::Y * 3.0,
                Point3::Z * 3.0,
            ];
            let faces = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];
            (
                measure(&workpiece_boolean::faceted("m", &p, &faces)?).volume,
                4.5,
            )
        }
        // A block written as STEP in inches and read back: 25.4 times
        // the size.
        "step-in-inches" => {
            let block = s.cube([0.0; 3], [1.0, 2.0, 3.0], turn)?;
            let text = workpiece_ops_step::export(
                "part",
                &[("b".into(), block)],
                workpiece_core::Unit::Inch,
            )?;
            let back = workpiece_ops_step::import(text.as_bytes())?;
            let body = back
                .bodies
                .first()
                .ok_or_else(|| anyhow::anyhow!("no body came back"))?;
            (measure(&body.solid).volume, 6.0 * 25.4_f64.powi(3))
        }
        // A curve through points goes through them: how far it is from
        // the last, beside 1.
        "spline-through-points" => {
            let points = [
                Point3::ZERO,
                Point3::new(1.0, 2.0, 0.5),
                Point3::new(3.0, 2.5, 2.0),
                Point3::new(4.0, 0.0, 3.0),
            ];
            let curve = through(&points, false, Default::default())?;
            // (Its parameter is how far along the points it is.)
            let mut at = 0.0;
            let off: f64 = (0..4)
                .map(|k| {
                    if k > 0 {
                        at += points[k - 1].distance(points[k]);
                    }
                    curve.point_at(at).distance(points[k])
                })
                .sum();
            (1.0 + off, 1.0)
        }
        // A helix is as long as its turns unrolled.
        "helix-length" => {
            let helix = workpiece_shape::swept::helix(
                Point3::ZERO,
                Point3::Z,
                Point3::X * 2.0,
                0.5,
                3.0,
                0.0,
                false,
            )?;
            let range = helix.domain();
            (
                long(&[(Arc::new(helix), range)]),
                3.0 * libm::hypot(TAU * 2.0, 0.5),
            )
        }
        other => anyhow::bail!("{other}: not made"),
    })
}

/// The cases.
pub fn cases() -> Vec<Case> {
    [
        "nut-m6",
        "screw-m8x30",
        "washer-m10",
        "bearing-6204",
        "screw-m16-area",
        "cylinder-crosses-plane",
        "ball-crosses-plane",
        "block-sides-cross",
        "cylinder-side-trimmed",
        "ball-skin-trimmed",
        "sheet-edge-rounded",
        "mesh-tetrahedron",
        "step-in-inches",
        "spline-through-points",
        "helix-length",
    ]
    .into_iter()
    .map(|name| Case {
        category: "features",
        name: name.to_owned(),
        run: Box::new(move || match run(name) {
            Err(why) => Grade::Refused(format!("{why:#}")),
            Ok((got, wanted)) if ((got - wanted) / wanted).abs() > 1e-6 => {
                Grade::Wrong(format!("{got} is not {wanted}"))
            }
            Ok(_) => Grade::Pass,
        }),
    })
    .collect()
}
