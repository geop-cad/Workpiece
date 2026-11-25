//! What the operations of this crate make.

use workpiece_ops_sweep::{Extent, Extrude};
use workpiece_ops_testing::*;
use workpiece_shape::Combine;

#[test]
fn slabs_stand_on_faces_of_every_kind() -> anyhow::Result<()> {
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs, AddSphere, AddSphereArgs};
    let face = |solid: &workpiece_boolean::Solid, name: &str| {
        let found = solid
            .faces
            .iter()
            .position(|f| f.name.as_deref() == Some(name));
        found.ok_or_else(|| anyhow::anyhow!("no face {name}"))
    };
    // On a box's top, out of it and into it.
    let mut b = Standalone::new();
    base(&mut b)?;
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    let top = face(&solid, "base/top")?;
    for d in [0.5, -0.5] {
        let (volume, area) = sound(&workpiece_shape::thick::slab(&solid, top, d, "slab")?)?;
        close(volume, 8.0, 1e-9);
        close(area, 32.0 + 8.0, 1e-9);
    }
    // On a cylinder's side: a tube around it, or inside it.
    AddCylinder {
        id: "rod".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(2.0),
            height: Measure::new(3.0),
        },
    }
    .apply(&mut b)?;
    let rod = workpiece_boolean::read(&b.world.topo, body(&b, "rod"))?;
    let side = face(&rod, "rod/side")?;
    for (d, wanted) in [
        (0.5, PI * (2.5 * 2.5 - 4.0) * 3.0),
        (-0.5, PI * (4.0 - 1.5 * 1.5) * 3.0),
    ] {
        let (volume, _) = sound(&workpiece_shape::thick::slab(&rod, side, d, "slab")?)?;
        close(volume, wanted, 1e-9);
    }
    // And on its flat end, which comes to a point in its middle.
    let end = face(&rod, "rod/top")?;
    let (volume, _) = sound(&workpiece_shape::thick::slab(&rod, end, 0.5, "slab")?)?;
    close(volume, PI * 4.0 * 0.5, 1e-9);
    // On a ball: a skin around it.
    AddSphere {
        id: "ball".into(),
        args: AddSphereArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(2.0),
        },
    }
    .apply(&mut b)?;
    let ball = workpiece_boolean::read(&b.world.topo, body(&b, "ball"))?;
    let (volume, area) = sound(&workpiece_shape::thick::slab(&ball, 0, 0.5, "slab")?)?;
    close(volume, 4.0 / 3.0 * PI * (2.5f64.powi(3) - 8.0), 1e-9);
    close(area, 4.0 * PI * (6.25 + 4.0), 1e-9);
    Ok(())
}

#[test]
fn bodies_grow_shrink_and_hollow() -> anyhow::Result<()> {
    let mut b = Standalone::new();
    base(&mut b)?;
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    // Shrunk by a half: the box a half in from every side.
    let (volume, area) = sound(&workpiece_shape::thick::thickened(
        &solid, -0.5, None, false,
    )?)?;
    close(volume, 9.0, 1e-9);
    close(area, 2.0 * (9.0 + 3.0 + 3.0), 1e-9);
    // Hollowed, open at its top: a tray.
    let top = solid
        .faces
        .iter()
        .position(|f| f.name.as_deref() == Some("base/top"));
    let (volume, _) = sound(&workpiece_shape::thick::thickened(
        &solid,
        -0.5,
        Some(&[top.expect("a top")]),
        false,
    )?)?;
    close(volume, 32.0 - 9.0 * 1.5, 1e-9);
    // Grown by a half: its faces moved out, its edges and corners round.
    let (volume, area) = sound(&workpiece_shape::thick::thickened(
        &solid, 0.5, None, false,
    )?)?;
    let d: f64 = 0.5;
    close(
        volume,
        32.0 + 64.0 * d + 40.0 * PI / 4.0 * d * d + 4.0 / 3.0 * PI * d * d * d,
        1e-6,
    );
    close(area, 64.0 + 40.0 * PI / 2.0 * d + 4.0 * PI * d * d, 1e-6);
    Ok(())
}

#[test]
fn bodies_are_shelled_open_at_picked_faces() -> anyhow::Result<()> {
    use workpiece_ops_faces::{Shell, ShellArgs};
    let shell = |b: &mut Standalone, faces: &[&str]| {
        Shell {
            id: "shell".into(),
            args: ShellArgs {
                faces: faces
                    .iter()
                    .map(|f| Source::Topology((*f).into()).into())
                    .collect(),
                thickness: Measure::new(0.5),
            },
        }
        .apply(b)
    };
    // A tray: the box open at its top.
    let mut b = Standalone::new();
    base(&mut b)?;
    shell(&mut b, &["base/top"])?;
    check(&b);
    close(
        volume(&b.world.topo, body(&b, "base")),
        32.0 - 9.0 * 1.5,
        1e-6,
    );
    // Its wall says how thick it is, once.
    assert_eq!(b.world.annotations["shell/thickness"].text(), "0,5");
    // The body itself picked: hollow, with no way in.
    let mut b = Standalone::new();
    base(&mut b)?;
    shell(&mut b, &["base"])?;
    let hollow = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    close(workpiece_boolean::measure(&hollow).volume, 32.0 - 9.0, 1e-9);
    // What is not there cannot be opened.
    let mut b = Standalone::new();
    base(&mut b)?;
    assert!(shell(&mut b, &["base/lid"]).is_err());
    Ok(())
}

#[test]
fn faces_are_drafted_about_a_plane() -> anyhow::Result<()> {
    use workpiece_ops_faces::{Draft, DraftArgs};
    let draft = |b: &mut Standalone, both: bool| {
        Draft {
            id: "draft".into(),
            args: DraftArgs {
                faces: vec![Source::Topology("base/right".into()).into()],
                neutral: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
                angle: Measure::new(10.0),
                flip: false,
                both,
            },
        }
        .apply(b)
    };
    // About the box's middle, one side leans in above it and out below:
    // as much is added as is taken.
    let mut b = Standalone::new();
    base(&mut b)?;
    draft(&mut b, false)?;
    check(&b);
    let made = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    close(workpiece_boolean::measure(&made).volume, 32.0, 1e-9);
    assert_eq!(made.faces.len(), 6);
    // Its angle is said of the model, for its drawing.
    assert_eq!(b.world.annotations["draft/angle"].text(), "10°");
    // Both ways from the middle, it leans in above and below: a ridge
    // along the side.
    let mut b = Standalone::new();
    base(&mut b)?;
    draft(&mut b, true)?;
    check(&b);
    let made = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    close(
        workpiece_boolean::measure(&made).volume,
        32.0 - 4.0 * 10f64.to_radians().tan(),
        1e-9,
    );
    Ok(())
}

#[test]
fn surfaces_are_thickened_into_solids() -> anyhow::Result<()> {
    use workpiece_ops_faces::{Thicken, ThickenArgs, ThickenSide};
    // The four walls of a 2 × 2 square, 2 tall, as a surface.
    let thick = |side: ThickenSide, sharp: bool| -> anyhow::Result<f64> {
        let mut b = Standalone::new();
        sketch(&mut b, "s", xy(), Some(([-1.0, -1.0], [1.0, 1.0])), &[])?;
        let mut walls = Extrude::of("walls", Some("s".into()), true);
        walls.args.distance = Measure::new(2.0);
        walls.apply(&mut b)?;
        Thicken {
            id: "thick".into(),
            args: ThickenArgs {
                faces: vec![Source::Topology("walls".into()).into()],
                distance: Measure::new(0.1),
                side,
                sharp,
                ..ThickenArgs::default()
            },
        }
        .apply(&mut b)?;
        check(&b);
        // The surface is used up.
        assert!(b.world.topo.resolve::<BodyId>("walls").is_none());
        Ok(volume(&b.world.topo, body(&b, "thick")))
    };
    let (flat, round) = (4.0 * 2.0 * 2.0 * 0.1, PI * 0.01 * 2.0);
    // Out of what the walls are around or into it; rounded where the
    // walls part, or run on to a sharp corner.
    let one = thick(ThickenSide::Front, false)?;
    let other = thick(ThickenSide::Back, false)?;
    let (out, into) = (one.max(other), one.min(other));
    close(out, flat + round, 2e-3);
    close(into, flat - 4.0 * 0.01 * 2.0, 1e-6);
    close(
        thick(ThickenSide::Front, true)?.max(thick(ThickenSide::Back, true)?),
        flat + 4.0 * 0.01 * 2.0,
        1e-6,
    );
    close(thick(ThickenSide::Both, true)?, 2.0 * flat, 1e-6);
    Ok(())
}

#[test]
fn areas_are_embossed_and_engraved_following_the_face() -> anyhow::Result<()> {
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    use workpiece_ops_faces::{Emboss, EmbossArgs};
    // A disk of radius 0.5 thrown along z onto the top of the 4 × 4 × 2
    // box: a flat pad, or a flat pocket.
    for (engrave, change) in [(false, 1.0), (true, -1.0)] {
        let mut b = Standalone::new();
        base(&mut b)?;
        sketch(&mut b, "disk", xy(), None, &[([0.0, 0.0], 0.5)])?;
        Emboss {
            id: "mark".into(),
            args: EmbossArgs {
                sketch: Some(Source::Profile("disk".into()).into()),
                faces: vec![Source::Topology("base/top".into()).into()],
                height: Measure::new(0.2),
                engrave,
                ..EmbossArgs::default()
            },
        }
        .apply(&mut b)?;
        check(&b);
        let made = volume(&b.world.topo, body(&b, "base"));
        close(made - 32.0, change * PI * 0.25 * 0.2, 2e-3);
    }
    // A square 0.6 wide thrown sideways onto a rod of radius 1: what is
    // raised follows the rod, and only on the side the sketch is.
    let mut b = Standalone::new();
    AddCylinder {
        id: "rod".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(1.0),
            height: Measure::new(2.0),
        },
    }
    .apply(&mut b)?;
    let side: Reference<workpiece_core::world::DatumPlane> = Reference::frame_axis(ORIGIN, Axis::X);
    let plane = b.world.resolve(&side)?;
    let flat = |p: [f64; 3]| {
        let d = workpiece_core::math::Point3::from_array(p) - plane.origin();
        [d.dot(plane.x_dir()), d.dot(plane.y_dir())]
    };
    let (a, c) = (flat([0.0, -0.3, 0.7]), flat([0.0, 0.3, 1.3]));
    let lo = [a[0].min(c[0]), a[1].min(c[1])];
    let hi = [a[0].max(c[0]), a[1].max(c[1])];
    sketch(&mut b, "square", side, Some((lo, hi)), &[])?;
    Emboss {
        id: "mark".into(),
        args: EmbossArgs {
            sketch: Some(Source::Profile("square".into()).into()),
            faces: vec![Source::Topology("rod/side".into()).into()],
            height: Measure::new(0.1),
            ..EmbossArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    // Between the radii 1 and 1.1, over the angle the square is seen
    // under from the axis at each, 0.6 tall: the area between two arcs
    // and the square's sides.
    let segment = |r: f64| {
        let half = (0.3_f64 / r).asin();
        r * r * half + 0.3 * (r * r - 0.09).sqrt()
    };
    let raised = (segment(1.1) - segment(1.0)) * 0.6;
    // (Measured on the solid itself: sampled, the rod alone is short by
    // as much as half of what is raised.)
    let rod = workpiece_boolean::read(&b.world.topo, body(&b, "rod"))?;
    close(
        workpiece_boolean::measure(&rod).volume - 2.0 * PI,
        raised,
        1e-6,
    );
    Ok(())
}

#[test]
fn an_area_is_raised_where_it_first_meets_a_ring_its_sides_as_asked() -> anyhow::Result<()> {
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    use workpiece_ops_faces::{Emboss, EmbossArgs, Outline};
    use workpiece_ops_surface::{CurveProjection, CurveProjectionArgs};
    // A square 0.6 wide on a plane 3 off a rod of radius 1, thrown
    // along x onto its side: it meets the rod twice, on both sides.
    let rod = || -> anyhow::Result<Standalone> {
        let mut b = Standalone::new();
        AddCylinder {
            id: "rod".into(),
            args: AddCylinderArgs {
                frame: Some(Reference::datum(ORIGIN)),
                radius: Measure::new(1.0),
                height: Measure::new(2.0),
            },
        }
        .apply(&mut b)?;
        let plane = workpiece_core::world::DatumPlane::new(
            workpiece_core::math::Point3::new(3.0, 0.0, 0.0),
            workpiece_core::math::Point3::X,
            workpiece_core::math::Point3::Y,
        )?;
        b.world.datums.insert("front".into(), plane.into());
        let flat = |p: [f64; 3]| {
            let d = workpiece_core::math::Point3::from_array(p) - plane.origin();
            [d.dot(plane.x_dir()), d.dot(plane.y_dir())]
        };
        let (a, c) = (flat([3.0, -0.3, 0.7]), flat([3.0, 0.3, 1.3]));
        let lo = [a[0].min(c[0]), a[1].min(c[1])];
        let hi = [a[0].max(c[0]), a[1].max(c[1])];
        sketch(
            &mut b,
            "square",
            Reference::datum("front"),
            Some((lo, hi)),
            &[],
        )?;
        Ok(b)
    };
    let raised = |b: &mut Standalone, args: EmbossArgs| -> anyhow::Result<(f64, f64, f64)> {
        Emboss {
            id: "mark".into(),
            args: EmbossArgs {
                faces: vec![Source::Topology("rod/side".into()).into()],
                height: Measure::new(0.1),
                ..args
            },
        }
        .apply(b)?;
        check(b);
        let rod = workpiece_boolean::read(&b.world.topo, body(b, "rod"))?;
        let x = rod.vertices.iter().map(|v| v.point.x);
        let (lo, hi) = x.fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| {
            (l.min(x), h.max(x))
        });
        Ok((workpiece_boolean::measure(&rod).volume - 2.0 * PI, lo, hi))
    };
    let thrown = || EmbossArgs {
        sketch: Some(Source::Profile("square".into()).into()),
        ..EmbossArgs::default()
    };
    // Text on the plane: its letters (an "A" and a "b", each with a
    // hole) raised on the near side only, either way.
    let lettered = |square: bool| -> anyhow::Result<(f64, f64, f64)> {
        use workpiece_ops_sketch::{Align, Shape, Sketch, TextBox};
        let mut b = rod()?;
        let mut text = Sketch::new("text");
        text.args.plane = Some(Reference::datum("front"));
        let corners = [[-0.5, 0.7], [0.5, 0.7], [0.5, 1.3], [-0.5, 1.3]].map(|p| {
            text.args
                .add_point(workpiece_core::math::Point2::from_array(p))
        });
        text.args.add(Shape::Text(TextBox {
            corners,
            text: "Ab".into(),
            size: 0.5,
            align: Align::Start,
            valign: Align::Start,
            wrap: false,
        }));
        text.apply(&mut b)?;
        let args = EmbossArgs {
            sketch: Some(Source::Profile("text".into()).into()),
            normal_to_face: square,
            ..EmbossArgs::default()
        };
        raised(&mut b, args)
    };
    let (along, lo, hi) = lettered(false)?;
    assert!(
        along > 0.0 && hi > 1.05 && lo > -1.0 - 1e-6,
        "{along} {lo} {hi}"
    );
    let (normal, lo, hi) = lettered(true)?;
    assert!(
        normal > 0.0 && hi > 1.05 && lo > -1.0 - 1e-6,
        "{normal} {lo} {hi}"
    );
    assert!((normal - along).abs() > 1e-5, "{normal} {along}");
    // Along the sketch's normal: on the near side only, its sides along x.
    let segment = |r: f64| {
        let half = (0.3_f64 / r).asin();
        r * r * half + 0.3 * (r * r - 0.09).sqrt()
    };
    let (along, lo, hi) = raised(&mut rod()?, thrown())?;
    close(along, (segment(1.1) - segment(1.0)) * 0.6, 1e-6);
    assert!(hi > 1.05 && lo > -1.0 - 1e-6, "{lo} {hi}");
    // Square to the face: its sides run out from the axis, standing on
    // what the square covers of the rod.
    let wedge = |angle: f64| (1.1 * 1.1 - 1.0) / 2.0 * angle * 0.6;
    let square = EmbossArgs {
        normal_to_face: true,
        ..thrown()
    };
    let (normal, lo, hi) = raised(&mut rod()?, square)?;
    close(normal, wedge(2.0 * 0.3_f64.asin()), 1e-4);
    assert!(hi > 1.05 && lo > -1.0 - 1e-6, "{lo} {hi}");
    assert!((normal - along).abs() > 1e-3, "{normal} {along}");
    // The square projected onto the rod first: curves on both its
    // sides, each raised square to the face.
    let mut b = rod()?;
    CurveProjection {
        id: "projected".into(),
        args: CurveProjectionArgs {
            onto: vec![Source::Topology("rod/side".into()).into()],
            curves: vec![Source::Profile("square".into()).into()],
            along: None,
        },
    }
    .apply(&mut b)?;
    let curves = EmbossArgs {
        outline: Outline::Curves,
        curves: vec![Source::Topology("projected".into()).into()],
        ..EmbossArgs::default()
    };
    let (both, lo, hi) = raised(&mut b, curves)?;
    close(both, 2.0 * normal, 1e-4);
    assert!(hi > 1.05 && lo < -1.04, "{lo} {hi}");
    Ok(())
}

#[test]
fn faces_are_pushed_and_pulled() -> anyhow::Result<()> {
    use workpiece_ops_faces::{FaceWay, MoveFace, MoveFaceArgs};
    // The 4 × 4 × 2 box with a hole of radius 0.5 through it.
    let drilled = || -> anyhow::Result<(Standalone, String)> {
        let mut b = Standalone::new();
        base(&mut b)?;
        sketch(&mut b, "circle", xy(), None, &[([0.0, 0.0], 0.5)])?;
        let mut hole = Extrude::of("hole", Some("circle".into()), false);
        hole.args.extent = Extent::Symmetric;
        hole.args.distance = Measure::new(4.0);
        hole.args.combine = Combine::Cut;
        hole.apply(&mut b)?;
        let topo = &b.world.topo;
        let faces = topo.body_faces(body(&b, "base")).expect("faces");
        let wall = faces
            .iter()
            .filter_map(|f| topo.name_of(*f))
            .find(|n| n.starts_with("hole/side"))
            .expect("the wall")
            .to_owned();
        Ok((b, wall))
    };
    let exact = |b: &Standalone| -> anyhow::Result<f64> {
        check(b);
        Ok(
            workpiece_boolean::measure(&workpiece_boolean::read(&b.world.topo, body(b, "base"))?)
                .volume,
        )
    };
    let moved = |faces: Vec<String>, set: &dyn Fn(&mut MoveFaceArgs)| -> anyhow::Result<f64> {
        let (mut b, wall) = drilled()?;
        let faces = faces
            .into_iter()
            .map(|f| if f == "wall" { wall.clone() } else { f });
        let mut op = MoveFace {
            id: "push".into(),
            args: MoveFaceArgs {
                faces: faces.map(|n| Source::Topology(n).into()).collect(),
                distance: Measure::new(0.5),
                ..MoveFaceArgs::default()
            },
        };
        set(&mut op.args);
        op.apply(&mut b)?;
        exact(&b)
    };
    let area = 16.0 - PI * 0.25;
    // The top lifted or sunk by 0.5: the hole runs on, or is cut back.
    close(moved(vec!["base/top".into()], &|_| {})?, area * 2.5, 1e-9);
    close(
        moved(vec!["base/top".into()], &|a| a.flip = true)?,
        area * 1.5,
        1e-9,
    );
    // The hole widened by 0.5 (its wall moved into the body), or
    // narrowed by 0.25.
    close(
        moved(vec!["wall".into()], &|a| a.flip = true)?,
        (16.0 - PI) * 2.0,
        1e-6,
    );
    close(
        moved(vec!["wall".into()], &|a| a.distance = Measure::new(0.25))?,
        (16.0 - PI * 0.0625) * 2.0,
        1e-6,
    );
    // A side moved along the x axis, the way it faces or back.
    let front = |flip: bool| {
        moved(vec!["base/front".into()], &|a| {
            a.way = FaceWay::Along;
            a.axis = Some(Reference::frame_axis(ORIGIN, Axis::X));
            a.flip = flip;
        })
    };
    close(front(false)?, area * 2.0 + 4.0, 1e-9);
    close(front(true)?, area * 2.0 - 4.0, 1e-9);
    // The top replaced by a plane half a unit above it, or below: the
    // body runs on to it, or is cut back.
    for (height, volume) in [(1.5, area * 2.5), (0.5, area * 1.5)] {
        let (mut b, _) = drilled()?;
        let pose = workpiece_core::Pose::from_translation(workpiece_core::math::Point3::new(
            0.0, 0.0, height,
        ));
        b.world.datums.insert(
            "level".to_owned(),
            workpiece_core::world::Datum::Frame(workpiece_core::world::DatumFrame::from_pose(
                pose,
            )?),
        );
        MoveFace {
            id: "replace".into(),
            args: MoveFaceArgs {
                faces: vec![Source::Topology("base/top".into()).into()],
                way: FaceWay::ToPlane,
                plane: Some(Reference::frame_axis("level", Axis::Z)),
                ..MoveFaceArgs::default()
            },
        }
        .apply(&mut b)?;
        close(exact(&b)?, volume, 1e-9);
    }
    Ok(())
}

#[test]
fn a_slot_on_a_flange_as_thick_as_the_wall_is_shelled_sharp_inside() -> anyhow::Result<()> {
    use workpiece_ops_faces::{Shell, ShellArgs};
    // A flange 100 × 60 × 5 with a slot 40 wide, 40 between its round
    // ends' centres and 30 high on it, hollowed from below to walls 5
    // thick: the inner wall's foot lies in the flange's top face. The
    // hollow is the flange less its wall and the slot 30 wide, sharp
    // where the slot's walls meet the flange's top from inside.
    let mut b = Standalone::new();
    sketch(&mut b, "f", xy(), Some(([-50.0, -30.0], [50.0, 30.0])), &[])?;
    let mut flange = Extrude::of("flange", Some("f".into()), false);
    flange.args.combine = Combine::NewBody;
    flange.args.distance = Measure::new(5.0);
    flange.apply(&mut b)?;
    let mut s = Sketch::new("s");
    s.args.plane = Some(xy());
    let at = |args: &mut workpiece_ops_sketch::SketchArgs, u: f64, v: f64| {
        args.add_point(Point2::new(u, v))
    };
    let (a, c) = (at(&mut s.args, -20.0, -20.0), at(&mut s.args, 20.0, -20.0));
    let (d, e) = (at(&mut s.args, 20.0, 20.0), at(&mut s.args, -20.0, 20.0));
    let (right, left) = (at(&mut s.args, 20.0, 0.0), at(&mut s.args, -20.0, 0.0));
    s.args.add_line(a, c);
    s.args.add(Shape::Arc {
        center: right,
        start: c,
        end: d,
    });
    s.args.add_line(d, e);
    s.args.add(Shape::Arc {
        center: left,
        start: e,
        end: a,
    });
    s.apply(&mut b)?;
    let mut slot = Extrude::of("slot", Some("s".into()), false);
    slot.args.distance = Measure::new(30.0);
    slot.apply(&mut b)?;
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "flange"))?;
    let bottom = solid
        .faces
        .iter()
        .find(|f| f.name.as_deref() == Some("flange/start"))
        .and_then(|f| f.name.clone())
        .expect("the flange's bottom");
    Shell {
        id: "shell".into(),
        args: ShellArgs {
            faces: vec![Source::Topology(bottom).into()],
            thickness: Measure::new(5.0),
        },
    }
    .apply(&mut b)?;
    check(&b);
    let slot = |w: f64| 40.0 * w + PI * (w / 2.0).powi(2);
    let sharp = 6000.0 * 5.0 + slot(40.0) * 25.0 - slot(30.0) * 25.0;
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "flange"))?;
    close(workpiece_boolean::measure(&solid).volume, sharp, 1e-6);
    Ok(())
}

#[test]
fn a_moved_face_has_an_arrow_for_how_far() -> anyhow::Result<()> {
    use workpiece_core::handles::Handle;
    use workpiece_ops_faces::{MoveFace, MoveFaceArgs};
    // The top of the 4 × 4 × 2 box, out by 0.5: an arrow up from its
    // middle to where it goes.
    let mut b = Standalone::new();
    base(&mut b)?;
    let op = MoveFace {
        id: "moved".into(),
        args: MoveFaceArgs {
            faces: vec![Source::Topology("base/top".into()).into()],
            distance: Measure::new(0.5),
            ..MoveFaceArgs::default()
        },
    };
    let edit = workpiece_core::operation::edit_arguments(&op, &b.world);
    let Some(Handle::Linear(arrow)) = edit.handles.get("distance") else {
        panic!("an arrow");
    };
    let top = workpiece_core::math::Point3::new(0.0, 0.0, 1.0);
    assert!(arrow.origin.distance(top) < 1e-6, "{}", arrow.origin);
    assert!(
        arrow
            .tip()
            .distance(top + workpiece_core::math::Point3::Z * 0.5)
            < 1e-6
    );
    // Dragged back through the face: in, by as much.
    let args = (arrow.on_drag)(-0.25);
    assert!(args.flip && (args.distance.value - 0.25).abs() < 1e-9);
    Ok(())
}
