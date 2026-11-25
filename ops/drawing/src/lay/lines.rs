//! Centre lines and the traces of datums (ISO 128-2, long-dashed dotted
//! narrow, type 04.1): of what is round in the model — bores, bosses,
//! turned bodies, told by their faces' geometry — and of its reference
//! planes, axes and points. A sheet draws some by itself (see
//! [`Centres`]), has others put on it one by one ([`SheetLine`]), and
//! shows the rest a view could have as what they would be.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::{Datum, World};

use super::views::Placed;
use super::{Laid, LaidNote, Pen, Placing, Text};
use crate::Sheet;
use crate::sheet::{Centres, Drafting, LabelAt, LineEnd, LineOf};

/// Something round of the model about one axis: the round faces about
/// one line, together (a counterbore and its bore are one).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Round {
    /// The name of its first round face (by name).
    pub name: String,
    /// The names of all its round faces.
    pub faces: Vec<String>,
    /// A point of its axis.
    pub foot: Point3,
    /// Its axis (a unit vector).
    pub axis: Point3,
    /// Its largest radius.
    pub radius: f64,
    /// How far along the axis from `foot` it reaches, least and most.
    pub from: f64,
    /// See `from`.
    pub to: f64,
    /// Whether it is a hole: its faces look towards the axis.
    pub hole: bool,
}

impl Round {
    /// Whether it is called `name`: a face of it is, or its faces'
    /// names start with `name/` (the step that made it) — or are a
    /// copy's of such a face, as a feature pattern (`bolts/2/bolt/…`)
    /// or a mirror (`mirror/bolt/…`) names it, copies of copies too.
    fn called(&self, name: &str) -> bool {
        let made_by = |f: &str| {
            f == name
                || f.strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with('/'))
        };
        self.faces.iter().any(|f| {
            let mut rest = f.as_str();
            loop {
                if made_by(rest) {
                    return true;
                }
                // (Past the copying step's name, and the copy's number.)
                let Some((_, after)) = rest.split_once('/') else {
                    return false;
                };
                rest = match after.split_once('/') {
                    Some((n, more)) if n.chars().all(|c| c.is_ascii_digit()) => more,
                    _ => after,
                };
            }
        })
    }
}

/// An arc of a circle along an edge: its middle, axis, radius, how far
/// it goes around, and its points.
fn circle_of(run: &[Point3]) -> Option<(Point3, Point3, f64, f64)> {
    let (a, b, c) = (run[0], run[run.len() / 3], run[2 * run.len() / 3]);
    let normal = (b - a).cross(c - a);
    let area = normal.length_squared();
    if area <= 1e-18 * a.distance(b).max(a.distance(c)).powi(4) {
        return None;
    }
    let (ab, ac) = (b - a, c - a);
    let middle = a
        + (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
            / (2.0 * area);
    let radius = middle.distance(a);
    let axis = normal.normalize();
    let round = run.iter().all(|p| {
        let d = *p - middle;
        (d.length() - radius).abs() <= radius * 1e-6 && d.dot(axis).abs() <= radius * 1e-6
    });
    if !round {
        return None;
    }
    let around = run
        .windows(2)
        .map(|w| (w[0] - middle).angle_between(w[1] - middle))
        .sum();
    Some((middle, axis, radius, around))
}

/// What is round of `world`'s solid bodies: each face that is part of
/// a cylinder or a cone about an axis its circular edges share, going
/// at least half way around; those about one line together.
fn find_rounds(world: &World) -> Vec<Round> {
    let mut faces: Vec<(String, Point3, Point3, f64, f64, f64, bool)> = Vec::new();
    for (id, body) in world.topo.bodies().iter() {
        if body.kind != workpiece_core::world::topo::BodyKind::Solid {
            continue;
        }
        let Ok(solid) = workpiece_boolean::read(&world.topo, id) else {
            continue;
        };
        for face in &solid.faces {
            let Some(name) = face.name.clone().or_else(|| face.also.first().cloned()) else {
                continue;
            };
            let mut circles = Vec::new();
            let mut points = Vec::new();
            for step in face.loops.iter().flatten() {
                let workpiece_boolean::model::Walk::Edge { edge, .. } = step.walk else {
                    continue;
                };
                let edge = &solid.edges[edge];
                let (t0, t1) = edge.range;
                let run: Vec<Point3> = (0..=24)
                    .map(|k| edge.curve.point_at(t0 + (t1 - t0) * f64::from(k) / 24.0))
                    .collect();
                if let Some(circle) = circle_of(&run) {
                    circles.push(circle);
                }
                points.extend(run);
            }
            let Some(&(c0, axis, _, _)) = circles.first() else {
                continue;
            };
            let size = circles.iter().map(|c| c.2).fold(0.0, f64::max);
            let near = size * 1e-6 + 1e-9;
            let on_line = |p: Point3| {
                let d = p - c0;
                (d - axis * d.dot(axis)).length() <= near * 10.0
            };
            if !circles
                .iter()
                .all(|c| c.1.cross(axis).length() <= 1e-6 && on_line(c.0))
            {
                continue;
            }
            // The radius along the axis, as the circles have it: the
            // same all along (a cylinder), or going on evenly (a cone).
            let along = |p: Point3| (p - c0).dot(axis);
            let (s0, r0) = (along(circles[0].0), circles[0].2);
            let other = circles
                .iter()
                .find(|c| (along(c.0) - s0).abs() > near * 100.0);
            let slope = other.map_or(0.0, |c| (c.2 - r0) / (along(c.0) - s0));
            let radius_at = |s: f64| r0 + slope * (s - s0);
            // Its surface, at points over its parameters, as round.
            let ((u0, u1), (v0, v1)) = face.surface.domain();
            let mut fits = true;
            let mut look = 0.0;
            for i in 0..5 {
                for j in 0..5 {
                    let (u, v) = (
                        u0 + (u1 - u0) * (f64::from(i) + 0.5) / 5.0,
                        v0 + (v1 - v0) * (f64::from(j) + 0.5) / 5.0,
                    );
                    let (p, du, dv) = face.surface.derivatives_at(u, v);
                    let s = along(p);
                    let out = p - c0 - axis * s;
                    if (out.length() - radius_at(s)).abs() > near * 100.0 {
                        fits = false;
                    }
                    let n = du.cross(dv).normalize_or_zero();
                    let n = if face.outward { n } else { -n };
                    look += n.dot(out.normalize_or_zero());
                }
            }
            if !fits {
                continue;
            }
            // (At least half way around, at one of its ends: not the
            // round of an edge.)
            let mut around: Vec<(f64, f64)> = Vec::new();
            for c in &circles {
                let s = along(c.0);
                match around.iter_mut().find(|a| (a.0 - s).abs() <= near * 100.0) {
                    Some(a) => a.1 += c.3,
                    None => around.push((s, c.3)),
                }
            }
            if around.iter().all(|a| a.1 < std::f64::consts::PI - 1e-6) {
                continue;
            }
            let (lo, hi) = points
                .iter()
                .map(|p| along(*p))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), s| {
                    (lo.min(s), hi.max(s))
                });
            let radius = radius_at(lo).max(radius_at(hi));
            let foot = c0;
            faces.push((name, foot, axis, radius, lo, hi, look < 0.0));
        }
    }
    faces.sort_by(|a, b| a.0.cmp(&b.0));
    // Those about one line, together.
    let mut rounds: Vec<Round> = Vec::new();
    for (name, foot, axis, radius, lo, hi, hole) in faces {
        let near = radius * 1e-5 + 1e-9;
        let same = rounds.iter_mut().find(|r| {
            let d = foot - r.foot;
            r.axis.cross(axis).length() <= 1e-6 && (d - r.axis * d.dot(r.axis)).length() <= near
        });
        match same {
            Some(r) => {
                let flip = if r.axis.dot(axis) < 0.0 { -1.0 } else { 1.0 };
                let shift = (foot - r.foot).dot(r.axis);
                let (a, b) = (shift + lo * flip, shift + hi * flip);
                r.from = r.from.min(a.min(b));
                r.to = r.to.max(a.max(b));
                r.radius = r.radius.max(radius);
                r.hole |= hole;
                r.faces.push(name);
            }
            None => rounds.push(Round {
                name: name.clone(),
                faces: vec![name],
                foot,
                axis,
                radius,
                from: lo,
                to: hi,
                hole,
            }),
        }
    }
    rounds
}

/// What is round of `world`, kept for the world it was found in.
pub(crate) fn rounds(world: &World) -> Arc<Vec<Round>> {
    let own = own_rounds(world);
    if world.instances.is_empty() {
        return own;
    }
    let mut all = (*own).clone();
    // And those of the parts placed in it, where they are placed, by
    // their names in the part after the name it is placed under.
    for (placed, instance) in &world.instances {
        let pose = instance.pose;
        for r in rounds(&instance.world).iter() {
            let named = |n: &String| format!("{placed}/{n}");
            all.push(Round {
                name: named(&r.name),
                faces: r.faces.iter().map(named).collect(),
                foot: pose.transform_point(r.foot),
                axis: pose.transform_vector(r.axis).normalize_or_zero(),
                ..r.clone()
            });
        }
    }
    Arc::new(all)
}

/// What is round of `world`'s own bodies, kept for the world it was
/// found in (where the parts placed in it are is not: they move).
fn own_rounds(world: &World) -> Arc<Vec<Round>> {
    static KEPT: OnceLock<Mutex<BTreeMap<u64, Arc<Vec<Round>>>>> = OnceLock::new();
    if world.stamp == 0 {
        return Arc::new(find_rounds(world));
    }
    let kept = KEPT.get_or_init(Default::default);
    if let Some(found) = kept
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&world.stamp)
    {
        return found.clone();
    }
    let found = Arc::new(find_rounds(world));
    let mut kept = kept.lock().unwrap_or_else(|e| e.into_inner());
    if kept.len() > 16 {
        kept.clear();
    }
    kept.insert(world.stamp, found.clone());
    found
}

/// A line as it is drawn in a view: its pieces, and where its name
/// stands while it is shown as what it would be.
struct Drawn {
    pieces: Vec<(Point2, Point2, Pen)>,
    label: Point2,
}

/// A centre mark (ISO 128-2): two centre lines crossing at `c` (on the
/// sheet), each a little past the circle `r` millimetres large — each
/// arm drawn from the middle out, so that they cross at long dashes;
/// of a circle less than `drafting.small` across, continuous narrow.
/// `ends` lengthens the across and the upright line.
pub(crate) fn mark(
    c: Point2,
    r: f64,
    drafting: &Drafting,
    ends: [f64; 2],
) -> Vec<(Point2, Point2, Pen)> {
    let mut pieces = Vec::new();
    for (way, more) in [(Point2::X, ends[0]), (Point2::Y, ends[1])] {
        let reach = (r + drafting.past + more).max(0.5);
        // (Of a small circle continuous narrow — unless its arms are long
        // enough for a long dash, a dot and a long dash: then chain, its
        // dashes fitted to them.)
        let pen = if 2.0 * r < drafting.small && reach < 4.0 {
            Pen::Narrow
        } else {
            Pen::Chain
        };
        for side in [way, -way] {
            // (Not quite from the middle: each arm a line by itself,
            // its dashes from there.)
            pieces.push((c + side * 1e-3, c + side * reach, pen));
        }
    }
    pieces
}

/// The centre marks and lines of what `placed` sees round in it, as
/// the views see them (of placed parts too): what a sheet that draws
/// every centre line by itself draws.
pub(crate) fn seen_centres(
    laid: &mut Laid,
    placed: &Placed<'_>,
    drafting: &Drafting,
    rounds: &[(Point2, f64)],
    axes: &[(Point2, Point2)],
    clip: &dyn Fn(Point2, Point2) -> Option<(Point2, Point2)>,
) {
    for (at, radius) in rounds {
        let c = placed.sheet(*at);
        for (a, b, pen) in mark(c, radius * placed.scale, drafting, [0.0; 2]) {
            let own = |p: Point2| placed.origin + (p - placed.centre) / placed.scale;
            if let Some((a, b)) = clip(own(a), own(b)) {
                laid.line(placed.sheet(a), placed.sheet(b), pen);
            }
        }
    }
    for (from, to) in axes {
        let way = (*to - *from).normalize_or_zero() * (drafting.past / placed.scale);
        if let Some((a, b)) = clip(*from - way, *to + way) {
            laid.line(placed.sheet(a), placed.sheet(b), Pen::Chain);
        }
    }
}

/// The infinite line through `p` along `way` (on the sheet) as much of
/// it as crosses the box from `lo` to `hi`.
fn across_box(p: Point2, way: Point2, lo: Point2, hi: Point2) -> Option<(Point2, Point2)> {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for (o, d, l, h) in [(p.x, way.x, lo.x, hi.x), (p.y, way.y, lo.y, hi.y)] {
        if d.abs() < 1e-12 {
            if o < l || o > h {
                return None;
            }
        } else {
            let (a, b) = ((l - o) / d, (h - o) / d);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
    }
    (t0 < t1).then(|| (p + way * t0, p + way * t1))
}

/// A line along `way` through `p`, across what the view `placed` draws
/// and on a little past its outline there (ISO 128-2: a centre line or
/// a datum's trace ends a short way past the outline, not across the
/// whole view); `ends` lengthens each end. In a detail, within its
/// circle.
fn through_view(
    placed: &Placed<'_>,
    p: Point2,
    way: Point2,
    d: &Drafting,
    ends: [f64; 2],
) -> Option<Drawn> {
    let way = way.try_normalize()?;
    let (lo, hi) = placed.bounds();
    let (a, b) = across_box(p, way, lo, hi)?;
    let at = |q: Point2| (q - p).dot(way);
    // Where it crosses (or runs along) the lines in view.
    let mut span: Option<(f64, f64)> = None;
    let mut take = |t: f64| {
        span = Some(match span {
            Some((l, h)) => (l.min(t), h.max(t)),
            None => (t, t),
        });
    };
    for (s, e, behind, _) in &placed.seen.lines {
        if *behind {
            continue;
        }
        let (s, e) = (placed.sheet(*s), placed.sheet(*e));
        let side = |q: Point2| (q - p).perp_dot(way);
        let (fs, fe) = (side(s), side(e));
        if fs.abs() <= 0.05 && fe.abs() <= 0.05 {
            take(at(s));
            take(at(e));
        } else if (fs > 0.0) != (fe > 0.0) {
            take(at(s.lerp(e, fs / (fs - fe))));
        }
    }
    let (t0, t1) = match span {
        Some((l, h)) if h - l > 1e-6 => (l.max(at(a)), h.min(at(b))),
        _ => (at(a), at(b)),
    };
    let (t0, t1) = (t0 - (d.past + ends[0]), t1 + (d.past + ends[1]));
    let (mut a, mut b) = (p + way * t0, p + way * t1);
    // (In a detail, what of it is within its circle.)
    if let Some(reach) = placed.reach {
        let (c, r) = (placed.centre, reach * placed.scale);
        let f = (a - c).dot(way);
        let off = (a - c).perp_dot(way);
        if off.abs() >= r {
            return None;
        }
        let half = (r * r - off * off).sqrt();
        let (s0, s1) = ((-f - half).max(0.0), (-f + half).min(t1 - t0));
        if s1 <= s0 {
            return None;
        }
        (a, b) = (a + way * s0, a + way * s1);
    }
    Some(Drawn {
        pieces: vec![(a, b, Pen::Chain)],
        label: b + way * 1.5,
    })
}

/// How `of` is drawn in the view `placed`; nothing where that view
/// cannot show it: a plane not seen edge on, what it does not name.
fn drawn(
    of: &LineOf,
    world: &World,
    rounds: &[Round],
    placed: &Placed<'_>,
    d: &Drafting,
    ends: [f64; 2],
) -> Option<Drawn> {
    // (Not in a detail, but the traces of datums: it shows what its
    // view has.)
    if placed.reach.is_some() && !matches!(of, LineOf::Datum(_)) {
        return None;
    }
    let towards = placed.basis[2];
    let axis_line = |origin: Point3, direction: Point3| {
        let way = placed.way(direction);
        if way.length() > 1e-6 {
            through_view(placed, placed.place(origin), way, d, ends)
        } else {
            // Seen end on: a point.
            let c = placed.place(origin);
            Some(Drawn {
                pieces: mark(c, 0.0, d, ends)
                    .into_iter()
                    .map(|(a, b, _)| (a, b, Pen::Narrow))
                    .collect(),
                label: c + Point2::new(1.5, 1.5) * (d.past + 1.0),
            })
        }
    };
    match of {
        // A flat pattern's bends (`flat` of `flat/bend1`, …): all their
        // centre lines.
        LineOf::Datum(name)
            if !world.datums.contains_key(name.as_str()) && {
                let prefix = format!("{name}/bend");
                world.datums.keys().any(|k| k.starts_with(&prefix))
            } =>
        {
            let prefix = format!("{name}/bend");
            let mut all: Option<Drawn> = None;
            for bend in world.datums.keys().filter(|k| k.starts_with(&prefix)) {
                let one = drawn(&LineOf::Datum(bend.clone()), world, rounds, placed, d, ends);
                match (&mut all, one) {
                    (Some(all), Some(one)) => all.pieces.extend(one.pieces),
                    (None, one) => all = one,
                    _ => {}
                }
            }
            all
        }
        LineOf::Datum(name) => {
            let (base, part) = match name.rsplit_once('/') {
                Some((base, part)) if matches!(part, "x" | "y" | "z" | "xy" | "yz" | "zx") => {
                    (base, Some(part))
                }
                _ => (name.as_str(), None),
            };
            match (world.datums.get(base)?, part) {
                (Datum::Plane(plane), None) => {
                    let n = plane.normal();
                    if n.dot(towards).abs() > 1e-6 {
                        return None;
                    }
                    through_view(
                        placed,
                        placed.place(plane.origin()),
                        placed.way(n.cross(towards)),
                        d,
                        ends,
                    )
                }
                (Datum::Axis(axis), None) => axis_line(axis.origin(), axis.direction()),
                (Datum::Frame(frame), None) => {
                    // A point: a small cross.
                    let c = placed.place(frame.origin());
                    Some(Drawn {
                        pieces: mark(c, 0.0, d, ends)
                            .into_iter()
                            .map(|(a, b, _)| (a, b, Pen::Narrow))
                            .collect(),
                        label: c + Point2::new(1.5, 1.5) * (d.past + 1.0),
                    })
                }
                // A frame's plane (`origin/zx`): its trace, seen edge on.
                (Datum::Frame(frame), Some(part @ ("xy" | "yz" | "zx"))) => {
                    let n = match part {
                        "xy" => frame.z(),
                        "yz" => frame.x(),
                        _ => frame.y(),
                    };
                    if n.dot(towards).abs() > 1e-6 {
                        return None;
                    }
                    through_view(
                        placed,
                        placed.place(frame.origin()),
                        placed.way(n.cross(towards)),
                        d,
                        ends,
                    )
                }
                (Datum::Frame(frame), Some(part)) => {
                    let direction = match part {
                        "x" => frame.x(),
                        "y" => frame.y(),
                        _ => frame.z(),
                    };
                    axis_line(frame.origin(), direction)
                }
                _ => None,
            }
        }
        LineOf::Axis(name) => {
            // (Of every round it names: a step's holes, all of them.)
            let mut all: Option<Drawn> = None;
            for round in rounds.iter().filter(|r| r.called(name)) {
                let facing = round.axis.dot(towards).abs();
                let one = if facing > 1.0 - 1e-9 {
                    let c = placed.place(round.foot);
                    Drawn {
                        pieces: mark(c, round.radius * placed.scale, d, ends),
                        label: c + Point2::new(1.0, 1.0)
                            * (round.radius * placed.scale * 0.7 + d.past),
                    }
                } else if facing < 1e-9 {
                    let (a, b) = (
                        placed.place(round.foot + round.axis * round.from),
                        placed.place(round.foot + round.axis * round.to),
                    );
                    let Some(way) = (b - a).try_normalize() else {
                        continue;
                    };
                    let (a, b) = (a - way * (d.past + ends[0]), b + way * (d.past + ends[1]));
                    Drawn {
                        pieces: vec![(a, b, Pen::Chain)],
                        label: b + way * 1.5,
                    }
                } else {
                    continue;
                };
                match &mut all {
                    Some(all) => all.pieces.extend(one.pieces),
                    None => all = Some(one),
                }
            }
            all
        }
        LineOf::Path(name) => {
            // Its curves as the sketch has them, where the view sees its
            // plane square on; run on past the ends as a centre line is.
            let profile = world.profiles.get(name)?;
            let frame = profile.plane.pose();
            if frame.transform_vector(Point3::Z).dot(towards).abs() < 1.0 - 1e-9 {
                return None;
            }
            let on = |p: Point2| placed.place(frame.transform_point(Point3::new(p.x, p.y, 0.0)));
            let mut pieces = Vec::new();
            for edge in &profile.edges {
                let points = edge.curve.polyline(edge.t0, edge.t1);
                pieces.extend(points.windows(2).map(|w| (on(w[0]), on(w[1]), Pen::Chain)));
            }
            // (Past its two loose ends: where only one piece ends.)
            let ends_at: Vec<Point2> = pieces.iter().flat_map(|p| [p.0, p.1]).collect();
            let loose = |p: Point2| ends_at.iter().filter(|q| q.distance(p) < 1e-6).count() == 1;
            let mut more = Vec::new();
            for (k, &(a, b, _)) in pieces.iter().enumerate() {
                for (end, from, n) in [(a, b, 0), (b, a, 1)] {
                    if loose(end) {
                        let way = (end - from).normalize_or_zero();
                        more.push((k, n, end + way * (d.past + ends[n.min(1)])));
                    }
                }
            }
            for (k, n, to) in more {
                match n {
                    0 => pieces[k].0 = to,
                    _ => pieces[k].1 = to,
                }
            }
            let label = pieces.last()?.1 + Point2::new(1.5, 1.5);
            Some(Drawn { pieces, label })
        }
        LineOf::Pitch(name) => {
            let (middle, axis, radius) = pitch(rounds, name)?;
            if axis.dot(towards).abs() < 1.0 - 1e-9 {
                return None;
            }
            let c = placed.place(middle);
            let r = radius * placed.scale;
            let steps = ((r * std::f64::consts::TAU / 1.5).ceil() as usize).clamp(24, 360);
            let at = |k: usize| {
                c + Point2::from_angle(std::f64::consts::TAU * k as f64 / steps as f64) * r
            };
            Some(Drawn {
                pieces: (0..steps).map(|k| (at(k), at(k + 1), Pen::Chain)).collect(),
                label: c + Point2::new(0.7, 0.7) * r,
            })
        }
    }
}

/// The circle through the middles of the holes about one middle of
/// which the hole `name` is one (three or more of them, alike, their
/// axes the same way): its middle, axis and radius.
fn pitch(rounds: &[Round], name: &str) -> Option<(Point3, Point3, f64)> {
    let first = rounds.iter().find(|r| r.called(name) && r.hole)?;
    let near = first.radius * 1e-4 + 1e-9;
    let alike: Vec<&Round> = rounds
        .iter()
        .filter(|r| {
            r.hole
                && r.axis.cross(first.axis).length() <= 1e-6
                && (r.radius - first.radius).abs() <= near
        })
        .collect();
    if alike.len() < 3 {
        return None;
    }
    let axis = first.axis;
    // (Their feet in one plane square to the axis.)
    let flat: Vec<Point3> = alike
        .iter()
        .map(|r| r.foot - axis * (r.foot - first.foot).dot(axis))
        .collect();
    let middle = flat.iter().fold(Point3::ZERO, |s, p| s + *p) / flat.len() as f64;
    let radius = middle.distance(flat[0]);
    let scale = radius.max(first.radius);
    (radius > near
        && flat
            .iter()
            .all(|p| (p.distance(middle) - radius).abs() <= scale * 1e-5))
    .then_some((middle, axis, radius))
}

/// Every line the view `placed` could show: the traces of the planes it
/// sees edge on, the axes, the points and the axes of frames, the
/// centre lines of what is round, and the pitch circles of holes.
fn offered(
    world: &World,
    rounds: &[Round],
    placed: &Placed<'_>,
    d: &Drafting,
) -> Vec<(LineOf, Drawn)> {
    let mut all: Vec<LineOf> = Vec::new();
    for (name, datum) in &world.datums {
        all.push(LineOf::Datum(name.clone()));
        if matches!(datum, Datum::Frame(_)) {
            for part in ["x", "y", "z"] {
                all.push(LineOf::Datum(format!("{name}/{part}")));
            }
        }
    }
    let mut pitched: Vec<(Point3, f64)> = Vec::new();
    for round in rounds {
        all.push(LineOf::Axis(round.name.clone()));
        if let Some((middle, _, radius)) = pitch(rounds, &round.name)
            && !pitched.iter().any(|(m, r)| {
                m.distance(middle) <= radius * 1e-6 && (r - radius).abs() <= radius * 1e-6
            })
        {
            pitched.push((middle, radius));
            all.push(LineOf::Pitch(round.name.clone()));
        }
    }
    all.into_iter()
        .filter_map(|of| drawn(&of, world, rounds, placed, d, [0.0; 2]).map(|shape| (of, shape)))
        .collect()
}

/// Whether `a` and `b` are the same line: what is round by any of its
/// names, a pitch circle by any of its holes'.
fn same(rounds: &[Round], a: &LineOf, b: &LineOf) -> bool {
    let round = |name: &str| rounds.iter().position(|r| r.called(name));
    match (a, b) {
        (LineOf::Axis(x), LineOf::Axis(y)) => {
            x == y || round(x).is_some_and(|k| round(y) == Some(k))
        }
        (LineOf::Pitch(x), LineOf::Pitch(y)) => {
            x == y
                || match (pitch(rounds, x), pitch(rounds, y)) {
                    (Some((m, _, r)), Some((n, _, s))) => {
                        m.distance(n) <= r * 1e-6 && (r - s).abs() <= r * 1e-6
                    }
                    _ => false,
                }
        }
        _ => a == b,
    }
}

/// Whether the sheet draws `of` by itself (see [`Centres`]).
fn by_itself(sheet: &Sheet, rounds: &[Round], of: &LineOf) -> bool {
    match (sheet.centres, of) {
        (Centres::All, LineOf::Axis(_)) => true,
        (Centres::Holes, LineOf::Axis(name)) => rounds.iter().any(|r| r.called(name) && r.hole),
        _ => false,
    }
}

/// The box of the pieces of a line.
fn bounds(pieces: &[(Point2, Point2, Pen)]) -> [Point2; 2] {
    pieces.iter().fold(
        [
            Point2::splat(f64::INFINITY),
            Point2::splat(f64::NEG_INFINITY),
        ],
        |[lo, hi], (a, b, _)| [lo.min(*a).min(*b), hi.max(*a).max(*b)],
    )
}

/// `pieces` (of one line, end to end) with the stretches `gaps` — from
/// and to, millimetres along it from its start — left out.
fn broken(pieces: &[(Point2, Point2, Pen)], gaps: &[[f64; 2]]) -> Vec<(Point2, Point2, Pen)> {
    if gaps.is_empty() {
        return pieces.to_vec();
    }
    let mut out = Vec::new();
    let mut s = 0.0;
    for &(a, b, pen) in pieces {
        let length = a.distance(b);
        // (What of this piece, from and to along it, is in no gap.)
        let mut keep = vec![(0.0_f64, length)];
        for [g0, g1] in gaps {
            let (g0, g1) = (g0.min(*g1) - s, g0.max(*g1) - s);
            keep = keep
                .into_iter()
                .flat_map(|(x, y)| [(x, y.min(g0)), (x.max(g1), y)])
                .filter(|(x, y)| y - x > 1e-9)
                .collect();
        }
        let way = (b - a).normalize_or_zero();
        out.extend(
            keep.into_iter()
                .map(|(x, y)| (a + way * x, a + way * y, pen)),
        );
        s += length;
    }
    out
}

/// The marks of a line `pieces` (end to end): of a line of symmetry two
/// short parallel narrow lines square across each end (ISO 128-2); and
/// words `label` at its far end.
fn marked(
    laid: &mut Laid,
    pieces: &[(Point2, Point2, Pen)],
    symmetry: bool,
    label: Option<&str>,
    at: Option<LabelAt>,
) {
    let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
        return;
    };
    let (start, end) = (first.0, last.1);
    let way = (end - start).normalize_or_zero();
    if way == Point2::ZERO {
        return;
    }
    let across = way.perp();
    if symmetry {
        for (at, inward) in [(start, way), (end, -way)] {
            for k in [0.0, 1.0] {
                let c = at + inward * (2.0 + k * 1.5);
                laid.line(c - across * 3.0, c + across * 3.0, Pen::Narrow);
            }
        }
    }
    if let Some(label) = label {
        // (Its words once all else is written: see `labels`.)
        LABELS.with(|l| {
            l.borrow_mut().push((start, end, label.to_owned(), at));
        });
    }
}

/// Words to put on a line: its start and end, the words, where asked.
type Label = (Point2, Point2, String, Option<LabelAt>);

thread_local! {
    /// The words of the lines of the sheet being laid out, to be put on
    /// them once all else stands: the line's start and end, the words,
    /// and where they are asked to stand.
    static LABELS: std::cell::RefCell<Vec<Label>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Writes the words of the lines on them (`CL SYM`): where they are
/// asked to stand, else at whichever end they stand clear of what is
/// written and drawn there (dimensions, their lines).
pub(crate) fn labels(laid: &mut Laid) {
    let pending = LABELS.with(|l| std::mem::take(&mut *l.borrow_mut()));
    for (start, end, label, at) in pending {
        let way = (end - start).normalize_or_zero();
        if way == Point2::ZERO {
            continue;
        }
        // (Upright or from the left, never upside down.)
        let flipped = way.x < -1e-9 || (way.x.abs() <= 1e-9 && way.y < 0.0);
        let read = if flipped { -way } else { way };
        let height = super::lettering();
        // The words' box, their foot at `foot` going on along the line
        // the way `out` (on past an end) or centred (along it).
        let place = |foot: Point2, out: Point2| -> (Point2, [f64; 2]) {
            // (Beside the line, a little off it: not across its dashes.)
            let side = read.perp() * (height * 0.5 + 0.8);
            match (out.dot(read) >= 0.0, out == Point2::ZERO) {
                (_, true) => (foot + side, [0.5, 0.0]),
                (true, false) => (foot + out * 1.5 + side * 0.0, [0.0, 0.5]),
                (false, false) => (foot + out * 1.5, [1.0, 0.5]),
            }
        };
        let candidates: Vec<(Point2, [f64; 2])> = match at {
            Some(LabelAt::End(LineEnd::Start)) => vec![place(start, -way)],
            Some(LabelAt::End(LineEnd::End)) => vec![place(end, way)],
            Some(LabelAt::Along(d)) => vec![place(start + way * d, Point2::ZERO)],
            None => vec![place(end, way), place(start, -way)],
        };
        let boxes: Vec<[Point2; 2]> = laid
            .texts
            .iter()
            .filter(|t| !t.preview)
            .map(super::notes::text_box)
            .collect();
        let clear = |(foot, anchor): &(Point2, [f64; 2])| {
            let text = super::Text {
                at: *foot,
                text: label.clone(),
                height,
                anchor: *anchor,
                angle: read.y.atan2(read.x),
                preview: false,
            };
            let [lo, hi] = super::notes::text_box(&text);
            let (lo, hi) = (lo - Point2::splat(0.5), hi + Point2::splat(0.5));
            boxes
                .iter()
                .all(|b| b[1].x <= lo.x || hi.x <= b[0].x || b[1].y <= lo.y || hi.y <= b[0].y)
                && laid.strokes.iter().all(|s| {
                    s.preview
                        || matches!(s.pen, Pen::Chain)
                        || !(0..=8).any(|k| {
                            let q = s.a.lerp(s.b, f64::from(k) / 8.0);
                            q.x > lo.x && q.x < hi.x && q.y > lo.y && q.y < hi.y
                        })
                })
        };
        let (foot, anchor) = candidates
            .iter()
            .find(|c| clear(c))
            .or(candidates.first())
            .copied()
            .unwrap_or((end, [0.0, 0.5]));
        laid.texts.push(super::Text {
            at: foot,
            text: label,
            height,
            anchor,
            angle: read.y.atan2(read.x),
            preview: false,
        });
    }
}

/// Draws the lines of `sheet` in its views `frames`: the centre lines
/// of holes where the sheet draws those by itself, and those put on it;
/// with `previews` (in the view named, or every view), what else each
/// view could show, as what it would be, with its name.
pub(crate) fn draw(
    laid: &mut Laid,
    sheet: &Sheet,
    world: &World,
    frames: &[Placed<'_>],
    previews: Option<Option<&str>>,
) {
    let rounds = rounds(world);
    let d = &sheet.drafting;
    // (Of everything round, the views draw them as they see them.)
    if sheet.centres == Centres::Holes {
        for placed in frames {
            for round in rounds.iter().filter(|r| r.hole) {
                let of = LineOf::Axis(round.name.clone());
                // (Not one the sheet has a say of: taken off, broken,
                // marked — drawn as that says, below.)
                let said = sheet.lines.iter().any(|l| {
                    l.view.as_ref().is_none_or(|v| *v == placed.view.id)
                        && rounds
                            .iter()
                            .find(|r| {
                                r.called(match &l.of {
                                    LineOf::Axis(n) => n,
                                    _ => "",
                                })
                            })
                            .is_some_and(|r| r.name == round.name)
                });
                if said {
                    continue;
                }
                if let Some(shape) = drawn(&of, world, &rounds, placed, d, [0.0; 2]) {
                    for (a, b, pen) in shape.pieces {
                        laid.line(a, b, pen);
                    }
                }
            }
        }
    }
    // Those put on it.
    for (index, line) in sheet.lines.iter().enumerate() {
        let mut shown = false;
        for placed in frames {
            if line.view.as_ref().is_some_and(|v| *v != placed.view.id) {
                continue;
            }
            let Some(mut shape) = drawn(&line.of, world, &rounds, placed, d, line.ends) else {
                continue;
            };
            shown = true;
            // (What the sheet draws of it by itself — every centre line
            // — gives way: this one is drawn as it says.)
            if matches!(line.of, LineOf::Axis(_)) {
                let (lo, hi) = placed.bounds();
                let (lo, hi) = (lo - Point2::splat(20.0), hi + Point2::splat(20.0));
                let over = |s: &super::Stroke| {
                    matches!(s.pen, Pen::Chain | Pen::Narrow)
                        && [s.a, s.b]
                            .iter()
                            .all(|p| p.cmpge(lo).all() && p.cmple(hi).all())
                        && shape.pieces.iter().any(|(a, b, _)| {
                            let way = (*b - *a).normalize_or_zero();
                            way != Point2::ZERO
                                && [s.a, s.b]
                                    .iter()
                                    .all(|p| (*p - *a).perp_dot(way).abs() <= 0.05)
                                && (s.b - s.a).normalize_or_zero().perp_dot(way).abs() < 1e-6
                        })
                };
                laid.strokes.retain(|s| !over(s));
            }
            if line.hidden {
                continue;
            }
            shape.pieces = broken(&shape.pieces, &line.gaps);
            for (a, b, pen) in &shape.pieces {
                laid.line(*a, *b, *pen);
            }
            marked(
                laid,
                &shape.pieces,
                line.symmetry,
                line.label.as_deref(),
                line.label_at,
            );
            // (What the sheet's own dimensions are put between: the
            // line, its ends and its middle.)
            if let Some(view) = laid.views.iter_mut().find(|v| v.id == placed.view.id) {
                for (a, b, _) in &shape.pieces {
                    view.lines.push((*a, *b));
                    view.snaps.extend([*a, *b]);
                }
            }
            laid.notes.push(LaidNote {
                index,
                kind: Placing::Line(line.of.clone()),
                rect: bounds(&shape.pieces),
                view: placed.view.id.clone(),
                preview: None,
                linear: None,
                path: shape.pieces.iter().map(|(a, b, _)| (*a, *b)).collect(),
                tips: Vec::new(),
            });
        }
        if !shown {
            laid.problems.push(format!(
                "the line of {} shows in no view{}",
                line.of.label(),
                line.view
                    .as_ref()
                    .map(|v| format!(" (asked for in {v})"))
                    .unwrap_or_default()
            ));
        }
    }
    // What else the views could show.
    let Some(only) = previews else {
        return;
    };
    for placed in frames {
        if only.is_some_and(|id| id != placed.view.id) {
            continue;
        }
        for (of, shape) in offered(world, &rounds, placed, d) {
            let put = sheet.lines.iter().any(|l| {
                same(&rounds, &l.of, &of) && l.view.as_ref().is_none_or(|v| *v == placed.view.id)
            });
            if put || by_itself(sheet, &rounds, &of) {
                continue;
            }
            for (a, b, pen) in &shape.pieces {
                laid.strokes.push(super::Stroke {
                    a: *a,
                    b: *b,
                    pen: *pen,
                    preview: true,
                });
            }
            let label = of.label();
            let height = 2.5;
            laid.texts.push(Text {
                at: shape.label,
                text: label.clone(),
                height,
                anchor: [0.0, 0.0],
                angle: 0.0,
                preview: true,
            });
            let w = super::notes::width(&label, height);
            let rect = [shape.label, shape.label + Point2::new(w, height)];
            laid.notes.push(LaidNote {
                index: 0,
                kind: Placing::Line(of),
                rect,
                view: placed.view.id.clone(),
                preview: Some((label, [0.0, 0.0])),
                linear: None,
                path: shape.pieces.iter().map(|(a, b, _)| (*a, *b)).collect(),
                tips: Vec::new(),
            });
        }
    }
}

/// Of the line from `a` to `b` (on the sheet), what is within the
/// circle of the detail `placed`; all of it, of a view that is no
/// detail.
pub(crate) fn in_circle(placed: &Placed<'_>, a: Point2, b: Point2) -> Option<(Point2, Point2)> {
    let Some(reach) = placed.reach else {
        return Some((a, b));
    };
    let (c, r) = (placed.centre, reach * placed.scale);
    let d = b - a;
    let length = d.length();
    if length < 1e-12 {
        return None;
    }
    let way = d / length;
    let f = (a - c).dot(way);
    let off = (a - c).perp_dot(way);
    if off.abs() >= r {
        return None;
    }
    let half = (r * r - off * off).sqrt();
    let (s0, s1) = ((-f - half).max(0.0), (-f + half).min(length));
    (s1 > s0).then(|| (a + way * s0, a + way * s1))
}

/// The traces of the datums of `world` the view `placed` sees edge on
/// — its datum planes, and the planes of its frames (the origin's XY,
/// YZ and ZX) — as they are drawn there (chain lines a little past the
/// outline), on the sheet.
pub(crate) fn traces(
    world: &World,
    placed: &Placed<'_>,
    d: &Drafting,
) -> Vec<(Point2, Point2, Pen)> {
    let mut traces: Vec<(Point2, Point2, Pen)> = Vec::new();
    for name in world.datums.keys() {
        let of = LineOf::Datum(name.clone());
        if let Some(shape) = drawn(&of, world, &[], placed, d, [0.0; 2])
            && let [piece] = shape.pieces[..]
        {
            traces.push(piece);
        }
    }
    let towards = placed.basis[2];
    for datum in world.datums.values() {
        let Datum::Frame(frame) = datum else {
            continue;
        };
        for normal in [frame.z(), frame.x(), frame.y()] {
            if normal.dot(towards).abs() > 1e-6 {
                continue;
            }
            let way = placed.way(normal.cross(towards));
            if let Some(shape) =
                through_view(placed, placed.place(frame.origin()), way, d, [0.0; 2])
                && let [piece] = shape.pieces[..]
            {
                traces.push(piece);
            }
        }
    }
    traces
}

/// Where a dimension of the view `placed` ends at `p` on the trace of a
/// datum (a plane's offset): that trace drawn (on to `p`, if it is
/// beyond the outline). Whether there is one.
pub(crate) fn datum_through(
    laid: &mut Laid,
    world: &World,
    placed: &Placed<'_>,
    d: &Drafting,
    p: Point2,
) -> bool {
    for (a, b, pen) in traces(world, placed, d) {
        let way = (b - a).normalize_or_zero();
        if (p - a).perp_dot(way).abs() > 0.05 {
            continue;
        }
        // (On past the point, if it is beyond what the view shows —
        // not out of a detail's circle, which is all a detail shows.)
        let (s, e) = (
            0.0_f64.min((p - a).dot(way) - d.past),
            (b - a).length().max((p - a).dot(way) + d.past),
        );
        let (from, to) = (a + way * s, a + way * e);
        let Some((from, to)) = in_circle(placed, from, to) else {
            return true;
        };
        laid.line(from, to, pen);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_names_its_holes_and_their_copies() {
        let round = |face: &str| Round {
            name: face.into(),
            faces: vec![face.into()],
            foot: Point3::ZERO,
            axis: Point3::Z,
            radius: 1.0,
            from: 0.0,
            to: 1.0,
            hole: true,
        };
        for face in [
            "bolt/side/e1",
            "bolts/2/bolt/side/e1",
            "mirror_x/bolt/side",
            "mirror_y/mirror_x/bolt/side",
        ] {
            assert!(round(face).called("bolt"), "{face}");
        }
        assert!(!round("bolts/2/side/e1").called("bolt"));
        assert!(!round("boltx/side").called("bolt"));
    }
}
