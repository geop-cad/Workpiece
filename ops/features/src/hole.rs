//! Hole: drilled holes at the points of a sketch — plain, counterbored,
//! countersunk or for a tap; to a depth or through everything.

use anyhow::{Context, ensure};
use shapecraft_boolean::{Solid, read};
use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::topo::{Curve2, Topo};
use shapecraft_core::world::{DatumPlane, Profile, ProfileEdge, ProfileRegion, ProfileVertex};
use shapecraft_core::{Builder, Measure, Operation};
use shapecraft_ops_sweep::Extent;

use shapecraft_shape::bodies::{self};
use shapecraft_shape::sweep::{self, Frame, Make, Turn};

use crate::norm;

/// What a hole's mouth is like.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum HoleKind {
    /// A plain bore.
    #[default]
    Simple,
    /// A wider, flat-bottomed bore at the mouth, for a screw's head.
    Counterbore,
    /// A cone at the mouth, for a countersunk head.
    Countersink,
    /// A bore of the size a tap cuts its thread into: the thread's
    /// diameter less its pitch.
    Tapped,
}

impl HoleKind {
    const ALL: [HoleKind; 4] = [
        HoleKind::Simple,
        HoleKind::Counterbore,
        HoleKind::Countersink,
        HoleKind::Tapped,
    ];

    fn label(self) -> &'static str {
        match self {
            HoleKind::Simple => "Simple",
            HoleKind::Counterbore => "Counterbore",
            HoleKind::Countersink => "Countersink",
            HoleKind::Tapped => "Tapped",
        }
    }
}

/// How a hole is placed.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum HolePlace {
    /// At every point of a sketch.
    #[default]
    Sketch,
    /// On a face, a distance from each of two planes.
    Offsets,
    /// On a face, a distance from an axis, at an angle around it.
    Polar,
}

impl shapecraft_core::Choices for HolePlace {
    const ALL: &'static [Self] = &[HolePlace::Sketch, HolePlace::Offsets, HolePlace::Polar];

    fn label(self) -> &'static str {
        match self {
            HolePlace::Sketch => "At a sketch's points",
            HolePlace::Offsets => "Off two planes",
            HolePlace::Polar => "Around an axis",
        }
    }
}

/// How easily a screw goes through its hole (ISO 273).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Clearance {
    /// A close hole, to H12.
    Fine,
    /// The usual hole, to H13.
    #[default]
    Medium,
    /// A wide hole, to H14.
    Coarse,
}

impl Clearance {
    /// The tolerance class its holes are made to.
    pub fn class(self) -> &'static str {
        match self {
            Clearance::Fine => "H12",
            Clearance::Medium => "H13",
            Clearance::Coarse => "H14",
        }
    }
}

impl shapecraft_core::Choices for Clearance {
    const ALL: &'static [Self] = &[Clearance::Fine, Clearance::Medium, Clearance::Coarse];

    fn label(self) -> &'static str {
        match self {
            Clearance::Fine => "Fine",
            Clearance::Medium => "Medium",
            Clearance::Coarse => "Coarse",
        }
    }
}

/// Which ends of a hole are chamfered, to take the burr off.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Deburr {
    /// Neither.
    #[default]
    None,
    /// Where the drill goes in.
    Entry,
    /// Where it comes out, of a hole that goes through.
    Exit,
    /// Both.
    Both,
}

impl shapecraft_core::Choices for Deburr {
    const ALL: &'static [Self] = &[Deburr::None, Deburr::Entry, Deburr::Exit, Deburr::Both];

    fn label(self) -> &'static str {
        match self {
            Deburr::None => "None",
            Deburr::Entry => "Entry",
            Deburr::Exit => "Exit",
            Deburr::Both => "Both",
        }
    }
}

/// Where the holes are, and what they are like.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
#[serde(default)]
pub struct HoleArgs {
    /// How it is placed.
    #[arg(choice, label = "Placement")]
    pub place: HolePlace,
    /// The sketch: a hole goes at each of its points, into what is
    /// behind its plane.
    #[arg(when = "self.place == HolePlace::Sketch")]
    pub sketch: Option<shapecraft_core::Reference<shapecraft_core::Sketched>>,
    /// The flat face (or plane) the hole is drilled into, where it is
    /// not placed by a sketch.
    #[arg(label = "On", when = "self.place != HolePlace::Sketch")]
    pub face: Option<shapecraft_core::Reference<DatumPlane>>,
    /// The first plane (or flat face) it is a distance from.
    #[arg(label = "From", when = "self.place == HolePlace::Offsets")]
    pub first: Option<shapecraft_core::Reference<DatumPlane>>,
    /// How far from the first: into the body from a face of it, to the
    /// front of a plane.
    #[arg(label = "Distance", when = "self.place == HolePlace::Offsets")]
    pub first_offset: Measure,
    /// The second plane (or flat face) it is a distance from.
    #[arg(label = "And from", when = "self.place == HolePlace::Offsets")]
    pub second: Option<shapecraft_core::Reference<DatumPlane>>,
    /// How far from the second.
    #[arg(label = "Distance", when = "self.place == HolePlace::Offsets")]
    pub second_offset: Measure,
    /// The axis it is a distance from.
    #[arg(label = "Around", when = "self.place == HolePlace::Polar")]
    pub axis: Option<shapecraft_core::Reference<shapecraft_core::world::DatumAxis>>,
    /// How far from the axis.
    #[arg(label = "Distance", when = "self.place == HolePlace::Polar")]
    pub radius: Measure,
    /// The angle around the axis, in degrees, from the face's u
    /// direction (a side face's u runs to the right seen from outside).
    #[arg(label = "Angle", row, when = "self.place == HolePlace::Polar")]
    pub angle: Measure,
    /// What the mouth is like.
    #[arg(choice)]
    pub kind: HoleKind,
    /// The screw it is for, by its thread's size: its sizes are what
    /// the standards say. None for sizes of its own.
    #[arg(label = "Size", one_of = "crate::norm::size_names()", empty = "Custom")]
    pub size: Option<String>,
    /// How easily the screw goes through (ISO 273).
    #[arg(
        choice,
        label = "Clearance",
        when = "self.size.is_some() && self.kind != HoleKind::Tapped"
    )]
    pub clearance: Clearance,
    /// Whether a counterbore is for a hexagon head, not a socket head
    /// cap screw.
    #[arg(
        label = "For",
        options = "Socket head (ISO 4762)|Hexagon head (ISO 4017)",
        when = "self.size.is_some() && self.kind == HoleKind::Counterbore"
    )]
    pub hexagon: bool,
    /// Whether a countersink is for a slotted or cross recessed screw,
    /// not a hexagon socket one.
    #[arg(
        label = "For",
        options = "Socket (ISO 10642)|Slotted, cross (ISO 2009)",
        when = "self.size.is_some() && self.kind == HoleKind::Countersink"
    )]
    pub slotted: bool,
    /// The fine pitch of a standard tapped hole's thread; none for the
    /// coarse one.
    #[arg(
        label = "Pitch",
        one_of = "crate::norm::fine_names(self.size.as_deref())",
        empty = "Coarse",
        when = "self.size.is_some() && self.kind == HoleKind::Tapped"
    )]
    pub fine: Option<String>,
    /// The bore's diameter (for a tapped hole, the thread's).
    #[arg(when = "self.size.is_none()")]
    pub diameter: Measure,
    /// The tolerance class a plain bore is made to (ISO 286).
    #[arg(
        label = "Fit",
        one_of = "crate::norm::class_names()",
        empty = "None",
        when = "self.size.is_none() && self.kind == HoleKind::Simple"
    )]
    pub class: Option<String>,
    /// Where the counterbore's floor is: a depth from the mouth, or
    /// at a face or datum plane.
    #[serde(default)]
    #[arg(
        label = "Head ends",
        among = "[Extent::Distance, Extent::ToFace]",
        when = "self.kind == HoleKind::Counterbore"
    )]
    pub head_extent: Extent,
    /// The diameter of the counterbore, or of the countersink at the
    /// mouth.
    #[arg(
        label = "Head diameter",
        when = "self.size.is_none() && matches!(self.kind, HoleKind::Counterbore | HoleKind::Countersink)"
    )]
    pub head: Measure,
    /// How deep the counterbore is.
    #[arg(
        row,
        when = "self.size.is_none() && self.kind == HoleKind::Counterbore && self.head_extent == Extent::Distance"
    )]
    pub head_depth: Measure,
    /// The face or plane the counterbore's floor is at.
    #[serde(default)]
    #[arg(
        label = "Head up to",
        when = "self.kind == HoleKind::Counterbore && self.head_extent == Extent::ToFace"
    )]
    pub head_up_to: Option<shapecraft_core::Reference<DatumPlane>>,
    /// How much deeper than that face or plane the floor is (less
    /// than nothing: short of it).
    #[arg(
        label = "Head offset",
        when = "self.kind == HoleKind::Counterbore && self.head_extent == Extent::ToFace"
    )]
    pub head_offset: Measure,
    /// The angle of the countersink's cone, in degrees.
    #[arg(
        row,
        when = "self.size.is_none() && self.kind == HoleKind::Countersink"
    )]
    pub head_angle: Measure,
    /// The pitch of a tapped hole's thread.
    #[arg(when = "self.size.is_none() && self.kind == HoleKind::Tapped")]
    pub pitch: Measure,
    /// Whether a tapped hole's thread is cut into its wall, not only
    /// said of it.
    #[arg(
        label = "Thread",
        options = "Cosmetic|Modelled",
        when = "self.kind == HoleKind::Tapped"
    )]
    pub modelled: bool,
    /// Where the bore ends: at a depth, through everything, through
    /// the wall it starts in (where it first comes out), or at a face
    /// or datum plane.
    #[serde(default)]
    #[arg(label = "Extent", among = "Extent::ONE_WAY")]
    pub extent: Extent,
    /// How deep the bore is, to where its full width ends.
    #[arg(when = "self.extent == Extent::Distance")]
    pub depth: Measure,
    /// The face or plane the bore's full width ends at.
    #[serde(default)]
    #[arg(label = "Up to", when = "self.extent == Extent::ToFace")]
    pub up_to: Option<shapecraft_core::Reference<DatumPlane>>,
    /// How much deeper than that face or plane it goes (less than
    /// nothing: short of it).
    #[arg(label = "Offset", when = "self.extent == Extent::ToFace")]
    pub offset: Measure,
    /// Whether it is drilled the other way: out of the sketch plane's
    /// front.
    #[arg(flip, row)]
    pub flip: bool,
    /// The angle of the drill's point at the bottom of a hole with a
    /// depth, in degrees; 180 for a flat bottom.
    #[arg(
        label = "Point angle",
        when = "matches!(self.extent, Extent::Distance | Extent::ToFace)"
    )]
    pub point: Measure,
    /// Which of its ends are chamfered, to take the burr off.
    #[arg(choice, label = "Deburr")]
    pub deburr: Deburr,
    /// How wide the chamfer is: how much wider than the hole its
    /// mouth is, on each side.
    #[arg(label = "Chamfer", when = "self.deburr != Deburr::None")]
    pub chamfer: Measure,
    /// The angle of the chamfer's cone, in degrees.
    #[arg(label = "Chamfer angle", row, when = "self.deburr != Deburr::None")]
    pub chamfer_angle: Measure,
    /// The body drilled; `None` for every solid body.
    #[arg(label = "In", empty = "All bodies")]
    pub target: Option<shapecraft_core::Reference<shapecraft_core::world::topo::BodyId>>,
}

impl Default for HoleArgs {
    fn default() -> Self {
        Self {
            place: HolePlace::Sketch,
            sketch: None,
            face: None,
            first: None,
            first_offset: Measure::new(10.0),
            second: None,
            second_offset: Measure::new(10.0),
            axis: None,
            radius: Measure::new(10.0),
            angle: Measure::new(0.0),
            kind: HoleKind::Simple,
            size: None,
            clearance: Clearance::Medium,
            hexagon: false,
            slotted: false,
            fine: None,
            diameter: Measure::new(6.0),
            class: None,
            extent: Extent::Distance,
            depth: Measure::new(10.0),
            up_to: None,
            offset: Measure::new(0.0),
            point: Measure::new(118.0),
            head: Measure::new(11.0),
            head_extent: Extent::Distance,
            head_depth: Measure::new(6.4),
            head_up_to: None,
            head_offset: Measure::new(0.0),
            head_angle: Measure::new(90.0),
            pitch: Measure::new(1.0),
            modelled: false,
            flip: false,
            deburr: Deburr::None,
            chamfer: Measure::new(0.5),
            chamfer_angle: Measure::new(90.0),
            target: None,
        }
    }
}

/// Drills holes at the points of a sketch.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hole {
    /// The operation's id.
    pub id: String,
    /// Where the holes are and what they are like.
    pub args: HoleArgs,
}

/// The solid that turning the outline `corners` — each how far from the
/// axis, and how far along it — about the axis from `origin` along
/// `direction` makes: what a drill of that shape takes away.
pub fn drill(
    id: &str,
    origin: Point3,
    direction: Point3,
    corners: &[(f64, f64)],
) -> anyhow::Result<Solid> {
    let along = direction
        .try_normalize()
        .context("a hole has a direction")?;
    let out = along.any_orthonormal_vector();
    let plane = DatumPlane::new(origin, out.cross(along), out)?;
    let mut profile = Profile::new(plane);
    let n = corners.len();
    let mut area = 0.0;
    for (k, &(r, s)) in corners.iter().enumerate() {
        let (next_r, next_s) = corners[(k + 1) % n];
        area += r * next_s - next_r * s;
        profile.vertices.push(ProfileVertex {
            name: format!("v{k}"),
            point: Point2::new(r, s),
            tolerance: 1e-9,
        });
        profile.edges.push(ProfileEdge {
            name: format!("e{k}"),
            entity: format!("e{k}"),
            span: (0.0, 1.0),
            curve: Curve2::line(Point2::new(r, s), Point2::new(next_r, next_s))?,
            t0: 0.0,
            t1: 1.0,
            start: k,
            end: (k + 1) % n,
            tolerance: 1e-9,
        });
    }
    let walk: Vec<(usize, bool)> = (0..n).map(|k| (k, false)).collect();
    profile.regions.push(ProfileRegion {
        outer: walk.clone(),
        holes: Vec::new(),
        area: area / 2.0,
    });
    let turn = Turn {
        origin,
        axis: along,
    };
    let mut scratch = Topo::new();
    let whole = (0.0, std::f64::consts::TAU);
    let body = sweep::revolve(
        &mut scratch,
        id,
        &profile,
        &[walk],
        true,
        turn,
        whole,
        Make::Solid,
    )?;
    Ok(read(&scratch, body)?)
}

/// The sizes of a hole, measured.
#[derive(Debug, Clone, Copy)]
pub struct HoleShape {
    /// What its mouth is like.
    pub kind: HoleKind,
    /// The bore's radius.
    pub radius: f64,
    /// How deep the bore's full width goes.
    pub depth: f64,
    /// The angle of the point at its bottom, in radians; none for a
    /// flat bottom.
    pub point: Option<f64>,
    /// The radius of the counterbore or countersink at the mouth.
    pub head: f64,
    /// How deep the counterbore is.
    pub head_depth: f64,
    /// The angle of the countersink's cone, in radians.
    pub head_angle: f64,
    /// The chamfer at its mouth: how wide, and its cone's angle, in
    /// radians.
    pub entry: Option<(f64, f64)>,
    /// The chamfer where it comes out, `depth` deep: how wide, and its
    /// cone's angle, in radians. The bore goes on at the chamfer's
    /// width, as far again.
    pub exit: Option<(f64, f64)>,
}

/// How deep a chamfer `wide` of a cone of `angle` is.
fn chamfer_depth(wide: f64, angle: f64) -> anyhow::Result<f64> {
    let half = angle / 2.0;
    ensure!(wide > 0.0 && wide.is_finite(), "a chamfer has some width");
    ensure!(
        half > 0.01 && half < 1.56,
        "a chamfer's cone has an angle under 180°"
    );
    Ok(wide / half.tan())
}

impl HoleShape {
    /// Its outline: from the axis at the mouth out, down and back to
    /// the axis, each corner how far from the axis and how deep.
    pub fn outline(&self) -> anyhow::Result<Vec<(f64, f64)>> {
        Ok(self.walled()?.0)
    }

    /// Its [outline](Self::outline), and which of its sides is the
    /// bore's wall: the one from that corner to the next.
    pub fn walled(&self) -> anyhow::Result<(Vec<(f64, f64)>, usize)> {
        let (r, depth) = (self.radius, self.depth);
        ensure!(r > 0.0 && r.is_finite(), "a hole has some width");
        ensure!(depth > 0.0 && depth.is_finite(), "a hole has some depth");
        let mut corners = vec![(0.0, 0.0)];
        // The mouth, chamfered: as wide as `wide`, the chamfer `deep`.
        let mouth = |corners: &mut Vec<(f64, f64)>, wide: f64| -> anyhow::Result<f64> {
            match self.entry {
                Some((by, angle)) => {
                    let deep = chamfer_depth(by, angle)?;
                    corners.extend([(wide + by, 0.0), (wide, deep)]);
                    Ok(deep)
                }
                None => {
                    corners.push((wide, 0.0));
                    Ok(0.0)
                }
            }
        };
        // How deep the bore's wall starts.
        let top = match self.kind {
            HoleKind::Simple | HoleKind::Tapped => mouth(&mut corners, r)?,
            HoleKind::Counterbore => {
                ensure!(self.head > r, "a counterbore is wider than its hole");
                ensure!(
                    self.head_depth > 0.0 && self.head_depth < depth,
                    "the counterbore's depth ({}) has to be less than the hole's ({depth})",
                    self.head_depth
                );
                let chamfer = mouth(&mut corners, self.head)?;
                ensure!(
                    chamfer < self.head_depth,
                    "the chamfer is deeper than the counterbore"
                );
                corners.extend([(self.head, self.head_depth), (r, self.head_depth)]);
                self.head_depth
            }
            HoleKind::Countersink => {
                ensure!(self.head > r, "a countersink is wider than its hole");
                let half = self.head_angle / 2.0;
                ensure!(
                    half > 0.01 && half < 1.56,
                    "a countersink's cone has an angle under 180°"
                );
                let deep = (self.head - r) / half.tan();
                // (A countersink is its mouth's chamfer.)
                corners.extend([(self.head, 0.0), (r, deep)]);
                deep
            }
        };
        let wall = corners.len() - 1;
        match self.exit {
            Some((by, angle)) => {
                let deep = chamfer_depth(by, angle)?;
                ensure!(
                    top + deep < depth,
                    "the chamfers are deeper than the hole is"
                );
                corners.extend([
                    (r, depth - deep),
                    (r + by, depth),
                    (r + by, depth * 2.0 + 1.0),
                    (0.0, depth * 2.0 + 1.0),
                ]);
            }
            None => {
                ensure!(top < depth, "the hole's mouth is deeper than the hole");
                corners.push((r, depth));
                match self.point {
                    Some(angle) if angle < 3.1 => {
                        ensure!(angle > 0.02, "a drill's point has some angle");
                        corners.push((0.0, depth + r / (angle / 2.0).tan()));
                    }
                    _ => corners.push((0.0, depth)),
                }
            }
        }
        Ok((corners, wall))
    }
}

/// The lines a bore of `radius` at `place` along `direction` is told
/// by: its axis, and eight around it near its wall — where its axis
/// runs through a gap (a slot under it), its wall still meets material.
fn lines(place: Point3, direction: Point3, radius: f64) -> Vec<Point3> {
    let (a, b) = direction.any_orthonormal_pair();
    let mut all = vec![place];
    for k in 0..8 {
        let turn = std::f64::consts::TAU * k as f64 / 8.0;
        all.push(place + (a * turn.cos() + b * turn.sin()) * (radius * 0.85));
    }
    all
}

/// Whether any of the lines from `starts` along `direction` is inside
/// one of `solids` a distance `t` on.
fn within(solids: &[&Solid], starts: &[Point3], direction: Point3, t: f64) -> bool {
    starts.iter().any(|start| {
        solids
            .iter()
            .any(|s| shapecraft_boolean::holds(s, *start + direction * t) == Some(true))
    })
}

/// How far along `direction` the lines from `starts` first come into
/// one of `solids`, looked for as far as `far`: nothing if they do not.
fn first_in(solids: &[&Solid], starts: &[Point3], direction: Point3, far: f64) -> Option<f64> {
    const STEPS: usize = 64;
    let step = far / STEPS as f64;
    (0..STEPS)
        .find(|k| within(solids, starts, direction, (*k as f64 + 0.5) * step))
        .map(|k| k as f64 * step)
}

/// How far along `direction` from `starts` the lines leave the last of
/// `solids`, looked for as far as `far`.
fn way_out(solids: &[&Solid], starts: &[Point3], direction: Point3, far: f64) -> Option<f64> {
    let inside = |t: f64| within(solids, starts, direction, t);
    const STEPS: usize = 48;
    let step = far / STEPS as f64;
    let last = (0..STEPS)
        .rev()
        .find(|k| inside((*k as f64 + 0.5) * step))?;
    let (mut within, mut beyond) = ((last as f64 + 0.5) * step, (last as f64 + 1.5) * step);
    for _ in 0..40 {
        let middle = (within + beyond) / 2.0;
        if inside(middle) {
            within = middle;
        } else {
            beyond = middle;
        }
    }
    Some((within + beyond) / 2.0)
}

/// How far along `direction` from `place` the line first leaves
/// `solids` after it has come into them, looked for as far as `far`;
/// and how far along it comes into them again (`far` and a little, if
/// it does not).
fn first_out(
    solids: &[&Solid],
    starts: &[Point3],
    direction: Point3,
    far: f64,
) -> Option<(f64, f64)> {
    let inside = |t: f64| within(solids, starts, direction, t);
    const STEPS: usize = 256;
    let step = far / STEPS as f64;
    let at = |k: usize| (k as f64 + 0.5) * step;
    let first = (0..STEPS).find(|k| inside(at(*k)))?;
    let last = (first..STEPS).find(|k| !inside(at(*k))).unwrap_or(STEPS);
    let (mut within, mut beyond) = (at(last - 1), at(last));
    for _ in 0..40 {
        let middle = (within + beyond) / 2.0;
        if inside(middle) {
            within = middle;
        } else {
            beyond = middle;
        }
    }
    let again = (last..STEPS)
        .find(|k| inside(at(*k)))
        .map_or(far * 1.1 + 1.0, at);
    Some(((within + beyond) / 2.0, again))
}

/// What a hole's sizes are: as its arguments have them, or as the
/// standards do for the screw it is for.
struct Sized {
    /// The diameter drilled.
    bore: f64,
    /// The diameter said of it: the bore's, or a thread's.
    nominal: f64,
    /// The tolerance the diameter is said with.
    tolerance: shapecraft_core::world::Tolerance,
    /// The head's diameter and depth, and its cone's angle in degrees.
    head: f64,
    head_depth: f64,
    head_angle: f64,
    /// A thread's pitch.
    pitch: f64,
}

impl HoleArgs {
    /// The standard size it is of, if it is of one.
    fn standard(&self) -> anyhow::Result<Option<&'static norm::Size>> {
        self.size
            .as_deref()
            .map(|name| {
                norm::sized(name).with_context(|| format!("there is no thread size {name:?}"))
            })
            .transpose()
    }

    /// The pitch of the thread of a standard tapped hole of `size`.
    fn standard_pitch(&self, size: &norm::Size) -> f64 {
        self.fine
            .as_deref()
            .and_then(|fine| size.fine.iter().find(|p| format!("{p}") == fine))
            .copied()
            .unwrap_or(size.coarse)
    }

    fn sized(&self, builder: &dyn Builder) -> anyhow::Result<Sized> {
        use shapecraft_core::world::Tolerance;
        let measure = |m: &Measure, what: &str| builder.measure(m, what);
        let Some(size) = self.standard()? else {
            let nominal = measure(&self.diameter, "the diameter")?;
            let pitch = measure(&self.pitch, "the pitch")?;
            let tapped = self.kind == HoleKind::Tapped;
            if tapped {
                ensure!(
                    pitch > 0.0 && pitch < nominal,
                    "a thread's pitch is less than its diameter"
                );
            }
            let class = self.class.clone().filter(|_| self.kind == HoleKind::Simple);
            return Ok(Sized {
                bore: if tapped { nominal - pitch } else { nominal },
                nominal,
                tolerance: match (&self.diameter.tolerance, class) {
                    (Tolerance::General, Some(class)) => Tolerance::Fit(class),
                    (said, _) => said.clone(),
                },
                head: measure(&self.head, "the head's diameter")?,
                head_depth: measure(&self.head_depth, "the counterbore's depth")?,
                head_angle: measure(&self.head_angle, "the countersink's angle")?,
                pitch,
            });
        };
        let through = size.clearance[self.clearance as usize];
        let fit = Tolerance::Fit(self.clearance.class().to_owned());
        let mut sized = Sized {
            bore: through,
            nominal: through,
            tolerance: fit,
            head: 0.0,
            head_depth: 0.0,
            head_angle: 90.0,
            pitch: size.coarse,
        };
        let name = size.name;
        match self.kind {
            HoleKind::Simple => {}
            HoleKind::Counterbore if self.hexagon => {
                let (bore, head) = size
                    .hexagon
                    .with_context(|| format!("DIN 974-2 has no counterbore for {name}"))?;
                sized.head = bore;
                sized.head_depth = head + norm::head_allowance(size.diameter);
            }
            HoleKind::Counterbore => {
                sized.head = size
                    .cap
                    .with_context(|| format!("DIN 974-1 has no counterbore for {name}"))?;
                // (A socket head is as high as its thread is wide.)
                sized.head_depth = size.diameter + norm::head_allowance(size.diameter);
            }
            HoleKind::Countersink => {
                let (head, norm) = if self.slotted {
                    (size.slotted_sink, "ISO 15065")
                } else {
                    (size.socket_sink, "DIN 74")
                };
                sized.head =
                    head.with_context(|| format!("{norm} has no countersink for {name}"))?;
            }
            HoleKind::Tapped => {
                sized.pitch = self.standard_pitch(size);
                sized.bore = norm::tap_drill(size.diameter, sized.pitch);
                sized.nominal = size.diameter;
                sized.tolerance = Tolerance::General;
            }
        }
        Ok(sized)
    }
}

impl Operation for Hole {
    type Args = HoleArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &HoleArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut HoleArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = shapecraft_shape::bodies::Out::default();
        let args = &self.args;
        let measure = |m: &Measure, what: &str| (*builder).measure(m, what);
        let sized = args.sized(builder)?;
        let (diameter, bore, pitch) = (sized.nominal, sized.bore, sized.pitch);
        let depth = measure(&args.depth, "the depth")?;
        let point = measure(&args.point, "the point's angle")?.to_radians();
        let chamfer = match args.deburr {
            Deburr::None => None,
            _ => Some((
                measure(&args.chamfer, "the chamfer")?,
                measure(&args.chamfer_angle, "the chamfer's angle")?.to_radians(),
            )),
        };
        let entry = chamfer.filter(|_| matches!(args.deburr, Deburr::Entry | Deburr::Both));
        let exit = chamfer.filter(|_| matches!(args.deburr, Deburr::Exit | Deburr::Both));
        let through = matches!(args.extent, Extent::ThroughAll | Extent::ToNext);
        ensure!(
            exit.is_none() || through,
            "only a hole that goes through has an end to chamfer where it comes out"
        );
        let tapped = args.kind == HoleKind::Tapped;
        let world = builder.tracked();
        // Where the holes go, and which way is out of what they are
        // drilled into.
        let (places, out_of) = match args.place {
            HolePlace::Sketch => {
                let sketch = args.sketch.as_ref().context("pick a sketch")?.name();
                let profile = world
                    .profiles
                    .get(sketch)
                    .with_context(|| format!("there is no sketch {sketch:?}"))?;
                // A hole at every point of the sketch that is no curve's
                // end.
                let mut used = vec![false; profile.vertices.len()];
                for edge in &profile.edges {
                    used[edge.start] = true;
                    used[edge.end] = true;
                }
                let frame = Frame::of(profile);
                let places: Vec<Point3> = (0..used.len())
                    .filter(|v| !used[*v])
                    .map(|v| frame.place(profile.vertices[v].point))
                    .collect();
                ensure!(
                    !places.is_empty(),
                    "the sketch {sketch:?} has no points: put one where each hole goes"
                );
                (places, frame.normal())
            }
            HolePlace::Offsets => {
                let on = (*builder)
                    .resolve(args.face.as_ref().context("pick the face to drill into")?)?;
                // The planes it is a distance from: a face of the body
                // is measured from into the body, a plane to its front.
                let mut rows = vec![(on.normal(), on.normal().dot(on.origin()))];
                for (plane, offset, what) in [
                    (&args.first, &args.first_offset, "the first distance"),
                    (&args.second, &args.second_offset, "the second distance"),
                ] {
                    let plane = plane.as_ref().context("pick the two planes it is off")?;
                    let offset = measure(offset, what)?;
                    let inward = matches!(plane.source, shapecraft_core::Source::Topology(_));
                    let plane = (*builder).resolve(plane)?;
                    let offset = if inward { -offset } else { offset };
                    rows.push((plane.normal(), plane.normal().dot(plane.origin()) + offset));
                }
                // Where the three planes meet.
                let (n, d): (Vec<Point3>, Vec<f64>) = rows.into_iter().unzip();
                let volume = n[0].dot(n[1].cross(n[2]));
                ensure!(
                    volume.abs() > 1e-9,
                    "the face and the two planes meet in no one point: two of them are parallel"
                );
                let at =
                    (n[1].cross(n[2]) * d[0] + n[2].cross(n[0]) * d[1] + n[0].cross(n[1]) * d[2])
                        / volume;
                (vec![at], on.normal())
            }
            HolePlace::Polar => {
                let on = (*builder)
                    .resolve(args.face.as_ref().context("pick the face to drill into")?)?;
                let axis = (*builder)
                    .resolve(args.axis.as_ref().context("pick the axis it is around")?)?;
                let radius = measure(&args.radius, "the distance from the axis")?;
                let angle = measure(&args.angle, "the angle")?.to_radians();
                let across = axis.direction().dot(on.normal());
                ensure!(
                    across.abs() > 1e-9,
                    "the axis lies along the face: it has to come through it"
                );
                let along = (on.origin() - axis.origin()).dot(on.normal()) / across;
                let centre = axis.origin() + axis.direction() * along;
                let at = centre + (on.x_dir() * angle.cos() + on.y_dir() * angle.sin()) * radius;
                (vec![at], on.normal())
            }
        };
        let direction = out_of * if args.flip { 1.0 } else { -1.0 };
        let topo = &world.topo;
        // The bodies drilled, as each hole leaves them for the next.
        let targets = bodies::targets(topo, &self.id, &args.target)?;
        anyhow::ensure!(!targets.is_empty(), "there is no solid body to work on");
        let mut drilled = Vec::new();
        for (body, name) in &targets {
            drilled.push((*body, name, shapecraft_boolean::read(topo, *body)?, false));
        }
        // It goes into the material that is nearest where it starts,
        // whichever side that is: from a plane off the body towards the
        // body, from a plane through it into what lies against the
        // plane there.
        let direction = {
            let all: Vec<_> = targets.iter().map(|t| t.0).collect();
            let solids: Vec<&Solid> = drilled.iter().map(|d| &d.2).collect();
            let comes_in = |way: Point3| {
                let at = places.first()?;
                let far = bodies::reach(topo, &all, *at, way).filter(|far| *far > 1e-9)?;
                let starts = lines(*at, way, bore / 2.0);
                first_in(&solids, &starts, way, far).map(|t| (t, far))
            };
            match (comes_in(direction), comes_in(-direction)) {
                (None, Some(_)) => -direction,
                (Some((said, far)), Some((other, _))) if other + far / 32.0 < said => -direction,
                _ => direction,
            }
        };
        let mut threads = Vec::new();
        // How deep the first hole is, and its head: for the callout.
        let mut shown = None;
        for (k, place) in places.iter().enumerate() {
            // How deep it is where it goes through: to where it comes
            // out, if that is asked for — for a chamfer there, or for
            // how long its thread is.
            let mut came_out = None;
            // How far along the drill's way from this hole's mouth the
            // plane of `face` is, and `offset` on.
            let up_to = |face: &Option<shapecraft_core::Reference<DatumPlane>>,
                         offset: &Measure,
                         what: &str|
             -> anyhow::Result<f64> {
                let plane = (*builder).resolve(
                    face.as_ref()
                        .context("pick the face or plane it goes up to")?,
                )?;
                let across = direction.dot(plane.normal());
                ensure!(
                    across.abs() > 1e-9,
                    "the face stands along the hole's way: it ends nowhere on it"
                );
                let along =
                    (plane.origin() - *place).dot(plane.normal()) / across + measure(offset, what)?;
                ensure!(
                    along > 1e-9,
                    "the face is not ahead of where the hole starts ({along:.3})"
                );
                Ok(along)
            };
            let depth = if through {
                let all: Vec<_> = targets.iter().map(|t| t.0).collect();
                let far = bodies::reach(topo, &all, *place, direction)
                    .filter(|far| *far > 1e-9)
                    .context("no body lies that way")?;
                let starts = lines(*place, direction, bore / 2.0);
                if exit.is_some() || tapped {
                    let solids: Vec<&Solid> = drilled.iter().map(|d| &d.2).collect();
                    came_out = way_out(&solids, &starts, direction, far);
                }
                if args.extent == Extent::ToNext {
                    // Through the wall it starts in, and a little on:
                    // not as far as where the next wall begins.
                    let solids: Vec<&Solid> = drilled.iter().map(|d| &d.2).collect();
                    let (out, again) = first_out(&solids, &starts, direction, far)
                        .context("the hole comes out of no body")?;
                    if exit.is_some() {
                        came_out = Some(out);
                        out
                    } else {
                        out + ((again - out) / 2.0).min(far * 0.01)
                    }
                } else {
                    match came_out.filter(|_| exit.is_some()) {
                        Some(out) => out,
                        None => {
                            ensure!(exit.is_none(), "the hole comes out of no body");
                            far * 1.1 + 1.0
                        }
                    }
                }
            } else if args.extent == Extent::ToFace {
                up_to(&args.up_to, &args.offset, "the offset")?
            } else {
                depth
            };
            let head_depth = match (args.kind, args.head_extent) {
                (HoleKind::Counterbore, Extent::ToFace) => {
                    up_to(&args.head_up_to, &args.head_offset, "the head's offset")?
                }
                _ => sized.head_depth,
            };
            shown.get_or_insert((depth, head_depth));
            let shape = HoleShape {
                kind: args.kind,
                radius: bore / 2.0,
                depth,
                point: (!through).then_some(point),
                head: sized.head / 2.0,
                head_depth,
                head_angle: sized.head_angle.to_radians(),
                entry,
                exit,
            };
            let id = if k == 0 {
                self.id.clone()
            } else {
                format!("{}/h{k}", self.id)
            };
            let (outline, side) = shape.walled()?;
            let tool = drill(&id, *place, direction, &outline)?;
            // (The bore's wall: what the outline's side sweeps.)
            let wall = format!("{id}/side/e{side}");
            let top = outline[side].1;
            for (_, name, solid, changed) in &mut drilled {
                let outcome =
                    shapecraft_boolean::combine(shapecraft_boolean::Op::Cut, solid, &tool)
                        .with_context(|| format!("cutting {name}"))?;
                if !outcome.touched {
                    continue;
                }
                *solid = outcome.solid;
                *changed = true;
                // A modelled thread: cut into the bore's wall.
                let face = solid
                    .faces
                    .iter()
                    .position(|f| f.name.as_deref() == Some(&*wall) || f.also.contains(&wall));
                if let Some(face) = face.filter(|_| tapped && args.modelled) {
                    let deep = (diameter - bore) / 2.0;
                    *solid = shapecraft_shape::threaded(solid, face, pitch, deep, false)
                        .with_context(|| format!("threading {name}"))?;
                }
            }
            // A cosmetic thread: said of the bore's wall, as long as
            // the bore is.
            if tapped && !args.modelled {
                let length = came_out.filter(|_| through).unwrap_or(depth) - top;
                let said = shapecraft_core::world::Thread {
                    face: wall,
                    origin: *place + direction * top,
                    axis: direction,
                    length,
                    major: diameter,
                    minor: bore,
                    pitch,
                    left: false,
                    internal: true,
                    designation: crate::thread::designation(diameter, pitch),
                };
                threads.push((format!("{id}/thread"), said));
            }
        }
        for (body, name, solid, changed) in &drilled {
            if *changed {
                out.remove(topo, *body);
                if !solid.is_empty() {
                    out.write(solid, name);
                }
            }
        }
        // What it is, said in one callout at the first hole (ISO
        // 129-1): `4× Ø6 ↧10`, its head under it — `⌴Ø11 ↧6`.
        if let Some(place) = places.first() {
            use shapecraft_core::world::{Annotation, Callout, Note};
            let text = |t: &str| Callout::Text(t.to_owned());
            let mut first = Vec::new();
            if places.len() > 1 {
                first.push(Callout::Count(places.len()));
            }
            if tapped {
                first.push(text(&crate::thread::designation(diameter, pitch)));
            } else {
                first.push(text("Ø"));
                first.push(Callout::Number(diameter));
                first.push(text(&sized.tolerance.suffix()));
            }
            let (depth, head_depth) = shown.unwrap_or((depth, sized.head_depth));
            // A depth that is no typed number — it comes of a face — is
            // a reference dimension, in brackets.
            let said = |x: f64, typed: bool| {
                if typed {
                    vec![Callout::Number(x)]
                } else {
                    vec![text("("), Callout::Number(x), text(")")]
                }
            };
            if !through {
                first.extend([text(" "), Callout::Depth]);
                first.extend(said(depth, args.extent == Extent::Distance));
            }
            let mut lines = vec![first];
            match args.kind {
                HoleKind::Counterbore => {
                    let mut line = vec![
                        Callout::Counterbore,
                        text("Ø"),
                        Callout::Number(sized.head),
                        text(" "),
                        Callout::Depth,
                    ];
                    line.extend(said(head_depth, args.head_extent == Extent::Distance));
                    lines.push(line);
                }
                HoleKind::Countersink => lines.push(vec![
                    Callout::Countersink,
                    text("Ø"),
                    Callout::Number(sized.head),
                    text(" × "),
                    Callout::Number(sized.head_angle),
                    text("°"),
                ]),
                _ => {}
            }
            let radius = bore.max(diameter) / 2.0;
            out.made.annotations.push((
                format!("{}/callout", self.id),
                Annotation {
                    at: *place + direction.any_orthonormal_vector() * (radius * 3.0),
                    sketch: None,
                    note: Note::Hole {
                        centre: *place,
                        axis: direction,
                        radius,
                        lines,
                        outside: false,
                    },
                },
            ));
        }
        out.made.threads = threads;
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let a = &self.args;
        let size = match (a.standard().ok().flatten(), a.kind) {
            (Some(size), HoleKind::Tapped) => size.designation(a.standard_pitch(size)),
            (Some(size), HoleKind::Simple) => format!("for {}", size.name),
            (Some(size), HoleKind::Counterbore) => format!("counterbore for {}", size.name),
            (Some(size), HoleKind::Countersink) => format!("countersink for {}", size.name),
            (None, HoleKind::Tapped) => format!("thread ⌀{} × {}", a.diameter, a.pitch),
            (None, HoleKind::Simple) => match &a.class {
                Some(class)
                    if a.diameter.tolerance == shapecraft_core::world::Tolerance::General =>
                {
                    let limits = norm::hole_limits(class, a.diameter.value)
                        .map(|(upper, lower)| format!(" (+{upper:.3}/+{lower:.3})"))
                        .unwrap_or_default();
                    format!("⌀{} {class}{limits}", a.diameter)
                }
                _ => format!("⌀{}", a.diameter),
            },
            (None, HoleKind::Counterbore) => format!("⌀{}, counterbore ⌀{}", a.diameter, a.head),
            (None, HoleKind::Countersink) => format!("⌀{}, countersink ⌀{}", a.diameter, a.head),
        };
        let placed = match a.place {
            HolePlace::Sketch => "",
            HolePlace::Offsets => ", off two planes",
            HolePlace::Polar => ", around an axis",
        };
        let deburred = match a.deburr {
            Deburr::None => "",
            Deburr::Entry => ", chamfered",
            Deburr::Exit => ", chamfered where it comes out",
            Deburr::Both => ", chamfered at both ends",
        };
        match a.extent {
            Extent::ThroughAll | Extent::ThroughBoth | Extent::Symmetric => {
                format!("{size}, through{placed}{deburred}")
            }
            Extent::ToNext => format!("{size}, through the first wall{placed}{deburred}"),
            Extent::ToFace => format!("{size}, up to a face{placed}{deburred}"),
            Extent::Distance => format!("{size}, {} deep{placed}{deburred}", a.depth),
        }
    }
}

impl shapecraft_core::Choices for HoleKind {
    const ALL: &'static [Self] = &Self::ALL;

    fn label(self) -> &'static str {
        Self::label(self)
    }
}

#[cfg(test)]
mod first_out_tests {
    use super::*;

    #[test]
    fn a_line_first_leaves_the_wall_it_starts_in() {
        // A plate 2 thick, and another 5 above it.
        let axes = [Point3::X, Point3::Y, Point3::Z];
        let plate = shapecraft_boolean::block("a", Point3::ZERO, axes, [4.0, 4.0, 2.0]).unwrap();
        let above =
            shapecraft_boolean::block("b", Point3::new(0.0, 0.0, 7.0), axes, [4.0, 4.0, 1.0])
                .unwrap();
        let from = Point3::new(2.0, 2.0, 0.0);
        let (out, again) = first_out(&[&plate, &above], &[from], Point3::Z, 10.0).unwrap();
        assert!((out - 2.0).abs() < 1e-9, "{out}");
        assert!(again > 6.9 && again < 7.1, "{again}");
        // (Alone, it comes into nothing again.)
        let (out, again) = first_out(&[&plate], &[from], Point3::Z, 10.0).unwrap();
        assert!((out - 2.0).abs() < 1e-9 && again > 10.0);
    }

    #[test]
    fn a_bore_over_a_slot_is_told_by_its_wall() {
        // Two plates 2 thick with a gap 1 wide between them, under the
        // bore's axis: the axis meets nothing, the bore's wall does.
        let axes = [Point3::X, Point3::Y, Point3::Z];
        let left =
            shapecraft_boolean::block("a", Point3::new(-5.0, -5.0, 0.0), axes, [4.5, 10.0, 2.0])
                .unwrap();
        let right =
            shapecraft_boolean::block("b", Point3::new(0.5, -5.0, 0.0), axes, [4.5, 10.0, 2.0])
                .unwrap();
        let from = Point3::new(0.0, 0.0, 5.0);
        assert!(first_out(&[&left, &right], &[from], -Point3::Z, 10.0).is_none());
        let around = lines(from, -Point3::Z, 3.0);
        let (out, _) = first_out(&[&left, &right], &around, -Point3::Z, 10.0).unwrap();
        assert!((out - 5.0).abs() < 1e-6, "{out}");
        // It comes into them 3 on: nearer than anything the other way.
        let comes_in = first_in(&[&left, &right], &around, -Point3::Z, 10.0).unwrap();
        assert!((comes_in - 3.0).abs() < 0.2, "{comes_in}");
        assert!(first_in(&[&left, &right], &around, Point3::Z, 10.0).is_none());
    }
}
