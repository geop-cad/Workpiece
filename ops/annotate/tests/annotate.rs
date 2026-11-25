//! What is said of the model on it.

use shapecraft_core::math::Point3;
use shapecraft_core::world::{Characteristic, Measured, Note, Tolerance};
use shapecraft_ops_annotate::{
    DatumFeature, DatumFeatureArgs, Dimension, DimensionArgs, DimensionKind, GeometricTolerance,
    GeometricToleranceArgs,
};
use shapecraft_ops_testing::*;

fn face(name: &str) -> Source {
    Source::Topology(format!("base/{name}"))
}

#[test]
fn datums_dimensions_and_tolerances_are_said_of_the_model() -> anyhow::Result<()> {
    // The box 4 × 4 × 2 about the origin.
    let mut b = Standalone::new();
    base(&mut b)?;
    // Its top is datum A: the next datum is B.
    DatumFeature {
        id: "datum1".into(),
        args: DatumFeatureArgs {
            feature: Some(face("top").into()),
            ..DatumFeatureArgs::next(&b.world)
        },
    }
    .apply(&mut b)?;
    assert_eq!(b.world.annotations["datum1"].text(), "Datum A");
    assert_eq!(DatumFeatureArgs::next(&b.world).letter, "B");
    let again = DatumFeature {
        id: "datum2".into(),
        args: DatumFeatureArgs {
            feature: Some(face("bottom").into()),
            letter: "A".into(),
            at: None,
        },
    };
    assert!(again.apply(&mut b).is_err());
    // How flat the top is; how square a side is to it — and to a datum
    // there is none of, not at all.
    let tolerance = |id: &str, of, feature: &str, datums: &str| GeometricTolerance {
        id: id.into(),
        args: GeometricToleranceArgs {
            feature: Some(face(feature).into()),
            characteristic: shapecraft_ops_annotate::Of(of),
            value: Measure::new(0.05),
            diametral: false,
            datums: datums.into(),
            at: None,
        },
    };
    tolerance("flat", Characteristic::Flatness, "top", "").apply(&mut b)?;
    tolerance("square", Characteristic::Perpendicularity, "right", "A").apply(&mut b)?;
    assert_eq!(b.world.annotations["flat"].text(), "Flatness 0,05");
    assert_eq!(
        b.world.annotations["square"].text(),
        "Perpendicularity 0,05 | A"
    );
    assert!(
        tolerance("no", Characteristic::Parallelism, "bottom", "C")
            .apply(&mut b)
            .is_err()
    );
    assert!(
        tolerance("no", Characteristic::Parallelism, "bottom", "")
            .apply(&mut b)
            .is_err()
    );
    // How high it is, to a tenth; and the angle of two of its faces.
    let dimension = |id: &str, kind, of: &[&str], tolerance| Dimension {
        id: id.into(),
        args: DimensionArgs {
            kind,
            of: of.iter().map(|f| face(f).into()).collect(),
            tolerance,
            ..Default::default()
        },
    };
    dimension(
        "height",
        DimensionKind::Distance,
        &["top", "bottom"],
        Tolerance::Symmetric(0.1),
    )
    .apply(&mut b)?;
    let height = &b.world.annotations["height"];
    assert_eq!(height.text(), "2 ±0,1");
    let Note::Dimension {
        measured: Measured::Length { from, to, .. },
        ..
    } = &height.note
    else {
        panic!("expected a length");
    };
    close(from.distance(*to), 2.0, 1e-12);
    close((*to - *from).x.abs() + (*to - *from).y.abs(), 0.0, 1e-12);
    dimension(
        "corner",
        DimensionKind::Angle,
        &["top", "right"],
        Tolerance::Deviations {
            upper: 0.5,
            lower: -0.25,
        },
    )
    .apply(&mut b)?;
    assert_eq!(b.world.annotations["corner"].text(), "90° +0,5/−0,25");
    // Asked for nothing in particular, it is what fits what is picked:
    // a distance between faces that are parallel, an angle between
    // faces that are not.
    dimension(
        "picked",
        DimensionKind::Auto,
        &["top", "bottom"],
        Tolerance::Limits {
            upper: 0.1,
            lower: -0.2,
        },
    )
    .apply(&mut b)?;
    assert_eq!(b.world.annotations["picked"].text(), "2 (−0,2…+0,1)");
    let askew = dimension(
        "askew",
        DimensionKind::Auto,
        &["top", "right"],
        Tolerance::General,
    );
    askew.apply(&mut b)?;
    assert_eq!(b.world.annotations["askew"].text(), "90°");
    // A click beside the model, with what it measures picked, stands
    // its text there — as far from the eye as what it measures — and
    // nothing, while nothing is picked.
    let world = std::sync::Arc::new(b.world.clone());
    let cache = shapecraft_core::Cache::default();
    let context = shapecraft_core::EditContext::standalone(&world, &cache);
    let ortho = shapecraft_core::ray::Projection::Orthographic { radius: 0.05 };
    let ray = shapecraft_core::ray::Ray::new(Point3::new(9.0, 20.0, 0.5), -Point3::Y, ortho)
        .expect("a ray");
    let place = askew
        .visualize_edit(&context, &())
        .place
        .expect("placed by a click");
    let placed = place(&ray)
        .and_then(|edit| edit.args)
        .expect("its text stands");
    let at = Point3::from_array(placed.at.expect("where it was clicked"));
    close(at.x, 9.0, 1e-9);
    close(at.z, 0.5, 1e-9);
    let empty = dimension("empty", DimensionKind::Auto, &[], Tolerance::General);
    let place = empty
        .visualize_edit(&context, &())
        .place
        .expect("placed by a click");
    assert!(place(&ray).is_none());
    // The other angle of two faces: what is left of a half turn; and
    // for reference, in brackets.
    let mut other = dimension(
        "other",
        DimensionKind::Angle,
        &["top", "right"],
        Tolerance::Reference,
    );
    other.args.supplement = true;
    other.apply(&mut b)?;
    assert_eq!(b.world.annotations["other"].text(), "(90°)");
    // Faces that are not parallel have no distance.
    assert!(
        dimension(
            "no",
            DimensionKind::Distance,
            &["top", "right"],
            Tolerance::General
        )
        .apply(&mut b)
        .is_err()
    );
    Ok(())
}

#[test]
fn one_thing_of_a_placed_part_is_measured_as_one_of_the_parts_own() -> anyhow::Result<()> {
    use shapecraft_core::Pose;
    use shapecraft_core::world::Instance;
    use shapecraft_ops_basic_shapes::{AddCylinder, AddCylinderArgs};
    // A pin of radius 5 and height 30, placed lying down and moved off.
    let mut pin = Standalone::new();
    AddCylinder {
        id: "pin".into(),
        args: AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(5.0),
            height: Measure::new(30.0),
        },
    }
    .apply(&mut pin)?;
    let pose = Pose::from_axes(Point3::new(7.0, 8.0, 9.0), -Point3::Z, Point3::Y);
    let mut b = Standalone::new();
    b.world.instances.insert(
        "pin1".into(),
        Instance {
            pose,
            world: std::sync::Arc::new(pin.world),
            fixed: true,
        },
    );
    let of = |name: &str| {
        Source::placed(
            &["pin1".to_owned()],
            Source::Topology(format!("pin/{name}")),
        )
    };
    let measure = |id: &str, kind, name: &str, b: &mut Standalone| -> anyhow::Result<f64> {
        Dimension {
            id: id.into(),
            args: DimensionArgs {
                kind,
                of: vec![of(name).into()],
                ..DimensionArgs::default()
            },
        }
        .apply(b)?;
        match &b.world.annotations[id].note {
            Note::Dimension {
                value, measured, ..
            } => {
                // (Where the part is, not where it was made.)
                if let Measured::Diameter { centre, axis, .. } = measured {
                    close(centre.y, 8.0, 1e-9);
                    close(axis.x.abs(), 1.0, 1e-9);
                }
                Ok(*value)
            }
            _ => anyhow::bail!("not a dimension"),
        }
    };
    // Its round side and its rim, by their diameter and radius; picked
    // alone, a whole round by its diameter.
    close(
        measure("side", DimensionKind::Diameter, "side", &mut b)?,
        10.0,
        1e-9,
    );
    close(
        measure("rim", DimensionKind::Radius, "top-rim", &mut b)?,
        5.0,
        1e-9,
    );
    close(
        measure("auto", DimensionKind::Auto, "side", &mut b)?,
        10.0,
        1e-9,
    );
    // Its seam, a straight edge, by its length.
    close(
        measure("seam", DimensionKind::Auto, "seam", &mut b)?,
        30.0,
        1e-9,
    );
    Ok(())
}

#[test]
fn bends_that_lean_are_measured_between_their_centres() -> anyhow::Result<()> {
    use shapecraft_ops_basic_shapes::{AddTorus, AddTorusArgs};
    // Two rings around z, 10 apart along x: the tube's own circle, which
    // a face of one is round about, leans along x.
    let mut b = Standalone::new();
    for (id, x) in [("a", 0.0), ("b", 10.0)] {
        let pose = shapecraft_core::Pose::from_translation(Point3::new(x, 0.0, 0.0));
        b.world.datums.insert(
            format!("{id}-frame"),
            shapecraft_core::world::Datum::Frame(shapecraft_core::world::DatumFrame::from_pose(
                pose,
            )?),
        );
        AddTorus {
            id: id.into(),
            args: AddTorusArgs {
                frame: Some(Reference::datum(format!("{id}-frame"))),
                radius: Measure::new(3.0),
                tube: Measure::new(1.0),
            },
        }
        .apply(&mut b)?;
    }
    // Two rods that lean 45°, 10 apart along x: the axis of each leans
    // along x, so they have no sides that way, only their middles.
    for (id, x) in [("c", 0.0), ("d", 10.0)] {
        let pose = shapecraft_core::Pose::from_axes(
            Point3::new(x, 20.0, 0.0),
            Point3::new(1.0, 0.0, -1.0).normalize(),
            Point3::Y,
        );
        b.world.datums.insert(
            format!("{id}-frame"),
            shapecraft_core::world::Datum::Frame(shapecraft_core::world::DatumFrame::from_pose(
                pose,
            )?),
        );
        shapecraft_ops_basic_shapes::AddCylinder {
            id: id.into(),
            args: shapecraft_ops_basic_shapes::AddCylinderArgs {
                frame: Some(Reference::datum(format!("{id}-frame"))),
                radius: Measure::new(1.0),
                height: Measure::new(6.0),
            },
        }
        .apply(&mut b)?;
    }
    for (a, b_, centres, far, wanted) in [
        ("a", "b", true, false, 10.0),
        ("a", "b", false, false, 8.0),
        ("c", "d", false, false, 10.0),
        ("c", "d", true, false, 10.0),
        ("c", "d", false, true, 10.0),
    ] {
        Dimension {
            id: "gap".into(),
            args: DimensionArgs {
                kind: DimensionKind::Distance,
                of: vec![
                    Source::Topology(format!("{a}/{}", if a == "a" { "face" } else { "side" }))
                        .into(),
                    Source::Topology(format!("{b_}/{}", if a == "a" { "face" } else { "side" }))
                        .into(),
                ],
                centres,
                far,
                along: Some(Reference::frame_axis(ORIGIN, Axis::X)),
                ..DimensionArgs::default()
            },
        }
        .apply(&mut b)?;
        let Note::Dimension {
            measured: Measured::Length { from, to, .. },
            ..
        } = &b.world.annotations["gap"].note
        else {
            panic!("a length");
        };
        close(from.distance(*to), wanted, 1e-6);
        b.world.annotations.remove("gap");
    }
    Ok(())
}

#[test]
fn a_cone_s_side_is_measured_against_its_base_at_its_slant() -> anyhow::Result<()> {
    use shapecraft_ops_basic_shapes::{AddCone, AddConeArgs};
    // A cone 1 across its base and 1 high: its side leans at
    // atan(1 / 0.5) ≈ 63.43° to its base.
    let mut b = Standalone::new();
    AddCone {
        id: "cone".into(),
        args: AddConeArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(0.5),
            top: Measure::new(0.0),
            height: Measure::new(1.0),
        },
    }
    .apply(&mut b)?;
    Dimension {
        id: "slant".into(),
        args: DimensionArgs {
            kind: DimensionKind::Angle,
            of: vec![
                Source::Topology("cone/bottom".into()).into(),
                Source::Topology("cone/side".into()).into(),
            ],
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let Note::Dimension { value, .. } = &b.world.annotations["slant"].note else {
        panic!("a dimension");
    };
    close(*value, 2f64.atan().to_degrees(), 1e-6);
    Ok(())
}
