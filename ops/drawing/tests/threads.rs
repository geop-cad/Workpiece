//! Threads that are said of the model, in its drawing (ISO 6410-1), and
//! what a tapped hole's callout says of them.

use std::f64::consts::PI;
use workpiece_core::math::angle_to;

use workpiece_core::math::Point3;
use workpiece_core::world::{Callout, Cutting, Note};
use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};
use workpiece_ops_drawing::Sheet;
use workpiece_ops_drawing::lay::{Kept, Laid, Pen, Subject, lay_out};
use workpiece_ops_drawing::sheet::{SheetView, Side, Style, ViewKind};
use workpiece_ops_features::{Deburr, Hole, HoleArgs, HoleKind, HolePlace, ThreadEnd};
use workpiece_ops_sweep::Extent;
use workpiece_ops_testing::*;

/// A 40 × 40 × 40 block about the origin with a tapped hole down from
/// the middle of its top (at z = 20), as `hole` changes the hole's
/// arguments; and the plane square to y through the hole's axis, as the
/// section `mid`.
fn tapped(hole: impl FnOnce(&mut HoleArgs)) -> anyhow::Result<Standalone> {
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
    points.args.plane = Some(Reference::new(Source::Topology("block/top".into())));
    let p = points.args.add_point(Point2::new(0.0, 0.0));
    points.args.add(Shape::Point(p));
    points.apply(&mut b)?;
    let mut args = HoleArgs {
        place: HolePlace::Sketch,
        sketch: Some(Source::Profile("points".into()).into()),
        kind: HoleKind::Tapped,
        size: Some("M8".into()),
        // (The drill's depth by the workshop's rule: the thread's 12
        // and half the diameter.)
        depth: Measure::new(16.0),
        ..HoleArgs::default()
    };
    hole(&mut args);
    Hole {
        id: "hole".into(),
        args,
    }
    .apply(&mut b)?;
    let cut = Cutting::plane(Point3::ZERO, Point3::Y);
    b.world.sections.insert("mid".into(), cut);
    Ok(b)
}

/// The top view of `b`, and the section through the hole beside it.
fn laid(b: &Standalone) -> Laid {
    let view = |id: &str, kind: ViewKind, at: [f64; 2]| SheetView {
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
    };
    let sheet = Sheet {
        views: vec![
            view("top", ViewKind::Base { side: Side::Top }, [100.0, 200.0]),
            view(
                "s",
                ViewKind::Section {
                    of: Some("top".into()),
                    section: "mid".into(),
                    letter: "A".into(),
                    turn: 0.0,
                },
                [250.0, 200.0],
            ),
        ],
        ..Sheet::default()
    };
    let properties = Default::default();
    let subject = Subject {
        world: &b.world,
        unit: workpiece_core::Unit::default(),
        material: None,
        name: "block",
        properties: &properties,
        parts: &Default::default(),
        densities: &Default::default(),
        sheet: [1, 1],
    };
    lay_out(&sheet, &subject, &Kept::default(), None)
}

/// How far around `middle` the lines of `laid` drawn with `pen` at the
/// distance `r` from it go, in radians.
fn around(laid: &Laid, pen: Pen, middle: Point2, r: f64) -> f64 {
    laid.strokes
        .iter()
        .filter(|s| s.pen == pen)
        .filter(|s| {
            (s.a.distance(middle) - r).abs() < 1e-3 && (s.b.distance(middle) - r).abs() < 1e-3
        })
        .map(|s| angle_to(s.a - middle, s.b - middle).abs())
        .sum()
}

#[test]
fn a_blind_tapped_hole_is_drawn_as_iso_6410_has_it() -> anyhow::Result<()> {
    let b = tapped(|_| {})?;
    let laid = laid(&b);
    let top = laid.views.iter().find(|v| v.id == "top").expect("top");
    let (middle, s) = (top.centre, top.scale);
    // Seen end on: the crest (the tap drill's bore, Ø6.8) a wide full
    // circle; the root (Ø8) three quarters of a narrow one, open at the
    // upper right — its ends a little past the centre lines.
    close(around(&laid, Pen::Wide, middle, 3.4 * s), 2.0 * PI, 0.05);
    close(
        around(&laid, Pen::Narrow, middle, 4.0 * s),
        1.5 * PI - 0.24,
        0.05,
    );
    let open = middle + Point2::new(1.0, 1.0).normalize() * 4.0 * s;
    assert!(
        laid.strokes
            .iter()
            .filter(|k| k.pen == Pen::Narrow)
            .all(|k| k.a.distance(open) > 1.0 * s && k.b.distance(open) > 1.0 * s)
    );
    // Cut open along its axis: the roots narrow lines 8 apart, 12 long
    // from the top; where the full thread ends a wide line across, out
    // to them; the crests (the bore's sides) wide, on to 16; the
    // drill's point under it.
    let sec = laid
        .views
        .iter()
        .find(|v| v.id == "s")
        .expect("the section");
    let (x, s) = (sec.centre.x, sec.scale);
    let top_y = sec.centre.y + 20.0 * s;
    let upright = |pen: Pen, at: f64, from: f64, to: f64| {
        laid.strokes.iter().any(|k| {
            k.pen == pen
                && (k.a.x - at).abs() < 1e-6
                && (k.b.x - at).abs() < 1e-6
                && (k.a.y.max(k.b.y) - (top_y - from * s)).abs() < 1e-6
                && (k.a.y.min(k.b.y) - (top_y - to * s)).abs() < 1e-6
        })
    };
    for side in [-1.0, 1.0] {
        assert!(upright(Pen::Narrow, x + side * 4.0 * s, 0.0, 12.0));
        assert!(upright(Pen::Wide, x + side * 3.4 * s, 0.0, 16.0));
    }
    let across = |y: f64, half: f64| {
        laid.strokes.iter().any(|k| {
            k.pen == Pen::Wide
                && (k.a.y - y).abs() < 1e-6
                && (k.b.y - y).abs() < 1e-6
                && ((k.a.x - k.b.x).abs() - 2.0 * half).abs() < 1e-6
        })
    };
    assert!(across(top_y - 12.0 * s, 4.0 * s));
    // (No line across at the mouth: the thread starts there.)
    assert!(!across(top_y, 4.0 * s));
    let tip = Point2::new(
        x,
        top_y - (16.0 + 3.4 / libm::tan((59.0_f64).to_radians())) * s,
    );
    assert!(
        laid.strokes
            .iter()
            .any(|k| k.pen == Pen::Wide && (k.a.distance(tip) < 1e-3 || k.b.distance(tip) < 1e-3))
    );
    Ok(())
}

#[test]
fn a_thread_through_all_has_no_end_and_a_chamfered_mouth_no_circle() -> anyhow::Result<()> {
    let b = tapped(|a| {
        a.extent = Extent::ThroughAll;
        a.thread_end = ThreadEnd::Through;
        a.deburr = Deburr::Entry;
        a.chamfer = Measure::new(0.8);
    })?;
    let laid = laid(&b);
    let top = laid.views.iter().find(|v| v.id == "top").expect("top");
    let (middle, s) = (top.centre, top.scale);
    // End on, the chamfer's circle (Ø8.4) is not drawn.
    assert_eq!(around(&laid, Pen::Wide, middle, 4.2 * s), 0.0);
    // Cut open, no wide line crosses the bore between the top and the
    // bottom (where the block's own outline runs on behind it).
    let sec = laid
        .views
        .iter()
        .find(|v| v.id == "s")
        .expect("the section");
    let (x, s) = (sec.centre.x, sec.scale);
    let top_y = sec.centre.y + 20.0 * s;
    assert!(!laid.strokes.iter().any(|k| {
        k.pen == Pen::Wide
            && (k.a.y - k.b.y).abs() < 1e-9
            && k.a.y < top_y - 1e-6
            && k.a.y > top_y - 40.0 * s + 1e-6
            && k.a.x.min(k.b.x) < x
            && k.a.x.max(k.b.x) > x
    }));
    Ok(())
}

#[test]
fn a_section_a_hair_off_a_chamfered_hole_s_axis_has_no_sliver_fills() -> anyhow::Result<()> {
    // (As in Plate.wpc: its cutting line drawn by hand 0.14 off an M4
    // hole's axis. There crumbs of the cut — the chamfer's and the drill
    // point's cones grazed — were filled black, and a filled triangle
    // with no area is drawn in spikes.)
    let mut b = Standalone::new();
    AddBox {
        id: "block".into(),
        args: AddBoxArgs {
            frame: Some(Reference::datum(ORIGIN)),
            size: [85.49, 98.07, 37.34].map(Measure::new),
        },
    }
    .apply(&mut b)?;
    let mut points = Sketch::new("points");
    points.args.plane = Some(Reference::new(Source::Topology("block/top".into())));
    let p = points.args.add_point(Point2::new(-30.595, 29.944));
    points.args.add(Shape::Point(p));
    points.apply(&mut b)?;
    Hole {
        id: "hole".into(),
        args: HoleArgs {
            place: HolePlace::Sketch,
            sketch: Some(Source::Profile("points".into()).into()),
            kind: HoleKind::Tapped,
            size: Some("M4".into()),
            depth: Measure::new(10.0),
            deburr: Deburr::Entry,
            ..HoleArgs::default()
        },
    }
    .apply(&mut b)?;
    let cut = Cutting::plane(Point3::new(-30.4558, 0.0, 0.0), Point3::X);
    b.world.sections.insert("mid".into(), cut);
    let laid = laid(&b);
    for fill in &laid.fills {
        let [a, b, c] = fill.corners;
        let long = a.distance(b).max(b.distance(c)).max(c.distance(a));
        assert!((b - a).perp_dot(c - a).abs() / long > 0.05, "{fill:?}");
    }
    Ok(())
}

/// What the callout of the hole says, line by line.
fn callout(b: &Standalone) -> Vec<String> {
    let note = &b.world.annotations["hole/callout"].note;
    let Note::Hole { lines, .. } = note else {
        panic!("a hole's callout");
    };
    lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|c| match c {
                    Callout::Count(n) => format!("{n}× "),
                    Callout::Text(t) => t.clone(),
                    Callout::Number(x) => format!("{x}"),
                    Callout::Depth => "↧".into(),
                    Callout::Counterbore => "⌴".into(),
                    Callout::Countersink => "⌵".into(),
                    tolerance @ Callout::Tolerance { .. } => tolerance.said(),
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_tapped_hole_s_callout_says_the_thread_and_the_bore() -> anyhow::Result<()> {
    // Blind, by the rule: the thread 1.5 × d, the bore half d deeper.
    assert_eq!(callout(&tapped(|_| {})?), ["M8 ↧12", "Ø6.8 ↧16"]);
    // Through all, it says so.
    let through = tapped(|a| {
        a.extent = Extent::ThroughAll;
        a.thread_end = ThreadEnd::Through;
    })?;
    assert_eq!(callout(&through), ["M8 THRU"]);
    // A fine thread names its pitch; a thread short of a bore through.
    let fine = tapped(|a| {
        a.fine = Some("1".into());
        a.thread_depth = Some(Measure::new(10.0));
        a.extent = Extent::ThroughAll;
    })?;
    assert_eq!(callout(&fine), ["M8×1 ↧10", "Ø7 THRU"]);
    // A thread through all of a blind bore is refused; so is one deeper
    // than its bore.
    assert!(
        tapped(|a| a.thread_end = ThreadEnd::Through).is_err(),
        "a thread through a blind bore"
    );
    assert!(tapped(|a| a.depth = Measure::new(10.0)).is_err());
    Ok(())
}
