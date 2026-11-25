//! Centre lines and the traces of datums: what a view offers of them,
//! putting one on, and the line types they are drawn with (ISO 128-2).

use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::{Datum, DatumAxis, DatumPlane};
use workpiece_ops_drawing::Sheet;
use workpiece_ops_drawing::lay::{Kept, Laid, Pen, Placing, Subject, lay_out};
use workpiece_ops_drawing::sheet::{Centres, LineOf, SheetLine, SheetView, Side, Style, ViewKind};
use workpiece_ops_features::{Hole, HoleArgs};
use workpiece_ops_sketch::{Shape, Sketch};
use workpiece_ops_testing::*;

/// A block 4 × 4 × 2 with a hole through at (1, 1), a datum plane
/// square to x and a datum axis along y.
fn drilled_block() -> anyhow::Result<Standalone> {
    let mut b = Standalone::new();
    base(&mut b)?;
    let mut points = Sketch::new("points");
    points.args.plane = Some(xy());
    let p = points.args.add_point(Point2::new(1.0, 1.0));
    points.args.add(Shape::Point(p));
    points.apply(&mut b)?;
    Hole {
        id: "hole".into(),
        args: HoleArgs {
            place: workpiece_ops_features::HolePlace::Sketch,
            sketch: Some(Source::Profile("points".into()).into()),
            diameter: Measure::new(0.5),
            extent: workpiece_ops_sweep::Extent::ThroughAll,
            ..HoleArgs::default()
        },
    }
    .apply(&mut b)?;
    let plane = DatumPlane::new(Point3::new(0.5, 0.0, 0.0), Point3::X, Point3::Y)?;
    b.world.datums.insert("mid".into(), Datum::Plane(plane));
    let axis = DatumAxis::new(Point3::new(0.0, -0.5, 0.0), Point3::Y)?;
    b.world.datums.insert("spine".into(), Datum::Axis(axis));
    Ok(b)
}

fn top() -> SheetView {
    SheetView {
        id: "top".into(),
        kind: ViewKind::Base { side: Side::Top },
        at: [150.0, 150.0],
        scale: None,
        style: Style::HiddenRemoved,
        bodies: Vec::new(),
        cut_only: false,
        half: None,
        section: None,
        caption_at: None,
    }
}

fn lay(b: &Standalone, sheet: &Sheet, missing: bool) -> Laid {
    let properties = Default::default();
    let subject = Subject {
        world: &b.world,
        unit: workpiece_core::Unit::default(),
        material: None,
        name: "block",
        properties: &properties,
        parts: &Default::default(),
        densities: &Default::default(),
    };
    lay_out(sheet, &subject, &Kept::default(), missing.then_some(None))
}

/// What the sheet shows of lines as what they would be.
fn offered(laid: &Laid) -> Vec<LineOf> {
    laid.notes
        .iter()
        .filter(|n| n.preview.is_some())
        .filter_map(|n| match &n.kind {
            Placing::Line(of) => Some(of.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_view_offers_datum_traces_and_hole_axes_and_keeps_one_put_on() -> anyhow::Result<()> {
    let b = drilled_block()?;
    let mut sheet = Sheet {
        views: vec![top()],
        scale: [10, 1],
        centres: Centres::None,
        ..Sheet::default()
    };
    // From above: the plane square to x is seen edge on, a line; the
    // axis along y a line; the hole end on, a centre mark.
    let laid = lay(&b, &sheet, true);
    let offers = offered(&laid);
    assert!(offers.contains(&LineOf::Datum("mid".into())), "{offers:?}");
    assert!(
        offers.contains(&LineOf::Datum("spine".into())),
        "{offers:?}"
    );
    let hole = offers
        .iter()
        .find(|of| matches!(of, LineOf::Axis(name) if name.starts_with("hole/")))
        .cloned()
        .expect("the hole's centre line is offered");
    // Seen from the front the plane is face on: not offered there.
    let mut front = top();
    front.kind = ViewKind::Base { side: Side::Front };
    let from_front = Sheet {
        views: vec![front],
        ..sheet.clone()
    };
    assert!(!offered(&lay(&b, &from_front, true)).contains(&LineOf::Datum("mid".into())));
    // Put on, it is drawn in its view, long-dashed dotted, bound to the
    // hole, and offered no longer.
    sheet.lines.push(SheetLine {
        view: Some("top".into()),
        of: hole.clone(),
        ends: [0.0; 2],
        hidden: false,
        gaps: Vec::new(),
        symmetry: false,
        label: None,
        label_at: None,
    });
    let laid = lay(&b, &sheet, true);
    assert!(!offered(&laid).contains(&hole));
    let put = laid
        .notes
        .iter()
        .find(|n| n.preview.is_none() && n.kind == Placing::Line(hole.clone()))
        .expect("the line on the sheet");
    assert_eq!((put.index, put.view.as_str()), (0, "top"));
    // (A cross, each arm from the middle out, past the circle by the
    // sheet's 3 mm: the hole is 5 mm across at 10:1.)
    assert_eq!(put.path.len(), 4);
    let middle = put.path[0].0;
    let reach = put
        .path
        .iter()
        .map(|(_, b)| b.distance(middle))
        .fold(0.0, f64::max);
    assert!((reach - (2.5 + 3.0)).abs() < 1e-2, "{reach}");
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    // Without previews the line stays; the datums are not drawn.
    let plain = lay(&b, &sheet, false);
    assert!(offered(&plain).is_empty());
    assert!(
        plain
            .notes
            .iter()
            .any(|n| n.kind == Placing::Line(hole.clone()))
    );
    Ok(())
}

#[test]
fn line_types_are_iso_128_2() {
    // The group 0.5 / 0.25: wide twice narrow.
    let wide = 0.5;
    assert_eq!(Pen::Wide.width(wide), 2.0 * Pen::Narrow.width(wide));
    assert_eq!(Pen::Chain.width(wide), 0.25);
    assert_eq!(Pen::ChainWide.width(wide), 0.5);
    // In the line's own width d: dashed 12d, 3d; long-dashed dotted 24d,
    // 3d, 0.5d, 3d; continuous none.
    assert_eq!(Pen::Dashed.dashes(wide), vec![3.0, 0.75]);
    assert_eq!(Pen::Chain.dashes(wide), vec![6.0, 0.75, 0.125, 0.75]);
    assert_eq!(Pen::ChainWide.dashes(wide), vec![12.0, 1.5, 0.25, 1.5]);
    assert!(Pen::Wide.dashes(wide).is_empty() && Pen::Narrow.dashes(wide).is_empty());
    // An A1 sheet's group is 0.7 / 0.35.
    let drafting = workpiece_ops_drawing::sheet::Drafting::default();
    assert_eq!(drafting.wide(workpiece_ops_drawing::sheet::Format::A1), 0.7);
    assert_eq!(drafting.wide(workpiece_ops_drawing::sheet::Format::A3), 0.5);
}

#[test]
fn the_holes_of_parts_placed_in_an_assembly_have_centre_lines() -> anyhow::Result<()> {
    // The drilled block placed twice, side by side: the sheet's centre
    // lines of holes, drawn by themselves, are of both parts' holes.
    let part = std::sync::Arc::new(drilled_block()?.world);
    let mut b = Standalone::new();
    for (name, x) in [("one", 0.0), ("two", 10.0)] {
        b.world.instances.insert(
            name.into(),
            workpiece_core::world::Instance {
                pose: workpiece_core::Pose::from_translation(Point3::new(x, 0.0, 0.0)),
                world: part.clone(),
                fixed: true,
            },
        );
    }
    let sheet = Sheet {
        views: vec![top()],
        centres: Centres::Holes,
        ..Sheet::default()
    };
    let laid = lay(&b, &sheet, false);
    let bare = lay(
        &b,
        &Sheet {
            centres: Centres::None,
            ..sheet.clone()
        },
        false,
    );
    // Seen from above, each hole a centre mark: what the centre lines
    // add, about two middles as far apart as the parts.
    let added: Vec<_> = laid
        .strokes
        .iter()
        .filter(|s| !bare.strokes.contains(s))
        .collect();
    let v = &laid.views[0];
    // (From above, x runs down the sheet.)
    let (lo, hi) = added
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), s| {
            (lo.min(s.a.y).min(s.b.y), hi.max(s.a.y).max(s.b.y))
        });
    let reach = 10.0 * v.scale;
    assert!(
        hi - lo > reach && hi - lo < reach + 0.5 * v.scale + 10.0,
        "{lo} {hi}"
    );
    Ok(())
}

#[test]
fn a_centre_line_can_be_taken_off_broken_and_marked() -> anyhow::Result<()> {
    let b = drilled_block()?;
    let base = Sheet {
        views: vec![top()],
        centres: Centres::Holes,
        ..Sheet::default()
    };
    let chains = |laid: &Laid| {
        laid.strokes
            .iter()
            .filter(|s| s.pen == Pen::Chain || s.pen == Pen::Narrow)
            .count()
    };
    let with = |line: SheetLine| {
        let mut sheet = base.clone();
        sheet.lines.push(line);
        lay(&b, &sheet, false)
    };
    let hole = LineOf::Axis("hole".into());
    let line = |hidden, gaps: Vec<[f64; 2]>, symmetry, label: Option<&str>| SheetLine {
        view: Some("top".into()),
        of: hole.clone(),
        ends: [0.0; 2],
        hidden,
        gaps,
        symmetry,
        label: label.map(str::to_owned),
        label_at: None,
    };
    let bare = lay(
        &b,
        &Sheet {
            centres: Centres::None,
            ..base.clone()
        },
        false,
    );
    let auto = lay(&b, &base, false);
    assert!(chains(&auto) > chains(&bare));
    // Taken off: as if the sheet drew none.
    assert_eq!(
        chains(&with(line(true, vec![], false, None))),
        chains(&bare)
    );
    // Marked and labelled: more lines, and its words.
    let marked = with(line(false, vec![], true, Some("CL SYM")));
    assert!(chains(&marked) > chains(&auto));
    assert!(marked.texts.iter().any(|t| t.text == "CL SYM"));
    // Broken: less of it drawn.
    let length = |laid: &Laid| -> f64 {
        laid.strokes
            .iter()
            .filter(|s| s.pen == Pen::Chain)
            .map(|s| s.a.distance(s.b))
            .sum()
    };
    // (The datum plane's trace, put on, whole and broken.)
    let trace = |gaps| SheetLine {
        of: LineOf::Datum("mid".into()),
        ..line(false, gaps, false, None)
    };
    let whole = with(trace(vec![]));
    let broken = with(trace(vec![[2.0, 5.0]]));
    assert!(
        (length(&whole) - length(&broken) - 3.0).abs() < 1e-6,
        "{} {}",
        length(&broken),
        length(&whole)
    );
    Ok(())
}

#[test]
fn a_line_said_of_one_hole_is_drawn_as_said_over_every_centre_line() -> anyhow::Result<()> {
    // Every centre line drawn by itself, and the hole's said to be
    // broken: from the front (the hole seen from its side) less of
    // its axis is drawn than with every centre line alone.
    let b = drilled_block()?;
    let front = SheetView {
        id: "front".into(),
        kind: ViewKind::Base { side: Side::Front },
        ..top()
    };
    let all = Sheet {
        views: vec![front],
        centres: Centres::All,
        ..Sheet::default()
    };
    let length = |laid: &Laid| -> f64 {
        laid.strokes
            .iter()
            .filter(|s| s.pen == Pen::Chain && (s.a.y - 150.0).abs() < 40.0)
            .map(|s| s.a.distance(s.b))
            .sum()
    };
    let mut broken = all.clone();
    broken.lines.push(SheetLine {
        view: Some("front".into()),
        of: LineOf::Axis("hole".into()),
        ends: [0.0; 2],
        hidden: false,
        gaps: vec![[1.0, 3.0]],
        symmetry: false,
        label: None,
        label_at: None,
    });
    let (whole, less) = (
        length(&lay(&b, &all, false)),
        length(&lay(&b, &broken, false)),
    );
    assert!(
        whole > 0.0 && (whole - less - 2.0).abs() < 0.2,
        "{whole} {less}"
    );
    Ok(())
}

#[test]
fn a_plane_of_symmetry_is_drawn_with_its_words_where_asked() -> anyhow::Result<()> {
    use workpiece_ops_drawing::sheet::{LabelAt, LineEnd};
    // The origin's ZX plane, seen edge on from above: its trace, marked
    // CL SYM at its start, then at its end.
    let b = drilled_block()?;
    let at = |label_at| {
        let sheet = Sheet {
            views: vec![top()],
            centres: Centres::None,
            lines: vec![SheetLine {
                view: Some("top".into()),
                of: LineOf::Datum("origin/zx".into()),
                ends: [0.0; 2],
                hidden: false,
                gaps: Vec::new(),
                symmetry: true,
                label: Some("CL SYM".into()),
                label_at,
            }],
            ..Sheet::default()
        };
        let laid = lay(&b, &sheet, false);
        assert!(laid.problems.is_empty(), "{:?}", laid.problems);
        laid.texts
            .iter()
            .find(|t| t.text == "CL SYM")
            .expect("its words")
            .at
    };
    let (start, end) = (
        at(Some(LabelAt::End(LineEnd::Start))),
        at(Some(LabelAt::End(LineEnd::End))),
    );
    assert!(start.distance(end) > 5.0, "{start} {end}");
    // By itself: at one of those ends.
    let free = at(None);
    assert!(free.distance(start) < 1e-9 || free.distance(end) < 1e-9);
    Ok(())
}
