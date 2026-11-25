//! How what is said of the model stands in the views: extension lines
//! from the outline, ends on what a view hides or on nothing, a detail's
//! lengths in the detail, radii each with a leader of their own, words
//! that belong to a dimension, notes clear of other views; and how the
//! views are drawn: tangent edges thin, half sections, dashes along a
//! curve, what overlaps told as a problem.

use workpiece_core::math::Point3;
use workpiece_core::world::{Annotation, Cutting, Measured, Note, Tolerance};
use workpiece_ops_drawing::Sheet;
use workpiece_ops_drawing::lay::{Kept, Laid, Pen, Subject, Wanted, lay_out, place_all};
use workpiece_ops_drawing::sheet::{
    Half, HalfSide, PlacedNote, SheetView, Side, Style, TangentEdges, ViewKind, Words,
};
use workpiece_ops_testing::*;

/// A block 4 × 4 × 4 about the origin, and one 2 × 2 × 2 about
/// (`at`, 0, 0).
fn blocks(at: f64) -> anyhow::Result<Standalone> {
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
    let there =
        workpiece_core::world::DatumFrame::new(Point3::new(at, 0.0, 0.0), Point3::X, Point3::Y)?;
    b.world
        .datums
        .insert("there".into(), workpiece_core::world::Datum::Frame(there));
    block(&mut b, "small", "there", 2.0)?;
    Ok(b)
}

fn say(b: &mut Standalone, name: &str, measured: Measured, value: f64) {
    let note = Annotation {
        note: Note::Dimension {
            measured,
            value,
            tolerance: Tolerance::General,
            elsewhere: Vec::new(),
        },
        at: Point3::ZERO,
        sketch: None,
    };
    b.world.annotations.insert(name.into(), note);
}

fn length(from: [f64; 3], to: [f64; 3]) -> Measured {
    Measured::Length {
        from: Point3::from_array(from),
        to: Point3::from_array(to),
        along: None,
    }
}

fn view(id: &str, kind: ViewKind, at: [f64; 2], style: Style) -> SheetView {
    SheetView {
        id: id.into(),
        kind,
        at,
        scale: None,
        style,
        bodies: Vec::new(),
        cut_only: false,
        half: None,
        section: None,
        caption_at: None,
    }
}

fn front(style: Style) -> SheetView {
    view(
        "front",
        ViewKind::Base { side: Side::Front },
        [100.0, 100.0],
        style,
    )
}

fn note(note: &str, view: &str, at: [f64; 2]) -> PlacedNote {
    PlacedNote {
        note: note.into(),
        view: view.into(),
        at,
        decimals: None,
        starts: [None; 2],
        count: None,
    }
}

fn lay(b: &Standalone, sheet: &Sheet) -> Laid {
    let properties = Default::default();
    let subject = Subject {
        world: &b.world,
        unit: workpiece_core::Unit::default(),
        material: None,
        name: "blocks",
        properties: &properties,
        parts: &Default::default(),
        densities: &Default::default(),
    };
    lay_out(sheet, &subject, &Kept::default(), None)
}

fn sheet(views: Vec<SheetView>, notes: Vec<PlacedNote>) -> Sheet {
    Sheet {
        views,
        notes,
        ..Sheet::default()
    }
}

#[test]
fn extension_lines_start_at_the_outline_nearest_the_dimension_line() -> anyhow::Result<()> {
    // The large block's height, measured through its middle, its text
    // 30 to the right of the view: its extension lines start beside
    // the right end of its top and bottom edges, not in the middle.
    let mut b = blocks(10.0)?;
    say(
        &mut b,
        "high",
        length([0.0, 0.0, -2.0], [0.0, 0.0, 2.0]),
        4.0,
    );
    let laid = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenRemoved)],
            vec![note("high", "front", [30.0, 0.0])],
        ),
    );
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let linear = laid.notes[0].linear.expect("a length");
    for from in linear.from {
        assert!((from.x - 102.0).abs() < 1e-6, "{from:?}");
    }
    // (Each extension line from there on, a gap off the outline.)
    let starts: Vec<f64> = laid
        .strokes
        .iter()
        .filter(|s| {
            s.pen == Pen::Narrow && (s.a.y - s.b.y).abs() < 1e-9 && (s.a.y - 102.0).abs() < 1e-6
        })
        .map(|s| s.a.x.min(s.b.x))
        .collect();
    assert!(starts.iter().all(|x| *x >= 102.0), "{starts:?}");
    Ok(())
}

#[test]
fn a_dimension_on_a_hidden_edge_or_on_nothing_is_told() -> anyhow::Result<()> {
    // The small block behind the large one, seen from the front: its
    // height ends on edges the view hides.
    let mut b = blocks(-10.0)?;
    say(
        &mut b,
        "hidden",
        length([-10.0, 0.0, -1.0], [-10.0, 0.0, 1.0]),
        2.0,
    );
    let notes = vec![note("hidden", "front", [30.0, 0.0])];
    let removed = lay(&b, &sheet(vec![front(Style::HiddenRemoved)], notes.clone()));
    assert!(
        removed
            .problems
            .iter()
            .any(|p| p.contains("hidden") && p.contains("hides")),
        "{:?}",
        removed.problems
    );
    // Drawn with its hidden edges, it is not a problem.
    let dashed = lay(&b, &sheet(vec![front(Style::HiddenDashed)], notes));
    assert!(dashed.problems.is_empty(), "{:?}", dashed.problems);
    // Between two points on nothing at all: its extension lines start
    // in empty space.
    say(
        &mut b,
        "loose",
        length([0.0, -0.5, 0.7], [0.0, 0.5, 0.7]),
        1.0,
    );
    let loose = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenDashed)],
            vec![note("loose", "front", [0.0, 30.0])],
        ),
    );
    assert!(
        loose
            .problems
            .iter()
            .any(|p| p.contains("loose") && p.contains("nothing")),
        "{:?}",
        loose.problems
    );
    Ok(())
}

#[test]
fn a_length_in_a_detail_stands_by_the_detail() -> anyhow::Result<()> {
    // The small block in front, its width measured far below it (as
    // between two faces' middles), shown in a detail of its middle at
    // 5:1: drawn there, from its sides as the detail shows them.
    let mut b = blocks(10.0)?;
    say(
        &mut b,
        "wide",
        length([10.0, -1.0, -50.0], [10.0, 1.0, -50.0]),
        2.0,
    );
    let detail = SheetView {
        scale: Some([5, 1]),
        ..view(
            "detail",
            ViewKind::Detail {
                of: "front".into(),
                centre: [0.0, 0.0],
                radius: 1.5,
                letter: "B".into(),
                letter_at: None,
            },
            [300.0, 150.0],
            Style::HiddenRemoved,
        )
    };
    let laid = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenRemoved), detail],
            vec![note("wide", "detail", [0.0, -12.0])],
        ),
    );
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let linear = laid.notes[0].linear.expect("a length");
    for from in linear.from {
        assert!(from.distance(Point2::new(300.0, 150.0)) < 7.6, "{from:?}");
    }
    assert!((linear.from[0].x - 295.0).abs() < 1e-6 || (linear.from[1].x - 295.0).abs() < 1e-6);
    // Its value (10 mm between the extension lines, too little for it
    // and the arrows) stands on neither arrow: past them.
    let text = laid.notes[0].rect;
    for arrow in [(295.0, 298.5), (301.5, 305.0)] {
        assert!(text[1].x <= arrow.0 || text[0].x >= arrow.1, "{text:?}");
    }
    // The detail's letter in the front view: on no line, not on the
    // circle.
    let letter = laid
        .texts
        .iter()
        .find(|t| t.text == "B")
        .expect("the letter");
    let (lo, hi) = (
        letter.at - Point2::splat(1.0),
        letter.at + Point2::splat(1.0),
    );
    for s in &laid.strokes {
        let crosses = (0..=20).any(|k| {
            let p = s.a.lerp(s.b, f64::from(k) / 20.0);
            p.x > lo.x && p.x < hi.x && p.y > lo.y && p.y < hi.y
        });
        assert!(!crosses, "{s:?} by {:?}", letter.at);
    }
    Ok(())
}

#[test]
fn a_length_with_one_end_out_of_a_detail_is_a_half_dimension() -> anyhow::Result<()> {
    // The small block's width (across the front view, from -1 to 1),
    // in a detail about its one side only, 0.8 around, at 5:1: its
    // other side is out of it.
    let mut b = blocks(10.0)?;
    say(
        &mut b,
        "wide",
        length([10.0, -1.0, -50.0], [10.0, 1.0, -50.0]),
        2.0,
    );
    let detail = SheetView {
        scale: Some([5, 1]),
        ..view(
            "detail",
            ViewKind::Detail {
                of: "front".into(),
                centre: [-1.0, 0.0],
                radius: 0.8,
                letter: "B".into(),
                letter_at: None,
            },
            [300.0, 150.0],
            Style::HiddenRemoved,
        )
    };
    // It is offered there: one end is in it.
    let views = vec![front(Style::HiddenRemoved), detail];
    let properties = Default::default();
    let subject = Subject {
        world: &b.world,
        unit: workpiece_core::Unit::default(),
        material: None,
        name: "blocks",
        properties: &properties,
        parts: &Default::default(),
        densities: &Default::default(),
    };
    let wanted = [Wanted {
        note: "wide".into(),
        view: Some("detail".into()),
        at: Some([0.0, -12.0]),
    }];
    let places = place_all(
        &sheet(views.clone(), vec![]),
        &subject,
        &Kept::default(),
        &wanted,
    );
    assert_eq!(
        places[0].as_ref().map(|p| (p.0.as_str(), p.2)),
        Some(("detail", true))
    );
    let without = lay(&b, &sheet(views.clone(), vec![]));
    let laid = lay(
        &b,
        &sheet(views, vec![note("wide", "detail", [0.0, -12.0])]),
    );
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    // One arrowhead; one extension line (upright, at its left side);
    // its line from there out a little past the circle (4 mm about
    // x 300), not on to its right side (at 310); the true value.
    assert_eq!(laid.fills.len(), without.fills.len() + 1);
    let new: Vec<_> = laid.strokes[without.strokes.len()..].to_vec();
    let upright: Vec<_> = new
        .iter()
        .filter(|s| (s.a.x - s.b.x).abs() < 1e-6 && s.a.y < 150.0)
        .collect();
    assert_eq!(upright.len(), 1, "{new:?}");
    assert!((upright[0].a.x - 300.0).abs() < 1e-6);
    let line = new
        .iter()
        .find(|s| (s.a.y - s.b.y).abs() < 1e-6 && (s.a.y - 138.0).abs() < 1.0)
        .expect("its dimension line");
    let (from, to) = (line.a.x.min(line.b.x), line.a.x.max(line.b.x));
    assert!((from - 300.0).abs() < 1e-6, "{line:?}");
    assert!(to > 304.0 && to < 310.0, "{line:?}");
    assert!(laid.texts.iter().any(|t| t.text == "2"));
    Ok(())
}

#[test]
fn a_diameter_across_the_axis_of_a_half_section_has_one_arrow() -> anyhow::Result<()> {
    // The rounded post (2 in radius about z), cut through its axis, its
    // left half only drawn: its diameter from the side runs from its
    // left side a little past the axis, with one arrowhead.
    let mut b = post()?;
    b.world.sections.insert(
        "mid".into(),
        Cutting::plane(Point3::new(0.0, 0.0, 0.0), Point3::Y),
    );
    say(
        &mut b,
        "across",
        Measured::Diameter {
            centre: Point3::new(0.0, 0.0, 1.0),
            axis: Point3::Z,
            radius: 2.0,
        },
        4.0,
    );
    let mut section = view(
        "s",
        ViewKind::Section {
            of: None,
            section: "mid".into(),
            letter: "A".into(),
            turn: 0.0,
        },
        [200.0, 100.0],
        Style::HiddenRemoved,
    );
    section.half = Some(Half {
        cut: HalfSide::Left,
        only: true,
    });
    section.scale = Some([5, 1]);
    let views = vec![section];
    let without = lay(&b, &sheet(views.clone(), vec![]));
    let laid = lay(&b, &sheet(views, vec![note("across", "s", [0.0, -10.0])]));
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    assert_eq!(laid.fills.len(), without.fills.len() + 1);
    let v = &laid.views[0];
    let axis = v.anchor.x + (0.0 - v.origin.x) * v.scale;
    let line = laid.strokes[without.strokes.len()..]
        .iter()
        .find(|s| (s.a.y - s.b.y).abs() < 1e-6 && s.a.y < v.anchor.y - 5.0)
        .expect("its dimension line");
    let (from, to) = (line.a.x.min(line.b.x), line.a.x.max(line.b.x));
    assert!(
        (from - (axis - 2.0 * v.scale)).abs() < 0.01,
        "{line:?} about {axis}"
    );
    assert!(
        to > axis + 1.0 && to < axis + 2.0 * v.scale,
        "{line:?} about {axis}"
    );
    assert!(laid.texts.iter().any(|t| t.text == "Ø4"));
    Ok(())
}

#[test]
fn two_radii_about_one_middle_each_have_a_leader_of_their_own() -> anyhow::Result<()> {
    let mut b = blocks(10.0)?;
    for (name, r) in [("outer", 2.0), ("inner", 1.5)] {
        let measured = Measured::Radius {
            centre: Point3::new(10.0, 0.0, 0.0),
            axis: Point3::X,
            radius: r,
        };
        say(&mut b, name, measured, r);
    }
    let sheet = sheet(vec![front(Style::HiddenRemoved)], vec![]);
    let properties = Default::default();
    let subject = Subject {
        world: &b.world,
        unit: workpiece_core::Unit::default(),
        material: None,
        name: "blocks",
        properties: &properties,
        parts: &Default::default(),
        densities: &Default::default(),
    };
    let wanted: Vec<Wanted> = ["inner", "outer"]
        .iter()
        .map(|n| Wanted {
            note: (*n).into(),
            view: Some("front".into()),
            at: None,
        })
        .collect();
    let places = place_all(&sheet, &subject, &Kept::default(), &wanted);
    let ways: Vec<Point2> = places
        .iter()
        .map(|p| Point2::from_array(p.as_ref().expect("placed").1).normalize())
        .collect();
    assert!(
        ways[0].angle_to(ways[1]).abs() > 10f64.to_radians(),
        "{ways:?}"
    );
    Ok(())
}

#[test]
fn words_that_belong_to_a_dimension_stand_under_its_value() -> anyhow::Result<()> {
    let mut b = blocks(10.0)?;
    // (A value no zone of the frame is called.)
    say(
        &mut b,
        "high",
        length([0.0, 2.0, -2.0], [0.0, 2.0, 2.0]),
        37.0,
    );
    let mut sheet = sheet(
        vec![front(Style::HiddenRemoved)],
        vec![note("high", "front", [20.0, 0.0])],
    );
    sheet.words.push(Words {
        view: None,
        at: [0.0, 0.0],
        text: "(2) PLACES".into(),
        height: 3.5,
        dimension: Some("high".into()),
        to: Vec::new(),
    });
    let laid = lay(&b, &sheet);
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let value = laid
        .texts
        .iter()
        .find(|t| t.text == "37")
        .expect("the value");
    let words = laid
        .texts
        .iter()
        .find(|t| t.text == "(2) PLACES")
        .expect("the words");
    // Turned as it is (upright, read from the right), beside it: under
    // it, or (where under it they would run across its own extension
    // line, the value standing past its arrows) after it on its line.
    assert!((words.angle - value.angle).abs() < 1e-9);
    assert!(
        words.at.distance(value.at) < 20.0,
        "{:?} {:?}",
        words.at,
        value.at
    );
    // Across none of the dimension's own lines.
    let rect = laid
        .notes
        .iter()
        .find(|n| n.kind == workpiece_ops_drawing::lay::Placing::Words)
        .expect("the words")
        .rect;
    assert!(
        !laid.strokes.iter().any(|s| (0..=20).any(|k| {
            let p = s.a.lerp(s.b, f64::from(k) / 20.0);
            p.x > rect[0].x + 0.1
                && p.x < rect[1].x - 0.1
                && p.y > rect[0].y + 0.1
                && p.y < rect[1].y - 0.1
        })),
        "{rect:?}"
    );
    // Words over another text are told.
    sheet.words.push(Words {
        view: None,
        at: [0.0, 0.0],
        text: "AGAIN".into(),
        height: 3.5,
        dimension: Some("high".into()),
        to: Vec::new(),
    });
    let laid = lay(&b, &sheet);
    assert!(
        laid.problems.iter().any(|p| p.contains("over each other")),
        "{:?}",
        laid.problems
    );
    Ok(())
}

#[test]
fn a_note_put_by_itself_keeps_clear_of_another_view() -> anyhow::Result<()> {
    // Two views close above each other; the large block's width in the
    // upper one is put below it by itself, which is where the lower
    // view is: it goes to the other side.
    let mut b = blocks(10.0)?;
    say(
        &mut b,
        "wide",
        length([2.0, -2.0, -1.5], [2.0, 2.0, -1.5]),
        4.0,
    );
    let upper = front(Style::HiddenRemoved);
    let lower = view(
        "lower",
        ViewKind::Base { side: Side::Front },
        [100.0, 88.0],
        Style::HiddenRemoved,
    );
    let sheet = sheet(vec![upper, lower], vec![]);
    let properties = Default::default();
    let subject = Subject {
        world: &b.world,
        unit: workpiece_core::Unit::default(),
        material: None,
        name: "blocks",
        properties: &properties,
        parts: &Default::default(),
        densities: &Default::default(),
    };
    let wanted = [Wanted {
        note: "wide".into(),
        view: Some("front".into()),
        at: None,
    }];
    let places = place_all(&sheet, &subject, &Kept::default(), &wanted);
    let (_, at, asked) = places[0].clone().expect("placed");
    assert!(asked);
    assert!(at[1] > 0.0, "{at:?}");
    Ok(())
}

/// A post 4 across and 3 high with its top rim rounded 0.5.
fn post() -> anyhow::Result<Standalone> {
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
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
    workpiece_ops_blend::Fillet {
        id: "round".into(),
        args: workpiece_ops_blend::FilletArgs {
            edges: Reference::each(vec![Source::Topology("post/top-rim".into())]),
            size: Measure::new(0.5),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    Ok(b)
}

#[test]
fn tangent_edges_are_thin_or_not_drawn_and_outlines_stay_wide() -> anyhow::Result<()> {
    let b = post()?;
    let top = view(
        "top",
        ViewKind::Base { side: Side::Top },
        [100.0, 100.0],
        Style::HiddenRemoved,
    );
    // (The strokes that lie on a circle of `r` about the view's middle.)
    let at = |laid: &Laid, r: f64, pen: Pen| {
        let c = laid.views[0].anchor;
        laid.strokes
            .iter()
            .filter(|s| {
                s.pen == pen
                    && ((s.a.distance(c) - r).abs() < 0.05)
                    && ((s.b.distance(c) - r).abs() < 0.05)
            })
            .count()
    };
    // From above: where the round meets the top, thin; its outline wide.
    let mut sheet = sheet(vec![top], vec![]);
    let thin = lay(&b, &sheet);
    assert!(at(&thin, 1.5, Pen::Narrow) > 0 && at(&thin, 1.5, Pen::Wide) == 0);
    assert!(at(&thin, 2.0, Pen::Wide) > 0);
    sheet.tangent_edges = TangentEdges::Hidden;
    let hidden = lay(&b, &sheet);
    assert!(at(&hidden, 1.5, Pen::Narrow) == 0 && at(&hidden, 1.5, Pen::Wide) == 0);
    sheet.tangent_edges = TangentEdges::Wide;
    let wide = lay(&b, &sheet);
    assert!(at(&wide, 1.5, Pen::Wide) > 0);
    Ok(())
}

#[test]
fn a_half_section_is_cut_on_one_side_of_its_centre_line() -> anyhow::Result<()> {
    let mut b = blocks(10.0)?;
    b.world.sections.insert(
        "mid".into(),
        Cutting::plane(Point3::new(0.0, 0.5, 0.0), Point3::Y),
    );
    let section = |half: Option<Half>| SheetView {
        half,
        ..view(
            "s",
            ViewKind::Section {
                of: None,
                section: "mid".into(),
                letter: "A".into(),
                turn: 0.0,
            },
            [200.0, 100.0],
            Style::HiddenRemoved,
        )
    };
    let whole = lay(&b, &sheet(vec![section(None)], vec![]));
    let left = Half {
        cut: HalfSide::Left,
        only: false,
    };
    let half = lay(&b, &sheet(vec![section(Some(left))], vec![]));
    assert!(half.problems.is_empty(), "{:?}", half.problems);
    // The hatching only left of the centre line, which is drawn.
    let hatching = |laid: &Laid| {
        laid.strokes
            .iter()
            .filter(|s| {
                s.pen == Pen::Narrow && (s.a.x - s.b.x).abs() > 1e-6 && (s.a.y - s.b.y).abs() > 1e-6
            })
            .map(|s| s.a.x.max(s.b.x))
            .collect::<Vec<f64>>()
    };
    // Divided at the section's own origin (nothing round is cut through
    // its axis here): the large block's middle, not the view's.
    let middle = half
        .strokes
        .iter()
        .find(|s| {
            s.pen == Pen::Chain && (s.a.x - s.b.x).abs() < 1e-9 && (s.a.y - 100.0).abs() < 50.0
        })
        .expect("the centre line")
        .a
        .x;
    assert!(hatching(&whole).iter().any(|x| *x > middle + 1.0));
    assert!(!hatching(&half).is_empty() && hatching(&half).iter().all(|x| *x <= middle + 1e-6));
    assert!(
        half.strokes
            .iter()
            .any(|s| s.pen == Pen::Chain && (s.a.x - middle).abs() < 1e-6)
    );
    assert!((middle - half.views[0].anchor.x).abs() > 1.0, "{middle}");
    // Only its cut half: narrower, cut off at that line.
    let only = lay(
        &b,
        &sheet(vec![section(Some(Half { only: true, ..left }))], vec![]),
    );
    assert!(
        only.views[0].half.x < whole.views[0].half.x - 0.5,
        "{} {} {middle}",
        only.views[0].half.x,
        whole.views[0].half.x
    );
    Ok(())
}

#[test]
fn a_section_at_a_scale_of_its_own_says_it() -> anyhow::Result<()> {
    let mut b = blocks(10.0)?;
    b.world.sections.insert(
        "mid".into(),
        Cutting::plane(Point3::new(0.0, 0.5, 0.0), Point3::Y),
    );
    let top = view(
        "top",
        ViewKind::Base { side: Side::Top },
        [100.0, 200.0],
        Style::HiddenRemoved,
    );
    let section = SheetView {
        scale: Some([2, 1]),
        ..view(
            "s",
            ViewKind::Section {
                of: Some("top".into()),
                section: "mid".into(),
                letter: "A".into(),
                turn: 0.0,
            },
            [100.0, 80.0],
            Style::HiddenRemoved,
        )
    };
    let laid = lay(&b, &sheet(vec![top, section], vec![]));
    assert!(laid.texts.iter().any(|t| t.text == "A–A (2:1)"));
    let s = laid
        .views
        .iter()
        .find(|v| v.id == "s")
        .expect("the section");
    assert!((s.scale - 2.0).abs() < 1e-9);
    Ok(())
}

#[test]
fn a_view_over_the_title_block_is_told() -> anyhow::Result<()> {
    let b = blocks(10.0)?;
    let low = view(
        "low",
        ViewKind::Base { side: Side::Front },
        [380.0, 30.0],
        Style::HiddenRemoved,
    );
    let laid = lay(&b, &sheet(vec![low], vec![]));
    assert!(
        laid.problems.iter().any(|p| p.contains("title block")),
        "{:?}",
        laid.problems
    );
    Ok(())
}

#[test]
fn a_size_said_at_several_places_is_drawn_at_the_one_its_text_is_by() -> anyhow::Result<()> {
    // A radius of two corners, one left and one right in the front
    // view; its text put by the right one: its leader is from there.
    let mut b = blocks(10.0)?;
    let at = |y: f64| Measured::Radius {
        centre: Point3::new(2.0, y, 1.0),
        axis: Point3::X,
        radius: 0.5,
    };
    b.world.annotations.insert(
        "r".into(),
        Annotation {
            note: Note::Dimension {
                measured: at(-1.5),
                value: 0.5,
                tolerance: Tolerance::General,
                elsewhere: vec![at(1.5)],
            },
            at: Point3::ZERO,
            sketch: None,
        },
    );
    let laid = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenRemoved)],
            vec![note("r", "front", [12.0, 10.0])],
        ),
    );
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    // (The leader's strokes all lie right of the view's middle.)
    let narrow: Vec<_> = laid
        .strokes
        .iter()
        .filter(|s| s.pen == Pen::Narrow && s.a.distance(Point2::new(100.0, 100.0)) < 30.0)
        .collect();
    assert!(
        !narrow.is_empty() && narrow.iter().all(|s| s.a.x > 100.0 && s.b.x > 100.0),
        "{narrow:?}"
    );
    Ok(())
}

/// An assembly: the blocks placed twice, as "left" and as "right"
/// 20 along y; nothing of its own.
fn assembly() -> anyhow::Result<Standalone> {
    let parts = std::sync::Arc::new(blocks(10.0)?.world);
    let mut b = Standalone::new();
    for (name, y) in [("left", 0.0), ("right", 20.0)] {
        b.world.instances.insert(
            name.into(),
            workpiece_core::world::Instance {
                pose: workpiece_core::Pose::from_translation(Point3::new(0.0, y, 0.0)),
                world: parts.clone(),
                fixed: true,
            },
        );
    }
    Ok(b)
}

#[test]
fn a_view_of_an_assembly_shows_the_parts_it_names() -> anyhow::Result<()> {
    let b = assembly()?;
    let top = |bodies: &[&str]| SheetView {
        bodies: bodies.iter().map(|n| (*n).to_owned()).collect(),
        ..view(
            "top",
            ViewKind::Base { side: Side::Top },
            [200.0, 150.0],
            Style::HiddenRemoved,
        )
    };
    let wide = |bodies: &[&str]| {
        let laid = lay(&b, &sheet(vec![top(bodies)], vec![]));
        (laid.views[0].half.x, laid.problems)
    };
    let (all, problems) = wide(&[]);
    assert!(problems.is_empty(), "{problems:?}");
    // One placed part; one body of one; both blocks of one.
    let (left, problems) = wide(&["left"]);
    assert!(
        problems.is_empty() && left < all * 0.5,
        "{left} {all} {problems:?}"
    );
    let (large, problems) = wide(&["left/large"]);
    assert!(
        problems.is_empty() && (large - 2.0).abs() < 1e-6,
        "{large} {problems:?}"
    );
    // A name that is nothing: told.
    let (_, problems) = wide(&["middle"]);
    assert!(
        problems.iter().any(|p| p.contains("middle")),
        "{problems:?}"
    );
    Ok(())
}

#[test]
fn a_pictorial_view_cut_by_a_section_is_hatched_where_it_cuts() -> anyhow::Result<()> {
    let mut b = blocks(10.0)?;
    b.world.sections.insert(
        "mid".into(),
        Cutting::plane(Point3::new(0.0, 0.5, 0.0), Point3::Y),
    );
    let iso = |section: Option<&str>| SheetView {
        section: section.map(str::to_owned),
        ..view(
            "iso",
            ViewKind::Base {
                side: Side::Isometric,
            },
            [200.0, 150.0],
            Style::HiddenRemoved,
        )
    };
    let narrow = |laid: &Laid| laid.strokes.iter().filter(|s| s.pen == Pen::Narrow).count();
    let whole = lay(&b, &sheet(vec![iso(None)], vec![]));
    let cut = lay(&b, &sheet(vec![iso(Some("mid"))], vec![]));
    assert!(cut.problems.is_empty(), "{:?}", cut.problems);
    assert!(
        narrow(&cut) > narrow(&whole),
        "{} {}",
        narrow(&cut),
        narrow(&whole)
    );
    let missing = lay(&b, &sheet(vec![iso(Some("nowhere"))], vec![]));
    assert!(
        missing.problems.iter().any(|p| p.contains("nowhere")),
        "{:?}",
        missing.problems
    );
    Ok(())
}

#[test]
fn a_half_section_divides_at_the_axis_of_what_it_cuts_through() -> anyhow::Result<()> {
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    // The blocks, and a post 1 in radius standing on (20, 0, 0): cut
    // through the post's axis, the halves divide there, not at the
    // view's middle nor at the section's origin.
    let mut b = blocks(10.0)?;
    let far =
        workpiece_core::world::DatumFrame::new(Point3::new(20.0, 0.0, 0.0), Point3::X, Point3::Y)?;
    b.world
        .datums
        .insert("far".into(), workpiece_core::world::Datum::Frame(far));
    AddCylinder {
        id: "post".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum("far")),
            radius: Measure::new(1.0),
            height: Measure::new(4.0),
        },
    }
    .apply(&mut b)?;
    b.world.sections.insert(
        "mid".into(),
        Cutting::plane(Point3::new(0.0, 0.0, 0.0), Point3::Y),
    );
    let mut section = view(
        "s",
        ViewKind::Section {
            of: None,
            section: "mid".into(),
            letter: "A".into(),
            turn: 0.0,
        },
        [200.0, 100.0],
        Style::HiddenRemoved,
    );
    section.half = Some(Half {
        cut: HalfSide::Left,
        only: false,
    });
    let laid = lay(&b, &sheet(vec![section], vec![]));
    let v = &laid.views[0];
    let line = laid
        .strokes
        .iter()
        .find(|s| {
            s.pen == Pen::Chain && (s.a.x - s.b.x).abs() < 1e-9 && (s.a.y - 100.0).abs() < 50.0
        })
        .expect("the dividing centre line");
    // The post's axis on the sheet: its round outline's middle, 10 from
    // the block's.
    let off = (line.a.x - v.anchor.x).abs();
    assert!(off > 1.0, "{} at {}", line.a.x, v.anchor.x);
    // Where the post's two sides are: the line halfway.
    let uprights: Vec<f64> = laid
        .strokes
        .iter()
        .filter(|s| {
            s.pen == Pen::Wide && (s.a.x - s.b.x).abs() < 1e-9 && (s.a.y - s.b.y).abs() > 1.0
        })
        .map(|s| s.a.x)
        .filter(|x| (x - line.a.x).abs() < 2.0 * v.scale + 1e-6)
        .collect();
    assert!(
        uprights
            .iter()
            .any(|x| (x - line.a.x - v.scale).abs() < 1e-6)
            || uprights
                .iter()
                .any(|x| (x - line.a.x + v.scale).abs() < 1e-6),
        "{uprights:?} about {}",
        line.a.x
    );
    Ok(())
}

#[test]
fn an_assembly_s_parts_list_counts_its_parts_and_balloons_point_at_them() -> anyhow::Result<()> {
    use workpiece_ops_drawing::lay::Placing;
    use workpiece_ops_drawing::sheet::Balloon;
    // Two of the same part (the blocks), and one other (a block alone).
    let mut b = assembly()?;
    let lid = std::sync::Arc::new(
        blocks(10.0)
            .map(|mut w| {
                w.world.material = Some("Brass".into());
                w.world.instances.clear();
                w
            })?
            .world,
    );
    let mut lid_world = (*lid).clone();
    lid_world.stamp = 7;
    b.world.instances.insert(
        "lid".into(),
        workpiece_core::world::Instance {
            pose: workpiece_core::Pose::from_translation(Point3::new(0.0, 40.0, 0.0)),
            world: std::sync::Arc::new(lid_world),
            fixed: true,
        },
    );
    let top = view(
        "top",
        ViewKind::Base { side: Side::Top },
        [150.0, 150.0],
        Style::HiddenRemoved,
    );
    let names = std::collections::BTreeMap::from([("lid".to_owned(), "cover".to_owned())]);
    let densities = std::collections::BTreeMap::from([("Brass".to_owned(), 8500.0)]);
    let properties = Default::default();
    let lay_with = |sheet: &Sheet| {
        let subject = Subject {
            world: &b.world,
            unit: workpiece_core::Unit::default(),
            material: None,
            name: "assembly",
            properties: &properties,
            parts: &names,
            densities: &densities,
        };
        lay_out(sheet, &subject, &Kept::default(), None)
    };
    let mut asked = Sheet {
        parts_list: true,
        auto_balloons: true,
        ..sheet(vec![top], vec![])
    };
    let laid = lay_with(&asked);
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let said = |t: &str| laid.texts.iter().any(|x| x.text == t);
    // Items: the blocks twice (named as placed, without the number that
    // tells them apart — here their first name), the cover once, in
    // brass.
    assert!(said("Item") && said("Qty") && said("cover") && said("Brass"));
    assert!(laid.texts.iter().filter(|t| t.text == "2").count() >= 2);
    // A balloon each, outside the view, its leader ending on the part.
    let balloons: Vec<_> = laid
        .notes
        .iter()
        .filter(|n| matches!(n.kind, Placing::Balloon(_)))
        .collect();
    assert_eq!(balloons.len(), 2);
    let v = &laid.views[0];
    for n in &balloons {
        let middle = (n.rect[0] + n.rect[1]) / 2.0;
        let off = (middle - v.centre).abs();
        assert!(off.x > v.half.x || off.y > v.half.y, "{middle:?}");
        let tip = *n.tips.first().expect("its leader's end");
        let into = (tip - v.centre).abs();
        assert!(
            into.x <= v.half.x + 1e-6 && into.y <= v.half.y + 1e-6,
            "{tip:?}"
        );
    }
    // Its columns as the sheet says: with how dense each is.
    let with_density = Sheet {
        parts_columns: vec![
            workpiece_ops_drawing::sheet::PartsColumn::Item,
            workpiece_ops_drawing::sheet::PartsColumn::Name,
            workpiece_ops_drawing::sheet::PartsColumn::Density,
        ],
        ..asked.clone()
    };
    let dense = lay_with(&with_density);
    assert!(dense.texts.iter().any(|t| t.text == "8500 kg/m³"));
    assert!(!dense.texts.iter().any(|t| t.text == "Qty"));
    // One put by hand stands where it is put.
    asked.balloons = vec![Balloon {
        part: "lid".into(),
        view: "top".into(),
        at: [-60.0, 40.0],
        to: None,
    }];
    let laid = lay_with(&asked);
    let lid = laid
        .notes
        .iter()
        .find(|n| n.kind == Placing::Balloon("lid".into()))
        .expect("the lid's balloon");
    assert_eq!(lid.index, 0);
    let middle = (lid.rect[0] + lid.rect[1]) / 2.0;
    assert!((middle - laid.views[0].centre - Point2::new(-60.0, 40.0)).length() < 1e-9);
    Ok(())
}

#[test]
fn lettering_is_as_high_as_the_sheet_says() -> anyhow::Result<()> {
    // A height of the large block, on a sheet lettered 5 mm high: its
    // value 5 high, a section's name the next size up, 7.
    let mut b = blocks(10.0)?;
    say(
        &mut b,
        "high",
        length([0.0, 2.0, -2.0], [0.0, 2.0, 2.0]),
        4.0,
    );
    b.world.sections.insert(
        "mid".into(),
        Cutting::plane(Point3::new(0.0, 0.5, 0.0), Point3::Y),
    );
    let section = view(
        "s",
        ViewKind::Section {
            of: None,
            section: "mid".into(),
            letter: "A".into(),
            turn: 0.0,
        },
        [300.0, 100.0],
        Style::HiddenRemoved,
    );
    for (letter, title) in [(3.5, 5.0), (5.0, 7.0)] {
        let mut asked = sheet(
            vec![front(Style::HiddenRemoved), section.clone()],
            vec![note("high", "front", [30.0, 0.0])],
        );
        asked.drafting.letter = letter;
        let laid = lay(&b, &asked);
        let height = |t: &str| {
            laid.texts
                .iter()
                .rev()
                .find(|x| x.text == t)
                .map(|x| x.height)
        };
        assert_eq!(height("4"), Some(letter));
        assert_eq!(height("A–A"), Some(title));
    }
    Ok(())
}

#[test]
fn a_hole_s_count_is_as_the_sheet_says_and_a_reference_depth_only_where_asked() -> anyhow::Result<()>
{
    use workpiece_core::world::Callout;
    // Two counterbored holes alike (the model's count), the counterbore's
    // floor up to a face: its depth a reference, in brackets.
    let mut b = blocks(10.0)?;
    let lines = vec![
        vec![
            Callout::Count(2),
            Callout::Text("Ø".into()),
            Callout::Number(1.0),
        ],
        vec![
            Callout::Counterbore,
            Callout::Text("Ø".into()),
            Callout::Number(2.0),
            Callout::Text(" ".into()),
            Callout::Depth,
            Callout::Text("(".into()),
            Callout::Number(0.5),
            Callout::Text(")".into()),
        ],
    ];
    b.world.annotations.insert(
        "bore".into(),
        Annotation {
            note: Note::Hole {
                centre: Point3::new(2.0, 0.0, 0.0),
                axis: -Point3::X,
                radius: 0.5,
                lines,
                outside: false,
            },
            at: Point3::new(2.0, 3.0, 3.0),
            sketch: None,
        },
    );
    let written = |count: Option<u32>, places: bool, references: bool| {
        let mut asked = sheet(
            vec![front(Style::HiddenRemoved)],
            vec![note("bore", "front", [30.0, 30.0])],
        );
        asked.notes[0].count = count;
        asked.places = places;
        asked.reference_depths = references;
        let laid = lay(&b, &asked);
        laid.texts
            .iter()
            .map(|t| t.text.clone())
            .collect::<Vec<_>>()
            .join("|")
    };
    // As the model says, ISO: `2× ` before it; no reference depth.
    let said = written(None, false, false);
    assert!(said.contains("2× "), "{said}");
    assert!(!said.contains('('), "{said}");
    // The count said on the sheet; or none.
    assert!(written(Some(6), false, false).contains("6× "));
    let none = written(Some(0), false, false);
    assert!(!none.contains('×'), "{none}");
    // As `(6) PLACES` after it, on a sheet that writes it so.
    let places = written(Some(6), true, false);
    assert!(
        places.contains("(6) PLACES") && !places.contains('×'),
        "{places}"
    );
    // The reference depth where the sheet asks for it.
    let reference = written(None, false, true);
    assert!(
        reference.contains('(') && reference.contains(')'),
        "{reference}"
    );
    Ok(())
}

#[test]
fn a_hole_through_says_thru_and_words_of_its_callout_stand_under_it() -> anyhow::Result<()> {
    use workpiece_core::world::Callout;
    // A counterbored hole into the large block's front face, through:
    // its callout two lines, the bore's without a depth.
    let mut b = blocks(10.0)?;
    let lines = vec![
        vec![Callout::Text("Ø".into()), Callout::Number(1.0)],
        vec![
            Callout::Counterbore,
            Callout::Text("Ø".into()),
            Callout::Number(2.0),
            Callout::Text(" ".into()),
            Callout::Depth,
            Callout::Number(0.5),
        ],
    ];
    b.world.annotations.insert(
        "bore".into(),
        Annotation {
            note: Note::Hole {
                centre: Point3::new(2.0, 0.0, 0.0),
                axis: -Point3::X,
                radius: 0.5,
                lines,
                outside: false,
            },
            at: Point3::new(2.0, 3.0, 3.0),
            sketch: None,
        },
    );
    let words = Words {
        view: None,
        at: [0.0, 0.0],
        dimension: Some("bore".into()),
        text: "4 PLACES".into(),
        height: 3.5,
        to: Vec::new(),
    };
    let mut asked = sheet(
        vec![front(Style::HiddenRemoved)],
        vec![note("bore", "front", [30.0, 30.0])],
    );
    asked.words = vec![words];
    let plain = lay(&b, &asked);
    assert!(!plain.texts.iter().any(|t| t.text.contains("THRU")));
    asked.thru = true;
    let laid = lay(&b, &asked);
    assert!(
        laid.texts.iter().any(|t| t.text.contains("THRU")),
        "{:?}",
        laid.texts
    );
    // The words under the counterbore's line, over none of it.
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let y = |t: &str| {
        laid.texts
            .iter()
            .filter(|x| x.text.contains(t))
            .map(|x| x.at.y)
            .fold(f64::INFINITY, f64::min)
    };
    // Each piece of the callout's line set on past the one before, as
    // wide as Helvetica (what the PDF and the SVG set it in) has it: Ø
    // is 0.778 of the font's size, a digit 0.556 (the capitals are
    // 0.718 of it high).
    let size = 3.5 / 0.718;
    let runs: Vec<_> = laid
        .texts
        .iter()
        .filter(|t| t.anchor == [0.0, 0.0] && (t.text == "Ø" || t.text == "1" || t.text == " THRU"))
        .collect();
    let at = |t: &str| runs.iter().find(|r| r.text == t).map(|r| r.at.x).expect(t);
    assert!((at("1") - at("Ø") - 0.778 * size).abs() < 1e-9, "{runs:?}");
    assert!(
        (at(" THRU") - at("1") - 0.556 * size).abs() < 1e-9,
        "{runs:?}"
    );
    // (Where the words stand — after the last line or under it — is
    // the words' layout's; on none of the callout, as told above.)
    let _ = y;
    Ok(())
}

#[test]
fn an_angle_says_its_own_value_wherever_its_text_is_put() -> anyhow::Result<()> {
    // Two faces 16° apart that meet far off the part (to the left of
    // it, seen from the front), measured as 16°: its text says 16°
    // between them and in the sector beside them alike (ISO 129-1: the
    // value is the dimension's; where the text is put moves it along
    // the arc, run on past an arm, never to the other angle).
    let mut b = blocks(10.0)?;
    let (s, c) = 8f64.to_radians().sin_cos();
    let measured = Measured::Angle {
        at: Point3::new(0.0, -40.0, 0.0),
        first: Point3::new(0.0, c, s),
        second: Point3::new(0.0, c, -s),
    };
    say(&mut b, "taper", measured, 16.0);
    let laid_at = |b: &Standalone, at: [f64; 2]| {
        lay(
            b,
            &sheet(
                vec![front(Style::HiddenRemoved)],
                vec![note("taper", "front", at)],
            ),
        )
    };
    let text = |b: &Standalone, at: [f64; 2]| {
        laid_at(b, at)
            .texts
            .iter()
            .filter(|t| t.text.ends_with('°'))
            .map(|t| t.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(text(&b, [20.0, 0.0]), ["16°"]);
    assert_eq!(text(&b, [10.0, 30.0]), ["16°"]);
    assert_eq!(text(&b, [10.0, -30.0]), ["16°"]);
    // Put above the sector, its text stands where it is put (on the
    // arc run on past the upper arm), not on the arc between the arms.
    let laid = laid_at(&b, [10.0, 30.0]);
    let t = laid
        .texts
        .iter()
        .find(|t| t.text == "16°")
        .expect("its text");
    let centre = laid.notes[0].rect;
    let middle = (centre[0] + centre[1]) / 2.0;
    let front = laid.notes[0].view.clone();
    assert_eq!(front, "front");
    assert!(t.at.y > middle.y - 5.0, "{:?}", t.at);
    // Measured the other way (`supplement`: the first face's direction
    // turned, 164°): 164° wherever its text is.
    let measured = Measured::Angle {
        at: Point3::new(0.0, -40.0, 0.0),
        first: Point3::new(0.0, -c, -s),
        second: Point3::new(0.0, c, -s),
    };
    say(&mut b, "taper", measured, 164.0);
    assert_eq!(text(&b, [20.0, 0.0]), ["164°"]);
    assert_eq!(text(&b, [10.0, 30.0]), ["164°"]);
    Ok(())
}

#[test]
fn a_small_angle_s_text_stands_off_its_arc_past_an_arm() -> anyhow::Result<()> {
    // A 4° angle with its apex in the front view's middle, its text put
    // on the arc between its arms, 15 out: the arc is too short for it
    // (about 1 mm), so it stands past an arm, clear of both arms.
    let mut b = blocks(10.0)?;
    let (s, c) = 2f64.to_radians().sin_cos();
    let measured = Measured::Angle {
        at: Point3::new(0.0, 0.0, 0.0),
        first: Point3::new(0.0, c, s),
        second: Point3::new(0.0, c, -s),
    };
    say(&mut b, "small", measured, 4.0);
    let laid = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenRemoved)],
            vec![note("small", "front", [15.0, 0.0])],
        ),
    );
    let rect = laid.notes[0].rect;
    // (Its arrows: tips on the arms, about 1 mm apart, pointing in from
    // outside.)
    let tips: Vec<_> = laid.fills.iter().map(|f| f.corners[0]).collect();
    assert_eq!(tips.len(), 2, "{tips:?}");
    let (lo, hi) = (tips[0].y.min(tips[1].y), tips[0].y.max(tips[1].y));
    assert!(hi - lo < 2.0, "{tips:?}");
    // Its text wholly past one arm: not across the arc between them.
    assert!(rect[0].y > hi || rect[1].y < lo, "{rect:?} {tips:?}");
    // Put out past the upper arm, it stands there, further out as far as
    // it is put.
    let far = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenRemoved)],
            vec![note("small", "front", [15.0, 6.0])],
        ),
    );
    let moved = far.notes[0].rect;
    assert!(
        moved[0].y > hi && moved[0].y > rect[0].y - 1e-6,
        "{moved:?} {rect:?}"
    );
    Ok(())
}

#[test]
fn a_radius_on_a_leader_stands_level_on_a_shoulder() -> anyhow::Result<()> {
    // A radius of 2 about the small block's middle, seen square on from
    // the front, its text out up and to the right at 45°.
    let mut b = blocks(10.0)?;
    let measured = Measured::Radius {
        centre: Point3::new(10.0, 0.0, 0.0),
        axis: Point3::X,
        radius: 2.0,
    };
    say(&mut b, "round", measured, 2.0);
    let mut asked = sheet(
        vec![front(Style::HiddenRemoved)],
        vec![note("round", "front", [30.0, 30.0])],
    );
    let laid = lay(&b, &asked);
    let text = laid
        .texts
        .iter()
        .find(|t| t.text == "R2")
        .expect("its text");
    assert!(text.angle.abs() < 1e-9, "{}", text.angle);
    // (Its shoulder: a level line under it.)
    let rect = laid.notes[0].rect;
    assert!(laid.strokes.iter().any(|s| {
        (s.a.y - s.b.y).abs() < 1e-9 && (s.a.x - s.b.x).abs() > 3.0 && s.a.y <= rect[0].y + 1e-6
    }));
    // Aligned, as the sheet may say: along the leader.
    asked.aligned_leaders = true;
    let laid = lay(&b, &asked);
    let text = laid
        .texts
        .iter()
        .find(|t| t.text == "R2")
        .expect("its text");
    assert!(
        (text.angle.abs() - std::f64::consts::FRAC_PI_4).abs() < 0.2,
        "{}",
        text.angle
    );
    Ok(())
}

#[test]
fn a_length_to_a_datum_plane_runs_on_from_its_trace() -> anyhow::Result<()> {
    // The large block's top (z = 2) to the origin's XY plane, measured
    // through the block's middle, its text to the right: the plane's
    // trace is drawn, a chain line ending a little past the outline,
    // and the extension line starts at its right end, not inside the
    // block.
    let mut b = blocks(10.0)?;
    say(
        &mut b,
        "high",
        length([0.0, 0.5, 2.0], [0.0, 0.5, 0.0]),
        2.0,
    );
    let laid = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenRemoved)],
            vec![note("high", "front", [60.0, 0.0])],
        ),
    );
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let v = &laid.views[0];
    let linear = laid.notes[0].linear.expect("a length");
    // (The view's own lines: where its right outline is.)
    let right = v
        .lines
        .iter()
        .map(|(a, b)| a.x.max(b.x))
        .fold(f64::MIN, f64::max);
    let start = linear
        .from
        .iter()
        .map(|p| p.x)
        .fold(f64::INFINITY, f64::min);
    assert!(start > right - 1e-6, "{start} within {right}");
    let trace = laid
        .strokes
        .iter()
        .find(|s| {
            s.pen == Pen::Chain
                && (s.a.y - s.b.y).abs() < 1e-9
                && (s.a.y - linear.from[1].y).abs() < 0.1
        })
        .expect("the trace");
    let left = v
        .lines
        .iter()
        .map(|(a, b)| a.x.min(b.x))
        .fold(f64::MAX, f64::min);
    // (A little past the outline both ways: not across the whole view.)
    assert!(trace.a.x.min(trace.b.x) < left && trace.a.x.min(trace.b.x) > left - 10.0);
    Ok(())
}

#[test]
fn a_radius_of_a_round_the_view_does_not_draw_points_at_its_outline() -> anyhow::Result<()> {
    // A radius of 1/2 about a point 0.4 under the large block's top,
    // seen from the front, where no such circle is drawn: its arrow on
    // the top edge, no circle drawn for it.
    let mut b = blocks(10.0)?;
    let measured = Measured::Radius {
        centre: Point3::new(0.0, 0.0, 1.6),
        axis: Point3::X,
        radius: 0.5,
    };
    say(&mut b, "round", measured, 0.5);
    let laid = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenRemoved)],
            vec![note("round", "front", [20.0, 30.0])],
        ),
    );
    assert!(
        !laid
            .strokes
            .iter()
            .any(|s| s.pen == Pen::Chain && (s.a.y - 100.0).abs() < 30.0)
    );
    let v = &laid.views[0];
    let top = v
        .lines
        .iter()
        .map(|(a, b)| a.y.max(b.y))
        .fold(f64::MIN, f64::max);
    // (The arrow: a filled triangle with a corner on the top edge.)
    assert!(
        laid.fills
            .iter()
            .any(|f| f.corners.iter().any(|p| (p.y - top).abs() < 1e-6))
    );
    Ok(())
}

#[test]
fn words_can_have_several_leaders() -> anyhow::Result<()> {
    use workpiece_ops_drawing::sheet::Target;
    let b = blocks(10.0)?;
    let words: Words = serde_json::from_value(serde_json::json!({
        "view": "front", "at": [40.0, 30.0], "text": "NOT TANGENT",
        "to": [[0.0, -2.0, 2.0], [0.0, 2.0, 2.0]]
    }))?;
    assert_eq!(words.to.len(), 2);
    // (One place, as sheets said it before, is one leader.)
    let one: Words = serde_json::from_value(serde_json::json!({
        "view": "front", "at": [40.0, 30.0], "text": "X", "to": [0.0, 2.0, 2.0]
    }))?;
    assert_eq!(one.to, vec![Target::Model([0.0, 2.0, 2.0])]);
    let mut asked = sheet(vec![front(Style::HiddenRemoved)], vec![]);
    asked.words = vec![words];
    let laid = lay(&b, &asked);
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let n = laid
        .notes
        .iter()
        .find(|n| n.kind == workpiece_ops_drawing::lay::Placing::Words)
        .expect("the words");
    assert_eq!(n.tips.len(), 2);
    assert!(n.tips[0].distance(n.tips[1]) > 1.0);
    Ok(())
}

#[test]
fn words_of_several_lines_stand_one_under_the_other() -> anyhow::Result<()> {
    let b = blocks(10.0)?;
    let mut asked = sheet(vec![front(Style::HiddenRemoved)], vec![]);
    asked.words = vec![Words {
        view: Some("front".into()),
        at: [40.0, 30.0],
        dimension: None,
        text: "ALL WIRES\nCUT TO MATCH".into(),
        height: 3.5,
        to: vec![workpiece_ops_drawing::sheet::Target::Model([0.0, 2.0, 2.0])],
    }];
    let laid = lay(&b, &asked);
    let row = |t: &str| laid.texts.iter().find(|x| x.text == t).expect(t);
    let (first, second) = (row("ALL WIRES"), row("CUT TO MATCH"));
    // Left-aligned, the second under the first; nothing written as "?".
    assert!((first.at.x - second.at.x).abs() < 1e-9 && second.at.y < first.at.y - 3.5);
    assert!(!laid.texts.iter().any(|t| t.text.contains('\n')));
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    Ok(())
}

#[test]
fn a_section_along_a_sketched_line_cuts_as_far_as_the_line_goes() -> anyhow::Result<()> {
    // The large block (4 across), cut along a line sketched on the top
    // plane from its middle out past its side: only from the middle on
    // — half of it — is cut; drawn as its cut only, half as wide as a
    // cut all across.
    let mut b = blocks(10.0)?;
    let line = |bounded| Cutting {
        origin: Point3::ZERO,
        x: Point3::X,
        y: Point3::Y,
        path: vec![
            workpiece_core::math::Point2::new(0.0, 0.0),
            workpiece_core::math::Point2::new(0.0, 3.0),
        ],
        bounded,
    };
    let wide = |b: &mut Standalone, bounded| {
        b.world.sections.insert("radial".into(), line(bounded));
        let section = SheetView {
            cut_only: true,
            bodies: vec!["large".into()],
            ..view(
                "s",
                ViewKind::Section {
                    of: None,
                    section: "radial".into(),
                    letter: "A".into(),
                    turn: 0.0,
                },
                [200.0, 100.0],
                Style::HiddenRemoved,
            )
        };
        let laid = lay(b, &sheet(vec![section], vec![]));
        assert!(laid.problems.is_empty(), "{:?}", laid.problems);
        laid.views[0].half.x / laid.views[0].scale
    };
    let all = wide(&mut b, false);
    let half = wide(&mut b, true);
    assert!((all - 2.0).abs() < 1e-6, "{all}");
    assert!((half - 1.0).abs() < 1e-6, "{half}");
    Ok(())
}

#[test]
fn a_placed_part_s_own_dimension_stands_on_the_assembly_s_sheet() -> anyhow::Result<()> {
    // The blocks, said a height of, placed twice: `right/high` is the
    // height of the part placed as `right`, where it is placed (20 on
    // along y).
    let mut part = blocks(10.0)?;
    say(
        &mut part,
        "high",
        length([0.0, 2.0, -2.0], [0.0, 2.0, 2.0]),
        4.0,
    );
    let part = std::sync::Arc::new(part.world);
    let mut b = Standalone::new();
    for (name, y) in [("left", 0.0), ("right", 20.0)] {
        b.world.instances.insert(
            name.into(),
            workpiece_core::world::Instance {
                pose: workpiece_core::Pose::from_translation(Point3::new(0.0, y, 0.0)),
                world: part.clone(),
                fixed: true,
            },
        );
    }
    let said = b
        .world
        .said("right/high")
        .expect("said of the part placed as right");
    let Note::Dimension {
        measured: Measured::Length { from, .. },
        ..
    } = &said.note
    else {
        panic!("a length");
    };
    assert!((from.y - 22.0).abs() < 1e-9, "{from}");
    let laid = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenRemoved)],
            vec![note("right/high", "front", [40.0, 0.0])],
        ),
    );
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    assert!(
        laid.notes
            .iter()
            .any(|n| n.kind == workpiece_ops_drawing::lay::Placing::Note && n.linear.is_some())
    );
    Ok(())
}

#[test]
fn in_a_detail_what_a_dimension_ends_on_stays_within_its_circle() -> anyhow::Result<()> {
    // The large block's top to the origin's XY plane, measured through
    // a point far off the detail, shown in a detail of the top's right
    // corner at 5:1: the plane's trace, drawn for it, is within the
    // detail's circle.
    let mut b = blocks(10.0)?;
    say(
        &mut b,
        "high",
        length([0.0, 1.5, 2.0], [0.0, 1.5, 0.0]),
        2.0,
    );
    let detail = SheetView {
        scale: Some([5, 1]),
        ..view(
            "detail",
            ViewKind::Detail {
                of: "front".into(),
                centre: [1.5, 0.5],
                radius: 1.0,
                letter: "B".into(),
                letter_at: None,
            },
            [300.0, 150.0],
            Style::HiddenRemoved,
        )
    };
    let laid = lay(
        &b,
        &sheet(
            vec![front(Style::HiddenRemoved), detail],
            vec![note("high", "detail", [20.0, 0.0])],
        ),
    );
    let (c, r) = (Point2::new(300.0, 150.0), 5.0 + 1e-6);
    for s in laid.strokes.iter().filter(|s| s.pen == Pen::Chain) {
        let near_detail = s.a.distance(c) < 40.0 || s.b.distance(c) < 40.0;
        if near_detail {
            assert!(s.a.distance(c) <= r && s.b.distance(c) <= r, "{s:?}");
        }
    }
    Ok(())
}

#[test]
fn a_half_section_s_cutting_line_ends_at_the_axis() -> anyhow::Result<()> {
    // The blocks cut square to y, its line shown in the view from
    // above: of a half section, the line runs from outside only to
    // where the halves divide, with its letter at its outer end only.
    let mut b = blocks(10.0)?;
    b.world.sections.insert(
        "mid".into(),
        Cutting::plane(Point3::new(0.0, 0.5, 0.0), Point3::Y),
    );
    let top = view(
        "top",
        ViewKind::Base { side: Side::Top },
        [100.0, 200.0],
        Style::HiddenRemoved,
    );
    let section = |half| SheetView {
        half,
        ..view(
            "s",
            ViewKind::Section {
                of: Some("top".into()),
                section: "mid".into(),
                letter: "A".into(),
                turn: 0.0,
            },
            [100.0, 80.0],
            Style::HiddenRemoved,
        )
    };
    let line = |laid: &Laid| -> f64 {
        laid.strokes
            .iter()
            .filter(|s| s.pen == Pen::Chain && (s.a.y - 200.0).abs() < 40.0)
            .map(|s| s.a.distance(s.b))
            .fold(0.0, f64::max)
    };
    let whole = lay(&b, &sheet(vec![top.clone(), section(None)], vec![]));
    let half = lay(
        &b,
        &sheet(
            vec![
                top,
                section(Some(Half {
                    cut: HalfSide::Left,
                    only: false,
                })),
            ],
            vec![],
        ),
    );
    assert!(
        line(&half) < line(&whole) * 0.8,
        "{} {}",
        line(&half),
        line(&whole)
    );
    let letters = |laid: &Laid| laid.texts.iter().filter(|t| t.text == "A").count();
    // (Besides the frame's own A row marks.)
    assert_eq!(letters(&whole) - letters(&half), 1);
    Ok(())
}

#[test]
fn two_lengths_on_one_line_whose_texts_cross_over_are_told() -> anyhow::Result<()> {
    // The two ends of the large block, 1 wide each, on one line above
    // it: each text put out past the other's arrows (the left one's on
    // the right, the right one's on the left) runs its line across the
    // other's — read as one line with no value. Put each beside its own
    // arrows on its outer side, nothing is said.
    let mut b = blocks(10.0)?;
    say(
        &mut b,
        "left",
        length([0.0, -2.0, 2.0], [0.0, -1.0, 2.0]),
        1.0,
    );
    say(
        &mut b,
        "right",
        length([0.0, 1.0, 2.0], [0.0, 2.0, 2.0]),
        1.0,
    );
    let told = |left: f64, right: f64| {
        let laid = lay(
            &b,
            &sheet(
                vec![front(Style::HiddenRemoved)],
                vec![
                    note("left", "front", [left, 20.0]),
                    note("right", "front", [right, 20.0]),
                ],
            ),
        );
        laid.problems
            .iter()
            .any(|p| p.contains("run into each other"))
    };
    assert!(told(30.0, -30.0));
    assert!(!told(-30.0, 30.0));
    Ok(())
}

#[test]
fn title_blocks_say_the_title_the_kind_and_the_mass_of_what_is_placed_too() -> anyhow::Result<()> {
    let cell = |laid: &Laid, label: &str| {
        let cell = laid.cells.iter().find(|c| c.label == label).expect("it");
        cell.value.clone()
    };
    let densities = std::collections::BTreeMap::from([("Steel".to_owned(), 7850.0)]);
    let lay_with = |b: &Standalone, properties: &std::collections::BTreeMap<String, String>| {
        let subject = Subject {
            world: &b.world,
            unit: workpiece_core::Unit::default(),
            material: b.world.material.clone(),
            name: "blocks",
            properties,
            parts: &Default::default(),
            densities: &densities,
        };
        lay_out(&sheet(vec![], vec![]), &subject, &Kept::default(), None)
    };
    // A part of steel: 72 mm³ of it, 0.57 g; titled by its name.
    let mut part = blocks(10.0)?;
    part.world.material = Some("Steel".into());
    let laid = lay_with(&part, &Default::default());
    assert_eq!(cell(&laid, "Mass"), "0.6 g");
    assert_eq!(cell(&laid, "Title"), "blocks");
    assert_eq!(cell(&laid, "Material"), "Steel");
    assert_eq!(cell(&laid, "Document type"), "Part drawing");
    // Its own title over its name.
    let titled = std::collections::BTreeMap::from([("Title".to_owned(), "TWO BLOCKS".to_owned())]);
    assert_eq!(cell(&lay_with(&part, &titled), "Title"), "TWO BLOCKS");
    // Placed twice: an assembly that weighs both; with a third part of
    // nothing said, at least that.
    let parts = std::sync::Arc::new(part.world);
    let mut b = Standalone::new();
    for (name, world) in [("left", parts.clone()), ("right", parts)] {
        b.world.instances.insert(
            name.into(),
            workpiece_core::world::Instance {
                pose: workpiece_core::Pose::IDENTITY,
                world,
                fixed: true,
            },
        );
    }
    let laid = lay_with(&b, &Default::default());
    assert_eq!(cell(&laid, "Mass"), "1.1 g");
    assert_eq!(cell(&laid, "Document type"), "Assembly drawing");
    b.world.instances.insert(
        "unknown".into(),
        workpiece_core::world::Instance {
            pose: workpiece_core::Pose::IDENTITY,
            world: std::sync::Arc::new(blocks(10.0)?.world),
            fixed: true,
        },
    );
    assert_eq!(cell(&lay_with(&b, &Default::default()), "Mass"), "≥ 1.1 g");
    Ok(())
}

#[test]
fn a_section_along_a_pin_leaves_it_whole_and_catalogue_parts_are_one_item() -> anyhow::Result<()> {
    use workpiece_core::world::{Instance, Sectioned};
    // Two pins of one designation (blocks standing for them), drawn whole
    // where a cut runs along their z; a plain part beside them.
    let pin = |sectioned| {
        let mut w = blocks(10.0).expect("blocks").world;
        w.designation = Some("Pin ISO 2338 4×12".into());
        w.sectioned = sectioned;
        std::sync::Arc::new(w)
    };
    let mut b = Standalone::new();
    for (name, world) in [
        ("pin1", pin(Sectioned::WholeAlong(Point3::Z))),
        ("pin2", pin(Sectioned::WholeAlong(Point3::Z))),
        ("plate", pin(Sectioned::Cut)),
    ] {
        b.world.instances.insert(
            name.into(),
            Instance {
                pose: workpiece_core::Pose::IDENTITY,
                world,
                fixed: true,
            },
        );
    }
    b.world.instances.get_mut("plate").expect("it").world =
        std::sync::Arc::new(blocks(10.0)?.world);
    let volume = |w: &workpiece_core::World| -> f64 {
        let topo = &w.topo;
        topo.bodies()
            .ids()
            .filter_map(|id| workpiece_boolean::read(topo, id).ok())
            .map(|s| workpiece_boolean::measure(&s).volume)
            .sum()
    };
    let whole = volume(&b.world.instances["pin1"].world);
    // Along z (the plane y = 0.5): the pins whole, the plate cut.
    let along = Cutting::plane(Point3::new(0.0, 0.5, 0.0), Point3::Y);
    let cut = workpiece_shape::cut::cut_left_of(
        &b.world,
        along.origin,
        along.x,
        along.y,
        &along.path,
        100.0,
    );
    assert!((volume(&cut.instances["pin1"].world) - whole).abs() < 1e-9);
    assert!(volume(&cut.instances["plate"].world) < whole - 1.0);
    // Across z: cut like anything else.
    let across = Cutting::plane(Point3::new(0.0, 0.0, 0.5), Point3::Z);
    let cut = workpiece_shape::cut::cut_left_of(
        &b.world,
        across.origin,
        across.x,
        across.y,
        &across.path,
        100.0,
    );
    assert!(volume(&cut.instances["pin1"].world) < whole - 1.0);
    // The parts list: the pins one item of two, by their designation.
    let items = workpiece_ops_drawing::lay::parts::items(&b.world, &Default::default());
    assert_eq!(items.len(), 2, "{items:?}");
    let pins = items
        .iter()
        .find(|i| i.placed.len() == 2)
        .expect("the pins");
    assert_eq!(pins.name, "Pin ISO 2338 4×12");
    Ok(())
}
