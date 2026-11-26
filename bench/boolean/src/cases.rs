//! The cases with solids built here: closed forms, coincidences,
//! tangencies, near misses, chains and random pairs.

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use workpiece_core::math::{rotation, rotation_x, rotation_y, rotation_z};

use glam::DQuat;
use workpiece_boolean::{Op, Solid, combine, measure};
use workpiece_core::math::Point3;

use crate::scene::{Scene, consistent, expect, sound};
use crate::{Case, Grade};

const ID: DQuat = DQuat::IDENTITY;

fn add(
    out: &mut Vec<Case>,
    category: &'static str,
    name: impl Into<String>,
    run: impl Fn() -> anyhow::Result<Grade> + Send + Sync + 'static,
) {
    out.push(Case {
        category,
        name: name.into(),
        run: Box::new(move || run().unwrap_or_else(|e| Grade::Crashed(format!("setup: {e:#}")))),
    });
}

/// The three booleans of a pair with known common volume: union, common
/// and both differences, each a case.
fn trio(
    out: &mut Vec<Case>,
    category: &'static str,
    name: &str,
    build: impl Fn(&mut Scene) -> anyhow::Result<(Solid, Solid)> + Send + Sync + Clone + 'static,
    common: f64,
) {
    for (what, op, swap) in [
        ("union", Op::Union, false),
        ("common", Op::Intersect, false),
        ("cut", Op::Cut, false),
        ("cut-back", Op::Cut, true),
    ] {
        let build = build.clone();
        add(out, category, format!("{name}/{what}"), move || {
            let (a, b) = build(&mut Scene::new())?;
            let (va, vb) = (measure(&a).volume, measure(&b).volume);
            let (a, b, wanted) = match (op, swap) {
                (Op::Union, _) => (a, b, va + vb - common),
                (Op::Intersect, _) => (a, b, common),
                (Op::Cut, false) => (a, b, va - common),
                (Op::Cut, true) => (b, a, vb - common),
            };
            Ok(expect(op, &a, &b, wanted, None))
        });
    }
}

/// One boolean with its expected volume and area.
fn one(
    out: &mut Vec<Case>,
    category: &'static str,
    name: &str,
    op: Op,
    build: impl Fn(&mut Scene) -> anyhow::Result<(Solid, Solid)> + Send + Sync + 'static,
    volume: f64,
    area: Option<f64>,
) {
    add(out, category, name, move || {
        let (a, b) = build(&mut Scene::new())?;
        Ok(expect(op, &a, &b, volume, area))
    });
}

/// The integral of `f` from `a` to `b` (Simpson, finely).
fn integral(f: impl Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    let n = 200_000;
    let h = (b - a) / n as f64;
    let mut sum = f(a) + f(b);
    for k in 1..n {
        sum += f(a + h * k as f64) * if k % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

/// The area of the part of the unit-radius-`r` circle beyond a chord at
/// distance `d` from its centre.
fn segment(r: f64, d: f64) -> f64 {
    if d >= r {
        0.0
    } else if d <= -r {
        PI * r * r
    } else {
        r * r * libm::acos(d / r) - d * (r * r - d * d).sqrt()
    }
}

/// The area two rectangles have in common: the first along the axes
/// around the origin with half sizes `a`, the second with half sizes
/// `b`, centred at `at` and turned by `angle`.
fn rectangles(a: [f64; 2], b: [f64; 2], at: [f64; 2], angle: f64) -> f64 {
    let (s, c) = libm::sincos(angle);
    let mut polygon: Vec<[f64; 2]> = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
        .iter()
        .map(|k| {
            let (x, y) = (k[0] * b[0], k[1] * b[1]);
            [at[0] + x * c - y * s, at[1] + x * s + y * c]
        })
        .collect();
    // Clipped by each side of the first.
    for (axis, sign, limit) in [
        (0, 1.0, a[0]),
        (0, -1.0, a[0]),
        (1, 1.0, a[1]),
        (1, -1.0, a[1]),
    ] {
        let inside = |p: &[f64; 2]| sign * p[axis] <= limit;
        let mut next = Vec::new();
        for k in 0..polygon.len() {
            let (p, q) = (polygon[k], polygon[(k + 1) % polygon.len()]);
            if inside(&p) {
                next.push(p);
            }
            if inside(&p) != inside(&q) {
                let t = (limit - sign * p[axis]) / (sign * (q[axis] - p[axis]));
                next.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        polygon = next;
    }
    let mut twice = 0.0;
    for k in 0..polygon.len() {
        let (p, q) = (polygon[k], polygon[(k + 1) % polygon.len()]);
        twice += p[0] * q[1] - p[1] * q[0];
    }
    twice.abs() / 2.0
}

fn boxes(out: &mut Vec<Case>) {
    // Two cubes of side 2, the second moved: crossing, flush, touching
    // in a face, an edge and a corner, apart.
    let offsets: [(&str, [f64; 3]); 9] = [
        ("corner", [1.0, 1.0, 1.0]),
        ("side", [0.5, 0.0, 0.0]),
        ("skew", [1.0, 0.3, 0.0]),
        ("generic", [0.7, 0.4, 1.3]),
        ("face-to-face", [2.0, 0.0, 0.0]),
        ("edge-to-edge", [2.0, 2.0, 0.0]),
        ("corner-to-corner", [2.0, 2.0, 2.0]),
        ("apart", [3.0, 0.5, 0.0]),
        ("same", [0.0, 0.0, 0.0]),
    ];
    for (name, d) in offsets {
        let common: f64 = d.iter().map(|x| (2.0 - x.abs()).max(0.0)).product();
        let build =
            move |s: &mut Scene| Ok((s.cube([0.0; 3], [2.0; 3], ID)?, s.cube(d, [2.0; 3], ID)?));
        trio(out, "box", name, build, common);
    }
    // One inside the other, and through it.
    let build = |s: &mut Scene| {
        Ok((
            s.cube([0.0; 3], [4.0; 3], ID)?,
            s.cube([0.2, 0.1, 0.3], [1.0; 3], ID)?,
        ))
    };
    trio(out, "box", "nested", build, 1.0);
    let build = |s: &mut Scene| {
        Ok((
            s.cube([0.0; 3], [4.0, 1.0, 1.0], ID)?,
            s.cube([0.0; 3], [1.0, 4.0, 1.0], ID)?,
        ))
    };
    trio(out, "box", "cross", build, 1.0);
    let build = |s: &mut Scene| {
        Ok((
            s.cube([0.0; 3], [4.0, 4.0, 2.0], ID)?,
            s.cube([0.0; 3], [1.0, 6.0, 1.0], ID)?,
        ))
    };
    trio(out, "box", "tunnel", build, 4.0);
    let build = |s: &mut Scene| {
        Ok((
            s.cube([0.0; 3], [4.0, 4.0, 2.0], ID)?,
            s.cube([0.0, 0.0, 0.5], [2.0, 2.0, 1.0], ID)?,
        ))
    };
    trio(out, "box", "pocket", build, 4.0);
    let build = |s: &mut Scene| {
        Ok((
            s.cube([0.0; 3], [4.0, 4.0, 2.0], ID)?,
            s.cube([2.0, 0.0, 0.0], [2.0, 1.0, 2.0], ID)?,
        ))
    };
    trio(out, "box", "slot", build, 2.0);
    // Turned about z: the common volume is a clipped rectangle's area
    // times the common height.
    for degrees in [7.0_f64, 30.0, 45.0, 60.0, 89.0] {
        let angle = degrees.to_radians();
        let (at, size) = ([0.7, 0.2, 0.5], [2.0, 1.5, 1.0]);
        // The second box is 1 high and all of that is in the first.
        let common = rectangles([1.0, 1.0], [1.0, 0.75], [0.7, 0.2], angle) * 1.0;
        let build = move |s: &mut Scene| {
            Ok((
                s.cube([0.0; 3], [2.0; 3], ID)?,
                s.cube(at, size, rotation_z(angle))?,
            ))
        };
        trio(out, "box", &format!("turned-{degrees}"), build, common);
    }
    // Turned every way: what must hold of any boolean.
    for (k, (axis, angle)) in [
        ([1.0, 1.0, 1.0], 0.7),
        ([1.0, -2.0, 0.5], 1.9),
        ([0.2, 0.3, 1.0], 2.6),
        ([3.0, 1.0, -1.0], 0.4),
    ]
    .into_iter()
    .enumerate()
    {
        add(out, "box", format!("tumbled-{k}"), move || {
            let mut s = Scene::new();
            let turn = rotation(Point3::from_array(axis).normalize(), angle);
            let a = s.cube([0.0; 3], [2.0; 3], ID)?;
            let b = s.cube([0.6, 0.3, 0.4], [1.5, 2.5, 1.0], turn)?;
            Ok(consistent(&a, &b, None))
        });
    }
}

fn plate(s: &mut Scene) -> anyhow::Result<Solid> {
    s.cube([0.0; 3], [4.0, 4.0, 2.0], ID)
}

fn cylinders(out: &mut Vec<Case>) {
    // A plate 4 × 4 × 2 and a cylinder of radius 1.
    let plate_area = 64.0;
    let through = |s: &mut Scene| Ok((plate(s)?, s.cylinder([0.5, 0.0, -2.0], 1.0, 4.0, ID)?));
    one(
        out,
        "cylinder",
        "hole/cut",
        Op::Cut,
        through,
        32.0 - 2.0 * PI,
        Some(plate_area + 2.0 * PI),
    );
    one(
        out,
        "cylinder",
        "hole/common",
        Op::Intersect,
        through,
        2.0 * PI,
        Some(6.0 * PI),
    );
    one(
        out,
        "cylinder",
        "hole/union",
        Op::Union,
        through,
        32.0 + 2.0 * PI,
        Some(plate_area + 4.0 * PI),
    );
    let boss = |s: &mut Scene| Ok((plate(s)?, s.cylinder([0.0, 0.5, 1.0], 1.0, 1.5, ID)?));
    one(
        out,
        "cylinder",
        "boss/union",
        Op::Union,
        boss,
        32.0 + 1.5 * PI,
        Some(plate_area + 3.0 * PI),
    );
    one(
        out,
        "cylinder",
        "boss/cut",
        Op::Cut,
        boss,
        32.0,
        Some(plate_area),
    );
    one(
        out,
        "cylinder",
        "boss/common",
        Op::Intersect,
        boss,
        0.0,
        None,
    );
    let blind = |s: &mut Scene| Ok((plate(s)?, s.cylinder([-0.5, 0.0, 0.0], 1.0, 1.0, ID)?));
    one(
        out,
        "cylinder",
        "blind/cut",
        Op::Cut,
        blind,
        32.0 - PI,
        Some(plate_area + 2.0 * PI),
    );
    one(
        out,
        "cylinder",
        "blind/union",
        Op::Union,
        blind,
        32.0,
        Some(plate_area),
    );
    let sunk = |s: &mut Scene| Ok((plate(s)?, s.cylinder([-0.5, 0.0, 0.25], 1.0, 2.0, ID)?));
    trio(out, "cylinder", "sunk", sunk, 0.75 * PI);
    // Through an edge of the plate, its axis on the side and beyond it.
    for (name, x) in [("notch", 2.0), ("nick", 2.5), ("bite", 1.4)] {
        let build = move |s: &mut Scene| Ok((plate(s)?, s.cylinder([x, 0.0, -2.0], 1.0, 4.0, ID)?));
        trio(
            out,
            "cylinder",
            name,
            build,
            (PI - segment(1.0, 2.0 - x)) * 2.0,
        );
    }
    // The cylinder's seam turned to where the plate's faces cut it.
    for degrees in [0.0_f64, 45.0, 90.0, 133.0, 180.0, 270.0] {
        let turn = rotation_z(degrees.to_radians());
        let build =
            move |s: &mut Scene| Ok((plate(s)?, s.cylinder([1.5, 0.0, -0.5], 1.0, 1.0, turn)?));
        trio(
            out,
            "cylinder",
            &format!("seam-{degrees}"),
            build,
            PI - segment(1.0, 0.5),
        );
    }
    // A hole at a slant: an ellipse in each face.
    for degrees in [20.0_f64, 30.0, 45.0] {
        let tilt = degrees.to_radians();
        let turn = rotation_y(tilt);
        let build = move |s: &mut Scene| {
            let axis = turn * Point3::Z;
            Ok((
                plate(s)?,
                s.cylinder((axis * -5.0).into(), 0.5, 10.0, turn)?,
            ))
        };
        trio(
            out,
            "cylinder",
            &format!("slant-{degrees}"),
            build,
            PI * 0.25 * 2.0 / libm::cos(tilt),
        );
    }
    // Cylinder against cylinder: on one axis, side by side, and across.
    let ring = |s: &mut Scene| {
        Ok((
            s.cylinder([0.0, 0.0, 0.0], 2.0, 2.0, ID)?,
            s.cylinder([0.0, 0.0, -1.0], 1.0, 4.0, ID)?,
        ))
    };
    trio(out, "cylinder", "ring", ring, 2.0 * PI);
    one(
        out,
        "cylinder",
        "ring/area",
        Op::Cut,
        ring,
        6.0 * PI,
        Some(6.0 * PI + 8.0 * PI + 4.0 * PI),
    );
    let lens = 2.0 * (PI / 3.0 - 3.0_f64.sqrt() / 4.0);
    let twins = |s: &mut Scene| {
        Ok((
            s.cylinder([0.0, 0.0, 0.0], 1.0, 4.0, ID)?,
            s.cylinder([1.0, 0.0, 0.0], 1.0, 4.0, ID)?,
        ))
    };
    trio(out, "cylinder", "twins", twins, lens * 4.0);
    let across = rotation_y(FRAC_PI_2);
    let steinmetz = move |s: &mut Scene| {
        Ok((
            s.cylinder([0.0, 0.0, -2.0], 1.0, 4.0, ID)?,
            s.cylinder([-2.0, 0.0, 0.0], 1.0, 4.0, across)?,
        ))
    };
    trio(out, "cylinder", "steinmetz", steinmetz, 16.0 / 3.0);
    for (name, r) in [("crossed-0.6", 0.6_f64), ("crossed-0.9", 0.9)] {
        let common = 8.0
            * integral(
                |y| ((r * r - y * y).max(0.0) * (1.0 - y * y)).sqrt(),
                0.0,
                r,
            );
        let build = move |s: &mut Scene| {
            Ok((
                s.cylinder([0.0, 0.0, -2.0], 1.0, 4.0, ID)?,
                s.cylinder([-2.0, 0.0, 0.0], r, 4.0, across)?,
            ))
        };
        trio(out, "cylinder", name, build, common);
    }
    // Across and off-centre, and at an angle: no closed form.
    for (k, (offset, angle)) in [(0.3, FRAC_PI_2), (0.0, 1.0), (0.5, 0.6), (1.2, 1.3)]
        .into_iter()
        .enumerate()
    {
        add(out, "cylinder", format!("skew-{k}"), move || {
            let mut s = Scene::new();
            let turn = rotation_y(angle);
            let a = s.cylinder([0.0, 0.0, -2.0], 1.0, 4.0, ID)?;
            let start = Point3::new(0.0, offset, 0.0) - turn * Point3::Z * 3.0;
            let b = s.cylinder(start.into(), 0.7, 6.0, turn)?;
            Ok(consistent(&a, &b, None))
        });
    }
}

fn cap(r: f64, h: f64) -> f64 {
    PI * h * h * (3.0 * r - h) / 3.0
}

fn ball_volume(r: f64) -> f64 {
    4.0 / 3.0 * PI * r.powi(3)
}

fn spheres(out: &mut Vec<Case>) {
    // A ball through a plate, its poles turned every way.
    let turns = [
        ("upright", ID),
        ("on-its-side", rotation_x(FRAC_PI_2)),
        (
            "leaning",
            rotation(Point3::new(1.0, 2.0, 0.5).normalize(), 0.9),
        ),
    ];
    for (name, turn) in turns {
        let build = move |s: &mut Scene| Ok((plate(s)?, s.ball([0.0; 3], 1.5, turn)?));
        trio(
            out,
            "sphere",
            &format!("through-plate-{name}"),
            build,
            ball_volume(1.5) - 2.0 * cap(1.5, 0.5),
        );
        // Its centre on a face, on an edge, on a corner of a big box.
        for (place, at, part) in [
            ("face", [3.0, 0.0, 0.0], 0.5),
            ("edge", [3.0, 3.0, 0.0], 0.25),
            ("corner", [3.0, 3.0, 3.0], 0.125),
        ] {
            let build =
                move |s: &mut Scene| Ok((s.cube([0.0; 3], [6.0; 3], ID)?, s.ball(at, 1.0, turn)?));
            trio(
                out,
                "sphere",
                &format!("on-{place}-{name}"),
                build,
                ball_volume(1.0) * part,
            );
        }
    }
    let build = |s: &mut Scene| {
        Ok((
            s.cube([0.0; 3], [4.0; 3], ID)?,
            s.ball([0.3, 0.1, -0.2], 1.0, ID)?,
        ))
    };
    trio(out, "sphere", "inside-box", build, ball_volume(1.0));
    let build = |s: &mut Scene| {
        Ok((
            s.ball([0.0; 3], 3.0, ID)?,
            s.cube([0.3, 0.1, -0.2], [1.0; 3], ID)?,
        ))
    };
    trio(out, "sphere", "box-inside", build, 1.0);
    // A cap cut off by a plane that misses the seam: a closed curve no
    // edge reaches.
    for (name, z) in [("cap-high", 1.3), ("cap-low", 1.1), ("cap-deep", 0.2)] {
        let build = move |s: &mut Scene| {
            Ok((
                s.ball([0.0; 3], 1.5, ID)?,
                s.cube([0.0, 0.0, z + 1.0], [6.0, 6.0, 2.0], ID)?,
            ))
        };
        trio(out, "sphere", name, build, cap(1.5, 1.5 - z).max(0.0));
    }
    let build = |s: &mut Scene| {
        Ok((
            s.ball([0.0; 3], 1.5, ID)?,
            s.cube([2.5, 0.0, 0.0], [3.0, 6.0, 6.0], ID)?,
        ))
    };
    trio(out, "sphere", "cap-side", build, cap(1.5, 0.5));
    // Ball against ball.
    let lenses: [(&str, f64, (f64, f64)); 3] = [
        ("lens", 1.5, (1.0, 1.0)),
        ("lens-unequal", 1.2, (1.0, 0.6)),
        ("lens-thin", 1.9, (1.0, 1.0)),
    ];
    for (name, d, (r1, r2)) in lenses {
        let common =
            PI * (r1 + r2 - d).powi(2) * (d * d + 2.0 * d * (r1 + r2) - 3.0 * (r1 - r2).powi(2))
                / (12.0 * d);
        let turn = rotation_y(0.7);
        let build = move |s: &mut Scene| {
            Ok((
                s.ball([0.0; 3], r1, ID)?,
                s.ball([d * 0.6, d * 0.8, 0.0], r2, turn)?,
            ))
        };
        trio(out, "sphere", name, build, common);
    }
    // A hole through a ball, along its poles and across them.
    for (name, turn) in [("bored", ID), ("bored-across", rotation_x(FRAC_PI_2))] {
        let r = 0.5_f64;
        let common = ball_volume(1.5) - 4.0 / 3.0 * PI * libm::pow(2.25 - r * r, 1.5);
        let build = move |s: &mut Scene| {
            let start = turn * Point3::Z * -3.0;
            Ok((
                s.ball([0.0; 3], 1.5, ID)?,
                s.cylinder(start.into(), r, 6.0, turn)?,
            ))
        };
        trio(out, "sphere", name, build, common);
    }
}

fn turned(out: &mut Vec<Case>) {
    // A cone of radius 1 and height 2 along y, cut by a slab.
    let cone = [[0.0, 0.0], [1.0, 0.0], [0.0, 2.0]];
    for (name, y) in [("cone-tip", 1.5), ("cone-half", 1.0), ("cone-foot", 0.25)] {
        // Above y: a cone of height 2 − y.
        let top = PI * (1.0 - y / 2.0_f64).powi(2) * (2.0 - y) / 3.0;
        let build = move |s: &mut Scene| {
            Ok((
                s.turned([0.0; 3], ID, &cone, None, None)?,
                s.cube([0.0, y + 2.0, 0.0], [4.0, 4.0, 4.0], ID)?,
            ))
        };
        trio(out, "turned", name, build, top);
    }
    // Bored along its axis: a cylinder below, a cone above.
    let build = move |s: &mut Scene| {
        Ok((
            s.turned([0.0; 3], ID, &cone, None, None)?,
            s.cylinder([0.0, -1.0, 0.0], 0.5, 4.0, rotation_x(-FRAC_PI_2))?,
        ))
    };
    trio(
        out,
        "turned",
        "cone-bored",
        build,
        PI * 0.25 + PI * 0.25 / 3.0,
    );
    // Split by a plane through its axis, and by one beside it.
    let build = move |s: &mut Scene| {
        Ok((
            s.turned([0.0; 3], ID, &cone, None, None)?,
            s.cube([2.0, 1.0, 0.0], [4.0, 4.0, 4.0], ID)?,
        ))
    };
    trio(out, "turned", "cone-split", build, PI / 3.0);
    // A ring: the circle of radius 0.5 around (2, 0), turned.
    let ring = Some(([2.0, 0.0], 0.5));
    let volume = TAU * 2.0 * PI * 0.25;
    let build = move |s: &mut Scene| {
        Ok((
            s.turned([0.0; 3], ID, &[], ring, None)?,
            s.cube([3.0, 0.0, 0.0], [6.0, 2.0, 6.0], ID)?,
        ))
    };
    trio(out, "turned", "ring-halved", build, volume / 2.0);
    let build = move |s: &mut Scene| {
        Ok((
            s.turned([0.0; 3], ID, &[], ring, None)?,
            s.cube([0.0, 1.0, 0.0], [6.0, 2.0, 6.0], ID)?,
        ))
    };
    trio(out, "turned", "ring-sliced", build, volume / 2.0);
    for (k, at) in [[2.0, 0.0, 0.0], [2.3, 0.2, 0.1], [1.5, 0.0, 0.0]]
        .into_iter()
        .enumerate()
    {
        add(out, "turned", format!("ring-ball-{k}"), move || {
            let mut s = Scene::new();
            let a = s.turned([0.0; 3], ID, &[], ring, None)?;
            let b = s.ball(at, 0.8, ID)?;
            Ok(consistent(&a, &b, None))
        });
    }
    add(out, "turned", "cone-ring", move || {
        let mut s = Scene::new();
        let a = s.turned([0.0; 3], ID, &[], ring, None)?;
        let b = s.turned([1.5, -1.0, 0.0], ID, &cone, None, None)?;
        Ok(consistent(&a, &b, None))
    });
}

fn coincident(out: &mut Vec<Case>) {
    // Boxes stacked and side by side, their faces in one plane.
    let pairs: [(&str, [f64; 3], [f64; 3], f64); 5] = [
        ("stacked", [0.0, 0.0, 2.0], [4.0, 4.0, 2.0], 0.0),
        ("stacked-smaller", [1.0, 0.0, 2.0], [2.0, 4.0, 2.0], 0.0),
        ("stacked-inside", [0.5, 0.5, 2.0], [1.0, 1.0, 2.0], 0.0),
        ("flush-side", [1.0, 0.0, 0.5], [2.0, 2.0, 2.0], 6.0),
        ("flush-three", [1.0, 1.0, 0.5], [2.0, 2.0, 1.0], 4.0),
    ];
    for (name, at, size, common) in pairs {
        let build = move |s: &mut Scene| Ok((plate(s)?, s.cube(at, size, ID)?));
        trio(out, "coincident", name, build, common);
    }
    // The same box, a quarter turn on.
    let build = |s: &mut Scene| {
        Ok((
            s.cube([0.0; 3], [2.0; 3], ID)?,
            s.cube([0.0; 3], [2.0; 3], rotation_z(FRAC_PI_2))?,
        ))
    };
    trio(out, "coincident", "same-turned", build, 8.0);
    // Cylinders on one axis with one radius: end to end, overlapping,
    // and with their seams apart.
    for (name, z, degrees, common) in [
        ("rods-end-to-end", 2.0, 0.0_f64, 0.0),
        ("rods-overlapping", 1.0, 0.0, PI),
        ("rods-seams-apart", 1.0, 77.0, PI),
        ("rods-same", 0.0, 123.0, 2.0 * PI),
    ] {
        let turn = rotation_z(degrees.to_radians());
        let build = move |s: &mut Scene| {
            Ok((
                s.cylinder([0.0; 3], 1.0, 2.0, ID)?,
                s.cylinder([0.0, 0.0, z], 1.0, 2.0, turn)?,
            ))
        };
        trio(out, "coincident", name, build, common);
    }
    // A rod in a hole of its own size, and a plug half in it.
    let build = |s: &mut Scene| {
        Ok((
            s.cylinder([0.0, 0.0, 0.0], 2.0, 2.0, ID)?,
            s.cylinder([0.0, 0.0, 1.0], 2.0, 3.0, rotation_z(0.4))?,
        ))
    };
    trio(out, "coincident", "rod-longer", build, 4.0 * PI);
    // The same ball twice, the second with its poles elsewhere.
    for (name, turn) in [("ball-same", ID), ("ball-turned", rotation_x(1.1))] {
        let build =
            move |s: &mut Scene| Ok((s.ball([0.0; 3], 1.0, ID)?, s.ball([0.0; 3], 1.0, turn)?));
        trio(out, "coincident", name, build, ball_volume(1.0));
    }
}

/// What builds the two solids of a case.
type Build = Box<dyn Fn(&mut Scene) -> anyhow::Result<(Solid, Solid)> + Send + Sync>;

fn tangent(out: &mut Vec<Case>) {
    // Solids that touch without crossing: nothing in common.
    let touching: Vec<(&str, Build)> = vec![
        (
            "ball-on-plate",
            Box::new(|s| Ok((plate(s)?, s.ball([0.0, 0.0, 2.0], 1.0, ID)?))),
        ),
        (
            "ball-on-ball",
            Box::new(|s| {
                Ok((
                    s.ball([0.0; 3], 1.0, ID)?,
                    s.ball([2.0, 0.0, 0.0], 1.0, ID)?,
                ))
            }),
        ),
        (
            "ball-on-ball-askew",
            Box::new(|s| {
                Ok((
                    s.ball([0.0; 3], 1.0, ID)?,
                    s.ball([1.2, 1.6, 0.0], 1.0, rotation_y(0.5))?,
                ))
            }),
        ),
        (
            "rod-on-plate",
            Box::new(|s| {
                Ok((
                    plate(s)?,
                    s.cylinder([-1.0, 0.0, 2.0], 1.0, 2.0, rotation_y(FRAC_PI_2))?,
                ))
            }),
        ),
        (
            "rod-beside-rod",
            Box::new(|s| {
                Ok((
                    s.cylinder([0.0; 3], 1.0, 2.0, ID)?,
                    s.cylinder([2.0, 0.0, 0.5], 1.0, 2.0, ID)?,
                ))
            }),
        ),
        (
            "rod-across-rod",
            Box::new(|s| {
                Ok((
                    s.cylinder([0.0, 0.0, -2.0], 1.0, 4.0, ID)?,
                    s.cylinder([-2.0, 2.0, 0.0], 1.0, 4.0, rotation_y(FRAC_PI_2))?,
                ))
            }),
        ),
        (
            "ball-on-rod",
            Box::new(|s| {
                Ok((
                    s.cylinder([0.0; 3], 1.0, 2.0, ID)?,
                    s.ball([2.0, 0.0, 1.0], 1.0, ID)?,
                ))
            }),
        ),
        (
            "ball-on-edge",
            Box::new(|s| {
                Ok((
                    plate(s)?,
                    s.ball([2.0 + 0.5_f64.sqrt(), 0.0, 1.0 + 0.5_f64.sqrt()], 1.0, ID)?,
                ))
            }),
        ),
    ];
    for (name, build) in touching {
        let build = std::sync::Arc::new(build);
        for (what, op) in [
            ("union", Op::Union),
            ("common", Op::Intersect),
            ("cut", Op::Cut),
        ] {
            let build = build.clone();
            add(out, "tangent", format!("{name}/{what}"), move || {
                let (a, b) = build(&mut Scene::new())?;
                let (va, vb) = (measure(&a).volume, measure(&b).volume);
                let wanted = match op {
                    Op::Union => va + vb,
                    Op::Intersect => 0.0,
                    Op::Cut => va,
                };
                Ok(expect(op, &a, &b, wanted, None))
            });
        }
    }
    // One inside the other, touching it from within.
    let inside: Vec<(&str, f64, Build)> = vec![
        (
            "ball-in-box",
            ball_volume(1.0),
            Box::new(|s| {
                Ok((
                    s.cube([0.0; 3], [4.0, 4.0, 2.0], ID)?,
                    s.ball([0.0; 3], 1.0, ID)?,
                ))
            }),
        ),
        (
            "rod-in-box",
            2.0 * PI,
            Box::new(|s| Ok((plate(s)?, s.cylinder([1.0, 0.0, -1.0], 1.0, 2.0, ID)?))),
        ),
        (
            "ball-in-rod",
            ball_volume(1.0),
            Box::new(|s| {
                Ok((
                    s.cylinder([0.0, 0.0, -2.0], 1.0, 4.0, ID)?,
                    s.ball([0.0; 3], 1.0, ID)?,
                ))
            }),
        ),
        (
            "ball-in-ball",
            ball_volume(0.5),
            Box::new(|s| {
                Ok((
                    s.ball([0.0; 3], 1.0, ID)?,
                    s.ball([0.5, 0.0, 0.0], 0.5, ID)?,
                ))
            }),
        ),
    ];
    for (name, common, build) in inside {
        let build = std::sync::Arc::new(build);
        trio(out, "tangent", name, move |s: &mut Scene| build(s), common);
    }
}

fn near_misses(out: &mut Vec<Case>) {
    // Faces brought within fractions and multiples of the tolerance of
    // each other.
    let tolerance = 1e-7;
    let steps = [
        0.0, 0.25, -0.25, 0.9, -0.9, 1.9, -1.9, 4.0, -4.0, 16.0, -16.0, 256.0, -256.0, 1e4, -1e4,
    ];
    for step in steps {
        let gap: f64 = step * tolerance;
        let sunk = (-gap).max(0.0);
        // A box on top, lifted or sunk by the gap.
        let lifted = move |s: &mut Scene| {
            Ok((
                plate(s)?,
                s.cube([0.0, 0.0, 2.0 + gap], [2.0, 2.0, 2.0], ID)?,
            ))
        };
        one(
            out,
            "nearmiss",
            &format!("lifted/union/{step}"),
            Op::Union,
            lifted,
            40.0 - 4.0 * sunk,
            None,
        );
        one(
            out,
            "nearmiss",
            &format!("lifted/cut/{step}"),
            Op::Cut,
            lifted,
            32.0 - 4.0 * sunk,
            None,
        );
        // A side flush but for the gap.
        let flush = move |s: &mut Scene| {
            Ok((
                plate(s)?,
                s.cube([1.0 + gap, 0.0, 1.5], [2.0, 2.0, 2.0], ID)?,
            ))
        };
        one(
            out,
            "nearmiss",
            &format!("flush/union/{step}"),
            Op::Union,
            flush,
            38.0 + gap.max(0.0),
            None,
        );
        one(
            out,
            "nearmiss",
            &format!("flush/cut/{step}"),
            Op::Cut,
            flush,
            30.0 + gap.max(0.0),
            None,
        );
        // A cylinder and a ball standing on top.
        let boss =
            move |s: &mut Scene| Ok((plate(s)?, s.cylinder([0.0, 0.0, 1.0 + gap], 1.0, 1.0, ID)?));
        one(
            out,
            "nearmiss",
            &format!("boss/union/{step}"),
            Op::Union,
            boss,
            32.0 + PI * (1.0 - sunk),
            None,
        );
        let resting = move |s: &mut Scene| Ok((plate(s)?, s.ball([0.0, 0.0, 2.0 + gap], 1.0, ID)?));
        one(
            out,
            "nearmiss",
            &format!("resting/union/{step}"),
            Op::Union,
            resting,
            32.0 + ball_volume(1.0) - cap(1.0, sunk),
            None,
        );
        // A hole whose wall is the gap from the plate's side.
        let edge =
            move |s: &mut Scene| Ok((plate(s)?, s.cylinder([1.0 + gap, 0.0, -2.0], 1.0, 4.0, ID)?));
        one(
            out,
            "nearmiss",
            &format!("edge/cut/{step}"),
            Op::Cut,
            edge,
            32.0 - (PI - segment(1.0, 1.0 - gap)) * 2.0,
            None,
        );
        // Rods end to end, and one a hair thicker than the other.
        let rods = move |s: &mut Scene| {
            Ok((
                s.cylinder([0.0; 3], 1.0, 2.0, ID)?,
                s.cylinder([0.0, 0.0, 2.0 + gap], 1.0, 2.0, ID)?,
            ))
        };
        one(
            out,
            "nearmiss",
            &format!("rods/union/{step}"),
            Op::Union,
            rods,
            PI * (4.0 - sunk),
            None,
        );
        let sleeve = move |s: &mut Scene| {
            Ok((
                s.cylinder([0.0; 3], 1.0, 2.0, ID)?,
                s.cylinder([0.0, 0.0, 1.0], 1.0 + gap, 2.0, ID)?,
            ))
        };
        let r = 1.0 + gap;
        one(
            out,
            "nearmiss",
            &format!("sleeve/union/{step}"),
            Op::Union,
            sleeve,
            PI * (2.0 + 2.0 * r * r - r.min(1.0).powi(2)),
            None,
        );
    }
}

fn chains(out: &mut Vec<Case>) {
    // A plate with rows of holes, drilled one after another.
    for n in [2_usize, 4] {
        add(out, "chain", format!("holes-{n}x{n}"), move || {
            let mut s = Scene::new();
            let mut body = s.cube([0.0; 3], [8.0, 8.0, 1.0], ID)?;
            let (r, pitch) = (0.4, 8.0 / n as f64);
            for i in 0..n {
                for j in 0..n {
                    let at = [
                        -4.0 + pitch * (i as f64 + 0.5),
                        -4.0 + pitch * (j as f64 + 0.5),
                        -1.0,
                    ];
                    let drill = s.cylinder(at, r, 2.0, rotation_z((i * n + j) as f64))?;
                    body = match combine(Op::Cut, &body, &drill) {
                        Ok(o) => o.solid,
                        Err(why) => return Ok(Grade::Refused(format!("hole {i},{j}: {why}"))),
                    };
                }
            }
            Ok(grade(&body, 64.0 - (n * n) as f64 * PI * r * r, 64.0))
        });
    }
    // A wheel with slots cut into its rim.
    for teeth in [6_usize, 16] {
        add(out, "chain", format!("slots-{teeth}"), move || {
            let mut s = Scene::new();
            let (radius, inner, width): (f64, f64, f64) = (3.0, 2.2, 0.5);
            let mut body = s.cylinder([0.0, 0.0, 0.0], radius, 1.0, ID)?;
            for k in 0..teeth {
                let turn = rotation_z(TAU * k as f64 / teeth as f64 + 0.1);
                let at = turn * Point3::new(inner + 1.5, 0.0, 0.5);
                let slot = s.cube(at.into(), [3.0, width, 2.0], turn)?;
                body = match combine(Op::Cut, &body, &slot) {
                    Ok(o) => o.solid,
                    Err(why) => return Ok(Grade::Refused(format!("slot {k}: {why}"))),
                };
            }
            // Between the slot's floor and the rim.
            let h = width / 2.0;
            let lost = 2.0
                * (h / 2.0 * (radius * radius - h * h).sqrt()
                    + radius * radius / 2.0 * libm::asin(h / radius))
                - inner * width;
            Ok(grade(
                &body,
                PI * radius * radius - teeth as f64 * lost,
                PI * radius * radius,
            ))
        });
    }
    // Steps joined one after another, and taken away again.
    add(out, "chain", "stairs", || {
        let mut s = Scene::new();
        let mut body = s.cube([0.0; 3], [2.0, 2.0, 1.0], ID)?;
        for k in 1..6 {
            let step = s.cube([k as f64, 0.0, k as f64 * 0.5], [2.0, 2.0, 1.0], ID)?;
            body = match combine(Op::Union, &body, &step) {
                Ok(o) => o.solid,
                Err(why) => return Ok(Grade::Refused(format!("step {k}: {why}"))),
            };
        }
        Ok(grade(&body, 6.0 * 4.0 - 5.0 * 1.0, 24.0))
    });
    add(out, "chain", "joined-and-cut-again", || {
        let mut s = Scene::new();
        let a = s.cube([0.0; 3], [4.0, 4.0, 2.0], ID)?;
        let b = s.cylinder([1.5, 0.5, -0.5], 1.0, 3.0, ID)?;
        let joined = match combine(Op::Union, &a, &b) {
            Ok(o) => o.solid,
            Err(why) => return Ok(Grade::Refused(format!("union: {why}"))),
        };
        // The rod is 1.5 deep in the plate and half out of its side.
        Ok(expect(
            Op::Cut,
            &joined,
            &b,
            32.0 - (PI - segment(1.0, 0.5)) * 1.5,
            None,
        ))
    });
    add(out, "chain", "ball-bored-thrice", || {
        let mut s = Scene::new();
        let mut body = s.ball([0.0; 3], 2.0, ID)?;
        let r = 0.5_f64;
        for (k, turn) in [ID, rotation_x(FRAC_PI_2), rotation_y(FRAC_PI_2)]
            .into_iter()
            .enumerate()
        {
            let start = turn * Point3::Z * -4.0;
            let drill = s.cylinder(start.into(), r, 8.0, turn)?;
            body = match combine(Op::Cut, &body, &drill) {
                Ok(o) => o.solid,
                Err(why) => return Ok(Grade::Refused(format!("bore {k}: {why}"))),
            };
        }
        // No closed form for three bores; the first alone is known,
        // and each more can only take away.
        match sound(&body) {
            Err(why) => Ok(Grade::Wrong(why)),
            Ok((v, _)) => {
                let once = 4.0 / 3.0 * PI * libm::pow(4.0 - r * r, 1.5);
                let least = ball_volume(2.0) - 3.0 * (ball_volume(2.0) - once);
                if v < once && v > least {
                    Ok(Grade::Pass)
                } else {
                    Ok(Grade::Wrong(format!(
                        "volume {v} is not between {least} and {once}"
                    )))
                }
            }
        }
    });
}

fn grade(solid: &Solid, volume: f64, scale: f64) -> Grade {
    match sound(solid) {
        Err(why) => Grade::Wrong(why),
        Ok((v, _)) if (v - volume).abs() <= crate::scene::CLOSE * scale => Grade::Pass,
        Ok((v, _)) => Grade::Wrong(format!("volume {v} is not {volume}")),
    }
}

/// A simple shape that says of a point whether it is in it.
#[derive(Clone, Copy)]
enum Shape {
    Box {
        at: Point3,
        size: Point3,
        turn: DQuat,
    },
    Rod {
        base: Point3,
        radius: f64,
        height: f64,
        turn: DQuat,
    },
    Ball {
        at: Point3,
        radius: f64,
        turn: DQuat,
    },
}

impl Shape {
    fn holds(&self, p: Point3) -> bool {
        match *self {
            Shape::Box { at, size, turn } => {
                let q = turn.inverse() * (p - at);
                q.abs().cmple(size / 2.0).all()
            }
            Shape::Rod {
                base,
                radius,
                height,
                turn,
            } => {
                let q = turn.inverse() * (p - base);
                q.z >= 0.0 && q.z <= height && libm::hypot(q.x, q.y) <= radius
            }
            Shape::Ball { at, radius, .. } => p.distance(at) <= radius,
        }
    }

    fn build(&self, s: &mut Scene) -> anyhow::Result<Solid> {
        match *self {
            Shape::Box { at, size, turn } => s.cube(at.into(), size.into(), turn),
            Shape::Rod {
                base,
                radius,
                height,
                turn,
            } => s.cylinder(base.into(), radius, height, turn),
            Shape::Ball { at, radius, turn } => s.ball(at.into(), radius, turn),
        }
    }
}

/// A source of numbers that is the same every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn between(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }

    fn turn(&mut self) -> DQuat {
        let axis = Point3::new(
            self.between(-1.0, 1.0),
            self.between(-1.0, 1.0),
            self.between(-1.0, 1.0),
        );
        rotation(
            axis.try_normalize().unwrap_or(Point3::Z),
            self.between(0.0, TAU),
        )
    }

    fn shape(&mut self, spread: f64) -> Shape {
        let at = Point3::new(
            self.between(-spread, spread),
            self.between(-spread, spread),
            self.between(-spread, spread),
        );
        match (self.next() * 3.0) as usize {
            0 => Shape::Box {
                at,
                size: Point3::new(
                    self.between(0.8, 2.5),
                    self.between(0.8, 2.5),
                    self.between(0.8, 2.5),
                ),
                turn: self.turn(),
            },
            1 => Shape::Rod {
                base: at,
                radius: self.between(0.4, 1.2),
                height: self.between(1.0, 3.0),
                turn: self.turn(),
            },
            _ => Shape::Ball {
                at,
                radius: self.between(0.5, 1.4),
                turn: self.turn(),
            },
        }
    }
}

fn random(out: &mut Vec<Case>) {
    for k in 0..80_u64 {
        add(out, "random", format!("pair-{k:02}"), move || {
            let mut rng =
                Random(0x9E37_79B9_7F4A_7C15 ^ (k + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
            let (a, b) = (rng.shape(0.0), rng.shape(1.2));
            let mut s = Scene::new();
            let (sa, sb) = (a.build(&mut s)?, b.build(&mut s)?);
            // The common volume, by counting points of a box around
            // both.
            let (n, half) = (400_000, 4.5);
            let mut hits = 0_usize;
            for _ in 0..n {
                let p = Point3::new(
                    rng.between(-half, half),
                    rng.between(-half, half),
                    rng.between(-half, half),
                );
                hits += usize::from(a.holds(p) && b.holds(p));
            }
            let all = (2.0 * half).powi(3);
            let p = hits as f64 / n as f64;
            let within = 6.0 * all * (p * (1.0 - p) / n as f64).sqrt() + all * 2e-5;
            Ok(consistent(&sa, &sb, Some((p * all, within))))
        });
    }
}

/// Every case built here.
pub fn all() -> Vec<Case> {
    let mut out = Vec::new();
    boxes(&mut out);
    cylinders(&mut out);
    spheres(&mut out);
    turned(&mut out);
    coincident(&mut out);
    tangent(&mut out);
    near_misses(&mut out);
    chains(&mut out);
    random(&mut out);
    out
}
