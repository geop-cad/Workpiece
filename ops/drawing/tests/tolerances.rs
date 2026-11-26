//! Tolerances on a sheet (ISO 129-1, ISO 1101, ISO 5459): each kind's
//! text, a tolerance frame with its datum, the general tolerance as the
//! sheet says; and a leader kept out of another view, hatching broken
//! where text stands on it.

use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::{
    Annotation, Callout, Characteristic, Cutting, Measured, Note, Numbers, Tolerance,
};
use workpiece_ops_drawing::Sheet;
use workpiece_ops_drawing::lay::{Kept, Laid, Pen, Subject, Wanted, lay_out, place_all};
use workpiece_ops_drawing::sheet::{PlacedNote, SheetView, Side, Style, ViewKind};
use workpiece_ops_testing::*;

/// A block 4 × 4 × 4 about the origin.
fn block() -> anyhow::Result<Standalone> {
    let mut b = Standalone::new();
    AddBox {
        id: "block".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [4.0; 3].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    Ok(b)
}

fn say(b: &mut Standalone, name: &str, note: Note) {
    b.world.annotations.insert(
        name.into(),
        Annotation {
            note,
            at: Point3::ZERO,
            sketch: None,
        },
    );
}

/// The block's width along y (as the front view sees it), at its top.
fn length(tolerance: Tolerance) -> Note {
    along(Point3::Y, tolerance)
}

/// The block's width along `way` (x or y), at its top.
fn along(way: Point3, tolerance: Tolerance) -> Note {
    Note::Dimension {
        measured: Measured::Length {
            from: Point3::new(0.0, 0.0, 2.0) - way * 2.0,
            to: Point3::new(0.0, 0.0, 2.0) + way * 2.0,
            along: None,
        },
        value: 4.0,
        tolerance,
        elsewhere: Vec::new(),
    }
}

fn view(id: &str, kind: ViewKind, at: [f64; 2]) -> SheetView {
    SheetView {
        id: id.into(),
        kind,
        at,
        scale: None,
        style: Style::HiddenRemoved,
        bodies: Vec::new(),
        cut_only: false,
        half: None,
        section: None,
        caption_at: None,
    }
}

fn front(at: [f64; 2]) -> SheetView {
    view("front", ViewKind::Base { side: Side::Front }, at)
}

fn note(note: &str, view: &str, at: [f64; 2]) -> PlacedNote {
    PlacedNote {
        note: note.into(),
        view: view.into(),
        at,
        decimals: None,
        starts: [None; 2],
        count: None,
        to: None,
    }
}

fn subject(b: &Standalone) -> Subject<'_> {
    Subject {
        world: &b.world,
        unit: workpiece_core::Unit::default(),
        material: None,
        name: "block",
        properties: Box::leak(Box::default()),
        parts: Box::leak(Box::default()),
        densities: Box::leak(Box::default()),
        sheet: [1, 1],
    }
}

fn lay(b: &Standalone, sheet: &Sheet) -> Laid {
    lay_out(sheet, &subject(b), &Kept::default(), None)
}

fn sheet(views: Vec<SheetView>, notes: Vec<PlacedNote>) -> Sheet {
    Sheet {
        views,
        notes,
        scale: [5, 1],
        ..Sheet::default()
    }
}

/// The texts written from `from` on, as they read.
fn texts(laid: &Laid) -> Vec<String> {
    laid.texts.iter().map(|t| t.text.clone()).collect()
}

#[test]
fn each_kind_of_tolerance_is_written_as_iso_129_has_it() -> anyhow::Result<()> {
    let kinds = [
        ("sym", Tolerance::Symmetric(0.1), vec!["4 ±0,1"]),
        (
            "dev",
            Tolerance::Deviations {
                upper: 0.25,
                lower: -0.1,
            },
            vec!["4", "+0,25", "−0,10"],
        ),
        (
            "zero",
            Tolerance::Deviations {
                upper: 0.0,
                lower: -0.03,
            },
            vec!["4", "0", "−0,03"],
        ),
        (
            "lim",
            Tolerance::Limits {
                upper: 0.05,
                lower: 0.0,
            },
            vec!["4,05", "4,00"],
        ),
        ("fit", Tolerance::Fit("H7".into()), vec!["4 H7"]),
        ("shaft", Tolerance::Fit("k6".into()), vec!["4 k6"]),
        ("ref", Tolerance::Reference, vec!["(4)"]),
    ];
    for (name, tolerance, said) in kinds {
        let mut b = block()?;
        say(&mut b, name, length(tolerance));
        let laid = lay(
            &b,
            &sheet(
                vec![front([150.0, 150.0])],
                vec![note(name, "front", [0.0, 25.0])],
            ),
        );
        let written = texts(&laid);
        for text in said {
            assert!(
                written.iter().any(|t| t == text),
                "{name}: {text} in {written:?} {:?}",
                laid.problems
            );
        }
    }
    // A fit with its deviations, where the sheet says: H7 of 4 is
    // +0,012/0, small in brackets after the class.
    let mut b = block()?;
    say(&mut b, "fit", length(Tolerance::Fit("H7".into())));
    let mut with = sheet(
        vec![front([150.0, 150.0])],
        vec![note("fit", "front", [0.0, 25.0])],
    );
    with.numbers = Numbers {
        fit_deviations: true,
        ..Numbers::default()
    };
    let written = texts(&lay(&b, &with));
    for text in ["4 H7 (", "+0,012", "0", ")"] {
        assert!(written.iter().any(|t| t == text), "{text} in {written:?}");
    }
    // An angle's ± with its unit.
    let mut b = block()?;
    say(
        &mut b,
        "angle",
        Note::Dimension {
            measured: Measured::Angle {
                at: Point3::new(-2.0, -2.0, -2.0),
                first: Point3::Y,
                second: Point3::Z,
            },
            value: 90.0,
            tolerance: Tolerance::Symmetric(0.5),
            elsewhere: Vec::new(),
        },
    );
    let laid = lay(
        &b,
        &sheet(
            vec![front([150.0, 150.0])],
            vec![note("angle", "front", [15.0, 15.0])],
        ),
    );
    assert!(
        texts(&laid).iter().any(|t| t == "90° ±0,5°"),
        "{:?}",
        texts(&laid)
    );
    Ok(())
}

#[test]
fn a_tolerance_frame_has_its_compartments_and_its_datum_a_triangle() -> anyhow::Result<()> {
    let mut b = block()?;
    let top = Point3::new(0.0, -1.0, 2.0);
    say(
        &mut b,
        "datum",
        Note::Datum {
            letter: "A".into(),
            on: Point3::new(0.0, -1.0, -2.0),
            out: -Point3::Z,
            axis: None,
        },
    );
    say(
        &mut b,
        "parallel",
        Note::Geometric {
            characteristic: Characteristic::Parallelism,
            value: 0.05,
            diametral: false,
            datums: vec!["A".into()],
            on: top,
            out: Point3::Z,
            axis: None,
        },
    );
    let laid = lay(
        &b,
        &sheet(
            vec![front([150.0, 150.0])],
            vec![
                note("datum", "front", [-20.0, -25.0]),
                note("parallel", "front", [25.0, 25.0]),
            ],
        ),
    );
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let written = texts(&laid);
    assert!(written.iter().any(|t| t == "0,05"), "{written:?}");
    // (The datum's letter twice: in its square, in the frame.)
    // (And twice on the frame's border, as a zone's.)
    assert_eq!(
        written.iter().filter(|t| *t == "A").count(),
        4,
        "{written:?}"
    );
    // The frame: three compartments side by side, of one height.
    let frame = laid.notes[1].rect;
    let walls = laid
        .strokes
        .iter()
        .filter(|s| {
            (s.a.x - s.b.x).abs() < 1e-9
                && (s.a.x - frame[0].x) > -1e-6
                && (s.a.x - frame[1].x) < 1e-6
                && (s.a.y.min(s.b.y) - frame[0].y).abs() < 1e-6
                && (s.a.y.max(s.b.y) - frame[1].y).abs() < 1e-6
        })
        .count();
    assert_eq!(walls, 4, "the frame's two ends and two walls between");
    // Its leader leaves the frame at an end, half way up (ISO 1101),
    // and ends in an arrow on the top outline.
    let middle = (frame[0].y + frame[1].y) / 2.0;
    assert!(
        laid.strokes.iter().any(|s| s.pen == Pen::Narrow
            && [s.a, s.b].iter().any(|p| (p.y - middle).abs() < 1e-6
                && ((p.x - frame[0].x).abs() < 1e-6 || (p.x - frame[1].x).abs() < 1e-6))),
        "a leader from an end of the frame"
    );
    // The datum: a filled triangle on the bottom outline (y 140 on the
    // sheet: 150 less 2 × 5), its apex down.
    let triangle = laid.fills.iter().any(|f| {
        f.corners
            .iter()
            .filter(|c| (c.y - 140.0).abs() < 1e-6)
            .count()
            == 2
            && f.corners.iter().any(|c| c.y < 140.0 - 1.0)
    });
    assert!(triangle, "{:?}", laid.fills);
    Ok(())
}

#[test]
fn the_general_tolerance_is_the_sheets_where_a_size_has_none_of_its_own() -> anyhow::Result<()> {
    let mut b = block()?;
    say(&mut b, "fit", length(Tolerance::Fit("h7".into())));
    say(&mut b, "free", along(Point3::X, Tolerance::General));
    let cell = |b: &Standalone, s: &Sheet| {
        lay(b, s)
            .cells
            .iter()
            .find(|c| c.label == "General tolerances")
            .map(|c| c.value.clone())
    };
    // (No size on the sheet, as an assembly's with balloons only; or
    // each with a tolerance of its own: none said.)
    let mut s = sheet(vec![front([150.0, 150.0])], vec![]);
    assert_eq!(cell(&b, &s).as_deref(), Some(""));
    s.notes.push(note("fit", "front", [0.0, 20.0]));
    assert_eq!(cell(&b, &s).as_deref(), Some(""));
    let mut top = front([150.0, 60.0]);
    top.id = "top".into();
    top.kind = ViewKind::Base { side: Side::Top };
    s.views.push(top);
    s.notes.push(note("free", "top", [0.0, 20.0]));
    assert_eq!(cell(&b, &s).as_deref(), Some("ISO 2768-mK"));
    s.general_tolerance = Some("ISO 8062-3 DCTG 9".into());
    assert_eq!(cell(&b, &s).as_deref(), Some("ISO 8062-3 DCTG 9"));
    s.general_tolerance = None;
    assert_eq!(cell(&b, &s).as_deref(), Some(""));
    Ok(())
}

#[test]
fn a_leader_is_kept_out_of_a_neighbouring_view() -> anyhow::Result<()> {
    // A hole's callout in the front view, with a view close to the
    // right and above it: put by itself, its leader keeps off that view;
    // put by hand across it, that is told.
    let mut b = block()?;
    say(
        &mut b,
        "bore",
        Note::Hole {
            centre: Point3::new(1.0, -2.0, 1.0),
            axis: Point3::Y,
            radius: 0.5,
            lines: vec![vec![Callout::Text("Ø".into()), Callout::Number(1.0)]],
            outside: false,
            faces: Vec::new(),
        },
    );
    let views = vec![
        front([150.0, 150.0]),
        view("other", ViewKind::Base { side: Side::Top }, [185.0, 185.0]),
    ];
    let mut s = sheet(views, vec![]);
    let wanted = [Wanted {
        note: "bore".into(),
        view: Some("front".into()),
        at: None,
    }];
    let (_, at, _) = place_all(&s, &subject(&b), &Kept::default(), &wanted)[0]
        .clone()
        .expect("placed");
    s.notes = vec![note("bore", "front", at)];
    let laid = lay(&b, &s);
    assert!(
        !laid.problems.iter().any(|p| p.contains("runs across")),
        "{:?}",
        laid.problems
    );
    s.notes = vec![note("bore", "front", [40.0, 40.0])];
    let laid = lay(&b, &s);
    assert!(
        laid.problems
            .iter()
            .any(|p| p.contains("runs across the view other")),
        "{:?}",
        laid.problems
    );
    Ok(())
}

#[test]
fn hatching_is_broken_where_text_stands_on_it() -> anyhow::Result<()> {
    // A section through the block, its width's text put in the middle
    // of the cut face: no hatching line runs through the text.
    let mut b = block()?;
    b.world.sections.insert(
        "mid".into(),
        Cutting::plane(Point3::new(0.0, 0.0, 0.0), Point3::Y),
    );
    say(&mut b, "wide", along(Point3::X, Tolerance::General));
    let section = view(
        "s",
        ViewKind::Section {
            of: None,
            section: "mid".into(),
            letter: "A".into(),
            turn: 0.0,
        },
        [150.0, 150.0],
    );
    let laid = lay(
        &b,
        &sheet(vec![section], vec![note("wide", "s", [0.0, 0.0])]),
    );
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let text = laid.texts.last().expect("the width's text");
    assert_eq!(text.text, "4");
    let (w, h) = (2.0, text.height);
    let lo = text.at - Point2::new(w / 2.0, 0.0);
    let hi = lo + Point2::new(w, h);
    let slanted: Vec<_> = laid
        .strokes
        .iter()
        .filter(|s| {
            s.pen == Pen::Narrow && (s.b - s.a).x.abs() > 1e-6 && (s.b - s.a).y.abs() > 1e-6
        })
        .collect();
    assert!(slanted.len() > 5, "the cut is hatched");
    for s in slanted {
        // (Sampled along it: none of it in the text's box.)
        for k in 0..=50 {
            let p = s.a.lerp(s.b, f64::from(k) / 50.0);
            assert!(
                !(p.cmpgt(lo).all() && p.cmplt(hi).all()),
                "hatching through the text at {p:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_tolerance_of_an_axis_points_at_its_diameters_dimension_line() -> anyhow::Result<()> {
    // A round feature of radius 1,8 about the z axis, its diameter shown
    // from the front below the block: the perpendicularity of its axis
    // and the datum that is its axis stand on the extension of that
    // dimension line (ISO 1101, ISO 5459), not on its wall.
    use workpiece_core::world::FeatureAxis;
    let mut b = block()?;
    let of = FeatureAxis {
        centre: Point3::ZERO,
        axis: Point3::Z,
        radius: 1.8,
        length: 4.0,
    };
    say(
        &mut b,
        "seat",
        Note::Dimension {
            measured: Measured::Diameter {
                centre: of.centre,
                axis: of.axis,
                radius: of.radius,
                faces: Vec::new(),
                hole: false,
            },
            value: 3.6,
            tolerance: Tolerance::General,
            elsewhere: Vec::new(),
        },
    );
    say(
        &mut b,
        "square",
        Note::Geometric {
            characteristic: Characteristic::Perpendicularity,
            value: 0.02,
            diametral: true,
            datums: vec!["B".into()],
            on: Point3::new(1.0, 0.0, 1.0),
            out: Point3::X,
            axis: Some(of),
        },
    );
    say(
        &mut b,
        "datum",
        Note::Datum {
            letter: "B".into(),
            on: Point3::new(-1.0, 0.0, 1.0),
            out: -Point3::X,
            axis: Some(of),
        },
    );
    let mut s = sheet(
        vec![front([150.0, 150.0])],
        // (The geometric notes first: they are drawn after the
        // dimension all the same.)
        vec![
            note("square", "front", [60.0, -50.0]),
            note("datum", "front", [-60.0, -50.0]),
            note("seat", "front", [0.0, -35.0]),
        ],
    );
    // (Large enough for the diameter's text between its arrows.)
    s.scale = [10, 1];
    let laid = lay(&b, &s);
    let seat = laid
        .notes
        .iter()
        .find(|n| n.index == 2)
        .and_then(|n| n.linear)
        .expect("the diameter as a length");
    let [l0, l1] = seat.line;
    assert!((l0.y - l1.y).abs() < 1e-9, "the dimension line is level");
    // The frame's arrow: its tip on an end of the dimension line,
    // pointing along it, inwards (from the frame's side, the right).
    let right = if l0.x > l1.x { l0 } else { l1 };
    let left = if l0.x > l1.x { l1 } else { l0 };
    let arrow_at = |tip: Point2, way: f64| {
        laid.fills.iter().any(|f| {
            f.corners[0].distance(tip) < 1e-6
                && ((f.corners[1] + f.corners[2]) / 2.0 - tip).x * way > 1.0
                && ((f.corners[1] + f.corners[2]) / 2.0 - tip).y.abs() < 1e-6
        })
    };
    assert!(
        arrow_at(right, 1.0),
        "the tolerance's arrow from outside, on the right"
    );
    // The datum's triangle: its base on the left extension line at the
    // dimension line's height, its apex out to the left.
    let triangle = laid.fills.iter().any(|f| {
        f.corners
            .iter()
            .filter(|c| (c.x - left.x).abs() < 1e-6)
            .count()
            == 2
            && f.corners
                .iter()
                .any(|c| c.x < left.x - 1.0 && (c.y - left.y).abs() < 1e-6)
    });
    assert!(triangle, "the datum's triangle on the left, in line");
    Ok(())
}

#[test]
fn a_straight_edge_is_no_arc_to_centre_mark() -> anyhow::Result<()> {
    // A length that ends below the block, as far from both ends of its
    // top edge: that edge is a chord of a circle about the end, but no
    // arc — no centre mark as large as that circle is drawn there.
    let mut b = block()?;
    say(
        &mut b,
        "below",
        Note::Dimension {
            measured: Measured::Length {
                from: Point3::new(0.0, -2.0, -5.0),
                to: Point3::new(2.0, -2.0, -5.0),
                along: None,
            },
            value: 2.0,
            tolerance: Tolerance::General,
            elsewhere: Vec::new(),
        },
    );
    let laid = lay(
        &b,
        &sheet(
            vec![front([150.0, 150.0])],
            vec![note("below", "front", [5.0, -35.0])],
        ),
    );
    let long = laid
        .strokes
        .iter()
        .filter(|s| s.pen == Pen::Chain && s.a.distance(s.b) > 15.0)
        .count();
    assert_eq!(long, 0, "no centre mark about the length's end");
    Ok(())
}

#[test]
fn leaders_of_datums_and_tolerances_end_apart_where_asked_and_reach_their_frames()
-> anyhow::Result<()> {
    // The block's bottom (y 140 on the sheet, x 140 to 160): a datum and
    // a flatness on it, their boxes side by side below; a third, put
    // above the block (behind the face, as seen from it), steered with
    // `to` to the bottom's left end.
    let mut b = block()?;
    say(
        &mut b,
        "datum",
        Note::Datum {
            letter: "A".into(),
            on: Point3::new(0.0, -1.0, -2.0),
            out: -Point3::Z,
            axis: None,
        },
    );
    say(
        &mut b,
        "flat",
        Note::Geometric {
            characteristic: Characteristic::Flatness,
            value: 0.05,
            diametral: false,
            datums: Vec::new(),
            on: Point3::new(0.0, -1.0, -2.0),
            out: -Point3::Z,
            axis: None,
        },
    );
    say(
        &mut b,
        "behind",
        Note::Geometric {
            characteristic: Characteristic::Flatness,
            value: 0.1,
            diametral: false,
            datums: Vec::new(),
            on: Point3::new(0.0, -1.0, -2.0),
            out: -Point3::Z,
            axis: None,
        },
    );
    let mut behind = note("behind", "front", [30.0, 20.0]);
    behind.to = Some([-7.0, -10.0]);
    let laid = lay(
        &b,
        &sheet(
            vec![front([150.0, 150.0])],
            vec![
                note("datum", "front", [0.0, -20.0]),
                note("flat", "front", [0.0, -20.0]),
                behind,
            ],
        ),
    );
    // Where each leader ends on the bottom: the datum's triangle (two
    // corners on it), the arrows' tips.
    let on_bottom: Vec<f64> = laid
        .fills
        .iter()
        .filter_map(|f| {
            let on: Vec<&Point2> = f
                .corners
                .iter()
                .filter(|c| (c.y - 140.0).abs() < 1e-6)
                .collect();
            match on[..] {
                [a, b] => Some((a.x + b.x) / 2.0),
                [a] if f.corners[0] == *a => Some(a.x),
                _ => None,
            }
        })
        .collect();
    assert_eq!(on_bottom.len(), 3, "{on_bottom:?}");
    for (k, x) in on_bottom.iter().enumerate() {
        assert!((140.0..=160.0).contains(x), "on the bottom: {x}");
        for y in &on_bottom[k + 1..] {
            assert!((x - y).abs() > 4.0, "apart: {on_bottom:?}");
        }
    }
    // Steered: the one put above ends at the bottom's left, as `to` asks.
    assert!(
        on_bottom.iter().any(|x| (x - 143.0).abs() < 0.5),
        "{on_bottom:?}"
    );
    // And its leader reaches its frame: a line of it ends on the frame's
    // nearer end, half way up.
    let frame = laid
        .notes
        .iter()
        .find(|n| n.index == 2)
        .expect("its frame")
        .rect;
    let middle = (frame[0].y + frame[1].y) / 2.0;
    assert!(
        laid.strokes
            .iter()
            .any(|s| [s.a, s.b].iter().any(|p| (p.y - middle).abs() < 1e-6
                && ((p.x - frame[0].x).abs() < 1e-6 || (p.x - frame[1].x).abs() < 1e-6))),
        "the leader reaches the frame"
    );
    Ok(())
}

#[test]
fn a_surface_texture_is_its_symbol_on_the_outline_and_the_sheet_says_the_others()
-> anyhow::Result<()> {
    // Ra 1,6 machined on the block's top, its symbol put right over it;
    // the sheet's own Ra 6,3 by the title block.
    use workpiece_core::world::Removal;
    let mut b = block()?;
    say(
        &mut b,
        "rough",
        Note::Texture {
            text: "Ra 1,6".into(),
            removal: Removal::Required,
            on: Point3::new(0.0, -2.0, 2.0),
            out: Point3::Z,
        },
    );
    let mut s = sheet(
        vec![front([150.0, 150.0])],
        vec![note("rough", "front", [0.0, 10.0])],
    );
    s.surface_texture = Some("Ra 6,3".into());
    let laid = lay(&b, &s);
    assert!(laid.problems.is_empty(), "{:?}", laid.problems);
    let written = texts(&laid);
    for text in ["Ra 1,6", "Ra 6,3", "(", ")"] {
        assert!(written.iter().any(|t| t == text), "{text} in {written:?}");
    }
    // Its apex on the top outline (y 160), its legs 60° off it: the
    // shorter 1,4 h high, the longer 3 h; a bar closes them at 1,4 h.
    let h = workpiece_ops_drawing::lay::lettering();
    let apex = Point2::new(150.0, 160.0);
    let leg = |height: f64, way: f64| {
        let top = apex + Point2::new(way * height * libm::tan(30f64.to_radians()), height);
        laid.strokes.iter().any(|s| {
            s.pen == Pen::Narrow
                && ((s.a - apex).length() < 1e-6 && (s.b - top).length() < 1e-6
                    || (s.b - apex).length() < 1e-6 && (s.a - top).length() < 1e-6)
        })
    };
    assert!(leg(1.4 * h, -1.0), "the shorter leg");
    assert!(leg(3.0 * h, 1.0), "the longer leg");
    let bar = laid.strokes.iter().any(|s| {
        (s.a.y - (apex.y + 1.4 * h)).abs() < 1e-6
            && (s.b.y - s.a.y).abs() < 1e-9
            && (s.b.x - s.a.x).abs() > 1.0
            && s.a.x.min(s.b.x) < apex.x
    });
    assert!(bar, "machined: the bar");
    Ok(())
}

#[test]
fn a_text_with_no_room_between_its_arrows_goes_past_the_end_it_is_put_by() -> anyhow::Result<()> {
    // A height of 1 at 1:1, upright in the front view: its text "1 ±0,1"
    // does not fit between the arrows; put a little above the middle, it
    // goes past the upper arrow; a little below, past the lower.
    let mut b = block()?;
    say(
        &mut b,
        "short",
        Note::Dimension {
            measured: Measured::Length {
                from: Point3::new(2.0, -2.0, 0.0),
                to: Point3::new(2.0, -2.0, 1.0),
                along: None,
            },
            value: 1.0,
            tolerance: Tolerance::Symmetric(0.1),
            elsewhere: Vec::new(),
        },
    );
    for (dy, above) in [(1.0, true), (-0.5, false)] {
        let mut s = sheet(
            vec![front([150.0, 150.0])],
            vec![note("short", "front", [12.0, 0.5 + dy])],
        );
        s.scale = [1, 1];
        let laid = lay(&b, &s);
        let text = laid
            .texts
            .iter()
            .find(|t| t.text.starts_with("1 "))
            .expect("its text");
        // (The height runs from 150 to 151 on the sheet.)
        if above {
            assert!(text.at.y > 151.0, "above: {:?}", text.at);
        } else {
            assert!(text.at.y < 150.0, "below: {:?}", text.at);
        }
    }
    Ok(())
}

#[test]
fn an_angle_too_long_for_its_arc_stands_over_its_middle_not_past_an_arm() -> anyhow::Result<()> {
    // The block's corner, 90° between its bottom and its side, with a
    // tolerance; its text put inside the corner's sector near the apex,
    // where the arc is too short for it: it stands over the arc's
    // middle, between the arms (not out past one, over what runs on).
    let mut b = block()?;
    say(
        &mut b,
        "corner",
        Note::Dimension {
            measured: Measured::Angle {
                at: Point3::new(-2.0, -2.0, -2.0),
                first: Point3::Y,
                second: Point3::Z,
            },
            value: 90.0,
            tolerance: Tolerance::Symmetric(0.5),
            elsewhere: Vec::new(),
        },
    );
    let laid = lay(
        &b,
        &sheet(
            vec![front([150.0, 150.0])],
            vec![note("corner", "front", [-6.5, -6.5])],
        ),
    );
    let text = laid
        .texts
        .iter()
        .find(|t| t.text.starts_with("90°"))
        .unwrap_or_else(|| panic!("{:?} {:?}", texts(&laid), laid.problems));
    // (The apex at 140, 140 on the sheet; the sector up and right of it.)
    let d = text.at - Point2::new(140.0, 140.0);
    assert!(d.x > 0.0 && d.y > 0.0, "within the sector: {:?}", text.at);
    Ok(())
}
