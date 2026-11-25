//! What the operations of this crate make.

use shapecraft_core::world::AreaPick;
use shapecraft_ops_sweep::{Extent, Extrude, Revolve, RevolveAxis};
use shapecraft_ops_testing::*;
use shapecraft_shape::Combine;

#[test]
fn sketches_extrude_into_plates_with_holes() -> anyhow::Result<()> {
    let mut b = Standalone::new();
    let (lo, hi) = ([-2.0, -1.0], [2.0, 1.0]);
    sketch(&mut b, "s", xy(), Some((lo, hi)), &[([0.5, 0.0], 0.5)])?;
    let mut plate = Extrude::of("plate", Some("s".into()), false);
    plate.args.distance = Measure::new(1.5);
    plate.apply(&mut b)?;
    check(&b);
    // Four sides, the hole's wall, and two ends; the circle inside the
    // rectangle is a hole.
    let topo = &b.world.topo;
    assert_eq!(topo.body_faces(body(&b, "plate")).map(|f| f.len()), Some(7));
    close(
        volume(topo, body(&b, "plate")),
        (8.0 - PI / 4.0) * 1.5,
        0.01,
    );
    assert!(topo.lookup("plate/start").is_some() && topo.lookup("plate/end").is_some());

    // Symmetric, and of the hole's area alone.
    let mut pin = Extrude::of("pin", Some("s".into()), false);
    pin.args.combine = Combine::NewBody;
    let profile = &b.world.profiles["s"];
    let disk = (0..profile.regions.len())
        .min_by(|&x, &y| profile.regions[x].area.total_cmp(&profile.regions[y].area))
        .expect("an area");
    pin.args.areas = vec![AreaPick::Bounded(profile.anchors(disk))];
    pin.args.extent = Extent::Symmetric;
    pin.args.distance = Measure::new(4.0);
    pin.apply(&mut b)?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "pin")), PI, 0.01);
    // A surface of it: the wall alone, or with its ends.
    for (id, caps, faces) in [("tube", false, 1), ("can", true, 3)] {
        let mut surface = Extrude::of(id, Some("s".into()), true);
        surface.args.areas = vec![AreaPick::Bounded(b.world.profiles["s"].anchors(disk))];
        surface.args.caps = caps;
        surface.apply(&mut b)?;
        check(&b);
        let topo = &b.world.topo;
        assert_eq!(
            topo.body(body(&b, id)).map(|b| b.kind),
            Some(BodyKind::Sheet)
        );
        assert_eq!(topo.body_faces(body(&b, id)).map(|f| f.len()), Some(faces));
    }
    Ok(())
}

#[test]
fn extrusions_join_cut_and_stop_where_told() -> anyhow::Result<()> {
    let hole = |b: &mut Standalone, id: &str, set: &dyn Fn(&mut Extrude)| {
        base(b)?;
        // A circle on the top of the box (a face's sketch starts where
        // the world's origin falls on its plane, the world's x its x).
        let top = Reference::topology("base/top");
        sketch(b, "s", top, None, &[([0.5, 0.0], 0.5)])?;
        let mut op = Extrude::of(id, Some("s".into()), false);
        op.args.distance = shapecraft_core::Measure::new(1.0);
        set(&mut op);
        op.apply(b)?;
        check(b);
        anyhow::Ok(volume(&b.world.topo, body(b, "base")))
    };
    let disk = PI / 4.0;
    // A boss on it.
    let mut b = Standalone::new();
    let joined = hole(&mut b, "boss", &|op| op.args.combine = Combine::Join)?;
    close(joined, 32.0 + disk, 0.01);
    assert!(
        b.world.topo.lookup("boss").is_none(),
        "the boss is of the base now"
    );
    assert!(b.world.topo.lookup("boss/end").is_some());
    // A blind hole into it.
    let mut b = Standalone::new();
    let blind = hole(&mut b, "hole", &|op| {
        op.args.combine = Combine::Cut;
        op.args.flip = true;
    })?;
    close(blind, 32.0 - disk, 0.01);
    // Through all of it, and up to where it comes out: the same here.
    for extent in [Extent::ThroughAll, Extent::ToNext] {
        let mut b = Standalone::new();
        let through = hole(&mut b, "hole", &|op| {
            op.args.combine = Combine::Cut;
            op.args.flip = true;
            op.args.extent = extent;
        })?;
        close(through, 32.0 - 2.0 * disk, 0.01);
    }
    // Said the way that leads away from the box, a cut turns to go into
    // it: through all of it, or as deep as it is told.
    let mut b = Standalone::new();
    let turned = hole(&mut b, "hole", &|op| {
        op.args.combine = Combine::Cut;
        op.args.extent = Extent::ThroughAll;
    })?;
    close(turned, 32.0 - 2.0 * disk, 0.01);
    let mut b = Standalone::new();
    let turned = hole(&mut b, "hole", &|op| op.args.combine = Combine::Cut)?;
    close(turned, 32.0 - disk, 0.01);

    // With a tower standing on the box beside the hole, above the
    // sketch but not above the hole: through all still turns into the
    // box. And from a sketch in the middle of the box, through all goes
    // the way it is said (up: half the box), not both ways.
    let tower = |b: &mut Standalone| {
        let there = shapecraft_core::world::DatumFrame::new(
            shapecraft_core::math::Point3::new(-1.5, 0.0, 4.0),
            shapecraft_core::math::Point3::X,
            shapecraft_core::math::Point3::Y,
        )?;
        b.world
            .datums
            .insert("there".into(), shapecraft_core::world::Datum::Frame(there));
        shapecraft_ops_basic_shapes::AddBox {
            id: "tower".into(),
            args: shapecraft_ops_basic_shapes::AddBoxArgs {
                frame: Some(Reference::datum("there")),
                size: [1.0, 4.0, 6.0].map(Measure::new),
            },
        }
        .apply(b)
    };
    let mut b = Standalone::new();
    tower(&mut b)?;
    let turned = hole(&mut b, "hole", &|op| {
        op.args.combine = Combine::Cut;
        op.args.extent = Extent::ThroughAll;
    })?;
    close(turned, 32.0 - 2.0 * disk, 0.01);
    let mut b = Standalone::new();
    base(&mut b)?;
    sketch(&mut b, "mid", xy(), None, &[([0.5, 0.0], 0.5)])?;
    let mut through = Extrude::of("through", Some("mid".into()), false);
    through.args.combine = Combine::Cut;
    through.args.extent = Extent::ThroughAll;
    through.apply(&mut b)?;
    close(volume(&b.world.topo, body(&b, "base")), 32.0 - disk, 0.01);

    // Half over the box's edge: half a hole, through the edge.
    let mut b = Standalone::new();
    base(&mut b)?;
    sketch(
        &mut b,
        "s",
        Reference::topology("base/top"),
        None,
        &[([-1.5, -2.0], 0.5)],
    )?;
    let mut notch = Extrude::of("notch", Some("s".into()), false);
    notch.args.flip = true;
    notch.args.distance = shapecraft_core::Measure::new(1.0);
    notch.args.combine = Combine::Cut;
    notch.apply(&mut b)?;
    check(&b);
    close(
        volume(&b.world.topo, body(&b, "base")),
        32.0 - disk / 2.0,
        0.01,
    );
    Ok(())
}

#[test]
fn posts_reach_up_to_the_next_body() -> anyhow::Result<()> {
    for extent in [Extent::ToFace, Extent::ToNext] {
        let mut b = Standalone::new();
        base(&mut b)?;
        // A floor well below the box, and a post from it up to the box.
        let low = shapecraft_core::world::DatumFrame::from_pose(
            shapecraft_core::Pose::from_translation(shapecraft_core::math::Point3::new(
                0.0, 0.0, -4.0,
            )),
        )?;
        b.world.datums.insert("low".into(), low.into());
        AddBox {
            id: "floor".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum("low")),
                size: [4.0, 4.0, 1.0].map(Measure::new),
            },
        }
        .apply(&mut b)?;
        sketch(
            &mut b,
            "s",
            Reference::topology("floor/top"),
            None,
            &[([0.5, 0.0], 0.5)],
        )?;
        let mut post = Extrude::of("post", Some("s".into()), false);
        post.args.extent = extent;
        post.args.up_to = Some(Reference::topology("base/bottom"));
        post.args.combine = Combine::Join;
        post.apply(&mut b)?;
        check(&b);
        // The floor, the post (from z = −3.5 to −1) and the box are one.
        let topo = &b.world.topo;
        assert!(topo.lookup("base").is_none() || topo.lookup("floor").is_none());
        let all: f64 = shapecraft_core::world::topo::Topo::bodies(topo)
            .ids()
            .map(|id| volume(topo, id))
            .sum();
        close(all, 32.0 + 16.0 + 2.5 * PI / 4.0, 0.02);
        assert_eq!(topo.bodies().len(), 1);
    }
    Ok(())
}

#[test]
fn sketches_revolve_into_rings_and_combine() -> anyhow::Result<()> {
    let mut b = Standalone::new();
    // A rectangle beside the v axis, turned around it.
    sketch(&mut b, "s", xy(), Some(([1.0, 0.0], [2.0, 1.0])), &[])?;
    let mut ring = Revolve::of("ring", Some("s".into()), false);
    ring.args.axis = RevolveAxis::SketchV;
    ring.apply(&mut b)?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "ring")), 3.0 * PI, 0.05);
    // A quarter of it, either way and symmetric, with flat ends.
    for (id, flip, symmetric) in [
        ("q1", false, false),
        ("q2", true, false),
        ("q3", false, true),
    ] {
        let mut quarter = Revolve::of(id, Some("s".into()), false);
        quarter.args.combine = Combine::NewBody;
        quarter.args.axis = RevolveAxis::SketchV;
        quarter.args.angle = Measure::new(90.0);
        quarter.args.flip = flip;
        quarter.args.symmetric = symmetric;
        quarter.apply(&mut b)?;
        check(&b);
        let topo = &b.world.topo;
        close(volume(topo, body(&b, id)), 0.75 * PI, 0.02);
        assert_eq!(topo.body_faces(body(&b, id)).map(|f| f.len()), Some(6));
    }
    // A groove turned into a block: planes and cylinders, so it cuts.
    let mut b = Standalone::new();
    AddBox {
        id: "block".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [6.0, 1.0, 6.0].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    sketch(&mut b, "s", xy(), Some(([1.0, 0.0], [2.0, 1.0])), &[])?;
    let mut groove = Revolve::of("groove", Some("s".into()), false);
    groove.args.axis = RevolveAxis::SketchV;
    groove.args.combine = Combine::Cut;
    groove.apply(&mut b)?;
    check(&b);
    close(
        volume(&b.world.topo, body(&b, "block")),
        36.0 - 1.5 * PI,
        0.05,
    );
    Ok(())
}

#[test]
fn a_sketch_with_a_rotation_axis_is_turned_about_it() -> anyhow::Result<()> {
    use shapecraft_core::math::Point2;
    use shapecraft_core::{EditContext, Operation, Widget};
    let mut b = Standalone::new();
    // A rectangle from x = 1 to 2, and a rotation axis upright at x = 3:
    // a ring 1 to 2 off the axis, 1 high.
    let mut s = shapecraft_ops_sketch::Sketch::new("s");
    s.args.plane = Some(xy());
    let corners = [[1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0]]
        .map(|p| s.args.add_point(Point2::from_array(p)));
    for k in 0..4 {
        s.args.add_line(corners[k], corners[(k + 1) % 4]);
    }
    let through = s.args.add_point(Point2::new(3.0, 0.0));
    s.args.add_rotation_axis(through, Point2::Y);
    s.apply(&mut b)?;
    // Picked in its dialog, the sketch's rotation axis is chosen.
    let picker = Revolve::of("ring", None, false);
    let world = std::sync::Arc::new(b.world.clone());
    let cache = shapecraft_core::Cache::default();
    let context = EditContext::standalone(&world, &cache);
    let mut picked = None;
    picker.visualize_edit(&context, &()).dialog.visit(&mut |w| {
        if let Widget::ReferenceInput(field) = w
            && field.label == "Sketch"
        {
            picked = Some(field.set(shapecraft_core::Source::Profile("s".into())));
        }
    });
    let picked = picked.expect("a sketch field").args.expect("arguments");
    assert_eq!(picked.axis, RevolveAxis::Drawn);
    let mut ring = picker;
    ring.args = picked;
    ring.apply(&mut b)?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "ring")), 3.0 * PI, 0.05);
    Ok(())
}

#[test]
fn sketches_sweep_along_paths() -> anyhow::Result<()> {
    use shapecraft_ops_sweep::{SweptLoft, SweptLoftArgs};
    let yz = || Reference::frame_axis(ORIGIN, Axis::X);
    // A square bar around a corner: as much as its centre line is long.
    let mut b = Standalone::new();
    sketch(
        &mut b,
        "section",
        yz(),
        Some(([-0.5, -0.5], [0.5, 0.5])),
        &[],
    )?;
    let mut path = Sketch::new("path");
    path.args.plane = Some(xy());
    let corners =
        [[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]].map(|p| path.args.add_point(Point2::from_array(p)));
    path.args.add_line(corners[0], corners[1]);
    path.args.add_line(corners[1], corners[2]);
    path.apply(&mut b)?;
    let sweep = |twist: f64| SweptLoft {
        id: "bar".into(),
        args: SweptLoftArgs {
            sections: vec![Source::Profile("section".into()).into()],
            path: Some(Source::Profile("path".into()).into()),
            twist: Measure::new(twist),
            ..SweptLoftArgs::default()
        },
    };
    sweep(0.0).apply(&mut b)?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "bar")), 7.0, 0.01);
    // As a surface: the four sides around the corner, no ends.
    let mut skin = sweep(0.0);
    skin.id = "skin".into();
    skin.args.surface = true;
    skin.apply(&mut b)?;
    check(&b);
    assert_eq!(
        b.world.topo.body(body(&b, "skin")).map(|b| b.kind),
        Some(BodyKind::Sheet)
    );
    let skin = shapecraft_boolean::read_sheet(&b.world.topo, body(&b, "skin"))?;
    let area: f64 = (0..skin.faces.len())
        .map(|f| shapecraft_boolean::face_area(&skin, f))
        .sum();
    close(area, 4.0 * 7.0, 1e-9);

    // A ring: a disk all the way around a circle through its middle.
    let mut b = Standalone::new();
    let xz = Reference::frame_axis(ORIGIN, Axis::Y);
    sketch(&mut b, "section", xz, None, &[([0.0, 0.0], 0.5)])?;
    sketch(&mut b, "path", xy(), None, &[([3.0, 0.0], 3.0)])?;
    sweep(0.0).apply(&mut b)?;
    check(&b);
    close(
        volume(&b.world.topo, body(&b, "bar")),
        2.0 * PI * 3.0 * PI * 0.25,
        0.1,
    );
    Ok(())
}

#[test]
fn sections_loft_into_solids() -> anyhow::Result<()> {
    use shapecraft_core::math::Point3;
    use shapecraft_core::world::topo::Curve3;
    use shapecraft_shape::swept::{circle, polygon, through_sections};
    // The sides of an outline, at a height.
    let raised = |(sides, points): (
        shapecraft_shape::swept::Sides,
        shapecraft_shape::swept::Weighted,
    ),
                  z: f64| {
        let mut curves = Vec::new();
        let mut at = 0;
        for (degree, knots, count) in sides {
            let (p, w): (Vec<Point3>, Vec<f64>) = points[at..at + count]
                .iter()
                .map(|(p, w)| (Point3::new(p.x, p.y, z), *w))
                .unzip();
            curves.push(Curve3::new(degree, knots, p, Some(w))?);
            at += count;
        }
        anyhow::Ok(curves)
    };
    let square = |half: f64| {
        polygon(&[
            Point2::new(-half, -half),
            Point2::new(half, -half),
            Point2::new(half, half),
            Point2::new(-half, half),
        ])
    };
    let measured = |sections: &[Vec<Curve3>]| {
        let solid = through_sections("loft", sections)?;
        let mut topo = shapecraft_core::world::Topo::new();
        let made = shapecraft_boolean::write(&mut topo, &solid, "loft")?;
        assert_eq!(topo.validate(), Ok(()));
        anyhow::Ok(volume(&topo, made))
    };
    // A frustum of a pyramid, between two squares.
    let pyramid = measured(&[raised(square(2.0), 0.0)?, raised(square(1.0), 3.0)?])?;
    close(pyramid, 3.0 / 3.0 * (16.0 + 4.0 + 8.0), 0.01);
    // Square to round, and a pentagon to a square: their sides are not
    // of a kind, or not as many.
    let round = measured(&[
        raised(square(2.0), 0.0)?,
        raised(circle(Point2::ZERO, 1.0), 3.0)?,
    ])?;
    assert!(round > 3.0 * PI && round < pyramid + 4.0, "{round}");
    let five: Vec<Point2> = (0..5)
        .map(|k| Point2::from_angle(k as f64 * PI * 0.4) * 2.0)
        .collect();
    let mixed = measured(&[raised(polygon(&five), 0.0)?, raised(square(1.0), 3.0)?])?;
    assert!(mixed > 12.0 && mixed < 28.0, "{mixed}");
    // Smoothly through three: fuller than straight between them.
    let barrel = measured(&[
        raised(square(1.0), 0.0)?,
        raised(square(2.0), 2.0)?,
        raised(square(1.0), 4.0)?,
    ])?;
    let straight = 2.0 * 2.0 / 3.0 * (4.0 + 16.0 + 8.0);
    assert!(barrel > straight && barrel < 64.0, "{barrel}");
    Ok(())
}

#[test]
fn ribs_fill_in_to_the_body() -> anyhow::Result<()> {
    use shapecraft_ops_sweep::{Rib, RibArgs};
    // An L of two boxes (a floor and a wall), and a line across its
    // corner in the plane y = 0: a triangular rib.
    let mut b = Standalone::new();
    for (id, at, size) in [
        ("floor", [0.0, 0.0, -0.5], [4.0, 2.0, 1.0]),
        ("wall", [-1.5, 0.0, 1.0], [1.0, 2.0, 2.0]),
    ] {
        let pose =
            shapecraft_core::Pose::from_translation(shapecraft_core::math::Point3::from_array(at));
        let frame = format!("{id}-frame");
        b.world.datums.insert(
            frame.clone(),
            shapecraft_core::world::Datum::Frame(shapecraft_core::world::DatumFrame::from_pose(
                pose,
            )?),
        );
        AddBox {
            id: id.into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(frame)),
                size: size.map(Measure::new),
            },
        }
        .apply(&mut b)?;
    }
    // In the plane y = 0, from the wall's face (x = -1) at z = 1.5 to
    // the floor's top (z = 0) at x = 0.5.
    let world = &b.world;
    let plane = world
        .resolve(&Reference::<shapecraft_core::world::DatumPlane>::frame_axis(ORIGIN, Axis::Y))?;
    let flat = |p: [f64; 3]| {
        let d = shapecraft_core::math::Point3::from_array(p) - plane.origin();
        Point2::new(d.dot(plane.x_dir()), d.dot(plane.y_dir()))
    };
    let mut line = Sketch::new("line");
    line.args.plane = Some(Reference::frame_axis(ORIGIN, Axis::Y));
    let from = line.args.add_point(flat([-1.0, 0.0, 1.5]));
    let to = line.args.add_point(flat([0.5, 0.0, 0.0]));
    line.args.add_line(from, to);
    line.apply(&mut b)?;
    let before = 12.0;
    // (Whichever way it is told to grow, it grows towards the corner:
    // nothing stops it the other way.)
    let made = |flip: bool| -> anyhow::Result<Standalone> {
        let mut b = Standalone {
            world: b.world.clone(),
            ..Standalone::new()
        };
        Rib {
            id: "rib".into(),
            args: RibArgs {
                sketch: Some(Source::Profile("line".into()).into()),
                thickness: Measure::new(0.2),
                flip,
                target: None,
            },
        }
        .apply(&mut b)?;
        Ok(b)
    };
    made(true)?;
    let made = made(false)?;
    check(&made);
    // Its thickness is said across it.
    assert_eq!(made.world.annotations["rib/thickness"].text(), "0,2");
    close(
        volume(&made.world.topo, body(&made, "floor")) - before,
        0.5 * 1.5 * 1.5 * 0.2,
        1e-6,
    );
    Ok(())
}

#[test]
fn a_sketch_of_several_runs_makes_a_rib_to_each() -> anyhow::Result<()> {
    use shapecraft_ops_sweep::{Rib, RibArgs};
    // A floor between two walls (x from -2 to -1 and from 1 to 2), the
    // floor's top at z = 0; lines in the plane y = 0 from one wall and
    // from the other down to the floor.
    let build = |lines: &[([f64; 2], [f64; 2])], steps: usize| -> anyhow::Result<f64> {
        let mut b = Standalone::new();
        for (id, at, size) in [
            ("floor", [0.0, 0.0, -0.5], [4.0, 2.0, 1.0]),
            ("left", [-1.5, 0.0, 1.0], [1.0, 2.0, 2.0]),
            ("right", [1.5, 0.0, 1.0], [1.0, 2.0, 2.0]),
        ] {
            let pose = shapecraft_core::Pose::from_translation(
                shapecraft_core::math::Point3::from_array(at),
            );
            let frame = format!("{id}-frame");
            b.world.datums.insert(
                frame.clone(),
                shapecraft_core::world::Datum::Frame(
                    shapecraft_core::world::DatumFrame::from_pose(pose)?,
                ),
            );
            AddBox {
                id: id.into(),
                args: AddBoxArgs {
                    frame: Some(Reference::datum(frame)),
                    size: size.map(Measure::new),
                },
            }
            .apply(&mut b)?;
        }
        let plane = b.world.resolve(
            &Reference::<shapecraft_core::world::DatumPlane>::frame_axis(ORIGIN, Axis::Y),
        )?;
        let flat = |p: [f64; 2]| {
            let d = shapecraft_core::math::Point3::new(p[0], 0.0, p[1]) - plane.origin();
            Point2::new(d.dot(plane.x_dir()), d.dot(plane.y_dir()))
        };
        // One sketch of all the lines (one rib step), or one sketch to
        // each (one step each).
        let sketches: Vec<&[([f64; 2], [f64; 2])]> = if steps == 1 {
            vec![lines]
        } else {
            lines.chunks(1).collect()
        };
        for (k, group) in sketches.iter().enumerate() {
            let mut line = Sketch::new(format!("line{k}"));
            line.args.plane = Some(Reference::frame_axis(ORIGIN, Axis::Y));
            for (from, to) in *group {
                let (from, to) = (
                    line.args.add_point(flat(*from)),
                    line.args.add_point(flat(*to)),
                );
                line.args.add_line(from, to);
            }
            line.apply(&mut b)?;
            Rib {
                id: format!("rib{k}"),
                args: RibArgs {
                    sketch: Some(Source::Profile(format!("line{k}")).into()),
                    thickness: Measure::new(0.2),
                    flip: false,
                    target: None,
                },
            }
            .apply(&mut b)?;
        }
        check(&b);
        Ok(volume(&b.world.topo, body(&b, "floor")))
    };
    // Apart, and crossing each other (a web).
    for lines in [
        [([-1.0, 1.0], [-0.2, 0.0]), ([1.0, 1.0], [0.2, 0.0])],
        [([-1.0, 1.5], [0.5, 0.0]), ([1.0, 1.5], [-0.5, 0.0])],
    ] {
        let together = build(&lines, 1)?;
        let apart = build(&lines, 2)?;
        close(together, apart, 1e-6);
        assert!(together > 8.0 + 0.1, "the ribs are joined to the floor");
    }
    Ok(())
}

#[test]
fn a_bent_rib_fills_in_from_all_of_its_curves() -> anyhow::Result<()> {
    use shapecraft_ops_sweep::{Rib, RibArgs};
    // A floor 4 × 2 × 1 (its top at z = 0), and on it in the plane
    // y = 0 a peak of two lines: from (-1, 0) up to (0, 3) and down to
    // (1, 0). The rib grows down from the peak to the floor: a triangle
    // 2 wide and 3 high, though the peak stands farther from the floor
    // than the lines' ends do.
    let mut b = Standalone::new();
    let pose =
        shapecraft_core::Pose::from_translation(shapecraft_core::math::Point3::new(0.0, 0.0, -0.5));
    b.world.datums.insert(
        "floor-frame".into(),
        shapecraft_core::world::Datum::Frame(shapecraft_core::world::DatumFrame::from_pose(pose)?),
    );
    AddBox {
        id: "floor".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum("floor-frame")),
            size: [4.0, 2.0, 1.0].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    let plane = b
        .world
        .resolve(&Reference::<shapecraft_core::world::DatumPlane>::frame_axis(ORIGIN, Axis::Y))?;
    let flat = |p: [f64; 3]| {
        let d = shapecraft_core::math::Point3::from_array(p) - plane.origin();
        Point2::new(d.dot(plane.x_dir()), d.dot(plane.y_dir()))
    };
    let mut peak = Sketch::new("peak");
    peak.args.plane = Some(Reference::frame_axis(ORIGIN, Axis::Y));
    let points =
        [[-1.0, 0.0, 0.0], [0.0, 0.0, 3.0], [1.0, 0.0, 0.0]].map(|p| peak.args.add_point(flat(p)));
    peak.args.add_line(points[0], points[1]);
    peak.args.add_line(points[1], points[2]);
    peak.apply(&mut b)?;
    let before = volume(&b.world.topo, body(&b, "floor"));
    Rib {
        id: "rib".into(),
        args: RibArgs {
            sketch: Some(Source::Profile("peak".into()).into()),
            thickness: Measure::new(0.2),
            flip: false,
            target: None,
        },
    }
    .apply(&mut b)?;
    check(&b);
    close(
        volume(&b.world.topo, body(&b, "floor")) - before,
        0.5 * 2.0 * 3.0 * 0.2,
        1e-6,
    );
    Ok(())
}

#[test]
fn sections_coil_around_an_axis() -> anyhow::Result<()> {
    use shapecraft_ops_sweep::{SweptLoft, SweptLoftArgs};
    // A disk of radius 0.2 in the xz plane, 2 out along x, coiled three
    // turns about z with a pitch of 1: a spring.
    let mut b = Standalone::new();
    let side: Reference<shapecraft_core::world::DatumPlane> =
        Reference::frame_axis(ORIGIN, Axis::Y);
    let plane = b.world.resolve(&side)?;
    let d = shapecraft_core::math::Point3::new(2.0, 0.0, 0.0) - plane.origin();
    let at = [d.dot(plane.x_dir()), d.dot(plane.y_dir())];
    sketch(&mut b, "wire", side, None, &[(at, 0.2)])?;
    SweptLoft {
        id: "spring".into(),
        args: SweptLoftArgs {
            sections: vec![Source::Profile("wire".into()).into()],
            coil: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            pitch: Measure::new(1.0),
            turns: Measure::new(3.0),
            ..SweptLoftArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let spring = shapecraft_boolean::read(&b.world.topo, body(&b, "spring"))?;
    // The wire's section (seen along the helix: a little less than the
    // disk, which leans to it) along the helix's length.
    let turn = ((2.0 * PI * 2.0_f64).powi(2) + 1.0).sqrt();
    let lean = 2.0 * PI * 2.0 / turn;
    close(
        shapecraft_boolean::measure(&spring).volume,
        PI * 0.04 * lean * 3.0 * turn,
        1e-3,
    );
    Ok(())
}

#[test]
fn surfaces_are_lofted_through_sketches_left_open() -> anyhow::Result<()> {
    use shapecraft_core::math::Point3;
    use shapecraft_ops_sweep::{SweptLoft, SweptLoftArgs};
    // An L of two lines (2 and 1 long), on the ground and again 3 up:
    // lofted as a surface, two walls 3 high — and nothing where the L
    // is open.
    let mut b = Standalone::new();
    let up = shapecraft_core::Pose::from_translation(Point3::new(0.0, 0.0, 3.0));
    let frame = shapecraft_core::world::DatumFrame::from_pose(up)?;
    b.world.datums.insert("up".into(), frame.into());
    for (id, plane) in [
        ("low", xy()),
        ("high", Reference::frame_axis("up", Axis::Z)),
    ] {
        let mut sketch = Sketch::new(id);
        sketch.args.plane = Some(plane);
        let args = &mut sketch.args;
        let corners =
            [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0]].map(|p| args.add_point(Point2::from_array(p)));
        args.add_line(corners[0], corners[1]);
        args.add_line(corners[1], corners[2]);
        sketch.apply(&mut b)?;
    }
    SweptLoft {
        id: "walls".into(),
        args: SweptLoftArgs {
            sections: vec![
                Source::Profile("low".into()).into(),
                Source::Profile("high".into()).into(),
            ],
            surface: true,
            ..SweptLoftArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let sheet = shapecraft_boolean::read_sheet(&b.world.topo, body(&b, "walls"))?;
    assert_eq!(sheet.faces.len(), 2);
    let area: f64 = (0..2)
        .map(|f| shapecraft_boolean::face_area(&sheet, f))
        .sum();
    close(area, 9.0, 1e-9);
    // Two splines that bend to different sides and run opposite ways:
    // the surface through them has each as an edge.
    for (id, plane, points) in [
        ("near", xy(), [[0.8, 1.8], [-0.3, 0.5], [0.1, -1.5]]),
        (
            "far",
            Reference::frame_axis("up", Axis::Z),
            [[-1.0, -1.4], [0.5, 0.5], [0.4, 2.2]],
        ),
    ] {
        let mut sketch = Sketch::new(id);
        sketch.args.plane = Some(plane);
        let control = points
            .map(|p| sketch.args.add_point(Point2::from_array(p)))
            .to_vec();
        sketch.args.add(Shape::Spline {
            control,
            degree: 3,
            knots: Vec::new(),
            weights: Vec::new(),
        });
        sketch.apply(&mut b)?;
    }
    SweptLoft {
        id: "wave".into(),
        args: SweptLoftArgs {
            sections: vec![
                Source::Profile("near".into()).into(),
                Source::Profile("far".into()).into(),
            ],
            surface: true,
            ..SweptLoftArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let sheet = shapecraft_boolean::read_sheet(&b.world.topo, body(&b, "wave"))?;
    let mut on_edges = Vec::new();
    for edge in &sheet.edges {
        let (t0, t1) = edge.range;
        on_edges
            .extend((0..=200).map(|k| edge.curve.point_at(t0 + (t1 - t0) * f64::from(k) / 200.0)));
    }
    for (id, z) in [("near", 0.0), ("far", 3.0)] {
        let edge = &b.world.profiles[id].edges[0];
        for k in 0..=10 {
            let p = edge
                .curve
                .point_at(edge.t0 + (edge.t1 - edge.t0) * f64::from(k) / 10.0);
            let p = Point3::new(p.x, p.y, z);
            let off = on_edges
                .iter()
                .map(|q| q.distance(p))
                .fold(f64::INFINITY, f64::min);
            assert!(off < 0.03, "the surface is {off} off the curve of {id}");
        }
    }
    Ok(())
}

#[test]
fn solids_are_measured_with_their_centre_of_gravity() -> anyhow::Result<()> {
    use shapecraft_ops_basic_shapes::{AddCone, AddConeArgs};
    // A cone on the origin: a third of its base times its height, and
    // balanced a quarter of the way up its axis.
    let mut b = Standalone::new();
    AddCone {
        id: "cone".into(),
        args: AddConeArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(1.5),
            top: Measure::new(0.0),
            height: Measure::new(4.0),
        },
    }
    .apply(&mut b)?;
    let m =
        shapecraft_boolean::measure(&shapecraft_boolean::read(&b.world.topo, body(&b, "cone"))?);
    close(m.volume, PI * 2.25 * 4.0 / 3.0, 1e-9);
    close(m.centre.x, 0.0, 1e-9);
    close(m.centre.y, 0.0, 1e-9);
    close(m.centre.z, 1.0, 1e-9);
    // The 4 × 4 × 2 box less a corner block: off centre by what is gone.
    let mut b = Standalone::new();
    base(&mut b)?;
    sketch(&mut b, "corner", xy(), Some(([0.0, 0.0], [2.0, 2.0])), &[])?;
    let mut cut = Extrude::of("cut", Some("corner".into()), false);
    cut.args.extent = Extent::Symmetric;
    cut.args.distance = Measure::new(4.0);
    cut.args.combine = Combine::Cut;
    cut.apply(&mut b)?;
    let m =
        shapecraft_boolean::measure(&shapecraft_boolean::read(&b.world.topo, body(&b, "base"))?);
    close(m.volume, 24.0, 1e-9);
    close(m.centre.x, -8.0 / 24.0, 1e-9);
    close(m.centre.y, -8.0 / 24.0, 1e-9);
    close(m.centre.z, 0.0, 1e-9);
    Ok(())
}

#[test]
fn bodies_are_written_as_stl() -> anyhow::Result<()> {
    let mut b = Standalone::new();
    base(&mut b)?;
    let stl = shapecraft_cad_rasterize::tessellate::stl(&b.world.topo);
    let count = u32::from_le_bytes([stl[80], stl[81], stl[82], stl[83]]) as usize;
    assert_eq!((count, stl.len()), (12, 84 + 50 * 12));
    // Its triangles face out of the box: they enclose its volume.
    let number = |at: usize| {
        f64::from(f32::from_le_bytes([
            stl[at],
            stl[at + 1],
            stl[at + 2],
            stl[at + 3],
        ]))
    };
    let mut volume = 0.0;
    for k in 0..count {
        let corner = |c: usize| {
            let at = 84 + 50 * k + 12 * (c + 1);
            shapecraft_core::math::Point3::new(number(at), number(at + 4), number(at + 8))
        };
        volume += corner(0).dot(corner(1).cross(corner(2))) / 6.0;
    }
    close(volume, 32.0, 1e-5);
    Ok(())
}

/// A sketch `id` on the plane of x and y: lines from point to point.
fn lines(b: &mut Standalone, id: &str, runs: &[&[[f64; 2]]]) -> anyhow::Result<()> {
    let mut sketch = shapecraft_ops_sketch::Sketch::new(id);
    sketch.args.plane = Some(xy());
    for run in runs {
        // (A run that ends where it began closes.)
        let mut points = Vec::new();
        for (k, p) in run.iter().enumerate() {
            if k > 0 && *p == run[0] {
                points.push(points[0]);
            } else {
                points.push(sketch.args.add_point(Point2::from_array(*p)));
            }
        }
        for pair in points.windows(2) {
            sketch.args.add_line(pair[0], pair[1]);
        }
    }
    sketch.apply(b)
}

#[test]
fn curves_left_open_sweep_into_surfaces() -> anyhow::Result<()> {
    let mut b = Standalone::new();
    // A bent line; and a triangle with a line beside it.
    lines(&mut b, "bent", &[&[[1.0, 0.0], [2.0, 0.0], [2.0, 1.0]]])?;
    lines(
        &mut b,
        "both",
        &[
            &[[1.0, 0.0], [2.0, 0.0], [1.0, 1.0], [1.0, 0.0]],
            &[[3.0, 0.0], [3.0, 1.0]],
        ],
    )?;
    let faces = |b: &Standalone, id: &str| {
        let topo = &b.world.topo;
        assert_eq!(
            topo.body(body(b, id)).map(|b| b.kind),
            Some(BodyKind::Sheet)
        );
        topo.body_faces(body(b, id)).map(|f| f.len())
    };
    for (sketch, caps, wanted) in [("bent", false, 2), ("both", false, 4), ("both", true, 6)] {
        let id = format!("up-{sketch}-{caps}");
        let mut up = Extrude::of(&*id, Some(sketch.into()), true);
        up.args.caps = caps;
        up.apply(&mut b)?;
        check(&b);
        assert_eq!(faces(&b, &id), Some(wanted), "{id}");
        let id = format!("around-{sketch}-{caps}");
        let mut around = Revolve::of(&*id, Some(sketch.into()), true);
        around.args.axis = RevolveAxis::SketchV;
        around.args.angle = Measure::new(90.0);
        around.args.caps = caps;
        around.apply(&mut b)?;
        check(&b);
        assert_eq!(faces(&b, &id), Some(wanted), "{id}");
    }
    // A solid cannot be made of what does not close.
    assert!(
        Extrude::of("no", Some("bent".into()), false)
            .apply(&mut b)
            .is_err()
    );
    Ok(())
}

#[test]
fn what_a_sweep_is_held_to_is_said_of_the_model_with_its_tolerance() -> anyhow::Result<()> {
    use shapecraft_core::world::{Measured, Note, Tolerance};
    use shapecraft_core::{Arguments, EditContext, Widget};
    let mut b = Standalone::new();
    sketch(&mut b, "s", xy(), Some(([1.0, 0.0], [2.0, 1.0])), &[])?;
    // The distance is entered with its tolerance, in its field.
    let mut plate = Extrude::of("plate", Some("s".into()), false);
    let (world, cache) = (std::sync::Arc::new(b.world.clone()), Default::default());
    let context = EditContext::standalone(&world, &cache);
    let mut entered = None;
    for field in plate.args.fields(&context) {
        field.visit(&mut |w| {
            if let Widget::NumericInput(input) = w
                && input.label == "Distance"
            {
                entered = input.enter("1.5 +0.2/-0.1");
            }
        });
    }
    plate.args = entered.expect("the distance's field");
    assert_eq!(plate.args.distance.to_string(), "1.5 +0.2/-0.1");
    plate.apply(&mut b)?;
    close(volume(&b.world.topo, body(&b, "plate")), 1.5, 0.01);
    // It is a dimension of the model: from the sketch as far up.
    let Note::Dimension {
        measured: Measured::Length { from, to, .. },
        value,
        tolerance,
        ..
    } = &b.world.annotations["plate/distance"].note
    else {
        anyhow::bail!("no length");
    };
    close(*value, 1.5, 1e-12);
    close((*to - *from).z, 1.5, 1e-12);
    let deviations = Tolerance::Deviations {
        upper: 0.2,
        lower: -0.1,
    };
    assert_eq!(*tolerance, deviations);
    // A part of a turn is one too; a whole turn is none.
    let mut ring = Revolve::of("ring", Some("s".into()), false);
    ring.args.combine = Combine::NewBody;
    ring.args.axis = RevolveAxis::SketchV;
    ring.apply(&mut b)?;
    assert!(!b.world.annotations.contains_key("ring/angle"));
    let mut part = ring.clone();
    part.id = "part".into();
    part.args.angle = Measure::new(90.0).within(Tolerance::Symmetric(0.5));
    part.apply(&mut b)?;
    assert_eq!(b.world.annotations["part/angle"].text(), "90° ±0,5");
    Ok(())
}

#[test]
fn a_round_topped_bar_is_cut_from_a_block_of_its_width() -> anyhow::Result<()> {
    use shapecraft_core::math::Point2;
    // A block 2 × 1 × 3 on the plane of x and y, and through it along x
    // a bar as wide and as high, its top rounded: its flat sides and
    // its bottom lie in the block's, and its round top touches the
    // block's top along a line. What they share is the bar, 2 long —
    // the round top only touches that face; it does not lie on it.
    let mut b = Standalone::new();
    sketch(&mut b, "a", xy(), Some(([-1.0, -0.5], [1.0, 0.5])), &[])?;
    let mut block = Extrude::of("block", Some("a".into()), false);
    block.args.combine = Combine::NewBody;
    block.args.distance = shapecraft_core::Measure::new(3.0);
    block.apply(&mut b)?;
    let mut d = shapecraft_ops_sketch::Sketch::new("d");
    d.args.plane = Some(Reference::frame_axis(ORIGIN, Axis::X));
    let at = |args: &mut shapecraft_ops_sketch::SketchArgs, u: f64, v: f64| {
        args.add_point(Point2::new(u, v))
    };
    let (p0, p1) = (at(&mut d.args, -0.5, 0.0), at(&mut d.args, 0.5, 0.0));
    let (p2, p3) = (at(&mut d.args, 0.5, 2.5), at(&mut d.args, -0.5, 2.5));
    let centre = at(&mut d.args, 0.0, 2.5);
    d.args.add_line(p0, p1);
    d.args.add_line(p1, p2);
    d.args.add(shapecraft_ops_sketch::Shape::Arc {
        center: centre,
        start: p2,
        end: p3,
    });
    d.args.add_line(p3, p0);
    d.apply(&mut b)?;
    let mut bar = Extrude::of("bar", Some("d".into()), false);
    bar.args.combine = Combine::Intersect;
    bar.args.extent = Extent::Symmetric;
    bar.args.distance = shapecraft_core::Measure::new(10.0);
    bar.apply(&mut b)?;
    check(&b);
    let solid = shapecraft_boolean::read(&b.world.topo, body(&b, "block"))?;
    close(
        shapecraft_boolean::measure(&solid).volume,
        2.0 * (2.5 + PI / 8.0),
        1e-9,
    );
    Ok(())
}

#[test]
fn cuts_to_the_next_face_take_all_of_the_wall_and_to_a_face_end_at_its_plane() -> anyhow::Result<()>
{
    // A ring (a box with a square hole through it), and across its top a
    // bar that reaches over both walls: cut to the next face, it takes
    // the bar's width out of both.
    let mut b = Standalone::new();
    base(&mut b)?;
    sketch(&mut b, "h", xy(), Some(([-1.0, -1.0], [1.0, 1.0])), &[])?;
    let mut hole = Extrude::of("hole", Some("h".into()), false);
    hole.args.combine = Combine::Cut;
    hole.args.extent = Extent::ThroughBoth;
    hole.apply(&mut b)?;
    let ring = 32.0 - 8.0;
    close(volume(&b.world.topo, body(&b, "base")), ring, 1e-9);
    let top = Reference::topology("base/top");
    sketch(&mut b, "s", top, Some(([-3.0, -0.25], [3.0, 0.25])), &[])?;
    let mut slot = Extrude::of("slot", Some("s".into()), false);
    slot.args.combine = Combine::Cut;
    slot.args.extent = Extent::ToNext;
    slot.apply(&mut b)?;
    check(&b);
    // (Each wall is 1 thick and 2 high, the bar half wide.)
    close(
        volume(&b.world.topo, body(&b, "base")),
        ring - 2.0 * 1.0,
        1e-9,
    );
    // From the bottom plane of a box up to its own top face: to that
    // plane, whichever way it is said.
    for flip in [false, true] {
        let mut b = Standalone::new();
        base(&mut b)?;
        let under = Reference::topology("base/bottom");
        sketch(&mut b, "s", under, None, &[([0.0, 0.0], 0.5)])?;
        let mut post = Extrude::of("post", Some("s".into()), false);
        post.args.combine = Combine::NewBody;
        post.args.extent = Extent::ToFace;
        post.args.flip = flip;
        post.args.up_to = Some(Reference::topology("base/top"));
        post.apply(&mut b)?;
        check(&b);
        close(
            volume(&b.world.topo, body(&b, "post")),
            PI * 0.25 * 2.0,
            0.01,
        );
    }
    Ok(())
}

#[test]
fn areas_are_picked_by_a_point_inside_them() -> anyhow::Result<()> {
    let mut b = Standalone::new();
    let (lo, hi) = ([-2.0, -1.0], [2.0, 1.0]);
    sketch(&mut b, "s", xy(), Some((lo, hi)), &[([0.5, 0.0], 0.5)])?;
    // The same, with a small circle drawn before the other: the curves'
    // ids shift.
    sketch(
        &mut b,
        "t",
        xy(),
        Some((lo, hi)),
        &[([-1.2, 0.0], 0.3), ([0.5, 0.0], 0.5)],
    )?;
    let disk = |b: &Standalone, s: &str| {
        let profile = &b.world.profiles[s];
        let around = shapecraft_shape::sweep::region_at(
            profile,
            shapecraft_core::math::Point2::new(0.5, 0.0),
        );
        AreaPick::Bounded(profile.anchors(around.expect("the disk")))
    };
    let pin = |b: &mut Standalone, id: &str, s: &str, pick: AreaPick| {
        let mut pin = Extrude::of(id, Some(s.into()), false);
        pin.args.combine = Combine::NewBody;
        pin.args.areas = vec![pick];
        pin.apply(b)?;
        check(b);
        anyhow::Ok(volume(&b.world.topo, body(b, id)))
    };
    let (big, small) = (PI * 0.25 * 10.0, PI * 0.09 * 10.0);
    // A point picks the disk around it in either sketch.
    close(
        pin(&mut b, "a", "s", AreaPick::inside(0.5, 0.0))?,
        big,
        0.05,
    );
    close(
        pin(&mut b, "b", "t", AreaPick::inside(0.5, 0.0))?,
        big,
        0.05,
    );
    // The boundary the disk had in the first names another curve in the
    // second: what a point is for.
    let by_boundary = disk(&b, "s");
    close(pin(&mut b, "c", "t", by_boundary)?, small, 0.05);
    // The plate around both holes, by a point in it.
    let plate = pin(&mut b, "d", "t", AreaPick::inside(1.5, 0.8))?;
    close(plate, (8.0 - PI * 0.25 - PI * 0.09) * 10.0, 0.1);
    // A point outside every area says so.
    let outside = pin(&mut b, "e", "s", AreaPick::inside(5.0, 5.0));
    let said = format!("{:#}", outside.expect_err("outside the sketch"));
    assert!(
        said.contains("no area of the sketch is around (5, 5)"),
        "{said}"
    );

    // Written as a point, or as anchors as before.
    let pick: AreaPick = serde_json::from_str(r#"{"inside": [0.5, 0.0]}"#)?;
    assert_eq!(pick, AreaPick::inside(0.5, 0.0));
    assert_eq!(serde_json::to_string(&pick)?, r#"{"inside":[0.5,0.0]}"#);
    let anchors: AreaPick = serde_json::from_str(r#"[{"entity": "e7", "left": true, "at": 0.5}]"#)?;
    assert!(matches!(anchors, AreaPick::Bounded(ref a) if a.len() == 1 && a[0].entity == "e7"));
    Ok(())
}
