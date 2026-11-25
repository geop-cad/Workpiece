//! Threads that are said of the model, in its drawing.

use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};
use workpiece_ops_drawing::quick::ThreeViews;
use workpiece_ops_features::{Hole, HoleArgs, HoleKind};
use workpiece_ops_testing::*;

#[test]
fn a_cosmetic_thread_is_drawn_from_the_side_along_its_axis_and_cut_open() -> anyhow::Result<()> {
    // A 40 × 40 × 40 block with an M6 hole tapped 10 deep, down from
    // its middle: drilled 5 across, the thread said of it.
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
    Hole {
        id: "hole".into(),
        args: HoleArgs {
            place: workpiece_ops_features::HolePlace::Sketch,
            sketch: Some(Source::Profile("points".into()).into()),
            kind: HoleKind::Tapped,
            size: Some("M6".into()),
            ..HoleArgs::default()
        },
    }
    .apply(&mut b)?;
    let drawn = ThreeViews::of(&b.world, "block", None)?;
    let view = |name: &str| {
        drawn
            .views
            .iter()
            .find(|v| v.name == name)
            .expect("the view")
    };
    // The lines of a view that stand upright `x` from the block's
    // middle, 10 long: whether each is hidden.
    let upright = |name: &str, x: f64| -> Vec<bool> {
        let v = view(name);
        let at = v.corner.x + 20.0 + x;
        v.lines
            .iter()
            .filter(|(a, b, _)| {
                (a.x - at).abs() < 1e-6
                    && (b.x - at).abs() < 1e-6
                    && ((a.y - b.y).abs() - 10.0).abs() < 1e-6
            })
            .map(|l| l.2)
            .collect()
    };
    // From the front the hole is inside the block: its thread's roots,
    // 6 apart, are hidden lines.
    assert_eq!(upright("FRONT", 3.0), [true]);
    assert_eq!(upright("FRONT", -3.0), [true]);
    // Cut open through the hole's axis, they are in view — outside the
    // bore, where the block is hatched.
    assert_eq!(upright("SECTION", 3.0), [false]);
    assert_eq!(upright("SECTION", -3.0), [false]);
    // And the thread ends in a line across, 6 wide.
    let across = view("SECTION")
        .lines
        .iter()
        .filter(|(a, b, hidden)| {
            !hidden && (a.y - b.y).abs() < 1e-9 && ((a.x - b.x).abs() - 6.0).abs() < 1e-6
        })
        .count();
    assert_eq!(across, 2);
    // From above: three quarters of a circle 6 across, about the
    // hole's axis, hidden in the block.
    let top = view("TOP");
    let middle = top.corner + Point2::splat(20.0);
    let arc: Vec<_> = top
        .lines
        .iter()
        .filter(|(a, b, _)| {
            (a.distance(middle) - 3.0).abs() < 1e-6 && (b.distance(middle) - 3.0).abs() < 1e-6
        })
        .collect();
    assert!(arc.iter().all(|l| l.2));
    let around: f64 = arc
        .iter()
        .map(|(a, b, _)| (*a - middle).angle_to(*b - middle).abs())
        .sum();
    close(around, PI * 1.5, 0.3);
    Ok(())
}
