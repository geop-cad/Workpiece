//! What is said of a hole, as it stands on the model in 3D: its leader
//! ends with its arrowhead on the hole's rim, where the hole is entered
//! (ISO 129-1), wherever its text is moved, as the model is built.

use workpiece_core::math::Point3;
use workpiece_core::world::Annotation;
use workpiece_ops_drawing::lay::on_model::{OnModel, on_model};
use workpiece_ops_features::{Hole, HoleArgs, HoleKind, HolePlace};
use workpiece_ops_sweep::Extent;
use workpiece_ops_testing::*;

/// A 40 × 40 × 40 block about the origin (its top at z = 20) with holes
/// drilled down from the top at `at`, as `hole` changes their arguments;
/// and a cut from the top, 4 deep, of `pocket` (its corners) if given.
fn drilled(
    at: &[(f64, f64)],
    hole: impl FnOnce(&mut HoleArgs),
    pocket: Option<[f64; 4]>,
) -> anyhow::Result<Standalone> {
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
    for (u, v) in at {
        let p = points.args.add_point(Point2::new(*u, *v));
        points.args.add(Shape::Point(p));
    }
    points.apply(&mut b)?;
    let mut args = HoleArgs {
        place: HolePlace::Sketch,
        sketch: Some(Source::Profile("points".into()).into()),
        diameter: Measure::new(8.0),
        depth: Measure::new(12.0),
        ..HoleArgs::default()
    };
    hole(&mut args);
    Hole {
        id: "hole".into(),
        args,
    }
    .apply(&mut b)?;
    if let Some([u0, v0, u1, v1]) = pocket {
        let mut outline = Sketch::new("pocket_outline");
        outline.args.plane = Some(Reference::new(Source::Topology("block/top".into())));
        let corners = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)]
            .map(|(u, v)| outline.args.add_point(Point2::new(u, v)));
        for k in 0..4 {
            outline
                .args
                .add(Shape::Line(corners[k], corners[(k + 1) % 4]));
        }
        outline.apply(&mut b)?;
        let mut pocket =
            workpiece_ops_sweep::Extrude::of("pocket", Some("pocket_outline".into()), false);
        pocket.args.distance = Measure::new(4.0);
        pocket.args.combine = workpiece_shape::Combine::Cut;
        pocket.apply(&mut b)?;
    }
    Ok(b)
}

/// The hole's callout in `b`.
fn callout(b: &Standalone) -> &Annotation {
    &b.world.annotations["hole/callout"]
}

/// `note` shown with its text at `at`, seen from `eye` (a way to who
/// looks; up as near +z as it can be).
fn shown(b: &Standalone, note: &Annotation, at: Point3, eye: Point3) -> OnModel {
    let back = eye.normalize();
    let right = Point3::Z.cross(back).try_normalize().unwrap_or(Point3::X);
    let up = back.cross(right);
    on_model(note, at, right, up, 0.1, &b.world.topo).expect("it is drawn")
}

/// Where its arrowhead's point is: the one corner of a filled triangle
/// that is sharp.
fn tip(drawn: &OnModel) -> Point3 {
    let sharp = |t: &[Point3; 3]| {
        let at = |k: usize| {
            let (a, b) = (t[(k + 1) % 3] - t[k], t[(k + 2) % 3] - t[k]);
            a.normalize().dot(b.normalize())
        };
        (0..3)
            .max_by(|i, j| at(*i).total_cmp(&at(*j)))
            .expect("three")
    };
    let arrows: Vec<Point3> = drawn.fills.iter().map(|t| t[sharp(t)]).collect();
    assert_eq!(arrows.len(), 1, "one arrowhead: {arrows:?}");
    arrows[0]
}

/// How far `p` is off the axis down through `(x, y)`, and its height.
fn off_axis(p: Point3, x: f64, y: f64) -> (f64, f64) {
    (Point3::new(p.x - x, p.y - y, 0.0).length(), p.z)
}

fn near(got: f64, wanted: f64) {
    assert!((got - wanted).abs() < 0.05, "{got} is not {wanted}");
}

/// Where the hole at `(u, v)` of the top's sketch is in the model.
fn centre(b: &Standalone) -> Point3 {
    match &callout(b).note {
        workpiece_core::world::Note::Hole { centre, .. } => *centre,
        _ => unreachable!("a callout"),
    }
}

#[test]
fn a_blind_hole_s_leader_ends_on_its_rim_wherever_its_text_is() -> anyhow::Result<()> {
    let b = drilled(&[(5.0, 3.0)], |_| {}, None)?;
    let c = centre(&b);
    let note = callout(&b);
    let eye = Point3::new(1.0, -0.8, 0.9);
    // Where the step put it, off the plane of the top, far off it to the
    // side, and on the far side of the part: the arrow on the rim (Ø8,
    // on the top at z = 20) each time.
    for at in [
        note.at,
        note.at + Point3::Z * 30.0,
        c + Point3::new(-60.0, 10.0, 45.0),
        c + Point3::new(10.0, 70.0, -50.0),
    ] {
        let (r, z) = off_axis(tip(&shown(&b, note, at, eye)), c.x, c.y);
        near(r, 4.0);
        near(z, 20.0);
    }
    // The point of the rim in line with its middle and the text, as
    // seen: here, the text right of the hole as seen (level with it, and
    // over it, nearer the eye), the arrow on the rim's right.
    let back = eye.normalize();
    let right = Point3::Z.cross(back).normalize();
    for at in [c + right * 30.0, c + right * 30.0 + back * 25.0] {
        let t = tip(&shown(&b, note, at, eye));
        assert!((t - c).normalize().dot(right) > 0.999);
    }
    Ok(())
}

#[test]
fn a_through_hole_s_leader_ends_on_the_rim_on_the_side_of_its_text() -> anyhow::Result<()> {
    let b = drilled(&[(0.0, 0.0)], |a| a.extent = Extent::ThroughAll, None)?;
    let c = centre(&b);
    let note = callout(&b);
    // Over the top, the rim on the top; under the part, the rim it
    // comes out of (no leader runs through the part).
    let above = tip(&shown(
        &b,
        note,
        c + Point3::new(20.0, 5.0, 15.0),
        Point3::Z + Point3::X,
    ));
    near(off_axis(above, c.x, c.y).0, 4.0);
    near(above.z, 20.0);
    let below = tip(&shown(
        &b,
        note,
        c + Point3::new(20.0, 5.0, -65.0),
        Point3::X - Point3::Z,
    ));
    near(off_axis(below, c.x, c.y).0, 4.0);
    near(below.z, -20.0);
    Ok(())
}

#[test]
fn a_counterbore_s_leader_ends_on_its_outer_rim() -> anyhow::Result<()> {
    let b = drilled(
        &[(0.0, 0.0)],
        |a| {
            a.kind = HoleKind::Counterbore;
            a.extent = Extent::ThroughAll;
            a.head = Measure::new(14.0);
            a.head_depth = Measure::new(8.0);
        },
        None,
    )?;
    let c = centre(&b);
    let note = callout(&b);
    // Wherever its text is — on the far side too: the head is where it
    // is entered — the arrow on the counterbore's rim, Ø14 on the top.
    for at in [
        note.at,
        c + Point3::new(-30.0, 20.0, 25.0),
        c + Point3::new(30.0, 0.0, -70.0),
    ] {
        let (r, z) = off_axis(tip(&shown(&b, note, at, Point3::ONE)), c.x, c.y);
        near(r, 7.0);
        near(z, 20.0);
    }
    Ok(())
}

#[test]
fn a_pattern_s_callout_counts_them_and_points_at_the_first() -> anyhow::Result<()> {
    let b = drilled(
        &[(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)],
        |_| {},
        None,
    )?;
    let note = callout(&b);
    assert!(note.text().starts_with("4× Ø8"), "{}", note.text());
    let c = centre(&b);
    let (r, z) = off_axis(
        tip(&shown(&b, note, note.at + Point3::Z * 20.0, Point3::ONE)),
        c.x,
        c.y,
    );
    near(r, 4.0);
    near(z, 20.0);
    Ok(())
}

#[test]
fn a_hole_cut_by_a_later_step_points_at_what_is_left_of_its_rim() -> anyhow::Result<()> {
    // A pocket 4 deep takes the side of the rim towards +x.
    let b = drilled(
        &[(0.0, 0.0)],
        |a| a.extent = Extent::ThroughAll,
        Some([0.0, -10.0, 10.0, 10.0]),
    )?;
    let c = centre(&b);
    let note = callout(&b);
    // The text off towards +x: the arrow on the rim where it is now, the
    // pocket's floor, 4 under the top — not where the top was.
    let t = tip(&shown(
        &b,
        note,
        c + Point3::new(30.0, 0.0, 10.0),
        Point3::new(0.2, -1.0, 0.6),
    ));
    near(off_axis(t, c.x, c.y).0, 4.0);
    near(t.z, 16.0);
    Ok(())
}

#[test]
fn a_hole_built_elsewhere_takes_its_callout_along() -> anyhow::Result<()> {
    // The same hole, its sketch's point moved and the part built again:
    // the callout's faces are the same names, the arrow on the new rim.
    let first = drilled(&[(0.0, 0.0)], |_| {}, None)?;
    let again = drilled(&[(12.0, -7.0)], |_| {}, None)?;
    let (was, now) = (callout(&first), callout(&again));
    let names = |a: &Annotation| match &a.note {
        workpiece_core::world::Note::Hole { faces, .. } => faces.clone(),
        _ => Vec::new(),
    };
    assert_eq!(names(was), names(now));
    assert!(!names(now).is_empty());
    // (Its text kept as far from it as it was put.)
    let off = Point3::new(15.0, 10.0, 20.0);
    let c = centre(&again);
    assert!(c.distance(centre(&first)) > 10.0);
    let t = tip(&shown(&again, now, now.origin() + off, Point3::ONE));
    let (r, z) = off_axis(t, c.x, c.y);
    near(r, 4.0);
    near(z, 20.0);
    Ok(())
}
