//! The boolean and tangent cases of brep-bench (a third-party case set and
//! harness, run through `boolean-bench brep-run`), built with this kernel and
//! graded by brep-bench's rule: volume and area both within a relative
//! 1e-6 of what the case expects.
//!
//! `grazing-cylinders` is built for brep-bench's harness only: it is
//! unsettled there, with no answer to grade it by.

use std::f64::consts::FRAC_PI_2;
use workpiece_core::math::{from_angle, rotation, rotation_x, rotation_y, rotation_z};

use glam::DQuat;
use workpiece_boolean::{Op, Solid, combine};
use workpiece_core::math::Point3;
use workpiece_shape::{BlendShape, By, Section, blend_solid, drafted, thicken, thickened};

use crate::brepbench_expected::EXPECTED;
use crate::scene::{Scene, sound};
use crate::{Case, Grade};

const ID: DQuat = DQuat::IDENTITY;

/// A case's solid, or how building it failed.
pub type Built = Result<Solid, Grade>;

fn apply(op: Op, a: &Solid, b: &Solid) -> Built {
    combine(op, a, b)
        .map(|o| o.solid)
        .map_err(|why| Grade::Refused(why.to_string()))
}

fn setup<T>(made: anyhow::Result<T>) -> Result<T, Grade> {
    made.map_err(|e| Grade::Crashed(format!("setup: {e:#}")))
}

/// The block between two corners.
fn block(s: &mut Scene, lo: [f64; 3], hi: [f64; 3]) -> Built {
    let at = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0);
    let size = std::array::from_fn(|k| hi[k] - lo[k]);
    setup(s.cube(at, size, ID))
}

/// A cylinder along z, x or y (`axis` 2, 0, 1) from `from` to `to` on
/// that axis, through `at` on the others, its seam turned by `seam`.
fn rod(
    s: &mut Scene,
    axis: usize,
    at: [f64; 2],
    from: f64,
    to: f64,
    radius: f64,
    seam: f64,
) -> Built {
    let (turn, base) = match axis {
        0 => (rotation_y(FRAC_PI_2), [from, at[0], at[1]]),
        1 => (rotation_x(-FRAC_PI_2), [at[0], from, at[1]]),
        _ => (ID, [at[0], at[1], from]),
    };
    setup(s.cylinder(base, radius, to - from, turn * rotation_z(seam)))
}

fn ball(s: &mut Scene, at: [f64; 3], radius: f64, turn: DQuat) -> Built {
    setup(s.ball(at, radius, turn))
}

/// What turns the y axis solids are turned about into the z axis.
fn upright() -> DQuat {
    rotation_x(FRAC_PI_2)
}

/// The torus about z around the origin, turned about z by `seam`.
fn torus(s: &mut Scene, major: f64, minor: f64, seam: f64) -> Built {
    setup(s.turned(
        [0.0; 3],
        rotation_z(seam) * upright(),
        &[],
        Some(([major, 0.0], minor)),
        None,
    ))
}

/// The cone about z with its tip at height `tip` and its base of
/// `radius` at height `base`.
fn cone(s: &mut Scene, tip: f64, base: f64, radius: f64) -> Built {
    let h = base - tip;
    setup(s.turned(
        [0.0, 0.0, tip],
        upright(),
        &[[0.0, 0.0], [radius, h], [0.0, h]],
        None,
        None,
    ))
}

/// The paraboloid x² + y² = `p`·z about z from its vertex at the origin
/// up to the height `top`, closed flat there.
fn paraboloid(s: &mut Scene, p: f64, top: f64) -> Built {
    setup(s.turned(
        [0.0; 3],
        upright(),
        &[],
        None,
        Some([(p * top).sqrt(), top]),
    ))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6
}

/// The edges of `solid` that `take` takes by their start, middle and
/// end.
fn pick(solid: &Solid, take: impl Fn(Point3, Point3, Point3) -> bool) -> Vec<usize> {
    (0..solid.edges.len())
        .filter(|&e| {
            let edge = &solid.edges[e];
            take(
                edge.point(edge.range.0),
                edge.point(edge.middle()),
                edge.point(edge.range.1),
            )
        })
        .collect()
}

/// The first face along edge `e` of `solid`: the one a chamfer's first
/// distance is across.
fn first_face(solid: &Solid, e: usize) -> usize {
    solid
        .faces
        .iter()
        .position(|f| {
            f.loops.iter().flatten().any(|step| {
                matches!(step.walk, workpiece_boolean::model::Walk::Edge { edge, .. } if edge == e)
            })
        })
        .unwrap_or(0)
}

/// Whether face `f` of `solid` faces up.
fn is_top(solid: &Solid, f: usize) -> bool {
    let face = &solid.faces[f];
    let (u, v) = face.surface.domain();
    let (_, du, dv) = face
        .surface
        .derivatives_at((u.0 + u.1) / 2.0, (v.0 + v.1) / 2.0);
    du.cross(dv).normalize_or_zero().z.abs() > 0.99
}

fn round(radius: f64) -> BlendShape {
    growing(radius, radius)
}

/// A round whose radius grows from `from` at its edge's start to `to`.
fn growing(from: f64, to: f64) -> BlendShape {
    BlendShape::Fillet {
        by: By::Radius,
        size: from,
        until: (from != to).then_some(to),
        section: Section::Circular,
        rho: 0.5,
    }
}

fn flat(first: f64, second: f64) -> BlendShape {
    BlendShape::Chamfer { first, second }
}

/// `solid` with its edges `edges` blended: refused where the kernel
/// says it cannot.
fn blended(solid: &Solid, edges: &[usize], shape: &BlendShape, corners: bool) -> Built {
    if edges.is_empty() {
        return Err(Grade::Crashed("setup: no edge to blend was found".into()));
    }
    blend_solid(solid, "blend", edges, shape, corners)
        .map_err(|why| Grade::Refused(format!("{why:#}")))
}

/// Builds brep-bench's case `id`.
pub fn build(id: &str) -> Built {
    let s = &mut Scene::new();
    let far = 100.0;
    match id {
        "boolean/boxes-common" | "boolean/boxes-cut" | "boolean/boxes-union" => {
            let a = block(s, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0])?;
            let b = block(s, [5.0, 5.0, 0.0], [15.0, 15.0, 10.0])?;
            let op = match id {
                "boolean/boxes-common" => Op::Intersect,
                "boolean/boxes-cut" => Op::Cut,
                _ => Op::Union,
            };
            apply(op, &a, &b)
        }
        "boolean/boxes-sharing-a-face" => {
            let a = block(s, [0.0; 3], [10.0; 3])?;
            let b = block(s, [10.0, 0.0, 0.0], [20.0, 10.0, 10.0])?;
            apply(Op::Union, &a, &b)
        }
        "boolean/boxes-sharing-part-of-a-face" => {
            let a = block(s, [0.0; 3], [10.0; 3])?;
            let b = block(s, [5.0, 0.0, 10.0], [15.0, 10.0, 20.0])?;
            apply(Op::Union, &a, &b)
        }
        "boolean/box-minus-through-cylinder" => {
            let a = block(s, [0.0; 3], [10.0; 3])?;
            let b = rod(s, 2, [5.0, 5.0], -1.0, 11.0, 3.0, 0.0)?;
            apply(Op::Cut, &a, &b)
        }
        "boolean/drilled-plate" => {
            let mut plate = block(s, [0.0; 3], [100.0, 20.0, 10.0])?;
            for k in 1..=9 {
                let drill = rod(s, 2, [10.0 * f64::from(k), 10.0], -1.0, 11.0, 2.0, 0.0)?;
                plate = apply(Op::Cut, &plate, &drill)?;
            }
            Ok(plate)
        }
        "boolean/golf-ball" => {
            let phi = (1.0 + 5.0_f64.sqrt()) / 2.0;
            let mut corners = Vec::new();
            for a in [-1.0, 1.0] {
                for b in [-phi, phi] {
                    corners.extend([
                        Point3::new(0.0, a, b),
                        Point3::new(a, b, 0.0),
                        Point3::new(b, 0.0, a),
                    ]);
                }
            }
            let near = |p: Point3, q: Point3| (p.distance(q) - 2.0).abs() < 1e-9;
            let mut ways: Vec<Point3> = corners.clone();
            for i in 0..12 {
                for j in i + 1..12 {
                    if !near(corners[i], corners[j]) {
                        continue;
                    }
                    ways.push(corners[i] + corners[j]);
                    for k in j + 1..12 {
                        if near(corners[i], corners[k]) && near(corners[j], corners[k]) {
                            ways.push(corners[i] + corners[j] + corners[k]);
                        }
                    }
                }
            }
            let mut body = ball(s, [0.0; 3], 10.0, ID)?;
            for way in ways {
                let dimple = ball(s, (way.normalize() * 10.6).into(), 1.2, ID)?;
                body = apply(Op::Cut, &body, &dimple)?;
            }
            Ok(body)
        }
        "boolean/half-cylinder-boss-tangent" => {
            let a = block(s, [0.0; 3], [10.0; 3])?;
            let b = rod(s, 2, [10.0, 5.0], 0.0, 10.0, 5.0, 0.0)?;
            apply(Op::Union, &a, &b)
        }
        "boolean/half-torus" => {
            let a = torus(s, 10.0, 2.0, 0.0)?;
            let b = block(s, [-far, -far, -far], [0.0, far, far])?;
            apply(Op::Cut, &a, &b)
        }
        "boolean/hemisphere" => {
            let a = ball(s, [0.0; 3], 5.0, ID)?;
            let b = block(s, [-far, -far, -far], [far, far, 0.0])?;
            apply(Op::Cut, &a, &b)
        }
        "boolean/indexed-pattern" => {
            let mut disk = rod(s, 2, [0.0, 0.0], -5.0, 0.0, 20.0, 0.0)?;
            for k in 0..6 {
                let (sin, cos) = libm::sincos((f64::from(k) * 60.0_f64).to_radians());
                let drill = rod(s, 2, [12.0 * cos, 12.0 * sin], -6.0, 1.0, 2.0, 0.0)?;
                disk = apply(Op::Cut, &disk, &drill)?;
            }
            Ok(disk)
        }
        "boolean/napkin-ring" => {
            let a = ball(s, [0.0; 3], 5.0, ID)?;
            let b = rod(s, 2, [0.0, 0.0], -6.0, 6.0, 3.0, 0.0)?;
            apply(Op::Cut, &a, &b)
        }
        "boolean/offset-hole-sphere" => {
            let a = ball(s, [0.0; 3], 20.0, ID)?;
            let b = rod(s, 1, [5.0, 0.0], -25.0, 25.0, 6.0, 0.0)?;
            apply(Op::Cut, &a, &b)
        }
        "boolean/sphere-lens" | "boolean/spheres-union" => {
            let a = ball(s, [0.0; 3], 5.0, ID)?;
            let b = ball(s, [6.0, 0.0, 0.0], 5.0, ID)?;
            apply(
                if id.ends_with("lens") {
                    Op::Intersect
                } else {
                    Op::Union
                },
                &a,
                &b,
            )
        }
        "boolean/sphere-pocket" => {
            let a = block(s, [-10.0, -10.0, -10.0], [10.0, 10.0, 0.0])?;
            let b = ball(s, [0.0; 3], 5.0, ID)?;
            apply(Op::Cut, &a, &b)
        }
        "boolean/steinmetz-common" | "boolean/steinmetz-union" => {
            let a = rod(s, 0, [0.0, 0.0], -15.0, 15.0, 5.0, 0.0)?;
            let b = rod(s, 1, [0.0, 0.0], -15.0, 15.0, 5.0, 0.0)?;
            apply(
                if id.ends_with("common") {
                    Op::Intersect
                } else {
                    Op::Union
                },
                &a,
                &b,
            )
        }
        "boolean/stepped-shaft" => {
            let a = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?;
            let b = rod(s, 2, [0.0, 0.0], 10.0, 20.0, 3.0, 0.0)?;
            apply(Op::Union, &a, &b)
        }
        "boolean/tube-flush-ends" | "boolean/tube" => {
            let a = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?;
            let (from, to) = if id.ends_with("ends") {
                (0.0, 10.0)
            } else {
                (-1.0, 11.0)
            };
            let b = rod(s, 2, [0.0, 0.0], from, to, 3.0, 0.0)?;
            apply(Op::Cut, &a, &b)
        }
        "tangent/ball-in-band" => {
            let a = ball(s, [0.0; 3], 5.0, ID)?;
            let b = rod(s, 2, [0.0, 0.0], -3.0, 3.0, 5.0, 0.0)?;
            apply(Op::Union, &a, &b)
        }
        "tangent/coaxial-overlap" => {
            let a = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?;
            let b = rod(s, 2, [0.0, 0.0], 6.0, 16.0, 5.0, FRAC_PI_2)?;
            apply(Op::Union, &a, &b)
        }
        "tangent/cone-tip-on-face" => {
            let a = block(s, [-10.0, -10.0, 0.0], [10.0, 10.0, 10.0])?;
            let b = cone(s, 0.0, 15.0, 5.0)?;
            apply(Op::Union, &a, &b)
        }
        "tangent/cube-turned-a-hair" => {
            let a = block(s, [-5.0, -5.0, 0.0], [5.0, 5.0, 10.0])?;
            let b = setup(s.cube(
                [0.0, 0.0, 5.0],
                [10.0; 3],
                rotation_z(0.001_f64.to_radians()),
            ))?;
            apply(Op::Union, &a, &b)
        }
        "tangent/cylinder-inscribed-through-a-cube" => {
            let a = block(s, [-5.0; 3], [5.0; 3])?;
            let b = rod(s, 0, [0.0, 0.0], -10.0, 10.0, 5.0, 0.3)?;
            apply(Op::Union, &a, &b)
        }
        "tangent/hairline-slot" => {
            let a = block(s, [0.0; 3], [10.0; 3])?;
            let b = block(s, [5.0, -1.0, 5.0], [5.000_001, 11.0, 11.0])?;
            apply(Op::Cut, &a, &b)
        }
        "tangent/half-balls-overlapping" => {
            let first = ball(s, [0.0; 3], 5.0, ID)?;
            let below = block(s, [-far, -far, -far], [far, far, 0.0])?;
            let first = apply(Op::Cut, &first, &below)?;
            let second = ball(s, [0.0; 3], 5.0, rotation_y(FRAC_PI_2))?;
            let behind = block(s, [-far, -far, -far], [0.0, far, far])?;
            let second = apply(Op::Cut, &second, &behind)?;
            apply(Op::Union, &first, &second)
        }
        "tangent/half-tori-overlapping" => {
            let first = torus(s, 10.0, 3.0, 0.0)?;
            let below = block(s, [-far, -far, -far], [far, far, 0.0])?;
            let first = apply(Op::Cut, &first, &below)?;
            let second = torus(s, 10.0, 3.0, FRAC_PI_2)?;
            let behind = block(s, [-far, -far, -far], [0.0, far, far])?;
            let second = apply(Op::Cut, &second, &behind)?;
            apply(Op::Union, &first, &second)
        }
        "tangent/ice-cream-cone" => {
            let a = ball(s, [0.0; 3], 15.0, ID)?;
            let b = cone(s, -25.0, -9.0, 12.0)?;
            apply(Op::Union, &a, &b)
        }
        "tangent/inscribed-ball-common" | "tangent/inscribed-ball-union" => {
            let a = block(s, [0.0; 3], [10.0; 3])?;
            let b = ball(s, [5.0; 3], 5.0, ID)?;
            apply(
                if id.ends_with("common") {
                    Op::Intersect
                } else {
                    Op::Union
                },
                &a,
                &b,
            )
        }
        "tangent/knife-edge-dish" => {
            let a = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?;
            let b = ball(s, [0.0, 0.0, 10.0], 5.0, ID)?;
            apply(Op::Cut, &a, &b)
        }
        "tangent/midsphere-common" => {
            let a = block(s, [-5.0; 3], [5.0; 3])?;
            let b = ball(s, [0.0; 3], 5.0 * 2.0_f64.sqrt(), ID)?;
            apply(Op::Intersect, &a, &b)
        }
        "tangent/pole-cap-a-ten-thousandth-high" => {
            let a = ball(s, [0.0; 3], 5.0, ID)?;
            let b = block(s, [-far, -far, 4.9999], [far, far, far])?;
            apply(Op::Intersect, &a, &b)
        }
        "tangent/tangent-tee-common" | "tangent/tangent-tee-union" => {
            let a = rod(s, 0, [0.0, 0.0], -15.0, 15.0, 5.0, 0.0)?;
            let b = rod(s, 1, [0.0, 2.0], -15.0, 15.0, 3.0, 0.0)?;
            apply(
                if id.ends_with("common") {
                    Op::Intersect
                } else {
                    Op::Union
                },
                &a,
                &b,
            )
        }
        "tangent/thin-lens" => {
            let a = ball(s, [0.0; 3], 5.0, ID)?;
            let b = ball(s, [9.999_999, 0.0, 0.0], 5.0, ID)?;
            apply(Op::Intersect, &a, &b)
        }
        "tangent/torus-in-drum" => {
            let a = torus(s, 10.0, 3.0, 0.0)?;
            let b = rod(s, 2, [0.0, 0.0], -1.0, 1.0, 13.0, 0.0)?;
            apply(Op::Union, &a, &b)
        }
        "tangent/torus-through-a-square-slab" => {
            let a = torus(s, 10.0, 3.0, 45.0_f64.to_radians())?;
            let b = block(s, [-13.0, -13.0, -1.0], [13.0, 13.0, 1.0])?;
            apply(Op::Union, &a, &b)
        }
        "tangent/villarceau-half-torus" => {
            let a = torus(s, 5.0, 3.0, 0.0)?;
            // The half space above 3x − 4z = 0: a block whose bottom
            // is that plane.
            let tilt = rotation_y(-libm::atan(0.75_f64));
            let up = tilt * Point3::Z;
            let b = setup(s.cube((up * 50.0).into(), [200.0, 200.0, 100.0], tilt))?;
            apply(Op::Cut, &a, &b)
        }
        "tangent/viviani-common" | "tangent/viviani-cut" | "tangent/viviani-union" => {
            let a = ball(s, [0.0; 3], 10.0, ID)?;
            let b = rod(s, 2, [5.0, 0.0], -12.0, 12.0, 5.0, 0.0)?;
            let op = if id.ends_with("common") {
                Op::Intersect
            } else if id.ends_with("cut") {
                Op::Cut
            } else {
                Op::Union
            };
            apply(op, &a, &b)
        }
        "tangent/wedge-a-millionth-steep" => {
            let a = block(s, [-5.0, -5.0, -10.0], [5.0, 5.0, 0.0])?;
            // The half space above z = y·10⁻⁶.
            let tilt = rotation_x(libm::atan(1e-6_f64));
            let up = tilt * Point3::Z;
            let b = setup(s.cube((up * 50.0).into(), [200.0, 200.0, 100.0], tilt))?;
            apply(Op::Intersect, &a, &b)
        }
        "tangent/paraboloid-tip-on-face" => {
            let a = block(s, [-10.0, -10.0, 0.0], [10.0, 10.0, 10.0])?;
            let b = paraboloid(s, 8.0, 12.5)?;
            apply(Op::Union, &a, &b)
        }
        "tangent/ball-in-a-paraboloid-cup" => {
            let a = paraboloid(s, 10.0, 5.0)?;
            let b = ball(s, [0.0, 0.0, 5.0], 5.0, ID)?;
            apply(Op::Union, &a, &b)
        }
        "tangent/paraboloid-tip-a-ten-thousandth-high" => {
            let a = paraboloid(s, 8.0, 8.0)?;
            let b = block(s, [-far, -far, -far], [far, far, 1e-4])?;
            apply(Op::Intersect, &a, &b)
        }
        "tangent/grazing-cylinders" => {
            let a = rod(s, 0, [0.0, 0.0], -15.0, 15.0, 5.0, 0.0)?;
            let b = rod(s, 1, [0.0, 9.99], -15.0, 15.0, 5.0, 0.0)?;
            apply(Op::Intersect, &a, &b)
        }
        "fillet/box-one-edge" | "fillet/variable-radius-edge" => {
            let cube = block(s, [0.0; 3], [10.0; 3])?;
            let edges = pick(&cube, |_, m, _| near(m.x, 10.0) && near(m.y, 10.0));
            // The radius grows with z, whichever way the edge runs.
            let up = cube.edges[edges[0]].point(cube.edges[edges[0]].range.0).z < 5.0;
            let shape = match (id, up) {
                ("fillet/box-one-edge", _) => round(2.0),
                (_, true) => growing(1.0, 2.0),
                (_, false) => growing(2.0, 1.0),
            };
            blended(&cube, &edges, &shape, true)
        }
        "fillet/box-all-edges" => {
            let cube = block(s, [0.0; 3], [10.0; 3])?;
            blended(&cube, &pick(&cube, |_, _, _| true), &round(1.0), true)
        }
        "fillet/cylinder-rim" | "fillet/cylinder-rim-to-dome" | "chamfer/cylinder-rim-to-point" => {
            let post = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?;
            let rim = pick(&post, |_, m, _| {
                near(m.z, 10.0) && near(libm::hypot(m.x, m.y), 5.0)
            });
            let shape = match id {
                "fillet/cylinder-rim" => round(1.0),
                "fillet/cylinder-rim-to-dome" => round(5.0),
                _ => flat(5.0, 5.0),
            };
            blended(&post, &rim, &shape, true)
        }
        "fillet/ball-and-post" => {
            let ball = ball(s, [0.0; 3], 10.0, ID)?;
            let post = rod(s, 2, [0.0, 0.0], 0.0, 15.0, 4.0, 0.0)?;
            let both = apply(Op::Union, &ball, &post)?;
            let root = pick(&both, |_, m, _| near(m.z, 84f64.sqrt()));
            blended(&both, &root, &round(2.0), true)
        }
        "fillet/boss-s-curve" => {
            let plate = block(s, [-20.0, -20.0, 0.0], [20.0, 20.0, 4.0])?;
            let boss = rod(s, 2, [0.0, 0.0], 4.0, 8.0, 5.0, 0.0)?;
            let both = apply(Op::Union, &plate, &boss)?;
            let rings = pick(&both, |_, m, _| {
                (near(m.z, 8.0) || near(m.z, 4.0)) && near(libm::hypot(m.x, m.y), 5.0)
            });
            blended(&both, &rings, &round(2.0), true)
        }
        "fillet/hole-waist" => {
            let plate = block(s, [-10.0, -10.0, 0.0], [10.0, 10.0, 2.0])?;
            let hole = rod(s, 2, [0.0, 0.0], -1.0, 3.0, 3.0, 0.0)?;
            let holed = apply(Op::Cut, &plate, &hole)?;
            let rims = pick(&holed, |_, m, _| {
                near(libm::hypot(m.x, m.y), 3.0) && (near(m.z, 0.0) || near(m.z, 2.0))
            });
            blended(&holed, &rims, &round(1.0), true)
        }
        "fillet/l-block-inside-consumed" | "chamfer/l-block-inside-consumed" => {
            let foot = block(s, [0.0, 0.0, 0.0], [10.0, 10.0, 3.0])?;
            let leg = block(s, [0.0, 0.0, 3.0], [3.0, 10.0, 10.0])?;
            let l = apply(Op::Union, &foot, &leg)?;
            let inside = pick(&l, |_, m, _| near(m.x, 3.0) && near(m.z, 3.0));
            let shape = if id.starts_with("fillet") {
                round(7.0)
            } else {
                flat(7.0, 7.0)
            };
            blended(&l, &inside, &shape, true)
        }
        "fillet/rounded-block-top-rim" => {
            let block = block(s, [-10.0, -6.0, 0.0], [10.0, 6.0, 5.0])?;
            let upright = pick(&block, |a, _, b| !near(a.z, b.z));
            let rounded = blended(&block, &upright, &round(3.0), true)?;
            let rim = pick(&rounded, |a, m, b| {
                near(a.z, 5.0) && near(m.z, 5.0) && near(b.z, 5.0)
            });
            blended(&rounded, &rim, &round(1.0), true)
        }
        "fillet/slab-half-round" | "chamfer/block-ridge" => {
            let slab = block(s, [0.0; 3], [10.0, 4.0, 6.0])?;
            let along = pick(&slab, |_, m, _| {
                near(m.z, 6.0) && (near(m.y, 0.0) || near(m.y, 4.0))
            });
            let shape = if id.starts_with("fillet") {
                round(2.0)
            } else {
                flat(2.0, 2.0)
            };
            blended(&slab, &along, &shape, true)
        }
        "fillet/sliver-strip" => {
            let plate = block(s, [0.0; 3], [20.0, 10.0, 2.0])?;
            let along = pick(&plate, |_, m, _| {
                near(m.y, 10.0) && (near(m.z, 0.0) || near(m.z, 2.0))
            });
            blended(&plate, &along, &round(0.9999), true)
        }
        "chamfer/box-top-asymmetric" => {
            let mut solid = block(s, [0.0; 3], [20.0, 10.0, 6.0])?;
            // 1 across the top and 2 down the sides: the edges whose
            // first face is the top, then the others.
            for top_first in [true, false] {
                let edges: Vec<usize> = pick(&solid, |_, m, _| {
                    near(m.z, 6.0)
                        && (near(m.x, 0.0) || near(m.x, 20.0) || near(m.y, 0.0) || near(m.y, 10.0))
                })
                .into_iter()
                .filter(|&e| is_top(&solid, first_face(&solid, e)) == top_first)
                .collect();
                if edges.is_empty() {
                    continue;
                }
                let shape = if top_first {
                    flat(1.0, 2.0)
                } else {
                    flat(2.0, 1.0)
                };
                solid = blended(&solid, &edges, &shape, false)?;
            }
            Ok(solid)
        }
        "chamfer/cube-all-edges" | "chamfer/cube-all-edges-mitered" => {
            let cube = block(s, [0.0; 3], [10.0; 3])?;
            let all = pick(&cube, |_, _, _| true);
            blended(&cube, &all, &flat(2.0, 2.0), id == "chamfer/cube-all-edges")
        }
        "chamfer/octahedron-all-edges" | "chamfer/octahedron-all-edges-mitered" => {
            // |x| + |y| + |z| ≤ 10: what four slabs across the cube's
            // diagonals share.
            let mut solid: Option<Solid> = None;
            for signs in [
                [1.0, 1.0, 1.0],
                [1.0, 1.0, -1.0],
                [1.0, -1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ] {
                let across = Point3::from_array(signs).normalize();
                let turn = DQuat::from_rotation_arc(Point3::Z, across);
                let slab = setup(s.cube([0.0; 3], [60.0, 60.0, 20.0 / 3f64.sqrt()], turn))?;
                solid = Some(match solid {
                    Some(so_far) => apply(Op::Intersect, &so_far, &slab)?,
                    None => slab,
                });
            }
            let solid = solid.expect("four slabs");
            let all = pick(&solid, |_, _, _| true);
            let d = 6f64.sqrt();
            blended(
                &solid,
                &all,
                &flat(d, d),
                id == "chamfer/octahedron-all-edges",
            )
        }
        other if other.starts_with("offset/") || other.starts_with("shell/") => thick(s, other),
        other if other.starts_with("thicken/") => sheets(s, other),
        other if other.starts_with("draft/") => drafts(s, other),
        other if other.starts_with("sweep/") => sweeps(s, other),
        other if other.starts_with("loft/") => lofts(other),
        other if other.starts_with("singular/") => singular(s, other),
        other if other.starts_with("edit/") => edits(s, other),
        other if other.starts_with("feature/") => features(s, other),
        other if other.starts_with("swept/") => swept_bodies(s, other),
        other if other.starts_with("geometry/") => geometry(s, other),
        other if other.starts_with("parts/") => parts(s, other),
        other if other.starts_with("nurbs/") => nurbs(s, other),
        other if other.starts_with("spline/") => spline(s, other),
        "primitives/box" => block(s, [0.0; 3], [10.0, 20.0, 30.0]),
        "primitives/cone-frustum" => setup(s.turned(
            [0.0; 3],
            upright(),
            &[[0.0, 0.0], [5.0, 0.0], [2.0, 6.0], [0.0, 6.0]],
            None,
            None,
        )),
        "primitives/cylinder" => rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0),
        "primitives/extruded-l" => l_plate(s),
        "primitives/revolved-washer" | "primitives/revolved-sector" => {
            let washer = setup(s.turned(
                [0.0; 3],
                upright(),
                &[[6.0, 0.0], [10.0, 0.0], [10.0, 4.0], [6.0, 4.0]],
                None,
                None,
            ))?;
            if id.ends_with("washer") {
                return Ok(washer);
            }
            let quarter = block(s, [0.0, 0.0, -1.0], [11.0, 11.0, 5.0])?;
            apply(Op::Intersect, &washer, &quarter)
        }
        "primitives/sphere" => ball(s, [0.0; 3], 5.0, ID),
        "primitives/torus" => torus(s, 10.0, 2.0, 0.0),
        other => Err(Grade::Refused(format!(
            "{other} needs an operation this kernel does not have yet (only booleans of boxes, \
             cylinders, balls and solids of revolution are built)"
        ))),
    }
}

/// The faces of `solid` that are where `at` says: all along their
/// edges, and in their middle.
fn faces_at(solid: &Solid, at: impl Fn(Point3) -> bool) -> Vec<usize> {
    (0..solid.faces.len())
        .filter(|&f| {
            let face = &solid.faces[f];
            let mut seen = Vec::new();
            for step in face.loops.iter().flatten() {
                let (a, b) = solid.span(step);
                for k in [0.0, 0.3, 0.7, 1.0] {
                    seen.push(step.pcurve.point_at(a + (b - a) * k));
                }
            }
            let Some(&first) = seen.first() else {
                return false;
            };
            let middle = seen.iter().fold(first * 0.0, |sum, uv| sum + *uv) / seen.len() as f64;
            seen.push(middle);
            seen.iter().all(|uv| at(face.surface.point_at(uv.x, uv.y)))
        })
        .collect()
}

/// The L-shaped plate of the offset and shell cases, 8 high.
fn l_plate(s: &mut Scene) -> Built {
    let long = block(s, [0.0, 0.0, 0.0], [20.0, 5.0, 8.0])?;
    let short = block(s, [0.0, 5.0, 0.0], [5.0, 15.0, 8.0])?;
    apply(Op::Union, &long, &short)
}

/// brep-bench's offset and shell cases.
fn thick(s: &mut Scene, id: &str) -> Built {
    let done = |made: anyhow::Result<Solid>| made.map_err(|why| Grade::Refused(format!("{why:#}")));
    let grown = |solid: &Solid, d: f64| done(thickened(solid, d, None, false));
    // A shell inward, open at the faces where `open` says.
    let shell = |solid: &Solid, t: f64, open: &dyn Fn(Point3) -> bool| {
        let faces = faces_at(solid, open);
        done(thickened(solid, -t, Some(&faces), false))
    };
    let nowhere = |_: Point3| false;
    let level = |z: f64| move |p: Point3| near(p.z, z);
    let box_ = |s: &mut Scene| block(s, [0.0; 3], [10.0, 8.0, 6.0]);
    let drilled = |s: &mut Scene, x: f64| {
        let block = block(s, [0.0; 3], [20.0, 20.0, 10.0])?;
        let bore = rod(s, 2, [x, 10.0], -1.0, 11.0, 3.0, 0.0)?;
        apply(Op::Cut, &block, &bore)
    };
    let rounded = |s: &mut Scene| {
        let plate = block(s, [0.0; 3], [20.0, 14.0, 10.0])?;
        let upright = pick(&plate, |a, _, b| near(a.x, b.x) && near(a.y, b.y));
        blended(&plate, &upright, &round(2.0), false)
    };
    let capsule = |s: &mut Scene| {
        let side = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?;
        let dome = ball(s, [0.0, 0.0, 10.0], 5.0, ID)?;
        apply(Op::Union, &side, &dome)
    };
    let capped = |s: &mut Scene, z: f64| {
        let whole = ball(s, [0.0; 3], 5.0, ID)?;
        let above = block(s, [-6.0, -6.0, z], [6.0, 6.0, 7.0])?;
        apply(Op::Cut, &whole, &above)
    };
    match id {
        "offset/box-grown" => grown(&box_(s)?, 1.0),
        "offset/tetrahedron-grown" => {
            // A block, cut back to the plane of each of the four faces.
            let mut solid = block(s, [-7.0; 3], [7.0; 3])?;
            for corner in [
                [3.0, 3.0, 3.0],
                [3.0, -3.0, -3.0],
                [-3.0, 3.0, -3.0],
                [-3.0, -3.0, 3.0],
            ] {
                let corner = Point3::from_array(corner);
                let [_, behind] = workpiece_shape::halves(&solid, -corner / 3.0, -corner)
                    .map_err(|why| Grade::Crashed(format!("setup: {why:#}")))?;
                solid = behind;
            }
            grown(&solid, 1.0)
        }
        "offset/box-shrunk" => grown(&box_(s)?, -1.0),
        "offset/cone-grown" => grown(&cone(s, 4.0, 0.0, 3.0)?, 1.0),
        "offset/cylinder-grown" => grown(&rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?, 1.0),
        "offset/drilled-plate-grown" => {
            let plate = block(s, [-10.0, -10.0, 0.0], [10.0, 10.0, 4.0])?;
            let hole = rod(s, 2, [0.0, 0.0], -1.0, 5.0, 3.0, 0.0)?;
            grown(&apply(Op::Cut, &plate, &hole)?, 1.0)
        }
        "offset/flanged-drum-shrunk" => {
            let drum = rod(s, 2, [0.0, 0.0], 0.0, 6.0, 8.0, 0.0)?;
            let flange = rod(s, 2, [0.0, 0.0], 2.2, 3.8, 11.0, 0.0)?;
            grown(&apply(Op::Union, &drum, &flange)?, -1.0)
        }
        "offset/grooved-drum-grown" => {
            let drum = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 10.0, 0.0)?;
            let wide = rod(s, 2, [0.0, 0.0], 4.2, 5.8, 12.0, 0.0)?;
            let core = rod(s, 2, [0.0, 0.0], 3.0, 7.0, 7.0, 0.0)?;
            let groove = apply(Op::Cut, &wide, &core)?;
            grown(&apply(Op::Cut, &drum, &groove)?, 1.0)
        }
        "offset/l-block-grown" => grown(&l_plate(s)?, 1.0),
        "offset/l-block-shrunk" => grown(&l_plate(s)?, -1.0),
        "offset/napkin-ring-grown" => {
            let whole = ball(s, [0.0; 3], 5.0, ID)?;
            let bore = rod(s, 2, [0.0, 0.0], -6.0, 6.0, 3.0, 0.0)?;
            grown(&apply(Op::Cut, &whole, &bore)?, 1.0)
        }
        "offset/torus-grown-past-its-hole" => grown(&torus(s, 4.0, 2.0, 0.0)?, 3.0),
        "offset/torus-shrunk" => grown(&torus(s, 10.0, 3.0, 0.0)?, -1.5),
        "shell/box-five-faces-open" => {
            let solid = box_(s)?;
            let bottom = faces_at(&solid, level(0.0));
            let open: Vec<usize> = (0..solid.faces.len())
                .filter(|f| !bottom.contains(f))
                .collect();
            done(thickened(&solid, -1.0, Some(&open), false))
        }
        "shell/box-hairline-cavity" => shell(&box_(s)?, 3.99, &level(6.0)),
        "shell/box-hollowed" => shell(&box_(s)?, 1.0, &nowhere),
        "shell/box-open-top-and-bottom" => {
            shell(&box_(s)?, 1.0, &|p| near(p.z, 6.0) || near(p.z, 0.0))
        }
        "shell/box-open-top-and-front" => {
            let solid = box_(s)?;
            let mut open = faces_at(&solid, level(6.0));
            open.extend(faces_at(&solid, |p| near(p.y, 0.0)));
            done(thickened(&solid, -1.0, Some(&open), false))
        }
        "shell/box-open-top" => shell(&box_(s)?, 1.0, &level(6.0)),
        "shell/box-paper-thin" => shell(&box_(s)?, 0.001, &level(6.0)),
        "shell/box-walls-meeting" => shell(&box_(s)?, 4.0, &level(6.0)),
        "shell/capsule-open-at-its-dome" => shell(&capsule(s)?, 1.0, &|p| p.z > 10.0 - 1e-6),
        "shell/capsule-open-base" => shell(&capsule(s)?, 1.0, &level(0.0)),
        "shell/cone-open-base" => shell(&cone(s, 8.0, 0.0, 6.0)?, 1.0, &level(0.0)),
        "shell/cylinder-open-top" => shell(
            &rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?,
            1.0,
            &level(10.0),
        ),
        "shell/cylinder-side-open" => {
            let solid = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?;
            let ends: Vec<usize> = [0.0, 10.0]
                .into_iter()
                .flat_map(|z| faces_at(&solid, level(z)))
                .collect();
            let open: Vec<usize> = (0..solid.faces.len())
                .filter(|f| !ends.contains(f))
                .collect();
            done(thickened(&solid, -1.0, Some(&open), false))
        }
        "shell/drilled-block-open-at-its-bore" => shell(&drilled(s, 10.0)?, 1.0, &|p| {
            near(libm::hypot(p.x - 10.0, p.y - 10.0), 3.0)
        }),
        "shell/drilled-block-open-top" => shell(&drilled(s, 10.0)?, 1.0, &level(10.0)),
        "shell/hole-skin-merging-wall" => shell(&drilled(s, 15.5)?, 1.0, &level(10.0)),
        "shell/dumbbell-neck-filled" => {
            let left = block(s, [0.0; 3], [10.0, 10.0, 6.0])?;
            let neck = block(s, [10.0, 4.0, 0.0], [14.0, 6.0, 6.0])?;
            let right = block(s, [14.0, 0.0, 0.0], [24.0, 10.0, 6.0])?;
            let both = apply(Op::Union, &left, &neck)?;
            shell(&apply(Op::Union, &both, &right)?, 1.5, &level(6.0))
        }
        "shell/half-torus-open-ends" => {
            let whole = torus(s, 6.0, 2.0, 0.0)?;
            let front = block(s, [-9.0, -9.0, -3.0], [9.0, 0.0, 3.0])?;
            shell(&apply(Op::Cut, &whole, &front)?, 0.5, &|p| near(p.y, 0.0))
        }
        "shell/hemisphere-open" => {
            let whole = ball(s, [0.0; 3], 5.0, ID)?;
            let above = block(s, [-6.0, -6.0, 0.0], [6.0, 6.0, 6.0])?;
            shell(&apply(Op::Cut, &whole, &above)?, 1.0, &level(0.0))
        }
        "shell/l-block-open-top" => shell(&l_plate(s)?, 1.0, &level(8.0)),
        "shell/round-consumed" => shell(&rounded(s)?, 3.0, &level(10.0)),
        "shell/rounded-tray" => shell(&rounded(s)?, 1.0, &level(10.0)),
        "shell/round-shrunk-to-an-edge" => shell(&rounded(s)?, 2.0, &level(10.0)),
        "shell/sphere-hollowed" => shell(&ball(s, [0.0; 3], 5.0, ID)?, 1.0, &nowhere),
        "shell/sphere-open-at-a-cap" => shell(&capped(s, 3.0)?, 1.0, &level(3.0)),
        "shell/sphere-opening-sealed" => shell(&capped(s, 4.5)?, 1.0, &level(4.5)),
        "shell/sphere-pinhole-opening" => shell(&capped(s, 3.9999)?, 1.0, &level(3.9999)),
        "shell/torus-hollowed" => shell(&torus(s, 6.0, 2.0, 0.0)?, 0.5, &nowhere),
        "offset/l-block-grown-sharp" => done(thickened(&l_plate(s)?, 1.0, None, true)),
        "shell/l-block-open-top-sharp" => {
            let solid = l_plate(s)?;
            let top = faces_at(&solid, level(8.0));
            done(thickened(&solid, -1.0, Some(&top), true))
        }
        "shell/box-open-top-outward" | "shell/box-open-top-outward-sharp" => {
            let solid = box_(s)?;
            let top = faces_at(&solid, level(6.0));
            done(thickened(&solid, 1.0, Some(&top), id.ends_with("sharp")))
        }
        other => Err(Grade::Refused(format!(
            "{other}: this shape is not made yet"
        ))),
    }
}

/// brep-bench's cases of bodies carried through bodies: those whose
/// sweep is a tube, a prism or a ring.
fn swept_bodies(s: &mut Scene, id: &str) -> Built {
    use workpiece_core::math::Point2;
    use workpiece_core::world::topo::Curve3;
    use workpiece_shape::swept::{Outline, along_path, circle, steady, swept};
    let done = |made: anyhow::Result<Solid>| made.map_err(|why| Grade::Refused(format!("{why:#}")));
    let curve = |made: Result<Curve3, _>| made.map_err(|_| Grade::Crashed("setup: a path".into()));
    // The tube a ball of `radius` leaves along `path`.
    let tube = |path: &Curve3, radius: f64, size: f64| {
        let (sides, points) = circle(Point2::ZERO, radius);
        let at = steady(&points);
        done(swept(
            "tube",
            &Outline { sides, at: &at },
            path,
            path.domain(),
            Point3::Z,
            size,
        ))
    };
    let skew = rotation(Point3::new(1.0, 2.0, 3.0).normalize(), 1.0);
    // A block less the tube along `path` and the ball at each of its ends.
    let grooved = |s: &mut Scene, lo: [f64; 3], hi: [f64; 3], path: &Curve3, radius: f64| {
        let (a, b) = path.domain();
        let mut left = block(s, lo, hi)?;
        left = apply(Op::Cut, &left, &tube(path, radius, 80.0)?)?;
        for t in [a, b] {
            left = apply(
                Op::Cut,
                &left,
                &ball(s, path.point_at(t).to_array(), radius, skew)?,
            )?;
        }
        Ok(left)
    };
    // What a torus about a line along y through (`x`, `y`), of these
    // radii, passes through as it slides along z: all within the
    // tube's radius of a line.
    let slid = |y: f64, major: f64, minor: f64| {
        let (x, w) = (11.5, std::f64::consts::FRAC_1_SQRT_2);
        let (lo, hi) = (x - major, x + major);
        let line = |a: (f64, f64), b: (f64, f64)| {
            (
                (1, vec![0.0, 0.0, 1.0, 1.0], 2),
                vec![(Point2::new(a.0, a.1), 1.0), (Point2::new(b.0, b.1), 1.0)],
            )
        };
        let arc = |c: f64, from: (f64, f64), to: (f64, f64)| {
            let p = |d: (f64, f64)| Point2::new(c + d.0 * minor, y + d.1 * minor);
            (
                (2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 3),
                vec![
                    (p(from), 1.0),
                    (p((from.0 + to.0, from.1 + to.1)), w),
                    (p(to), 1.0),
                ],
            )
        };
        let pieces = [
            line((lo, y - minor), (hi, y - minor)),
            arc(hi, (0.0, -1.0), (1.0, 0.0)),
            arc(hi, (1.0, 0.0), (0.0, 1.0)),
            line((hi, y + minor), (lo, y + minor)),
            arc(lo, (0.0, 1.0), (-1.0, 0.0)),
            arc(lo, (-1.0, 0.0), (0.0, -1.0)),
        ];
        let sides = pieces.iter().map(|p| p.0.clone()).collect();
        let points: Vec<(Point2, f64)> = pieces.iter().flat_map(|p| p.1.clone()).collect();
        let at = steady(&points);
        let path = curve(Curve3::line(
            Point3::new(0.0, 0.0, -5.0),
            Point3::new(0.0, 0.0, 25.0),
        ))?;
        done(swept(
            &format!("slid{minor}"),
            &Outline { sides, at: &at },
            &path,
            (0.0, 1.0),
            Point3::Y,
            60.0,
        ))
    };
    match id {
        "swept/ball-along-spline-groove" => {
            let path = curve(Curve3::new(
                3,
                vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                vec![
                    Point3::ZERO,
                    Point3::new(10.0, 0.0, 0.0),
                    Point3::new(20.0, 20.0, 0.0),
                    Point3::new(30.0, 20.0, 0.0),
                ],
                None,
            ))?;
            grooved(s, [-6.0, -6.0, -8.0], [36.0, 26.0, 0.0], &path, 3.0)
        }
        "swept/ball-swung-groove" => {
            let path = curve(Curve3::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![
                    Point3::new(12.0, 0.0, 0.0),
                    Point3::new(12.0, 12.0, 0.0),
                    Point3::new(0.0, 12.0, 0.0),
                ],
                Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            ))?;
            grooved(s, [-6.0, -6.0, -8.0], [18.0, 18.0, 0.0], &path, 3.0)
        }
        "swept/ball-thread-groove" => {
            let (radius, rise, turns, pieces) = (5.0_f64, 4.0_f64, 6.0_f64, 48_usize * 4);
            let step = turns * std::f64::consts::TAU / pieces as f64;
            let at = |a: f64| {
                Point3::new(
                    radius * libm::cos(a),
                    radius * libm::sin(a),
                    rise * a / std::f64::consts::TAU - 2.0,
                )
            };
            let way = |a: f64| {
                Point3::new(
                    -radius * libm::sin(a),
                    radius * libm::cos(a),
                    rise / std::f64::consts::TAU,
                )
            };
            let handle = 4.0 / 3.0 * libm::tan(step / 4.0);
            let mut points = vec![at(0.0)];
            let mut knots = vec![0.0; 4];
            for k in 0..pieces {
                let (a, b) = (step * k as f64, step * (k + 1) as f64);
                points.extend([at(a) + way(a) * handle, at(b) - way(b) * handle, at(b)]);
                knots.extend([(k + 1) as f64; 3]);
            }
            knots.push(pieces as f64);
            let path = curve(Curve3::new(3, knots, points, None))?;
            let rod = rod(s, 2, [0.0, 0.0], 0.0, 20.0, 5.0, 0.0)?;
            apply(Op::Cut, &rod, &tube(&path, 1.0, 40.0)?)
        }
        "swept/box-slid-aslant" => {
            // The box, and what each of the three faces that lead
            // passes through.
            let slide = Point3::new(8.0, 5.0, 3.0);
            let path = [curve(Curve3::line(Point3::ZERO, slide))?];
            let mut all = block(s, [0.0; 3], [10.0, 6.0, 4.0])?;
            let size = Point3::new(10.0, 6.0, 4.0);
            for axis in 0..3 {
                let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
                let corner = |i: f64, j: f64| {
                    let mut p = Point3::ZERO;
                    p[axis] = size[axis];
                    p[a] = size[a] * i;
                    p[b] = size[b] * j;
                    p
                };
                let corners = [
                    corner(0.0, 0.0),
                    corner(1.0, 0.0),
                    corner(1.0, 1.0),
                    corner(0.0, 1.0),
                ];
                let mut outline = Vec::new();
                for k in 0..4 {
                    outline.push(curve(Curve3::line(corners[k], corners[(k + 1) % 4]))?);
                }
                let up = if axis == 2 { Point3::X } else { Point3::Z };
                let prism = done(along_path(
                    &format!("led{axis}"),
                    &outline,
                    &path,
                    false,
                    up,
                    0.0,
                    1.0,
                ))?;
                all = apply(Op::Union, &all, &prism)?;
            }
            Ok(all)
        }
        "swept/torus-slid-through-rod" => {
            let rod = rod(s, 2, [0.0, 0.0], 0.0, 20.0, 5.0, 0.0)?;
            apply(Op::Cut, &rod, &slid(0.0, 6.0, 2.0)?)
        }
        "swept/two-beads-slid-through-rod" => {
            let rod = rod(s, 2, [0.0, 0.0], 0.0, 20.0, 5.0, 0.0)?;
            let one = apply(Op::Cut, &rod, &slid(0.0, 6.0, 2.0)?)?;
            apply(Op::Cut, &one, &slid(1.5, 6.0, 1.5)?)
        }
        other => Err(Grade::Refused(format!(
            "{other}: what a body passes through as it turns and moves at once is not made yet"
        ))),
    }
}

/// brep-bench's singular cases: cuts and meetings through the points a
/// surface comes to (a ball's poles, a cone's apex).
fn singular(s: &mut Scene, id: &str) -> Built {
    // What of `solid` is behind the plane through `at` across `normal`.
    let behind = |solid: &Solid, at: [f64; 3], normal: [f64; 3]| -> Built {
        let halves =
            workpiece_shape::halves(solid, Point3::from_array(at), Point3::from_array(normal));
        halves
            .map(|[_, behind]| behind)
            .map_err(|why| Grade::Refused(format!("{why:#}")))
    };
    let cone = |s: &mut Scene, radius: f64, height: f64| {
        setup(s.turned(
            [0.0; 3],
            upright(),
            &[[0.0, 0.0], [radius, 0.0], [0.0, height]],
            None,
            None,
        ))
    };
    match id {
        "singular/ball-cut-aslant-through-a-pole" => {
            let ball = ball(s, [0.0; 3], 5.0, ID)?;
            behind(&ball, [5.0, 0.0, 0.0], [1.0, 0.0, 1.0])
        }
        "singular/ball-halved-through-its-poles" => {
            let ball = ball(s, [0.0; 3], 5.0, ID)?;
            behind(&ball, [0.0; 3], [-1.0, 0.0, 0.0])
        }
        "singular/balls-meeting-through-a-pole" => {
            let a = ball(s, [0.0; 3], 5.0, ID)?;
            let b = ball(s, [4.8, 0.0, 3.6], 5.0, ID)?;
            apply(Op::Union, &a, &b)
        }
        "singular/cone-cut-aslant-through-its-apex" => {
            let cone = cone(s, 5.0, 10.0)?;
            behind(&cone, [0.0, 0.0, 10.0], [4.0, 0.0, 1.0])
        }
        "singular/cone-halved-through-its-apex" => {
            let cone = cone(s, 5.0, 10.0)?;
            behind(&cone, [0.0; 3], [-1.0, 0.0, 0.0])
        }
        "singular/napkin-ring-poles-on-its-rims" => {
            let lean = DQuat::from_rotation_arc(Point3::Z, Point3::new(0.6, 0.0, 0.8));
            let a = ball(s, [0.0; 3], 5.0, lean)?;
            let b = rod(s, 2, [0.0, 0.0], -6.0, 6.0, 3.0, 0.0)?;
            apply(Op::Cut, &a, &b)
        }
        "singular/saddle-drum-cut-level-through-its-saddle" => {
            use workpiece_core::math::Point2;
            use workpiece_core::world::topo::Curve3;
            use workpiece_shape::swept::{Outline, polygon, swept};
            // All over the saddle z = 10 + xy/8 above the square: a
            // section across x whose floor leans as x goes.
            let (sides, _) = polygon(&[Point2::ZERO, Point2::X, Point2::ONE, Point2::Y]);
            let at = |f: f64| {
                let x = -10.0 + 20.0 * f;
                polygon(&[
                    Point2::new(-10.0, 10.0 - 1.25 * x),
                    Point2::new(10.0, 10.0 + 1.25 * x),
                    Point2::new(10.0, 40.0),
                    Point2::new(-10.0, 40.0),
                ])
                .1
            };
            let path = Curve3::line(Point3::new(-10.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0))
                .map_err(|_| Grade::Crashed("setup: a path".into()))?;
            let above = swept(
                "above",
                &Outline { sides, at: &at },
                &path,
                (0.0, 1.0),
                Point3::Z,
                60.0,
            )
            .map_err(|why| Grade::Refused(format!("{why:#}")))?;
            let whole = rod(s, 2, [0.0, 0.0], 0.0, 30.0, 8.0, 0.3)?;
            let drum = apply(Op::Cut, &whole, &above)?;
            behind(&drum, [0.0, 0.0, 10.0], [0.0, 0.0, 1.0])
        }
        "singular/sectors-sharing-an-apex" => {
            // A cone from the origin up, wide and tall enough to reach
            // out of the ball; and the same, leant 60° about y.
            let ball = ball(s, [0.0; 3], 10.0, rotation_x(0.7))?;
            let up = setup(s.turned(
                [0.0; 3],
                upright(),
                &[[0.0, 0.0], [12.0, 12.0], [0.0, 12.0]],
                None,
                None,
            ))?;
            let lean = rotation_y(60.0_f64.to_radians());
            let leant = up
                .moved(&|p| lean * p)
                .map_err(|why| Grade::Crashed(format!("setup: {why}")))?;
            let first = apply(Op::Intersect, &ball, &up)?;
            apply(Op::Intersect, &first, &leant)
        }
        other => Err(Grade::Refused(format!(
            "{other}: this shape is not made yet"
        ))),
    }
}

/// brep-bench's direct edits: faces of a finished body moved or offset.
fn edits(s: &mut Scene, id: &str) -> Built {
    use workpiece_shape::{faces_moved, faces_offset};
    let done = |made: anyhow::Result<Solid>| made.map_err(|why| Grade::Refused(format!("{why:#}")));
    let drilled = |s: &mut Scene, height: f64| {
        let block = block(s, [0.0; 3], [40.0, 30.0, height])?;
        let bore = rod(s, 2, [20.0, 15.0], -1.0, height + 1.0, 5.0, 0.0)?;
        apply(Op::Cut, &block, &bore)
    };
    let up = Point3::new(0.0, 0.0, 6.0);
    match id {
        "edit/block-top-moved-up" => {
            let block = drilled(s, 10.0)?;
            let top = faces_at(&block, |p| near(p.z, 10.0));
            done(faces_moved(&block, &top, up))
        }
        "edit/hole-widened" => {
            let plate = drilled(s, 8.0)?;
            let wall = faces_at(&plate, |p| near(libm::hypot(p.x - 20.0, p.y - 15.0), 5.0));
            done(faces_offset(&plate, &wall, -2.0))
        }
        "edit/l-block-riser-moved" => {
            let foot = block(s, [0.0, 0.0, 0.0], [40.0, 20.0, 10.0])?;
            let upright = block(s, [0.0, 0.0, 10.0], [15.0, 20.0, 30.0])?;
            let l = apply(Op::Union, &foot, &upright)?;
            let riser = faces_at(&l, |p| near(p.x, 15.0));
            done(faces_moved(&l, &riser, Point3::new(10.0, 0.0, 0.0)))
        }
        "edit/rounded-block-top-moved-up" => {
            let block = block(s, [0.0; 3], [40.0, 30.0, 10.0])?;
            let edge = pick(&block, |a, _, b| {
                near(a.z, 10.0) && near(b.z, 10.0) && near(a.y, 30.0) && near(b.y, 30.0)
            });
            let rounded = blended(&block, &edge, &round(3.0), false)?;
            // The top, and the round with it.
            let mut carried = faces_at(&rounded, |p| near(p.z, 10.0));
            carried.extend(faces_at(&rounded, |p| {
                near(libm::hypot(p.y - 27.0, p.z - 7.0), 3.0)
            }));
            done(faces_moved(&rounded, &carried, up))
        }
        other => Err(Grade::Refused(format!("{other}: not made yet"))),
    }
}

/// brep-bench's feature cases: what is extruded or turned until it
/// meets the body ahead.
fn features(s: &mut Scene, id: &str) -> Built {
    use workpiece_core::math::Point2;
    use workpiece_core::world::topo::Curve3;
    use workpiece_shape::swept::{Outline, polygon, steady, swept};
    let lumps = |solid: &Solid| solid.lumps().map_err(|why| Grade::Refused(why.to_string()));
    // The lump of `solid` with a corner where `there` says.
    let lump_at = |solid: &Solid, there: &dyn Fn(Point3) -> bool| -> Built {
        lumps(solid)?
            .into_iter()
            .find(|lump| lump.vertices.iter().any(|v| there(v.point)))
            .ok_or_else(|| Grade::Refused("nothing lies ahead to stop at".into()))
    };
    // A prism over the polygon `corners` in the xy plane, between two
    // heights.
    let prism = |corners: &[Point2], from: f64, to: f64, id: &str| -> Built {
        let path = Curve3::line(Point3::new(0.0, 0.0, from), Point3::new(0.0, 0.0, to))
            .map_err(|_| Grade::Crashed("setup: a path".into()))?;
        let (sides, points) = polygon(corners);
        let at = steady(&points);
        swept(
            id,
            &Outline { sides, at: &at },
            &path,
            (0.0, 1.0),
            Point3::Y,
            100.0,
        )
        .map_err(|why| Grade::Refused(format!("{why:#}")))
    };
    let ray = |degrees: f64, far: f64| from_angle(degrees.to_radians()) * far;
    match id {
        "feature/hole-through-the-upper-arm" => {
            let lower = block(s, [0.0, -10.0, 0.0], [40.0, 10.0, 6.0])?;
            let back = block(s, [0.0, -10.0, 6.0], [6.0, 10.0, 24.0])?;
            let upper = block(s, [0.0, -10.0, 24.0], [40.0, 10.0, 30.0])?;
            let bracket = apply(Op::Union, &apply(Op::Union, &lower, &back)?, &upper)?;
            // The drill, and of what it meets, what it meets first.
            let drill = rod(s, 2, [28.0, 0.0], -1.0, 31.0, 4.0, 0.0)?;
            let met = apply(Op::Intersect, &drill, &bracket)?;
            let first = lump_at(&met, &|p| near(p.z, 30.0))?;
            apply(Op::Cut, &bracket, &first)
        }
        "feature/pad-down-to-cylinder" => {
            let drum = rod(s, 0, [0.0, 0.0], -20.0, 20.0, 10.0, 0.0)?;
            let pad = block(s, [-3.0, -3.0, -15.0], [3.0, 3.0, 15.0])?;
            let rest = apply(Op::Cut, &pad, &drum)?;
            let above = lump_at(&rest, &|p| near(p.z, 15.0))?;
            apply(Op::Union, &drum, &above)
        }
        "feature/post-up-to-dome" => {
            let plate = rod(s, 2, [0.0, 0.0], -2.0, 0.0, 20.0, 0.0)?;
            let outer = ball(s, [0.0; 3], 20.0, ID)?;
            let inner = ball(s, [0.0; 3], 18.0, ID)?;
            let below = block(s, [-25.0, -25.0, -25.0], [25.0, 25.0, 0.0])?;
            let shell = apply(Op::Cut, &apply(Op::Cut, &outer, &inner)?, &below)?;
            let dome = apply(Op::Union, &plate, &shell)?;
            let post = rod(s, 2, [0.0, 0.0], -1.0, 25.0, 3.0, 0.3)?;
            let rest = apply(Op::Cut, &post, &dome)?;
            let inside = lump_at(&rest, &|p| near(p.z, 0.0))?;
            apply(Op::Union, &dome, &inside)
        }
        "feature/rib-revolved-to-a-stop" => {
            let hub = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 10.0, 0.0)?;
            let reach = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 25.0, 0.2)?;
            let between = prism(
                &[Point2::ZERO, ray(120.0, 60.0), ray(135.0, 60.0)],
                -1.0,
                11.0,
                "wedge",
            )?;
            let stop = apply(Op::Intersect, &reach, &between)?;
            let body = apply(Op::Union, &hub, &stop)?;
            // The rib, turned on past the stop.
            let disk = rod(s, 2, [0.0, 0.0], 0.0, 4.0, 20.0, 0.4)?;
            let bore = rod(s, 2, [0.0, 0.0], -1.0, 5.0, 5.0, 0.6)?;
            let ring = apply(Op::Cut, &disk, &bore)?;
            let turned = prism(
                &[
                    Point2::ZERO,
                    ray(0.0, 60.0),
                    ray(45.0, 80.0),
                    ray(90.0, 60.0),
                    ray(128.0, 60.0),
                ],
                -1.0,
                5.0,
                "turned",
            )?;
            let rib = apply(Op::Intersect, &ring, &turned)?;
            let rest = apply(Op::Cut, &rib, &body)?;
            let first = lump_at(&rest, &|p| near(p.y, 0.0) && p.x > 1.0)?;
            apply(Op::Union, &body, &first)
        }
        other => Err(Grade::Refused(format!("{other}: not made yet"))),
    }
}

/// brep-bench's loft cases: sections up the z axis.
fn lofts(id: &str) -> Built {
    use workpiece_core::math::Point2;
    use workpiece_core::world::topo::Curve3;
    use workpiece_shape::swept::{Outline, polygon, swept};
    let up = |height: f64,
              sides: Vec<(usize, Vec<f64>, usize)>,
              at: &dyn Fn(f64) -> Vec<(Point2, f64)>| {
        let path = Curve3::line(Point3::ZERO, Point3::new(0.0, 0.0, height))
            .map_err(|e| Grade::Crashed(format!("setup: {e}")))?;
        swept(
            "loft",
            &Outline { sides, at },
            &path,
            (0.0, 1.0),
            Point3::Y,
            60.0,
        )
        .map_err(|why| Grade::Refused(format!("{why:#}")))
    };
    let lines = |n: usize| vec![(1, vec![0.0, 0.0, 1.0, 1.0], 2); n];
    // A rectangle of half sides `a` and `b`, turned by `angle`.
    let turned = |a: f64, b: f64, angle: f64| {
        let turn = |p: Point2| from_angle(angle).rotate(p);
        polygon(&[
            turn(Point2::new(-a, -b)),
            turn(Point2::new(a, -b)),
            turn(Point2::new(a, b)),
            turn(Point2::new(-a, b)),
        ])
        .1
    };
    // Where the guide with these control heights is at the height z,
    // and what the coordinate with these control values is there.
    let bezier = |c: [f64; 4], t: f64| {
        let u = 1.0 - t;
        u * u * u * c[0] + 3.0 * u * u * t * c[1] + 3.0 * u * t * t * c[2] + t * t * t * c[3]
    };
    let heights = [0.0, 8.0, 19.0, 24.0];
    let on_guide = |z: f64| {
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        for _ in 0..80 {
            let mid = (lo + hi) / 2.0;
            if bezier(heights, mid) < z {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    };
    match id {
        "loft/twisted-tapering-square" => up(40.0, lines(4), &|f| {
            let z = 40.0 * f;
            turned(15.0 - z / 4.0, 15.0 - z / 4.0, (4.5 * z).to_radians())
        }),
        "loft/twisted-thin-blade" => up(60.0, lines(4), &|f| {
            turned(20.0, 0.2, (360.0 * f).to_radians())
        }),
        "loft/slab-pinched-on-four-guides" => up(24.0, lines(4), &|f| {
            let t = on_guide(24.0 * f);
            turned(
                bezier([12.0, 18.0, 15.0, 3.0], t),
                bezier([3.2, -2.8, 0.2, 12.2], t),
                0.0,
            )
        }),
        "loft/square-spire-on-four-guides" => up(24.0, lines(4), &|f| {
            let t = if f >= 1.0 { 1.0 } else { on_guide(24.0 * f) };
            let half = bezier([9.0, 15.0, 12.0, 0.0], t);
            turned(half, half, 0.0)
        }),
        "loft/square-to-octagon" => {
            // Around from the side facing -y: a side of the square to
            // the side of the octagon over it, then a corner to a cut
            // side.
            let foot = [(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)];
            let top = [
                (-6.0, -14.0),
                (6.0, -14.0),
                (14.0, -6.0),
                (14.0, 6.0),
                (6.0, 14.0),
                (-6.0, 14.0),
                (-14.0, 6.0),
                (-14.0, -6.0),
            ];
            up(30.0, lines(8), &|f| {
                let at = |k: usize| {
                    let (a, b) = (foot[k.div_ceil(2) % 4], top[k % 8]);
                    (Point2::new(a.0, a.1).lerp(Point2::new(b.0, b.1), f), 1.0)
                };
                (0..8).flat_map(|k| [at(k), at(k + 1)]).collect()
            })
        }
        "loft/square-to-round" => {
            // A side of the square to a point of the circle, then a
            // corner to a quarter of the circle.
            let foot = [(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)];
            let round = [(0.0, -15.0), (15.0, 0.0), (0.0, 15.0), (-15.0, 0.0)];
            let w = std::f64::consts::FRAC_1_SQRT_2;
            let mut sides = Vec::new();
            for _ in 0..4 {
                sides.push((1, vec![0.0, 0.0, 1.0, 1.0], 2));
                sides.push((2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 3));
            }
            up(30.0, sides, &|f| {
                let p = |a: (f64, f64)| Point2::new(a.0, a.1);
                let mut all = Vec::new();
                for k in 0..4 {
                    let (a, b) = (p(foot[k]), p(foot[(k + 1) % 4]));
                    let (r, q) = (p(round[k]), p(round[(k + 1) % 4]));
                    all.push((a.lerp(r, f), 1.0));
                    all.push((b.lerp(r, f), 1.0));
                    all.push((b.lerp(r, f), 1.0));
                    all.push((b.lerp(r + q, f), w));
                    all.push((b.lerp(q, f), 1.0));
                }
                all
            })
        }
        other => Err(Grade::Refused(format!("{other}: not lofted yet"))),
    }
}

/// brep-bench's sweep cases.
fn sweeps(s: &mut Scene, id: &str) -> Built {
    use workpiece_core::math::Point2;
    use workpiece_core::world::topo::Curve3;
    use workpiece_shape::swept::{Outline, circle, polygon, steady, swept, swept_runs};
    let done = |made: anyhow::Result<Solid>| made.map_err(|why| Grade::Refused(format!("{why:#}")));
    // A quarter of a circle of `radius` in the xy plane about z, from +x.
    let quarter = |radius: f64| {
        Curve3::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(radius, 0.0, 0.0),
                Point3::new(radius, radius, 0.0),
                Point3::new(0.0, radius, 0.0),
            ],
            Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
        )
        .map_err(|e| Grade::Crashed(format!("setup: {e}")))
    };
    let square = |h: f64| {
        [
            Point2::new(-h, -h),
            Point2::new(h, -h),
            Point2::new(h, h),
            Point2::new(-h, h),
        ]
    };
    // A square duct: its outside less its inside, each half as wide as
    // `half` says at each share of the way.
    let duct = |path: &Curve3, half: &dyn Fn(f64) -> f64, wall: f64, size: f64| -> Built {
        let tube = |less: f64, id: &str| {
            let (sides, _) = polygon(&square(1.0));
            let at = |f: f64| polygon(&square(half(f) - less)).1;
            let outline = Outline { sides, at: &at };
            done(swept(id, &outline, path, path.domain(), Point3::Z, size))
        };
        apply(Op::Cut, &tube(0.0, "outside")?, &tube(wall, "inside")?)
    };
    match id {
        "sweep/circle-along-arc" => {
            let (sides, points) = circle(Point2::ZERO, 2.0);
            let at = steady(&points);
            let path = quarter(10.0)?;
            done(swept(
                "sweep",
                &Outline { sides, at: &at },
                &path,
                (0.0, 1.0),
                Point3::Z,
                20.0,
            ))
        }
        "sweep/duct-elbow" => duct(&quarter(30.0)?, &|_| 9.0, 2.0, 60.0),
        "sweep/narrowing-bend" => duct(&quarter(30.0)?, &|f| 9.0 - 3.0 * f, 2.0, 60.0),
        "sweep/hollow-reducer" => {
            let path = Curve3::line(Point3::ZERO, Point3::new(0.0, 40.0, 0.0))
                .map_err(|e| Grade::Crashed(format!("setup: {e}")))?;
            duct(&path, &|f| 12.0 - 6.0 * f, 2.0, 60.0)
        }
        "sweep/twisted-duct" => {
            let path = Curve3::line(Point3::ZERO, Point3::new(0.0, 40.0, 0.0))
                .map_err(|e| Grade::Crashed(format!("setup: {e}")))?;
            let from = square(10.0);
            let to = [
                Point2::new(0.0, -12.0),
                Point2::new(12.0, 0.0),
                Point2::new(0.0, 12.0),
                Point2::new(-12.0, 0.0),
            ];
            let (sides, _) = polygon(&from);
            let at = |f: f64| {
                let corners: Vec<Point2> = (0..4).map(|k| from[k].lerp(to[k], f)).collect();
                polygon(&corners).1
            };
            done(swept(
                "sweep",
                &Outline { sides, at: &at },
                &path,
                (0.0, 1.0),
                Point3::Z,
                60.0,
            ))
        }
        "sweep/helical-pipe" | "sweep/helical-pipe-round-ends" => {
            // The helix as cubics an eighth of a turn each, with the
            // helix's own directions at their ends.
            let (radius, rise, turns, pieces) = (10.0_f64, 5.0_f64, 3.0_f64, 24_usize * 4);
            let step = turns * std::f64::consts::TAU / pieces as f64;
            let at = |a: f64| {
                Point3::new(
                    radius * libm::cos(a),
                    radius * libm::sin(a),
                    rise * a / std::f64::consts::TAU,
                )
            };
            let way = |a: f64| {
                Point3::new(
                    -radius * libm::sin(a),
                    radius * libm::cos(a),
                    rise / std::f64::consts::TAU,
                )
            };
            let handle = 4.0 / 3.0 * libm::tan(step / 4.0);
            let mut points = vec![at(0.0)];
            let mut knots = vec![0.0; 4];
            for k in 0..pieces {
                let (a, b) = (step * k as f64, step * (k + 1) as f64);
                points.extend([at(a) + way(a) * handle, at(b) - way(b) * handle, at(b)]);
                knots.extend([(k + 1) as f64; 3]);
            }
            knots.push(pieces as f64);
            let path = Curve3::new(3, knots, points, None)
                .map_err(|e| Grade::Crashed(format!("setup: {e}")))?;
            let (sides, points) = circle(Point2::ZERO, 1.0);
            let at = steady(&points);
            let range = path.domain();
            let pipe = done(swept(
                "sweep",
                &Outline { sides, at: &at },
                &path,
                range,
                Point3::Z,
                30.0,
            ))?;
            if id == "sweep/helical-pipe" {
                return Ok(pipe);
            }
            // A ball at each end, turned to no plane of the pipe's.
            let skew = rotation(Point3::new(1.0, 2.0, 3.0).normalize(), 1.0);
            let (first, last) = (path.point_at(range.0), path.point_at(range.1));
            let one = apply(Op::Union, &pipe, &ball(s, first.to_array(), 1.0, skew)?)?;
            apply(Op::Union, &one, &ball(s, last.to_array(), 1.0, skew)?)
        }
        "sweep/round-cornered-pipe" => {
            let (sides, points) = circle(Point2::ZERO, 3.0);
            let run = |from: Point3, to: Point3, id: &str| {
                let line =
                    Curve3::line(from, to).map_err(|e| Grade::Crashed(format!("setup: {e}")))?;
                let at = steady(&points);
                let outline = Outline {
                    sides: sides.clone(),
                    at: &at,
                };
                done(swept(id, &outline, &line, (0.0, 1.0), Point3::Z, 60.0))
            };
            let first = run(Point3::new(-30.0, 0.0, 0.0), Point3::ZERO, "first")?;
            let second = run(Point3::ZERO, Point3::new(0.0, 30.0, 0.0), "second")?;
            let skew = rotation(Point3::new(1.0, 2.0, 3.0).normalize(), 1.0);
            let corner = ball(s, [0.0; 3], 3.0, skew)?;
            let one = apply(Op::Union, &first, &corner)?;
            apply(Op::Union, &one, &second)
        }
        "sweep/mitered-elbow" => {
            let (sides, points) = polygon(&square(5.0));
            let corners = [
                Point3::ZERO,
                Point3::new(40.0, 0.0, 0.0),
                Point3::new(20.0, 20.0 * 3f64.sqrt(), 0.0),
            ];
            done(swept_runs(
                "sweep",
                &sides,
                &points,
                &corners,
                false,
                Point3::Z,
                80.0,
            ))
        }
        "sweep/mitered-pipe-on-cube-edges" => {
            let (sides, points) = circle(Point2::ZERO, 2.0);
            let corners = [
                Point3::ZERO,
                Point3::new(30.0, 0.0, 0.0),
                Point3::new(30.0, 30.0, 0.0),
                Point3::new(30.0, 30.0, 30.0),
            ];
            done(swept_runs(
                "sweep",
                &sides,
                &points,
                &corners,
                false,
                Point3::Z,
                60.0,
            ))
        }
        "sweep/mitered-picture-frame" => {
            // (s, z) with s out from the path: the section's x is to
            // the left of the way around, which goes counter-clockwise.
            let outline: Vec<Point2> = [
                (0.0, 2.0),
                (3.0, 2.0),
                (3.0, 0.0),
                (8.0, 0.0),
                (8.0, 6.0),
                (0.0, 6.0),
            ]
            .into_iter()
            .map(|(s, z)| Point2::new(-s, z))
            .collect();
            let (sides, points) = polygon(&outline);
            let corners = [
                Point3::new(-30.0, -20.0, 0.0),
                Point3::new(30.0, -20.0, 0.0),
                Point3::new(30.0, 20.0, 0.0),
                Point3::new(-30.0, 20.0, 0.0),
            ];
            done(swept_runs(
                "sweep",
                &sides,
                &points,
                &corners,
                true,
                Point3::Z,
                100.0,
            ))
        }
        other => Err(Grade::Refused(format!("{other}: not swept yet"))),
    }
}

/// brep-bench's draft cases: the pull is +z everywhere.
fn drafts(s: &mut Scene, id: &str) -> Built {
    let done = |made: anyhow::Result<Solid>| made.map_err(|why| Grade::Refused(format!("{why:#}")));
    let draft =
        |solid: &Solid, at: &dyn Fn(Point3) -> bool, neutral: f64, degrees: f64, both: bool| {
            let faces = faces_at(solid, at);
            if faces.is_empty() {
                return Err(Grade::Crashed("setup: no face to draft was found".into()));
            }
            let from = Point3::new(0.0, 0.0, neutral);
            done(drafted(
                solid,
                &faces,
                Point3::Z,
                from,
                degrees.to_radians(),
                both,
            ))
        };
    // The upright faces: those that are not all at one height.
    let upright = |solid: &Solid| -> Vec<usize> {
        (0..solid.faces.len())
            .filter(|&f| {
                let face = &solid.faces[f];
                let mut heights = face.loops.iter().flatten().map(|step| {
                    let uv = solid.start(step);
                    face.surface.point_at(uv.x, uv.y).z
                });
                let first = heights.next().unwrap_or(0.0);
                heights.any(|z| !near(z, first))
            })
            .collect()
    };
    let sides = |solid: &Solid, neutral: f64, degrees: f64, of: &dyn Fn(Point3) -> bool| {
        let faces: Vec<usize> = upright(solid)
            .into_iter()
            .filter(|f| faces_at(solid, of).contains(f))
            .collect();
        let from = Point3::new(0.0, 0.0, neutral);
        done(drafted(
            solid,
            &faces,
            Point3::Z,
            from,
            degrees.to_radians(),
            false,
        ))
    };
    let all = |_: Point3| true;
    let pocketed = |s: &mut Scene, rounded: bool| {
        let whole = block(s, [0.0; 3], [40.0, 30.0, 15.0])?;
        let mut pocket = block(s, [8.0, 8.0, 5.0], [32.0, 22.0, 16.0])?;
        if rounded {
            let corners = pick(&pocket, |a, _, b| near(a.x, b.x) && near(a.y, b.y));
            pocket = blended(&pocket, &corners, &round(4.0), false)?;
        }
        apply(Op::Cut, &whole, &pocket)
    };
    let in_pocket = |p: Point3| p.x > 7.9 && p.x < 32.1 && p.y > 7.9 && p.y < 22.1 && p.z > 4.9;
    match id {
        "draft/box-four-sides" => sides(&block(s, [0.0; 3], [20.0, 12.0, 10.0])?, 0.0, 5.0, &all),
        "draft/box-outward" => sides(&block(s, [0.0; 3], [20.0, 12.0, 10.0])?, 0.0, -5.0, &all),
        "draft/box-about-its-middle" => {
            sides(&block(s, [0.0; 3], [20.0, 12.0, 10.0])?, 5.0, 5.0, &all)
        }
        "draft/box-top-drafted-away" => {
            sides(&block(s, [0.0; 3], [30.0, 10.0, 10.0])?, 0.0, 30.0, &all)
        }
        "draft/box-one-side" => draft(
            &block(s, [0.0; 3], [20.0, 12.0, 10.0])?,
            &|p| near(p.x, 20.0),
            0.0,
            10.0,
            false,
        ),
        "draft/box-cross-drilled" => {
            let whole = block(s, [0.0; 3], [30.0, 20.0, 20.0])?;
            let bore = rod(s, 0, [10.0, 10.0], -1.0, 31.0, 4.0, 0.0)?;
            let drilled = apply(Op::Cut, &whole, &bore)?;
            draft(
                &drilled,
                &|p| near(p.x, 0.0) || near(p.x, 30.0),
                0.0,
                8.0,
                false,
            )
            .or_else(|_| {
                let mut faces = faces_at(&drilled, |p| near(p.x, 0.0));
                faces.extend(faces_at(&drilled, |p| near(p.x, 30.0)));
                done(drafted(
                    &drilled,
                    &faces,
                    Point3::Z,
                    Point3::ZERO,
                    8f64.to_radians(),
                    false,
                ))
            })
        }
        "draft/boss-on-plate" => {
            let plate = block(s, [-20.0, -20.0, 0.0], [20.0, 20.0, 5.0])?;
            let boss = rod(s, 2, [0.0, 0.0], 5.0, 17.0, 8.0, 0.0)?;
            let both = apply(Op::Union, &plate, &boss)?;
            draft(
                &both,
                &|p| near(libm::hypot(p.x, p.y), 8.0),
                5.0,
                4.0,
                false,
            )
        }
        "draft/cylinder-parted-both-ways" => {
            let solid = rod(s, 2, [0.0, 0.0], 0.0, 20.0, 10.0, 0.0)?;
            draft(
                &solid,
                &|p| near(libm::hypot(p.x, p.y), 10.0),
                10.0,
                3.0,
                true,
            )
        }
        "draft/hole-tapered" => {
            let whole = block(s, [-15.0, -15.0, 0.0], [15.0, 15.0, 20.0])?;
            let bore = rod(s, 2, [0.0, 0.0], -1.0, 21.0, 6.0, 0.0)?;
            draft(
                &apply(Op::Cut, &whole, &bore)?,
                &|p| near(libm::hypot(p.x, p.y), 6.0),
                0.0,
                3.0,
                false,
            )
        }
        "draft/l-block-sides" => {
            let long = block(s, [0.0, 0.0, 0.0], [30.0, 10.0, 12.0])?;
            let short = block(s, [0.0, 10.0, 0.0], [10.0, 25.0, 12.0])?;
            sides(&apply(Op::Union, &long, &short)?, 0.0, 6.0, &all)
        }
        "draft/pocket-walls" => sides(&pocketed(s, false)?, 5.0, 5.0, &in_pocket),
        "draft/rounded-pocket-walls" => sides(&pocketed(s, true)?, 5.0, 5.0, &in_pocket),
        "draft/rounded-box-sides" => {
            let whole = block(s, [0.0; 3], [30.0, 20.0, 10.0])?;
            let corners = pick(&whole, |a, _, b| near(a.x, b.x) && near(a.y, b.y));
            sides(
                &blended(&whole, &corners, &round(4.0), false)?,
                0.0,
                5.0,
                &all,
            )
        }
        other => Err(Grade::Refused(format!("{other}: not made yet"))),
    }
}

/// brep-bench's thicken cases: each sheet is faces of a solid, thickened
/// out of that solid (a distance more than nothing) or into it.
fn sheets(s: &mut Scene, id: &str) -> Built {
    let done = |made: anyhow::Result<Solid>| made.map_err(|why| Grade::Refused(format!("{why:#}")));
    let make = |solid: &Solid, at: &dyn Fn(Point3) -> bool, d: f64, both: bool, sharp: bool| {
        let faces = faces_at(solid, at);
        if faces.is_empty() {
            return Err(Grade::Crashed(
                "setup: no face of the sheet was found".into(),
            ));
        }
        done(thicken(solid, &faces, d, both, sharp))
    };
    let on_ball = |r: f64| move |p: Point3| near(p.length(), r);
    let around = |r: f64| move |p: Point3| near(libm::hypot(p.x, p.y), r);
    match id {
        "thicken/angle-bracket-inward"
        | "thicken/angle-bracket-rounded"
        | "thicken/angle-bracket-sharp" => {
            let solid = block(s, [0.0; 3], [10.0, 8.0, 10.0])?;
            let mut faces = faces_at(&solid, |p| near(p.z, 0.0));
            faces.extend(faces_at(&solid, |p| near(p.x, 0.0)));
            let d = if id.ends_with("inward") { -1.0 } else { 1.0 };
            done(thicken(&solid, &faces, d, false, id.ends_with("sharp")))
        }
        "thicken/bent-flange" | "thicken/bent-flange-past-its-bend" => {
            let solid = block(s, [-10.0, 0.0, 0.0], [2.0, 8.0, 12.0])?;
            let bend = pick(&solid, |a, _, b| {
                near(a.x, 2.0) && near(b.x, 2.0) && near(a.z, 0.0) && near(b.z, 0.0)
            });
            let bent = blended(&solid, &bend, &round(2.0), false)?;
            let sheet = |p: Point3| {
                near(p.z, 0.0)
                    || near(p.x, 2.0)
                    || near(libm::hypot(p.x - 0.0, p.z - 2.0), 2.0)
                        && p.x >= -1e-9
                        && p.z <= 2.0 + 1e-9
            };
            let mut faces = faces_at(&bent, |p| near(p.z, 0.0));
            faces.extend(faces_at(&bent, |p| near(p.x, 2.0)));
            faces.extend(faces_at(&bent, |p| {
                near(libm::hypot(p.x, p.z - 2.0), 2.0) && !near(p.z, 0.0) | !near(p.x, 2.0)
            }));
            let _ = sheet;
            faces.sort_unstable();
            faces.dedup();
            let d = if id.ends_with("bend") { -3.0 } else { 1.0 };
            done(thicken(&bent, &faces, d, false, false))
        }
        "thicken/cone-frustum" => {
            let solid = setup(s.turned(
                [0.0; 3],
                upright(),
                &[[0.0, 0.0], [6.0, 0.0], [3.0, 4.0], [0.0, 4.0]],
                None,
                None,
            ))?;
            make(
                &solid,
                &|p| near(libm::hypot(p.x, p.y), 6.0 - 0.75 * p.z) && libm::hypot(p.x, p.y) > 2.9,
                1.0,
                false,
                false,
            )
        }
        "thicken/half-pipe-inward" | "thicken/half-pipe-outward" => {
            let whole = rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?;
            let front = block(s, [-6.0, -6.0, -1.0], [6.0, 0.0, 11.0])?;
            let half = apply(Op::Cut, &whole, &front)?;
            let d = if id.ends_with("inward") { -2.0 } else { 1.0 };
            make(&half, &around(5.0), d, false, false)
        }
        "thicken/hemisphere-both-sides" | "thicken/hemisphere-to-its-center" => {
            let whole = ball(s, [0.0; 3], 5.0, ID)?;
            let below = block(s, [-6.0, -6.0, -6.0], [6.0, 6.0, 0.0])?;
            let half = apply(Op::Cut, &whole, &below)?;
            if id.ends_with("sides") {
                make(&half, &on_ball(5.0), 1.0, true, false)
            } else {
                make(&half, &on_ball(5.0), -5.0, false, false)
            }
        }
        "thicken/open-box-inward"
        | "thicken/open-box-outward-rounded"
        | "thicken/open-box-outward-sharp" => {
            let solid = block(s, [0.0; 3], [10.0; 3])?;
            let top = faces_at(&solid, |p| near(p.z, 10.0));
            let faces: Vec<usize> = (0..solid.faces.len())
                .filter(|f| !top.contains(f))
                .collect();
            let d = if id.ends_with("inward") { -1.0 } else { 1.0 };
            done(thicken(&solid, &faces, d, false, id.ends_with("sharp")))
        }
        "thicken/parabolic-trough" => {
            use workpiece_core::math::Point2;
            use workpiece_core::world::topo::Curve3;
            use workpiece_shape::swept::{Outline, steady, swept};
            // What is over the parabola, under a roof: its underside is
            // the sheet.
            let line = (1, vec![0.0, 0.0, 1.0, 1.0], 2);
            let sides = vec![
                (2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 3),
                line.clone(),
                line,
            ];
            let p = |x: f64, z: f64| (Point2::new(x, z), 1.0);
            let points = [
                p(-4.0, 2.0),
                p(0.0, -2.0),
                p(4.0, 2.0),
                p(4.0, 2.0),
                p(0.0, 6.0),
                p(0.0, 6.0),
                p(-4.0, 2.0),
            ];
            let at = steady(&points);
            let path = Curve3::line(Point3::ZERO, Point3::new(0.0, 10.0, 0.0))
                .map_err(|_| Grade::Crashed("setup: a path".into()))?;
            let over = done(swept(
                "trough",
                &Outline { sides, at: &at },
                &path,
                (0.0, 1.0),
                Point3::Z,
                30.0,
            ))?;
            make(&over, &|p| near(p.z, p.x * p.x / 8.0), -1.0, false, false)
        }
        "thicken/plate-with-holes" => {
            let plate = block(s, [-10.0, -10.0, 0.0], [10.0, 10.0, 5.0])?;
            let left = rod(s, 2, [-5.0, 0.0], -1.0, 6.0, 3.0, 0.0)?;
            let right = rod(s, 2, [5.0, 0.0], -1.0, 6.0, 3.0, 0.0)?;
            let once = apply(Op::Cut, &plate, &left)?;
            make(
                &apply(Op::Cut, &once, &right)?,
                &|p| near(p.z, 0.0),
                1.0,
                true,
                false,
            )
        }
        "thicken/rectangle" => {
            let solid = block(s, [0.0; 3], [10.0, 8.0, 5.0])?;
            make(&solid, &|p| near(p.z, 0.0), -2.0, false, false)
        }
        "thicken/sphere-skin" => make(&ball(s, [0.0; 3], 5.0, ID)?, &|_| true, 1.0, false, false),
        "thicken/spherical-cap" => {
            let whole = ball(s, [0.0; 3], 10.0, ID)?;
            let below = block(s, [-11.0, -11.0, -11.0], [11.0, 11.0, 6.0])?;
            make(
                &apply(Op::Cut, &whole, &below)?,
                &on_ball(10.0),
                2.0,
                false,
                false,
            )
        }
        "thicken/torus-inner-half" => {
            let whole = torus(s, 6.0, 2.0, 0.0)?;
            let core = rod(s, 2, [0.0, 0.0], -3.0, 3.0, 6.0, 0.0)?;
            let inner = apply(Op::Intersect, &whole, &core)?;
            make(
                &inner,
                &|p| libm::hypot(p.x, p.y) < 6.0 - 1e-6 || near(p.z.abs(), 2.0),
                1.0,
                false,
                false,
            )
        }
        "thicken/tube" => make(
            &rod(s, 2, [0.0, 0.0], 0.0, 10.0, 5.0, 0.0)?,
            &around(5.0),
            1.0,
            false,
            false,
        ),
        other => Err(Grade::Refused(format!(
            "{other}: this sheet is not made yet"
        ))),
    }
}

/// Solids put together by hand, face by face, where a case gives the
/// surface itself; and outlines to sweep, side by side.
mod hand {
    use std::sync::Arc;

    use workpiece_boolean::Solid;
    use workpiece_boolean::model::{Edge, Face, Use, Vertex, Walk};
    use workpiece_core::math::{Point2, Point3};
    use workpiece_core::world::topo::{Curve2, Curve3, NurbsError, NurbsSurface};
    use workpiece_shape::swept::{Sides, Weighted};

    /// How close what is put together here is to what it stands for:
    /// its curves and surfaces are the very ones.
    const FINE: f64 = 1e-9;

    /// A solid in the making.
    #[derive(Default)]
    pub struct Hand {
        /// What there is of it.
        pub solid: Solid,
    }

    impl Hand {
        /// A vertex at `point`.
        pub fn vertex(&mut self, point: Point3) -> usize {
            self.solid.vertices.push(Vertex {
                point,
                tolerance: FINE,
                name: None,
            });
            self.solid.vertices.len() - 1
        }

        /// An edge all along `curve`, between the vertices `ends`.
        pub fn edge(&mut self, curve: Curve3, ends: [usize; 2]) -> usize {
            self.solid.edges.push(Edge {
                range: curve.domain(),
                curve: Arc::new(curve),
                ends,
                tolerance: FINE,
                name: None,
                also: Vec::new(),
            });
            self.solid.edges.len() - 1
        }

        /// A step along `edge` (or against it, `reversed`) where the
        /// edge runs straight in its face's (u, v), from `from` at its
        /// start to `to` at its end.
        pub fn step(
            &self,
            edge: usize,
            reversed: bool,
            from: Point2,
            to: Point2,
        ) -> Result<Use, NurbsError> {
            let (a, b) = self.solid.edges[edge].range;
            Ok(Use {
                walk: Walk::Edge { edge, reversed },
                pcurve: Arc::new(Curve2::new(1, vec![a, a, b, b], vec![from, to], None)?),
            })
        }

        /// A step that stays at `vertex`, where a side of the face's
        /// surface is that one point, from `from` to `to` in (u, v).
        pub fn pole(&self, vertex: usize, from: Point2, to: Point2) -> Result<Use, NurbsError> {
            let pcurve = Curve2::line(from, to)?;
            Ok(Use {
                walk: Walk::Pole {
                    vertex,
                    range: pcurve.domain(),
                },
                pcurve: Arc::new(pcurve),
            })
        }

        /// A face on `surface`, which faces out of the solid, within
        /// the loop `steps`.
        pub fn face(&mut self, surface: NurbsSurface, steps: Vec<Use>) {
            self.solid.faces.push(Face {
                surface: Arc::new(surface),
                outward: true,
                loops: vec![steps],
                tolerance: FINE,
                name: None,
                also: Vec::new(),
                smooth: Vec::new(),
            });
        }

        /// A flat face within the edges of `ring` (each walked along
        /// itself or against, one after the other with the face on the
        /// left), which lie in a plane along `x` and `y`: it faces out
        /// of the solid along x × y. Each edge's curve in the plane is
        /// its own, laid flat.
        pub fn flat(
            &mut self,
            ring: &[(usize, bool)],
            x: Point3,
            y: Point3,
        ) -> Result<(), NurbsError> {
            let curves: Vec<Arc<Curve3>> = ring
                .iter()
                .map(|r| self.solid.edges[r.0].curve.clone())
                .collect();
            let origin = curves
                .first()
                .and_then(|c| c.control_points().first().copied())
                .unwrap_or(Point3::ZERO);
            let flat = |p: Point3| Point2::new((p - origin).dot(x), (p - origin).dot(y));
            let (mut low, mut high) = (Point2::INFINITY, Point2::NEG_INFINITY);
            for p in curves.iter().flat_map(|c| c.control_points()) {
                (low, high) = (low.min(flat(*p)), high.max(flat(*p)));
            }
            let pad = (high - low).length() * 0.1;
            let (low, high) = (low - Point2::splat(pad), high + Point2::splat(pad));
            let place = |u: f64, v: f64| origin + x * u + y * v;
            let surface = NurbsSurface::new(
                1,
                1,
                vec![low.x, low.x, high.x, high.x],
                vec![low.y, low.y, high.y, high.y],
                2,
                2,
                vec![
                    place(low.x, low.y),
                    place(low.x, high.y),
                    place(high.x, low.y),
                    place(high.x, high.y),
                ],
                None,
            )?;
            let mut steps = Vec::new();
            for (&(edge, reversed), curve) in ring.iter().zip(&curves) {
                let pcurve = Curve2::new(
                    curve.degree(),
                    curve.knots().to_vec(),
                    curve.control_points().iter().map(|p| flat(*p)).collect(),
                    Some(curve.weights().to_vec()),
                )?;
                steps.push(Use {
                    walk: Walk::Edge { edge, reversed },
                    pcurve: Arc::new(pcurve),
                });
            }
            self.face(surface, steps);
            Ok(())
        }
    }

    /// The side of `surface` (its knots clamped) along u where v is
    /// least or (`last`) most, or `along_v` where u is: the curve of
    /// its outermost row of control points.
    fn border(surface: &NurbsSurface, along_v: bool, last: bool) -> Result<Curve3, NurbsError> {
        let (du, dv) = surface.degrees();
        let (ku, kv) = surface.knots();
        let (nu, nv) = surface.counts();
        let weights = surface.weights();
        let (degree, knots, count) = if along_v { (dv, kv, nv) } else { (du, ku, nu) };
        let at = |k: usize| match (along_v, last) {
            (false, false) => (k, 0),
            (false, true) => (k, nv - 1),
            (true, false) => (0, k),
            (true, true) => (nu - 1, k),
        };
        let points = (0..count).map(|k| {
            let (i, j) = at(k);
            surface.control_point(i, j)
        });
        let weights = (0..count).map(|k| {
            let (i, j) = at(k);
            weights[i * nv + j]
        });
        Curve3::new(
            degree,
            knots.to_vec(),
            points.collect(),
            Some(weights.collect()),
        )
    }

    /// The block under `top`: a surface with clamped knots that is a
    /// height over a rectangle of the xy plane, above it everywhere,
    /// its x growing with u alone and its y with v alone. The block's
    /// floor is flat at z = 0 and its four sides are upright, each the
    /// flat region between the floor and a side of `top`.
    pub fn under(top: NurbsSurface) -> Result<Solid, NurbsError> {
        let ((u0, u1), (v0, v1)) = top.domain();
        let mut hand = Hand::default();
        // Around the rectangle from where u and v are least.
        let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)].map(|(u, v)| top.point_at(u, v));
        let feet = corners.map(|p| Point3::new(p.x, p.y, 0.0));
        let t = corners.map(|p| hand.vertex(p));
        let f = feet.map(|p| hand.vertex(p));
        let rim = [
            hand.edge(border(&top, false, false)?, [t[0], t[1]]),
            hand.edge(border(&top, true, true)?, [t[1], t[2]]),
            hand.edge(border(&top, false, true)?, [t[3], t[2]]),
            hand.edge(border(&top, true, false)?, [t[0], t[3]]),
        ];
        let mut up = [0; 4];
        for k in 0..4 {
            up[k] = hand.edge(Curve3::line(feet[k], corners[k])?, [f[k], t[k]]);
        }
        let floor = [
            hand.edge(Curve3::line(feet[0], feet[1])?, [f[0], f[1]]),
            hand.edge(Curve3::line(feet[1], feet[2])?, [f[1], f[2]]),
            hand.edge(Curve3::line(feet[3], feet[2])?, [f[3], f[2]]),
            hand.edge(Curve3::line(feet[0], feet[3])?, [f[0], f[3]]),
        ];
        let uv = Point2::new;
        let steps = vec![
            hand.step(rim[0], false, uv(u0, v0), uv(u1, v0))?,
            hand.step(rim[1], false, uv(u1, v0), uv(u1, v1))?,
            hand.step(rim[2], true, uv(u0, v1), uv(u1, v1))?,
            hand.step(rim[3], true, uv(u0, v0), uv(u0, v1))?,
        ];
        hand.face(top, steps);
        let (x, y, z) = (Point3::X, Point3::Y, Point3::Z);
        hand.flat(
            &[
                (floor[0], false),
                (up[1], false),
                (rim[0], true),
                (up[0], true),
            ],
            x,
            z,
        )?;
        hand.flat(
            &[
                (floor[1], false),
                (up[2], false),
                (rim[1], true),
                (up[1], true),
            ],
            y,
            z,
        )?;
        hand.flat(
            &[
                (floor[2], true),
                (up[3], false),
                (rim[2], false),
                (up[2], true),
            ],
            -x,
            z,
        )?;
        hand.flat(
            &[
                (floor[3], true),
                (up[0], false),
                (rim[3], false),
                (up[3], true),
            ],
            -y,
            z,
        )?;
        hand.flat(
            &[
                (floor[3], false),
                (floor[2], false),
                (floor[1], true),
                (floor[0], true),
            ],
            x,
            -y,
        )?;
        hand.solid.settle();
        Ok(hand.solid)
    }

    /// The unit circle about z as a rational quadratic: its nine
    /// control points with their weights, and its knots.
    fn around() -> ([(Point2, f64); 9], Vec<f64>) {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let c = |x: f64, y: f64, w: f64| (Point2::new(x, y), w);
        let points = [
            c(1.0, 0.0, 1.0),
            c(1.0, 1.0, w),
            c(0.0, 1.0, 1.0),
            c(-1.0, 1.0, w),
            c(-1.0, 0.0, 1.0),
            c(-1.0, -1.0, w),
            c(0.0, -1.0, 1.0),
            c(1.0, -1.0, w),
            c(1.0, 0.0, 1.0),
        ];
        let knots = vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ];
        (points, knots)
    }

    /// The ball of `radius` about the origin as one face: the
    /// biquadratic sphere of 9 × 5 control points, around the z axis
    /// and from the south pole to the north, closed on its seam and at
    /// its two poles.
    pub fn ball(radius: f64) -> Result<Solid, NurbsError> {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let meridian = [
            (0.0, -1.0, 1.0),
            (1.0, -1.0, w),
            (1.0, 0.0, 1.0),
            (1.0, 1.0, w),
            (0.0, 1.0, 1.0),
        ];
        let (circle, knots) = around();
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for (c, wc) in circle {
            for (rho, z, wm) in meridian {
                points.push(Point3::new(rho * c.x, rho * c.y, z) * radius);
                weights.push(wc * wm);
            }
        }
        let surface = NurbsSurface::new(
            2,
            2,
            knots,
            vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            9,
            5,
            points,
            Some(weights),
        )?;
        let mut hand = Hand::default();
        let south = hand.vertex(Point3::new(0.0, 0.0, -radius));
        let north = hand.vertex(Point3::new(0.0, 0.0, radius));
        let seam = hand.edge(border(&surface, true, false)?, [south, north]);
        let uv = Point2::new;
        let steps = vec![
            hand.pole(south, uv(0.0, 0.0), uv(1.0, 0.0))?,
            hand.step(seam, false, uv(1.0, 0.0), uv(1.0, 1.0))?,
            hand.pole(north, uv(1.0, 1.0), uv(0.0, 1.0))?,
            hand.step(seam, true, uv(0.0, 0.0), uv(0.0, 1.0))?,
        ];
        hand.face(surface, steps);
        hand.solid.settle();
        Ok(hand.solid)
    }

    /// The cone on the disk of `radius` about z at z = 0 with its apex
    /// at `height`: its side one face of 9 × 2 control points, around
    /// the axis and up to the apex, where its whole upper side is one
    /// point; and the flat disk under it.
    pub fn cone(radius: f64, height: f64) -> Result<Solid, NurbsError> {
        let (circle, knots) = around();
        let apex = Point3::new(0.0, 0.0, height);
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for (c, w) in circle {
            points.extend([Point3::new(c.x, c.y, 0.0) * radius, apex]);
            weights.extend([w, w]);
        }
        let surface = NurbsSurface::new(
            2,
            1,
            knots,
            vec![0.0, 0.0, 1.0, 1.0],
            9,
            2,
            points,
            Some(weights),
        )?;
        let mut hand = Hand::default();
        let foot = hand.vertex(Point3::new(radius, 0.0, 0.0));
        let tip = hand.vertex(apex);
        let rim = hand.edge(border(&surface, false, false)?, [foot, foot]);
        let seam = hand.edge(border(&surface, true, false)?, [foot, tip]);
        let uv = Point2::new;
        let steps = vec![
            hand.step(rim, false, uv(0.0, 0.0), uv(1.0, 0.0))?,
            hand.step(seam, false, uv(1.0, 0.0), uv(1.0, 1.0))?,
            hand.pole(tip, uv(1.0, 1.0), uv(0.0, 1.0))?,
            hand.step(seam, true, uv(0.0, 0.0), uv(0.0, 1.0))?,
        ];
        hand.face(surface, steps);
        hand.flat(&[(rim, true)], Point3::X, -Point3::Y)?;
        hand.solid.settle();
        Ok(hand.solid)
    }

    /// An outline to sweep, side after side.
    #[derive(Default)]
    pub struct Sided {
        /// Its sides.
        pub sides: Sides,
        /// Their control points.
        pub points: Weighted,
    }

    impl Sided {
        /// A straight side.
        pub fn line(&mut self, from: Point2, to: Point2) {
            self.sides.push((1, vec![0.0, 0.0, 1.0, 1.0], 2));
            self.points.extend([(from, 1.0), (to, 1.0)]);
        }

        /// An arc of less than half the circle of `radius` about
        /// `centre`, between the directions `from` and `to` from the
        /// centre (each of length 1).
        pub fn arc(&mut self, centre: Point2, radius: f64, from: Point2, to: Point2) {
            let cos = from.dot(to);
            self.sides.push((2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 3));
            self.points.extend([
                (centre + from * radius, 1.0),
                (
                    centre + (from + to) * (radius / (1.0 + cos)),
                    ((1.0 + cos) / 2.0).sqrt(),
                ),
                (centre + to * radius, 1.0),
            ]);
        }

        /// A Bézier side through its control points `hull`.
        pub fn bezier(&mut self, hull: &[Point2]) {
            let degree = hull.len().saturating_sub(1);
            let mut knots = vec![0.0; degree + 1];
            knots.extend(std::iter::repeat_n(1.0, degree + 1));
            self.sides.push((degree, knots, hull.len()));
            self.points.extend(hull.iter().map(|p| (*p, 1.0)));
        }
    }
}

/// The solid the outline with these `sides`, whose control points are
/// `at` each share of the way, makes carried straight from `from` to
/// `to` with its y along `up`.
fn carried(
    sides: &workpiece_shape::swept::Sides,
    at: &dyn Fn(f64) -> workpiece_shape::swept::Weighted,
    from: Point3,
    to: Point3,
    up: Point3,
    size: f64,
) -> Built {
    use workpiece_core::world::topo::Curve3;
    use workpiece_shape::swept::{Outline, swept};
    let path = Curve3::line(from, to).map_err(|e| Grade::Crashed(format!("setup: {e}")))?;
    let outline = Outline {
        sides: sides.clone(),
        at,
    };
    swept("carried", &outline, &path, (0.0, 1.0), up, size)
        .map_err(|why| Grade::Refused(format!("{why:#}")))
}

/// What is put together by hand, or how that failed.
fn handmade(made: Result<Solid, workpiece_core::world::topo::NurbsError>) -> Built {
    made.map_err(|e| Grade::Crashed(format!("setup: {e}")))
}

/// The surface over an even or uneven grid: of `degrees` and `knots`
/// along x and y, its control point (i, j) at `xs[i]`, `ys[j]` and the
/// height `heights[i][j]`, with the weight `weights[j]` (the same along
/// x).
fn graph(
    degrees: (usize, usize),
    knots: (Vec<f64>, Vec<f64>),
    xs: &[f64],
    ys: &[f64],
    heights: &dyn Fn(usize, usize) -> f64,
    weights: Option<&[f64]>,
) -> Result<workpiece_core::world::topo::NurbsSurface, Grade> {
    let mut points = Vec::new();
    let mut all = Vec::new();
    for (i, x) in xs.iter().enumerate() {
        for (j, y) in ys.iter().enumerate() {
            points.push(Point3::new(*x, *y, heights(i, j)));
            all.push(weights.map_or(1.0, |w| w[j]));
        }
    }
    workpiece_core::world::topo::NurbsSurface::new(
        degrees.0,
        degrees.1,
        knots.0,
        knots.1,
        xs.len(),
        ys.len(),
        points,
        Some(all),
    )
    .map_err(|e| Grade::Crashed(format!("setup: {e}")))
}

/// The knots of one Bézier piece of `degree`.
fn bezier_knots(degree: usize) -> Vec<f64> {
    let mut knots = vec![0.0; degree + 1];
    knots.extend(std::iter::repeat_n(1.0, degree + 1));
    knots
}

/// The block under the bicubic Bézier dome of brep-bench's nurbs cases.
fn bezier_dome() -> Built {
    let heights = [
        [4.0, 6.0, 7.0, 5.0],
        [6.0, 14.0, 12.0, 7.0],
        [5.0, 13.0, 16.0, 8.0],
        [3.0, 6.0, 8.0, 6.0],
    ];
    let grid: Vec<f64> = (0..4).map(|k| 20.0 * f64::from(k) / 3.0).collect();
    let top = graph(
        (3, 3),
        (bezier_knots(3), bezier_knots(3)),
        &grid,
        &grid,
        &|i, j| heights[i][j],
        None,
    )?;
    handmade(hand::under(top))
}

/// The block on the square of half side 12 under a barrel vault: an
/// arch of radius 15 about the line y = 0, z = 1, as a surface straight
/// along x and one weighted quadratic arc along y.
fn barrel_vault() -> Built {
    let top = graph(
        (1, 2),
        (bezier_knots(1), bezier_knots(2)),
        &[-12.0, 12.0],
        &[-12.0, 0.0, 12.0],
        &|_, j| [10.0, 26.0, 10.0][j],
        Some(&[1.0, 0.6, 1.0]),
    )?;
    handmade(hand::under(top))
}

/// brep-bench's geometry cases: quadrics, tori that close their hole,
/// and faces on given NURBS surfaces.
fn geometry(s: &mut Scene, id: &str) -> Built {
    use crate::scene::Piece;
    use workpiece_core::math::Point2;
    use workpiece_core::world::topo::NurbsSurface;
    use workpiece_shape::swept::circle;
    let far = 100.0;
    // `solid` stretched along x, y and z about the origin.
    let stretched = |solid: &Solid, by: [f64; 3]| {
        let most = by.into_iter().fold(1.0, f64::max);
        handmade(solid.mapped(&|p| p * Point3::from_array(by), false, most))
    };
    let turned = |s: &mut Scene, start: [f64; 2], pieces: &[(Piece, [f64; 2])]| {
        s.outlined([0.0; 3], upright(), start, pieces)
            .map_err(|why| Grade::Refused(format!("{why:#}")))
    };
    // One weighted quadratic piece through the control point `through`.
    let conic = |through: [f64; 2], weight: f64| Piece::Spline {
        between: vec![through],
        degree: 2,
        knots: Vec::new(),
        weights: vec![1.0, weight, 1.0],
    };
    // The periodic uniform quadratic B-spline on the polygon `corners`
    // as its parabolic arcs: each from the middle of a side, by the
    // corner, to the middle of the next side.
    let rounded = |corners: [Point2; 4]| -> [[Point2; 3]; 4] {
        std::array::from_fn(|k| {
            let (before, corner, after) = (corners[(k + 3) % 4], corners[k], corners[(k + 1) % 4]);
            [(before + corner) / 2.0, corner, (corner + after) / 2.0]
        })
    };
    match id {
        "geometry/ellipsoid" | "geometry/ellipsoid-half" => {
            let whole = stretched(&ball(s, [0.0; 3], 1.0, ID)?, [6.0, 4.0, 3.0])?;
            if id == "geometry/ellipsoid" {
                return Ok(whole);
            }
            let below = block(s, [-far, -far, -far], [far, far, 0.0])?;
            apply(Op::Cut, &whole, &below)
        }
        "geometry/elliptic-cylinder" => stretched(
            &rod(s, 2, [0.0, 0.0], 0.0, 10.0, 1.0, 0.0)?,
            [5.0, 3.0, 1.0],
        ),
        "geometry/elliptic-hole" => {
            let whole = block(s, [-12.0, -8.0, 0.0], [12.0, 8.0, 8.0])?;
            let bore = stretched(
                &rod(s, 2, [0.0, 0.0], -1.0, 9.0, 1.0, 0.0)?,
                [6.0, 3.0, 1.0],
            )?;
            apply(Op::Cut, &whole, &bore)
        }
        "geometry/high-degree-bezier-dome" => {
            let grid: Vec<f64> = (0..16).map(|k| 20.0 * f64::from(k) / 15.0).collect();
            let top = graph(
                (15, 15),
                (bezier_knots(15), bezier_knots(15)),
                &grid,
                &grid,
                &|i, j| 4.0 + ((3 * i + 5 * j) % 7) as f64,
                None,
            )?;
            handmade(hand::under(top))
        }
        "geometry/horn-torus" => torus(s, 5.0, 5.0, 0.0),
        "geometry/torus-nearly-horn" => torus(s, 5.0, 4.99, 0.0),
        "geometry/hyperboloid" => {
            // The hyperbola x = 4 cosh t, z = 6 sinh t between sinh t =
            // ±1 is one conic piece: its middle control point where the
            // tangents at its ends meet, weighing cosh of half the way.
            let reach = 4.0 * 2f64.sqrt();
            turned(
                s,
                [0.0, -6.0],
                &[
                    (Piece::Line, [reach, -6.0]),
                    (conic([2.0 * 2f64.sqrt(), 0.0], 2f64.sqrt()), [reach, 6.0]),
                    (Piece::Line, [0.0, 6.0]),
                    (Piece::Line, [0.0, -6.0]),
                ],
            )
        }
        "geometry/hyperboloid-of-two-sheets" => {
            // z = 3 cosh t, x = 4 sinh t from the vertex to cosh t = 2.
            let reach = 4.0 * 3f64.sqrt();
            turned(
                s,
                [0.0, 3.0],
                &[
                    (conic([4.0 / 3f64.sqrt(), 3.0], 1.5f64.sqrt()), [reach, 6.0]),
                    (Piece::Line, [0.0, 6.0]),
                    (Piece::Line, [0.0, 3.0]),
                ],
            )
        }
        "geometry/hyperboloid-ruled" => {
            // Straight from each point of the lower circle to the point
            // a quarter turn on of the upper one.
            let (sides, lower) = circle(Point2::ZERO, 5.0);
            let at = |f: f64| {
                lower
                    .iter()
                    .map(|(p, w)| (p.lerp(p.perp(), f), *w))
                    .collect()
            };
            carried(
                &sides,
                &at,
                Point3::new(0.0, 0.0, -5.0),
                Point3::new(0.0, 0.0, 5.0),
                Point3::Y,
                20.0,
            )
        }
        "geometry/lemon" | "geometry/spindle-torus" => {
            // The arc from the axis out and back to it, around the
            // circle's center on this side of the axis or beyond it.
            let center = if id == "geometry/lemon" { -3.0 } else { 3.0 };
            let arc = Piece::Arc {
                center: [center, 0.0],
                clockwise: false,
            };
            turned(
                s,
                [0.0, -4.0],
                &[(arc, [0.0, 4.0]), (Piece::Line, [0.0, -4.0])],
            )
        }
        "geometry/nurbs-cone" => handmade(hand::cone(5.0, 10.0)),
        "geometry/nurbs-sphere" => handmade(hand::ball(5.0)),
        "geometry/oblate-spheroid" => stretched(&ball(s, [0.0; 3], 6.0, ID)?, [1.0, 1.0, 0.5]),
        "geometry/paraboloid" => paraboloid(s, 8.0, 8.0),
        "geometry/periodic-bspline-prism" => {
            let arcs = rounded([
                Point2::new(10.0, 10.0),
                Point2::new(-10.0, 10.0),
                Point2::new(-10.0, -10.0),
                Point2::new(10.0, -10.0),
            ]);
            let mut outline = hand::Sided::default();
            for arc in &arcs {
                outline.bezier(arc);
            }
            let points = outline.points;
            carried(
                &outline.sides,
                &|_| points.clone(),
                Point3::ZERO,
                Point3::new(0.0, 0.0, 10.0),
                Point3::Y,
                40.0,
            )
        }
        "geometry/periodic-bspline-ring" => {
            let arcs = rounded([
                Point2::new(13.0, 3.0),
                Point2::new(7.0, 3.0),
                Point2::new(7.0, -3.0),
                Point2::new(13.0, -3.0),
            ]);
            let pieces = arcs.map(|[_, corner, to]| {
                let piece = Piece::Spline {
                    between: vec![corner.to_array()],
                    degree: 2,
                    knots: Vec::new(),
                    weights: Vec::new(),
                };
                (piece, to.to_array())
            });
            turned(s, arcs[0][0].to_array(), &pieces)
        }
        "geometry/saddle-drum" | "geometry/saddle-drum-bored" => {
            let saddle = NurbsSurface::bilinear(
                Point3::new(-10.0, -10.0, 22.5),
                Point3::new(10.0, -10.0, -2.5),
                Point3::new(-10.0, 10.0, -2.5),
                Point3::new(10.0, 10.0, 22.5),
            );
            let under = handmade(saddle.and_then(hand::under))?;
            let round = rod(s, 2, [0.0, 0.0], -5.0, 30.0, 8.0, 0.3)?;
            let drum = apply(Op::Intersect, &under, &round)?;
            if id == "geometry/saddle-drum" {
                return Ok(drum);
            }
            let bore = rod(s, 2, [0.0, 0.0], -1.0, 30.0, 3.0, 0.7)?;
            apply(Op::Cut, &drum, &bore)
        }
        other => Err(Grade::Refused(format!("{other}: not made yet"))),
    }
}

/// brep-bench's parts: each put together as its description says, of
/// blocks, cylinders, outlines turned or pushed, and booleans of them.
fn parts(s: &mut Scene, id: &str) -> Built {
    use crate::scene::Piece;
    use workpiece_core::math::Point2;
    match id {
        "parts/flange" => {
            let plate = rod(s, 2, [0.0, 0.0], 0.0, 6.0, 32.0, 0.0)?;
            let hub = rod(s, 2, [0.0, 0.0], 6.0, 16.0, 15.0, 0.0)?;
            let bore = rod(s, 2, [0.0, 0.0], -1.0, 17.0, 8.0, 0.0)?;
            let mut flange = apply(Op::Cut, &apply(Op::Union, &plate, &hub)?, &bore)?;
            for k in 0..6 {
                let (sin, cos) = libm::sincos((f64::from(k) * 60.0_f64).to_radians());
                let drill = rod(s, 2, [24.0 * cos, 24.0 * sin], -1.0, 7.0, 3.0, 0.0)?;
                flange = apply(Op::Cut, &flange, &drill)?;
            }
            Ok(flange)
        }
        "parts/pin-lug" => {
            let plate = block(s, [0.0, -5.0, 0.0], [80.0, 5.0, 40.0])?;
            let pin = rod(s, 1, [60.0, 20.0], -6.0, 6.0, 8.0, 0.0)?;
            apply(Op::Cut, &plate, &pin)
        }
        "parts/pocketed-tray" => {
            let whole = block(s, [-36.0, 0.0, -24.0], [36.0, 16.0, 24.0])?;
            let pocket = block(s, [-33.0, -1.0, -21.0], [33.0, 13.0, 21.0])?;
            let mut tray = apply(Op::Cut, &whole, &pocket)?;
            for (x, z) in [(26.0, 14.0), (-26.0, 14.0), (-26.0, -14.0), (26.0, -14.0)] {
                let post = rod(s, 1, [x, z], 5.0, 13.0, 5.0, 0.0)?;
                let hole = rod(s, 1, [x, z], 4.0, 14.0, 2.0, 0.0)?;
                let standoff = apply(Op::Cut, &post, &hole)?;
                tray = apply(Op::Union, &tray, &standoff)?;
            }
            Ok(tray)
        }
        "parts/shelf-bracket" => {
            // Pushed along y with z up: the section's x is then
            // against the world's.
            let p = |x: f64, z: f64| Point2::new(-x, z);
            let mut outline = hand::Sided::default();
            outline.line(p(0.0, 0.0), p(20.0, 0.0));
            outline.line(p(20.0, 0.0), p(20.0, 92.0));
            outline.arc(p(28.0, 92.0), 8.0, Point2::X, Point2::Y);
            outline.line(p(28.0, 100.0), p(100.0, 100.0));
            outline.line(p(100.0, 100.0), p(100.0, 120.0));
            outline.line(p(100.0, 120.0), p(0.0, 120.0));
            outline.line(p(0.0, 120.0), p(0.0, 0.0));
            let points = outline.points;
            carried(
                &outline.sides,
                &|_| points.clone(),
                Point3::new(0.0, -5.0, 0.0),
                Point3::new(0.0, 5.0, 0.0),
                Point3::Z,
                200.0,
            )
        }
        "parts/star-handle" => {
            let (tip, fillet) = (4.0, 3.0);
            let (reach, waist) = (33.1, 15.9);
            let (height, lean) = (12.0, libm::tan(2f64.to_radians()));
            // Across a side of the first arm, from its tip's circle to
            // the fillet's: the line touches both, between them.
            let first = Point2::new(reach, 0.0);
            let beside = from_angle(std::f64::consts::FRAC_PI_4) * waist;
            let apart = first.distance(beside);
            let across =
                from_angle(-libm::acos((tip + fillet) / apart)).rotate((beside - first) / apart);
            let mirrored = Point2::new(across.y, across.x);
            let mut sides = hand::Sided::default();
            // The outline moved `d` inward: the tips' circles that much
            // smaller and the fillets' that much larger, about the same
            // centers, and the lines between touching them as before.
            let outline = |d: f64| {
                let mut all = hand::Sided::default();
                for k in 0..4 {
                    let turn = from_angle(f64::from(k) * std::f64::consts::FRAC_PI_2);
                    let (n, m) = (turn.rotate(across), turn.rotate(mirrored));
                    let (center, corner) = (turn.rotate(first), turn.rotate(beside));
                    let next = turn.rotate(first).perp();
                    let before = turn.rotate(Point2::new(across.x, -across.y));
                    all.arc(center, tip - d, before, turn);
                    all.arc(center, tip - d, turn, n);
                    all.line(center + n * (tip - d), corner - n * (fillet + d));
                    all.arc(corner, fillet + d, -n, -m);
                    all.line(corner - m * (fillet + d), next + m * (tip - d));
                }
                all
            };
            sides.sides = outline(0.0).sides;
            carried(
                &sides.sides,
                &|f| outline(height * lean * f).points,
                Point3::ZERO,
                Point3::new(0.0, height, 0.0),
                Point3::Z,
                100.0,
            )
        }
        "parts/turned-shaft" => {
            let round = Piece::Arc {
                center: [16.0, 64.0],
                clockwise: true,
            };
            s.outlined(
                [0.0; 3],
                upright(),
                [0.0, 0.0],
                &[
                    (Piece::Line, [20.0, 0.0]),
                    (Piece::Line, [20.0, 60.0]),
                    (Piece::Line, [16.0, 60.0]),
                    (round, [12.0, 64.0]),
                    (Piece::Line, [12.0, 160.0]),
                    (Piece::Line, [0.0, 160.0]),
                    (Piece::Line, [0.0, 0.0]),
                ],
            )
            .map_err(|why| Grade::Refused(format!("{why:#}")))
        }
        "parts/v-belt-pulley" => {
            let hub = rod(s, 2, [0.0, 0.0], -14.0, 14.0, 12.0, 0.0)?;
            let rim = rod(s, 2, [0.0, 0.0], -9.0, 9.0, 28.0, 0.0)?;
            let bore = rod(s, 2, [0.0, 0.0], -15.0, 15.0, 6.0, 0.0)?;
            let groove = setup(s.turned(
                [0.0; 3],
                upright(),
                &[[22.0, 0.0], [29.0, -5.0], [29.0, 5.0]],
                None,
                None,
            ))?;
            let pulley = apply(Op::Cut, &apply(Op::Union, &hub, &rim)?, &bore)?;
            apply(Op::Cut, &pulley, &groove)
        }
        other => Err(Grade::Refused(format!("{other}: not made yet"))),
    }
}

/// brep-bench's nurbs cases: blocks under B-spline surfaces, and
/// booleans of them.
fn nurbs(s: &mut Scene, id: &str) -> Built {
    let far = 100.0;
    match id {
        "nurbs/bezier-dome" => bezier_dome(),
        "nurbs/bspline-dome" => {
            let heights = [
                [3.0, 5.0, 6.0, 5.0, 4.0],
                [5.0, 9.0, 11.0, 8.0, 5.0],
                [6.0, 12.0, 15.0, 10.0, 6.0],
                [4.0, 10.0, 18.0, 9.0, 5.0],
                [5.0, 8.0, 9.0, 7.0, 4.0],
                [2.0, 4.0, 5.0, 4.0, 3.0],
            ];
            let top = graph(
                (3, 3),
                (
                    vec![0.0, 0.0, 0.0, 0.0, 0.4, 0.4, 1.0, 1.0, 1.0, 1.0],
                    vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 1.0, 1.0, 1.0, 1.0],
                ),
                &[0.0, 3.0, 7.0, 11.0, 16.0, 20.0],
                &[0.0, 5.0, 10.0, 15.0, 20.0],
                &|i, j| heights[i][j],
                None,
            )?;
            handmade(hand::under(top))
        }
        "nurbs/drilled-dome" => {
            let drill = rod(s, 2, [12.0, 8.0], -1.0, 30.0, 3.0, 0.0)?;
            apply(Op::Cut, &bezier_dome()?, &drill)
        }
        "nurbs/groin-vault" => {
            let first = barrel_vault()?;
            let second = handmade(first.moved(&|p| Point3::new(-p.y, p.x, p.z)))?;
            apply(Op::Intersect, &first, &second)
        }
        // (Not one of brep-bench's: the same two vaults, the first made
        // by sweeping its arched end. Their common part once came back
        // as one of them whole.)
        "nurbs/groin-vault-swept" => {
            use workpiece_core::math::Point2;
            use workpiece_core::world::topo::Curve3;
            use workpiece_shape::swept::{Outline, steady, swept};
            let line = (1, vec![0.0, 0.0, 1.0, 1.0], 2);
            let arch = (2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 3);
            let sides = vec![line.clone(), line.clone(), arch, line];
            let p = |y: f64, z: f64, w: f64| (Point2::new(y, z), w);
            let points = [
                p(-12.0, 0.0, 1.0),
                p(12.0, 0.0, 1.0),
                p(12.0, 0.0, 1.0),
                p(12.0, 10.0, 1.0),
                p(12.0, 10.0, 1.0),
                p(0.0, 26.0, 0.6),
                p(-12.0, 10.0, 1.0),
                p(-12.0, 10.0, 1.0),
                p(-12.0, 0.0, 1.0),
            ];
            let at = steady(&points);
            let path = Curve3::line(Point3::new(-12.0, 0.0, 0.0), Point3::new(12.0, 0.0, 0.0))
                .map_err(|_| Grade::Crashed("setup: a path".into()))?;
            let first = swept(
                "vault",
                &Outline { sides, at: &at },
                &path,
                (0.0, 1.0),
                Point3::Z,
                60.0,
            )
            .map_err(|why| Grade::Refused(format!("{why:#}")))?;
            let second = handmade(barrel_vault()?.moved(&|p| Point3::new(-p.y, p.x, p.z)))?;
            apply(Op::Intersect, &first, &second)
        }
        "nurbs/sliced-dome" => {
            // What is behind the plane z = 4 + x/2, seen from above it.
            let halves = workpiece_shape::halves(
                &bezier_dome()?,
                Point3::new(0.0, 0.0, 4.0),
                Point3::new(-0.5, 0.0, 1.0),
            );
            halves
                .map(|[_, behind]| behind)
                .map_err(|why| Grade::Refused(format!("{why:#}")))
        }
        "nurbs/vault-tangent-plane" => {
            let above = block(s, [-far, -far, 16.0], [far, far, far])?;
            apply(Op::Cut, &barrel_vault()?, &above)
        }
        other => Err(Grade::Refused(format!("{other}: not made yet"))),
    }
}

/// brep-bench's spline cases: splines pushed, turned and followed.
fn spline(s: &mut Scene, id: &str) -> Built {
    use crate::scene::Piece;
    use workpiece_core::math::Point2;
    use workpiece_core::world::topo::Curve3;
    use workpiece_shape::swept::{Outline, Sides, Weighted, circle, polygon, steady, swept};
    let turned = |s: &mut Scene, start: [f64; 2], pieces: &[(Piece, [f64; 2])]| {
        s.outlined([0.0; 3], upright(), start, pieces)
            .map_err(|why| Grade::Refused(format!("{why:#}")))
    };
    // The outline carried along the cubic Bézier curve of this hull,
    // square to it, with z up.
    let followed = |outline: (Sides, Weighted), hull: [[f64; 3]; 4], size: f64| {
        let path = Curve3::new(
            3,
            bezier_knots(3),
            hull.map(Point3::from_array).to_vec(),
            None,
        )
        .map_err(|e| Grade::Crashed(format!("setup: {e}")))?;
        let (sides, points) = outline;
        let at = steady(&points);
        swept(
            "followed",
            &Outline { sides, at: &at },
            &path,
            path.domain(),
            Point3::Z,
            size,
        )
        .map_err(|why| Grade::Refused(format!("{why:#}")))
    };
    match id {
        "spline/peanut-prism" => {
            let hull = [
                (14.0, 0.0),
                (12.0, 8.0),
                (4.0, 6.0),
                (0.0, -21.0 / 8.0),
                (-4.0, 6.0),
                (-12.0, 8.0),
                (-14.0, 0.0),
                (-12.0, -8.0),
                (-4.0, -6.0),
                (0.0, 21.0 / 8.0),
                (4.0, -6.0),
                (12.0, -8.0),
            ]
            .map(|(x, y)| Point2::new(x, y));
            // Each span of the periodic uniform cubic as a Bézier piece.
            let mut outline = hand::Sided::default();
            for k in 0..hull.len() {
                let p: [Point2; 4] = std::array::from_fn(|i| hull[(k + i) % hull.len()]);
                outline.bezier(&[
                    (p[0] + p[1] * 4.0 + p[2]) / 6.0,
                    (p[1] * 2.0 + p[2]) / 3.0,
                    (p[1] + p[2] * 2.0) / 3.0,
                    (p[1] + p[2] * 4.0 + p[3]) / 6.0,
                ]);
            }
            let points = outline.points;
            carried(
                &outline.sides,
                &|_| points.clone(),
                Point3::ZERO,
                Point3::new(0.0, 0.0, 10.0),
                Point3::Y,
                60.0,
            )
        }
        "spline/pipe-on-planar-cubic" => followed(
            circle(Point2::ZERO, 20.0),
            [
                [0.0, 0.0, 0.0],
                [12.0, -16.0, 0.0],
                [32.0, -16.0, 0.0],
                [44.0, 0.0, 0.0],
            ],
            120.0,
        ),
        "spline/pipe-on-twisted-cubic" => followed(
            circle(Point2::ZERO, 8.0),
            [
                [0.0, 0.0, 0.0],
                [12.0, 16.0, 0.0],
                [26.0, 8.0, 0.0],
                [24.0, 0.0, 16.0],
            ],
            80.0,
        ),
        "spline/square-on-planar-cubic" => followed(
            polygon(&[
                Point2::new(-10.0, -10.0),
                Point2::new(10.0, -10.0),
                Point2::new(10.0, 10.0),
                Point2::new(-10.0, 10.0),
            ]),
            [
                [0.0, 0.0, 0.0],
                [16.0, 12.0, 0.0],
                [18.0, 26.0, 0.0],
                [12.0, 34.0, 0.0],
            ],
            100.0,
        ),
        "spline/rational-cap" => {
            let low = 5.0 * 3f64.sqrt();
            let arc = Piece::Spline {
                between: vec![[2.0 * low, 10.0]],
                degree: 2,
                knots: Vec::new(),
                weights: vec![1.0, 0.5, 1.0],
            };
            turned(
                s,
                [0.0, -5.0],
                &[
                    (Piece::Line, [low, -5.0]),
                    (arc, [0.0, 10.0]),
                    (Piece::Line, [0.0, -5.0]),
                ],
            )
        }
        "spline/rational-spheroid" => {
            let w = std::f64::consts::FRAC_1_SQRT_2;
            let half = Piece::Spline {
                between: vec![[6.0, -10.0], [6.0, 0.0], [6.0, 10.0]],
                degree: 2,
                knots: vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
                weights: vec![1.0, w, 1.0, w, 1.0],
            };
            turned(
                s,
                [0.0, -10.0],
                &[(half, [0.0, 10.0]), (Piece::Line, [0.0, -10.0])],
            )
        }
        "spline/revolved-cubic-dome" => {
            let side = Piece::Spline {
                between: vec![[12.0, 4.0], [9.0, 10.0]],
                degree: 3,
                knots: Vec::new(),
                weights: Vec::new(),
            };
            turned(
                s,
                [0.0, 0.0],
                &[
                    (Piece::Line, [9.0, 0.0]),
                    (side, [0.0, 10.0]),
                    (Piece::Line, [0.0, 0.0]),
                ],
            )
        }
        other => Err(Grade::Refused(format!("{other}: not made yet"))),
    }
}

/// brep-bench's cases, as cases of this suite.
pub fn cases() -> Vec<Case> {
    EXPECTED
        .iter()
        .map(|&(id, volume, area)| Case {
            category: match id.split('/').next() {
                Some("boolean") => "brep-boolean",
                Some("fillet") => "brep-fillet",
                Some("chamfer") => "brep-chamfer",
                Some("offset") => "brep-offset",
                Some("shell") => "brep-shell",
                Some("thicken") => "brep-thicken",
                Some("draft") => "brep-draft",
                Some("primitives") => "brep-primitives",
                Some("sweep") => "brep-sweep",
                Some("loft") => "brep-loft",
                Some("singular") => "brep-singular",
                Some("edit") => "brep-edit",
                Some("feature") => "brep-feature",
                Some("swept") => "brep-swept",
                Some("geometry") => "brep-geometry",
                Some("parts") => "brep-parts",
                Some("nurbs") => "brep-nurbs",
                Some("spline") => "brep-spline",
                _ => "brep-tangent",
            },
            name: id.split('/').nth(1).unwrap_or(id).to_owned(),
            run: Box::new(move || {
                let solid = match build(id) {
                    Ok(solid) => solid,
                    Err(grade) => return grade,
                };
                match sound(&solid) {
                    Err(why) => Grade::Wrong(why),
                    Ok((v, a)) => {
                        let off = |got: f64, wanted: f64| ((got - wanted) / wanted).abs();
                        if off(v, volume) > 1e-6 {
                            Grade::Wrong(format!("volume {v} is not {volume}"))
                        } else if off(a, area) > 1e-6 {
                            Grade::Wrong(format!("area {a} is not {area}"))
                        } else {
                            Grade::Pass
                        }
                    }
                }
            }),
        })
        .collect()
}
