//! What a view of a sheet shows: all of the model, or the bodies it
//! names; of a section, all behind the cut, or only what the cut meets.

use workpiece_core::world::Cutting;
use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};
use workpiece_ops_drawing::Sheet;
use workpiece_ops_drawing::lay::{Kept, Laid, Subject, lay_out};
use workpiece_ops_drawing::sheet::{PlacedNote, SheetView, Side, Style, ViewKind};
use workpiece_ops_testing::*;

#[test]
fn a_view_shows_the_bodies_it_names_and_a_section_only_its_cut() -> anyhow::Result<()> {
    // Two blocks: 4 × 4 × 4 at the origin, and 2 × 2 × 2 at a frame 10 on.
    let mut b = Standalone::new();
    let block = |b: &mut Standalone, id: &str, frame: &str, size: f64| {
        AddBox {
            id: id.into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(frame)),
                size: [size; 3].map(Measure::new),
            },
        }
        .apply(b)
    };
    block(&mut b, "large", ORIGIN, 4.0)?;
    let there = workpiece_core::world::DatumFrame::new(
        workpiece_core::math::Point3::new(10.0, 0.0, 0.0),
        workpiece_core::math::Point3::X,
        workpiece_core::math::Point3::Y,
    )?;
    b.world
        .datums
        .insert("there".into(), workpiece_core::world::Datum::Frame(there));
    block(&mut b, "small", "there", 2.0)?;
    let cut = Cutting::plane(
        workpiece_core::math::Point3::ZERO,
        workpiece_core::math::Point3::Y,
    );
    b.world.sections.insert("mid".into(), cut);
    let view = |id: &str, kind, bodies: &[&str], cut_only| SheetView {
        id: id.into(),
        kind,
        at: [100.0, 100.0],
        scale: None,
        style: Style::HiddenRemoved,
        bodies: bodies.iter().map(|b| (*b).to_owned()).collect(),
        cut_only,
        half: None,
        section: None,
        caption_at: None,
    };
    let top = || ViewKind::Base { side: Side::Top };
    let section = || ViewKind::Section {
        of: None,
        section: "mid".into(),
        letter: "A".into(),
        turn: 0.0,
    };
    let properties = Default::default();
    let lines = |v: SheetView| {
        let sheet = Sheet {
            views: vec![v],
            ..Sheet::default()
        };
        let subject = Subject {
            world: &b.world,
            unit: workpiece_core::Unit::default(),
            material: None,
            name: "blocks",
            properties: &properties,
            parts: &Default::default(),
            densities: &Default::default(),
        };
        let laid = lay_out(&sheet, &subject, &Kept::default(), None);
        assert!(laid.problems.is_empty(), "{:?}", laid.problems);
        laid.views[0].lines.len()
    };
    // From above both blocks are squares: four lines each.
    assert_eq!(lines(view("all", top(), &[], false)), 8);
    assert_eq!(lines(view("one", top(), &["small"], false)), 4);
    // The section through both shows two cut squares; drawn as the cut
    // only it has no more lines than their outlines, and of one body
    // only that body's.
    let whole = lines(view("s", section(), &[], false));
    let only = lines(view("s", section(), &[], true));
    assert_eq!(only, 8);
    assert!(whole >= only);
    assert_eq!(lines(view("s", section(), &["large"], true)), 4);
    Ok(())
}

/// Two blocks, as above: 4 × 4 × 4 at the origin, 2 × 2 × 2 ten on
/// along x; cut by the plane y = 1.
fn blocks() -> anyhow::Result<Standalone> {
    let mut b = Standalone::new();
    let block = |b: &mut Standalone, id: &str, frame: &str, size: f64| {
        AddBox {
            id: id.into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum(frame)),
                size: [size; 3].map(Measure::new),
            },
        }
        .apply(b)
    };
    block(&mut b, "large", ORIGIN, 4.0)?;
    let there = workpiece_core::world::DatumFrame::new(
        workpiece_core::math::Point3::new(10.0, 0.0, 0.0),
        workpiece_core::math::Point3::X,
        workpiece_core::math::Point3::Y,
    )?;
    b.world
        .datums
        .insert("there".into(), workpiece_core::world::Datum::Frame(there));
    block(&mut b, "small", "there", 2.0)?;
    let cut = Cutting::plane(
        workpiece_core::math::Point3::new(0.0, 1.0, 0.0),
        workpiece_core::math::Point3::Y,
    );
    b.world.sections.insert("mid".into(), cut);
    Ok(b)
}

fn laid(b: &Standalone, views: Vec<SheetView>, notes: Vec<PlacedNote>) -> Laid {
    let properties = Default::default();
    let sheet = Sheet {
        views,
        notes,
        ..Sheet::default()
    };
    let subject = Subject {
        world: &b.world,
        unit: workpiece_core::Unit::default(),
        material: None,
        name: "blocks",
        properties: &properties,
        parts: &Default::default(),
        densities: &Default::default(),
    };
    lay_out(&sheet, &subject, &Kept::default(), None)
}

fn sheet_view(id: &str, kind: ViewKind, at: [f64; 2], bodies: &[&str]) -> SheetView {
    SheetView {
        id: id.into(),
        kind,
        at,
        scale: None,
        style: Style::HiddenRemoved,
        bodies: bodies.iter().map(|b| (*b).to_owned()).collect(),
        cut_only: false,
        half: None,
        section: None,
        caption_at: None,
    }
}

#[test]
fn a_view_of_some_bodies_stands_on_them_and_a_section_turns() -> anyhow::Result<()> {
    let b = blocks()?;
    // From above, only the small block: its middle where the view is put.
    let top = ViewKind::Base { side: Side::Top };
    let one = laid(
        &b,
        vec![sheet_view("one", top, [100.0, 100.0], &["small"])],
        vec![],
    );
    let ends = one.views[0].lines.iter().flat_map(|(a, c)| [*a, *c]);
    let (lo, hi) = ends.fold(
        (
            Point2::splat(f64::INFINITY),
            Point2::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    );
    assert!(
        ((lo + hi) / 2.0 - Point2::new(100.0, 100.0)).length() < 1e-6,
        "{lo:?} {hi:?}"
    );
    // The section, as it is and a quarter turned: what was wide stands
    // high.
    let section = |turn| ViewKind::Section {
        of: None,
        section: "mid".into(),
        letter: "A".into(),
        turn,
    };
    let half = |turn| {
        laid(
            &b,
            vec![sheet_view("s", section(turn), [100.0, 100.0], &[])],
            vec![],
        )
        .views[0]
            .half
    };
    let (flat, turned) = (half(0.0), half(90.0));
    assert!(flat.x > flat.y * 2.0, "{flat:?}");
    assert!(
        (flat.x - turned.y).abs() < 1e-6 && (flat.y - turned.x).abs() < 1e-6,
        "{flat:?} {turned:?}"
    );
    Ok(())
}

#[test]
fn dimensions_stand_where_a_view_sees_them_and_nowhere_else() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_core::world::{Annotation, Measured, Note, Tolerance};
    let mut b = blocks()?;
    let say = |b: &mut Standalone, name: &str, measured: Measured, value: f64| {
        let note = Annotation {
            sketch: None,
            note: Note::Dimension {
                measured,
                value,
                tolerance: Tolerance::General,
                elsewhere: Vec::new(),
            },
            at: Point3::ZERO,
        };
        b.world.annotations.insert(name.into(), note);
    };
    // A radius about z, seen from the front: from the side, as a
    // length out from its axis. A height in the isometric view. A
    // length up past what a detail of the front view shows.
    let r = Measured::Radius {
        centre: Point3::new(2.0, 2.0, 0.0),
        axis: Point3::Z,
        radius: 2.0,
    };
    say(&mut b, "r", r, 2.0);
    let high = Measured::Length {
        from: Point3::ZERO,
        to: Point3::new(0.0, 0.0, 4.0),
        along: None,
    };
    say(&mut b, "high", high, 4.0);
    let far = Measured::Length {
        from: Point3::new(0.0, 0.0, 10.0),
        to: Point3::new(0.0, 0.0, 14.0),
        along: None,
    };
    say(&mut b, "far", far, 2.0);
    let detail = ViewKind::Detail {
        of: "front".into(),
        centre: [0.0, 0.0],
        radius: 3.0,
        letter: "B".into(),
        letter_at: None,
    };
    let views = vec![
        sheet_view(
            "front",
            ViewKind::Base { side: Side::Front },
            [100.0, 100.0],
            &[],
        ),
        sheet_view(
            "iso",
            ViewKind::Base {
                side: Side::Isometric,
            },
            [250.0, 100.0],
            &[],
        ),
        sheet_view("detail", detail, [300.0, 200.0], &[]),
    ];
    let note = |note: &str, view: &str| PlacedNote {
        note: note.into(),
        view: view.into(),
        at: [0.0, 20.0],
        decimals: None,
        starts: [None; 2],
        count: None,
        to: None,
    };
    let notes = vec![
        note("r", "front"),
        note("high", "iso"),
        note("far", "detail"),
    ];
    let laid = laid(&b, views, notes);
    assert_eq!(laid.notes.len(), 2, "{:?}", laid.problems);
    assert_eq!(laid.problems.len(), 1, "{:?}", laid.problems);
    assert!(laid.problems[0].contains("far"));
    assert_eq!(laid.missing, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_section_shows_its_line_stands_upright_and_hatches_each_body_its_own_way() -> anyhow::Result<()>
{
    use workpiece_ops_drawing::lay::Pen;
    // The blocks cut by the plane y = 1, from above: the plane's line
    // shows in the top view, though the section says it in no plane of
    // its own.
    let b = blocks()?;
    let top = sheet_view(
        "top",
        ViewKind::Base { side: Side::Top },
        [100.0, 200.0],
        &[],
    );
    let section = sheet_view(
        "s",
        ViewKind::Section {
            of: Some("top".into()),
            section: "mid".into(),
            letter: "A".into(),
            turn: 0.0,
        },
        [300.0, 200.0],
        &[],
    );
    let laid = laid(&b, vec![top, section], vec![]);
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    assert_eq!(laid.cuts.len(), 1);
    assert_eq!(laid.cuts[0].of, "top");
    // Upright, as the blocks stand: 12 wide, 4 high — not on its side,
    // as a view turned down out of the top view would lie.
    let s = laid
        .views
        .iter()
        .find(|v| v.id == "s")
        .expect("the section");
    assert!(s.half.x > s.half.y * 2.0, "{:?}", s.half);
    // The two blocks it cuts are hatched the one way and the other.
    let leaning = |way: f64| {
        laid.strokes.iter().any(|k| {
            let d = (k.b - k.a).normalize_or_zero();
            k.pen == Pen::Narrow && (d.x * d.y * 2.0 - way).abs() < 1e-6
        })
    };
    assert!(leaning(1.0) && leaning(-1.0));
    Ok(())
}

#[test]
fn walls_seen_edge_on_are_their_edges_drawn_once() -> anyhow::Result<()> {
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    // A post standing on the origin, seen from above: its rim, a circle,
    // and nothing of its wall, which is seen edge on all round — no
    // outline of it zigzagging between its sides, no second circle of
    // its foot over the rim.
    let mut b = Standalone::new();
    AddCylinder {
        id: "post".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(2.0),
            height: Measure::new(3.0),
        },
    }
    .apply(&mut b)?;
    // From the front: its two outlines, the full height, beside its
    // rim and its foot.
    let front = sheet_view(
        "front",
        ViewKind::Base { side: Side::Front },
        [100.0, 100.0],
        &[],
    );
    let seen = laid(&b, vec![front], vec![]);
    let view = &seen.views[0];
    let upright: Vec<_> = view
        .lines
        .iter()
        .filter(|(a, c)| (a.x - c.x).abs() < 1e-6 && (a.y - c.y).abs() > 1e-6)
        .collect();
    let height: f64 = upright.iter().map(|(a, c)| a.distance(*c)).sum();
    assert!(
        (height - 2.0 * 3.0 * view.scale).abs() < 1e-6,
        "{upright:?}"
    );
    let xs: std::collections::BTreeSet<i64> = upright
        .iter()
        .map(|(a, _)| (a.x * 1e3).round() as i64)
        .collect();
    assert_eq!(xs.len(), 2, "{upright:?}");
    let top = sheet_view(
        "top",
        ViewKind::Base { side: Side::Top },
        [100.0, 100.0],
        &[],
    );
    let laid = laid(&b, vec![top], vec![]);
    let view = &laid.views[0];
    let length: f64 = view.lines.iter().map(|(a, c)| a.distance(*c)).sum();
    let rim = std::f64::consts::TAU * 2.0 * view.scale;
    assert!((length - rim).abs() < rim * 0.01, "{length} for {rim}");
    Ok(())
}

#[test]
fn a_seam_on_the_outline_of_a_disc_leaves_both_its_sides_drawn() -> anyhow::Result<()> {
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    // A disc 270 across and 30 thick, its seam turned a quarter at a
    // time: twice on an outline seen from the front, twice square to
    // it. However its seam lies, both its sides are drawn, each once,
    // the full height.
    for quarter in 0..4 {
        let turn = f64::from(quarter) * std::f64::consts::FRAC_PI_2;
        let mut b = Standalone::new();
        let turned = workpiece_core::world::DatumFrame::new(
            workpiece_core::math::Point3::ZERO,
            workpiece_core::math::Point3::new(turn.cos(), turn.sin(), 0.0),
            workpiece_core::math::Point3::new(-turn.sin(), turn.cos(), 0.0),
        )?;
        b.world
            .datums
            .insert("turned".into(), workpiece_core::world::Datum::Frame(turned));
        AddCylinder {
            id: "disc".into(),
            args: AddCylinderArgs {
                frame: Some(Reference::datum("turned")),
                radius: Measure::new(135.0),
                height: Measure::new(30.0),
            },
        }
        .apply(&mut b)?;
        let front = sheet_view(
            "front",
            ViewKind::Base { side: Side::Front },
            [200.0, 150.0],
            &[],
        );
        let seen = laid(&b, vec![front], vec![]);
        let view = &seen.views[0];
        let upright: Vec<_> = view
            .lines
            .iter()
            .filter(|(a, c)| (a.x - c.x).abs() < 1e-6 && (a.y - c.y).abs() > 1e-6)
            .collect();
        let xs: std::collections::BTreeSet<i64> = upright
            .iter()
            .map(|(a, _)| (a.x * 1e3).round() as i64)
            .collect();
        assert_eq!(xs.len(), 2, "turned {quarter} quarters: {upright:?}");
        let height: f64 = upright.iter().map(|(a, c)| a.distance(*c)).sum();
        assert!(
            (height - 2.0 * 30.0 * view.scale).abs() < 1e-6,
            "turned {quarter} quarters: {upright:?}"
        );
    }
    Ok(())
}

#[test]
fn the_round_of_a_post_s_rim_is_outlined_from_the_side() -> anyhow::Result<()> {
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    // A post 2 in radius and 3 high, the rim of its top rounded 1/2:
    // from the front its outline goes up past where the round starts,
    // to the top, round over the corners — not a box that stops where
    // the wall does.
    let mut b = Standalone::new();
    AddCylinder {
        id: "post".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(2.0),
            height: Measure::new(3.0),
        },
    }
    .apply(&mut b)?;
    let rim = vec![Source::Topology("post/top-rim".into())];
    workpiece_ops_blend::Fillet {
        id: "round".into(),
        args: workpiece_ops_blend::FilletArgs {
            edges: Reference::each(rim),
            size: Measure::new(0.5),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let front = sheet_view(
        "front",
        ViewKind::Base { side: Side::Front },
        [100.0, 100.0],
        &[],
    );
    let seen = laid(&b, vec![front], vec![]);
    let view = &seen.views[0];
    let s = view.scale;
    // Of what is drawn, how much lies in the corner of the outline,
    // between where the wall ends and the top, beside the top face.
    let (lo, hi) = view.lines.iter().fold(
        (
            Point2::splat(f64::INFINITY),
            Point2::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), (a, c)| (lo.min(a.min(*c)), hi.max(a.max(*c))),
    );
    let centre = (lo.x + hi.x) / 2.0;
    let corner: f64 = view
        .lines
        .iter()
        .filter(|(a, c)| {
            let m = (*a + *c) / 2.0;
            // (Neither level nor upright: not the edges beside it.)
            let slanted = (a.x - c.x).abs() > 1e-9 && (a.y - c.y).abs() > 1e-9;
            slanted
                && (m.x - centre).abs() > 1.5 * s + 1e-6
                && ((m.y - lo.y).abs() < 0.5 * s || (m.y - hi.y).abs() < 0.5 * s)
        })
        .map(|(a, c)| a.distance(*c))
        .sum();
    // A quarter circle of 1/2 on each side, each drawn once.
    let quarters = 2.0 * PI / 2.0 * 0.5 * s;
    assert!(
        (hi.x - lo.x - 4.0 * s).abs() < 1e-6 && (hi.y - lo.y - 3.0 * s).abs() < 1e-6,
        "{lo} {hi}"
    );
    assert!(
        (corner - quarters).abs() < 0.03 * quarters,
        "{corner} of {quarters}: {:?}",
        view.lines
    );
    Ok(())
}

/// The lines in view of a view of `b` from `side`, and its scale.
fn seen_from(b: &Standalone, side: Side) -> (Vec<(Point2, Point2)>, f64) {
    let view = sheet_view("v", ViewKind::Base { side }, [150.0, 150.0], &[]);
    let laid = laid(b, vec![view], vec![]);
    (laid.views[0].lines.clone(), laid.views[0].scale)
}

#[test]
fn a_ball_seen_from_anywhere_is_its_outline_once() -> anyhow::Result<()> {
    use workpiece_ops_basic_shapes::{AddSphere, AddSphereArgs};
    let mut b = Standalone::new();
    AddSphere {
        id: "ball".into(),
        args: AddSphereArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(3.0),
        },
    }
    .apply(&mut b)?;
    for side in [
        Side::Front,
        Side::Top,
        Side::Isometric,
        Side::IsometricBelowBackLeft,
    ] {
        let (lines, s) = seen_from(&b, side);
        let (lo, hi) = lines.iter().fold(
            (
                Point2::splat(f64::INFINITY),
                Point2::splat(f64::NEG_INFINITY),
            ),
            |(lo, hi), (a, c)| (lo.min(a.min(*c)), hi.max(a.max(*c))),
        );
        let centre = (lo + hi) / 2.0;
        // A circle of 3 all round, nothing else, drawn once.
        for (a, c) in &lines {
            for p in [a, c] {
                let r = p.distance(centre) / s;
                assert!((r - 3.0).abs() < 0.02, "{side:?}: {r}");
            }
        }
        let length: f64 = lines.iter().map(|(a, c)| a.distance(*c)).sum::<f64>() / s;
        let round = std::f64::consts::TAU * 3.0;
        assert!(
            (length - round).abs() < 0.01 * round,
            "{side:?}: {length} of {round}: {lines:?}"
        );
    }
    Ok(())
}

#[test]
fn a_post_seen_isometric_has_both_its_outlines_the_full_height() -> anyhow::Result<()> {
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    let mut b = Standalone::new();
    AddCylinder {
        id: "post".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(2.0),
            height: Measure::new(5.0),
        },
    }
    .apply(&mut b)?;
    let (lines, s) = seen_from(&b, Side::Isometric);
    // Its axis is upright on the sheet, foreshortened as an isometric
    // view has it: √(2/3).
    let upright: Vec<_> = lines
        .iter()
        .filter(|(a, c)| (a.x - c.x).abs() < 1e-6 * s && (a.y - c.y).abs() > 1e-6)
        .collect();
    let xs: std::collections::BTreeSet<i64> = upright
        .iter()
        .map(|(a, _)| (a.x / s * 1e3).round() as i64)
        .collect();
    assert_eq!(xs.len(), 2, "{upright:?}");
    let height: f64 = upright.iter().map(|(a, c)| a.distance(*c)).sum::<f64>() / s;
    let wanted = 2.0 * 5.0 * (2.0_f64 / 3.0).sqrt();
    assert!((height - wanted).abs() < 1e-6, "{height} of {wanted}");
    Ok(())
}

#[test]
fn the_quarter_rounds_of_a_rounded_block_s_top_are_outlined() -> anyhow::Result<()> {
    // A block 4 × 4 × 2, its upright edges rounded 1 and then its top
    // edges 1/2: at each top corner a quarter of a torus. From the
    // front the outline turns round over the top corners, a quarter
    // circle of 1/2 each, which only those quarter tori make.
    let mut b = Standalone::new();
    base(&mut b)?;
    let fillet = |b: &mut Standalone, id: &str, edges: Vec<Source>, r: f64| {
        workpiece_ops_blend::Fillet {
            id: id.into(),
            args: workpiece_ops_blend::FilletArgs {
                edges: Reference::each(edges),
                size: Measure::new(r),
                ..Default::default()
            },
        }
        .apply(b)
    };
    let upright = edges_where(&b, "base", |a, c| {
        (a.x - c.x).abs() < 1e-9 && (a.y - c.y).abs() < 1e-9
    });
    assert_eq!(upright.len(), 4);
    fillet(&mut b, "corners", upright, 1.0)?;
    // (All the edges of its top face.)
    fillet(
        &mut b,
        "top",
        vec![Source::Topology("base/top".into())],
        0.5,
    )?;
    let (lines, s) = seen_from(&b, Side::Front);
    let (lo, hi) = lines.iter().fold(
        (
            Point2::splat(f64::INFINITY),
            Point2::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), (a, c)| (lo.min(a.min(*c)), hi.max(a.max(*c))),
    );
    assert!((hi.x - lo.x - 4.0 * s).abs() < 1e-6 && (hi.y - lo.y - 2.0 * s).abs() < 1e-6);
    let centre = (lo.x + hi.x) / 2.0;
    let corner: f64 = lines
        .iter()
        .filter(|(a, c)| {
            let m = (*a + *c) / 2.0;
            // (Neither level nor upright: not the edges beside it.)
            let slanted = (a.x - c.x).abs() > 1e-9 && (a.y - c.y).abs() > 1e-9;
            slanted
                && (m.x - centre).abs() > 1.5 * s + 1e-6
                && ((m.y - lo.y).abs() < 0.5 * s || (m.y - hi.y).abs() < 0.5 * s)
        })
        .map(|(a, c)| a.distance(*c))
        .sum();
    // A quarter circle of 1/2 at each end, each drawn once.
    let quarters = 2.0 * PI / 2.0 * 0.5 * s;
    assert!(
        (corner - quarters).abs() < 0.03 * quarters,
        "{corner} of {quarters}: {lines:?}"
    );
    Ok(())
}

#[test]
fn a_section_of_only_its_cut_is_framed_by_the_cut() -> anyhow::Result<()> {
    // The two blocks, and a third 2 × 2 × 2 high up behind the cut;
    // cut square to x through the large block only. Shown whole, the
    // section is as high as the blocks behind the cut reach; of only
    // its cut, it is the large block's 4 × 4 square: its frame, its
    // name over it and its lines all about that.
    let mut b = blocks()?;
    let high = workpiece_core::world::DatumFrame::new(
        workpiece_core::math::Point3::new(-10.0, 0.0, 10.0),
        workpiece_core::math::Point3::X,
        workpiece_core::math::Point3::Y,
    )?;
    b.world
        .datums
        .insert("high".into(), workpiece_core::world::Datum::Frame(high));
    AddBox {
        id: "high".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum("high")),
            size: [2.0; 3].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    let large = workpiece_boolean::read(&b.world.topo, body(&b, "large"))?;
    let (lo, hi) = large.extent();
    let middle = (lo + hi) / 2.0;
    b.world.sections.insert(
        "across".into(),
        Cutting::plane(middle, workpiece_core::math::Point3::X),
    );
    let section = |cut_only| SheetView {
        cut_only,
        half: None,
        section: None,
        ..sheet_view(
            "s",
            ViewKind::Section {
                of: None,
                section: "across".into(),
                letter: "A".into(),
                turn: 0.0,
            },
            [150.0, 150.0],
            &[],
        )
    };
    let whole = laid(&b, vec![section(false)], vec![]);
    let only = laid(&b, vec![section(true)], vec![]);
    let (w, o) = (&whole.views[0], &only.views[0]);
    assert!(w.half.y > o.half.y + 1.0, "{:?} {:?}", w.half, o.half);
    assert!(
        (o.half - Point2::splat(2.0 * o.scale)).length() < 1e-6,
        "{:?}",
        o.half
    );
    // Its lines are the cut's, and the frame is around them.
    let ends = o.lines.iter().flat_map(|(a, c)| [*a, *c]);
    let (lo, hi) = ends.fold(
        (
            Point2::splat(f64::INFINITY),
            Point2::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    );
    assert!(
        ((lo + hi) / 2.0 - o.centre).length() < 1e-6,
        "{lo} {hi} {:?}",
        o.centre
    );
    Ok(())
}

#[test]
fn a_pictorial_view_from_below_looks_up_with_the_model_s_up_upright() {
    for side in [
        Side::IsometricBelowFrontRight,
        Side::IsometricBelowFrontLeft,
        Side::IsometricBelowBackRight,
        Side::IsometricBelowBackLeft,
    ] {
        let [right, up, towards] = side.basis();
        // Looking up at it from under it, as isometric: alike all three.
        assert!((towards.z + 1.0 / 3f64.sqrt()).abs() < 1e-9, "{side:?}");
        assert!(towards.x.abs() > 0.5 && towards.y.abs() > 0.5);
        // The model's up, up on the sheet.
        assert!(up.z > 0.0 && right.dot(up).abs() < 1e-12);
    }
}

#[test]
fn a_length_between_the_middles_of_drawn_arcs_ends_on_what_the_view_draws() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_core::world::{Annotation, Measured, Note, Tolerance};
    // A block 4 × 4 × 2, its upright edges rounded 1: from the top four
    // quarter circles. A length from the middle of one to the middle of
    // the next along x ends on what the view draws (each end at an arc's
    // middle, which is then marked), not on nothing.
    let mut b = Standalone::new();
    base(&mut b)?;
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    let (lo, hi) = solid.vertices.iter().fold(
        (
            Point3::splat(f64::INFINITY),
            Point3::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), v| (lo.min(v.point), hi.max(v.point)),
    );
    let upright = edges_where(&b, "base", |a, c| {
        (a.x - c.x).abs() < 1e-9 && (a.y - c.y).abs() < 1e-9
    });
    workpiece_ops_blend::Fillet {
        id: "corners".into(),
        args: workpiece_ops_blend::FilletArgs {
            edges: Reference::each(upright),
            size: Measure::new(1.0),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let z = hi.z;
    b.world.annotations.insert(
        "between".into(),
        Annotation {
            note: Note::Dimension {
                measured: Measured::Length {
                    from: Point3::new(lo.x + 1.0, lo.y + 1.0, z),
                    to: Point3::new(hi.x - 1.0, lo.y + 1.0, z),
                    along: Some(Point3::X),
                },
                value: hi.x - lo.x - 2.0,
                tolerance: Tolerance::General,
                elsewhere: Vec::new(),
            },
            at: Point3::new(0.0, lo.y - 3.0, z),
            sketch: None,
        },
    );
    let top = sheet_view(
        "top",
        ViewKind::Base { side: Side::Top },
        [150.0, 150.0],
        &[],
    );
    let note = PlacedNote {
        note: "between".into(),
        view: "top".into(),
        at: [0.0, -30.0],
        decimals: None,
        starts: [None; 2],
        count: None,
        to: None,
    };
    let laid = laid(&b, vec![top], vec![note]);
    assert!(
        !laid.problems.iter().any(|p| p.contains("ends on nothing")),
        "{:?}",
        laid.problems
    );
    Ok(())
}

#[test]
fn a_radius_of_a_round_off_the_text_s_side_is_one_straight_leader_through_its_centre()
-> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_core::world::{Annotation, Measured, Note, Tolerance};
    use workpiece_ops_drawing::lay::Pen;
    // A block 4 × 4 × 2, its upright edges rounded 1; the radius of one
    // corner, its text put on the far side of the round's middle (the
    // arc does not reach round to it): the leader runs from the arrow on
    // the arc straight through the middle on to a level shoulder under
    // the text — no second bend (ISO 129-1).
    let mut b = Standalone::new();
    base(&mut b)?;
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    let (lo, hi) = solid.vertices.iter().fold(
        (
            Point3::splat(f64::INFINITY),
            Point3::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), v| (lo.min(v.point), hi.max(v.point)),
    );
    let upright = edges_where(&b, "base", |a, c| {
        (a.x - c.x).abs() < 1e-9 && (a.y - c.y).abs() < 1e-9
    });
    workpiece_ops_blend::Fillet {
        id: "corners".into(),
        args: workpiece_ops_blend::FilletArgs {
            edges: Reference::each(upright),
            size: Measure::new(1.0),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let centre = Point3::new(lo.x + 1.0, lo.y + 1.0, hi.z);
    b.world.annotations.insert(
        "round".into(),
        Annotation {
            note: Note::Dimension {
                measured: Measured::Radius {
                    centre,
                    axis: Point3::Z,
                    radius: 1.0,
                },
                value: 1.0,
                tolerance: Tolerance::General,
                elsewhere: Vec::new(),
            },
            at: centre + Point3::new(2.0, 2.0, 0.0),
            sketch: None,
        },
    );
    let top = sheet_view(
        "top",
        ViewKind::Base { side: Side::Top },
        [150.0, 150.0],
        &[],
    );
    let laid_with = |at: [f64; 2]| {
        laid(
            &b,
            vec![top.clone()],
            vec![PlacedNote {
                note: "round".into(),
                view: "top".into(),
                at,
                decimals: None,
                starts: [None; 2],
                count: None,
                to: None,
            }],
        )
    };
    let laid = laid_with([8.0, 8.0]);
    let arrow = laid.fills.first().expect("its arrow").corners;
    let tip = arrow[0];
    let way = (tip - (arrow[1] + arrow[2]) / 2.0).normalize();
    // (Its narrow lines that are not level: on the one line through the
    // arrow.)
    for s in laid.strokes.iter().filter(|s| s.pen == Pen::Narrow) {
        if (s.a.y - s.b.y).abs() < 1e-9 || s.a.distance(tip) > 40.0 || s.b.distance(tip) > 40.0 {
            continue;
        }
        for p in [s.a, s.b] {
            assert!(
                (p - tip).perp_dot(way).abs() < 1e-6,
                "{s:?} off the line {tip:?} {way:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_detail_of_a_section_hatches_only_within_its_circle() -> anyhow::Result<()> {
    use workpiece_ops_drawing::lay::Pen;
    // The blocks' section, and a detail of it (a small circle over the
    // large block's cut, five times larger) far off on the sheet.
    let b = blocks()?;
    let section = || {
        sheet_view(
            "s",
            ViewKind::Section {
                of: None,
                section: "mid".into(),
                letter: "A".into(),
                turn: 0.0,
            },
            [100.0, 200.0],
            &[],
        )
    };
    let alone = laid(&b, vec![section()], vec![]);
    let s = &alone.views[0];
    // (A point of the large block's cut: the middle of a hatch line of
    // the left half of the view.)
    let hatch = |k: &workpiece_ops_drawing::lay::Stroke| {
        let d = (k.b - k.a).normalize_or_zero();
        k.pen == Pen::Narrow && (d.x * d.y).abs() > 0.4
    };
    let on = alone
        .strokes
        .iter()
        .filter(|k| hatch(k) && k.a.lerp(k.b, 0.5).x < s.centre.x)
        .map(|k| k.a.lerp(k.b, 0.5))
        .next()
        .expect("the large block is hatched");
    let centre = s.origin + (on - s.anchor) / s.scale;
    let mut detail = sheet_view(
        "d",
        ViewKind::Detail {
            of: "s".into(),
            centre: centre.to_array(),
            radius: 0.8,
            letter: "B".into(),
            letter_at: None,
        },
        [300.0, 100.0],
        &[],
    );
    detail.scale = Some([5, 1]);
    let laid = laid(&b, vec![section(), detail], vec![]);
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let d = laid.views.iter().find(|v| v.id == "d").expect("the detail");
    let r = 0.8 * d.scale;
    // Its hatching: some lines, all within the circle.
    let near: Vec<_> = laid
        .strokes
        .iter()
        .filter(|k| hatch(k) && k.a.distance(d.centre).min(k.b.distance(d.centre)) < 100.0)
        .collect();
    assert!(!near.is_empty());
    for k in near {
        assert!(
            k.a.distance(d.centre) <= r + 1e-6 && k.b.distance(d.centre) <= r + 1e-6,
            "{k:?} is out of the circle of {r} about {:?}",
            d.centre
        );
    }
    Ok(())
}

#[test]
fn a_caption_keeps_where_it_was_dragged_as_its_view_moves() -> anyhow::Result<()> {
    use workpiece_core::math::Point2;
    // The section of the blocks, its caption dragged 30 to the right of
    // its middle and 5 down; the view moved: the caption with it.
    let b = blocks()?;
    let section = |at: [f64; 2]| SheetView {
        caption_at: Some([30.0, -5.0]),
        ..sheet_view(
            "s",
            ViewKind::Section {
                of: None,
                section: "mid".into(),
                letter: "A".into(),
                turn: 0.0,
            },
            at,
            &[],
        )
    };
    for at in [[200.0, 150.0], [260.0, 90.0]] {
        let laid = laid(&b, vec![section(at)], vec![]);
        let v = &laid.views[0];
        let [lo, hi] = v.caption.expect("a caption");
        let middle = (lo + hi) / 2.0;
        assert!(
            (middle - v.anchor - Point2::new(30.0, -5.0)).length() < 1e-6,
            "{middle:?}"
        );
        let text = laid.texts.iter().find(|t| t.text == "A–A").expect("A–A");
        assert!((text.at - middle).length() < 0.5);
    }
    Ok(())
}

#[test]
fn a_section_s_cutting_line_is_put_into_a_view_that_sees_it_edge_on() -> anyhow::Result<()> {
    // The blocks cut by the plane y = 1, in no view; a view from above
    // (which sees the plane edge on) and one from the right (square on).
    let b = blocks()?;
    let views = |of: Option<&str>| {
        vec![
            sheet_view(
                "top",
                ViewKind::Base { side: Side::Top },
                [100.0, 200.0],
                &[],
            ),
            sheet_view(
                "right",
                ViewKind::Base { side: Side::Right },
                [100.0, 80.0],
                &[],
            ),
            sheet_view(
                "s",
                ViewKind::Section {
                    of: of.map(str::to_owned),
                    section: "mid".into(),
                    letter: "A".into(),
                    turn: 0.0,
                },
                [300.0, 200.0],
                &[],
            ),
        ]
    };
    let alone = laid(&b, views(None), vec![]);
    assert!(alone.cuts.is_empty());
    let s = alone
        .views
        .iter()
        .find(|v| v.id == "s")
        .expect("the section");
    assert!(
        s.cut_shows_in.contains(&"top".to_owned()),
        "{:?}",
        s.cut_shows_in
    );
    assert!(
        !s.cut_shows_in.contains(&"right".to_owned()),
        "{:?}",
        s.cut_shows_in
    );
    // Put there: its line, arrows and letters in the top view.
    let there = laid(&b, views(Some("top")), vec![]);
    assert!(there.problems.is_empty(), "{:?}", there.problems);
    assert_eq!(there.cuts.len(), 1);
    assert_eq!(there.cuts[0].of, "top");
    assert_eq!(there.cuts[0].arrows.len(), 2);
    let top = there.views.iter().find(|v| v.id == "top").expect("top");
    for end in &there.cuts[0].drawn {
        let off = (*end - top.centre).abs() - top.half;
        assert!(off.x > 0.0 || off.y > 0.0, "past the view: {end:?}");
    }
    let letters = |laid: &Laid| laid.texts.iter().filter(|t| t.text == "A").count();
    assert_eq!(letters(&there), letters(&alone) + 2);
    // A dimension's text put where a letter stands: the letter (and its
    // arrow) moves out along the line, off it.
    let letter = there
        .texts
        .iter()
        .find(|t| t.text == "A" && t.at.distance(there.cuts[0].arrows[0]) < 15.0)
        .expect("a letter by an arrow")
        .at;
    let mut b = b;
    b.world.annotations.insert(
        "wide".into(),
        workpiece_core::world::Annotation {
            note: workpiece_core::world::Note::Dimension {
                measured: workpiece_core::world::Measured::Length {
                    from: workpiece_core::math::Point3::new(-2.0, -2.0, 2.0),
                    to: workpiece_core::math::Point3::new(2.0, -2.0, 2.0),
                    along: None,
                },
                value: 4.0,
                tolerance: workpiece_core::world::Tolerance::General,
                elsewhere: Vec::new(),
            },
            at: workpiece_core::math::Point3::ZERO,
            sketch: None,
        },
    );
    let top = there.views.iter().find(|v| v.id == "top").expect("top");
    let at = letter - top.anchor;
    let note = PlacedNote {
        note: "wide".into(),
        view: "top".into(),
        at: [at.x, at.y],
        decimals: None,
        starts: [None; 2],
        count: None,
        to: None,
    };
    let moved = laid(&b, views(Some("top")), vec![note]);
    assert!(
        !moved.problems.iter().any(|p| p.contains("over each other")),
        "{:?}",
        moved.problems
    );
    let four = moved
        .texts
        .iter()
        .filter(|t| t.text == "4")
        .min_by(|a, b| a.at.distance(letter).total_cmp(&b.at.distance(letter)))
        .expect("the dimension");
    for t in moved.texts.iter().filter(|t| t.text == "A") {
        assert!(t.at.distance(four.at) > 3.0, "{:?} on {:?}", t.at, four.at);
    }
    Ok(())
}
