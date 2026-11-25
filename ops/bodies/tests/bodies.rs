//! What the operations of this crate make.

use workpiece_ops_bodies::{CombineArgs, CombineBodies};
use workpiece_ops_sweep::{Extrude, Revolve};
use workpiece_ops_testing::*;
use workpiece_shape::Combine;

#[test]
fn bodies_combine_whatever_their_faces_are() -> anyhow::Result<()> {
    let mut b = Standalone::new();
    base(&mut b)?;
    sketch(&mut b, "s", xy(), Some(([0.0; 2], [3.0; 2])), &[])?;
    let mut tool = Extrude::of("tool", Some("s".into()), false);
    tool.args.combine = workpiece_shape::Combine::NewBody;
    tool.args.distance = Measure::new(3.0);
    tool.apply(&mut b)?;
    let combine = |b: &mut Standalone, how| {
        CombineBodies {
            id: "combine".into(),
            args: CombineArgs {
                target: Some(Source::Topology("base".into()).into()),
                tool: Some(Source::Topology("tool".into()).into()),
                how,
            },
        }
        .apply(b)
    };
    // The tool overlaps the box in 2 × 2 × 1.
    let mut cut = b.clone();
    combine(&mut cut, Combine::Cut)?;
    check(&cut);
    close(volume(&cut.world.topo, body(&cut, "base")), 28.0, 1e-9);
    assert!(cut.world.topo.lookup("tool").is_none());
    let mut joined = b.clone();
    combine(&mut joined, Combine::Join)?;
    close(
        volume(&joined.world.topo, body(&joined, "base")),
        32.0 + 27.0 - 4.0,
        1e-9,
    );
    let mut common = b.clone();
    combine(&mut common, Combine::Intersect)?;
    close(volume(&common.world.topo, body(&common, "base")), 4.0, 1e-9);

    // A cone (a slanted line turned) is a body like any other.
    let mut b = Standalone::new();
    base(&mut b)?;
    let mut slanted = Sketch::new("s");
    slanted.args.plane = Some(xy());
    let points =
        [[0.0, 0.0], [1.0, 0.0], [0.0, 2.0]].map(|p| slanted.args.add_point(Point2::from_array(p)));
    for k in 0..3 {
        slanted.args.add_line(points[k], points[(k + 1) % 3]);
    }
    slanted.apply(&mut b)?;
    let mut cone = Revolve::of("cone", Some("s".into()), false);
    cone.args.combine = workpiece_shape::Combine::NewBody;
    cone.args.axis = Some(Reference::frame_axis(ORIGIN, Axis::Y));
    let mut alone = b.clone();
    cone.apply(&mut alone)?;
    check(&alone);
    close(
        volume(&alone.world.topo, body(&alone, "cone")),
        2.0 * PI / 3.0,
        0.02,
    );
    // It lies in the box, touching its top and bottom with its rim and
    // a side with its tip: joined it adds nothing, cut out it leaves a
    // hollow.
    let mut joined = b.clone();
    cone.args.combine = Combine::Join;
    cone.apply(&mut joined)?;
    check(&joined);
    close(
        volume(&joined.world.topo, body(&joined, "base")),
        32.0,
        1e-9,
    );
    cone.args.combine = Combine::Intersect;
    cone.apply(&mut b)?;
    check(&b);
    close(
        volume(&b.world.topo, body(&b, "base")),
        2.0 * PI / 3.0,
        0.02,
    );
    Ok(())
}

#[test]
fn bodies_are_moved_mirrored_scaled_and_repeated() -> anyhow::Result<()> {
    use workpiece_ops_bodies::{
        Along, Motion, Pattern, PatternArgs, Repeat, Transform, TransformArgs,
    };
    let z = || Some(Reference::frame_axis(ORIGIN, Axis::Z));
    let x = || Some(Reference::frame_axis(ORIGIN, Axis::X));
    let base_of = |_: &Standalone| Some(Source::Topology("base".into()));
    let transform = |b: &mut Standalone, id: &str, set: &dyn Fn(&mut TransformArgs)| {
        let mut args = TransformArgs {
            body: base_of(b).map(Into::into),
            ..Default::default()
        };
        set(&mut args);
        Transform {
            id: id.into(),
            args,
        }
        .apply(b)
    };
    // A copy moved: both are there, the copy where it was moved to.
    let mut b = Standalone::new();
    base(&mut b)?;
    transform(&mut b, "copy", &|a| {
        a.by = [10.0, 0.0, 0.0].map(Measure::new);
        a.copy = true;
    })?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "copy")), 32.0, 1e-9);
    let moved = workpiece_boolean::read(&b.world.topo, body(&b, "copy"))?;
    assert!(moved.vertices.iter().all(|v| v.point.x > 7.9));
    // Turned, mirrored and scaled, it is still a solid: mirrored, with
    // its inside still inside.
    transform(&mut b, "t", &|a| {
        a.motion = Motion::Rotate;
        a.axis = x();
        a.angle = Measure::new(30.0);
    })?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "base")), 32.0, 1e-9);
    transform(&mut b, "t", &|a| {
        a.motion = Motion::Mirror;
        a.plane = Some(Reference::frame_axis(ORIGIN, Axis::Y));
    })?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "base")), 32.0, 1e-9);
    transform(&mut b, "t", &|a| {
        a.motion = Motion::Scale;
        a.factor = Measure::new(0.5);
    })?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "base")), 4.0, 1e-9);

    // Three along x, three apart, joined: one bar 10 long.
    let pattern = |b: &mut Standalone, set: &dyn Fn(&mut PatternArgs)| {
        let mut args = PatternArgs {
            of: Repeat::Bodies,
            bodies: base_of(b).map(Into::into).into_iter().collect(),
            ..Default::default()
        };
        set(&mut args);
        Pattern {
            id: "pattern".into(),
            args,
        }
        .apply(b)
    };
    let mut b = Standalone::new();
    base(&mut b)?;
    pattern(&mut b, &|a| {
        a.axis = x();
        a.spacing = Measure::new(3.0);
        a.combine = Combine::Join;
    })?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "base")), 80.0, 1e-9);
    // How far apart they are is said, along x.
    assert_eq!(b.world.annotations["pattern/spacing"].text(), "3");
    // Four around z, as bodies of their own.
    let mut b = Standalone::new();
    base(&mut b)?;
    transform(&mut b, "t", &|a| a.by = [6.0, 0.0, 0.0].map(Measure::new))?;
    pattern(&mut b, &|a| {
        a.along = Along::Circle;
        a.axis = z();
        a.count = 4;
    })?;
    check(&b);
    for k in 1..4 {
        close(
            volume(&b.world.topo, body(&b, &format!("pattern/{k}"))),
            32.0,
            1e-9,
        );
    }
    let opposite = workpiece_boolean::read(&b.world.topo, body(&b, "pattern/2"))?;
    assert!(opposite.vertices.iter().all(|v| v.point.x < -3.9));
    // (All the way around: no angle to say.)
    assert!(!b.world.annotations.contains_key("pattern/angle"));

    // Moved 2 along −x first, so it touches the plane x = 0, and
    // mirrored in that: a body of its own on the other side, or one with
    // it (64 in all).
    use workpiece_ops_bodies::{Mirror, MirrorArgs};
    for (how, called, all) in [
        (Combine::NewBody, "mirror", 32.0),
        (Combine::Join, "base", 64.0),
    ] {
        let mut b = Standalone::new();
        base(&mut b)?;
        transform(&mut b, "t", &|a| a.by = [-2.0, 0.0, 0.0].map(Measure::new))?;
        Mirror {
            id: "mirror".into(),
            args: MirrorArgs {
                of: Repeat::Bodies,
                bodies: vec![Source::Topology("base".into()).into()],
                plane: Some(Reference::frame_axis(ORIGIN, Axis::X)),
                combine: how,
                ..Default::default()
            },
        }
        .apply(&mut b)?;
        check(&b);
        close(volume(&b.world.topo, body(&b, called)), all, 1e-9);
        if how == Combine::NewBody {
            let image = workpiece_boolean::read(&b.world.topo, body(&b, "mirror"))?;
            assert!(image.vertices.iter().all(|v| v.point.x > -1e-9));
        }
    }

    // A small block filling the top of a big one: a square grid 3 apart
    // from where the block is, a margin of 1 in from the top's edges.
    let mut b = Standalone::new();
    AddBox {
        id: "plate".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [20.0, 20.0, 2.0].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    AddBox {
        id: "base".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [1.0, 1.0, 1.0].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    pattern(&mut b, &|a| {
        a.along = Along::Fill;
        a.face = Some(Source::Topology("plate/top".into()).into());
        a.spacing = Measure::new(3.0);
        a.margin = Measure::new(1.0);
    })?;
    check(&b);
    // The block reaches 0.71 from its middle: places at most 8.29 from
    // the middle of the plate, −6 … 6 both ways: 5 × 5 less itself.
    let copies = (1..)
        .take_while(|k| {
            b.world
                .topo
                .resolve::<BodyId>(&format!("pattern/{k}"))
                .is_some()
        })
        .count();
    assert_eq!(copies, 24);
    Ok(())
}

#[test]
fn bodies_are_split_by_planes() -> anyhow::Result<()> {
    use workpiece_ops_bodies::{Keep, Split, SplitArgs};
    // The 4 × 4 × 2 box, by the plane x = 0 and by the plane y = 0.
    let split = |keep: Keep, axis: Axis| -> anyhow::Result<Standalone> {
        let mut b = Standalone::new();
        base(&mut b)?;
        Split {
            id: "half".into(),
            args: SplitArgs {
                target: None,
                by: vec![Reference::frame_axis(ORIGIN, axis)],
                keep,
            },
        }
        .apply(&mut b)?;
        check(&b);
        Ok(b)
    };
    let b = split(Keep::Both, Axis::X)?;
    close(volume(&b.world.topo, body(&b, "base")), 16.0, 1e-6);
    close(volume(&b.world.topo, body(&b, "half")), 16.0, 1e-6);
    for keep in [Keep::Front, Keep::Behind] {
        let b = split(keep, Axis::Y)?;
        close(volume(&b.world.topo, body(&b, "base")), 16.0, 1e-6);
        assert!(b.world.topo.resolve::<BodyId>("half").is_none());
    }
    Ok(())
}

#[test]
fn a_body_is_split_by_a_surface_and_a_half_deleted() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_core::world::topo::Curve3;
    use workpiece_ops_bodies::{DeleteBody, DeleteBodyArgs, Keep, Split, SplitArgs};
    let mut b = Standalone::new();
    base(&mut b)?;
    // A bent surface through the 4 × 4 × 2 box, from below it to above.
    let bend = Curve3::new(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(-5.0, -1.0, -5.0),
            Point3::new(0.0, 2.0, -5.0),
            Point3::new(5.0, -1.0, -5.0),
        ],
        None,
    )?;
    let sheet =
        workpiece_shape::surface::ruled(&bend, (0.0, 1.0), Point3::new(0.0, 0.0, 10.0), "bent")?;
    workpiece_boolean::write_sheet(&mut b.world.topo, &sheet, "bent")?;
    let face = b
        .world
        .topo
        .name_of(b.world.topo.body_faces(body(&b, "bent")).unwrap()[0]);
    let face = face.unwrap().to_owned();
    // By the surface as a body, and by its face: the same.
    for by in ["bent".to_owned(), face] {
        let mut b = b.clone();
        Split {
            id: "half".into(),
            args: SplitArgs {
                target: None,
                by: vec![Source::Topology(by).into()],
                keep: Keep::Both,
            },
        }
        .apply(&mut b)?;
        check(&b);
        let (front, behind) = (
            volume(&b.world.topo, body(&b, "base")),
            volume(&b.world.topo, body(&b, "half")),
        );
        close(front + behind, 32.0, 1e-6);
        assert!(front > 1.0 && behind > 1.0 && (front - behind).abs() > 0.1);
        // The half and the surface go; the rest stays.
        DeleteBody {
            id: "delete".into(),
            args: DeleteBodyArgs {
                bodies: vec![
                    Source::Topology("half".into()).into(),
                    Source::Topology("bent".into()).into(),
                ],
            },
        }
        .apply(&mut b)?;
        assert!(b.world.topo.resolve::<BodyId>("half").is_none());
        assert!(b.world.topo.resolve::<BodyId>("bent").is_none());
        close(volume(&b.world.topo, body(&b, "base")), front, 1e-9);
    }
    Ok(())
}

#[test]
fn bodies_are_repeated_along_paths_and_at_points() -> anyhow::Result<()> {
    use workpiece_ops_bodies::{Along, Pattern, PatternArgs, Repeat};
    // A small box at the origin; a path from there along x and then y.
    let small = |b: &mut Standalone| {
        AddBox {
            id: "peg".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(ORIGIN)),
                size: [0.4, 0.2, 0.2].map(Measure::new),
            },
        }
        .apply(b)
    };
    let centre = |b: &Standalone, name: &str| -> anyhow::Result<workpiece_core::math::Point3> {
        let solid = workpiece_boolean::read(&b.world.topo, body(b, name))?;
        let n = solid.vertices.len() as f64;
        Ok(solid
            .vertices
            .iter()
            .map(|v| v.point)
            .sum::<workpiece_core::math::Point3>()
            / n)
    };
    let mut b = Standalone::new();
    small(&mut b)?;
    let mut path = Sketch::new("path");
    path.args.plane = Some(xy());
    let corners =
        [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]].map(|p| path.args.add_point(Point2::from_array(p)));
    path.args.add_line(corners[0], corners[1]);
    path.args.add_line(corners[1], corners[2]);
    path.apply(&mut b)?;
    Pattern {
        id: "row".into(),
        args: PatternArgs {
            of: Repeat::Bodies,
            bodies: vec![Source::Topology("peg".into()).into()],
            along: Along::Path,
            sketch: Some(Source::Profile("path".into()).into()),
            count: 5,
            ..PatternArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    // What each copy is made of is called after the copy: a face of the
    // third is found by that name, whatever the others are.
    let faces = |of: &str| -> Vec<String> {
        let names = b.world.topo.names();
        let all = names
            .iter()
            .filter(|(_, id)| workpiece_core::world::topo::FaceId::try_from(*id).is_ok());
        let its = all.filter(|(n, _)| n.starts_with(of));
        its.map(|(n, _)| n.to_owned()).collect()
    };
    let third = faces("row/3/");
    assert!(
        !third.is_empty() && third.len() == faces("row/1/").len(),
        "{third:?}"
    );
    assert!(third.iter().all(|n| !n.contains('#')), "{third:?}");
    // Every 2 along it; past the corner they are turned with it: 0.2
    // along x where the peg is 0.4.
    for (k, at) in [
        (1, [2.0, 0.0]),
        (2, [4.0, 0.0]),
        (3, [4.0, 2.0]),
        (4, [4.0, 4.0]),
    ] {
        let c = centre(&b, &format!("row/{k}"))?;
        close(c.x, at[0], 1e-9);
        close(c.y, at[1], 1e-9);
    }
    let turned = workpiece_boolean::read(&b.world.topo, body(&b, "row/4"))?;
    let wide = |s: &workpiece_boolean::Solid| {
        let xs = s.vertices.iter().map(|v| v.point.x);
        xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
    };
    close(wide(&turned), 0.2, 1e-9);

    // At the points of a sketch: the peg is at the first.
    let mut b = Standalone::new();
    small(&mut b)?;
    let mut points = Sketch::new("points");
    points.args.plane = Some(xy());
    for at in [[0.1, 0.0], [1.0, 2.0], [-3.0, 1.0]] {
        let p = points.args.add_point(Point2::from_array(at));
        points.args.add(Shape::Point(p));
    }
    points.apply(&mut b)?;
    Pattern {
        id: "spots".into(),
        args: PatternArgs {
            of: Repeat::Bodies,
            bodies: vec![Source::Topology("peg".into()).into()],
            along: Along::Points,
            sketch: Some(Source::Profile("points".into()).into()),
            ..PatternArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let mut places = [centre(&b, "spots/1")?, centre(&b, "spots/2")?];
    places.sort_by(|a, b| a.x.total_cmp(&b.x));
    close(places[0].x, -3.1, 1e-9);
    close(places[0].y, 1.0, 1e-9);
    close(places[1].x, 0.9, 1e-9);
    close(places[1].y, 2.0, 1e-9);
    Ok(())
}
