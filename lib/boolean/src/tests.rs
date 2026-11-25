//! A few quick checks that booleans work at all. The thorough ones —
//! closed forms, near misses, random pairs, public STEP parts — are the
//! benchmark suite's (`bench/boolean`), run when this crate changes.

use workpiece_core::math::Point3;
use workpiece_core::world::topo::{BodyId, Topo};
use workpiece_core::world::{Datum, DatumFrame};
use workpiece_core::{Measure, Operation, Pose, Reference, Standalone};
use workpiece_ops_basic_shapes::{
    AddBox, AddBoxArgs, AddCylinder, AddCylinderArgs, AddSphere, AddSphereArgs,
};

use crate::{Op, Refusal, combine, read, write};

/// A world to put shapes in, each on a frame of its own.
struct Bench(Standalone);

impl Bench {
    fn new() -> Self {
        Self(Standalone::new())
    }

    fn frame(&mut self, id: &str, at: Point3) -> Reference<DatumFrame> {
        let name = format!("{id}-frame");
        let frame = DatumFrame::from_pose(Pose::from_translation(at)).expect("a frame");
        self.0
            .world
            .datums
            .insert(name.clone(), Datum::Frame(frame));
        Reference::datum(name)
    }

    /// A box centred at `at`.
    fn cube(&mut self, id: &str, at: [f64; 3], size: [f64; 3]) -> BodyId {
        let frame = Some(self.frame(id, Point3::from_array(at)));
        let args = AddBoxArgs {
            frame,
            size: size.map(Measure::new),
        };
        let op = AddBox {
            id: id.into(),
            args,
        };
        op.apply(&mut self.0).expect("a box");
        self.0.world.topo.resolve(id).expect("its body")
    }

    /// A cylinder standing on `at`.
    fn cylinder(&mut self, id: &str, at: [f64; 3], radius: f64, height: f64) -> BodyId {
        let frame = Some(self.frame(id, Point3::from_array(at)));
        let args = AddCylinderArgs {
            frame,
            radius: Measure::new(radius),
            height: Measure::new(height),
        };
        let op = AddCylinder {
            id: id.into(),
            args,
        };
        op.apply(&mut self.0).expect("a cylinder");
        self.0.world.topo.resolve(id).expect("its body")
    }

    /// The volume of `a` and `b` combined, written out and checked.
    fn run(&self, op: Op, a: BodyId, b: BodyId) -> Result<f64, Refusal> {
        let topo = &self.0.world.topo;
        let outcome = combine(op, &read(topo, a)?, &read(topo, b)?)?;
        if outcome.solid.is_empty() {
            return Ok(0.0);
        }
        let mut out = Topo::new();
        let body = write(&mut out, &outcome.solid, "result")?;
        assert_eq!(out.validate(), Ok(()));
        // What was written reads back as the same solid.
        let again = read(&out, body)?;
        assert_eq!(again.faces.len(), outcome.solid.faces.len());
        let exact = crate::measure(&again).volume;
        Ok(exact)
    }
}

fn close(got: f64, wanted: f64, within: f64) {
    assert!((got - wanted).abs() <= within, "{got} is not {wanted}");
}

const PI: f64 = std::f64::consts::PI;

#[test]
fn boxes_that_overlap_unite_cut_and_intersect() -> anyhow::Result<()> {
    let mut b = Bench::new();
    let first = b.cube("a", [0.0; 3], [2.0; 3]);
    let second = b.cube("b", [1.0; 3], [2.0; 3]);
    close(b.run(Op::Union, first, second)?, 15.0, 1e-9);
    close(b.run(Op::Intersect, first, second)?, 1.0, 1e-9);
    close(b.run(Op::Cut, first, second)?, 7.0, 1e-9);
    close(b.run(Op::Cut, second, first)?, 7.0, 1e-9);
    Ok(())
}

#[test]
fn holes_are_drilled_and_bosses_stand_on_faces() -> anyhow::Result<()> {
    let mut b = Bench::new();
    let plate = b.cube("plate", [0.0; 3], [4.0, 4.0, 2.0]);
    // A cylinder through the plate: a hole, with its seam.
    let drill = b.cylinder("drill", [0.5, 0.0, -2.0], 1.0, 4.0);
    close(b.run(Op::Cut, plate, drill)?, 32.0 - 2.0 * PI, 1e-6);
    // One standing on the plate, its bottom in the plate's top: a boss.
    let boss = b.cylinder("boss", [0.0, 0.5, 1.0], 1.0, 1.5);
    close(b.run(Op::Union, plate, boss)?, 32.0 + 1.5 * PI, 1e-6);
    // One sunk into the plate from its top: a blind hole.
    let sunk = b.cylinder("sunk", [-0.5, 0.0, 0.0], 1.0, 1.0);
    close(b.run(Op::Cut, plate, sunk)?, 32.0 - PI, 1e-6);
    Ok(())
}

#[test]
fn faces_in_one_plane_are_sorted_out() -> anyhow::Result<()> {
    let mut b = Bench::new();
    let base = b.cube("base", [0.0; 3], [4.0, 4.0, 2.0]);
    // Stacked, the faces between them the same; flush on a side, too.
    let stacked = b.cube("stacked", [1.0, 0.0, 2.0], [2.0, 4.0, 2.0]);
    close(b.run(Op::Union, base, stacked)?, 48.0, 1e-9);
    close(b.run(Op::Intersect, base, stacked)?, 0.0, 1e-9);
    // A pocket from the top, its opening in the top face.
    let pocket = b.cube("pocket", [0.0, 0.0, 0.5], [2.0, 2.0, 1.0]);
    close(b.run(Op::Cut, base, pocket)?, 28.0, 1e-9);
    // The very same box.
    let same = b.cube("same", [0.0; 3], [4.0, 4.0, 2.0]);
    close(b.run(Op::Union, base, same)?, 32.0, 1e-9);
    close(b.run(Op::Cut, base, same)?, 0.0, 1e-9);
    Ok(())
}

#[test]
fn bodies_apart_and_inside_each_other() -> anyhow::Result<()> {
    let mut b = Bench::new();
    let outer = b.cube("outer", [0.0; 3], [4.0; 3]);
    let inner = b.cube("inner", [0.2, 0.1, 0.3], [1.0; 3]);
    let far = b.cube("far", [10.0, 0.0, 0.0], [1.0; 3]);
    let topo = &b.0.world.topo;
    let apart = combine(Op::Union, &read(topo, outer)?, &read(topo, far)?)?;
    assert!(!apart.touched);
    assert_eq!(apart.solid.lumps()?.len(), 2);
    close(b.run(Op::Intersect, outer, far)?, 0.0, 1e-12);
    // A box inside a box: cutting leaves a hollow.
    let hollow = combine(Op::Cut, &read(topo, outer)?, &read(topo, inner)?)?;
    assert!(hollow.touched);
    assert_eq!(
        hollow.solid.lump_shells()?,
        vec![vec![(0..6).collect::<Vec<_>>(), (6..12).collect()]]
    );
    close(b.run(Op::Cut, outer, inner)?, 63.0, 1e-9);
    close(b.run(Op::Union, outer, inner)?, 64.0, 1e-9);
    close(b.run(Op::Intersect, outer, inner)?, 1.0, 1e-9);
    close(b.run(Op::Cut, inner, outer)?, 0.0, 1e-9);
    Ok(())
}

#[test]
fn balls_are_cut_like_anything_else() -> anyhow::Result<()> {
    let mut b = Bench::new();
    let plate = b.cube("plate", [0.0; 3], [4.0, 4.0, 2.0]);
    let frame = Some(b.frame("ball", Point3::ZERO));
    let ball = AddSphere {
        id: "ball".into(),
        args: AddSphereArgs {
            frame,
            radius: Measure::new(1.5),
        },
    };
    ball.apply(&mut b.0).expect("a sphere");
    let ball = b.0.world.topo.resolve("ball").expect("its body");
    // A ball of radius 1.5 through a plate 2 thick: a cap of height 0.5
    // sticks out on each side.
    let cap = PI * 0.25 * (3.0 * 1.5 - 0.5) / 3.0;
    let whole = 4.0 / 3.0 * PI * 1.5_f64.powi(3);
    close(b.run(Op::Union, plate, ball)?, 32.0 + 2.0 * cap, 1e-6);
    close(b.run(Op::Cut, plate, ball)?, 32.0 - whole + 2.0 * cap, 1e-6);
    Ok(())
}

#[test]
fn blocks_side_by_side_become_one_block() -> anyhow::Result<()> {
    let mut b = Bench::new();
    // Two cubes set flush: one block, with nothing left of where they
    // met.
    let first = b.cube("a", [0.0; 3], [2.0; 3]);
    let second = b.cube("b", [2.0, 0.0, 0.0], [2.0; 3]);
    let topo = &b.0.world.topo;
    let block = combine(Op::Union, &read(topo, first)?, &read(topo, second)?)?.solid;
    close(crate::measure(&block).volume, 16.0, 1e-9);
    assert_eq!(
        (block.faces.len(), block.edges.len(), block.vertices.len()),
        (6, 12, 8)
    );
    // A lower one beside it: a step, the faces they share still one.
    let low = b.cube("c", [4.0, 0.0, -0.5], [2.0, 2.0, 1.0]);
    let step = combine(Op::Union, &block, &read(&b.0.world.topo, low)?)?.solid;
    close(crate::measure(&step).volume, 20.0, 1e-9);
    assert_eq!(
        (step.faces.len(), step.edges.len(), step.vertices.len()),
        (8, 18, 12)
    );
    // The faces that were made one with another are still known by
    // their names.
    let mut out = Topo::new();
    write(&mut out, &step, "result")?;
    assert_eq!(out.validate(), Ok(()));
    assert!(out.referred("b/top").is_some() && out.lookup("b/top").is_none());
    assert_eq!(out.referred("b/top"), out.referred("a/top"));
    Ok(())
}

#[test]
fn blocks_are_sound_and_cut_as_blocks_do() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    let block = |at: f64| {
        crate::block(
            "b",
            Point3::splat(at),
            [Point3::X, Point3::Y, Point3::Z],
            [2.0, 3.0, 4.0],
        )
    };
    let (a, b) = (block(0.0)?, block(1.0)?);
    let mut topo = workpiece_core::world::Topo::new();
    crate::write(&mut topo, &a, "a")?;
    assert_eq!(topo.validate(), Ok(()));
    let m = crate::measure(&a);
    assert!((m.volume - 24.0).abs() < 1e-12 && (m.area - 52.0).abs() < 1e-12);
    let common = crate::combine(crate::Op::Intersect, &a, &b)?.solid;
    assert!((crate::measure(&common).volume - 6.0).abs() < 1e-9);
    Ok(())
}

#[test]
fn closed_meshes_bound_solids_with_flat_faces() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    // A cube of twelve triangles: six faces, of volume 8.
    let points: Vec<Point3> = (0..8)
        .map(|i| Point3::new(f64::from(i & 1), f64::from(i >> 1 & 1), f64::from(i >> 2)) * 2.0)
        .collect();
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let triangles: Vec<[usize; 3]> = quads
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .collect();
    let cube = crate::faceted("m", &points, &triangles)?;
    let mut topo = workpiece_core::world::Topo::new();
    crate::write(&mut topo, &cube, "m")?;
    assert_eq!(topo.validate(), Ok(()));
    assert_eq!(cube.faces.len(), 6);
    let m = crate::measure(&cube);
    assert!((m.volume - 8.0).abs() < 1e-9 && (m.area - 24.0).abs() < 1e-9);
    // With a triangle missing it is no solid.
    assert!(crate::faceted("m", &points, &triangles[1..]).is_err());
    Ok(())
}

#[test]
fn what_is_under_a_surface_is_a_solid() {
    use workpiece_core::world::topo::NurbsSurface;
    // A 10 by 10 patch 5 up, its middle control point 7 up.
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let mut points = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let h = if (i, j) == (1, 1) { 7.0 } else { 5.0 };
            points.push(Point3::new(5.0 * f64::from(i), 5.0 * f64::from(j), h));
        }
    }
    let top = NurbsSurface::new(2, 2, knots.clone(), knots, 3, 3, points, None).expect("a patch");
    let solid = crate::under("slab", &top, Point3::ZERO, Point3::Z).expect("a solid");
    let mut out = Topo::new();
    write(&mut out, &solid, "slab").expect("written");
    assert_eq!(out.validate(), Ok(()));
    // (The middle function of a quadratic span holds a third of it.)
    let volume = crate::measure(&solid).volume;
    assert!((volume - (500.0 + 200.0 / 9.0)).abs() < 1e-9, "{volume}");
    let low = top
        .control_points()
        .iter()
        .map(|p| *p - Point3::Z * 6.0)
        .collect();
    let k = top.knots().0.to_vec();
    let low = NurbsSurface::new(2, 2, k.clone(), k, 3, 3, low, None).expect("a patch");
    assert!(crate::under("slab", &low, Point3::ZERO, Point3::Z).is_err());
}

#[test]
fn a_cylinder_is_met_shown_and_drawn() -> anyhow::Result<()> {
    use std::collections::HashMap;
    let mut b = Bench::new();
    let cylinder = b.cylinder("c", [0.0, 0.0, 0.0], 1.0, 2.0);
    let near = b.cube("near", [2.5, 0.0, 1.0], [2.0, 2.0, 2.0]);
    let through = b.cube("through", [1.0, 0.0, 1.0], [2.0, 4.0, 6.0]);
    let topo = &b.0.world.topo;
    let (cylinder, near, through) = (
        read(topo, cylinder)?,
        read(topo, near)?,
        read(topo, through)?,
    );

    // Half a unit from the box beside it; half of it in the other.
    let met = crate::interference(&cylinder, &near)?;
    assert_eq!(met.volume, 0.0);
    close(met.gap, 0.5, 1e-9);
    let met = crate::interference(&cylinder, &through)?;
    close(met.volume, std::f64::consts::PI, 1e-6);
    assert_eq!(met.gap, 0.0);

    // Its triangles close: every side walked once each way.
    let shown = crate::display(&cylinder, 0.01);
    let mut sides: HashMap<([u64; 3], [u64; 3]), i32> = HashMap::new();
    let mut volume = 0.0;
    for face in &shown.faces {
        for t in &face.triangles {
            let p = t.map(|i| face.positions[i]);
            volume += p[0].dot(p[1].cross(p[2])) / 6.0;
            for n in 0..3 {
                let key = |p: Point3| p.to_array().map(f64::to_bits);
                *sides.entry((key(p[n]), key(p[(n + 1) % 3]))).or_default() += 1;
                *sides.entry((key(p[(n + 1) % 3]), key(p[n]))).or_default() -= 1;
            }
            assert!((p[1] - p[0]).cross(p[2] - p[0]).dot(face.normals[t[0]]) > 0.0);
        }
    }
    assert!(sides.values().all(|&n| n == 0));
    close(volume, std::f64::consts::TAU, 0.05);
    assert_eq!((shown.edges.len(), shown.seams.len()), (2, 3));
    for turn in shown.edge_turns.iter().flatten() {
        close(turn[0], std::f64::consts::FRAC_PI_2, 1e-9);
    }

    // Seen from above and the side: both rims, the far half of the
    // bottom one hidden, and the two outlines of its side.
    let drawn = crate::project(&cylinder, Point3::new(1.0, 0.0, -1.0), 0.001)?;
    let length = |lines: &[Vec<Point3>]| -> f64 {
        let chords = lines.iter().flat_map(|l| l.windows(2));
        chords.map(|w| w[0].distance(w[1])).sum()
    };
    close(length(&drawn.hidden), std::f64::consts::PI, 0.01);
    close(
        length(&drawn.visible),
        3.0 * std::f64::consts::PI + 4.0,
        0.01,
    );
    Ok(())
}

#[test]
fn equal_cylinders_cross_through_a_seam_and_far_from_the_origin() -> anyhow::Result<()> {
    // Two cylinders of one size, their axes crossing: where they cross
    // is two ellipses that pass through each other at two points —
    // one of them on the second cylinder's seam.
    let mut b = Bench::new();
    let a = b.cylinder("a", [0.0, 0.0, -2.0], 1.0, 4.0);
    let a = read(&b.0.world.topo, a)?;
    let across = a.moved(&|p| Point3::new(p.z, p.x, p.y))?;
    let shared = combine(Op::Intersect, &a, &across)?.solid;
    close(crate::measure(&shared).volume, 16.0 / 3.0, 1e-6);
    // The same a long way out, where coordinates are kept less closely
    // than crossings are looked for.
    let far = Point3::new(2.0e5, -1.0e5, 3.0e5);
    let (a, across) = (a.moved(&|p| p + far)?, across.moved(&|p| p + far)?);
    let shared = combine(Op::Intersect, &a, &across)?.solid;
    close(crate::measure(&shared).volume, 16.0 / 3.0, 1e-6);
    let placer = crate::Placer::new(&shared);
    assert_eq!(placer.side(far, 1e-7), Some(crate::Side::In));
    assert_eq!(
        placer.side(far + Point3::new(0.9, 0.9, 0.0), 1e-7),
        Some(crate::Side::Out)
    );
    Ok(())
}

#[test]
fn many_points_are_placed_as_one_by_one_and_a_cylinder_measures_exactly() -> anyhow::Result<()> {
    let mut b = Bench::new();
    let cylinder = b.cylinder("c", [0.0, 0.0, 0.0], 1.0, 2.0);
    let cube = b.cube("cube", [1.0, 0.0, 1.0], [2.0, 4.0, 6.0]);
    let topo = &b.0.world.topo;
    let (cylinder, cube) = (read(topo, cylinder)?, read(topo, cube)?);
    // The cylinder with a box cut out of it: flat faces, a round wall,
    // edges where they cross.
    let cut = crate::combine(crate::Op::Cut, &cylinder, &cube)?.solid;
    let placer = crate::Placer::new(&cut);
    let (lo, hi) = placer.bounds().expect("not empty");
    let mut points = Vec::new();
    for i in 0..9 {
        for j in 0..9 {
            for k in 0..9 {
                let at = Point3::new(i as f64, j as f64, k as f64) / 8.0;
                points.push(lo + (hi - lo) * at * 1.02 - (hi - lo) * 0.01);
            }
        }
    }
    let one_by_one: Vec<_> = points.iter().map(|p| placer.side(*p, 1e-7)).collect();
    assert_eq!(placer.sides(&points, 1e-7), one_by_one);
    assert!(one_by_one.iter().all(Option::is_some));
    // The wall runs straight: what is summed along it is summed exactly.
    let whole = crate::measure(&cylinder);
    let pi = std::f64::consts::PI;
    assert!((whole.volume - 2.0 * pi).abs() < 1e-11, "{}", whole.volume);
    assert!((whole.area - 6.0 * pi).abs() < 1e-9, "{}", whole.area);
    Ok(())
}

#[test]
fn faces_are_told_for_what_they_are() -> anyhow::Result<()> {
    use crate::known::{Closed, Known, closed, known, never_branch};
    let mut b = Bench::new();
    let cube = b.cube("cube", [0.0; 3], [2.0, 2.0, 2.0]);
    let cylinder = b.cylinder("c", [0.0, 0.0, 0.0], 1.0, 2.0);
    let frame = Some(b.frame("ball", Point3::ZERO));
    let ball = AddSphere {
        id: "ball".into(),
        args: AddSphereArgs {
            frame,
            radius: Measure::new(1.5),
        },
    };
    ball.apply(&mut b.0).expect("a sphere");
    let ball = b.0.world.topo.resolve("ball").expect("its body");
    let topo = &b.0.world.topo;
    let of = |body: BodyId| -> anyhow::Result<Vec<Option<Known>>> {
        let data = crate::query::SolidData::new(&read(topo, body)?);
        Ok(data.faces.iter().map(|f| known(&f.patches)).collect())
    };
    // A box is six planes; a cylinder two planes and a wall of its
    // radius; a ball a sphere.
    let flat = |k: &Option<Known>| matches!(k, Some(Known::Plane { .. }));
    assert!(of(cube)?.iter().all(flat));
    let faces = of(cylinder)?;
    assert_eq!(faces.iter().filter(|k| flat(k)).count(), 2);
    let wall = faces.iter().flatten().find_map(|k| match k {
        Known::Cylinder {
            axis,
            radius,
            length,
            ..
        } => Some((*axis, *radius, *length)),
        _ => None,
    });
    let (axis, radius, length) = wall.expect("a wall");
    assert!(axis.cross(Point3::Z).length() < 1e-12);
    close(radius, 1.0, 1e-12);
    close(length, 2.0, 1e-12);
    let round = of(ball)?;
    let Some(Known::Sphere {
        centre,
        radius: of_ball,
    }) = round[0]
    else {
        panic!("{round:?}");
    };
    assert!(centre.length() < 1e-12);
    close(of_ball, 1.5, 1e-12);
    // A plane along the wall's axis crosses it in straight lines; one
    // across it, in a circle, of which three places are told; a plane
    // through the ball likewise, and none that misses it.
    let wall = Known::Cylinder {
        centre: Point3::ZERO,
        axis,
        radius,
        length,
    };
    let (side, top) = (
        Known::Plane { normal: Point3::X },
        Known::Plane { normal: Point3::Z },
    );
    let at = (Point3::new(0.0, 0.0, 1.0), Point3::ZERO);
    assert!(matches!(closed(&side, &wall, at, 1e-7), Closed::Never));
    let Closed::Through(places) = closed(&top, &wall, at, 1e-7) else {
        panic!("a circle");
    };
    for p in places {
        close(p.z, 1.0, 1e-12);
        close(p.truncate().length(), 1.0, 1e-12);
    }
    let sphere = Known::Sphere {
        centre: Point3::ZERO,
        radius: 1.5,
    };
    let Closed::Through(places) = closed(&top, &sphere, at, 1e-7) else {
        panic!("a circle");
    };
    for p in places {
        close(p.length(), 1.5, 1e-12);
        close(p.z, 1.0, 1e-12);
    }
    let far = (Point3::new(0.0, 0.0, 2.0), Point3::ZERO);
    assert!(matches!(closed(&top, &sphere, far, 1e-7), Closed::Never));
    assert!(never_branch(&side, &wall) && never_branch(&sphere, &sphere));
    assert!(!never_branch(&wall, &sphere));
    Ok(())
}

#[test]
fn a_plane_cuts_outlines_out_of_solids_without_a_boolean() -> anyhow::Result<()> {
    let length = |lines: &[Vec<Point3>]| -> f64 {
        let pairs = lines.iter().flat_map(|l| l.windows(2));
        pairs.map(|w| w[0].distance(w[1])).sum()
    };
    // A sphere of radius 2, cut 1 off its middle: a circle of radius
    // √3, wherever on the sphere that is.
    let mut b = Bench::new();
    let frame = Some(b.frame("ball", Point3::ZERO));
    let ball = AddSphere {
        id: "ball".into(),
        args: AddSphereArgs {
            frame,
            radius: Measure::new(2.0),
        },
    };
    ball.apply(&mut b.0).expect("a sphere");
    let ball = b.0.world.topo.resolve("ball").expect("its body");
    let ball = read(&b.0.world.topo, ball)?;
    for normal in [Point3::Z, Point3::X, Point3::new(1.0, 2.0, -0.5)] {
        let normal = normal.normalize();
        let cut = crate::plane_section(&ball, normal, normal, 1e-4).expect("a circle");
        let round = 2.0 * std::f64::consts::PI * 3f64.sqrt();
        let got = length(&cut);
        assert!(
            got <= round + 1e-9 && got >= round - 1e-3,
            "{got} of {round}"
        );
        for p in cut.iter().flatten() {
            assert!((p.length() - 2.0).abs() < 1e-9 && (p.dot(normal) - 1.0).abs() < 1e-9);
        }
    }
    // A box 2 × 3 × 4 cut level: its outline. Through a corner it is
    // not told this way.
    let axes = [Point3::X, Point3::Y, Point3::Z];
    let block = crate::block("b", Point3::ZERO, axes, [2.0, 3.0, 4.0])?;
    let cut =
        crate::plane_section(&block, Point3::new(0.3, 0.2, 1.0), Point3::Z, 1e-4).expect("lines");
    assert_eq!(cut.len(), 4);
    close(length(&cut), 10.0, 1e-12);
    assert!(crate::plane_section(&block, Point3::ZERO, Point3::new(1.0, 1.0, 1.0), 1e-4).is_none());
    Ok(())
}

#[test]
fn the_spread_of_a_block_is_what_the_books_say() {
    use workpiece_core::math::Point3;
    // A 2 × 3 × 4 block: m (b² + c²) / 12 about each axis through its middle.
    let axes = [Point3::X, Point3::Y, Point3::Z];
    let block = crate::block("b", Point3::new(5.0, 0.0, 0.0), axes, [2.0, 3.0, 4.0]).unwrap();
    let s = crate::spread(&block);
    assert!((s.volume - 24.0).abs() < 1e-9);
    assert!((s.centre - Point3::new(6.0, 1.5, 2.0)).length() < 1e-9);
    let wanted = [24.0 * 25.0 / 12.0, 24.0 * 20.0 / 12.0, 24.0 * 13.0 / 12.0];
    for (k, w) in wanted.iter().enumerate() {
        assert!((s.inertia[k][k] - w).abs() < 1e-9, "{:?}", s.inertia);
    }
    assert!(s.inertia[0][1].abs() < 1e-9 && s.inertia[1][2].abs() < 1e-9);
    // A cylinder of radius 3 and height 5 standing on (1, 2, 0): m r² / 2
    // about its axis, m (3 r² + h²) / 12 across it.
    let mut b = Bench::new();
    let body = b.cylinder("c", [1.0, 2.0, 0.0], 3.0, 5.0);
    let s = crate::spread(&read(&b.0.world.topo, body).unwrap());
    let m = std::f64::consts::PI * 9.0 * 5.0;
    let wanted = [m * (27.0 + 25.0) / 12.0, m * (27.0 + 25.0) / 12.0, m * 4.5];
    assert!((s.volume - m).abs() < 1e-9 * m);
    assert!((s.centre - Point3::new(1.0, 2.0, 2.5)).length() < 1e-9);
    for (k, w) in wanted.iter().enumerate() {
        assert!((s.inertia[k][k] - w).abs() < 1e-6 * w, "{:?}", s.inertia);
    }
}
