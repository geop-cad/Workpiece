//! Dimensions of and between round things: the radii of what bends two
//! ways, and distances to their sides.

use workpiece_core::math::Point3;
use workpiece_core::world::{Datum, DatumFrame};
use workpiece_ops_annotate::{Bend, Dimension, DimensionArgs, DimensionKind};
use workpiece_ops_basic_shapes::{
    AddCylinder, AddCylinderArgs, AddSphere, AddSphereArgs, AddTorus, AddTorusArgs,
};
use workpiece_ops_testing::*;

/// A frame `name` at `at`, the origin's directions kept.
fn frame(b: &mut Standalone, name: &str, at: Point3) -> anyhow::Result<()> {
    let frame = DatumFrame::new(at, Point3::X, Point3::Y)?;
    b.world.datums.insert(name.into(), Datum::Frame(frame));
    Ok(())
}

/// What the dimension `args` says, measured in `b`.
fn says(b: &mut Standalone, args: DimensionArgs) -> anyhow::Result<String> {
    Dimension {
        id: "d".into(),
        args,
    }
    .apply(b)?;
    Ok(b.world.annotations["d"].text())
}

fn of(names: &[&str]) -> Vec<Reference<workpiece_core::Anything>> {
    names
        .iter()
        .map(|n| Reference::new(Source::Topology((*n).to_owned())))
        .collect()
}

#[test]
fn a_bent_bar_has_its_own_radius_and_its_bends() -> anyhow::Result<()> {
    // A Ø6 bar bent around at R9: a torus.
    let mut b = Standalone::new();
    AddTorus {
        id: "ring".into(),
        args: AddTorusArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(9.0),
            tube: Measure::new(3.0),
        },
    }
    .apply(&mut b)?;
    let radius = |b: &mut Standalone, bend| {
        says(
            b,
            DimensionArgs {
                kind: DimensionKind::Radius,
                of: of(&["ring/face"]),
                bend,
                ..Default::default()
            },
        )
    };
    assert_eq!(radius(&mut b, Bend::Own)?, "R3");
    assert_eq!(radius(&mut b, Bend::Middle)?, "R9");
    assert_eq!(radius(&mut b, Bend::Inside)?, "R6");
    assert_eq!(radius(&mut b, Bend::Outside)?, "R12");
    // Its edges are circles: the one around its outside, the one
    // around its tube.
    for (edge, wanted) in [("ring/equator", "R12"), ("ring/seam", "R3")] {
        let said = says(
            &mut b,
            DimensionArgs {
                kind: DimensionKind::Radius,
                of: of(&[edge]),
                ..Default::default()
            },
        )?;
        assert_eq!(said, wanted, "{edge}");
    }
    Ok(())
}

#[test]
fn distances_reach_the_sides_of_round_things() -> anyhow::Result<()> {
    // Two upright bars, R2 about x = 0 and R3 about x = 10; a ball of
    // R2 at z = 10 above the first.
    let mut b = Standalone::new();
    frame(&mut b, "there", Point3::new(10.0, 0.0, 0.0))?;
    frame(&mut b, "above", Point3::new(0.0, 0.0, 10.0))?;
    for (id, at, r) in [("a", None, 2.0), ("b", Some("there"), 3.0)] {
        AddCylinder {
            id: id.into(),
            args: AddCylinderArgs {
                frame: Some(Reference::datum(at.unwrap_or(ORIGIN))),
                radius: Measure::new(r),
                height: Measure::new(4.0),
            },
        }
        .apply(&mut b)?;
    }
    AddSphere {
        id: "ball".into(),
        args: AddSphereArgs {
            frame: Some(Reference::datum("above")),
            radius: Measure::new(2.0),
        },
    }
    .apply(&mut b)?;
    let between = |b: &mut Standalone, names: &[&str], how: fn(&mut DimensionArgs)| {
        let mut args = DimensionArgs {
            of: of(names),
            ..Default::default()
        };
        how(&mut args);
        says(b, args)
    };
    let rim = DimensionArgs {
        of: of(&["a/top-rim"]),
        ..Default::default()
    };
    assert_eq!(says(&mut b, rim)?, "⌀4");
    let bars = ["a/side", "b/side"];
    // The gap, the far sides, the middles; and from the one's far side
    // to the other's far side, the side facing the same way.
    assert_eq!(between(&mut b, &bars, |_| {})?, "5");
    assert_eq!(between(&mut b, &bars, |a| a.far = true)?, "15");
    assert_eq!(between(&mut b, &bars, |a| a.centres = true)?, "10");
    assert_eq!(between(&mut b, &bars, |a| a.same_side = true)?, "11");
    // Along z, from the bar's rim at its top to the ball: what each
    // reaches that way.
    let up = |a: &mut DimensionArgs| a.along = Some(Reference::frame_axis(ORIGIN, Axis::Z));
    let rim_and_ball = ["a/top-rim", "ball/face"];
    assert_eq!(between(&mut b, &rim_and_ball, up)?, "4");
    assert_eq!(between(&mut b, &rim_and_ball, |a| a.centres = true)?, "6");
    assert_eq!(
        between(&mut b, &rim_and_ball, |a| {
            a.along = Some(Reference::frame_axis(ORIGIN, Axis::Z));
            a.far = true;
        })?,
        "8"
    );
    Ok(())
}
