//! What the operations of this crate make.

use shapecraft_ops_sweep::{Extent, Extrude};
use shapecraft_ops_testing::*;
use shapecraft_shape::Combine;

#[test]
fn holes_are_drilled_at_the_points_of_a_sketch() -> anyhow::Result<()> {
    use shapecraft_ops_features::{Hole, HoleArgs, HoleKind};
    // Two points in the middle plane of the 4 × 4 × 2 box: the holes go
    // down from there.
    let drilled = |set: &dyn Fn(&mut HoleArgs)| -> anyhow::Result<f64> {
        let mut b = Standalone::new();
        base(&mut b)?;
        let mut points = Sketch::new("points");
        points.args.plane = Some(xy());
        for at in [[1.0, 1.0], [-1.0, -1.0]] {
            let p = points.args.add_point(Point2::from_array(at));
            points.args.add(Shape::Point(p));
        }
        points.apply(&mut b)?;
        let mut hole = Hole {
            id: "hole".into(),
            args: HoleArgs {
                sketch: Some(Source::Profile("points".into()).into()),
                diameter: Measure::new(0.5),
                depth: Measure::new(0.6),
                point: Measure::new(180.0),
                ..HoleArgs::default()
            },
        };
        set(&mut hole.args);
        hole.apply(&mut b)?;
        check(&b);
        Ok(32.0 - volume(&b.world.topo, body(&b, "base")))
    };
    let bore = PI * 0.25 * 0.25 * 0.6;
    close(drilled(&|_| {})?, 2.0 * bore, 0.01);
    close(
        drilled(&|a| a.extent = Extent::ThroughAll)?,
        2.0 * bore / 0.6,
        0.01,
    );
    // A drill's point: a cone of the bore's radius.
    let tip = PI * 0.25 * 0.25 * 0.25 / (59.0_f64.to_radians().tan()) / 3.0;
    close(
        drilled(&|a| a.point = Measure::new(118.0))?,
        2.0 * (bore + tip),
        0.01,
    );
    // A counterbore 1 wide and 0.25 deep, around the bore.
    let head = PI * 0.25 * 0.25 - PI * 0.25 * 0.25 * 0.25;
    close(
        drilled(&|a| {
            a.kind = HoleKind::Counterbore;
            a.head = Measure::new(1.0);
            a.head_depth = Measure::new(0.25);
        })?,
        2.0 * (bore + head),
        0.01,
    );
    // A countersink 1 wide at 90°: a cone's frustum 0.25 deep.
    let cone = PI * 0.25 / 3.0 * (0.25 + 0.0625 + 0.125) - PI * 0.25 * 0.25 * 0.25;
    close(
        drilled(&|a| {
            a.kind = HoleKind::Countersink;
            a.head = Measure::new(1.0);
        })?,
        2.0 * (bore + cone),
        0.01,
    );
    // A tapped hole is drilled at the thread's diameter less its pitch.
    close(
        drilled(&|a| {
            a.kind = HoleKind::Tapped;
            a.pitch = Measure::new(0.1);
        })?,
        2.0 * PI * 0.2 * 0.2 * 0.6,
        0.01,
    );
    Ok(())
}

#[test]
fn holes_are_placed_off_planes_and_around_axes() -> anyhow::Result<()> {
    use shapecraft_ops_features::{Hole, HoleArgs, HolePlace};
    // The box 4 × 4 × 2 about the origin; a hole ⌀1 through its top.
    let through = |place: HoleArgs| HoleArgs {
        diameter: Measure::new(1.0),
        extent: Extent::ThroughAll,
        face: Some(Source::Topology("base/top".into()).into()),
        ..place
    };
    let hole_at = |b: &Standalone| {
        // (The middle of the rims of the bore: of its round edges.)
        let solid = shapecraft_boolean::read(&b.world.topo, body(b, "base")).unwrap();
        let rims: Vec<_> = solid
            .edges
            .iter()
            .filter(|e| e.curve.degree() > 1)
            .map(|e| {
                // (A whole circle, or half of one.)
                let (a, c) = e.range;
                let across = if e.ends[0] == e.ends[1] {
                    (a + c) / 2.0
                } else {
                    c
                };
                (e.curve.point_at(a) + e.curve.point_at(across)) / 2.0
            })
            .collect();
        rims.iter().sum::<shapecraft_core::math::Point3>() / rims.len() as f64
    };
    // 0.5 in from the right side and 1.5 in from the back.
    let mut b = Standalone::new();
    base(&mut b)?;
    Hole {
        id: "hole".into(),
        args: through(HoleArgs {
            place: HolePlace::Offsets,
            first: Some(Source::Topology("base/right".into()).into()),
            first_offset: Measure::new(0.5),
            second: Some(Source::Topology("base/back".into()).into()),
            second_offset: Measure::new(1.5),
            ..Default::default()
        }),
    }
    .apply(&mut b)?;
    check(&b);
    close(
        volume(&b.world.topo, body(&b, "base")),
        32.0 - PI / 2.0,
        0.01,
    );
    let at = hole_at(&b);
    // (The box's right is across y, its back across x.)
    close(at.y.abs(), 1.5, 1e-9);
    close(at.x.abs(), 0.5, 1e-9);
    // 1.2 from the axis z of the origin, a quarter turn around.
    let mut b = Standalone::new();
    base(&mut b)?;
    Hole {
        id: "hole".into(),
        args: through(HoleArgs {
            place: HolePlace::Polar,
            axis: Some(Reference::frame_axis(ORIGIN, Axis::Z)),
            radius: Measure::new(1.2),
            angle: Measure::new(90.0),
            ..Default::default()
        }),
    }
    .apply(&mut b)?;
    check(&b);
    close(
        volume(&b.world.topo, body(&b, "base")),
        32.0 - PI / 2.0,
        0.01,
    );
    let at = hole_at(&b);
    close(at.x.hypot(at.y), 1.2, 1e-9);
    close(at.x * at.y, 0.0, 1e-9);
    Ok(())
}

#[test]
fn threads_are_cut_into_rods_and_holes() -> anyhow::Result<()> {
    use shapecraft_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    use shapecraft_ops_features::{Thread, ThreadArgs};
    // What a V groove takes from a face of this radius in a turn: its
    // triangle (less the lip beyond the face), carried around.
    let (pitch, depth) = (0.25, 0.12);
    let tan = (PI / 6.0).tan();
    let turn = |radius: f64, hole: bool| {
        // The triangle within the material, and how far its middle is
        // from the axis.
        let area = depth * depth * tan;
        let middle = if hole {
            radius + depth / 3.0
        } else {
            radius - depth / 3.0
        };
        area * 2.0 * PI * middle
    };
    for left in [false, true] {
        let mut b = Standalone::new();
        AddCylinder {
            id: "rod".into(),
            args: AddCylinderArgs {
                frame: Some(Reference::datum(ORIGIN)),
                radius: Measure::new(0.5),
                height: Measure::new(2.0),
            },
        }
        .apply(&mut b)?;
        Thread {
            id: "thread".into(),
            args: ThreadArgs {
                face: Some(Source::Topology("rod/side".into()).into()),
                pitch: Measure::new(pitch),
                depth: Measure::new(depth),
                left,
                modelled: true,
                custom: true,
            },
        }
        .apply(&mut b)?;
        check(&b);
        let taken = PI * 0.25 * 2.0 - volume(&b.world.topo, body(&b, "rod"));
        // Eight turns in the rod's length, to within what the helix's
        // slant and the mesh are off by.
        close(taken, 8.0 * turn(0.5, false), 0.01);
    }
    // A hole through the 4 × 4 × 2 box: its wall takes a thread too.
    let mut b = Standalone::new();
    base(&mut b)?;
    sketch(&mut b, "circle", xy(), None, &[([0.0, 0.0], 0.5)])?;
    let mut hole = Extrude::of("hole", Some("circle".into()), false);
    hole.args.extent = Extent::Symmetric;
    hole.args.distance = Measure::new(4.0);
    hole.args.combine = Combine::Cut;
    hole.apply(&mut b)?;
    let before = volume(&b.world.topo, body(&b, "base"));
    let wall = {
        let topo = &b.world.topo;
        let faces = topo.body_faces(body(&b, "base")).expect("faces");
        faces
            .iter()
            .filter_map(|f| topo.name_of(*f))
            .find(|n| n.starts_with("hole/side"))
            .expect("the wall")
            .to_owned()
    };
    Thread {
        id: "thread".into(),
        args: ThreadArgs {
            face: Some(Source::Topology(wall).into()),
            pitch: Measure::new(pitch),
            depth: Measure::new(depth),
            left: false,
            modelled: true,
            custom: true,
        },
    }
    .apply(&mut b)?;
    check(&b);
    close(
        before - volume(&b.world.topo, body(&b, "base")),
        8.0 * turn(0.5, true),
        0.02,
    );
    Ok(())
}

#[test]
fn dialogs_are_made_from_the_arguments() {
    use shapecraft_core::{Arguments, Cache, EditContext, Widget};
    use shapecraft_ops_features::{HoleArgs, HoleKind, HolePlace};
    let world = std::sync::Arc::new(shapecraft_core::World::new());
    let cache = Cache::default();
    let context = EditContext::standalone(&world, &cache);
    let mut args = HoleArgs::default();
    // Placement, sketch, kind, size, diameter, fit, extent, depth
    // beside its flip, point, deburring, body.
    let fields = args.fields(&context);
    assert_eq!(fields.len(), 11);
    assert!(matches!(&fields[7], Widget::HBox(row) if row.children.len() == 2));
    // A counterbore has a head, of a diameter and a depth, in a row,
    // and no fit.
    args.kind = HoleKind::Counterbore;
    let fields = args.fields(&context);
    assert_eq!(fields.len(), 12);
    assert!(matches!(&fields[6], Widget::HBox(row) if row.children.len() == 2));
    // Through all, there is no depth and no point (the flip stands
    // beside the extent), and how far through it goes: everything, or
    // to where it first comes out.
    args.extent = Extent::ThroughAll;
    assert_eq!(args.fields(&context).len(), 10);
    // For a screw size, the sizes are the standard's: in their place,
    // how close the hole is and which head it is for.
    args.size = Some("M6".into());
    assert_eq!(args.fields(&context).len(), 10);
    args.size = None;
    // A chamfer has a width and an angle, in a row.
    args.deburr = shapecraft_ops_features::Deburr::Both;
    assert_eq!(args.fields(&context).len(), 11);
    args.deburr = shapecraft_ops_features::Deburr::None;
    // What the arguments refer to is what their reference fields hold.
    args.sketch = Some(Source::Profile("s".into()).into());
    args.target = Some(Source::Topology("b".into()).into());
    assert_eq!(args.references().len(), 2);
    // A choice sets its field.
    let Widget::Choice(kind) = &args.fields(&context)[2] else {
        panic!("expected the kind");
    };
    assert_eq!(kind.set(3).map(|a| a.kind), Some(HoleKind::Tapped));
    // Off two planes: the face, and each plane with its distance, in
    // place of the sketch.
    args.place = HolePlace::Offsets;
    assert_eq!(args.fields(&context).len(), 10 - 1 + 5);
    // Around an axis: the face, the axis, and the distance beside the
    // angle.
    args.place = HolePlace::Polar;
    assert_eq!(args.fields(&context).len(), 10 - 1 + 3);
}

#[test]
fn a_cosmetic_thread_is_said_of_its_face_and_changes_nothing() -> anyhow::Result<()> {
    use shapecraft_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    use shapecraft_ops_features::{Thread, ThreadArgs};
    let mut b = Standalone::new();
    AddCylinder {
        id: "rod".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(3.0),
            height: Measure::new(20.0),
        },
    }
    .apply(&mut b)?;
    let before = volume(&b.world.topo, body(&b, "rod"));
    Thread {
        id: "thread".into(),
        args: ThreadArgs {
            face: Some(Source::Topology("rod/side".into()).into()),
            ..ThreadArgs::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    close(volume(&b.world.topo, body(&b, "rod")), before, 1e-9);
    // A rod 6 across is M6: a pitch of 1, its roots 0.6134 deep.
    let said = &b.world.threads["thread"];
    assert_eq!((said.designation.as_str(), said.pitch), ("M6", 1.0));
    assert!(!said.internal && !said.left);
    close(said.major, 6.0, 1e-9);
    close(said.minor, 6.0 - 2.0 * 0.6134, 1e-9);
    close(said.length, 20.0, 1e-9);
    // Its designation on a leader to the rod's outline, best from the
    // side.
    let callout = &b.world.annotations["thread/callout"];
    let shapecraft_core::world::Note::Hole { lines, outside, .. } = &callout.note else {
        panic!("a callout");
    };
    assert!(*outside);
    assert_eq!(callout.text(), "M6");
    assert_eq!(lines.len(), 1);
    Ok(())
}

#[test]
fn holes_take_their_sizes_from_the_standards_and_are_chamfered() -> anyhow::Result<()> {
    use shapecraft_ops_basic_shapes::{AddBox, AddBoxArgs};
    use shapecraft_ops_features::{Deburr, Hole, HoleArgs, HoleKind};
    // One point in the middle plane of a 40 × 40 × 40 block: the hole
    // goes down from there, through 20.
    let drilled = |set: &dyn Fn(&mut HoleArgs)| -> anyhow::Result<(f64, Standalone)> {
        let mut b = Standalone::new();
        AddBox {
            id: "block".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(ORIGIN)),
                size: [40.0; 3].map(Measure::new),
            },
        }
        .apply(&mut b)?;
        let mut points = Sketch::new("points");
        points.args.plane = Some(xy());
        let p = points.args.add_point(Point2::new(0.0, 0.0));
        points.args.add(Shape::Point(p));
        points.apply(&mut b)?;
        let mut hole = Hole {
            id: "hole".into(),
            args: HoleArgs {
                sketch: Some(Source::Profile("points".into()).into()),
                size: Some("M6".into()),
                point: Measure::new(180.0),
                ..HoleArgs::default()
            },
        };
        set(&mut hole.args);
        hole.apply(&mut b)?;
        check(&b);
        let taken = 40.0 * 40.0 * 40.0 - volume(&b.world.topo, body(&b, "block"));
        Ok((taken, b))
    };
    let disk = |across: f64, deep: f64| PI * across * across / 4.0 * deep;
    // (Volumes are of meshes: to a hundredth.)
    let about = |got: f64, wanted: f64| close(got, wanted, wanted * 0.01);
    // For M6, medium: a hole 6.6 across.
    let (blind, _) = drilled(&|_| {})?;
    about(blind, disk(6.6, 10.0));
    // A socket head's counterbore: 11 across, 6.4 deep.
    about(
        drilled(&|a| a.kind = HoleKind::Counterbore)?.0,
        disk(6.6, 10.0) + disk(11.0, 6.4) - disk(6.6, 6.4),
    );
    // Tapped: drilled 5 across, the thread said of its wall.
    let (taken, b) = drilled(&|a| a.kind = HoleKind::Tapped)?;
    about(taken, disk(5.0, 10.0));
    let said = &b.world.threads["hole/thread"];
    assert_eq!((said.designation.as_str(), said.internal), ("M6", true));
    close(said.major, 6.0, 1e-9);
    close(said.minor, 5.0, 1e-9);
    close(said.length, 10.0, 1e-9);
    let wall = b
        .world
        .topo
        .resolve::<shapecraft_core::world::topo::FaceId>(&said.face);
    assert!(wall.is_some(), "{}", said.face);
    // A fine pitch: M6×0.75, drilled 5.25 across. Through, the thread
    // is as long as the block is thick.
    let (taken, b) = drilled(&|a| {
        a.kind = HoleKind::Tapped;
        a.fine = Some("0.75".into());
        a.extent = Extent::ThroughAll;
    })?;
    about(taken, disk(5.25, 20.0));
    let said = &b.world.threads["hole/thread"];
    assert_eq!(said.designation, "M6×0.75");
    close(said.length, 20.0, 1e-6);
    // Modelled, the thread is cut: more is taken than the drill takes.
    let (taken, b) = drilled(&|a| {
        a.kind = HoleKind::Tapped;
        a.modelled = true;
    })?;
    assert!(taken > disk(5.0, 10.0) * 1.05, "{taken}");
    assert!(b.world.threads.is_empty());
    // Chamfered 0.5 wide at 45° where it goes in and where it comes
    // out: a cone's frustum more at each end.
    let frustum = PI * 0.5 / 3.0 * (3.8 * 3.8 + 3.8 * 3.3 + 3.3 * 3.3) - disk(6.6, 0.5);
    let (plain, _) = drilled(&|a| a.extent = Extent::ThroughAll)?;
    about(plain, disk(6.6, 20.0));
    let (both, _) = drilled(&|a| {
        a.extent = Extent::ThroughAll;
        a.deburr = Deburr::Both;
    })?;
    close(both - plain, 2.0 * frustum, 0.3);
    close(
        drilled(&|a| a.deburr = Deburr::Entry)?.0 - blind,
        frustum,
        0.2,
    );
    // Only a hole that goes through comes out.
    assert!(drilled(&|a| a.deburr = Deburr::Exit).is_err());
    Ok(())
}

#[test]
fn a_counterbore_says_its_head() -> anyhow::Result<()> {
    use shapecraft_ops_features::{Hole, HoleArgs, HoleKind};
    let mut b = Standalone::new();
    base(&mut b)?;
    let mut points = Sketch::new("points");
    points.args.plane = Some(xy());
    let p = points.args.add_point(Point2::from_array([1.0, 1.0]));
    points.args.add(Shape::Point(p));
    points.apply(&mut b)?;
    Hole {
        id: "hole".into(),
        args: HoleArgs {
            sketch: Some(Source::Profile("points".into()).into()),
            kind: HoleKind::Counterbore,
            diameter: Measure::new(0.4),
            head: Measure::new(0.8),
            head_depth: Measure::new(0.3),
            extent: Extent::ThroughAll,
            ..HoleArgs::default()
        },
    }
    .apply(&mut b)?;
    // One callout (ISO 129-1): the bore, through — no depth — over its
    // counterbore.
    assert_eq!(
        b.world.annotations["hole/callout"].text(),
        "Ø0,4 ⌴Ø0,8 ↧0,3"
    );
    Ok(())
}

#[test]
fn holes_end_up_to_a_face_or_a_datum_plane() -> anyhow::Result<()> {
    use shapecraft_ops_datum::{AddPlane, AddPlaneArgs, PlaneMode};
    use shapecraft_ops_features::{Hole, HoleArgs, HoleKind};
    // One point in the middle plane of the 4 × 4 × 2 box (z from -1 to
    // 1): the hole goes down from there. A plane `floor` is 1.5 under
    // the box's top, at z = -0.5.
    let drilled = |set: &dyn Fn(&mut HoleArgs)| -> anyhow::Result<(f64, Standalone)> {
        let mut b = Standalone::new();
        base(&mut b)?;
        AddPlane {
            id: "floor".into(),
            args: AddPlaneArgs {
                mode: PlaneMode::Offset,
                references: vec![Source::Topology("base/top".into()).into()],
                distance: -1.5,
                ..AddPlaneArgs::default()
            },
        }
        .apply(&mut b)?;
        let mut points = Sketch::new("points");
        points.args.plane = Some(xy());
        let p = points.args.add_point(Point2::new(0.0, 0.0));
        points.args.add(Shape::Point(p));
        points.apply(&mut b)?;
        let mut hole = Hole {
            id: "hole".into(),
            args: HoleArgs {
                sketch: Some(Source::Profile("points".into()).into()),
                diameter: Measure::new(0.5),
                kind: HoleKind::Counterbore,
                head: Measure::new(1.0),
                head_depth: Measure::new(0.5),
                depth: Measure::new(0.9),
                point: Measure::new(180.0),
                ..HoleArgs::default()
            },
        };
        set(&mut hole.args);
        hole.apply(&mut b)?;
        check(&b);
        Ok((32.0 - volume(&b.world.topo, body(&b, "base")), b))
    };
    let (typed, _) = drilled(&|_| {})?;
    // The counterbore's floor at the datum plane: the same body, and a
    // callout that says the depth in brackets.
    let (at_plane, b) = drilled(&|a| {
        a.head_extent = Extent::ToFace;
        a.head_up_to = Some(Source::Datum("floor".into()).into());
    })?;
    close(at_plane, typed, 1e-9);
    let callout = &b.world.annotations["hole/callout"];
    let shapecraft_core::world::Note::Hole { lines, .. } = &callout.note else {
        panic!("a hole's callout")
    };
    let said: Vec<String> = lines
        .iter()
        .map(|l| l.iter().map(|c| c.said()).collect())
        .collect();
    assert_eq!(said, ["Ø0,5 ↧0,9", "⌴Ø1 ↧(0,5)"], "{said:?}");
    // An offset past the plane, or short of it.
    let (deeper, _) = drilled(&|a| {
        a.head_extent = Extent::ToFace;
        a.head_up_to = Some(Source::Datum("floor".into()).into());
        a.head_offset = Measure::new(0.25);
    })?;
    let (shallow, _) = drilled(&|a| a.head_depth = Measure::new(0.75))?;
    close(deeper, shallow, 1e-9);
    // The bore up to the datum plane, and up to the box's bottom face
    // less half: as a depth of 0.5.
    let (blind, _) = drilled(&|a| {
        a.head_depth = Measure::new(0.25);
        a.depth = Measure::new(0.5);
    })?;
    let (to_plane, _) = drilled(&|a| {
        a.head_depth = Measure::new(0.25);
        a.extent = Extent::ToFace;
        a.up_to = Some(Source::Datum("floor".into()).into());
    })?;
    close(to_plane, blind, 1e-9);
    let (to_face, _) = drilled(&|a| {
        a.head_depth = Measure::new(0.25);
        a.extent = Extent::ToFace;
        a.up_to = Some(Source::Topology("base/bottom".into()).into());
        a.offset = Measure::new(-0.5);
    })?;
    close(to_face, blind, 1e-9);
    Ok(())
}
