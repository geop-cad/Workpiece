//! What the operations of this crate make.

use shapecraft_ops_sweep::{Extent, Extrude};
use shapecraft_ops_testing::*;
use shapecraft_shape::Combine;

#[test]
fn slabs_stand_on_faces_of_every_kind() -> anyhow::Result<()> {
    use shapecraft_ops_basic_shapes::{AddCylinder, AddCylinderArgs, AddSphere, AddSphereArgs};
    let face = |solid: &shapecraft_boolean::Solid, name: &str| {
        let found = solid
            .faces
            .iter()
            .position(|f| f.name.as_deref() == Some(name));
        found.ok_or_else(|| anyhow::anyhow!("no face {name}"))
    };
    // On a box's top, out of it and into it.
    let mut b = Standalone::new();
    base(&mut b)?;
    let solid = shapecraft_boolean::read(&b.world.topo, body(&b, "base"))?;
    let top = face(&solid, "base/top")?;
    for d in [0.5, -0.5] {
        let (volume, area) = sound(&shapecraft_shape::thick::slab(&solid, top, d, "slab")?)?;
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
    let rod = shapecraft_boolean::read(&b.world.topo, body(&b, "rod"))?;
    let side = face(&rod, "rod/side")?;
    for (d, wanted) in [
        (0.5, PI * (2.5 * 2.5 - 4.0) * 3.0),
        (-0.5, PI * (4.0 - 1.5 * 1.5) * 3.0),
    ] {
        let (volume, _) = sound(&shapecraft_shape::thick::slab(&rod, side, d, "slab")?)?;
        close(volume, wanted, 1e-9);
    }
    // And on its flat end, which comes to a point in its middle.
    let end = face(&rod, "rod/top")?;
    let (volume, _) = sound(&shapecraft_shape::thick::slab(&rod, end, 0.5, "slab")?)?;
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
    let ball = shapecraft_boolean::read(&b.world.topo, body(&b, "ball"))?;
    let (volume, area) = sound(&shapecraft_shape::thick::slab(&ball, 0, 0.5, "slab")?)?;
    close(volume, 4.0 / 3.0 * PI * (2.5f64.powi(3) - 8.0), 1e-9);
    close(area, 4.0 * PI * (6.25 + 4.0), 1e-9);
    Ok(())
}

#[test]
fn bodies_grow_shrink_and_hollow() -> anyhow::Result<()> {
    let mut b = Standalone::new();
    base(&mut b)?;
    let solid = shapecraft_boolean::read(&b.world.topo, body(&b, "base"))?;
    // Shrunk by a half: the box a half in from every side.
    let (volume, area) = sound(&shapecraft_shape::thick::thickened(
        &solid, -0.5, None, false,
    )?)?;
    close(volume, 9.0, 1e-9);
    close(area, 2.0 * (9.0 + 3.0 + 3.0), 1e-9);
    // Hollowed, open at its top: a tray.
    let top = solid
        .faces
        .iter()
        .position(|f| f.name.as_deref() == Some("base/top"));
    let (volume, _) = sound(&shapecraft_shape::thick::thickened(
        &solid,
        -0.5,
        Some(&[top.expect("a top")]),
        false,
    )?)?;
    close(volume, 32.0 - 9.0 * 1.5, 1e-9);
    // Grown by a half: its faces moved out, its edges and corners round.
    let (volume, area) = sound(&shapecraft_shape::thick::thickened(
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
    use shapecraft_ops_faces::{Shell, ShellArgs};
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
    let hollow = shapecraft_boolean::read(&b.world.topo, body(&b, "base"))?;
    close(
        shapecraft_boolean::measure(&hollow).volume,
        32.0 - 9.0,
        1e-9,
    );
    // What is not there cannot be opened.
    let mut b = Standalone::new();
    base(&mut b)?;
    assert!(shell(&mut b, &["base/lid"]).is_err());
    Ok(())
}

#[test]
fn faces_are_drafted_about_a_plane() -> anyhow::Result<()> {
    use shapecraft_ops_faces::{Draft, DraftArgs};
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
    let made = shapecraft_boolean::read(&b.world.topo, body(&b, "base"))?;
    close(shapecraft_boolean::measure(&made).volume, 32.0, 1e-9);
    assert_eq!(made.faces.len(), 6);
    // Its angle is said of the model, for its drawing.
    assert_eq!(b.world.annotations["draft/angle"].text(), "10°");
    // Both ways from the middle, it leans in above and below: a ridge
    // along the side.
    let mut b = Standalone::new();
    base(&mut b)?;
    draft(&mut b, true)?;
    check(&b);
    let made = shapecraft_boolean::read(&b.world.topo, body(&b, "base"))?;
    close(
        shapecraft_boolean::measure(&made).volume,
        32.0 - 4.0 * 10f64.to_radians().tan(),
        1e-9,
    );
    Ok(())
}

#[test]
fn surfaces_are_thickened_into_solids() -> anyhow::Result<()> {
    use shapecraft_ops_faces::{Thicken, ThickenArgs, ThickenSide};
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
    use shapecraft_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    use shapecraft_ops_faces::{Emboss, EmbossArgs};
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
    let side: Reference<shapecraft_core::world::DatumPlane> =
        Reference::frame_axis(ORIGIN, Axis::X);
    let plane = b.world.resolve(&side)?;
    let flat = |p: [f64; 3]| {
        let d = shapecraft_core::math::Point3::from_array(p) - plane.origin();
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
    let rod = shapecraft_boolean::read(&b.world.topo, body(&b, "rod"))?;
    close(
        shapecraft_boolean::measure(&rod).volume - 2.0 * PI,
        raised,
        1e-6,
    );
    Ok(())
}

#[test]
fn faces_are_pushed_and_pulled() -> anyhow::Result<()> {
    use shapecraft_ops_faces::{FaceWay, MoveFace, MoveFaceArgs};
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
            shapecraft_boolean::measure(&shapecraft_boolean::read(&b.world.topo, body(b, "base"))?)
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
        let pose = shapecraft_core::Pose::from_translation(shapecraft_core::math::Point3::new(
            0.0, 0.0, height,
        ));
        b.world.datums.insert(
            "level".to_owned(),
            shapecraft_core::world::Datum::Frame(shapecraft_core::world::DatumFrame::from_pose(
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
fn deleted_faces_are_closed_over() -> anyhow::Result<()> {
    use shapecraft_ops_faces::{DeleteFace, DeleteFaceArgs};
    let exact = |b: &Standalone| -> anyhow::Result<f64> {
        check(b);
        Ok(
            shapecraft_boolean::measure(&shapecraft_boolean::read(&b.world.topo, body(b, "base"))?)
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
        DeleteFace {
            id: "heal".into(),
            args: DeleteFaceArgs {
                faces: shapecraft_core::Reference::each(faces),
            },
        }
        .apply(&mut b)?;
        close(exact(&b)?, 32.0, 1e-9);
        let healed = shapecraft_boolean::read(&b.world.topo, body(&b, "base"))?;
        assert_eq!(healed.faces.len(), 6);
    }
    Ok(())
}

#[test]
fn a_slot_on_a_flange_as_thick_as_the_wall_is_shelled_sharp_inside() -> anyhow::Result<()> {
    use shapecraft_ops_faces::{Shell, ShellArgs};
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
    let at = |args: &mut shapecraft_ops_sketch::SketchArgs, u: f64, v: f64| {
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
    let solid = shapecraft_boolean::read(&b.world.topo, body(&b, "flange"))?;
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
    let solid = shapecraft_boolean::read(&b.world.topo, body(&b, "flange"))?;
    close(shapecraft_boolean::measure(&solid).volume, sharp, 1e-6);
    Ok(())
}
