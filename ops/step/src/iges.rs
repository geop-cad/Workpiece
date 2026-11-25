//! IGES files: trimmed NURBS surfaces read as the faces of bodies, and
//! bodies written as such.
//!
//! What is read: trimmed surfaces (entity 144) on rational B-spline
//! surfaces (128), planes (108), ruled surfaces between curves made
//! alike (118), surfaces of revolution (120) and tabulated cylinders
//! (122), and the faces of IGES's own solids (510, with 508, 504 and
//! 502) on those or on planes, cylinders, cones, balls and tori (190
//! to 198),
//! bounded by curves on them (142) that are B-spline
//! curves (126), lines (110), circular arcs (100) or chains of those
//! (102). The file's faces share nothing; they are sewn where their
//! edges lie on each other, and what closes is a solid. Other surfaces
//! (offset surfaces, ruled surfaces between unlike curves) are told of
//! and left out.

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

use anyhow::Context;
use shapecraft_boolean::model::{Edge, Face, Use, Vertex, Walk};
use shapecraft_boolean::{Solid, geometry, stitched};
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::{Curve2, Curve3, NurbsSurface};

/// A curve, and the part of it that counts.
type Piece = (Curve3, (f64, f64));

/// What an IGES file holds.
pub struct IgesImported {
    /// The bodies: each a sheet, or (with `true`) a solid its faces
    /// close around.
    pub bodies: Vec<(Solid, bool)>,
    /// The faces that could not be read, each with why.
    pub refused: Vec<String>,
}

/// Whether `path` names an IGES file.
pub fn is_iges(path: &str) -> bool {
    let lower = path.trim().to_ascii_lowercase();
    lower.ends_with(".igs") || lower.ends_with(".iges")
}

/// An entity of the file: its kind, its numbers, and the matrix it is
/// moved by.
struct Entity {
    kind: u32,
    numbers: Vec<f64>,
    matrix: usize,
}

struct File {
    /// The entities, by the number of their directory line.
    entities: HashMap<usize, Entity>,
    /// How many millimetres one of the file's units is.
    scale: f64,
}

/// A number as IGES writes it: with `D` for the exponent, too.
fn number(text: &str) -> f64 {
    let text = text.trim().replace(['D', 'd'], "E");
    text.parse().unwrap_or(0.0)
}

impl File {
    fn new(bytes: &[u8]) -> anyhow::Result<Self> {
        let text = String::from_utf8_lossy(bytes);
        let (mut global, mut directory, mut parameters) = (String::new(), Vec::new(), Vec::new());
        for line in text.lines() {
            let line = format!("{line:<80}");
            let Some(section) = line.get(72..73) else {
                continue;
            };
            match section {
                "G" => global.push_str(&line[..72]),
                "D" => directory.push(line),
                "P" => parameters.push(line),
                _ => {}
            }
        }
        anyhow::ensure!(
            !directory.is_empty() && directory.len() % 2 == 0,
            "not an IGES file: it has no directory"
        );
        // The global section: what parts numbers, what ends a record,
        // and the unit — after text given by its length (`4HINCH`).
        let mut fields = Vec::new();
        let (mut delimiter, mut end) = (',', ';');
        let chars: Vec<char> = global.chars().collect();
        let mut at = 0;
        while at < chars.len() {
            let mut field = String::new();
            let digits: String = chars[at..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !digits.is_empty() && chars.get(at + digits.len()) == Some(&'H') {
                let from = at + digits.len() + 1;
                let to = (from + digits.parse::<usize>().unwrap_or(0)).min(chars.len());
                field = chars[from..to].iter().collect();
                at = to;
            }
            while at < chars.len() && chars[at] != delimiter && chars[at] != end {
                field.push(chars[at]);
                at += 1;
            }
            match fields.len() {
                0 if field.len() == 1 => delimiter = field.chars().next().unwrap_or(','),
                1 if field.len() == 1 => end = field.chars().next().unwrap_or(';'),
                _ => {}
            }
            fields.push(field);
            if chars.get(at) == Some(&end) {
                break;
            }
            at += 1;
        }
        let flag = fields.get(13).map_or(2.0, |f| number(f));
        let scale = match flag as i64 {
            1 => 25.4,
            4 => 304.8,
            6 => 1000.0,
            8 => 0.0254,
            9 => 0.001,
            10 => 10.0,
            _ => 1.0,
        };
        // Each entity's numbers, from its lines of the parameter
        // section.
        let whole = |line: &str, field: usize| -> usize {
            let text = line.get(field * 8..field * 8 + 8).unwrap_or_default();
            text.trim().parse().unwrap_or(0)
        };
        let mut entities = HashMap::new();
        for (k, pair) in directory.chunks(2).enumerate() {
            let (first, count) = (whole(&pair[0], 1), whole(&pair[1], 3));
            let mut record = String::new();
            for line in parameters.iter().skip(first.saturating_sub(1)).take(count) {
                record.push_str(&line[..64]);
            }
            let record = record.split(end).next().unwrap_or_default();
            let mut numbers: Vec<f64> = record.split(delimiter).map(number).collect();
            if !numbers.is_empty() {
                numbers.remove(0);
            }
            let entity = Entity {
                kind: whole(&pair[0], 0) as u32,
                numbers,
                matrix: whole(&pair[0], 6),
            };
            entities.insert(2 * k + 1, entity);
        }
        Ok(Self { entities, scale })
    }

    fn entity(&self, at: f64) -> anyhow::Result<&Entity> {
        let at = at as usize;
        self.entities
            .get(&at)
            .with_context(|| format!("there is no entity {at}"))
    }

    /// Where the matrix of `entity` and the file's unit put `p`.
    fn placed(&self, entity: &Entity, p: Point3) -> Point3 {
        let moved = match self.entities.get(&entity.matrix) {
            Some(m) if m.kind == 124 && m.numbers.len() >= 12 => {
                let r = &m.numbers;
                Point3::new(
                    r[0] * p.x + r[1] * p.y + r[2] * p.z + r[3],
                    r[4] * p.x + r[5] * p.y + r[6] * p.z + r[7],
                    r[8] * p.x + r[9] * p.y + r[10] * p.z + r[11],
                )
            }
            _ => p,
        };
        moved * self.scale
    }

    /// The curves entity `at` is, one after the other.
    fn curves(&self, at: f64, out: &mut Vec<Piece>) -> anyhow::Result<()> {
        let entity = self.entity(at)?;
        let n = &entity.numbers;
        let short = || anyhow::anyhow!("entity {} is cut short", at as usize);
        match entity.kind {
            102 => {
                let count = *n.first().ok_or_else(short)? as usize;
                for k in 0..count {
                    self.curves(*n.get(1 + k).ok_or_else(short)?, out)?;
                }
            }
            110 => {
                anyhow::ensure!(n.len() >= 6, short());
                let (a, b) = (Point3::new(n[0], n[1], n[2]), Point3::new(n[3], n[4], n[5]));
                let (a, b) = (self.placed(entity, a), self.placed(entity, b));
                if a.distance(b) > 0.0 {
                    out.push((Curve3::line(a, b)?, (0.0, 1.0)));
                }
            }
            100 => {
                anyhow::ensure!(n.len() >= 7, short());
                let (centre, from, to) = (
                    Point2::new(n[1], n[2]),
                    Point2::new(n[3], n[4]),
                    Point2::new(n[5], n[6]),
                );
                let radius = centre.distance(from);
                let start = (from - centre).to_angle();
                let mut sweep = (to - centre).to_angle() - start;
                if sweep <= 1e-12 {
                    sweep += std::f64::consts::TAU;
                }
                // Arcs of a quarter turn at most, as one rational curve.
                let pieces = (sweep / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
                let step = sweep / pieces as f64;
                let at_angle = |a: f64, far: f64| {
                    let p = centre + Point2::from_angle(a) * far;
                    self.placed(entity, Point3::new(p.x, p.y, n[0]))
                };
                let (mut points, mut weights) = (vec![at_angle(start, radius)], vec![1.0]);
                let mut knots = vec![0.0; 3];
                for k in 0..pieces {
                    let a = start + step * k as f64;
                    let w = (step / 2.0).cos();
                    points.extend([
                        at_angle(a + step / 2.0, radius / w),
                        at_angle(a + step, radius),
                    ]);
                    weights.extend([w, 1.0]);
                    knots.extend([(k + 1) as f64; 2]);
                }
                knots.push(pieces as f64);
                let curve = Curve3::new(2, knots, points, Some(weights))?;
                out.push((curve, (0.0, pieces as f64)));
            }
            126 => {
                anyhow::ensure!(n.len() >= 2, short());
                let (k, degree) = (n[0] as usize + 1, n[1] as usize);
                let knots = 6..6 + k + degree + 1;
                let weights = knots.end..knots.end + k;
                let points = weights.end..weights.end + 3 * k;
                anyhow::ensure!(n.len() >= points.end + 2, short());
                let control = n[points.clone()]
                    .chunks(3)
                    .map(|p| self.placed(entity, Point3::new(p[0], p[1], p[2])))
                    .collect();
                let curve = Curve3::new(
                    degree,
                    n[knots].to_vec(),
                    control,
                    Some(n[weights].to_vec()),
                )?;
                let (lo, hi) = curve.domain();
                let range = (n[points.end].max(lo), n[points.end + 1].min(hi));
                // (A curve of no length is a point: a pole of the face.)
                out.push((curve, if range.0 < range.1 { range } else { (lo, hi) }));
            }
            other => anyhow::bail!("a boundary is a curve of kind {other}, which is not read"),
        }
        Ok(())
    }

    /// Where the matrix of `entity` puts `p`, which is in the file's
    /// unit no longer: what it is made of was placed already.
    fn moved(&self, entity: &Entity, p: Point3) -> Point3 {
        match self.entities.get(&entity.matrix) {
            Some(m) if m.kind == 124 && m.numbers.len() >= 12 => {
                let r = &m.numbers;
                Point3::new(
                    r[0] * p.x + r[1] * p.y + r[2] * p.z + r[3] * self.scale,
                    r[4] * p.x + r[5] * p.y + r[6] * p.z + r[7] * self.scale,
                    r[8] * p.x + r[9] * p.y + r[10] * p.z + r[11] * self.scale,
                )
            }
            _ => p,
        }
    }

    /// The plane entity `at` is, as far as `around` (places on what
    /// bounds the face) reaches and as much again.
    fn plane(&self, entity: &Entity, around: &[Point3]) -> anyhow::Result<NurbsSurface> {
        let n = &entity.numbers;
        anyhow::ensure!(n.len() >= 4, "a plane cut short");
        let normal = Point3::new(n[0], n[1], n[2]);
        let long = normal.length();
        anyhow::ensure!(long > 0.0, "a plane without a normal");
        // (A x + B y + C z = D, in the file's unit, then where its
        // matrix puts it.)
        let on = self.placed(entity, normal * (n[3] / (long * long)));
        let turned = self.placed(entity, normal) - self.placed(entity, Point3::ZERO);
        let normal = turned.try_normalize().context("a plane without a normal")?;
        anyhow::ensure!(
            !around.is_empty(),
            "a plane with nothing bounding the face on it"
        );
        let middle = around.iter().sum::<Point3>() / around.len() as f64;
        let middle = middle - normal * (middle - on).dot(normal);
        let reach = around
            .iter()
            .map(|p| p.distance(middle))
            .fold(0.0, f64::max)
            * 2.0;
        anyhow::ensure!(reach > 0.0, "a face of no size");
        let x = normal.any_orthonormal_vector() * reach;
        let y = normal.cross(x);
        Ok(NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            vec![
                middle - x - y,
                middle - x + y,
                middle + x - y,
                middle + x + y,
            ],
            None,
        )?)
    }

    /// The surface of revolution entity `at` is: a curve turned about a
    /// line, from one angle to another.
    fn revolved(&self, entity: &Entity) -> anyhow::Result<NurbsSurface> {
        let n = &entity.numbers;
        anyhow::ensure!(n.len() >= 4, "a surface of revolution cut short");
        let mut axis = Vec::new();
        self.curves(n[0], &mut axis)?;
        let (line, range) = axis
            .first()
            .context("a surface of revolution without an axis")?;
        let origin = line.point_at(range.0);
        let along = (line.point_at(range.1) - origin)
            .try_normalize()
            .context("an axis of no length")?;
        let mut turned = Vec::new();
        self.curves(n[1], &mut turned)?;
        anyhow::ensure!(
            turned.len() == 1,
            "a surface of revolution of a chain of curves"
        );
        let (curve, range) = &turned[0];
        let curve = curve.trimmed(range.0, range.1)?;
        let (from, to) = (n[2], n[3]);
        anyhow::ensure!(to > from, "a surface of revolution through no angle");
        let turned = turned_about(origin, along, &curve, from, to)?;
        let (p, q) = turned.degrees();
        let (nu, nv) = turned.counts();
        let (ku, kv) = turned.knots();
        Ok(NurbsSurface::new(
            p,
            q,
            ku.to_vec(),
            kv.to_vec(),
            nu,
            nv,
            turned
                .control_points()
                .iter()
                .map(|c| self.moved(entity, *c))
                .collect(),
            Some(turned.weights().to_vec()),
        )?)
    }

    /// A point (116) or a direction (123): its three numbers.
    fn triple(&self, at: f64) -> anyhow::Result<Point3> {
        let n = &self.entity(at)?.numbers;
        anyhow::ensure!(n.len() >= 3, "a point or direction cut short");
        Ok(Point3::new(n[0], n[1], n[2]))
    }

    /// The plane, cylinder, cone, ball or torus entity `at` is (190 to
    /// 198), as far as `around` — places on what bounds the face —
    /// reaches along it.
    fn analytic(&self, entity: &Entity, around: &[Point3]) -> anyhow::Result<NurbsSurface> {
        use std::f64::consts::{FRAC_PI_2, TAU};
        let n = &entity.numbers;
        anyhow::ensure!(n.len() >= 2, "a surface cut short");
        let at = self.placed(entity, self.triple(n[0])?);
        let turn = |d: Point3| self.placed(entity, d) - self.placed(entity, Point3::ZERO);
        // Where the surface starts around its axis: towards the
        // direction the file gives for that (its edges that close
        // around the axis start and end there), square to the axis.
        let towards = |given: Option<&f64>, axis: Point3| -> anyhow::Result<Point3> {
            let given = match given.filter(|g| **g != 0.0) {
                Some(&g) => turn(self.triple(g)?)
                    .reject_from_normalized(axis)
                    .try_normalize(),
                None => None,
            };
            Ok(given.unwrap_or_else(|| axis.any_orthonormal_vector()))
        };
        if entity.kind == 196 {
            // A ball: half a circle from pole to pole, turned about
            // the axis through them.
            let radius = n[1] * self.scale;
            let axis = match n.get(2).filter(|a| **a != 0.0) {
                Some(&a) => turn(self.triple(a)?).normalize(),
                None => Point3::Z,
            };
            let out = towards(n.get(3), axis)?;
            let half = arc(at, out, axis, radius, -FRAC_PI_2, FRAC_PI_2)?;
            return turned_about(at, axis, &half, 0.0, TAU);
        }
        let axis = turn(self.triple(n[1])?)
            .try_normalize()
            .context("a surface without an axis")?;
        if entity.kind == 190 {
            // (A plane through a point, square to a direction.)
            anyhow::ensure!(
                !around.is_empty(),
                "a plane with nothing bounding the face on it"
            );
            let middle = around.iter().sum::<Point3>() / around.len() as f64;
            let middle = middle - axis * (middle - at).dot(axis);
            let reach = around
                .iter()
                .map(|p| p.distance(middle))
                .fold(0.0, f64::max)
                * 2.0;
            anyhow::ensure!(reach > 0.0, "a face of no size");
            let x = axis.any_orthonormal_vector() * reach;
            let y = axis.cross(x);
            return Ok(NurbsSurface::new(
                1,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
                2,
                2,
                vec![
                    middle - x - y,
                    middle - x + y,
                    middle + x - y,
                    middle + x + y,
                ],
                None,
            )?);
        }
        anyhow::ensure!(n.len() >= 3, "a surface cut short");
        let out = towards(n.get(if entity.kind == 192 { 3 } else { 4 }), axis)?;
        // How far along the axis the face reaches, and a little more.
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in around {
            let along = (*p - at).dot(axis);
            (lo, hi) = (lo.min(along), hi.max(along));
        }
        let more = (hi - lo).max(n[2].abs() * self.scale) * 0.1;
        let (lo, hi) = (lo - more, hi + more);
        let side = match entity.kind {
            192 => {
                let radius = n[2] * self.scale;
                anyhow::ensure!(
                    lo.is_finite(),
                    "a cylinder with nothing bounding the face on it"
                );
                Curve3::line(at + axis * lo + out * radius, at + axis * hi + out * radius)?
            }
            194 => {
                // (As wide as its radius where it is placed, wider
                // along its axis by its half angle; not past its tip.)
                anyhow::ensure!(n.len() >= 4, "a cone cut short");
                let (radius, lean) = (n[2] * self.scale, n[3].to_radians().tan());
                anyhow::ensure!(
                    lo.is_finite(),
                    "a cone with nothing bounding the face on it"
                );
                let tip = if lean.abs() > 1e-12 {
                    -radius / lean
                } else {
                    f64::NEG_INFINITY
                };
                let (lo, hi) = if lean > 0.0 {
                    (lo.max(tip), hi)
                } else {
                    (lo, hi.min(tip))
                };
                let wide = |z: f64| (radius + z * lean).max(0.0);
                Curve3::line(
                    at + axis * lo + out * wide(lo),
                    at + axis * hi + out * wide(hi),
                )?
            }
            198 => {
                anyhow::ensure!(n.len() >= 4, "a torus cut short");
                let (major, minor) = (n[2] * self.scale, n[3] * self.scale);
                arc(at + out * major, out, axis, minor, 0.0, TAU)?
            }
            other => anyhow::bail!("a face lies on a surface of kind {other}, which is not read"),
        };
        turned_about(at, axis, &side, 0.0, TAU)
    }

    /// The ruled surface entity `at` is: straight from each place of
    /// one curve to the same share along another. Read where the two
    /// curves are made the same way (as two lines are): then that is
    /// the surface between their control points.
    fn ruled(&self, entity: &Entity) -> anyhow::Result<NurbsSurface> {
        let n = &entity.numbers;
        anyhow::ensure!(n.len() >= 3, "a ruled surface cut short");
        let side = |at: f64| -> anyhow::Result<Curve3> {
            let mut pieces = Vec::new();
            self.curves(at, &mut pieces)?;
            anyhow::ensure!(pieces.len() == 1, "a ruled surface of a chain of curves");
            let (curve, range) = &pieces[0];
            Ok(curve.trimmed(range.0, range.1)?)
        };
        let (first, mut second) = (side(n[0])?, side(n[1])?);
        if n[2] != 0.0 {
            second = second.reversed();
        }
        let alike = first.degree() == second.degree()
            && first.control_points().len() == second.control_points().len();
        anyhow::ensure!(
            alike,
            "a ruled surface between curves that are not made the same way"
        );
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for k in 0..first.control_points().len() {
            points.extend([
                self.moved(entity, first.control_points()[k]),
                self.moved(entity, second.control_points()[k]),
            ]);
            weights.extend([first.weights()[k], second.weights()[k]]);
        }
        Ok(NurbsSurface::new(
            first.degree(),
            1,
            first.knots().to_vec(),
            vec![0.0, 0.0, 1.0, 1.0],
            first.control_points().len(),
            2,
            points,
            Some(weights),
        )?)
    }

    /// The loops of the face entity `at` (510), each the curves of its
    /// edges one after the other, as the face walks them.
    fn face_loops(&self, entity: &Entity) -> anyhow::Result<Vec<Vec<Piece>>> {
        let n = &entity.numbers;
        anyhow::ensure!(n.len() >= 3, "a face cut short");
        let count = n[1] as usize;
        let mut loops = Vec::new();
        for &at in n.get(3..3 + count).context("a face cut short")? {
            let walk = &self.entity(at)?.numbers;
            let mut pieces = Vec::new();
            let mut k = 1;
            for _ in 0..walk.first().copied().unwrap_or(0.0) as usize {
                let step = walk.get(k..k + 5).context("a loop cut short")?;
                let (kind, list, index, forward, under) = (
                    step[0],
                    step[1],
                    step[2] as usize,
                    step[3] != 0.0,
                    step[4] as usize,
                );
                k += 5 + 2 * under;
                if kind != 0.0 {
                    // (A vertex alone: a point the surface comes to.)
                    continue;
                }
                let edges = &self.entity(list)?.numbers;
                let curve = *edges
                    .get(1 + 5 * (index.max(1) - 1))
                    .context("an edge that is not there")?;
                let mut of_edge = Vec::new();
                self.curves(curve, &mut of_edge)?;
                if !forward {
                    of_edge.reverse();
                    for (curve, range) in &mut of_edge {
                        *curve = curve.trimmed(range.0, range.1)?.reversed();
                        *range = curve.domain();
                    }
                }
                pieces.extend(of_edge);
            }
            if !pieces.is_empty() {
                loops.push(pieces);
            }
        }
        Ok(loops)
    }

    /// The tabulated cylinder entity `at` is: a curve carried straight
    /// to where its start comes to a given point.
    fn carried(&self, entity: &Entity) -> anyhow::Result<NurbsSurface> {
        let n = &entity.numbers;
        anyhow::ensure!(n.len() >= 4, "a tabulated cylinder cut short");
        let mut along = Vec::new();
        self.curves(n[0], &mut along)?;
        anyhow::ensure!(
            along.len() == 1,
            "a tabulated cylinder of a chain of curves"
        );
        let (curve, range) = &along[0];
        let curve = curve.trimmed(range.0, range.1)?;
        let to = Point3::new(n[1], n[2], n[3]) * self.scale;
        let by = to - curve.point_at(curve.domain().0);
        anyhow::ensure!(by.length() > 0.0, "a tabulated cylinder carried nowhere");
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for (p, weight) in curve.control_points().iter().zip(curve.weights()) {
            points.extend([self.moved(entity, *p), self.moved(entity, *p + by)]);
            weights.extend([*weight, *weight]);
        }
        Ok(NurbsSurface::new(
            curve.degree(),
            1,
            curve.knots().to_vec(),
            vec![0.0, 0.0, 1.0, 1.0],
            curve.control_points().len(),
            2,
            points,
            Some(weights),
        )?)
    }

    /// The surface entity `at` is; `around` are places on what bounds
    /// the face on it (a plane has no size of its own).
    fn surface(&self, at: f64, around: &[Point3]) -> anyhow::Result<NurbsSurface> {
        let entity = self.entity(at)?;
        let n = &entity.numbers;
        match entity.kind {
            108 => return self.plane(entity, around),
            118 => return self.ruled(entity),
            120 => return self.revolved(entity),
            122 => return self.carried(entity),
            190..=198 => return self.analytic(entity, around),
            _ => {}
        }
        anyhow::ensure!(
            entity.kind == 128,
            "a face lies on a surface of kind {}, which is not read",
            entity.kind
        );
        let short = || anyhow::anyhow!("surface {} is cut short", at as usize);
        anyhow::ensure!(n.len() >= 9, short());
        let (nu, nv) = (n[0] as usize + 1, n[1] as usize + 1);
        let (p, q) = (n[2] as usize, n[3] as usize);
        let ku = 9..9 + nu + p + 1;
        let kv = ku.end..ku.end + nv + q + 1;
        let weights = kv.end..kv.end + nu * nv;
        let points = weights.end..weights.end + 3 * nu * nv;
        anyhow::ensure!(n.len() >= points.end, short());
        // (The file runs along u first; the surface here along v.)
        let mut control = vec![Point3::ZERO; nu * nv];
        let mut weight = vec![1.0; nu * nv];
        for j in 0..nv {
            for i in 0..nu {
                let from = j * nu + i;
                let xyz = &n[points.start + 3 * from..points.start + 3 * from + 3];
                control[i * nv + j] = self.placed(entity, Point3::new(xyz[0], xyz[1], xyz[2]));
                weight[i * nv + j] = n[weights.start + from];
            }
        }
        Ok(NurbsSurface::new(
            p,
            q,
            n[ku].to_vec(),
            n[kv].to_vec(),
            nu,
            nv,
            control,
            Some(weight),
        )?)
    }
}

/// The arc about `centre` in the plane of `x` and `y` (unit vectors,
/// square to each other), of `radius`, from angle `from` to `to`: in
/// pieces of a quarter turn at most.
fn arc(
    centre: Point3,
    x: Point3,
    y: Point3,
    radius: f64,
    from: f64,
    to: f64,
) -> anyhow::Result<Curve3> {
    let pieces = ((to - from) / std::f64::consts::FRAC_PI_2 - 1e-9)
        .ceil()
        .max(1.0) as usize;
    let step = (to - from) / pieces as f64;
    let w = (step / 2.0).cos();
    let at = |angle: f64, far: f64| centre + (x * angle.cos() + y * angle.sin()) * far;
    let (mut points, mut weights) = (vec![at(from, radius)], vec![1.0]);
    let mut knots = vec![0.0; 3];
    for k in 0..pieces {
        let a = from + step * k as f64;
        points.extend([at(a + step / 2.0, radius / w), at(a + step, radius)]);
        weights.extend([w, 1.0]);
        knots.extend([(k + 1) as f64; 2]);
    }
    knots.push(pieces as f64);
    Ok(Curve3::new(2, knots, points, Some(weights))?)
}

/// The surface `curve` sweeps turned about the axis through `origin`
/// along `along`, from angle `from` to `to`: around in arcs of a
/// quarter turn at most, each control point of the curve on its own
/// circle about the axis.
fn turned_about(
    origin: Point3,
    along: Point3,
    curve: &Curve3,
    from: f64,
    to: f64,
) -> anyhow::Result<NurbsSurface> {
    let pieces = ((to - from) / std::f64::consts::FRAC_PI_2 - 1e-9)
        .ceil()
        .max(1.0) as usize;
    let step = (to - from) / pieces as f64;
    let w = (step / 2.0).cos();
    let around = |p: Point3, angle: f64, stretch: f64| {
        let foot = origin + along * (p - origin).dot(along);
        let out = p - foot;
        foot + (out * angle.cos() + along.cross(out) * angle.sin()) * stretch
    };
    let count = curve.control_points().len();
    let (mut points, mut weights) = (Vec::new(), Vec::new());
    for (p, weight) in curve.control_points().iter().zip(curve.weights()) {
        for k in 0..=2 * pieces {
            let angle = from + step * k as f64 / 2.0;
            let (stretch, share) = if k % 2 == 1 { (1.0 / w, w) } else { (1.0, 1.0) };
            points.push(around(*p, angle, stretch));
            weights.push(weight * share);
        }
    }
    let mut knots = vec![0.0; 3];
    for k in 1..pieces {
        knots.extend([k as f64; 2]);
    }
    knots.extend([pieces as f64; 3]);
    Ok(NurbsSurface::new(
        curve.degree(),
        2,
        curve.knots().to_vec(),
        knots,
        count,
        2 * pieces + 1,
        points,
        Some(weights),
    )?)
}

/// The four sides of `surface`, around it.
fn sides(surface: &NurbsSurface) -> anyhow::Result<Vec<Piece>> {
    let (p, q) = surface.degrees();
    let (nu, nv) = surface.counts();
    let (ku, kv) = surface.knots();
    let at = |i: usize, j: usize| {
        let k = i * nv + j;
        (surface.control_points()[k], surface.weights()[k])
    };
    let along_u = |j: usize| -> anyhow::Result<Curve3> {
        let (points, weights): (Vec<_>, Vec<_>) = (0..nu).map(|i| at(i, j)).unzip();
        Ok(Curve3::new(p, ku.to_vec(), points, Some(weights))?)
    };
    let along_v = |i: usize| -> anyhow::Result<Curve3> {
        let (points, weights): (Vec<_>, Vec<_>) = (0..nv).map(|j| at(i, j)).unzip();
        Ok(Curve3::new(q, kv.to_vec(), points, Some(weights))?)
    };
    let whole = |curve: Curve3| {
        let range = curve.domain();
        (curve, range)
    };
    Ok(vec![
        whole(along_u(0)?),
        whole(along_v(nu - 1)?),
        whole(along_u(nv - 1)?.reversed()),
        whole(along_v(0)?.reversed()),
    ])
}

/// The face of `surface` inside `loops` (the first around it, the
/// others around its holes; each a chain of curves on the surface), as
/// a sheet of its own.
fn face(surface: NurbsSurface, loops: &[Vec<Piece>], id: &str) -> anyhow::Result<Solid> {
    let corners = surface.control_points();
    let (lo, hi) = corners.iter().fold(
        (
            Point3::splat(f64::INFINITY),
            Point3::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let size = (hi - lo).length();
    anyhow::ensure!(size > 0.0, "a surface of no size");
    let near = size * 1e-5;
    let ((u0, u1), (v0, v1)) = surface.domain();
    let mut out = Solid::default();
    let mut walks = Vec::new();
    let mut loose = near;
    for (k, pieces) in loops.iter().enumerate() {
        // Where each piece is in (u, v): on from where the last ended.
        let mut last: Option<Point2> = None;
        let mut flat: Vec<Option<Curve2>> = vec![None; pieces.len()];
        let mut seeds: Vec<(Point3, Point2)> = Vec::new();
        // From a piece that is at one place in (u, v) only — not one
        // along a seam, which is at two — and on around from there, so
        // that each seam is taken where the piece before it ends.
        let plain = |(curve, range): &Piece| {
            let middle = curve.point_at((range.0 + range.1) / 2.0);
            let ends = curve.point_at(range.0).distance(middle);
            ends > near && geometry::on_surface(&surface, middle, near * 10.0).len() == 1
        };
        let first = pieces.iter().position(plain).unwrap_or(0);
        for i in (0..pieces.len()).map(|i| (first + i) % pieces.len()) {
            let (curve, range) = &pieces[i];
            let long: f64 = (0..8)
                .map(|s| {
                    let at =
                        |s: i32| curve.point_at(range.0 + (range.1 - range.0) * f64::from(s) / 8.0);
                    at(s).distance(at(s + 1))
                })
                .sum();
            if long <= near {
                continue;
            }
            // From its middle (its ends may be where the surface comes
            // to a point, which is everywhere along a side of (u, v)).
            // Along a seam there are two places in (u, v): the one no
            // piece of this boundary through the same place has taken.
            let half = (range.0 + range.1) / 2.0;
            let middle = curve.point_at(half);
            let hits = geometry::on_surface(&surface, middle, near * 10.0);
            let guess = last.unwrap_or(Point2::new((u0 + u1) / 2.0, (v0 + v1) / 2.0));
            let taken = |uv: &Point2| {
                let span = Point2::new(u1 - u0, v1 - v0);
                seeds.iter().any(|(p, q): &(Point3, Point2)| {
                    p.distance(middle) <= near * 10.0 && ((*q - *uv) / span).length() <= 1e-3
                })
            };
            let seed = hits
                .into_iter()
                .filter(|uv| !taken(uv))
                .min_by(|a, b| a.distance(guess).total_cmp(&b.distance(guess)))
                .unwrap_or_else(|| geometry::foot(&surface, middle, guess).0);
            seeds.push((middle, seed));
            let (pcurve, off) = geometry::pcurve(&surface, curve, *range, (half, seed), near * 0.1)
                .context("a boundary curve does not lie on its surface")?;
            anyhow::ensure!(
                off <= near * 100.0,
                "a boundary curve is {off} off its surface"
            );
            loose = loose.max(off);
            last = Some(pcurve.point_at(range.1));
            flat[i] = Some(pcurve);
        }
        anyhow::ensure!(flat.iter().any(Option::is_some), "a boundary of no length");
        // A piece of no length is a point the surface comes to: its
        // curve in (u, v) runs from where the piece before ends to
        // where the one after starts.
        let count = pieces.len();
        let ends = |i: usize, start: bool| -> Option<Point2> {
            let (range, pcurve) = (pieces[i].1, flat[i].as_ref()?);
            Some(pcurve.point_at(if start { range.0 } else { range.1 }))
        };
        let mut steps: Vec<Use> = Vec::new();
        let mut around: Vec<Point2> = Vec::new();
        for i in 0..count {
            let (curve, range) = &pieces[i];
            let vertex = |out: &mut Solid, p: Point3| {
                let found = out
                    .vertices
                    .iter()
                    .position(|v| v.point.distance(p) <= near * 10.0);
                found.unwrap_or_else(|| {
                    out.vertices.push(Vertex {
                        point: p,
                        tolerance: near * 10.0,
                        name: None,
                    });
                    out.vertices.len() - 1
                })
            };
            match &flat[i] {
                Some(pcurve) => {
                    let ends = [
                        vertex(&mut out, curve.point_at(range.0)),
                        vertex(&mut out, curve.point_at(range.1)),
                    ];
                    out.edges.push(Edge {
                        curve: Arc::new(curve.clone()),
                        range: *range,
                        ends,
                        tolerance: near,
                        name: Some(format!("{id}/l{k}e{i}")),
                        also: Vec::new(),
                    });
                    around.extend((0..8).map(|s| {
                        pcurve.point_at(range.0 + (range.1 - range.0) * f64::from(s) / 8.0)
                    }));
                    steps.push(Use {
                        walk: Walk::Edge {
                            edge: out.edges.len() - 1,
                            reversed: false,
                        },
                        pcurve: Arc::new(pcurve.clone()),
                    });
                }
                None => {
                    let before = (1..count).find_map(|d| ends((i + count - d) % count, false));
                    let after = (1..count).find_map(|d| ends((i + d) % count, true));
                    let (Some(from), Some(to)) = (before, after) else {
                        continue;
                    };
                    if from.distance(to) <= 1e-12 {
                        continue;
                    }
                    let at = vertex(&mut out, curve.point_at(range.0));
                    around.push(from);
                    steps.push(Use {
                        walk: Walk::Pole {
                            vertex: at,
                            range: (0.0, 1.0),
                        },
                        pcurve: Arc::new(Curve2::line(from, to)?),
                    });
                }
            }
        }
        // The face is to the left of its boundary: counter-clockwise
        // around it, clockwise around a hole.
        let turned: f64 = (0..around.len())
            .map(|s| around[s].perp_dot(around[(s + 1) % around.len()]))
            .sum();
        if (turned < 0.0) == (k == 0) {
            steps.reverse();
            for step in &mut steps {
                if let Walk::Edge { reversed, .. } = &mut step.walk {
                    *reversed = !*reversed;
                }
                if let Walk::Pole { .. } = step.walk {
                    step.pcurve = Arc::new(step.pcurve.reversed());
                }
            }
        }
        walks.push(steps);
    }
    out.faces.push(Face {
        surface: Arc::new(surface),
        outward: true,
        loops: walks,
        tolerance: loose,
        name: Some(format!("{id}/face")),
        also: Vec::new(),
        smooth: Vec::new(),
    });
    Ok(out)
}

/// The bodies of an IGES file, their faces called `{id}/f0/…`: the
/// trimmed surfaces sewn where their edges lie on each other; what
/// closes is a solid, what does not a sheet.
pub fn import_iges(bytes: &[u8], id: &str) -> anyhow::Result<IgesImported> {
    let file = File::new(bytes)?;
    let mut order: Vec<usize> = file.entities.keys().copied().collect();
    order.sort_unstable();
    let mut sheets = Vec::new();
    let mut refused = Vec::new();
    for at in order {
        let entity = &file.entities[&at];
        if entity.kind != 144 && entity.kind != 510 {
            continue;
        }
        let made = (|| -> anyhow::Result<Solid> {
            let n = &entity.numbers;
            anyhow::ensure!(n.len() >= 4, "a trimmed surface cut short");
            let mut loops = if entity.kind == 510 {
                file.face_loops(entity)?
            } else {
                Vec::new()
            };
            let of_solid = entity.kind == 510;
            let bounds = (!of_solid && n[1] != 0.0).then_some(n[3]);
            let holes = if of_solid {
                &[][..]
            } else {
                n.get(4..4 + n[2] as usize).unwrap_or_default()
            };
            for &bound in bounds.iter().chain(holes) {
                let on = file.entity(bound)?;
                anyhow::ensure!(
                    on.kind == 142 && on.numbers.len() >= 4,
                    "a boundary that is no curve on a surface"
                );
                anyhow::ensure!(
                    on.numbers[3] != 0.0,
                    "a boundary given in the surface's parameters only"
                );
                let mut pieces = Vec::new();
                file.curves(on.numbers[3], &mut pieces)?;
                loops.push(pieces);
            }
            let around: Vec<Point3> = loops
                .iter()
                .flatten()
                .flat_map(|(curve, range): &Piece| {
                    (0..=4).map(move |k| {
                        curve.point_at(range.0 + (range.1 - range.0) * f64::from(k) / 4.0)
                    })
                })
                .collect();
            let surface = file.surface(n[0], &around)?;
            if bounds.is_none() && !of_solid {
                loops.insert(0, sides(&surface)?);
            }
            anyhow::ensure!(!loops.is_empty(), "a face that nothing bounds");
            face(surface, &loops, &format!("{id}/f{}", sheets.len()))
        })();
        match made {
            Ok(sheet) => sheets.push(sheet),
            Err(why) => refused.push(format!("trimmed surface {at}: {why:#}")),
        }
    }
    anyhow::ensure!(
        !sheets.is_empty() || !refused.is_empty(),
        "the file has no trimmed surfaces"
    );
    let mut bodies = Vec::new();
    if !sheets.is_empty() {
        let size = sheets
            .iter()
            .map(|s| {
                let (lo, hi) = s.extent();
                (hi - lo).length()
            })
            .fold(0.0, f64::max);
        match stitched(&sheets, size * 1e-5) {
            Ok((sewn, closed)) => bodies.push((sewn, closed)),
            // (Faces that cannot be sewn are still faces.)
            Err(_) => bodies.extend(sheets.into_iter().map(|s| (s, false))),
        }
    }
    Ok(IgesImported { bodies, refused })
}

/// Writes the sections of an IGES file.
#[derive(Default)]
struct Writer {
    directory: Vec<String>,
    parameters: Vec<String>,
}

impl Writer {
    /// Adds an entity of `kind` with `numbers` (reals; `whole` says
    /// which of them are written as whole numbers) and its `status`;
    /// gives the number of its directory line.
    fn add(&mut self, kind: u32, status: &str, numbers: &[(f64, bool)]) -> usize {
        let at = self.directory.len() + 1;
        let first = self.parameters.len() + 1;
        let mut fields = vec![kind.to_string()];
        fields.extend(numbers.iter().map(|&(x, whole)| {
            if whole {
                format!("{}", x as i64)
            } else {
                format!("{x:.15E}")
            }
        }));
        let mut line = String::new();
        let count = fields.len();
        for (k, field) in fields.into_iter().enumerate() {
            let field = format!("{field}{}", if k + 1 == count { ';' } else { ',' });
            if line.len() + field.len() > 64 {
                self.parameters.push(format!("{line:<64} {at:>7}"));
                line.clear();
            }
            line.push_str(&field);
        }
        self.parameters.push(format!("{line:<64} {at:>7}"));
        let lines = self.parameters.len() + 1 - first;
        self.directory.push(format!(
            "{kind:>8}{first:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}{status:>8}",
            0, 0, 0, 0, 0, 0
        ));
        self.directory.push(format!(
            "{kind:>8}{:>8}{:>8}{lines:>8}{:>8}{:>8}{:>8}{:>8}{:>8}",
            0, 0, 0, "", "", "", 0
        ));
        at
    }

    fn curve(&mut self, curve: &Curve3, flat: bool) -> usize {
        let n = curve.control_points().len();
        let (lo, hi) = curve.domain();
        let mut numbers = vec![
            ((n - 1) as f64, true),
            (curve.degree() as f64, true),
            (f64::from(u8::from(flat)), true),
            (0.0, true),
            (f64::from(u8::from(!curve.is_rational())), true),
            (0.0, true),
        ];
        numbers.extend(curve.knots().iter().map(|k| (*k, false)));
        numbers.extend(curve.weights().iter().map(|w| (*w, false)));
        for p in curve.control_points() {
            numbers.extend(p.to_array().map(|x| (x, false)));
        }
        numbers.extend([
            (lo, false),
            (hi, false),
            (0.0, false),
            (0.0, false),
            (1.0, false),
        ]);
        self.add(126, if flat { "00010500" } else { "00010000" }, &numbers)
    }
}

/// `bodies` as an IGES file with its numbers in `unit`: every face a
/// trimmed surface (144) on a B-spline surface (128), bounded by curves
/// given both in space and in the surface's parameters (142).
pub fn export_iges(
    name: &str,
    bodies: &[(String, Solid)],
    unit: shapecraft_core::Unit,
) -> anyhow::Result<String> {
    let mut w = Writer::default();
    for (_, body) in bodies {
        for face in &body.faces {
            let s = &face.surface;
            let (p, q) = s.degrees();
            let (nu, nv) = s.counts();
            let (ku, kv) = s.knots();
            let mut numbers = vec![
                ((nu - 1) as f64, true),
                ((nv - 1) as f64, true),
                (p as f64, true),
                (q as f64, true),
            ];
            numbers.extend([(0.0, true); 5]);
            numbers.extend(ku.iter().chain(kv).map(|k| (*k, false)));
            // (Along u first, as the file has it.)
            let at = |i: usize, j: usize| i * nv + j;
            for j in 0..nv {
                numbers.extend((0..nu).map(|i| (s.weights()[at(i, j)], false)));
            }
            for j in 0..nv {
                for i in 0..nu {
                    numbers.extend(s.control_points()[at(i, j)].to_array().map(|x| (x, false)));
                }
            }
            let ((u0, u1), (v0, v1)) = s.domain();
            numbers.extend([u0, u1, v0, v1].map(|x| (x, false)));
            let surface = w.add(128, "00010000", &numbers);
            let mut bounds = Vec::new();
            for steps in &face.loops {
                let (mut space, mut flat) = (Vec::new(), Vec::new());
                for step in steps {
                    let (in_space, in_surface) = match step.walk {
                        Walk::Edge { edge, reversed } => {
                            let e = &body.edges[edge];
                            let piece = e.curve.trimmed(e.range.0, e.range.1)?;
                            let under = step.pcurve.trimmed(e.range.0, e.range.1)?;
                            if reversed {
                                (piece.reversed(), under.reversed())
                            } else {
                                (piece, under)
                            }
                        }
                        Walk::Pole { vertex, range } => {
                            let at = body.vertices[vertex].point;
                            let point =
                                Curve3::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![at, at], None)?;
                            (point, step.pcurve.trimmed(range.0, range.1)?)
                        }
                    };
                    let lifted = Curve3::new(
                        in_surface.degree(),
                        in_surface.knots().to_vec(),
                        in_surface
                            .control_points()
                            .iter()
                            .map(|p| Point3::new(p.x, p.y, 0.0))
                            .collect(),
                        Some(in_surface.weights().to_vec()),
                    )?;
                    space.push(w.curve(&in_space, false));
                    flat.push(w.curve(&lifted, true));
                }
                let chain = |w: &mut Writer, of: &[usize], status: &str| {
                    let mut numbers = vec![(of.len() as f64, true)];
                    numbers.extend(of.iter().map(|k| (*k as f64, true)));
                    w.add(102, status, &numbers)
                };
                let (b, c) = (
                    chain(&mut w, &flat, "00010500"),
                    chain(&mut w, &space, "00010000"),
                );
                let on = [1.0, surface as f64, b as f64, c as f64, 3.0].map(|x| (x, true));
                bounds.push(w.add(142, "00010000", &on));
            }
            let Some((outer, holes)) = bounds.split_first() else {
                continue;
            };
            let mut numbers = vec![
                (surface as f64, true),
                (1.0, true),
                (holes.len() as f64, true),
                (*outer as f64, true),
            ];
            numbers.extend(holes.iter().map(|h| (*h as f64, true)));
            w.add(144, "00000000", &numbers);
        }
    }
    anyhow::ensure!(!w.directory.is_empty(), "there is no face to write");
    Ok(w.finish(name, unit))
}

impl Writer {
    /// The file of what was added, called `name`, its numbers in `unit`.
    fn finish(&self, name: &str, unit: shapecraft_core::Unit) -> String {
        let (flag, called) = match unit {
            shapecraft_core::Unit::Inch => (1, "4HINCH"),
            shapecraft_core::Unit::Millimetre => (2, "2HMM"),
            shapecraft_core::Unit::Metre => (6, "1HM"),
            shapecraft_core::Unit::Centimetre => (10, "2HCM"),
        };
        let text = |s: &str| format!("{}H{s}", s.len());
        let global = format!(
            "1H,,1H;,{},{},{},{},32,38,6,308,15,{},1.0,{flag},{called},1,1.0,{},1.0E-6,1.0E6,,,11,0,{};",
            text(name),
            text(&format!("{name}.igs")),
            text("shapecraft"),
            text("shapecraft"),
            text(name),
            text("20261116.000000"),
            text("20261116.000000"),
        );
        let mut out = String::new();
        let _ = writeln!(
            out,
            "{:<72}S{:>7}",
            format!("{name}, written by ShapeCraft"),
            1
        );
        let chars: Vec<char> = global.chars().collect();
        let lines: Vec<String> = chars.chunks(72).map(|c| c.iter().collect()).collect();
        for (k, line) in lines.iter().enumerate() {
            let _ = writeln!(out, "{line:<72}G{:>7}", k + 1);
        }
        for (k, line) in self.directory.iter().enumerate() {
            let _ = writeln!(out, "{line:<72}D{:>7}", k + 1);
        }
        for (k, line) in self.parameters.iter().enumerate() {
            let _ = writeln!(out, "{line:<72}P{:>7}", k + 1);
        }
        let ends = format!(
            "S{:>7}G{:>7}D{:>7}P{:>7}",
            1,
            lines.len(),
            self.directory.len(),
            self.parameters.len()
        );
        let _ = writeln!(out, "{ends:<72}T{:>7}", 1);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planes_and_surfaces_of_revolution_are_read() -> anyhow::Result<()> {
        let real = |x: f64| (x, false);
        let whole = |x: usize| (x as f64, true);
        let mut w = Writer::default();
        // The plane z = 1, trimmed to a square 2 wide by four lines.
        let plane = w.add(
            108,
            "00010000",
            &[real(0.0), real(0.0), real(1.0), real(1.0), whole(0)],
        );
        let corners = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        let mut lines = vec![whole(4)];
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let line = [a[0], a[1], 1.0, b[0], b[1], 1.0].map(real);
            lines.push(whole(w.add(110, "00010000", &line)));
        }
        let chain = w.add(102, "00010000", &lines);
        let on = w.add(
            142,
            "00010000",
            &[whole(0), whole(plane), whole(0), whole(chain), whole(2)],
        );
        w.add(
            144,
            "00000000",
            &[whole(plane), whole(1), whole(0), whole(on)],
        );
        // A line 2 long, 1 from the z axis, turned all the way about it:
        // a tube, bounded by nothing but its own sides.
        let axis = w.add(110, "00010000", &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0].map(real));
        let side = w.add(110, "00010000", &[1.0, 0.0, 0.0, 1.0, 0.0, 2.0].map(real));
        let tube = w.add(
            120,
            "00010000",
            &[
                whole(axis),
                whole(side),
                real(0.0),
                real(std::f64::consts::TAU),
            ],
        );
        w.add(
            144,
            "00000000",
            &[whole(tube), whole(0), whole(0), whole(0)],
        );
        // And a line 3 long carried 2 up: a wall, bounded by its sides.
        let foot = w.add(110, "00010000", &[5.0, 0.0, 0.0, 8.0, 0.0, 0.0].map(real));
        let wall = w.add(
            122,
            "00010000",
            &[whole(foot), real(5.0), real(0.0), real(2.0)],
        );
        w.add(
            144,
            "00000000",
            &[whole(wall), whole(0), whole(0), whole(0)],
        );
        // A face of a solid, as IGES has those: on a plane through a
        // point square to a direction, around a loop of edges between
        // vertices — a square 2 wide at x = 20, its second edge walked
        // against the way its line runs.
        let at = w.add(116, "00010000", &[20.0, 0.0, 0.0].map(real));
        let facing = w.add(123, "00010000", &[1.0, 0.0, 0.0].map(real));
        let flat = w.add(190, "00010000", &[whole(at), whole(facing)]);
        let square = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        let mut vertices = vec![whole(4)];
        for [y, z] in square {
            vertices.extend([20.0, y, z].map(real));
        }
        let vertices = w.add(502, "00010000", &vertices);
        let mut edges = vec![whole(4)];
        for k in 0..4 {
            // (The second line is drawn from its end to its start.)
            let (from, to) = if k == 1 { (2, 1) } else { (k, (k + 1) % 4) };
            let (a, b) = (square[from], square[to]);
            let line = w.add(
                110,
                "00010000",
                &[20.0, a[0], a[1], 20.0, b[0], b[1]].map(real),
            );
            edges.extend([line, vertices, from + 1, vertices, to + 1].map(whole));
        }
        let edges = w.add(504, "00010000", &edges);
        let mut walk = vec![whole(4)];
        for k in 0..4 {
            walk.extend([0, edges, k + 1, usize::from(k != 1), 0].map(whole));
        }
        let walk = w.add(508, "00010000", &walk);
        w.add(
            510,
            "00010000",
            &[whole(flat), whole(1), whole(1), whole(walk)],
        );
        let text = w.finish("shapes", shapecraft_core::Unit::Centimetre);
        let found = import_iges(text.as_bytes(), "i")?;
        assert!(found.refused.is_empty(), "{:?}", found.refused);
        // (In centimetres: ten times as large in each direction.)
        let area: f64 = found
            .bodies
            .iter()
            .flat_map(|(body, _)| {
                (0..body.faces.len()).map(|f| shapecraft_boolean::face_area(body, f))
            })
            .sum();
        let wanted = (4.0 + std::f64::consts::TAU * 2.0 + 6.0 + 4.0) * 100.0;
        assert!((area - wanted).abs() < 1e-6 * wanted, "{area} {wanted}");
        Ok(())
    }
}
