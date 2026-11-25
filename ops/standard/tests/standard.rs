//! What the operations of this crate make.

use workpiece_ops_testing::*;

#[test]
fn standard_parts_have_the_sizes_the_standards_give() -> anyhow::Result<()> {
    use workpiece_ops_standard::{StandardKind, StandardPart, StandardPartArgs, lengths, standard};
    let hexagon = |flats: f64| 3.0_f64.sqrt() / 2.0 * flats * flats;
    // An M6 nut: a hexagon 10 across, 5.2 high, with a hole of 6.
    let nut = standard("n", StandardKind::HexNut, "M6", 0.0)?;
    close(
        workpiece_boolean::measure(&nut).volume,
        (hexagon(10.0) - PI * 9.0) * 5.2,
        1e-6,
    );
    // An M8 × 30 screw: a head 13 across and 5.3 high on a shank of 8.
    let screw = standard("s", StandardKind::HexScrew, "M8", 30.0)?;
    close(
        workpiece_boolean::measure(&screw).volume,
        hexagon(13.0) * 5.3 + PI * 16.0 * 30.0,
        1e-6,
    );
    // A 608 bearing: 8 × 22 × 7. Placed by the operation, it is a part
    // of its own, to be dragged and held by joints like any other.
    let mut b = Standalone::new();
    StandardPart {
        id: "bearing".into(),
        args: StandardPartArgs {
            kind: StandardKind::Bearing,
            size: "608".into(),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let placed = &b.world.instances["bearing"];
    assert_eq!(placed.world.topo.validate(), Ok(()));
    // Its two rings, each 0.36 of the section's height; of steel, called
    // by its designation, cut by a section like any ring.
    let ring = |name: &str| -> anyhow::Result<f64> {
        let id = placed.world.topo.resolve(name).expect("the ring");
        let ring = workpiece_boolean::read(&placed.world.topo, id)?;
        Ok(workpiece_boolean::measure(&ring).volume)
    };
    let wall: f64 = 7.0 * 0.36;
    close(
        ring("bearing")?,
        PI * (121.0 - (11.0 - wall).powi(2)) * 7.0,
        1e-6,
    );
    close(
        ring("bearing/inner")?,
        PI * ((4.0 + wall).powi(2) - 16.0) * 7.0,
        1e-6,
    );
    assert_eq!(placed.world.material.as_deref(), Some("Steel"));
    assert_eq!(
        placed.world.designation.as_deref(),
        Some("Deep groove ball bearing 608")
    );
    assert_eq!(
        placed.world.sectioned,
        workpiece_core::world::Sectioned::Cut
    );
    // A screw is drawn whole where a section runs along it.
    assert_eq!(
        StandardKind::SocketScrew.designation("M5", 16.0),
        "Socket head cap screw ISO 4762 M5×16"
    );
    assert!(
        !StandardKind::SocketScrew
            .sectioned()
            .cuts(workpiece_core::math::Point3::X)
    );
    // Screws come in the lengths of the standard, each size in its own
    // range of them; no other length is made.
    assert_eq!(
        lengths("M3").first().zip(lengths("M3").last()),
        Some((&6.0, &30.0))
    );
    assert!(lengths("M8").contains(&35.0) && !lengths("M8").contains(&12.0));
    let screw = |length: f64| StandardPart {
        id: "screw".into(),
        args: StandardPartArgs {
            size: "M8".into(),
            length,
            ..Default::default()
        },
    };
    assert!(screw(35.0).apply(&mut b.clone()).is_ok());
    assert!(screw(33.0).apply(&mut b).is_err());
    assert!(standard("x", StandardKind::Washer, "M7", 0.0).is_err());
    Ok(())
}

#[test]
fn the_catalogue_is_one_list_that_filters_narrow() -> anyhow::Result<()> {
    use workpiece_core::widgets::Shown;
    use workpiece_core::{Operation, Widget};
    use workpiece_ops_standard::{Family, Filter, StandardKind, StandardPart, catalogue};
    let all = catalogue();
    // Every screw in every length, and one of everything else a size.
    let label = "Socket head cap screw M5 × 16 · ISO 4762";
    assert!(all.iter().any(|e| e.label() == label));
    assert!(
        all.iter()
            .any(|e| e.label() == "Deep groove ball bearing 608 · ISO 15")
    );
    let nuts = Filter {
        family: Some(Family::Nuts),
        size: Some("M8".into()),
    };
    let shown: Vec<_> = all.iter().filter(|e| nuts.takes(e)).collect();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].kind, StandardKind::HexNut);
    // The dialog: the filters, then the list, searched; choosing a row
    // makes the part that row is.
    let op = StandardPart {
        id: "s".into(),
        args: Default::default(),
    };
    let world = std::sync::Arc::new(workpiece_core::World::new());
    let cache = workpiece_core::Cache::default();
    let context = workpiece_core::EditContext::standalone(&world, &cache);
    let screws = Filter {
        family: Some(Family::Screws),
        size: Some("M8".into()),
    };
    let edit = op.visualize_edit(&context, &screws);
    let Widget::VBox(fields) = &edit.dialog else {
        anyhow::bail!("expected a column of fields");
    };
    let Some(Widget::Choice(list)) = fields.children.get(1) else {
        anyhow::bail!("expected the catalogue");
    };
    assert_eq!(list.shown, Shown::Searched);
    assert!(list.options.iter().all(|o| o.contains(" M8 ")));
    // (An M6 screw is placed: the filter hides it, so none is chosen.)
    assert!(list.selected >= list.options.len());
    let row = list
        .options
        .iter()
        .position(|o| o.starts_with("Socket head cap screw M8 × 30"));
    let args = list
        .set(row.expect("the row"))
        .and_then(|e| e.args)
        .expect("arguments");
    assert_eq!(
        (args.kind, args.size.as_str(), args.length),
        (StandardKind::SocketScrew, "M8", 30.0)
    );
    let screw = StandardPart {
        id: "s".into(),
        args,
    };
    screw.apply(&mut Standalone::new())?;
    Ok(())
}

/// A builder holding a plate 3 thick with a hole 17 across through its
/// middle (an M16 washer, as good a plate as any), placed and fixed:
/// its top at z = 3.
fn plate() -> anyhow::Result<Standalone> {
    use workpiece_ops_standard::{StandardKind, standard};
    let mut world = workpiece_core::World::new();
    let ring = standard("plate", StandardKind::Washer, "M16", 0.0)?;
    workpiece_boolean::write(&mut world.topo, &ring, "plate")?;
    let mut b = Standalone::new();
    b.world.instances.insert(
        "plate".into(),
        workpiece_core::world::Instance {
            pose: workpiece_core::Pose::IDENTITY,
            world: std::sync::Arc::new(world),
            fixed: true,
        },
    );
    Ok(b)
}

/// Picks, in the dialog of `op`, its first field `label` with the
/// pointer at `at` on `source` of the plate: as a click there would.
fn pick(
    op: &workpiece_ops_standard::StandardPart,
    b: &Standalone,
    label: &str,
    source: &str,
    at: workpiece_core::math::Point3,
) -> anyhow::Result<workpiece_ops_standard::StandardPart> {
    use workpiece_core::Widget;
    use workpiece_core::widgets::Picked;
    let world = std::sync::Arc::new(b.world.clone());
    let context = workpiece_core::EditContext::standalone(&world, &b.cache);
    let edit = op.visualize_edit(&context, &Default::default());
    let mut found = None;
    edit.dialog.visit(&mut |w| {
        if let Widget::ConnectorInput(input) = w
            && input.label == label
            && found.is_none()
        {
            found = Some(input.set(Picked {
                instance: vec!["plate".into()],
                source: Source::Topology(source.into()),
                at,
                near: 0.5,
                within: b.world.instances["plate"].world.clone(),
            }));
        }
    });
    let args = found
        .and_then(|e| e.args)
        .ok_or_else(|| anyhow::anyhow!("no field {label}"))?;
    Ok(workpiece_ops_standard::StandardPart {
        id: op.id.clone(),
        args,
    })
}

#[test]
fn every_kind_has_a_seat_and_a_screw_sits_on_a_hole_by_one_pick() -> anyhow::Result<()> {
    use workpiece_core::math::{Axis, Point3};
    use workpiece_core::world::{Datum, JointKind};
    use workpiece_ops_instances::assembly;
    use workpiece_ops_standard::{SEAT, StandardKind, StandardPart, lengths};
    // Each kind is placed with its seat as a frame of its own, on its
    // axis and on its solid; its first mate starts there.
    for kind in StandardKind::ALL {
        let mut op = StandardPart::new("p");
        op.args.kind = kind;
        op.args.size = kind.sizes()[3].to_owned();
        op.args.length = lengths(&op.args.size).first().copied().unwrap_or(0.0);
        let mut b = Standalone::new();
        op.apply(&mut b)?;
        let part = &b.world.instances["p"].world;
        let Some(Datum::Frame(seat)) = part.datums.get(SEAT) else {
            anyhow::bail!("{kind:?} has no seat");
        };
        let o = seat.pose().translation();
        assert!(o.x.abs() + o.y.abs() < 1e-9, "{kind:?}: {o}");
        let (lo, hi) = part.extent().expect("a solid");
        assert!(o.z >= lo.z - 1e-9 && o.z <= hi.z + 1e-9, "{kind:?}");
        assert_eq!(op.args.hold.mates.len(), 1);
        assert_eq!(op.args.hold.mates[0].kind, JointKind::Revolute);
        assert!(op.args.hold.mates[0].onto.is_none());
    }
    // An M6 screw: one pick on the plate's top, by the rim of the
    // hole, and it stands in the hole, its head on the top.
    let mut b = plate()?;
    let screw = StandardPart::new("screw");
    let screw = pick(&screw, &b, "Onto", "plate/end", Point3::new(-6.1, 6.1, 3.0))?;
    screw.apply(&mut b)?;
    assembly::settle(&mut b.world, &mut b.cache);
    let pose = b.world.instances["screw"].pose;
    assert!(
        pose.translation()
            .abs_diff_eq(Point3::new(0.0, 0.0, 3.0), 1e-6),
        "{pose:?}"
    );
    assert!(pose.axis(Axis::Z).abs_diff_eq(Point3::Z, 1e-6));
    // Pointed at the rim itself, the same: a rim is out of its face.
    let world = &b.world.instances["plate"].world;
    let topo = &world.topo;
    let rim = topo
        .edges()
        .iter()
        .filter_map(|(id, e)| {
            let name = topo.name_of(id)?;
            let p = topo.vertex(e.start)?.point;
            (p.z > 2.9 && p.truncate().length() < 9.0).then(|| name.to_owned())
        })
        .find(|name| workpiece_ops_instances::connector::rim_face(world, name).is_some())
        .expect("the hole's rim on the top");
    let mut again = plate()?;
    let by_rim = pick(
        &StandardPart::new("screw"),
        &again,
        "Onto",
        &rim,
        Point3::new(-6.0, 6.0, 3.0),
    )?;
    by_rim.apply(&mut again)?;
    assembly::settle(&mut again.world, &mut again.cache);
    let by_rim = again.world.instances["screw"].pose;
    assert!(by_rim.translation().abs_diff_eq(pose.translation(), 1e-6));
    assert!(by_rim.axis(Axis::Z).abs_diff_eq(Point3::Z, 1e-6));

    // Turned over, by the button beside its own reference: the head
    // under the plate's top, the shank up out of it.
    let mut flipped = screw.clone();
    flipped.args.hold.mates[0].flip_with = true;
    let mut again = plate()?;
    again.cache = b.cache.clone();
    flipped.apply(&mut again)?;
    assembly::settle(&mut again.world, &mut again.cache);
    let pose = again.world.instances["screw"].pose;
    assert!(
        pose.translation()
            .abs_diff_eq(Point3::new(0.0, 0.0, 3.0), 1e-6)
    );
    assert!(pose.axis(Axis::Z).abs_diff_eq(-Point3::Z, 1e-6), "{pose:?}");

    // One mate leaves it to turn about its axis; a second, fastening
    // its origin 3 above the plate's, holds that too: dragged round, it
    // stays.
    let turned = |b: &Standalone| {
        let drag = assembly::Drag {
            instance: "screw".into(),
            local: Point3::new(5.0, 0.0, 0.0),
            target: Point3::new(0.0, 5.0, 3.0),
        };
        let solved = assembly::solve(&b.world, Some(&drag));
        solved.poses["screw"].axis(Axis::X).y
    };
    assert!(turned(&b) > 0.5);
    let mut two = screw.clone();
    let origin = |instance: &[&str]| workpiece_ops_instances::Anchor {
        instance: instance.iter().map(|s| (*s).to_owned()).collect(),
        source: Source::Datum(ORIGIN.into()),
        snap: workpiece_ops_instances::connector::Snap::Datum,
    };
    two.args.hold.mates.push(workpiece_ops_instances::Mating {
        offset: [0.0, 0.0, 3.0],
        ..workpiece_ops_instances::Mating::new(JointKind::Fastened, origin(&["plate"]), origin(&[]))
    });
    let mut b = plate()?;
    two.apply(&mut b)?;
    let names: Vec<_> = b.world.joints.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(names, ["screw", "screw/2"]);
    assembly::settle(&mut b.world, &mut b.cache);
    assert!(turned(&b).abs() < 1e-6, "{}", turned(&b));
    Ok(())
}
