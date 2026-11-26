//! Flange: sheet metal bent up along a straight edge of a plate — a
//! bend of a radius through an angle, and a flange on from it.

use anyhow::{Context, ensure};
use workpiece_boolean::model::Walk;
use workpiece_boolean::{Op, Solid, combine, face_area, read};
use workpiece_core::math::Point2;
use workpiece_core::math::from_angle;
use workpiece_core::world::topo::Curve3;
use workpiece_core::{Builder, Measure, Operation};

use workpiece_shape::swept::{Outline, Sides, Weighted, steady, swept};

/// The edge bent along, and the bend's and the flange's sizes.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct FlangeArgs {
    /// The straight edge: between the plate's face the flange rises
    /// from and the plate's end.
    pub edge: Option<workpiece_core::Reference<workpiece_core::world::topo::EdgeId>>,
    /// How thick the sheet is.
    pub thickness: Measure,
    /// The radius of the bend, on its inside.
    #[arg(label = "Bend radius", row)]
    pub radius: Measure,
    /// The angle the sheet is bent through, in degrees.
    #[arg(label = "Angle °")]
    pub angle: Measure,
    /// How long the flange is, on from the bend.
    #[arg(row)]
    pub length: Measure,
    /// Whether the flange rises from the other face at the edge.
    #[serde(default)]
    #[arg(flip, row)]
    pub flip: bool,
}

impl Default for FlangeArgs {
    fn default() -> Self {
        Self {
            edge: None,
            thickness: Measure::new(2.0),
            radius: Measure::new(2.0),
            angle: Measure::new(90.0),
            length: Measure::new(10.0),
            flip: false,
        }
    }
}

/// Bends a flange up along an edge of a plate: the body runs on from
/// its end around a bend and into a flat flange, as thick as the sheet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Flange {
    /// The operation's id.
    pub id: String,
    /// The edge and the sizes.
    pub args: FlangeArgs,
}

/// How long the flat sheet is that a bend of inside `radius` through
/// `angle` (radians) and a flange `length` long are made of: the
/// flange, and the bend along the layer that neither stretches nor
/// shrinks, a share `k` of the `thickness` in from the inside (0.44 for
/// mild steel bent in air).
pub fn flat_length(radius: f64, thickness: f64, angle: f64, length: f64, k: f64) -> f64 {
    length + angle * (radius + k * thickness)
}

/// The arc about `centre` of `radius` from angle `from` to `to`, as
/// sides of an outline: pieces of a quarter turn at most.
fn arc(centre: Point2, radius: f64, from: f64, to: f64, sides: &mut Sides, points: &mut Weighted) {
    let pieces = ((to - from).abs() / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let step = (to - from) / pieces as f64;
    for k in 0..pieces {
        let a = from + step * k as f64;
        let w = libm::cos(step / 2.0);
        sides.push((2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 3));
        points.extend([
            (centre + from_angle(a) * radius, 1.0),
            (centre + from_angle(a + step / 2.0) * (radius / w), w),
            (centre + from_angle(a + step) * radius, 1.0),
        ]);
    }
}

/// `solid` with a flange bent up along its edge `edge`, rising from the
/// larger of the two flat faces at it (or, with `flip`, the other): a
/// bend of inside `radius` through `angle` (radians), then `length` of
/// flat sheet, `thickness` thick.
pub fn flanged(
    solid: &Solid,
    edge: usize,
    (thickness, radius, angle, length): (f64, f64, f64, f64),
    flip: bool,
    id: &str,
) -> anyhow::Result<Solid> {
    ensure!(
        thickness > 0.0 && radius > 0.0,
        "the sheet has a thickness, the bend a radius"
    );
    ensure!(
        angle > 0.0 && angle < std::f64::consts::PI,
        "the sheet is bent through more than nothing and less than half a turn"
    );
    ensure!(length >= 0.0, "the flange is not shorter than nothing");
    let it = &solid.edges[edge];
    let (from, to) = (it.curve.point_at(it.range.0), it.curve.point_at(it.range.1));
    let middle = it.curve.point_at((it.range.0 + it.range.1) / 2.0);
    let long = from.distance(to);
    ensure!(
        long > 0.0 && middle.distance((from + to) / 2.0) <= long * 1e-9,
        "a flange is bent along a straight edge"
    );
    // The two faces at the edge, and which way each faces there.
    let mut beside = Vec::new();
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if matches!(step.walk, Walk::Edge { edge: e, .. } if e == edge) {
                let uv = step.pcurve.point_at((it.range.0 + it.range.1) / 2.0);
                let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
                let out = du.cross(dv).normalize_or_zero() * if face.outward { 1.0 } else { -1.0 };
                beside.push((f, out));
            }
        }
    }
    let [mut a, mut b] = beside[..] else {
        anyhow::bail!("the edge is between two faces");
    };
    if (face_area(solid, a.0) < face_area(solid, b.0)) != flip {
        std::mem::swap(&mut a, &mut b);
    }
    // The flange rises from face a, out past face b: the plate's end.
    let (up, out) = (a.1, b.1);
    ensure!(
        up.dot(out).abs() <= 1e-9,
        "the plate's end is square to the face the flange rises from"
    );
    // The section, with x out past the end and y up from the face: the
    // bend about a middle the radius above the edge, and the flange on
    // from it; around it with what it is of to the left.
    let centre = Point2::new(0.0, radius);
    let start = -std::f64::consts::FRAC_PI_2;
    let end = start + angle;
    let along = from_angle(angle) * length;
    let (mut sides, mut points): (Sides, Weighted) = (Vec::new(), Vec::new());
    let line = |a: Point2, b: Point2, sides: &mut Sides, points: &mut Weighted| {
        if a.distance(b) > 0.0 {
            sides.push((1, vec![0.0, 0.0, 1.0, 1.0], 2));
            points.extend([(a, 1.0), (b, 1.0)]);
        }
    };
    let outer = radius + thickness;
    let at = |far: f64, angle: f64| centre + from_angle(angle) * far;
    line(Point2::ZERO, at(outer, start), &mut sides, &mut points);
    arc(centre, outer, start, end, &mut sides, &mut points);
    line(
        at(outer, end),
        at(outer, end) + along,
        &mut sides,
        &mut points,
    );
    line(
        at(outer, end) + along,
        at(radius, end) + along,
        &mut sides,
        &mut points,
    );
    line(
        at(radius, end) + along,
        at(radius, end),
        &mut sides,
        &mut points,
    );
    arc(centre, radius, end, start, &mut sides, &mut points);
    // Along the edge, the way that has the end to the right of up.
    let way = out.cross(up);
    let (first, last) = if (to - from).dot(way) > 0.0 {
        (from, to)
    } else {
        (to, from)
    };
    let path = Curve3::line(first, last)?;
    let held = steady(&points);
    let outline = Outline { sides, at: &held };
    let reach = long + radius + thickness + length;
    let bent = swept(id, &outline, &path, path.domain(), up, reach * 4.0)?;
    Ok(combine(Op::Union, solid, &bent)
        .context("joining the flange to the plate")?
        .solid)
}

impl Operation for Flange {
    type Args = FlangeArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &FlangeArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut FlangeArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let picked = args
            .edge
            .as_ref()
            .context("pick the edge to bend along")?
            .name();
        let sizes = (
            (*builder).measure(&args.thickness, "the thickness")?,
            (*builder).measure(&args.radius, "the radius")?,
            (*builder).measure(&args.angle, "the angle")?.to_radians(),
            (*builder).measure(&args.length, "the length")?,
        );
        let topo = &builder.tracked().topo;
        for (body, name) in workpiece_shape::bodies::solids_with(topo, &[picked]) {
            let solid = read(topo, body)?;
            let found = solid.edges.iter().position(|e| {
                e.name.as_deref() == Some(picked) || e.also.iter().any(|n| n == picked)
            });
            let Some(edge) = found else {
                continue;
            };
            let made = flanged(&solid, edge, sizes, args.flip, &self.id)
                .with_context(|| format!("bending a flange on {name}"))?;
            out.remove(topo, body);
            out.write(&made, &name);
            return Ok(out.made);
        }
        anyhow::bail!("there is no edge {picked:?} to bend along")
    }

    fn summary(&self) -> String {
        let a = &self.args;
        // (Flat, as mild steel bends in air.)
        let flat = flat_length(
            a.radius.value,
            a.thickness.value,
            a.angle.value.to_radians(),
            a.length.value,
            0.44,
        );
        format!("{} long at {}°, {flat:.3} flat", a.length, a.angle)
    }
}

/// A bend of a sheet laid out flat: its centre line on the flat
/// pattern's side the fixed face is on, and how it is bent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bend {
    /// Where its centre line starts and ends.
    pub line: [workpiece_core::math::Point3; 2],
    /// Through how much it bends, in radians.
    pub angle: f64,
    /// Its inside radius.
    pub radius: f64,
    /// Whether it bends up: towards the side the fixed face looks to.
    pub up: bool,
}

/// The flat sheet `solid` is bent from, laid out in the plane of its
/// face `fixed`: every flat face on that side of the sheet, turned back
/// about the bends between them, with each bend as long as its layer
/// that neither stretches nor shrinks (a share `k` of the `thickness`
/// in from the inside of the bend); and `thickness` thick. With it, its
/// bends where they lie flat.
pub fn unfolded(
    solid: &Solid,
    fixed: usize,
    thickness: f64,
    k: f64,
    id: &str,
) -> anyhow::Result<(Solid, Vec<Bend>)> {
    use std::collections::BTreeMap;
    use workpiece_core::math::Point3;
    ensure!(thickness > 0.0, "the sheet has a thickness");
    // Which way a face faces, where its edge `edge` is; `None` for a
    // face that is not flat.
    let facing = |f: usize, at: Option<usize>| -> Option<Point3> {
        let face = &solid.faces[f];
        let mut normals = Vec::new();
        for step in face.loops.iter().flatten() {
            let Walk::Edge { edge, .. } = step.walk else {
                continue;
            };
            let range = solid.edges[edge].range;
            let uv = step.pcurve.point_at((range.0 + range.1) / 2.0);
            let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
            let n = du.cross(dv).normalize_or_zero() * if face.outward { 1.0 } else { -1.0 };
            if at == Some(edge) {
                return Some(n);
            }
            normals.push(n);
        }
        let first = *normals.first()?;
        (at.is_none() && normals.iter().all(|n| n.dot(first) > 1.0 - 1e-9)).then_some(first)
    };
    // The faces at each edge.
    let mut beside: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                beside.entry(edge).or_default().push(f);
            }
        }
    }
    let straight = |e: usize| {
        let it = &solid.edges[e];
        let (a, b) = (it.curve.point_at(it.range.0), it.curve.point_at(it.range.1));
        let middle = it.curve.point_at((it.range.0 + it.range.1) / 2.0);
        (middle.distance((a + b) / 2.0) <= a.distance(b) * 1e-9).then_some((a, b))
    };
    ensure!(
        facing(fixed, None).is_some(),
        "the face the sheet is laid out from is flat"
    );
    // Each flat face with how it is carried to where it lies flat:
    // turned by an angle about an axis, then moved, one bend after the
    // other, from the last bend back to the first.
    type Carry = Vec<(Point3, Point3, f64, Point3)>;
    let carry = |steps: &Carry, mut p: Point3| {
        for &(origin, axis, angle, by) in steps.iter().rev() {
            let r = p - origin;
            let along = axis * r.dot(axis);
            let across = r - along;
            p = origin
                + along
                + across * libm::cos(angle)
                + axis.cross(across) * libm::sin(angle)
                + by;
        }
        p
    };
    let mut flat: BTreeMap<usize, Carry> = BTreeMap::from([(fixed, Vec::new())]);
    let mut todo = vec![fixed];
    let mut pieces: Vec<Solid> = Vec::new();
    let mut bends: Vec<Bend> = Vec::new();
    let fixed_up = facing(fixed, None).unwrap_or(Point3::Z);
    while let Some(f) = todo.pop() {
        let steps = flat[&f].clone();
        let here = facing(f, None).context("a face of the sheet that is not flat")?;
        let edges: Vec<usize> = solid.faces[f]
            .loops
            .iter()
            .flatten()
            .filter_map(|s| match s.walk {
                Walk::Edge { edge, .. } => Some(edge),
                Walk::Pole { .. } => None,
            })
            .collect();
        // Where the face is, about: to tell which way is out of it.
        let corners: Vec<Point3> = edges
            .iter()
            .map(|&e| solid.edges[e].curve.point_at(solid.edges[e].range.0))
            .collect();
        let middle = corners.iter().sum::<Point3>() / corners.len().max(1) as f64;
        for &edge in &edges {
            let Some((from, to)) = straight(edge) else {
                continue;
            };
            // A bend at this edge: a round face that leaves the flat
            // one the way it faces.
            let Some(&bend) = beside[&edge].iter().find(|&&g| g != f) else {
                continue;
            };
            let leaves = facing(bend, Some(edge)).is_some_and(|n| n.dot(here) > 1.0 - 1e-6);
            let Ok(round) = workpiece_shape::thread::cylinder(solid, bend) else {
                continue;
            };
            if !leaves || facing(bend, None).is_some() {
                continue;
            }
            // And the flat face at its other straight edge.
            let beyond = solid.faces[bend].loops.iter().flatten().find_map(|s| {
                let Walk::Edge { edge: other, .. } = s.walk else {
                    return None;
                };
                let next = *beside[&other].iter().find(|&&g| g != bend)?;
                let flat_too = facing(next, None)?;
                let tangent = facing(bend, Some(other))?.dot(flat_too) > 1.0 - 1e-6;
                (other != edge && straight(other).is_some() && tangent).then_some((next, flat_too))
            });
            let Some((next, there)) = beyond else {
                continue;
            };
            if flat.contains_key(&next) {
                continue;
            }
            // Turned back about the bend's axis until it faces as this
            // one does; then out past the edge by the bend's length.
            let angle = libm::acos(here.dot(there).clamp(-1.0, 1.0));
            let turned = |sign: f64| {
                let along = round.axis * there.dot(round.axis);
                let across = there - along;
                along
                    + across * libm::cos(sign * angle)
                    + round.axis.cross(across) * libm::sin(sign * angle)
            };
            let sign = if turned(1.0).dot(here) >= turned(-1.0).dot(here) {
                1.0
            } else {
                -1.0
            };
            let radius = round.radius.abs();
            let neutral = if round.hole {
                radius + k * thickness
            } else {
                radius - (1.0 - k) * thickness
            };
            let way = (to - from).normalize();
            let out = ((from + to) / 2.0 - middle)
                .reject_from_normalized(way)
                .normalize();
            let wide = angle * neutral;
            let mut onward = steps.clone();
            onward.push((round.origin, round.axis, sign * angle, out * wide));
            // The bend itself, flat: from the edge out by its length.
            let corner = from - here * thickness;
            let third = way.cross(out);
            // (A block's axes turn the right way around: its third is
            // the first two's cross.)
            let (first, second, up) = if third.dot(here) > 0.0 {
                (way, out, third)
            } else {
                (out, way, out.cross(way))
            };
            let sizes = if third.dot(here) > 0.0 {
                [from.distance(to), wide, thickness]
            } else {
                [wide, from.distance(to), thickness]
            };
            let strip = workpiece_boolean::block(
                &format!("{id}/bend{}", pieces.len()),
                corner,
                [first, second, up],
                sizes,
            )?;
            pieces.push(strip.moved(&|p| carry(&steps, p))?);
            // Its centre line, on the face's side of the sheet: up where
            // the next face turns towards the way this one faces.
            let half = out * (wide / 2.0);
            bends.push(Bend {
                line: [carry(&steps, from + half), carry(&steps, to + half)],
                angle,
                radius: if round.hole {
                    radius
                } else {
                    radius - thickness
                },
                // (The next face turns towards the way this one faces
                // where its normal leans back over this face; this one
                // faces as the fixed one does, laid out, or the other way.)
                up: (there.dot(out) < 0.0)
                    == ((carry(&steps, from + here) - carry(&steps, from)).dot(fixed_up) > 0.0),
            });
            flat.insert(next, onward);
            todo.push(next);
        }
    }
    // Each flat face as a plate behind it, where it lies flat.
    for (&f, steps) in &flat {
        let sheet = workpiece_shape::faces_of(solid, &[f]);
        let plate = workpiece_shape::thick::thicken(&sheet, &[0], -thickness, false, true)
            .context("a face of the sheet could not be given its thickness")?;
        pieces.push(plate.moved(&|p| carry(steps, p))?);
    }
    let mut whole = pieces.pop().context("there is nothing to lay out")?;
    for piece in pieces.iter().rev() {
        whole = combine(Op::Union, &whole, piece)
            .context("joining the flat pieces")?
            .solid;
    }
    Ok((whole, bends))
}

/// The face laid out from, and the sheet it is of.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct UnfoldArgs {
    /// The flat face that stays where it is: the others are laid out
    /// in its plane.
    #[arg(label = "Fixed face")]
    pub face: Option<workpiece_core::Reference<workpiece_core::world::topo::FaceId>>,
    /// How thick the sheet is.
    pub thickness: Measure,
    /// Where in the sheet's thickness a bend neither stretches nor
    /// shrinks: a share of it, from the inside of the bend.
    #[arg(label = "K-factor", row)]
    pub k: Measure,
}

impl Default for UnfoldArgs {
    fn default() -> Self {
        Self {
            face: None,
            thickness: Measure::new(0.2),
            k: Measure::new(0.44),
        }
    }
}

/// Lays a bent sheet out flat: a new body, named after the operation's
/// id, that is the sheet before it was bent — its flat pattern — in the
/// plane of the picked face; and the centre line of each bend on it, a
/// datum axis `{id}/bend1`, … (what a drawing of the flat pattern draws
/// as its bend lines). The bent body stays as it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Unfold {
    /// The operation's id; also the flat body's name.
    pub id: String,
    /// The face and the sheet.
    pub args: UnfoldArgs,
}

impl Operation for Unfold {
    type Args = UnfoldArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &UnfoldArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut UnfoldArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let picked = args
            .face
            .as_ref()
            .context("pick the face to lay the sheet out from")?
            .name();
        let thickness = (*builder).measure(&args.thickness, "the thickness")?;
        let k = (*builder).measure(&args.k, "the K-factor")?;
        ensure!(
            (0.0..=1.0).contains(&k),
            "the K-factor is a share of the thickness: from 0 to 1"
        );
        let topo = &builder.tracked().topo;
        for (body, name) in workpiece_shape::bodies::solids_with(topo, &[picked]) {
            let solid = read(topo, body)?;
            let found = solid.faces.iter().position(|f| {
                f.name.as_deref() == Some(picked) || f.also.iter().any(|n| n == picked)
            });
            let Some(face) = found else {
                continue;
            };
            let (flat, bends) = unfolded(&solid, face, thickness, k, &self.id)
                .with_context(|| format!("laying {name} out flat"))?;
            out.write(&flat, &self.id);
            for (n, bend) in bends.iter().enumerate() {
                let [from, to] = bend.line;
                let axis = workpiece_core::world::DatumAxis::new(from, to - from)?;
                out.made
                    .datums
                    .push((format!("{}/bend{}", self.id, n + 1), axis.into()));
            }
            return Ok(out.made);
        }
        anyhow::bail!("there is no face {picked:?} to lay the sheet out from")
    }

    fn summary(&self) -> String {
        format!("{} thick, K {}", self.args.thickness, self.args.k)
    }
}
