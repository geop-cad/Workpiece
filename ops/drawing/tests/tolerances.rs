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
fn the_general_tolerance_is_the_sheets() -> anyhow::Result<()> {
    let b = block()?;
    let cell = |laid: &Laid| {
        laid.cells
            .iter()
            .find(|c| c.label == "General tolerances")
            .map(|c| c.value.clone())
    };
    let mut s = sheet(vec![front([150.0, 150.0])], vec![]);
    assert_eq!(cell(&lay(&b, &s)).as_deref(), Some("ISO 2768-mK"));
    s.general_tolerance = Some("ISO 8062-3 DCTG 9".into());
    assert_eq!(cell(&lay(&b, &s)).as_deref(), Some("ISO 8062-3 DCTG 9"));
    s.general_tolerance = None;
    assert_eq!(cell(&lay(&b, &s)).as_deref(), Some(""));
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
