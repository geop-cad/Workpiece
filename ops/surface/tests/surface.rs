//! What the operations of this crate make.

use workpiece_ops_sweep::{Extent, Extrude};
use workpiece_ops_testing::*;
use workpiece_shape::Combine;

#[test]
fn surfaces_are_copied_sewn_and_taken_apart() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    let act = |b: &mut Standalone, id: &str, way: SurfaceWay, picked: &[&str]| {
        Surface {
            id: id.into(),
            args: SurfaceArgs {
                way,
                picked: picked
                    .iter()
                    .map(|p| Source::Topology((*p).to_owned()).into())
                    .collect(),
                distance: Measure::new(0.5),
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
        SurfaceWay::Copy,
        &["base/top", "base/front"],
    )?;
    check(&b);
    assert_eq!(
        (kind(&b, "skin"), faces(&b, "skin")),
        (Some(BodyKind::Sheet), Some(2))
    );
    let skin = workpiece_boolean::read_sheet(&b.world.topo, body(&b, "skin"))?;
    assert_eq!(skin.edges.len(), 7);
    // All of the box as a surface, taken apart into six, and sewn again:
    // closed all around, a solid of the box's volume.
    act(&mut b, "all", SurfaceWay::Copy, &["base"])?;
    act(&mut b, "piece", SurfaceWay::Unstitch, &["all"])?;
    check(&b);
    let pieces: Vec<String> = (1..=6).map(|k| format!("piece/{k}")).collect();
    let pieces: Vec<&str> = pieces.iter().map(String::as_str).collect();
    assert!(pieces.iter().all(|p| faces(&b, p) == Some(1)));
    act(&mut b, "again", SurfaceWay::Stitch, &pieces)?;
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
    act(&mut b, "off", SurfaceWay::Offset, &["again"])?;
    check(&b);
    assert_eq!(faces(&b, "off"), Some(6));
    let off = workpiece_boolean::read_sheet(&b.world.topo, body(&b, "off"))?;
    let area: f64 = (0..6).map(|f| workpiece_boolean::face_area(&off, f)).sum();
    close(area, 64.0, 1e-9);
    // A face deleted from the two-faced surface leaves one.
    act(&mut b, "less", SurfaceWay::Delete, &["skin/face1"]).or_else(|_| {
        let topo = &b.world.topo;
        let first = topo.body_faces(body(&b, "skin")).expect("faces")[0];
        let name = topo.name_of(first).expect("a name").to_owned();
        act(&mut b, "less", SurfaceWay::Delete, &[&name])
    })?;
    check(&b);
    assert_eq!(faces(&b, "skin"), Some(1));
    Ok(())
}

#[test]
fn flat_holes_of_surfaces_are_filled() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    // A tube of radius 0.5, 2 long, open at both ends: filled, a rod.
    let mut b = Standalone::new();
    sketch(&mut b, "s", xy(), None, &[([0.0, 0.0], 0.5)])?;
    let mut tube = Extrude::of("tube", Some("s".into()), true);
    tube.args.distance = Measure::new(2.0);
    tube.apply(&mut b)?;
    Surface {
        id: "fill".into(),
        args: SurfaceArgs {
            way: SurfaceWay::Fill,
            picked: vec![Source::Topology("tube".into()).into()],
            ..SurfaceArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
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
fn edges_sweep_ruled_surfaces() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    // Two edges of the box's top that meet, run 3 up: two faces of one
    // surface, sharing the edge above the corner.
    let mut b = Standalone::new();
    base(&mut b)?;
    let top = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    let face = top
        .faces
        .iter()
        .find(|f| f.name.as_deref() == Some("base/top"))
        .expect("the top");
    let edges: Vec<String> = face.loops[0]
        .iter()
        .take(2)
        .filter_map(|step| match step.walk {
            workpiece_boolean::model::Walk::Edge { edge, .. } => top.edges[edge].name.clone(),
            workpiece_boolean::model::Walk::Pole { .. } => None,
        })
        .collect();
    assert_eq!(edges.len(), 2);
    Surface {
        id: "fence".into(),
        args: SurfaceArgs {
            way: SurfaceWay::Ruled,
            picked: edges
                .into_iter()
                .map(|n| Source::Topology(n).into())
                .collect(),
            distance: Measure::new(3.0),
            axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            ..SurfaceArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let fence = workpiece_boolean::read_sheet(&b.world.topo, body(&b, "fence"))?;
    assert_eq!((fence.faces.len(), fence.edges.len()), (2, 7));
    let area: f64 = (0..2)
        .map(|f| workpiece_boolean::face_area(&fence, f))
        .sum();
    close(area, 24.0, 1e-9);
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
    act(&mut b, "skin", SurfaceWay::Copy, "base/top", 0.0)?;
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
    act(&mut b, "skin", SurfaceWay::Copy, "base/top", 0.0)?;
    close(area(&b, "skin")?, 16.0 - PI * 0.25, 1e-9);
    act(&mut b, "all", SurfaceWay::Untrim, "skin", 0.0)?;
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
fn surfaces_are_trimmed_and_split_by_planes() -> anyhow::Result<()> {
    use workpiece_ops_surface::{Surface, SurfaceArgs, SurfaceWay};
    let act = |b: &mut Standalone, id: &str, way: SurfaceWay, picked: &str, plane: bool| {
        Surface {
            id: id.into(),
            args: SurfaceArgs {
                way,
                picked: vec![Source::Topology(picked.to_owned()).into()],
                distance: Measure::new(0.0),
                axis: None,
                plane: plane.then(|| Reference::frame_axis(ORIGIN, Axis::X)),
                flip: false,
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
    act(&mut b, "skin", SurfaceWay::Copy, "base/top", false)?;
    let whole = area(&b, "skin")?;
    act(&mut b, "cut", SurfaceWay::Split, "skin", true)?;
    check(&b);
    close(area(&b, "skin")? + area(&b, "cut")?, whole, 1e-9);
    let behind = area(&b, "skin")?;
    assert!(behind > 0.0 && behind < whole);
    // A tube of radius 0.5, 2 long, about z: trimmed to the plane
    // x = 0, half of its wall is left.
    let mut t = Standalone::new();
    sketch(&mut t, "s", xy(), None, &[([0.0, 0.0], 0.5)])?;
    let mut tube = Extrude::of("tube", Some("s".into()), true);
    tube.args.distance = Measure::new(2.0);
    tube.apply(&mut t)?;
    act(&mut t, "half", SurfaceWay::Trim, "tube", true)?;
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
    act(&mut b, "gone", SurfaceWay::Trim, "cut", true)?;
    assert!(
        b.world
            .topo
            .resolve::<workpiece_core::world::topo::BodyId>("cut")
            .is_none(),
        "all of it was in front"
    );
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
    use workpiece_ops_surface::{Curve, CurveArgs, CurveWay, through};
    // A curve through points goes through them.
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 2.0, 0.5),
        Point3::new(3.0, 2.5, 2.0),
        Point3::new(4.0, 0.0, 3.0),
    ];
    let curve = through(&points)?;
    for (k, p) in points.iter().enumerate() {
        assert!(curve.point_at(k as f64).distance(*p) < 1e-12);
    }
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
    Curve {
        id: "coil".into(),
        args: CurveArgs {
            way: CurveWay::Helix,
            axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            radius: Measure::new(2.0),
            pitch: Measure::new(0.5),
            ..CurveArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let (from, to) = wire(&b, "coil")[0];
    close((to - from).z, 1.5, 1e-9);
    close(from.truncate().length(), 2.0, 1e-9);
    // A flat spiral from radius 1, growing 0.5 a turn, for 2 turns:
    // it ends 2 out, where it started along.
    Curve {
        id: "spiral".into(),
        args: CurveArgs {
            way: CurveWay::Helix,
            axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            radius: Measure::new(1.0),
            pitch: Measure::new(0.0),
            turns: Measure::new(2.0),
            grow: Measure::new(0.5),
            ..CurveArgs::default()
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
    Curve {
        id: "edge".into(),
        args: CurveArgs {
            way: CurveWay::Intersection,
            picked: vec![
                Source::Topology("base/top".into()).into(),
                Source::Topology("base/front".into()).into(),
            ],
            ..CurveArgs::default()
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
    Curve {
        id: "shadow".into(),
        args: CurveArgs {
            way: CurveWay::Projection,
            picked: vec![
                Source::Topology("over".into()).into(),
                Source::Topology("base/top".into()).into(),
            ],
            axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            ..CurveArgs::default()
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
    let bent = through(&[
        Point3::ZERO,
        Point3::new(0.5, 0.0, 2.0),
        Point3::new(2.0, 1.0, 4.0),
    ])?;
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
    sweep.args.sections = vec![Source::Profile("disk".into()).into()];
    sweep.args.path = Some(Source::Topology("way".into()).into());
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
    Curve {
        id: "cable".into(),
        args: CurveArgs {
            way: CurveWay::Spline,
            picked: vec![
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
    let old = r#"{"id":"s","way":"Stitch","args":{"picked":[]}}"#;
    let read: Surface = serde_json::from_str(old)?;
    assert_eq!(read.args.way, SurfaceWay::Stitch);
    // As it is written now, and read back the same.
    let text = serde_json::to_string(&read)?;
    assert!(!text.contains(r#""way":"Stitch","args""#), "{text}");
    assert_eq!(serde_json::from_str::<Surface>(&text)?, read);
    Ok(())
}
