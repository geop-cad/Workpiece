//! Dimension: a size of the model, said on the model with its
//! tolerance — a distance, a length, a diameter or radius, an angle.

use anyhow::{Context, ensure};
use shapecraft_core::math::Point3;
use shapecraft_core::widgets::{Choice, NumericInput, TextInput};
use shapecraft_core::world::topo::{EdgeId, Topo};
use shapecraft_core::world::{Annotation, DatumAxis, DatumPlane, Measured, Note, Tolerance, World};
use shapecraft_core::{
    Builder, EditContext, Operation, Pose, Position, Reference, ReferenceKind, Resolve, Source,
    Widget,
};

/// What is measured.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum DimensionKind {
    /// What fits what is picked: of one round face its diameter, of one
    /// edge its length; of two things their distance, or their angle
    /// if they are not parallel.
    #[default]
    Auto,
    /// How far two things are apart — faces, edges, points — or how
    /// long one edge is.
    Distance,
    /// The diameter of a round face.
    Diameter,
    /// The radius of a round face.
    Radius,
    /// The angle between two flat faces, or two straight edges.
    Angle,
}

impl shapecraft_core::Choices for DimensionKind {
    const ALL: &'static [Self] = &[
        DimensionKind::Auto,
        DimensionKind::Distance,
        DimensionKind::Diameter,
        DimensionKind::Radius,
        DimensionKind::Angle,
    ];

    fn label(self) -> &'static str {
        match self {
            DimensionKind::Auto => "As picked",
            DimensionKind::Distance => "Distance or length",
            DimensionKind::Diameter => "Diameter",
            DimensionKind::Radius => "Radius",
            DimensionKind::Angle => "Angle",
        }
    }
}

/// Which radius of something that bends two ways is meant.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Bend {
    /// The tighter round: a bent bar's own radius, a fillet's.
    #[default]
    Own,
    /// The bend's, along the middle of what is bent.
    Middle,
    /// The bend's on its inside.
    Inside,
    /// The bend's on its outside.
    Outside,
}

/// What is measured, of what, and how exactly it has to be.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct DimensionArgs {
    /// What is measured.
    #[arg(choice, label = "Measure")]
    pub kind: DimensionKind,
    /// What it is measured of: two things for a distance or an angle
    /// (one edge for its length), one round face for a diameter or a
    /// radius.
    #[arg(with = "of_field")]
    pub of: Vec<Reference<shapecraft_core::Anything>>,
    /// For an angle: the other one of the two the directions make —
    /// what is left of a half turn (45° where the faces themselves
    /// stand 135° apart: how far a chamfer leans off its wall).
    #[serde(default)]
    #[arg(
        label = "Angle",
        options = "Between them|The other one",
        when = "self.kind == DimensionKind::Angle"
    )]
    pub supplement: bool,
    /// For a distance to or between round things: to their far sides,
    /// not their near ones.
    #[serde(default)]
    #[arg(hidden)]
    pub far: bool,
    /// For a distance to or between round things: to their middles
    /// (their axes, a ball's centre), not their sides.
    #[serde(default)]
    #[arg(hidden)]
    pub centres: bool,
    /// For a distance between round things: both on the same side —
    /// the far side of the one from the other, and the side of the
    /// other that faces the same way (how far a rounded end stands out
    /// past another).
    #[serde(default)]
    #[arg(hidden)]
    pub same_side: bool,
    /// For a distance: the direction it is measured along, from how
    /// far the one thing reaches that way to how far the other does —
    /// the top of a round bar over a face, the height of one thing
    /// over another that is not square to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[arg(hidden)]
    pub along: Option<Reference<DatumAxis>>,
    /// For the radius of something that bends two ways (a bent bar, a
    /// fillet around a rim): which of its radii — the tighter one (the
    /// bar's own), or of the bend it is swept around.
    #[serde(default)]
    #[arg(hidden)]
    pub bend: Bend,
    /// A radius or diameter written as the bare number, without its
    /// `R` or `Ø`: as a reference in brackets has it, "(3)".
    #[serde(default)]
    #[arg(hidden)]
    pub plain: bool,
    /// Its tolerance.
    #[serde(default)]
    #[arg(with = "tolerance_fields")]
    pub tolerance: Tolerance,
    /// Where its text stands, if that was said: a click beside the
    /// model, with what it measures picked, puts it there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[arg(hidden)]
    pub at: Option<[f64; 3]>,
}

impl Default for DimensionArgs {
    fn default() -> Self {
        Self {
            kind: DimensionKind::Auto,
            of: Vec::new(),
            supplement: false,
            far: false,
            centres: false,
            same_side: false,
            along: None,
            bend: Bend::Own,
            plain: false,
            tolerance: Tolerance::General,
            at: None,
        }
    }
}

/// Says a size of the model, with its tolerance. The value is the
/// model's own: it follows the model.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Dimension {
    /// The operation's id.
    pub id: String,
    /// What is measured and its tolerance.
    pub args: DimensionArgs,
}

/// What something picked is, to measure from: a plane (a flat face), a
/// line (a straight edge, an axis), a point.
enum Thing {
    Plane(DatumPlane),
    Line(DatumAxis),
    Point(Point3),
}

fn thing(world: &World, source: &Source) -> anyhow::Result<Thing> {
    if let Ok(plane) = DatumPlane::resolve(world, source) {
        return Ok(Thing::Plane(plane));
    }
    if let Ok(axis) = DatumAxis::resolve(world, source) {
        return Ok(Thing::Line(axis));
    }
    if let Ok(point) = Position::resolve(world, source) {
        return Ok(Thing::Point(point.0));
    }
    anyhow::bail!(
        "{} is no flat face, straight edge or point to measure from",
        source.described()
    )
}

/// The world what `source` names is of, the name it has there, and
/// where that world lies in `world`: `world` itself, or the world of a
/// part placed in it (and so on down), where that part is.
fn within<'w>(world: &'w World, source: &'w Source) -> Option<(&'w World, &'w str, Pose)> {
    match source {
        Source::Placed { instance, source } => {
            let placed = world.instances.get(instance)?;
            let (inner, name, pose) = within(&placed.world, source)?;
            Some((inner, name, placed.pose * pose))
        }
        _ => Some((world, source.name(), Pose::IDENTITY)),
    }
}

/// Whether `source` names an edge (of the part, or of one placed).
fn is_edge(world: &World, source: &Source) -> bool {
    within(world, source).is_some_and(|(w, name, _)| w.topo.resolve::<EdgeId>(name).is_some())
}

/// The ends of the edge `source` names.
fn ends(world: &World, source: &Source) -> Option<(Point3, Point3)> {
    let (world, name, pose) = within(world, source)?;
    let topo = &world.topo;
    let edge = topo.edge(topo.resolve::<EdgeId>(name)?)?;
    let curve = topo.curve(edge.curve)?;
    Some((
        pose.transform_point(curve.point_at(edge.t0)),
        pose.transform_point(curve.point_at(edge.t1)),
    ))
}

/// The round face called by `source`, if it is one: a point of its
/// axis, the axis, and its radius.
fn round(world: &World, source: &Source) -> Option<(Point3, Point3, f64)> {
    let (world, name, pose) = within(world, source)?;
    let (solid, face) = shapecraft_shape::on::face(&world.topo, name).ok()?;
    let round = shapecraft_shape::thread::cylinder(&solid, face).ok()?;
    let middle = (round.reach.0 + round.reach.1) / 2.0;
    Some((
        pose.transform_point(round.origin + round.axis * middle),
        pose.transform_vector(round.axis),
        round.radius,
    ))
}

/// Something round, however it was made: a point of its axis (its
/// centre), the axis, its radius, whether it goes all the way around,
/// and whether it is round every way (a ball: it has a centre, no
/// axis).
struct Rounded {
    centre: Point3,
    axis: Point3,
    radius: f64,
    whole: bool,
    ball: bool,
    /// Whether it is an edge: a circle in a plane, not a face.
    edge: bool,
    /// Of what bends two ways, the wider round: the centre, axis and
    /// radius of the circle the tighter one's middle goes around.
    bend: Option<(Point3, Point3, f64)>,
}

/// The centre and the axis of a round thing as its middle is measured
/// to: a bend's (the point of the axis the bend goes around that is
/// nearest the face's middle), else its own.
fn middle_of(r: &Rounded) -> (Point3, Point3) {
    r.bend.map_or((r.centre, r.axis), |(c, a, _)| (c, a))
}

/// The circle through three points: its centre, its axis and its
/// radius. Nothing for points in a line.
fn circle(a: Point3, b: Point3, c: Point3) -> Option<(Point3, Point3, f64)> {
    let (u, v) = (b - a, c - a);
    let n = u.cross(v);
    let size = u.length().max(v.length());
    if n.length() <= 1e-9 * size * size {
        return None;
    }
    let centre = a
        + (v.cross(n) * u.length_squared() + n.cross(u) * v.length_squared())
            / (2.0 * n.length_squared());
    Some((centre, n.normalize(), centre.distance(a)))
}

/// The circle the points `at(0)` to `at(1)` run along, if they do: told
/// by three of them, held by the others to a hundred-thousandth of its
/// radius.
fn arc_through(at: &dyn Fn(f64) -> Point3) -> Option<(Point3, Point3, f64)> {
    let closed = at(0.0).distance(at(1.0)) <= 1e-9 * (1.0 + at(0.0).distance(at(0.5)));
    let third = if closed { 2.0 / 3.0 } else { 1.0 };
    let (centre, axis, radius) = circle(at(0.0), at(third / 2.0), at(third))?;
    // (To a hundred-thousandth of its radius: what a curve fitted to
    // an arc — the edge of a sweep along one — is off it.)
    let holds = [0.125, 0.25, 0.375, 0.625, 0.75, 0.875].iter().all(|t| {
        let p = at(*t);
        (p.distance(centre) - radius).abs() <= 1e-5 * radius
            && (p - centre).dot(axis).abs() <= 1e-5 * radius
    });
    holds.then_some((centre, axis, radius))
}

/// What `source` names as something round: a face of a cylinder, an
/// edge that is an arc or a circle, or a face that bends as a circle
/// one way across (a fillet around a rim, a ball, what a sweep makes
/// along an arc) — of such a face, the tighter of its two rounds.
fn rounded(world: &World, source: &Source) -> Option<Rounded> {
    let (world, name, pose) = within(world, source)?;
    let mut rounded = rounded_in(&world.topo, name)?;
    // (Where the part it is of is placed.)
    rounded.centre = pose.transform_point(rounded.centre);
    rounded.axis = pose.transform_vector(rounded.axis);
    rounded.bend = rounded
        .bend
        .map(|(c, a, r)| (pose.transform_point(c), pose.transform_vector(a), r));
    Some(rounded)
}

/// [`rounded`], of what is called `name` in `topo`.
fn rounded_in(topo: &Topo, name: &str) -> Option<Rounded> {
    // An edge: an arc, or a circle.
    if let Some(edge) = topo.resolve::<EdgeId>(name).and_then(|id| topo.edge(id)) {
        let curve = topo.curve(edge.curve)?;
        let (t0, t1) = (edge.t0, edge.t1);
        let at = |s: f64| curve.point_at(t0 + (t1 - t0) * s);
        let (centre, axis, radius) = arc_through(&at)?;
        let whole = at(0.0).distance(at(1.0)) <= 1e-9 * (1.0 + radius);
        return Some(Rounded {
            centre,
            axis,
            radius,
            whole,
            ball: false,
            edge: true,
            bend: None,
        });
    }
    let (solid, face) = shapecraft_shape::on::face(topo, name).ok()?;
    if let Ok(round) = shapecraft_shape::thread::cylinder(&solid, face) {
        // All of the body's faces on this cylinder, together (a full
        // one may be of two halves).
        let on_it = |f: usize| {
            shapecraft_shape::thread::cylinder(&solid, f)
                .ok()
                .filter(|c| {
                    (c.radius - round.radius).abs() < 1e-9 * (1.0 + round.radius)
                        && c.axis.cross(round.axis).length() < 1e-9
                        && (c.origin - round.origin).cross(round.axis).length()
                            < 1e-9 * (1.0 + round.radius)
                })
        };
        let area: f64 = (0..solid.faces.len())
            .filter(|&f| on_it(f).is_some())
            .map(|f| shapecraft_boolean::face_area(&solid, f))
            .sum();
        let long = (round.reach.1 - round.reach.0).abs();
        let whole = area >= std::f64::consts::TAU * round.radius * long * 0.99;
        let middle = (round.reach.0 + round.reach.1) / 2.0;
        return Some(Rounded {
            centre: round.origin + round.axis * middle,
            axis: round.axis,
            radius: round.radius,
            whole,
            ball: false,
            edge: false,
            bend: None,
        });
    }
    // Any other face: how it bends across its middle, each way.
    let surface = &solid.faces[face].surface;
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    let along_u = arc_through(&|s| surface.point_at(u0 + (u1 - u0) * s, vm));
    let along_v = arc_through(&|s| surface.point_at(um, v0 + (v1 - v0) * s));
    let (centre, axis, radius, ball, bend) = match (along_u, along_v) {
        (Some(a), Some(b)) => {
            let ball = (a.2 - b.2).abs() <= 1e-5 * a.2 && a.0.distance(b.0) <= 1e-5 * a.2;
            let (tighter, wider) = if a.2 <= b.2 { (a, b) } else { (b, a) };
            // The bend: about the wider round's axis, as far out as the
            // tighter one's middle is from it.
            let off = tighter.0 - wider.0;
            let off = off - wider.1 * off.dot(wider.1);
            let bend = (!ball && off.length() > 1e-9 * wider.2).then(|| {
                let on = wider.0 + wider.1 * (tighter.0 - wider.0).dot(wider.1);
                (on, wider.1, off.length())
            });
            (tighter.0, tighter.1, tighter.2, ball, bend)
        }
        (Some(a), None) | (None, Some(a)) => {
            // (Round every way, though only one way across its middle
            // tells a circle — a ball whose other way runs through its
            // poles: all of it as far from the middle.)
            let ball = (0..=4).all(|i| {
                (0..=4).all(|j| {
                    let p = surface.point_at(
                        u0 + (u1 - u0) * f64::from(i) / 4.0,
                        v0 + (v1 - v0) * f64::from(j) / 4.0,
                    );
                    (p.distance(a.0) - a.2).abs() <= 1e-5 * a.2
                })
            });
            (a.0, a.1, a.2, ball, None)
        }
        (None, None) => return None,
    };
    Some(Rounded {
        centre,
        axis,
        radius,
        whole: false,
        ball,
        edge: false,
        bend,
    })
}

/// Two round faces about one axis: a point of the axis and the two
/// radii.
fn coaxial(
    world: &World,
    of: &[Reference<shapecraft_core::Anything>],
) -> Option<(Point3, Point3, f64, f64)> {
    let [a, b] = of else { return None };
    let ((p, axis, r), (q, other, s)) = (round(world, &a.source)?, round(world, &b.source)?);
    let along = axis.cross(other).length() < 1e-9;
    let on = (q - p).cross(axis).length() < 1e-9 * (1.0 + r + s);
    (along && on).then_some((p, axis, r, s))
}

/// From where to where a distance is measured that is to or between
/// round things not about one axis: two of them (their gap, their far
/// sides, or their middles), or one and a flat face or a straight edge
/// along it (from that to its near side, its far side or its middle).
/// Nothing if neither is round, or they do not lie so.
fn between_rounds(
    world: &World,
    of: &[Reference<shapecraft_core::Anything>],
    args: &DimensionArgs,
) -> Option<(Point3, Point3)> {
    let (far, centres, same_side) = (args.far, args.centres, args.same_side);
    let [a, b] = of else { return None };
    // (A flat face or a straight edge is that; a round face is round,
    // though it also names its axis.)
    let flat = |r: &Reference<shapecraft_core::Anything>| {
        DatumPlane::resolve(world, &r.source).is_ok()
            || (DatumAxis::resolve(world, &r.source).is_ok() && is_edge(world, &r.source))
            || matches!(r.source, Source::Datum(_) | Source::FrameAxis { .. })
    };
    let (ra, rb) = (
        (!flat(a)).then(|| rounded(world, &a.source)).flatten(),
        (!flat(b)).then(|| rounded(world, &b.source)).flatten(),
    );
    // How far past its middle a round thing reaches towards `way`.
    let reach = |r: &Rounded| {
        if centres {
            0.0
        } else if far {
            r.radius
        } else {
            -r.radius
        }
    };
    match (ra, rb) {
        (Some(r), Some(s)) => {
            // Between their middles: for balls their centres, else
            // their axes, which have to run alike.
            let ((rc, ra), (sc, sa)) = if centres {
                (middle_of(&r), middle_of(&s))
            } else {
                ((r.centre, r.axis), (s.centre, s.axis))
            };
            let (p, q) = if r.ball || s.ball {
                (rc, sc)
            } else if ra.cross(sa).length() > 1e-9 {
                // Axes that run different ways have no gap to measure:
                // from the one's centre to the other's.
                if !centres {
                    return None;
                }
                (rc, sc)
            } else {
                let across = (sc - rc) - ra * (sc - rc).dot(ra);
                (rc, rc + across)
            };
            let way = (q - p).try_normalize()?;
            // On the same side: where each reaches farthest the way
            // the second lies from the first.
            if same_side {
                return Some((p + way * r.radius, q + way * s.radius));
            }
            // (The gap between them: each one's near side.)
            Some((p - way * reach(&r), q + way * reach(&s)))
        }
        (Some(r), None) | (None, Some(r)) => {
            let other = if flat(a) { a } else { b };
            // (To its middle: a bend's centre, a leaning round's axis
            // where it is nearest its middle, whichever way it leans.)
            let centre = if centres { middle_of(&r).0 } else { r.centre };
            // (A round that leans over it has no side there: its middle.)
            let mut by_middle = centres;
            // From a flat face or a straight edge along it, square to
            // that.
            let foot = if let Ok(plane) = DatumPlane::resolve(world, &other.source) {
                // A circle that lies level with the face (a rim, the
                // end of a chamfer): how far its plane is off the
                // face's.
                if r.edge && r.axis.cross(plane.normal()).length() < 1e-9 {
                    let off = (centre - plane.origin()).dot(plane.normal());
                    return (off.abs() > 0.0).then(|| (centre - plane.normal() * off, centre));
                }
                if !r.ball && r.axis.dot(plane.normal()).abs() > 1e-9 {
                    by_middle = true;
                }
                let off = (centre - plane.origin()).dot(plane.normal());
                centre - plane.normal() * off
            } else {
                let line = DatumAxis::resolve(world, &other.source).ok()?;
                if !r.ball && r.axis.cross(line.direction()).length() > 1e-9 {
                    by_middle = true;
                }
                let along = (centre - line.origin()).dot(line.direction());
                line.origin() + line.direction() * along
            };
            let way = (centre - foot).try_normalize()?;
            Some((foot, centre + way * if by_middle { 0.0 } else { reach(&r) }))
        }
        (None, None) => None,
    }
}

/// How far what `source` names reaches along the direction `d`: a
/// point of it, and the least and the most of it that way — of a flat
/// face or a straight edge square to `d` its level, of something round
/// its two sides, of an edge its ends, of a point itself.
fn reach_along(
    world: &World,
    source: &Source,
    d: Point3,
    centres: bool,
) -> anyhow::Result<(Point3, f64, f64)> {
    let flat = DatumPlane::resolve(world, source).is_ok()
        || matches!(source, Source::Datum(_) | Source::FrameAxis { .. });
    if !flat && let Some(r) = rounded(world, source) {
        let lean = r.axis.dot(d);
        // Its sides along `d` are told only where its axis is square to
        // `d`; where it leans (or the middles are asked for), a bend is
        // its centre point and any other round the point of its axis
        // nearest its middle.
        if !(r.ball || r.edge) && (lean.abs() >= 1e-9 || (centres && r.bend.is_some())) {
            let (centre, _) = middle_of(&r);
            let level = centre.dot(d);
            return Ok((centre, level, level));
        }
        let half = if r.ball {
            r.radius
        } else {
            r.radius * (1.0 - lean * lean).max(0.0).sqrt()
        };
        let middle = r.centre.dot(d);
        return Ok((r.centre, middle - half, middle + half));
    }
    if let Some((from, to)) = ends(world, source) {
        let (a, b) = (from.dot(d), to.dot(d));
        return Ok(((from + to) / 2.0, a.min(b), a.max(b)));
    }
    match thing(world, source)? {
        Thing::Plane(p) => {
            ensure!(
                p.normal().cross(d).length() < 1e-9,
                "{} is not square to the direction it is measured along",
                source.described()
            );
            let level = p.origin().dot(d);
            // (About where the face is, if it is one.)
            Ok((lies(world, source, &p), level, level))
        }
        Thing::Line(l) => {
            ensure!(
                l.direction().dot(d).abs() < 1e-9,
                "{} is not square to the direction it is measured along",
                source.described()
            );
            let level = l.origin().dot(d);
            Ok((l.origin(), level, level))
        }
        Thing::Point(x) => Ok((x, x.dot(d), x.dot(d))),
    }
}

/// Where a flat face is, to tell which way it lies from a line in its
/// plane: about its middle, if `source` is a face; else its plane's
/// origin.
fn lies(world: &World, source: &Source, plane: &DatumPlane) -> Point3 {
    within(world, source)
        .and_then(|(world, name, pose)| {
            let (solid, face) = shapecraft_shape::on::face(&world.topo, name).ok()?;
            Some(pose.transform_point(shapecraft_shape::on::middle(&solid, face).0))
        })
        .unwrap_or_else(|| plane.origin())
}

/// Of a face made of straight lines that is not flat — a cone, or a
/// cylinder — the straight line along it through its middle: what a
/// view shows of it in profile. Nothing for anything else.
fn ruling(world: &World, source: &Source) -> Option<DatumAxis> {
    let (inner, name, pose) = within(world, source)?;
    let (solid, face) = shapecraft_shape::on::face(&inner.topo, name).ok()?;
    let surface = &solid.faces[face].surface;
    if DatumPlane::resolve(world, source).is_ok() {
        return None;
    }
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    let middle = surface.point_at(um, vm);
    // (Whichever way across it is straight: its middle on the chord.)
    let straight = |a: Point3, m: Point3, b: Point3| {
        let chord = b - a;
        chord.length() > 1e-9 && (m - a).cross(chord).length() <= 1e-7 * chord.length_squared()
    };
    let (au, bu) = (surface.point_at(u0, vm), surface.point_at(u1, vm));
    let (av, bv) = (surface.point_at(um, v0), surface.point_at(um, v1));
    let along = if straight(au, middle, bu) {
        bu - au
    } else if straight(av, middle, bv) {
        bv - av
    } else {
        return None;
    };
    DatumAxis::new(
        pose.transform_point(middle),
        pose.transform_vector(along.normalize()),
    )
    .ok()
}

/// Where what `source` names lies: about its middle, for a face; else
/// `fallback`.
fn lies_any(world: &World, source: &Source, fallback: Point3) -> Point3 {
    within(world, source)
        .and_then(|(world, name, pose)| {
            let (solid, face) = shapecraft_shape::on::face(&world.topo, name).ok()?;
            Some(pose.transform_point(shapecraft_shape::on::middle(&solid, face).0))
        })
        .unwrap_or(fallback)
}

/// What the dimension measures in `world`, and its value.
fn measured(world: &World, args: &DimensionArgs) -> anyhow::Result<(Measured, f64)> {
    let of = &args.of;
    let kind = match args.kind {
        DimensionKind::Auto => match &of[..] {
            [one] => {
                // Something round all the way around is said by its
                // diameter; a part of a round (a fillet, a slot's end)
                // by its radius.
                let round = rounded(world, &one.source).map(|r| r.whole);
                match round {
                    Some(true) => DimensionKind::Diameter,
                    Some(false) => DimensionKind::Radius,
                    None => DimensionKind::Distance,
                }
            }
            // Along a direction: always a distance.
            [_, _] if args.along.is_some() => DimensionKind::Distance,
            // Two round faces about one axis: how far the one is off
            // the other.
            [_, _] if coaxial(world, of).is_some() => DimensionKind::Distance,
            [_, _] if between_rounds(world, of, args).is_some() => DimensionKind::Distance,
            [a, b] => match (thing(world, a)?, thing(world, b)?) {
                (Thing::Plane(p), Thing::Plane(q))
                    if p.normal().cross(q.normal()).length() >= 1e-9 =>
                {
                    DimensionKind::Angle
                }
                (Thing::Line(l), Thing::Line(m))
                    if l.direction().cross(m.direction()).length() >= 1e-9 =>
                {
                    DimensionKind::Angle
                }
                _ => DimensionKind::Distance,
            },
            _ => anyhow::bail!(
                "pick what it measures: a round face, an edge, or two things; then click beside the model where it stands"
            ),
        },
        kind => kind,
    };
    match kind {
        DimensionKind::Auto => unreachable!("said above"),
        DimensionKind::Distance => {
            // One edge: its length from end to end.
            if let [one] = &of[..] {
                let (from, to) = ends(world, &one.source)
                    .context("pick two things, or one edge for its length")?;
                let measured = Measured::Length {
                    from,
                    to,
                    along: None,
                };
                return Ok((measured, from.distance(to)));
            }
            ensure!(of.len() == 2, "pick the two things it is between");
            // Along a direction: from how far the one reaches that way
            // to how far the other does.
            if let Some(along) = &args.along {
                let d = world.resolve(along)?.direction();
                let (p, plo, phi) = reach_along(world, &of[0].source, d, args.centres)?;
                let (q, qlo, qhi) = reach_along(world, &of[1].source, d, args.centres)?;
                // (From the one that lies lower that way to the other.)
                let ((p, plo, phi), (q, qlo, qhi)) = if plo + phi <= qlo + qhi {
                    ((p, plo, phi), (q, qlo, qhi))
                } else {
                    ((q, qlo, qhi), (p, plo, phi))
                };
                let (a, b) = if args.centres {
                    ((plo + phi) / 2.0, (qlo + qhi) / 2.0)
                } else if args.same_side {
                    (phi, qhi)
                } else if args.far {
                    (plo, qhi)
                } else {
                    (phi, qlo)
                };
                let at = |p: Point3, level: f64| p + d * (level - p.dot(d));
                let measured = Measured::Length {
                    from: at(p, a),
                    to: at(q, b),
                    along: Some(d),
                };
                return Ok((measured, (b - a).abs()));
            }
            if let Some((centre, axis, r, s)) = coaxial(world, of) {
                // (Whichever way across the axis a view shows it.)
                let measured = Measured::Radial {
                    centre,
                    axis,
                    inner: r.min(s),
                    outer: r.max(s),
                };
                return Ok((measured, (r - s).abs()));
            }
            // To or between round things that are not about one axis:
            // to their near sides, their far ones, or their middles.
            if let Some((from, to)) = between_rounds(world, of, args) {
                let measured = Measured::Length {
                    from,
                    to,
                    along: None,
                };
                return Ok((measured, from.distance(to)));
            }
            // (A flat face taken about where it is, not wherever its
            // plane's origin is: a face moved far off it has its
            // dimension by it.)
            let at_face = |source: &Source| -> anyhow::Result<Thing> {
                Ok(match thing(world, source)? {
                    Thing::Plane(p) => {
                        let at = lies(world, source, &p);
                        let at = at - p.normal() * (at - p.origin()).dot(p.normal());
                        Thing::Plane(DatumPlane::new(at, p.normal(), p.x_dir())?)
                    }
                    other => other,
                })
            };
            // (An axis of a frame is a line, or the plane square to it:
            // whichever goes with the other — a face along the Z axis
            // is measured to the axis, a face square to it to the
            // plane.)
            let as_line = |source: &Source| -> Option<Thing> {
                matches!(source, Source::FrameAxis { .. })
                    .then(|| DatumAxis::resolve(world, source).ok().map(Thing::Line))
                    .flatten()
            };
            let fits = |a: &Thing, b: &Thing| match (a, b) {
                (Thing::Plane(p), Thing::Plane(q)) => p.normal().cross(q.normal()).length() < 1e-9,
                (Thing::Plane(p), Thing::Line(l)) | (Thing::Line(l), Thing::Plane(p)) => {
                    l.direction().dot(p.normal()).abs() < 1e-9
                }
                (Thing::Line(l), Thing::Line(m)) => {
                    l.direction().cross(m.direction()).length() < 1e-9
                }
                _ => true,
            };
            let (mut a, mut b) = (at_face(&of[0].source)?, at_face(&of[1].source)?);
            if !fits(&a, &b) {
                if let Some(line) = as_line(&of[0].source).filter(|l| fits(l, &b)) {
                    a = line;
                } else if let Some(line) = as_line(&of[1].source).filter(|l| fits(&a, l)) {
                    b = line;
                }
            }
            // From the first to the second, square to what is flat or
            // straight of them.
            // (Two flat things — a face and a datum plane: by the face,
            // where it is, not at the plane's origin, far off.)
            let face = |source: &Source| {
                within(world, source).is_some_and(|(world, name, _)| {
                    shapecraft_shape::on::face(&world.topo, name).is_ok()
                })
            };
            let (from, to) = match (&a, &b) {
                (Thing::Plane(p), Thing::Plane(q))
                    if p.normal().cross(q.normal()).length() < 1e-9
                        && (face(&of[0].source) || face(&of[1].source)) =>
                {
                    let at = if face(&of[0].source) {
                        p.origin()
                    } else {
                        q.origin()
                    };
                    let onto = |plane: &DatumPlane| {
                        at - plane.normal() * (at - plane.origin()).dot(plane.normal())
                    };
                    (onto(p), onto(q))
                }
                (Thing::Plane(p), other) | (other, Thing::Plane(p)) => {
                    let to = match other {
                        Thing::Plane(q) => {
                            ensure!(
                                p.normal().cross(q.normal()).length() < 1e-9,
                                "the two faces are not parallel: measure their angle"
                            );
                            q.origin()
                        }
                        Thing::Line(l) => {
                            ensure!(
                                l.direction().dot(p.normal()).abs() < 1e-9,
                                "the edge is not along the face"
                            );
                            l.origin()
                        }
                        Thing::Point(x) => *x,
                    };
                    let off = (to - p.origin()).dot(p.normal());
                    (to - p.normal() * off, to)
                }
                (Thing::Line(l), other) | (other, Thing::Line(l)) => {
                    let to = match other {
                        Thing::Line(m) => {
                            ensure!(
                                l.direction().cross(m.direction()).length() < 1e-9,
                                "the two edges are not parallel"
                            );
                            m.origin()
                        }
                        Thing::Point(x) => *x,
                        Thing::Plane(_) => unreachable!("a plane is taken first"),
                    };
                    let along = (to - l.origin()).dot(l.direction());
                    (l.origin() + l.direction() * along, to)
                }
                (Thing::Point(x), Thing::Point(y)) => (*x, *y),
            };
            let measured = Measured::Length {
                from,
                to,
                along: None,
            };
            Ok((measured, from.distance(to)))
        }
        DimensionKind::Diameter | DimensionKind::Radius => {
            let [one] = &of[..] else {
                anyhow::bail!("pick the one round face");
            };
            let round = rounded(world, &one.source).with_context(|| {
                format!(
                    "{} is not round: neither a face that bends as a circle nor an arc of an edge",
                    one.source.described()
                )
            })?;
            // Of what bends two ways, the round that is asked for.
            let (centre, axis, radius) = match (args.bend, round.bend) {
                (Bend::Own, _) => (round.centre, round.axis, round.radius),
                (which, Some((centre, axis, middle))) => {
                    let radius = match which {
                        Bend::Inside => middle - round.radius,
                        Bend::Outside => middle + round.radius,
                        _ => middle,
                    };
                    (centre, axis, radius)
                }
                (_, None) => anyhow::bail!(
                    "{} does not bend around anything: it has the one radius",
                    one.source.described()
                ),
            };
            // (A ball is round every way: it has no axis.)
            let axis = if round.ball { Point3::ZERO } else { axis };
            // (As the bare number: from its middle to its rim, or
            // across it — whichever way across a view shows it.)
            if args.plain {
                let (inner, outer, value) = if kind == DimensionKind::Radius {
                    (0.0, radius, radius)
                } else {
                    (-radius, radius, radius * 2.0)
                };
                let measured = Measured::Radial {
                    centre,
                    axis,
                    inner,
                    outer,
                };
                return Ok((measured, value));
            }
            Ok(if kind == DimensionKind::Diameter {
                let measured = Measured::Diameter {
                    centre,
                    axis,
                    radius,
                };
                (measured, radius * 2.0)
            } else {
                let measured = Measured::Radius {
                    centre,
                    axis,
                    radius,
                };
                (measured, radius)
            })
        }
        DimensionKind::Angle => {
            ensure!(of.len() == 2, "pick the two faces or edges it is between");
            // (A cone, or any face made of straight lines — seen in
            // profile, a straight outline: its line through its middle.)
            let either = |r: &Reference<_>| -> anyhow::Result<Thing> {
                match ruling(world, &r.source) {
                    Some(line) => Ok(Thing::Line(line)),
                    None => thing(world, r),
                }
            };
            let (a, b) = (either(&of[0])?, either(&of[1])?);
            let (at, first, second) = match (&a, &b) {
                (Thing::Plane(p), Thing::Line(l)) | (Thing::Line(l), Thing::Plane(p)) => {
                    // A line against a face: where it meets the face's
                    // plane, along it, and along the face square under it.
                    let n = p.normal();
                    let d = l.direction();
                    let across = d - n * d.dot(n);
                    ensure!(
                        across.length() > 1e-9,
                        "the line stands square on the face: its angle is 90°"
                    );
                    ensure!(d.dot(n).abs() > 1e-12, "the line is along the face");
                    let t = (p.origin() - l.origin()).dot(n) / d.dot(n);
                    let at = l.origin() + d * t;
                    let mut line = d;
                    let mut flat = across.normalize();
                    // (Each towards where its face is.)
                    let (line_src, plane_src) = match (&a, &b) {
                        (Thing::Plane(_), _) => (&of[1].source, &of[0].source),
                        _ => (&of[0].source, &of[1].source),
                    };
                    if line.dot(lies_any(world, line_src, l.origin()) - at) < 0.0 {
                        line = -line;
                    }
                    if flat.dot(lies(world, plane_src, p) - at) < 0.0 {
                        flat = -flat;
                    }
                    if matches!(a, Thing::Plane(_)) {
                        (at, flat, line)
                    } else {
                        (at, line, flat)
                    }
                }
                (Thing::Plane(p), Thing::Plane(q)) => {
                    // Where the two planes cross; each one's direction
                    // away from that line, in itself.
                    let line = p.normal().cross(q.normal());
                    ensure!(line.length() > 1e-9, "the two faces are parallel");
                    let line = line.normalize();
                    let (n, m) = (p.normal(), q.normal());
                    let (d, e) = (n.dot(p.origin()), m.dot(q.origin()));
                    let at = (m.cross(line) * d + line.cross(n) * e) / n.dot(m.cross(line));
                    let mut first = line.cross(n);
                    let mut second = m.cross(line);
                    // (Each towards where its face is.)
                    if first.dot(lies(world, &of[0].source, p) - at) < 0.0 {
                        first = -first;
                    }
                    if second.dot(lies(world, &of[1].source, q) - at) < 0.0 {
                        second = -second;
                    }
                    (at, first, second)
                }
                (Thing::Line(l), Thing::Line(m)) => {
                    // Where the two lines cross (or come nearest, a hair
                    // apart): the apex; each one's direction from there
                    // towards where its edge is. Parallel: at the first.
                    let (d, e) = (l.direction(), m.direction());
                    let n = d.cross(e);
                    if n.length_squared() < 1e-18 {
                        (l.origin(), d, e)
                    } else {
                        let t = (m.origin() - l.origin()).cross(e).dot(n) / n.length_squared();
                        let at = l.origin() + d * t;
                        let towards = |line: &DatumAxis, source: &Source, way: Point3| {
                            let there =
                                ends(world, source).map_or(line.origin(), |(p, q)| (p + q) / 2.0);
                            if way.dot(there - at) < 0.0 { -way } else { way }
                        };
                        (
                            at,
                            towards(l, &of[0].source, d),
                            towards(m, &of[1].source, e),
                        )
                    }
                }
                _ => anyhow::bail!("an angle is between two flat faces or two straight edges"),
            };
            // (The other angle: from the first on past where they
            // meet, to the second.)
            let first = if args.supplement { -first } else { first };
            let angle = first.angle_between(second).to_degrees();
            let measured = Measured::Angle { at, first, second };
            Ok((measured, angle))
        }
    }
}

impl Operation for Dimension {
    type Args = DimensionArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &DimensionArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut DimensionArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let world = builder.tracked();
        let (measured, value) = measured(world, &self.args)?;
        // Its text stands off what it measures, by a share of how large
        // that is.
        let reach = match &measured {
            Measured::Angle { at, .. } => world
                .topo
                .vertices()
                .iter()
                .fold(1e-9_f64, |m, (_, v)| m.max(v.point.distance(*at))),
            _ => 0.0,
        };
        let at = match self.args.at {
            Some(at) => Point3::from_array(at),
            None => measured.beside(reach * 0.4),
        };
        let note = Annotation {
            sketch: None,
            note: Note::Dimension {
                measured,
                value,
                tolerance: self.args.tolerance.clone(),
                elsewhere: Vec::new(),
            },
            at,
        };
        Ok(shapecraft_core::Made {
            annotations: vec![(self.id.clone(), note)],
            ..Default::default()
        })
    }

    /// With what it measures picked, a click beside the model puts
    /// its text there, and it is done: as a sketch's dimensions are
    /// made.
    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        (): &(),
    ) -> shapecraft_core::EditVisualization<shapecraft_core::Edit<DimensionArgs, ()>> {
        let about = measured(context.world, &self.args)
            .ok()
            .map(|(measured, _)| {
                let anchors = Annotation {
                    sketch: None,
                    note: Note::Dimension {
                        measured,
                        value: 0.0,
                        tolerance: Tolerance::General,
                        elsewhere: Vec::new(),
                    },
                    at: Point3::ZERO,
                }
                .anchors();
                anchors.iter().sum::<Point3>() / anchors.len().max(1) as f64
            });
        crate::placed(&self.args, context, about, |args, at| DimensionArgs {
            at: Some(at),
            ..args.clone()
        })
    }

    fn summary(&self) -> String {
        let a = &self.args;
        let of: Vec<String> = a.of.iter().map(|s| s.source.described()).collect();
        let what = match a.kind {
            DimensionKind::Auto => "dimension",
            DimensionKind::Distance if of.len() == 1 => "length",
            DimensionKind::Distance => "distance",
            DimensionKind::Diameter => "diameter",
            DimensionKind::Radius => "radius",
            DimensionKind::Angle => "angle",
        };
        if of.is_empty() {
            format!("{what} of (nothing picked)")
        } else {
            format!("{what} of {}{}", of.join(", "), a.tolerance.suffix())
        }
    }
}

/// The box that takes what is measured, of the kinds its measure takes.
fn of_field(args: &DimensionArgs, _: &EditContext<'_>) -> Vec<Widget<DimensionArgs>> {
    let (label, kinds) = match args.kind {
        DimensionKind::Auto => (
            "Of",
            vec![
                ReferenceKind::Face,
                ReferenceKind::Plane,
                ReferenceKind::Axis,
                ReferenceKind::Edge,
                ReferenceKind::Vertex,
                ReferenceKind::Point,
            ],
        ),
        DimensionKind::Distance => (
            "Between",
            vec![
                ReferenceKind::Plane,
                ReferenceKind::Axis,
                ReferenceKind::Edge,
                ReferenceKind::Vertex,
                ReferenceKind::Point,
            ],
        ),
        DimensionKind::Diameter | DimensionKind::Radius => ("Of", vec![ReferenceKind::Face]),
        DimensionKind::Angle => ("Between", vec![ReferenceKind::Plane, ReferenceKind::Axis]),
    };
    let picked = Reference::sources(&args.of);
    let args = args.clone();
    let set = move |picked: Vec<Source>| DimensionArgs {
        of: Reference::each(picked),
        ..args.clone()
    };
    vec![
        shapecraft_core::widgets::ReferenceList::new(label, kinds, picked, set)
            .required()
            .into(),
    ]
}

/// The fields of a tolerance: which kind, and its numbers.
pub(crate) fn tolerance_fields(
    args: &DimensionArgs,
    _: &EditContext<'_>,
) -> Vec<Widget<DimensionArgs>> {
    let with = {
        let args = args.clone();
        move |tolerance: Tolerance| DimensionArgs {
            tolerance,
            ..args.clone()
        }
    };
    let selected = match args.tolerance {
        Tolerance::General => 0,
        Tolerance::Symmetric(_) => 1,
        Tolerance::Deviations { .. } => 2,
        Tolerance::Limits { .. } => 3,
        Tolerance::Fit(_) => 4,
        Tolerance::Reference => 5,
    };
    let options = [
        "General",
        "± Symmetric",
        "Deviations",
        "Limits of size",
        "Fit (ISO 286)",
        "Reference (in brackets)",
    ]
    .map(str::to_owned)
    .to_vec();
    let kind = Choice::new("Tolerance", options, selected, {
        let (with, was) = (with.clone(), args.tolerance.clone());
        move |i| {
            with(was.as_kind(&match i {
                1 => Tolerance::Symmetric(0.1),
                2 => Tolerance::Deviations {
                    upper: 0.1,
                    lower: -0.1,
                },
                3 => Tolerance::Limits {
                    upper: 0.1,
                    lower: -0.1,
                },
                4 => Tolerance::Fit("H7".to_owned()),
                5 => Tolerance::Reference,
                _ => Tolerance::General,
            }))
        }
    });
    let mut fields: Vec<Widget<DimensionArgs>> = vec![kind.into()];
    match args.tolerance.clone() {
        Tolerance::General | Tolerance::Reference => {}
        Tolerance::Symmetric(t) => {
            let with = with.clone();
            fields.push(
                NumericInput::new("±", t, move |x: f64| with(Tolerance::Symmetric(x.abs()))).into(),
            );
        }
        Tolerance::Deviations { upper, lower } => {
            let (up, low) = (with.clone(), with.clone());
            fields.push(
                NumericInput::new("Upper", upper, move |x| {
                    up(Tolerance::Deviations { upper: x, lower })
                })
                .into(),
            );
            fields.push(
                NumericInput::new("Lower", lower, move |x| {
                    low(Tolerance::Deviations { upper, lower: x })
                })
                .into(),
            );
        }
        Tolerance::Limits { upper, lower } => {
            let (up, low) = (with.clone(), with.clone());
            fields.push(
                NumericInput::new("Upper", upper, move |x| {
                    up(Tolerance::Limits { upper: x, lower })
                })
                .into(),
            );
            fields.push(
                NumericInput::new("Lower", lower, move |x| {
                    low(Tolerance::Limits { upper, lower: x })
                })
                .into(),
            );
        }
        Tolerance::Fit(class) => {
            let with = with.clone();
            fields.push(
                TextInput::new("Class", class, move |class: String| {
                    with(Tolerance::Fit(class.trim().to_owned()))
                })
                .into(),
            );
        }
    }
    fields
}

#[cfg(test)]
mod round_tests {
    use super::*;

    #[test]
    fn points_along_an_arc_tell_its_circle_and_points_off_one_do_not() {
        // A quarter of the circle of radius 3 about (1, 2, 5), in the
        // plane z = 5.
        let centre = Point3::new(1.0, 2.0, 5.0);
        let at = |s: f64| {
            let t = s * std::f64::consts::FRAC_PI_2;
            centre + Point3::new(t.cos(), t.sin(), 0.0) * 3.0
        };
        let (c, axis, r) = arc_through(&at).expect("an arc");
        assert!(c.distance(centre) < 1e-12 && (r - 3.0).abs() < 1e-12);
        assert!(axis.cross(Point3::Z).length() < 1e-12);
        // All the way around: told by thirds of it.
        let around = |s: f64| {
            let t = s * std::f64::consts::TAU;
            centre + Point3::new(t.cos(), t.sin(), 0.0) * 3.0
        };
        let (c, _, r) = arc_through(&around).expect("a circle");
        assert!(c.distance(centre) < 1e-12 && (r - 3.0).abs() < 1e-12);
        // A parabola is no arc; a line has no circle.
        assert!(arc_through(&|s| Point3::new(s, s * s, 0.0)).is_none());
        assert!(arc_through(&|s| Point3::new(s, 2.0 * s, 0.0)).is_none());
    }
}
