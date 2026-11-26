//! What the operations of this crate make.

use workpiece_ops_sweep::{Extent, Extrude};
use workpiece_ops_testing::*;
use workpiece_shape::Combine;

#[test]
fn faces_are_copied_by_an_offset_of_nothing() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    let act = |b: &mut Standalone, id: &str, way: SurfaceWay, picked: &[&str], by: f64| {
        Surface {
            id: id.into(),
            args: SurfaceArgs {
                way,
                picked: picked
                    .iter()
                    .map(|p| Source::Topology((*p).to_owned()).into())
                    .collect(),
                distance: Measure::new(by),
                ..SurfaceArgs::default()
            },
        }
        .apply(b)
    };
    let kind = |b: &Standalone, name: &str| b.world.topo.body(body(b, name)).map(|b| b.kind);
    let faces =
        |b: &Standalone, name: &str| b.world.topo.body_faces(body(b, name)).map(|f| f.len());
    // The box's top and front as a surface of two faces sharing an edge.
    let mut b = Standalone::new();
    base(&mut b)?;
    act(
        &mut b,
        "skin",
        SurfaceWay::Offset,
        &["base/top", "base/front"],
        0.0,
    )?;
    check(&b);
    assert_eq!(
        (kind(&b, "skin"), faces(&b, "skin")),
        (Some(BodyKind::Sheet), Some(2))
    );
    let skin = workpiece_boolean::read_sheet(&b.world.topo, body(&b, "skin"))?;
    assert_eq!(skin.edges.len(), 7);
    // All of the box copied: closed all around, a solid of its volume.
    act(&mut b, "again", SurfaceWay::Offset, &["base"], 0.0)?;
    check(&b);
    assert_eq!(kind(&b, "again"), Some(BodyKind::Solid));
    let again = workpiece_boolean::read(&b.world.topo, body(&b, "again"))?;
    close(workpiece_boolean::measure(&again).volume, 32.0, 1e-9);
    assert_eq!(
        (again.faces.len(), again.edges.len(), again.vertices.len()),
        (6, 12, 8)
    );
    // A surface half a unit off the solid: six faces as large as its
    // own (they do not run on to meet each other).
    act(&mut b, "off", SurfaceWay::Offset, &["again"], 0.5)?;
    check(&b);
    assert_eq!(faces(&b, "off"), Some(6));
    let off = workpiece_boolean::read_sheet(&b.world.topo, body(&b, "off"))?;
    let area: f64 = (0..6).map(|f| workpiece_boolean::face_area(&off, f)).sum();
    close(area, 64.0, 1e-9);
    // A face deleted from the two-faced surface leaves one.
    let first = b.world.topo.body_faces(body(&b, "skin")).expect("faces")[0];
    let name = b.world.topo.name_of(first).expect("a name").to_owned();
    act(&mut b, "less", SurfaceWay::Delete, &[&name], 0.0)?;
    check(&b);
    assert_eq!(faces(&b, "skin"), Some(1));
    Ok(())
}

#[test]
fn flat_holes_are_filled_by_a_boundary_into_a_solid() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Boundary, BoundaryArgs, BoundarySide};
    // A tube of radius 0.5, 2 long, open at both ends: each rim (one
    // round edge) filled flat and sewn to the tube; both, a rod.
    let mut b = Standalone::new();
    sketch(&mut b, "s", xy(), None, &[([0.0, 0.0], 0.5)])?;
    let mut tube = Extrude::of("tube", Some("s".into()), true);
    tube.args.distance = Measure::new(2.0);
    tube.apply(&mut b)?;
    let rims = free_edges(&b, "tube");
    assert_eq!(rims.len(), 2);
    for (k, rim) in rims.into_iter().enumerate() {
        Boundary {
            id: format!("lid{k}"),
            args: BoundaryArgs {
                sides: vec![BoundarySide::free(rim.into())],
            },
        }
        .apply(&mut b)?;
        check(&b);
    }
    assert_eq!(
        b.world.topo.body(body(&b, "tube")).map(|b| b.kind),
        Some(BodyKind::Solid)
    );
    let rod = workpiece_boolean::read(&b.world.topo, body(&b, "tube"))?;
    close(
        workpiece_boolean::measure(&rod).volume,
        PI * 0.25 * 2.0,
        1e-9,
    );
    Ok(())
}

#[test]
fn a_face_is_split_by_a_sketch_and_by_curves_on_it() -> anyhow::Result<()> {
    use workpiece_ops_surface::{
        CurveProjection, CurveProjectionArgs, SplitBy, Surface, SurfaceArgs, SurfaceWay,
    };
    let area = |b: &Standalone, name: &str| -> anyhow::Result<Vec<f64>> {
        let sheet = workpiece_boolean::read_sheet(&b.world.topo, body(b, name))?;
        Ok((0..sheet.faces.len())
            .map(|f| workpiece_boolean::face_area(&sheet, f))
            .collect())
    };
    let split = |b: &mut Standalone, by: SplitBy, sketch: bool, curves: &[&str]| {
        Surface {
            id: "cut".into(),
            args: SurfaceArgs {
                way: SurfaceWay::Split,
                split: by,
                sketch: sketch.then(|| Source::Profile("circle".into()).into()),
                curves: curves
                    .iter()
                    .map(|c| Source::Topology((*c).to_owned()).into())
                    .collect(),
                picked: vec![Source::Topology("skin".into()).into()],
                ..SurfaceArgs::default()
            },
        }
        .apply(b)
    };
    // The top of the 4 × 4 × 2 box as a surface, and a circle of
    // radius 1 sketched below it: thrown up onto it, the top is two
    // faces, the disc and the rest, as large as it was together.
    let start = |b: &mut Standalone| -> anyhow::Result<()> {
        base(b)?;
        sketch(b, "circle", xy(), None, &[([0.5, 0.0], 1.0)])?;
        Surface {
            id: "skin".into(),
            args: SurfaceArgs {
                way: SurfaceWay::Offset,
                picked: vec![Source::Topology("base/top".into()).into()],
                distance: Measure::new(0.0),
                ..SurfaceArgs::default()
            },
        }
        .apply(b)
    };
    let mut b = Standalone::new();
    start(&mut b)?;
    split(&mut b, SplitBy::Sketch, true, &[])?;
    check(&b);
    let mut faces = area(&b, "skin")?;
    faces.sort_by(f64::total_cmp);
    assert_eq!(faces.len(), 2);
    close(faces[0], PI, 1e-6);
    close(faces[0] + faces[1], 16.0, 1e-6);
    // The same from the circle thrown onto the top first, as a curve.
    let mut b = Standalone::new();
    start(&mut b)?;
    let top = b.world.topo.body_faces(body(&b, "skin")).expect("faces")[0];
    let top = b.world.topo.name_of(top).expect("a name").to_owned();
    CurveProjection {
        id: "ring".into(),
        args: CurveProjectionArgs {
            onto: vec![Source::Topology(top).into()],
            curves: vec![Source::Profile("circle".into()).into()],
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    split(&mut b, SplitBy::Curves, false, &["ring"])?;
    check(&b);
    let mut faces = area(&b, "skin")?;
    faces.sort_by(f64::total_cmp);
    assert_eq!(faces.len(), 2);
    close(faces[0], PI, 1e-4);
    close(faces[0] + faces[1], 16.0, 1e-6);
    Ok(())
}

#[test]
fn a_loft_between_two_edges_is_a_ruled_surface() -> anyhow::Result<()> {
    use workpiece_ops_sweep::{Section, SweptLoft, SweptLoftArgs};
    // The box's top front edge and its bottom back edge, 4 long each:
    // the surface between them is flat, 4 by the diagonal across the
    // box's side (4 deep, 2 high).
    let mut b = Standalone::new();
    base(&mut b)?;
    let at = |y: f64, z: f64| {
        move |p: workpiece_core::math::Point3, q: workpiece_core::math::Point3| {
            (p.y - y).abs() < 1e-9
                && (q.y - y).abs() < 1e-9
                && (p.z - z).abs() < 1e-9
                && (q.z - z).abs() < 1e-9
        }
    };
    let front = edges_where(&b, "base", at(-2.0, 1.0));
    let back = edges_where(&b, "base", at(2.0, -1.0));
    assert_eq!((front.len(), back.len()), (1, 1));
    SweptLoft {
        id: "ramp".into(),
        args: SweptLoftArgs {
            sections: vec![Section::of(front[0].clone()), Section::of(back[0].clone())],
            surface: true,
            ..SweptLoftArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let ramp = workpiece_boolean::read_sheet(&b.world.topo, body(&b, "ramp"))?;
    assert_eq!(ramp.faces.len(), 1);
    close(
        workpiece_boolean::face_area(&ramp, 0),
        4.0 * 20.0_f64.sqrt(),
        1e-6,
    );
    Ok(())
}

#[test]
fn surfaces_run_on_and_go_back_to_what_they_were() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    let act = |b: &mut Standalone, id: &str, way: SurfaceWay, picked: &str, distance: f64| {
        Surface {
            id: id.into(),
            args: SurfaceArgs {
                way,
                picked: vec![Source::Topology(picked.to_owned()).into()],
                distance: Measure::new(distance),
                ..SurfaceArgs::default()
            },
        }
        .apply(b)
    };
    let area = |b: &Standalone, name: &str| -> anyhow::Result<f64> {
        let sheet = workpiece_boolean::read_sheet(&b.world.topo, body(b, name))?;
        Ok((0..sheet.faces.len())
            .map(|f| workpiece_boolean::face_area(&sheet, f))
            .sum())
    };
    // The top of the 4 × 4 × 2 box as a surface: run on by 1 all
    // around, it is 6 × 6.
    let mut b = Standalone::new();
    base(&mut b)?;
    act(&mut b, "skin", SurfaceWay::Offset, "base/top", 0.0)?;
    act(&mut b, "wider", SurfaceWay::Extend, "skin", 1.0)?;
    check(&b);
    close(area(&b, "skin")?, 36.0, 1e-6);
    // The top with a hole in it: untrimmed, its whole square again.
    let mut b = Standalone::new();
    base(&mut b)?;
    sketch(&mut b, "circle", xy(), None, &[([0.0, 0.0], 0.5)])?;
    let mut hole = Extrude::of("hole", Some("circle".into()), false);
    hole.args.extent = Extent::Symmetric;
    hole.args.distance = Measure::new(4.0);
    hole.args.combine = Combine::Cut;
    hole.apply(&mut b)?;
    act(&mut b, "skin", SurfaceWay::Offset, "base/top", 0.0)?;
    close(area(&b, "skin")?, 16.0 - PI * 0.25, 1e-9);
    Surface {
        id: "all".into(),
        args: SurfaceArgs {
            way: SurfaceWay::Extend,
            whole: true,
            picked: vec![Source::Topology("skin".into()).into()],
            ..SurfaceArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    close(area(&b, "skin")?, 16.0, 1e-9);
    Ok(())
}

#[test]
fn four_edges_bound_a_surface() -> anyhow::Result<()> {
    use std::sync::Arc;
    use workpiece_core::math::Point3;
    use workpiece_core::world::topo::Curve3;
    use workpiece_shape::between;
    // A square frame, two of its sides arched up by 1: the surface runs
    // along all four.
    let arch = |a: Point3, b: Point3| {
        let top = (a + b) / 2.0 + Point3::Z * 2.0;
        Curve3::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![a, top, b], None)
    };
    let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
    let sides = [
        (Arc::new(arch(p(0.0, 0.0), p(4.0, 0.0))?), (0.0, 1.0)),
        (
            Arc::new(Curve3::line(p(4.0, 4.0), p(4.0, 0.0))?),
            (0.0, 1.0),
        ),
        (Arc::new(arch(p(0.0, 4.0), p(4.0, 4.0))?), (0.0, 1.0)),
        (
            Arc::new(Curve3::line(p(0.0, 0.0), p(0.0, 4.0))?),
            (0.0, 1.0),
        ),
    ];
    let sheet = between(&sides, "roof")?;
    let mut topo = workpiece_core::world::Topo::new();
    workpiece_boolean::write_sheet(&mut topo, &sheet, "roof")?;
    assert_eq!(topo.validate(), Ok(()));
    let surface = &sheet.faces[0].surface;
    let ((u0, u1), (v0, v1)) = surface.domain();
    // A barrel: over the middle of the frame it is as high as the arches.
    let middle = surface.point_at((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    assert!(
        middle.distance(Point3::new(2.0, 2.0, 1.0)) < 1e-6,
        "{middle}"
    );
    // The arch's length (a parabola 4 wide and 1 high) times the 4 it
    // is carried.
    let arc = (0..4000)
        .map(|k| {
            let at = |t: f64| sides[0].0.point_at(t);
            at(f64::from(k) / 4000.0).distance(at(f64::from(k + 1) / 4000.0))
        })
        .sum::<f64>();
    close(workpiece_boolean::face_area(&sheet, 0), arc * 4.0, 1e-5);
    Ok(())
}

#[test]
fn surfaces_are_split_by_planes_and_trimmed_by_a_delete() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    let act = |b: &mut Standalone, id: &str, way: SurfaceWay, picked: &str, plane: bool| {
        Surface {
            id: id.into(),
            args: SurfaceArgs {
                way,
                picked: vec![Source::Topology(picked.to_owned()).into()],
                distance: Measure::new(0.0),
                split: workpiece_ops_surface::SplitBy::Plane,
                plane: plane.then(|| Reference::frame_axis(ORIGIN, Axis::X)),
                ..SurfaceArgs::default()
            },
        }
        .apply(b)
    };
    let area = |b: &Standalone, name: &str| -> anyhow::Result<f64> {
        let sheet = workpiece_boolean::read_sheet(&b.world.topo, body(b, name))?;
        Ok((0..sheet.faces.len())
            .map(|f| workpiece_boolean::face_area(&sheet, f))
            .sum())
    };
    // The top of the 4 × 4 × 2 box, about the origin: trimmed to the
    // plane x = 0, half of it is left; split, both halves are.
    let mut b = Standalone::new();
    base(&mut b)?;
    act(&mut b, "skin", SurfaceWay::Offset, "base/top", false)?;
    let whole = area(&b, "skin")?;
    act(&mut b, "cut", SurfaceWay::Split, "skin", true)?;
    check(&b);
    close(area(&b, "skin")? + area(&b, "cut")?, whole, 1e-9);
    let behind = area(&b, "skin")?;
    assert!(behind > 0.0 && behind < whole);
    // A tube of radius 0.5, 2 long, about z: split by the plane x = 0
    // and what is in front deleted, half of its wall is left.
    let mut t = Standalone::new();
    sketch(&mut t, "s", xy(), None, &[([0.0, 0.0], 0.5)])?;
    let mut tube = Extrude::of("tube", Some("s".into()), true);
    tube.args.distance = Measure::new(2.0);
    tube.apply(&mut t)?;
    act(&mut t, "half", SurfaceWay::Split, "tube", true)?;
    act(&mut t, "gone", SurfaceWay::Delete, "half", false)?;
    check(&t);
    close(area(&t, "tube")?, PI, 1e-6);
    // The trimmed tube moved 5 along x, as a body is: as large, and
    // 5 on.
    let mut shift = workpiece_ops_bodies::Transform {
        id: "shift".into(),
        args: Default::default(),
    };
    shift.args.body = Some(Source::Topology("tube".into()).into());
    shift.args.by[0] = Measure::new(5.0);
    shift.apply(&mut t)?;
    check(&t);
    close(area(&t, "tube")?, PI, 1e-6);
    let moved = workpiece_boolean::read_sheet(&t.world.topo, body(&t, "tube"))?;
    assert!(moved.extent().0.x > 4.0);
    Ok(())
}

#[test]
fn edges_inside_surfaces_are_rounded() -> anyhow::Result<()> {
    use workpiece_shape::{faces_of, rounded};
    // The top (4 × 4) and the front (4 × 2) of the box, as one
    // surface: the edge between them rounded to 0.5.
    let mut b = Standalone::new();
    base(&mut b)?;
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    let face = |name: &str| {
        let found = solid
            .faces
            .iter()
            .position(|f| f.name.as_deref() == Some(name));
        found.expect("the face")
    };
    let (top, front) = (face("base/top"), face("base/front"));
    let sheet = faces_of(&solid, &[top, front]);
    let inner = (0..sheet.edges.len())
        .find(|&e| {
            let steps = sheet.faces.iter().flat_map(|f| f.loops.iter().flatten());
            let walked = steps.filter(
                |s| matches!(s.walk, workpiece_boolean::model::Walk::Edge { edge, .. } if edge == e),
            );
            walked.count() == 2
        })
        .expect("the edge between them");
    let r = 0.5;
    let made = rounded(&sheet, &[inner], r, "round")?;
    let mut topo = workpiece_core::world::Topo::new();
    workpiece_boolean::write_sheet(&mut topo, &made, "sheet")?;
    assert_eq!(topo.validate(), Ok(()));
    assert_eq!(made.faces.len(), 3);
    let area: f64 = (0..made.faces.len())
        .map(|f| workpiece_boolean::face_area(&made, f))
        .sum();
    close(
        area,
        (4.0 - r) * 4.0 + (2.0 - r) * 4.0 + PI * r / 2.0 * 4.0,
        1e-6,
    );
    Ok(())
}

#[test]
fn curves_in_space_are_bodies_of_their_own() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_ops_surface::{
        CurveIntersection, CurveIntersectionArgs, CurveProjection, CurveProjectionArgs,
        CurveThrough, CurveThroughArgs, EndHeld, Helix, HelixArgs, through,
    };
    // A curve through points goes through them.
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 2.0, 0.5),
        Point3::new(3.0, 2.5, 2.0),
        Point3::new(4.0, 0.0, 3.0),
    ];
    let curve = through(&points, false, [EndHeld::default(); 2])?;
    // (Its parameter is how far along the points it is.)
    let mut at = 0.0;
    for (k, p) in points.iter().enumerate() {
        if k > 0 {
            at += points[k - 1].distance(*p);
        }
        assert!(curve.point_at(at).distance(*p) < 1e-9);
    }
    // Closed, it comes back to the first point smoothly; held at its
    // ends, it leaves and arrives along what it is held to.
    let closed = through(&points, true, [EndHeld::default(); 2])?;
    let (t0, t1) = closed.domain();
    assert!(closed.point_at(t1).distance(points[0]) < 1e-12);
    let (leaves, arrives) = (closed.derivative_at(t0).1, closed.derivative_at(t1).1);
    assert!(leaves.normalize().dot(arrives.normalize()) > 1.0 - 1e-9);
    let along = |d: Point3| EndHeld {
        along: Some(d),
        ..EndHeld::default()
    };
    let held = through(&points, false, [along(Point3::Z), along(-Point3::X)])?;
    let (t0, t1) = held.domain();
    let (leaves, arrives) = (held.derivative_at(t0).1, held.derivative_at(t1).1);
    assert!(leaves.normalize().dot(Point3::Z) > 1.0 - 1e-9);
    assert!(arrives.normalize().dot(Point3::X).abs() > 1.0 - 1e-9);
    // Its start flipped: it leaves the other way along z, through the
    // same points.
    let flipped = EndHeld {
        flip: true,
        ..along(Point3::Z)
    };
    let back = through(&points, false, [flipped, along(-Point3::X)])?;
    assert!(back.derivative_at(t0).1.normalize().dot(-Point3::Z) > 1.0 - 1e-9);
    assert!(back.point_at(t1).distance(points[3]) < 1e-9);
    // Fuller (twice the length): along z all the same, but further out
    // before it turns to its points.
    let fuller = EndHeld {
        length: 2.0,
        ..along(Point3::Z)
    };
    let full = through(&points, false, [fuller, along(-Point3::X)])?;
    let d = full.derivative_at(t0).1;
    assert!(d.normalize().dot(Point3::Z) > 1.0 - 1e-9);
    close(d.length(), 2.0 * leaves.length(), 1e-6);
    let early = (t1 - t0) * 0.05;
    assert!(full.point_at(t0 + early).z > held.point_at(t0 + early).z);
    let wire = |b: &Standalone, name: &str| -> Vec<(Point3, Point3)> {
        let topo = &b.world.topo;
        let it = topo.body(body(b, name)).expect("the curve");
        assert_eq!(it.kind, workpiece_core::world::topo::BodyKind::Wire);
        let ends = it.free_edges.iter().map(|&e| {
            let edge = topo.edge(e).expect("an edge");
            let curve = topo.curve(edge.curve).expect("its curve");
            (curve.point_at(edge.t0), curve.point_at(edge.t1))
        });
        ends.collect()
    };
    // A helix of radius 2 about z, 3 turns of pitch 0.5: it ends 1.5 up.
    let mut b = Standalone::new();
    base(&mut b)?;
    Helix {
        id: "coil".into(),
        args: HelixArgs {
            axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            radius: Measure::new(2.0),
            pitch: Measure::new(0.5),
            ..HelixArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let (from, to) = wire(&b, "coil")[0];
    close((to - from).z, 1.5, 1e-9);
    close(from.truncate().length(), 2.0, 1e-9);
    // Started a quarter turn on, it starts a quarter turn on about z.
    Helix {
        id: "turned".into(),
        args: HelixArgs {
            axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            radius: Measure::new(2.0),
            pitch: Measure::new(0.5),
            start: Measure::new(90.0),
            ..HelixArgs::default()
        },
    }
    .apply(&mut b)?;
    let (turned, _) = wire(&b, "turned")[0];
    close(turned.z, from.z, 1e-12);
    close(turned.truncate().dot(from.truncate()), 0.0, 1e-9);
    close(from.truncate().perp_dot(turned.truncate()), 4.0, 1e-9);
    // A flat spiral from radius 1, growing 0.5 a turn, for 2 turns:
    // it ends 2 out, where it started along.
    Helix {
        id: "spiral".into(),
        args: HelixArgs {
            axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            radius: Measure::new(1.0),
            pitch: Measure::new(0.0),
            turns: Measure::new(2.0),
            grow: Measure::new(0.5),
            ..HelixArgs::default()
        },
    }
    .apply(&mut b)?;
    let (from, to) = wire(&b, "spiral")[0];
    close((to - from).z, 0.0, 1e-12);
    close(to.truncate().length(), 2.0, 1e-9);
    close(
        to.truncate().normalize().dot(from.truncate().normalize()),
        1.0,
        1e-9,
    );
    // Where the top and the front of the 4 × 4 × 2 box cross: the edge
    // between them, 4 long.
    CurveIntersection {
        id: "edge".into(),
        args: CurveIntersectionArgs {
            first: Some(Source::Topology("base/top".into()).into()),
            second: Some(Source::Topology("base/front".into()).into()),
        },
    }
    .apply(&mut b)?;
    check(&b);
    let long: f64 = wire(&b, "edge").iter().map(|(a, b)| a.distance(*b)).sum();
    close(long, 4.0, 1e-9);
    // A straight curve 1 above the box's top, from inside it to well
    // past its side, thrown down z onto the top: as much of it as is
    // over the top, 1.5 long, on the top.
    let over = workpiece_core::world::topo::Curve3::line(
        Point3::new(0.5, 0.0, 3.0),
        Point3::new(9.0, 0.0, 3.0),
    )?;
    workpiece_ops_surface::write_wire(
        &mut b.world.topo,
        "over",
        &[(std::sync::Arc::new(over), (0.0, 1.0))],
    )?;
    CurveProjection {
        id: "shadow".into(),
        args: CurveProjectionArgs {
            onto: vec![Source::Topology("base/top".into()).into()],
            curves: vec![Source::Topology("over".into()).into()],
            along: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
        },
    }
    .apply(&mut b)?;
    check(&b);
    let shadow = wire(&b, "shadow");
    let long: f64 = shadow.iter().map(|(a, b)| a.distance(*b)).sum();
    close(long, 1.5, 1e-9);
    let top = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?
        .extent()
        .1
        .z;
    assert!(
        shadow
            .iter()
            .all(|(a, b)| (a.z - top).abs() < 1e-9 && (b.z - top).abs() < 1e-9)
    );
    // A disk of radius 0.2 swept along a curve through three points in
    // space: about as much as the disk times the curve's length.
    let mut s = Standalone::new();
    sketch(&mut s, "disk", xy(), None, &[([0.0, 0.0], 0.2)])?;
    let bent = through(
        &[
            Point3::ZERO,
            Point3::new(0.5, 0.0, 2.0),
            Point3::new(2.0, 1.0, 4.0),
        ],
        false,
        [EndHeld::default(); 2],
    )?;
    let range = bent.domain();
    let length: f64 = (0..2000)
        .map(|k| {
            let at = |k: i32| bent.point_at(range.0 + (range.1 - range.0) * f64::from(k) / 2000.0);
            at(k).distance(at(k + 1))
        })
        .sum();
    workpiece_ops_surface::write_wire(
        &mut s.world.topo,
        "way",
        &[(std::sync::Arc::new(bent), range)],
    )?;
    let mut sweep = workpiece_ops_sweep::SweptLoft {
        id: "pipe".into(),
        args: Default::default(),
    };
    sweep.args.sections = vec![workpiece_ops_sweep::Section::of(Source::Profile(
        "disk".into(),
    ))];
    sweep.args.rails = vec![workpiece_ops_sweep::Rail {
        of: Source::Topology("way".into()).into(),
        role: workpiece_ops_sweep::Role::Path,
    }];
    sweep.apply(&mut s)?;
    check(&s);
    let pipe = workpiece_boolean::read(&s.world.topo, body(&s, "pipe"))?;
    close(
        workpiece_boolean::measure(&pipe).volume,
        PI * 0.04 * length,
        0.05,
    );
    // A cable laid over the box's top: through points on the face, a
    // radius off it.
    let mut b = Standalone::new();
    base(&mut b)?;
    CurveThrough {
        id: "cable".into(),
        args: CurveThroughArgs {
            points: vec![
                Source::Topology("base/top".into()).into(),
                Source::Topology("base/top".into()).into(),
                Source::Topology("base/top".into()).into(),
            ],
            // (Pointed at a little off the face: it is the face's
            // nearest point that counts.)
            spots: vec![
                Some([-1.5, -1.0, 1.02]),
                Some([0.0, 0.5, 0.97]),
                Some([1.5, -1.0, 1.0]),
            ],
            offset: Measure::new(0.1),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let topo = &b.world.topo;
    let wire = topo.body(body(&b, "cable")).expect("the cable");
    let edge = topo.edge(wire.free_edges[0]).expect("its edge");
    let curve = topo.curve(edge.curve).expect("its curve");
    for (t, x) in [(edge.t0, -1.5), (edge.t1, 1.5)] {
        let p = curve.point_at(t);
        close(p.z, 1.1, 1e-9);
        close(p.x, x, 1e-9);
    }
    Ok(())
}

#[test]
fn a_surface_step_saved_with_its_way_beside_its_arguments_still_reads() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceWay};
    let old = r#"{"id":"s","way":"Delete","args":{"picked":[]}}"#;
    let read: Surface = serde_json::from_str(old)?;
    assert_eq!(read.args.way, SurfaceWay::Delete);
    // As it is written now, and read back the same.
    let text = serde_json::to_string(&read)?;
    assert!(!text.contains(r#""way":"Delete","args""#), "{text}");
    assert_eq!(serde_json::from_str::<Surface>(&text)?, read);
    Ok(())
}

#[test]
fn a_whole_sketch_is_thrown_onto_a_cylinder() -> anyhow::Result<()> {
    use workpiece_ops_surface::{CurveProjection, CurveProjectionArgs};
    // A drum of radius 2 standing on the origin, and a square sketched
    // on the plane square to y, thrown along that plane's normal: onto
    // the drum's side in front and behind, every piece on the side.
    let mut b = Standalone::new();
    workpiece_ops_basic_shapes::AddCylinder {
        id: "drum".into(),
        args: workpiece_ops_basic_shapes::AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(2.0),
            height: Measure::new(4.0),
        },
    }
    .apply(&mut b)?;
    sketch(
        &mut b,
        "logo",
        Reference::frame_axis(ORIGIN, Axis::Y),
        Some(([1.0, 1.0], [1.5, 1.5])),
        &[],
    )?;
    CurveProjection {
        id: "wrapped".into(),
        args: CurveProjectionArgs {
            onto: vec![Source::Topology("drum/side".into()).into()],
            curves: vec![Source::Profile("logo".into()).into()],
            along: None,
        },
    }
    .apply(&mut b)?;
    check(&b);
    let topo = &b.world.topo;
    let wire = topo.body(body(&b, "wrapped")).expect("the curves");
    assert_eq!(wire.kind, BodyKind::Wire);
    // Four sides, each thrown onto the front and the back.
    assert_eq!(wire.free_edges.len(), 8);
    let (mut front, mut back) = (0, 0);
    for &e in &wire.free_edges {
        let edge = topo.edge(e).expect("an edge");
        let curve = topo.curve(edge.curve).expect("its curve");
        for k in 0..=4 {
            let t = edge.t0 + (edge.t1 - edge.t0) * f64::from(k) / 4.0;
            let p = curve.point_at(t);
            close(p.truncate().length(), 2.0, 1e-6);
        }
        if curve.point_at(edge.t0).y > 0.0 {
            front += 1;
        } else {
            back += 1;
        }
    }
    assert_eq!((front, back), (4, 4));
    Ok(())
}

#[test]
fn faces_of_round_bodies_cross() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_core::world::{Datum, DatumFrame};
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs, AddTorus, AddTorusArgs};
    use workpiece_ops_surface::{CurveIntersection, CurveIntersectionArgs};
    let frame = |b: &mut Standalone, name: &str, origin: Point3, x: Point3, y: Point3| {
        let frame = DatumFrame::new(origin, x, y).expect("a frame");
        b.world.datums.insert(name.into(), Datum::Frame(frame));
    };
    // How long the curve where `f` and `g` cross is.
    let cross = |b: &mut Standalone, id: &str, f: &str, g: &str| -> anyhow::Result<f64> {
        CurveIntersection {
            id: id.into(),
            args: CurveIntersectionArgs {
                first: Some(Source::Topology(f.into()).into()),
                second: Some(Source::Topology(g.into()).into()),
            },
        }
        .apply(b)?;
        check(b);
        let topo = &b.world.topo;
        let mut long = 0.0;
        for &e in &topo.body(body(b, id)).expect("the curve").free_edges {
            let edge = topo.edge(e).expect("an edge");
            let curve = topo.curve(edge.curve).expect("its curve");
            let at = |k: i32| curve.point_at(edge.t0 + (edge.t1 - edge.t0) * f64::from(k) / 400.0);
            long += (0..400).map(|k| at(k).distance(at(k + 1))).sum::<f64>();
        }
        Ok(long)
    };
    let box_at = |b: &mut Standalone, id: &str, frame: &str, size: [f64; 3]| {
        AddBox {
            id: id.into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(frame)),
                size: size.map(Measure::new),
            },
        }
        .apply(b)
    };
    let cylinder = |b: &mut Standalone, id: &str, frame: &str, radius: f64, height: f64| {
        AddCylinder {
            id: id.into(),
            args: AddCylinderArgs {
                frame: Some(Reference::datum(frame)),
                radius: Measure::new(radius),
                height: Measure::new(height),
            },
        }
        .apply(b)
    };
    let mut b = Standalone::new();
    // A cylinder of radius 1 up z, and a plate tilted across it: an
    // ellipse of half-axes 1 / cos and 1.
    cylinder(&mut b, "drum", ORIGIN, 1.0, 4.0)?;
    frame(
        &mut b,
        "tilt",
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(1.0, 0.0, 0.3),
        Point3::Y,
    );
    box_at(&mut b, "plate", "tilt", [5.0, 5.0, 0.5])?;
    let a = (1.0_f64 + 0.09).sqrt();
    let ellipse = PI * (3.0 * (a + 1.0) - ((3.0 * a + 1.0) * (a + 3.0)).sqrt());
    close(
        cross(&mut b, "c1", "drum/side", "plate/top")?,
        ellipse,
        1e-3,
    );
    // A thinner cylinder across it, off its axis: two closed curves.
    frame(
        &mut b,
        "across",
        Point3::new(-3.0, 0.3, 1.5),
        Point3::Y,
        Point3::Z,
    );
    cylinder(&mut b, "pipe", "across", 0.6, 6.0)?;
    let long = cross(&mut b, "c2", "drum/side", "pipe/side")?;
    assert!(long > 2.0 * 2.0 * PI * 0.6, "{long}");
    // The side and the top of the cylinder meet along its rim.
    close(
        cross(&mut b, "c3", "drum/side", "drum/top")?,
        2.0 * PI,
        1e-3,
    );
    // A torus cut by a plane through it, a little tilted: two rings.
    frame(
        &mut b,
        "ring",
        Point3::new(10.0, 0.0, 0.0),
        Point3::X,
        Point3::Y,
    );
    AddTorus {
        id: "torus".into(),
        args: AddTorusArgs {
            frame: Some(Reference::datum("ring")),
            radius: Measure::new(2.0),
            tube: Measure::new(0.5),
        },
    }
    .apply(&mut b)?;
    frame(
        &mut b,
        "cut",
        Point3::new(10.0, 0.0, 0.1),
        Point3::new(1.0, 0.0, 0.05),
        Point3::Y,
    );
    box_at(&mut b, "slab", "cut", [8.0, 8.0, 0.2])?;
    close(
        cross(&mut b, "c4", "torus/face", "slab/top")?,
        8.0 * PI,
        0.05,
    );
    // A cylinder through its tube: a curve where it goes in, one where
    // it comes out.
    frame(
        &mut b,
        "pin",
        Point3::new(12.0, 0.0, -2.0),
        Point3::X,
        Point3::Y,
    );
    cylinder(&mut b, "rod", "pin", 0.4, 4.0)?;
    let long = cross(&mut b, "c5", "torus/face", "rod/side")?;
    assert!(long > 2.0 * 2.0 * PI * 0.4, "{long}");
    // A revolved ring and a plane square to its axis: its inner and
    // outer sides cross it in circles of radius 1 and 2, its flat ends
    // not at all.
    let mut r = Standalone::new();
    sketch(&mut r, "s", xy(), Some(([1.0, 0.0], [2.0, 1.0])), &[])?;
    let mut ring = workpiece_ops_sweep::Revolve::of("ring", Some("s".into()), false);
    ring.args.axis = Some(Reference::frame_axis(ORIGIN, Axis::Y));
    ring.apply(&mut r)?;
    frame(
        &mut r,
        "level",
        Point3::new(0.0, 0.6, 0.0),
        Point3::Z,
        Point3::X,
    );
    box_at(&mut r, "level", "level", [8.0, 8.0, 0.2])?;
    let faces: Vec<String> = r
        .world
        .topo
        .body_faces(body(&r, "ring"))
        .expect("faces")
        .iter()
        .map(|f| r.world.topo.name_of(*f).expect("a name").to_owned())
        .collect();
    let mut found = Vec::new();
    for (k, face) in faces.iter().enumerate() {
        if let Ok(long) = cross(&mut r, &format!("x{k}"), face, "level/top") {
            found.push(long);
        }
    }
    found.sort_by(f64::total_cmp);
    assert_eq!(found.len(), 2, "{found:?}");
    close(found[0], 2.0 * PI, 1e-3);
    close(found[1], 4.0 * PI, 1e-3);
    // A rounded edge of the box, and a plane square to it: a quarter
    // circle of the radius.
    let mut f = Standalone::new();
    base(&mut f)?;
    let edge = edges_where(&f, "base", |p, q| {
        p.z > 0.5 && q.z > 0.5 && p.x > 1.5 && q.x > 1.5
    });
    workpiece_ops_blend::Fillet {
        id: "fillet".into(),
        args: workpiece_ops_blend::FilletArgs {
            edges: Reference::each(edge),
            size: Measure::new(0.5),
            ..Default::default()
        },
    }
    .apply(&mut f)?;
    frame(&mut f, "upright", Point3::ZERO, Point3::X, Point3::Z);
    box_at(&mut f, "wall", "upright", [8.0, 8.0, 0.2])?;
    close(
        cross(&mut f, "arc", "fillet/e0/side/e0", "wall/top")?,
        PI / 4.0,
        1e-3,
    );
    Ok(())
}
#[test]
fn deleted_faces_are_closed_over() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    let exact = |b: &Standalone| -> anyhow::Result<f64> {
        check(b);
        Ok(
            workpiece_boolean::measure(&workpiece_boolean::read(&b.world.topo, body(b, "base"))?)
                .volume,
        )
    };
    let named = |b: &Standalone, prefix: &str| -> Vec<Source> {
        let topo = &b.world.topo;
        let faces = topo.body_faces(body(b, "base")).expect("faces");
        let names = faces.iter().filter_map(|f| topo.name_of(*f));
        names
            .filter(|n| n.starts_with(prefix))
            .map(|n| Source::Topology(n.to_owned()))
            .collect()
    };
    // The 4 × 4 × 2 box with a hole through it, a pocket in its top, or
    // a pin on its top: without those faces, the box again.
    for (extent, distance, combine) in [
        (Extent::Symmetric, 4.0, Combine::Cut),
        (Extent::Distance, 0.5, Combine::Join),
    ] {
        let mut b = Standalone::new();
        base(&mut b)?;
        sketch(
            &mut b,
            "circle",
            Reference::topology("base/top"),
            None,
            &[([0.3, 0.2], 0.5)],
        )?;
        let mut feature = Extrude::of("feature", Some("circle".into()), false);
        feature.args.extent = extent;
        feature.args.distance = Measure::new(distance);
        feature.args.combine = combine;
        feature.apply(&mut b)?;
        assert!((exact(&b)? - 32.0).abs() > 0.1);
        let faces = named(&b, "feature/");
        assert!(!faces.is_empty());
        // (Delete takes faces of a solid off and closes it over them.)
        Surface {
            id: "heal".into(),
            args: SurfaceArgs {
                way: SurfaceWay::Delete,
                picked: workpiece_core::Reference::each(faces),
                ..Default::default()
            },
        }
        .apply(&mut b)?;
        close(exact(&b)?, 32.0, 1e-9);
        let healed = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
        assert_eq!(healed.faces.len(), 6);
    }
    Ok(())
}

#[test]
fn a_curve_through_points_is_held_at_its_ends() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    use workpiece_ops_surface::{CurveEnd, CurveThrough, CurveThroughArgs, EndWay};
    // The box, and a drum of radius 1 beside it: a curve from a corner
    // of the box's top to the drum's top rim, through a point between.
    let mut b = Standalone::new();
    base(&mut b)?;
    b.world.datums.insert(
        "beside".into(),
        workpiece_core::world::Datum::Frame(workpiece_core::world::DatumFrame::new(
            Point3::new(6.0, 0.0, -1.0),
            Point3::X,
            Point3::Y,
        )?),
    );
    AddCylinder {
        id: "drum".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum("beside")),
            radius: Measure::new(1.0),
            height: Measure::new(2.0),
        },
    }
    .apply(&mut b)?;
    // Its ends: leaving the corner along the box's top front edge
    // (G1), arriving on the rim bending as it does (G2).
    let edge = edges_where(&b, "base", |p, q| {
        (p.y + 2.0).abs() < 1e-9 && (q.y + 2.0).abs() < 1e-9 && p.z > 0.5 && q.z > 0.5
    });
    let topo = b.world.topo.clone();
    let vertex = |p: Point3| {
        let found = topo
            .vertices()
            .iter()
            .find(|(_, v)| v.point.distance(p) < 1e-9);
        let id = found.map(|(id, _)| id).expect("a vertex there");
        Source::Topology(topo.name_of(id).expect("a name").to_owned())
    };
    let start = vertex(Point3::new(2.0, -2.0, 1.0));
    let end = vertex(Point3::new(7.0, 0.0, 1.0));
    let curve_of = |b: &Standalone, id: &str| {
        let topo = &b.world.topo;
        let wire = topo.body(body(b, id)).expect("the curve");
        let edge = topo.edge(wire.free_edges[0]).expect("its edge");
        (
            topo.curve(edge.curve).expect("its curve").clone(),
            (edge.t0, edge.t1),
        )
    };
    let bend_at = |curve: &workpiece_core::world::topo::Curve3, t: f64| {
        let h = 1e-5;
        let d = curve.derivative_at(t).1;
        let dd = (curve.derivative_at(t + h).1 - curve.derivative_at(t - h).1) / (2.0 * h);
        let u = d.normalize();
        (u, (dd - u * dd.dot(u)) / d.length_squared())
    };
    CurveThrough {
        id: "wire".into(),
        args: CurveThroughArgs {
            points: vec![
                start.into(),
                Source::Datum(ORIGIN.into()).into(),
                end.into(),
            ],
            start: CurveEnd {
                way: EndWay::Tangent,
                to: Some(edge[0].clone().into()),
                ..Default::default()
            },
            end: CurveEnd {
                way: EndWay::Curvature,
                to: Some(Source::Topology("drum/top-rim".into()).into()),
                ..Default::default()
            },
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let (curve, (t0, t1)) = curve_of(&b, "wire");
    // It leaves along the edge (x), and arrives along the rim, bending
    // as it does: towards the drum's axis, one over its radius.
    let (leaves, _) = bend_at(&curve, t0 + 1e-4);
    assert!(leaves.dot(Point3::X).abs() > 1.0 - 1e-6, "{leaves}");
    let (arrives, bend) = bend_at(&curve, t1 - 1e-4);
    assert!(arrives.dot(Point3::Y).abs() > 1.0 - 1e-6, "{arrives}");
    assert!(bend.distance(Point3::new(-1.0, 0.0, 0.0)) < 1e-2, "{bend}");
    // Square to the box's top at the start, lying in it at the end.
    CurveThrough {
        id: "post".into(),
        args: CurveThroughArgs {
            points: vec![
                vertex(Point3::new(2.0, -2.0, 1.0)).into(),
                Source::Datum("beside".into()).into(),
                vertex(Point3::new(-2.0, 2.0, 1.0)).into(),
            ],
            start: CurveEnd {
                way: EndWay::Normal,
                to: Some(Source::Topology("base/top".into()).into()),
                ..Default::default()
            },
            end: CurveEnd {
                way: EndWay::InFace,
                to: Some(Source::Topology("base/top".into()).into()),
                ..Default::default()
            },
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let (curve, (t0, t1)) = curve_of(&b, "post");
    let (leaves, _) = bend_at(&curve, t0 + 1e-6);
    assert!(leaves.dot(Point3::Z).abs() > 1.0 - 1e-4, "{leaves}");
    let (arrives, _) = bend_at(&curve, t1 - 1e-6);
    assert!(arrives.z.abs() < 1e-4, "{arrives}");
    Ok(())
}

/// The faces of body `name` of `b` whose middle and outward normal
/// `pick` takes, by their names.
fn faces_where(
    b: &Standalone,
    name: &str,
    pick: impl Fn(workpiece_core::math::Point3, workpiece_core::math::Point3) -> bool,
) -> Vec<Source> {
    let solid = workpiece_boolean::read(&b.world.topo, body(b, name)).expect("a solid");
    (0..solid.faces.len())
        .filter(|&f| {
            let (at, out) = workpiece_shape::on::middle(&solid, f);
            pick(at, out)
        })
        .map(|f| Source::Topology(solid.faces[f].name.clone().expect("a name")))
        .collect()
}

/// The names of the free edges of surface `name` of `b`.
fn free_edges(b: &Standalone, name: &str) -> Vec<Source> {
    let sheet = workpiece_boolean::read_sheet(&b.world.topo, body(b, name)).expect("a surface");
    let mut walked = vec![0; sheet.edges.len()];
    for step in sheet.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        if let workpiece_boolean::model::Walk::Edge { edge, .. } = step.walk {
            walked[edge] += 1;
        }
    }
    (0..sheet.edges.len())
        .filter(|&e| walked[e] == 1)
        .map(|e| Source::Topology(sheet.edges[e].name.clone().expect("a name")))
        .collect()
}

#[test]
fn a_five_sided_hole_is_filled_running_on_from_its_walls() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_ops_surface::{
        Boundary, BoundaryArgs, BoundarySide, Continuity, Surface, SurfaceArgs, SurfaceWay,
    };
    // A pentagonal prism 3 high, its walls and floor as a surface: the
    // hole at its top has five sides.
    let mut b = Standalone::new();
    let mut sketch = Sketch::new("pentagon");
    sketch.args.plane = Some(xy());
    let corners: Vec<_> = (0..5)
        .map(|k| {
            let a = std::f64::consts::TAU * f64::from(k) / 5.0;
            sketch
                .args
                .add_point(Point2::new(2.0 * libm::cos(a), 2.0 * libm::sin(a)))
        })
        .collect();
    for k in 0..5 {
        sketch.args.add_line(corners[k], corners[(k + 1) % 5]);
    }
    sketch.apply(&mut b)?;
    let mut prism = Extrude::of("prism", Some("pentagon".into()), false);
    prism.args.distance = Measure::new(3.0);
    prism.args.combine = Combine::NewBody;
    prism.apply(&mut b)?;
    let kept = faces_where(&b, "prism", |at, _| at.z < 2.9);
    assert_eq!(kept.len(), 6);
    Surface {
        id: "cup".into(),
        args: SurfaceArgs {
            way: SurfaceWay::Offset,
            distance: Measure::new(0.0),
            picked: kept.into_iter().map(Into::into).collect(),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let rim = free_edges(&b, "cup");
    assert_eq!(rim.len(), 5);
    // Filled, tangent to the walls all around: a dome, sewn to the cup
    // into a solid.
    Boundary {
        id: "lid".into(),
        args: BoundaryArgs {
            sides: rim
                .iter()
                .map(|edge| BoundarySide {
                    way: Continuity::Tangent,
                    ..BoundarySide::free(edge.clone().into())
                })
                .collect(),
        },
    }
    .apply(&mut b)?;
    check(&b);
    assert_eq!(
        b.world.topo.body(body(&b, "cup")).map(|b| b.kind),
        Some(BodyKind::Solid)
    );
    let cup = workpiece_boolean::read(&b.world.topo, body(&b, "cup"))?;
    let lid = cup.clone();
    let at = (lid.faces.iter())
        .position(|f| f.name.as_deref() == Some("lid/face"))
        .expect("the lid's face");
    // Along the rim the lid leaves as the walls do: its normal along
    // theirs within 1.5° over the middle of each side, and it rises
    // above the rim.
    let face = &lid.faces[at];
    let mut worst = 0.0_f64;
    for step in &face.loops[0] {
        let workpiece_boolean::model::Walk::Edge { edge, .. } = step.walk else {
            continue;
        };
        let (t0, t1) = lid.edges[edge].range;
        // (Away from the corners: there the walls lean to each other,
        // and no surface leaves both as they do.)
        for k in 3..=7 {
            let t = t0 + (t1 - t0) * f64::from(k) / 10.0;
            let uv = step.pcurve.point_at(t);
            let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
            let normal = du.cross(dv).normalize();
            let p = lid.edges[edge].curve.point_at(t);
            let wall = (0..cup.faces.len())
                .find(|&f| workpiece_boolean::lies_on(&cup, f, p - Point3::Z * 0.01, 1e-6))
                .expect("the wall below");
            let (_, out) = workpiece_shape::on::middle(&cup, wall);
            worst = worst.max(libm::acos(normal.dot(out).abs().min(1.0)).to_degrees());
        }
    }
    assert!(worst < 1.5, "{worst}°");
    let top = face.surface.point_at(0.5, 0.5);
    assert!(top.z > 3.3, "{top}");
    Ok(())
}

#[test]
fn six_sheets_turned_any_way_are_copied_into_an_outward_solid() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    // The box's six faces, each a surface of its own, three of them
    // turned to face in.
    let mut b = Standalone::new();
    base(&mut b)?;
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    let mut names = Vec::new();
    for f in 0..6 {
        let mut sheet = workpiece_shape::faces_of(&solid, &[f]);
        if f % 2 == 0 {
            sheet.faces[0].outward = !sheet.faces[0].outward;
        }
        let name = format!("sheet{f}");
        workpiece_boolean::write_sheet(&mut b.world.topo, &sheet, &name)?;
        names.push(Source::Topology(name).into());
    }
    Surface {
        id: "sewn".into(),
        args: SurfaceArgs {
            way: SurfaceWay::Offset,
            distance: Measure::new(0.0),
            picked: names,
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let sewn = workpiece_boolean::read(&b.world.topo, body(&b, "sewn"))?;
    // Facing out: its volume as much as the box's, not less than none.
    close(workpiece_boolean::measure(&sewn).volume, 32.0, 1e-9);
    let (volume, _) = sound(&sewn)?;
    close(volume, 32.0, 1e-9);
    Ok(())
}

#[test]
fn a_fillet_rounds_an_edge_of_a_surface() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    // The box's top and front as one surface: the Fillet step rounds
    // the edge between them as it would a solid's.
    let mut b = Standalone::new();
    base(&mut b)?;
    Surface {
        id: "skin".into(),
        args: SurfaceArgs {
            way: SurfaceWay::Offset,
            distance: Measure::new(0.0),
            picked: vec![
                Source::Topology("base/top".into()).into(),
                Source::Topology("base/front".into()).into(),
            ],
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let sheet = workpiece_boolean::read_sheet(&b.world.topo, body(&b, "skin"))?;
    let inner: Vec<Source> = free_edges(&b, "skin");
    let between = sheet
        .edges
        .iter()
        .filter_map(|e| e.name.clone())
        .map(Source::Topology)
        .find(|s| !inner.contains(s))
        .expect("the edge between them");
    let r = 0.5;
    workpiece_ops_blend::Fillet {
        id: "round".into(),
        args: workpiece_ops_blend::FilletArgs {
            edges: Reference::each([between]),
            size: Measure::new(r),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let made = workpiece_boolean::read_sheet(&b.world.topo, body(&b, "skin"))?;
    assert_eq!(
        b.world.topo.body(body(&b, "skin")).map(|b| b.kind),
        Some(BodyKind::Sheet)
    );
    assert_eq!(made.faces.len(), 3);
    let area: f64 = (0..made.faces.len())
        .map(|f| workpiece_boolean::face_area(&made, f))
        .sum();
    close(
        area,
        (4.0 - r) * 4.0 + (2.0 - r) * 4.0 + PI * r / 2.0 * 4.0,
        1e-6,
    );
    Ok(())
}

#[test]
fn a_loft_follows_its_guides() -> anyhow::Result<()> {
    use std::sync::Arc;
    use workpiece_core::math::Point3;
    use workpiece_ops_surface::{EndHeld, through, write_wire};
    use workpiece_ops_sweep::{Rail, Role, Section, SweptLoft, SweptLoftArgs};
    // Two squares 2 across, 4 apart up z; two guides from corner to
    // corner bulging out between them, on opposite corners.
    let mut b = Standalone::new();
    b.world.datums.insert(
        "up".into(),
        workpiece_core::world::Datum::Frame(workpiece_core::world::DatumFrame::new(
            Point3::new(0.0, 0.0, 4.0),
            Point3::X,
            Point3::Y,
        )?),
    );
    sketch(&mut b, "low", xy(), Some(([-1.0, -1.0], [1.0, 1.0])), &[])?;
    let up = Reference::frame_axis("up", Axis::Z);
    sketch(&mut b, "high", up, Some(([-1.0, -1.0], [1.0, 1.0])), &[])?;
    let guides = [
        [
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(1.8, 1.8, 2.0),
            Point3::new(1.0, 1.0, 4.0),
        ],
        [
            Point3::new(-1.0, -1.0, 0.0),
            Point3::new(-1.5, -1.5, 1.5),
            Point3::new(-1.0, -1.0, 4.0),
        ],
    ];
    let mut curves = Vec::new();
    for (k, points) in guides.iter().enumerate() {
        let curve = through(points, false, [EndHeld::default(); 2])?;
        let range = curve.domain();
        write_wire(
            &mut b.world.topo,
            &format!("guide{k}"),
            &[(Arc::new(curve.clone()), range)],
        )?;
        curves.push(curve);
    }
    SweptLoft {
        id: "vase".into(),
        args: SweptLoftArgs {
            sections: vec![
                Section::of(Source::Profile("low".into())),
                Section::of(Source::Profile("high".into())),
            ],
            rails: (0..2)
                .map(|k| Rail {
                    of: Source::Topology(format!("guide{k}")).into(),
                    role: Role::Guide,
                })
                .collect(),
            ..SweptLoftArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let vase = workpiece_boolean::read(&b.world.topo, body(&b, "vase"))?;
    // Every place along the guides is on the loft's side.
    for curve in &curves {
        let (a, z) = curve.domain();
        for k in 0..=20 {
            let p = curve.point_at(a + (z - a) * f64::from(k) / 20.0);
            let on = (0..vase.faces.len()).any(|f| workpiece_boolean::lies_on(&vase, f, p, 1e-3));
            assert!(on, "{p} is off the loft");
        }
    }
    // Bulging out between the squares: more than the 16 of a prism.
    assert!(workpiece_boolean::measure(&vase).volume > 18.0);
    Ok(())
}

#[test]
fn edges_and_curves_in_space_are_extruded_and_revolved() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Helix, HelixArgs};
    use workpiece_ops_sweep::Revolve;
    let sheet_area = |b: &Standalone, name: &str| -> anyhow::Result<f64> {
        let sheet = workpiece_boolean::read_sheet(&b.world.topo, body(b, name))?;
        Ok((0..sheet.faces.len())
            .map(|f| workpiece_boolean::face_area(&sheet, f))
            .sum())
    };
    let mut b = Standalone::new();
    base(&mut b)?;
    // The box's top front edge, 4 long: on its own it lies in no plane,
    // so it is extruded along a direction picked — y, by 3: 4 × 3.
    let front = edges_where(&b, "base", |p, q| {
        (p.y + 2.0).abs() < 1e-9 && (q.y + 2.0).abs() < 1e-9 && p.z > 0.9 && q.z > 0.9
    });
    assert_eq!(front.len(), 1);
    let mut wall = Extrude::of("wall", None, true);
    wall.args.curves = vec![front[0].clone().into()];
    wall.args.distance = Measure::new(3.0);
    let refused = wall.clone().apply(&mut b).expect_err("no direction");
    assert!(format!("{refused:#}").contains("direction"), "{refused:#}");
    wall.args.direction = Some(Reference::frame_axis(ORIGIN, Axis::Y));
    anyhow::Context::context(wall.apply(&mut b), "the wall")?;
    check(&b);
    close(sheet_area(&b, "wall")?, 12.0, 1e-9);
    // As a solid an open edge is refused.
    let mut solid = Extrude::of("solid", None, false);
    solid.args.curves = vec![front[0].clone().into()];
    solid.args.direction = Some(Reference::frame_axis(ORIGIN, Axis::Y));
    let refused = solid.apply(&mut b).expect_err("an open edge");
    assert!(format!("{refused:#}").contains("surface"), "{refused:#}");
    // An upright edge of the box (2 high, √8 from z) turned all the way
    // about z: a cylinder's side.
    let upright = edges_where(&b, "base", |p, q| {
        (p.x + 2.0).abs() < 1e-9
            && (q.x + 2.0).abs() < 1e-9
            && (p.y + 2.0).abs() < 1e-9
            && (q.y + 2.0).abs() < 1e-9
    });
    assert_eq!(upright.len(), 1);
    let mut drum = Revolve::of("drum", None, true);
    drum.args.curves = vec![upright[0].clone().into()];
    drum.args.axis = Some(Reference::frame_axis(ORIGIN, Axis::Z));
    anyhow::Context::context(drum.apply(&mut b), "the drum")?;
    check(&b);
    close(
        sheet_area(&b, "drum")?,
        2.0 * PI * 8.0_f64.sqrt() * 2.0,
        1e-6,
    );
    // A helix in space, extruded along z and turned a quarter about x.
    Helix {
        id: "coil".into(),
        args: HelixArgs {
            axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            radius: Measure::new(1.0),
            pitch: Measure::new(1.0),
            turns: Measure::new(2.0),
            ..HelixArgs::default()
        },
    }
    .apply(&mut b)?;
    let coil = || -> Vec<Reference<workpiece_core::Anything>> {
        vec![Source::Topology("coil".into()).into()]
    };
    let mut ribbon = Extrude::of("ribbon", None, true);
    ribbon.args.curves = coil();
    ribbon.args.direction = Some(Reference::frame_axis(ORIGIN, Axis::Z));
    ribbon.args.distance = Measure::new(0.25);
    anyhow::Context::context(ribbon.apply(&mut b), "the ribbon")?;
    check(&b);
    assert!(sheet_area(&b, "ribbon")? > 0.25 * 2.0 * PI * 2.0 * 0.99);
    let mut turned = Revolve::of("turned", None, true);
    turned.args.curves = coil();
    turned.args.axis = Some(Reference::frame_axis(ORIGIN, Axis::X));
    turned.args.angle = Measure::new(90.0);
    anyhow::Context::context(turned.apply(&mut b), "turned")?;
    check(&b);
    assert!(sheet_area(&b, "turned")? > 0.0);
    Ok(())
}
