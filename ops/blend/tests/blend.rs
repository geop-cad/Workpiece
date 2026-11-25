//! What the operations of this crate make.

use workpiece_ops_bodies::{CombineArgs, CombineBodies};
use workpiece_ops_testing::*;
use workpiece_shape::Combine;

fn fillet(
    b: &mut Standalone,
    edges: Vec<Source>,
    change: impl Fn(&mut workpiece_ops_blend::FilletArgs),
) -> anyhow::Result<()> {
    let mut args = workpiece_ops_blend::FilletArgs {
        edges: workpiece_core::Reference::each(edges),
        size: Measure::new(0.5),
        ..Default::default()
    };
    change(&mut args);
    workpiece_ops_blend::Fillet {
        id: "fillet".into(),
        args,
    }
    .apply(b)
}

#[test]
fn edges_are_rounded_and_cut_off() -> anyhow::Result<()> {
    // One edge of the box: the top one at x = 2, 4 long.
    let one = |a: workpiece_core::math::Point3, b: workpiece_core::math::Point3| {
        a.x > 1.9 && b.x > 1.9 && a.z > 0.9 && b.z > 0.9
    };
    let r: f64 = 0.5;
    let mut b = Standalone::new();
    base(&mut b)?;
    let edges = edges_where(&b, "base", one);
    assert_eq!(edges.len(), 1);
    fillet(&mut b, edges.clone(), |_| {})?;
    check(&b);
    // (To within what a quarter round sampled as twelve flats is short.)
    close(
        volume(&b.world.topo, body(&b, "base")),
        32.0 - (r * r - PI * r * r / 4.0) * 4.0,
        4e-3,
    );
    // The box with one face more, and nothing left of what cut it.
    let made = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    assert_eq!(
        (made.faces.len(), made.edges.len(), made.vertices.len()),
        (7, 15, 10)
    );

    // By width: the chord of the quarter circle.
    let mut b = Standalone::new();
    base(&mut b)?;
    fillet(&mut b, edges.clone(), |a| {
        a.by = workpiece_shape::By::Width;
        a.size = Measure::new(r * 2f64.sqrt());
    })?;
    close(
        volume(&b.world.topo, body(&b, "base")),
        32.0 - (r * r - PI * r * r / 4.0) * 4.0,
        4e-3,
    );

    // A conic and a curvature-continuous section take less or more.
    for section in [
        workpiece_shape::Section::Conic,
        workpiece_shape::Section::Curvature,
    ] {
        let mut b = Standalone::new();
        base(&mut b)?;
        fillet(&mut b, edges.clone(), |a| a.section = section)?;
        check(&b);
        let taken = 32.0 - volume(&b.world.topo, body(&b, "base"));
        assert!(
            taken > 0.05 && taken < r * r / 2.0 * 4.0,
            "{section:?} took {taken}"
        );
    }

    // A chamfer with two distances.
    let mut b = Standalone::new();
    base(&mut b)?;
    workpiece_ops_blend::Chamfer {
        id: "chamfer".into(),
        args: workpiece_ops_blend::ChamferArgs {
            edges: workpiece_core::Reference::each(edges.clone()),
            distance: Measure::new(0.5),
            two: true,
            second: Measure::new(0.25),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    close(
        volume(&b.world.topo, body(&b, "base")),
        32.0 - 0.5 * 0.25 / 2.0 * 4.0,
        1e-6,
    );
    Ok(())
}

#[test]
fn corners_between_blends_are_closed() -> anyhow::Result<()> {
    // The three edges at one corner of the box.
    let at = |p: workpiece_core::math::Point3| p.x > 1.9 && p.y > 1.9 && p.z > 0.9;
    let r: f64 = 0.5;
    let mut b = Standalone::new();
    base(&mut b)?;
    let edges = edges_where(&b, "base", |a, b| at(a) || at(b));
    assert_eq!(edges.len(), 3);
    fillet(&mut b, edges.clone(), |_| {})?;
    check(&b);
    // Each edge less its corner, and the corner under its ball.
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    let edge = (r * r - PI * r * r / 4.0) * (2.0 * (4.0 - r) + (2.0 - r));
    let corner = r * r * r - PI * r * r * r / 6.0;
    close(
        workpiece_boolean::measure(&solid).volume,
        32.0 - edge - corner,
        1e-9,
    );

    // Chamfered, the corner is cut flat: all of its cube but a
    // tetrahedron is gone.
    let d: f64 = 0.5;
    let mut b = Standalone::new();
    base(&mut b)?;
    workpiece_ops_blend::Chamfer {
        id: "chamfer".into(),
        args: workpiece_ops_blend::ChamferArgs {
            edges: workpiece_core::Reference::each(edges),
            distance: Measure::new(d),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    check(&b);
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    let edge = d * d / 2.0 * (2.0 * (4.0 - d) + (2.0 - d));
    close(
        workpiece_boolean::measure(&solid).volume,
        32.0 - edge - d * d * d * 5.0 / 6.0,
        1e-9,
    );
    Ok(())
}

#[test]
fn inside_corners_and_other_sections_close_too() -> anyhow::Result<()> {
    let r: f64 = 0.5;
    // A notch out of the box's corner: its inside corner at (0, 0, 0),
    // three inside edges 2, 2 and 1 long.
    let notched = || -> anyhow::Result<Standalone> {
        let mut b = Standalone::new();
        base(&mut b)?;
        b.world.datums.insert(
            "at".into(),
            workpiece_core::world::Datum::Frame(workpiece_core::world::DatumFrame::new(
                workpiece_core::math::Point3::new(1.0, 1.0, 0.5),
                workpiece_core::math::Point3::X,
                workpiece_core::math::Point3::Y,
            )?),
        );
        AddBox {
            id: "notch".into(),
            args: AddBoxArgs {
                frame: Some(Reference::datum("at")),
                size: [2.0, 2.0, 1.0].map(Measure::new),
            },
        }
        .apply(&mut b)?;
        CombineBodies {
            id: "cut".into(),
            args: CombineArgs {
                target: Some(Source::Topology("base".into()).into()),
                tool: Some(Source::Topology("notch".into()).into()),
                how: Combine::Cut,
            },
        }
        .apply(&mut b)?;
        Ok(b)
    };
    let inside = |a: workpiece_core::math::Point3, b: workpiece_core::math::Point3| {
        let o = workpiece_core::math::Point3::ZERO;
        a.distance(o) < 1e-9 || b.distance(o) < 1e-9
    };
    let mut b = notched()?;
    let edges = edges_where(&b, "base", inside);
    assert_eq!(edges.len(), 3);
    fillet(&mut b, edges.clone(), |_| {})?;
    check(&b);
    let solid = workpiece_boolean::read(&b.world.topo, body(&b, "base"))?;
    let along = (r * r - PI * r * r / 4.0) * ((2.0 - r) * 2.0 + (1.0 - r));
    let corner = r * r * r - PI * r * r * r / 6.0;
    close(
        workpiece_boolean::measure(&solid).volume,
        32.0 - 4.0 + along + corner,
        1e-9,
    );

    // A conic and a curvature-continuous section: one patch at the
    // corner, between the three rounds, inside and outside.
    for section in [
        workpiece_shape::Section::Conic,
        workpiece_shape::Section::Curvature,
    ] {
        let wanted = 1;
        let patches = |b: &Standalone| {
            let topo = &b.world.topo;
            topo.faces()
                .iter()
                .filter(|(id, _)| {
                    topo.name_of(*id)
                        .is_some_and(|n| n.ends_with("corner/patch"))
                })
                .count()
        };
        let mut b = notched()?;
        fillet(&mut b, edges.clone(), |a| a.section = section)?;
        check(&b);
        assert_eq!(patches(&b), wanted, "{section:?} inside");
        let mut b = Standalone::new();
        base(&mut b)?;
        let at = |p: workpiece_core::math::Point3| p.x > 1.9 && p.y > 1.9 && p.z > 0.9;
        let outside = edges_where(&b, "base", |a, b| at(a) || at(b));
        fillet(&mut b, outside, |a| a.section = section)?;
        check(&b);
        assert_eq!(patches(&b), wanted, "{section:?} outside");
    }
    Ok(())
}

#[test]
fn blends_say_their_sizes_where_they_are() -> anyhow::Result<()> {
    use workpiece_core::world::{Measured, Note};
    // The top edge of the box at x = 2, rounded 0.5: the ball's middle
    // is half in from the side and half down from the top.
    let one = |a: workpiece_core::math::Point3, b: workpiece_core::math::Point3| {
        a.x > 1.9 && b.x > 1.9 && a.z > 0.9 && b.z > 0.9
    };
    let mut b = Standalone::new();
    base(&mut b)?;
    let edges = edges_where(&b, "base", one);
    fillet(&mut b, edges.clone(), |_| {})?;
    let said = &b.world.annotations["fillet/radius"];
    assert_eq!(said.text(), "R0,5");
    let Note::Dimension {
        measured: Measured::Radius { centre, axis, .. },
        ..
    } = &said.note
    else {
        panic!("a radius: {said:?}");
    };
    close(centre.x, 1.5, 1e-9);
    close(centre.z, 0.5, 1e-9);
    close(axis.y.abs(), 1.0, 1e-9);
    // A face picked rounds all its edges, and says so too; a round
    // measured by its width says that width, from rail to rail.
    let mut b = Standalone::new();
    base(&mut b)?;
    fillet(&mut b, vec![Source::Topology("base/top".into())], |_| {})?;
    assert_eq!(b.world.annotations["fillet/radius"].text(), "R0,5");
    let mut b = Standalone::new();
    base(&mut b)?;
    fillet(&mut b, edges.clone(), |a| {
        a.by = workpiece_shape::blend::By::Width
    })?;
    let said = &b.world.annotations["fillet/width"];
    let Note::Dimension {
        measured: Measured::Length { from, to, .. },
        ..
    } = &said.note
    else {
        panic!("a length: {said:?}");
    };
    close(from.distance(*to), 0.5, 1e-9);
    // Cut off 0.3 instead: said as a length across a face from the edge.
    let mut b = Standalone::new();
    base(&mut b)?;
    workpiece_ops_blend::Chamfer {
        id: "chamfer".into(),
        args: workpiece_ops_blend::ChamferArgs {
            edges: workpiece_core::Reference::each(edges),
            distance: Measure::new(0.3),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    assert_eq!(b.world.annotations["chamfer/distance"].text(), "0,3");
    // A round rim cut off: said at its middle, and also where it turns
    // across each axis — where a view from the side sees the cut true.
    let mut b = Standalone::new();
    workpiece_ops_basic_shapes::AddCylinder {
        id: "post".into(),
        args: workpiece_ops_basic_shapes::AddCylinderArgs {
            frame: Some(Reference::datum(ORIGIN)),
            radius: Measure::new(2.0),
            height: Measure::new(3.0),
        },
    }
    .apply(&mut b)?;
    workpiece_ops_blend::Chamfer {
        id: "chamfer".into(),
        args: workpiece_ops_blend::ChamferArgs {
            edges: vec![Reference::topology("post/top-rim")],
            distance: Measure::new(0.3),
            ..Default::default()
        },
    }
    .apply(&mut b)?;
    let Note::Dimension {
        measured,
        elsewhere,
        ..
    } = &b.world.annotations["chamfer/distance"].note
    else {
        panic!("a dimension");
    };
    // (Where the rim turns across x and across y: the four points a
    // view from a side sees the cut at true.)
    let froms: Vec<_> = std::iter::once(measured)
        .chain(elsewhere)
        .filter_map(|m| match m {
            Measured::Length { from, .. } => Some(*from),
            _ => None,
        })
        .collect();
    for (x, y) in [(2.0, 0.0), (0.0, 2.0), (-2.0, 0.0), (0.0, -2.0)] {
        let wanted = workpiece_core::math::Point3::new(x, y, 3.0);
        assert!(
            froms.iter().any(|p| p.distance(wanted) < 1e-6),
            "{wanted:?} in {froms:?}"
        );
    }
    Ok(())
}
