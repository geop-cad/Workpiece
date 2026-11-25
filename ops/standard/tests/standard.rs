//! What the operations of this crate make.

use shapecraft_ops_testing::*;

#[test]
fn standard_parts_have_the_sizes_the_standards_give() -> anyhow::Result<()> {
    use shapecraft_ops_standard::{
        StandardKind, StandardPart, StandardPartArgs, lengths, standard,
    };
    let hexagon = |flats: f64| 3.0_f64.sqrt() / 2.0 * flats * flats;
    // An M6 nut: a hexagon 10 across, 5.2 high, with a hole of 6.
    let nut = standard("n", StandardKind::HexNut, "M6", 0.0)?;
    close(
        shapecraft_boolean::measure(&nut).volume,
        (hexagon(10.0) - PI * 9.0) * 5.2,
        1e-6,
    );
    // An M8 × 30 screw: a head 13 across and 5.3 high on a shank of 8.
    let screw = standard("s", StandardKind::HexScrew, "M8", 30.0)?;
    close(
        shapecraft_boolean::measure(&screw).volume,
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
        let ring = shapecraft_boolean::read(&placed.world.topo, id)?;
        Ok(shapecraft_boolean::measure(&ring).volume)
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
        shapecraft_core::world::Sectioned::Cut
    );
    // A screw is drawn whole where a section runs along it.
    assert_eq!(
        StandardKind::SocketScrew.designation("M5", 16.0),
        "Socket head cap screw ISO 4762 M5×16"
    );
    assert!(
        !StandardKind::SocketScrew
            .sectioned()
            .cuts(shapecraft_core::math::Point3::X)
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
    use shapecraft_core::widgets::Shown;
    use shapecraft_core::{Operation, Widget};
    use shapecraft_ops_standard::{Family, Filter, StandardKind, StandardPart, catalogue};
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
    let world = std::sync::Arc::new(shapecraft_core::World::new());
    let cache = shapecraft_core::Cache::default();
    let context = shapecraft_core::EditContext::standalone(&world, &cache);
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
