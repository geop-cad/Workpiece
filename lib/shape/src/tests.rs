//! Quick checks of what is made here from solids alone. The thorough
//! ones are the conformance suite's (`bench/conformance`).

use workpiece_boolean::{Solid, block, measure};
use workpiece_core::math::Point3;

use crate::blend::Shape;

/// The edge of `solid` whose middle is at `p`.
fn edge_at(solid: &Solid, p: Point3) -> usize {
    let at = |e: &usize| solid.edges[*e].point(solid.edges[*e].middle()).distance(p);
    (0..solid.edges.len())
        .min_by(|a, b| at(a).total_cmp(&at(b)))
        .expect("an edge")
}

#[test]
fn chamfers_of_three_sizes_meet_at_a_corner() -> anyhow::Result<()> {
    let sides = [10.0, 12.0, 9.0];
    let solid = block(
        "box",
        Point3::ZERO,
        [Point3::X, Point3::Y, Point3::Z],
        sides,
    )?;
    let sizes = [1.0, 1.5, 2.0];
    let mut blends = Vec::new();
    for axis in 0..3 {
        let mut middle = Point3::from_array(sides);
        middle[axis] /= 2.0;
        let d = sizes[axis];
        let shape = Shape::Chamfer {
            first: d,
            second: d,
        };
        blends.push((edge_at(&solid, middle), shape));
    }
    let made = crate::blend_each(&solid, "chamfer", &blends)?;
    // Between what chamfers of the largest size leave, all three, and
    // what those of the smallest do.
    let all_at = |d: f64| {
        let along: f64 = sides.iter().map(|l| 0.5 * d * d * (l - d)).sum();
        sides.iter().product::<f64>() - along - 0.75 * d * d * d
    };
    let volume = measure(&made).volume;
    assert!(all_at(2.0) < volume && volume < all_at(1.0), "{volume}");
    // Away from the corner each edge is cut by its own size.
    let placer = workpiece_boolean::Placer::new(&made);
    let side = |p: [f64; 3]| placer.side(Point3::from_array(p), 1e-9);
    assert_eq!(side([5.0, 11.4, 8.4]), Some(workpiece_boolean::Side::In));
    assert_eq!(side([5.0, 11.6, 8.6]), Some(workpiece_boolean::Side::Out));
    assert_eq!(side([9.1, 11.1, 4.0]), Some(workpiece_boolean::Side::Out));
    // One too wide for its face is refused.
    blends[0].1 = Shape::Chamfer {
        first: 30.0,
        second: 30.0,
    };
    assert!(crate::blend_each(&solid, "chamfer", &blends).is_err());
    Ok(())
}

#[test]
fn a_side_turned_about_its_lower_edge_leans() -> anyhow::Result<()> {
    let solid = block(
        "box",
        Point3::ZERO,
        [Point3::X, Point3::Y, Point3::Z],
        [4.0, 3.0, 2.0],
    )?;
    let side = (0..solid.faces.len())
        .find(|&f| workpiece_boolean::lies_on(&solid, f, Point3::new(4.0, 1.5, 1.0), 1e-9))
        .expect("the side");
    // About the edge along y through (4, 0, 0), its top inwards by one.
    let (sin, cos) = libm::sincos(libm::atan(0.5_f64));
    let turn = |p: Point3| {
        let (x, z) = (p.x - 4.0, p.z);
        Point3::new(4.0 + x * cos - z * sin, p.y, x * sin + z * cos)
    };
    let leant = crate::faces_placed(&solid, &[side], &turn)?;
    // A wedge 1 by 2 off the side, 3 long, is gone.
    let volume = measure(&leant).volume;
    assert!((volume - (24.0 - 3.0)).abs() < 1e-9, "{volume}");
    // Shifted only, it is moved as it is.
    let wider = crate::faces_placed(&solid, &[side], &|p| p + Point3::X)?;
    assert!((measure(&wider).volume - 30.0).abs() < 1e-9);
    Ok(())
}

#[test]
fn a_round_on_one_edge_is_made_where_it_is() -> anyhow::Result<()> {
    use crate::blend::{By, Section};
    use crate::roll::Rolled;
    // A 2 × 3 × 4 block, its edge along z at x = 2, y = 3 rounded by
    // half: no tool is cut from it, and it is what a tool leaves.
    let solid = block(
        "box",
        Point3::ZERO,
        [Point3::X, Point3::Y, Point3::Z],
        [2.0, 3.0, 4.0],
    )?;
    let edge = edge_at(&solid, Point3::new(2.0, 3.0, 2.0));
    let shape = Shape::Fillet {
        by: By::Radius,
        size: 0.5,
        until: None,
        section: Section::Circular,
        rho: 0.5,
    };
    let found = vec![(edge, Rolled::of(&solid, edge, &shape)?)];
    let made = crate::local::blended(&solid, "round", &found, &shape, 0.05, 29.0_f64.sqrt())
        .expect("made where it is");
    assert_eq!(made.faces.len(), 7);
    let lost = 0.25 * (1.0 - std::f64::consts::FRAC_PI_4) * 4.0;
    assert!((measure(&made).volume - (24.0 - lost)).abs() < 1e-9);
    let whole = crate::blend_chains(&solid, "round", &[edge], &shape, true)?;
    assert!((measure(&whole).volume - (24.0 - lost)).abs() < 1e-9);
    // Three edges at a corner: closed by an eighth of a ball, laid
    // there as well.
    let more: Vec<_> = [
        Point3::new(2.0, 3.0, 2.0),
        Point3::new(2.0, 1.5, 4.0),
        Point3::new(1.0, 3.0, 4.0),
    ]
    .into_iter()
    .map(|p| edge_at(&solid, p))
    .map(|e| Rolled::of(&solid, e, &shape).map(|r| (e, r)))
    .collect::<anyhow::Result<_>>()?;
    let size = 29.0_f64.sqrt();
    let made = crate::local::blended(&solid, "round", &more, &shape, 0.05, size).expect("made");
    assert_eq!(made.faces.len(), 10);
    let lost = 0.25 * (1.0 - std::f64::consts::FRAC_PI_4) * (3.5 + 2.5 + 1.5)
        + 0.125 * (1.0 - std::f64::consts::FRAC_PI_6);
    let volume = measure(&made).volume;
    assert!((volume - (24.0 - lost)).abs() < 1e-9, "{volume}");
    Ok(())
}

#[test]
fn faces_move_and_walls_shell_where_they_are() -> anyhow::Result<()> {
    // A 2 × 3 × 4 box, turned so that nothing of it lies along an axis.
    let turned = |p: Point3| {
        let (s, c) = libm::sincos(0.7_f64);
        let q = Point3::new(p.x * c - p.y * s, p.x * s + p.y * c, p.z);
        Point3::new(q.x, q.y * c - q.z * s, q.y * s + q.z * c) + Point3::new(5.0, -3.0, 2.0)
    };
    let straight = block(
        "box",
        Point3::ZERO,
        [Point3::X, Point3::Y, Point3::Z],
        [2.0, 3.0, 4.0],
    )?;
    // (Its top: the face all at z = 4.)
    let top = (0..straight.faces.len())
        .find(|&f| {
            let mut steps = straight.faces[f].loops.iter().flatten();
            steps.all(|step| {
                let uv = straight.start(step);
                (straight.faces[f].surface.point_at(uv.x, uv.y).z - 4.0).abs() < 1e-12
            })
        })
        .expect("a top");
    for solid in [straight.clone(), straight.moved(&turned)?] {
        let size = 29.0_f64.sqrt();
        let name = solid.faces[top].name.clone();
        // Its top pushed out by a half: made up as it was, and larger
        // by the top's area times that; pushed in, smaller.
        let out = crate::tweak::faces_off_same(&solid, &[top], &|_| 0.5, size).expect("made");
        assert_eq!((out.faces.len(), out.edges.len()), (6, 12));
        assert!((measure(&out).volume - 27.0).abs() < 1e-9);
        assert_eq!(out.faces[top].name, name);
        let inward = crate::thick::faces_offset(&solid, &[top], -1.0)?;
        assert!((measure(&inward).volume - 18.0).abs() < 1e-9);
        // Pushed in through its bottom, it is not this body any more.
        assert!(crate::tweak::faces_off_same(&solid, &[top], &|_| -5.0, size).is_none());
        // Its shell a quarter thick, open at the top: the box less the
        // hollow, which reaches the top.
        let shell = crate::thick::thickened(&solid, -0.25, Some(&[top]), false)?;
        let hollow = 1.5 * 2.5 * 3.75;
        assert!((measure(&shell).volume - (24.0 - hollow)).abs() < 1e-9);
        assert_eq!(shell.faces.len(), 11);
        let made = crate::tweak::shrunk_same(&solid, 0.25, Some(&[top]), size).expect("made");
        assert!((measure(&made).volume - (24.0 - hollow)).abs() < 1e-9);
        // Closed all around, the hollow is a box of its own inside.
        let closed = crate::thick::thickened(&solid, -0.25, Some(&[]), false)?;
        assert!((measure(&closed).volume - (24.0 - 1.5 * 2.5 * 3.5)).abs() < 1e-9);
        assert_eq!(closed.shells().len(), 2);
        // Walls as thick as half the box leave nothing to hollow: not
        // made here (and refused by what it is left to).
        assert!(crate::tweak::shrunk_same(&solid, 1.0, Some(&[]), size).is_none());
    }
    Ok(())
}

#[test]
fn edges_that_lean_are_rounded_past_where_their_faces_end() -> anyhow::Result<()> {
    use crate::blend::{By, Section};
    // A 12 × 14 × 2 block, its four sides leant in by 10° about its
    // bottom: the edges between them lean, and a ball rolled along one
    // rests, at its ends, on the planes of the sides past where the
    // sides themselves end.
    let solid = block(
        "box",
        Point3::ZERO,
        [Point3::X, Point3::Y, Point3::Z],
        [12.0, 14.0, 2.0],
    )?;
    let sides: Vec<usize> = [
        Point3::new(0.0, 7.0, 1.0),
        Point3::new(12.0, 7.0, 1.0),
        Point3::new(6.0, 0.0, 1.0),
        Point3::new(6.0, 14.0, 1.0),
    ]
    .into_iter()
    .map(|p| {
        (0..solid.faces.len())
            .find(|&f| workpiece_boolean::lies_on(&solid, f, p, 1e-9))
            .expect("a side")
    })
    .collect();
    let lean = 10.0_f64.to_radians();
    let leant = crate::thick::drafted(&solid, &sides, Point3::Z, Point3::ZERO, lean, false)?;
    let before = measure(&leant).volume;
    assert!(before < 12.0 * 14.0 * 2.0 - 1.0, "{before}");
    let off = libm::tan(lean);
    let corners: Vec<usize> = [
        (off, off),
        (12.0 - off, off),
        (off, 14.0 - off),
        (12.0 - off, 14.0 - off),
    ]
    .into_iter()
    .map(|(x, y)| edge_at(&leant, Point3::new(x, y, 1.0)))
    .collect();
    let shape = Shape::Fillet {
        by: By::Radius,
        size: 2.0,
        until: None,
        section: Section::Circular,
        rho: 0.5,
    };
    let round = crate::blend_chains(&leant, "round", &corners, &shape, true)?;
    // About (1 − π/4) r² off each corner, along its 2 of height.
    let lost = before - measure(&round).volume;
    let about = 4.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 2.0;
    assert!(
        lost > 0.85 * about && lost < 1.15 * about,
        "{lost} of {about}"
    );
    Ok(())
}

#[test]
fn the_walls_of_a_shell_meet_sharply_inside_an_inside_edge() -> anyhow::Result<()> {
    use workpiece_boolean::{Op, combine};
    // A flange 6 × 5 × 1 with a house 4 × 3 × 2 on it, hollowed from
    // below to walls 3/8 thick: the hollow is two boxes, sharp where
    // the house's walls meet the flange's top from inside.
    let axes = [Point3::X, Point3::Y, Point3::Z];
    let flange = block(
        "flange",
        Point3::new(-3.0, -2.5, 0.0),
        axes,
        [6.0, 5.0, 1.0],
    )?;
    let house = block("house", Point3::new(-2.0, -1.5, 1.0), axes, [4.0, 3.0, 2.0])?;
    let solid = combine(Op::Union, &flange, &house)?.solid;
    let bottom = (0..solid.faces.len())
        .find(|&f| workpiece_boolean::lies_on(&solid, f, Point3::ZERO, 1e-9))
        .expect("the bottom");
    let t = 0.375;
    let hollow = crate::thick::shelled(&solid, t, &[bottom])?;
    let cavity =
        (6.0 - 2.0 * t) * (5.0 - 2.0 * t) * (1.0 - t) + (4.0 - 2.0 * t) * (3.0 - 2.0 * t) * 2.0;
    let volume = measure(&hollow).volume;
    assert!((volume - (54.0 - cavity)).abs() < 1e-9, "{volume}");
    Ok(())
}

#[test]
fn a_sweep_asks_for_no_section_past_its_ends() -> anyhow::Result<()> {
    // The mean of three knots all at the end comes to a hair past it for
    // some ends (1.6, 2.7, 62.9959390278628): a section asked for there is
    // past the end, where a blend runs on past its edge's end rather
    // than ends at it.
    let line = workpiece_core::world::topo::Curve3::line(Point3::ZERO, Point3::X)?;
    for to in [1.6, 2.7, 62.995_939_027_862_8] {
        let mut flat = |t: f64| -> anyhow::Result<Vec<[f64; 4]>> {
            anyhow::ensure!((0.0..=to).contains(&t), "asked for {t}, past {to}");
            Ok(vec![[t, 0.0, 0.0, 1.0], [t, 1.0, 0.0, 1.0]])
        };
        crate::sweep::along(&line, (0.0, to), 1e-9, to, &mut flat)?;
    }
    Ok(())
}

/// A wing standing on a plate: lofted from a root section below the
/// plate's top (z = 0) to a smaller tip section set back and up, each an
/// airfoil-ish outline of cubic pieces in its plane — the egg of four
/// quarters (`sharp` false: smooth all round, a tight round tail) or two
/// pieces that meet smoothly at the nose and at an angle at the tail —
/// and joined with the plate. The edges where the wing meets the plate.
fn wing_on_plate(sharp: bool) -> anyhow::Result<(workpiece_boolean::Solid, Vec<usize>)> {
    use workpiece_core::world::topo::Curve3;
    let cubic = |p: [Point3; 4]| {
        Curve3::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            p.to_vec(),
            None,
        )
    };
    // In the plane z, the nose at x = at, `scale` times as large.
    let section = |z: f64, at: f64, scale: f64| -> anyhow::Result<Vec<Curve3>> {
        let p = |x: f64, y: f64| Point3::new(at + x * scale, y * scale, z);
        let (front, back, half) = (8.0, 24.0, 3.0);
        let k = 0.552_284_75;
        Ok(if sharp {
            vec![
                cubic([
                    p(0.0, 0.0),
                    p(0.0, half * 1.3),
                    p(back * 0.4, half * 1.3),
                    p(back, 0.0),
                ])?,
                cubic([
                    p(back, 0.0),
                    p(back * 0.4, -half * 1.1),
                    p(0.0, -half * 1.1),
                    p(0.0, 0.0),
                ])?,
            ]
        } else {
            // An egg about (front, 0): a short nose, a long tail.
            let c = |x: f64, y: f64| p(front + x, y);
            vec![
                cubic([
                    c(-front, 0.0),
                    c(-front, half * k),
                    c(-front * k, half),
                    c(0.0, half),
                ])?,
                cubic([
                    c(0.0, half),
                    c(back * k, half),
                    c(back, half * k),
                    c(back, 0.0),
                ])?,
                cubic([
                    c(back, 0.0),
                    c(back, -half * k),
                    c(back * k, -half),
                    c(0.0, -half),
                ])?,
                cubic([
                    c(0.0, -half),
                    c(-front * k, -half),
                    c(-front, -half * k),
                    c(-front, 0.0),
                ])?,
            ]
        })
    };
    let wing = crate::swept::through_sections(
        "wing",
        &[section(-2.0, 0.0, 1.0)?, section(30.0, 6.0, 0.7)?],
        &Default::default(),
    )?;
    let plate = block(
        "plate",
        Point3::new(-20.0, -20.0, -6.0),
        [Point3::X, Point3::Y, Point3::Z],
        [80.0, 40.0, 6.0],
    )?;
    let solid = workpiece_boolean::combine(workpiece_boolean::Op::Union, &plate, &wing)?.solid;
    // (On the plate's top, inside its rim.)
    let junction = (0..solid.edges.len())
        .filter(|&e| {
            let edge = &solid.edges[e];
            let (t0, t1) = edge.range;
            (0..=4).all(|k| {
                let p = edge.point(t0 + (t1 - t0) * f64::from(k) / 4.0);
                p.z.abs() < 1e-6 && p.x > -15.0 && p.x < 55.0 && p.y.abs() < 15.0
            })
        })
        .collect();
    Ok((solid, junction))
}

/// What a solid's checker finds wrong with it, written to a topology.
fn faults(solid: &workpiece_boolean::Solid) -> Vec<String> {
    let mut topo = workpiece_core::world::topo::Topo::new();
    if let Err(e) = workpiece_boolean::write(&mut topo, solid, "solid") {
        return vec![e.to_string()];
    }
    topo.validate()
        .err()
        .map(|all| all.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}

fn round(size: f64) -> Shape {
    Shape::Fillet {
        by: crate::blend::By::Radius,
        size,
        until: None,
        section: crate::blend::Section::Circular,
        rho: 0.5,
    }
}

#[test]
fn a_wing_is_rounded_where_it_meets_the_plate() -> anyhow::Result<()> {
    let (solid, junction) = wing_on_plate(false)?;
    // The egg's four pieces, each running on smoothly into the next.
    assert_eq!(junction.len(), 4);
    let before = measure(&solid).volume;
    // One of them alone: it ends square at the vertices where the next
    // one goes on (cut back to the plane through each, square to it).
    let one = crate::blend_solid(&solid, "fillet", &junction[..1], &round(1.0), true)?;
    assert_eq!(faults(&one), Vec::<String>::new());
    let added = measure(&one).volume - before;
    assert!(added > 0.0, "{added}");
    // All four: one round all the way round, made as one tool. What it
    // adds is about what a quarter round in a square corner holds,
    // (1 − π/4) r² — the wing's side stands up from the plate at a
    // slant — times the outline's length.
    let all = crate::blend_solid(&solid, "fillet", &junction, &round(1.0), true)?;
    assert_eq!(faults(&all), Vec::<String>::new());
    let length: f64 = junction
        .iter()
        .map(|&e| {
            let edge = &solid.edges[e];
            let (t0, t1) = edge.range;
            (0..64)
                .map(|k| {
                    let at = |k: usize| edge.point(t0 + (t1 - t0) * k as f64 / 64.0);
                    at(k).distance(at(k + 1))
                })
                .sum::<f64>()
        })
        .sum();
    let added = measure(&all).volume - before;
    let corner = (1.0 - std::f64::consts::FRAC_PI_4) * length;
    assert!(
        added > corner * 0.7 && added < corner * 1.5,
        "{added} {corner}"
    );
    // Mirrored, the same.
    let mirror = |p: Point3| Point3::new(p.x, -p.y, p.z);
    let mirrored = solid.mapped(&mirror, true, 1.0)?;
    let theirs = crate::blend_solid(&mirrored, "fillet", &junction, &round(1.0), true)?;
    let delta = measure(&theirs).volume - measure(&all).volume;
    assert!(delta.abs() < 1e-6 * before, "{delta}");
    Ok(())
}

#[test]
fn a_sharp_tailed_wing_is_rounded_all_round_and_its_tip_refused_when_too_large()
-> anyhow::Result<()> {
    let (solid, junction) = wing_on_plate(true)?;
    // Smooth at the nose, at an angle at the tail: the one round runs
    // from the tail round to the tail, and is cut back at the plane
    // halfway between its two ends there.
    assert!(junction.len() >= 2);
    let before = measure(&solid).volume;
    let made = crate::blend_solid(&solid, "fillet", &junction, &round(1.0), true)?;
    assert_eq!(faults(&made), Vec::<String>::new());
    assert!(measure(&made).volume > before);
    // The egg's tail bends tighter than 2 at the tip: a round of its top
    // edge that large would cut into itself there, and is refused,
    // saying where.
    let (egg, _) = wing_on_plate(false)?;
    let tip: Vec<usize> = (0..egg.edges.len())
        .filter(|&e| {
            let edge = &egg.edges[e];
            let middle = edge.point(edge.middle());
            (middle.z - 30.0).abs() < 1e-6
        })
        .collect();
    let tail = tip
        .iter()
        .copied()
        .max_by(|&a, &b| {
            let x = |e: usize| egg.edges[e].point(egg.edges[e].middle()).x;
            x(a).total_cmp(&x(b))
        })
        .expect("a tip edge");
    let why = crate::blend_solid(&egg, "fillet", &[tail], &round(2.0), true)
        .err()
        .map(|e| format!("{e:#}"))
        .unwrap_or_default();
    assert!(
        why.contains("bends more tightly") && why.contains("near ("),
        "{why}"
    );
    Ok(())
}
