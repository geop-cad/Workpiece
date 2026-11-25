//! Cutting paths: the outline of a part where a plane halfway up it
//! cuts it, as G-code for a machine that cuts sheet along a path — a
//! laser, a plasma torch, a water jet.

use std::fmt::Write;

use workpiece_boolean::model::Walk;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::World;
use workpiece_core::world::topo::BodyKind;
use workpiece_core::{Pose, Unit};

/// The outlines of what the plane square to z through `middle` cuts
/// through the solid bodies of `world` (and of what is placed in it,
/// where `pose` puts it): each a closed run of points in x and y.
fn outlines(world: &World, pose: Pose, middle: Point3, far: f64, out: &mut Vec<Vec<Point2>>) {
    let back = pose.inverse();
    let (origin, normal) = (
        back.transform_point(middle),
        back.transform_vector(Point3::Z),
    );
    let x = normal.any_orthonormal_vector();
    let y = normal.cross(x);
    let corner = origin - (x + y + normal) * far;
    let below = workpiece_boolean::block(
        "section",
        corner,
        [x, y, normal],
        [far * 2.0, far * 2.0, far],
    );
    let topo = &world.topo;
    for (id, body) in topo.bodies().iter() {
        if body.kind != BodyKind::Solid {
            continue;
        }
        let (Ok(solid), Ok(below)) = (workpiece_boolean::read(topo, id), &below) else {
            continue;
        };
        let Ok(cut) = workpiece_boolean::combine(workpiece_boolean::Op::Intersect, &solid, below)
        else {
            continue;
        };
        let cut = cut.solid;
        for face in &cut.faces {
            if !face
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with("section/f1"))
            {
                continue;
            }
            for steps in &face.loops {
                let mut run: Vec<Point2> = Vec::new();
                for step in steps {
                    let Walk::Edge { edge, reversed } = step.walk else {
                        continue;
                    };
                    let e = &cut.edges[edge];
                    let straight = e.curve.degree() == 1 && e.curve.control_points().len() == 2;
                    let pieces = if straight { 1 } else { 48 };
                    let (from, to) = if reversed {
                        (e.range.1, e.range.0)
                    } else {
                        e.range
                    };
                    for k in 0..pieces {
                        let t = from + (to - from) * f64::from(k) / f64::from(pieces);
                        run.push(pose.transform_point(e.curve.point_at(t)).truncate());
                    }
                }
                if run.len() >= 3 {
                    out.push(run);
                }
            }
        }
    }
    for instance in world.instances.values() {
        outlines(&instance.world, pose * instance.pose, middle, far, out);
    }
}

/// A box around the solid bodies of `world`, where `pose` puts them.
fn extent(world: &World, pose: Pose, into: &mut Option<(Point3, Point3)>) {
    for (id, body) in world.topo.bodies().iter() {
        if body.kind != BodyKind::Solid {
            continue;
        }
        let Ok(solid) = workpiece_boolean::read(&world.topo, id) else {
            continue;
        };
        for v in &solid.vertices {
            let p = pose.transform_point(v.point);
            *into = Some(into.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
        }
    }
    for instance in world.instances.values() {
        extent(&instance.world, pose * instance.pose, into);
    }
}

/// G-code that cuts `world` out of a sheet: along every outline of what
/// a plane halfway up it cuts through it — holes first, then what is
/// around them — with the beam (or the torch, or the jet) on along an
/// outline and off between them, at `feed` units a minute. The path is
/// the outline itself: nothing is allowed for the width of the cut.
pub fn cut_path(world: &World, unit: Unit, feed: f64) -> anyhow::Result<String> {
    let mut around = None;
    extent(world, Pose::IDENTITY, &mut around);
    let (lo, hi) = around.ok_or_else(|| anyhow::anyhow!("there is no solid body to cut out"))?;
    let far = (hi - lo).length() * 2.0 + lo.length() + hi.length();
    let mut runs = Vec::new();
    outlines(world, Pose::IDENTITY, (lo + hi) / 2.0, far, &mut runs);
    anyhow::ensure!(
        !runs.is_empty(),
        "the plane halfway up cuts through nothing"
    );
    // The smallest first: a hole is cut before what it is a hole in
    // comes loose.
    let size = |run: &Vec<Point2>| {
        let (lo, hi) = run.iter().fold(
            (
                Point2::splat(f64::INFINITY),
                Point2::splat(f64::NEG_INFINITY),
            ),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        (hi - lo).length()
    };
    runs.sort_by(|a, b| size(a).total_cmp(&size(b)));
    let units = match unit {
        Unit::Inch => "G20 (inches)".to_owned(),
        Unit::Millimetre => "G21 (millimetres)".to_owned(),
        // (A machine counts in millimetres or inches: other units are
        // written as millimetres.)
        other => format!("G21 (millimetres, from {})", other.label()),
    };
    let scale = match unit {
        Unit::Inch | Unit::Millimetre => 1.0,
        other => other.millimetres(),
    };
    let mut out = format!("(cut path, written by Workpiece)\n{units}\nG90\nM5\n");
    for run in &runs {
        let at = |p: &Point2| format!("X{:.4} Y{:.4}", p.x * scale, p.y * scale);
        let _ = writeln!(out, "G0 {}", at(&run[0]));
        out.push_str("M3\n");
        for (k, p) in run.iter().enumerate().skip(1) {
            if k == 1 {
                let _ = writeln!(out, "G1 {} F{:.1}", at(p), feed * scale);
            } else {
                let _ = writeln!(out, "G1 {}", at(p));
            }
        }
        let _ = writeln!(out, "G1 {}", at(&run[0]));
        out.push_str("M5\n");
    }
    out.push_str("M2\n");
    Ok(out)
}

/// G-code that mills `world` out of a plate, around every outline of
/// what a plane halfway up it cuts through it: from its top down to
/// its bottom in passes `step_down` deep, with a tool `tool` across
/// that the machine keeps to the waste side of the outline (cutter
/// compensation to the right, `G42`: outlines are walked with what is
/// kept to their left) — holes first, at `feed` units a minute,
/// plunging at a third of it. Sizes are in `unit`; a part in
/// centimetres or metres is written in millimetres.
pub fn mill_path(
    world: &World,
    unit: Unit,
    feed: f64,
    tool: f64,
    step_down: f64,
) -> anyhow::Result<String> {
    anyhow::ensure!(
        tool > 0.0 && step_down > 0.0,
        "the tool has a size, a pass a depth"
    );
    let mut around = None;
    extent(world, Pose::IDENTITY, &mut around);
    let (lo, hi) = around.ok_or_else(|| anyhow::anyhow!("there is no solid body to mill out"))?;
    let far = (hi - lo).length() * 2.0 + lo.length() + hi.length();
    let mut runs = Vec::new();
    outlines(world, Pose::IDENTITY, (lo + hi) / 2.0, far, &mut runs);
    anyhow::ensure!(
        !runs.is_empty(),
        "the plane halfway up cuts through nothing"
    );
    let size = |run: &Vec<Point2>| {
        let (lo, hi) = run.iter().fold(
            (
                Point2::splat(f64::INFINITY),
                Point2::splat(f64::NEG_INFINITY),
            ),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        (hi - lo).length()
    };
    runs.sort_by(|a, b| size(a).total_cmp(&size(b)));
    let (units, scale) = match unit {
        Unit::Inch => ("G20 (inches)".to_owned(), 1.0),
        Unit::Millimetre => ("G21 (millimetres)".to_owned(), 1.0),
        other => (
            format!("G21 (millimetres, from {})", other.label()),
            other.millimetres(),
        ),
    };
    let passes = ((hi.z - lo.z) / step_down - 1e-9).ceil().max(1.0) as usize;
    let clear = (hi.z + (hi.z - lo.z).max(tool)) * scale;
    let mut out = format!(
        "(contour milling, written by Workpiece)\n(tool {:.3} across, in D1; {passes} passes of {:.3})\n{units}\nG90 G17\nG0 Z{clear:.4}\nM3\n",
        tool * scale,
        (hi.z - lo.z) / passes as f64 * scale,
    );
    for run in &runs {
        let at = |p: &Point2| format!("X{:.4} Y{:.4}", p.x * scale, p.y * scale);
        // In from beside the outline's start, the tool's width out on
        // the waste side: the machine needs a move to take its place.
        let way = (run[1] - run[0]).normalize_or_zero();
        let lead = run[0] - way * tool + Point2::new(way.y, -way.x) * tool;
        let _ = writeln!(out, "G0 {}", at(&lead));
        for pass in 1..=passes {
            let z = (hi.z - (hi.z - lo.z) * pass as f64 / passes as f64) * scale;
            let _ = writeln!(out, "G1 Z{z:.4} F{:.1}", feed * scale / 3.0);
            let _ = writeln!(out, "G42 D1 G1 {} F{:.1}", at(&run[0]), feed * scale);
            for p in &run[1..] {
                let _ = writeln!(out, "G1 {}", at(p));
            }
            let _ = writeln!(out, "G1 {}", at(&run[0]));
            let _ = writeln!(out, "G40 G1 {}", at(&lead));
        }
        let _ = writeln!(out, "G0 Z{clear:.4}");
    }
    out.push_str("M5\nM2\n");
    Ok(out)
}
