//! What a sketch is made of: points, the entities drawn through them, and
//! the constraints and dimensions between them. All of it is the sketch
//! operation's arguments: it is saved with the part, solved.

use std::collections::{BTreeMap, BTreeSet};

use workpiece_core::math::Point2;
use workpiece_core::world::DatumPlane;
use workpiece_core::{Reference, Source};

/// The id of a point, an entity or a constraint of a sketch. Ids are
/// never reused within a sketch, so names made from them stay.
pub type Id = u32;

/// The tolerance of what is drawn: the solver holds constraints far
/// tighter, so it only has to absorb rounding (as for the basic shapes).
pub const DEFAULT_TOLERANCE: f64 = 1e-7;

fn default_tolerance() -> f64 {
    DEFAULT_TOLERANCE
}

/// One of the sketch plane's two axes.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SketchAxis {
    /// The plane's x direction.
    U,
    /// The plane's y direction.
    V,
}

impl SketchAxis {
    /// The unit vector along the axis.
    pub fn unit(self) -> Point2 {
        match self {
            SketchAxis::U => Point2::X,
            SketchAxis::V => Point2::Y,
        }
    }

    /// The other axis.
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            SketchAxis::U => SketchAxis::V,
            SketchAxis::V => SketchAxis::U,
        }
    }

    /// The axis most nearly along `direction` (either way).
    pub fn nearest(direction: Point2) -> Self {
        if direction.x.abs() >= direction.y.abs() {
            SketchAxis::U
        } else {
            SketchAxis::V
        }
    }
}

/// A point of a sketch.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    /// Where it is, in the plane's coordinates.
    pub at: Point2,
    /// Whether it stays where it is: fixed by the user, or projected.
    #[serde(default)]
    pub fixed: bool,
    /// Radius of the disk around `at` that the true point lies in.
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
}

/// The geometry of an entity, through points of the sketch.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Shape {
    /// A point on its own.
    Point(Id),
    /// A segment between two points.
    Line(Id, Id),
    /// A circle.
    Circle {
        /// Its center point.
        center: Id,
        /// Its radius.
        radius: f64,
    },
    /// An arc, counter-clockwise from `start` to `end`.
    Arc {
        /// Its center point.
        center: Id,
        /// Where it starts.
        start: Id,
        /// Where it ends.
        end: Id,
    },
    /// A NURBS curve through its hull: the control points.
    Spline {
        /// The control points.
        control: Vec<Id>,
        /// The degree (lowered while there are too few points for it).
        degree: usize,
        /// The knots; empty for clamped uniform ones.
        #[serde(default)]
        knots: Vec<f64>,
        /// A weight per control point; empty for all 1.
        #[serde(default)]
        weights: Vec<f64>,
    },
    /// A rotation axis: the axis the sketch is turned about, a
    /// construction line without end through a point, in a direction
    /// (a unit vector the solver keeps one long): three degrees of
    /// freedom. Dimensioned to from a point or a line, it gives a
    /// diameter.
    Axis {
        /// The point it passes through.
        through: Id,
        /// Its direction, of length one.
        direction: Point2,
    },
}

/// Something drawn in a sketch.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entity {
    /// Its geometry.
    pub shape: Shape,
    /// Whether it is construction geometry: there to constrain against,
    /// not part of the profile.
    #[serde(default)]
    pub construction: bool,
    /// Half-width of the band around the curve that the true curve lies
    /// in.
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    /// What it is the projection of, if it is one: it follows that, and
    /// cannot be moved.
    #[serde(default)]
    pub projected: Option<Source>,
    /// Whether it was projected without being asked for, for lying in
    /// the sketch plane: if nothing uses it, it goes when what it is the
    /// projection of does.
    #[serde(default)]
    pub auto: bool,
    /// Whether it is where the sketch plane cuts a solid body, offered
    /// unasked (with `auto`): it follows nothing, and goes with the
    /// next offer unless something uses it.
    #[serde(default)]
    pub section: bool,
    /// The axis of the sketch plane it is, if it is one: a construction
    /// line from the origin that goes on without end either way, stays
    /// where it is and cannot be removed.
    #[serde(default)]
    pub axis: Option<SketchAxis>,
}

/// What a constraint says. Points are named by point ids, lines, circles
/// and arcs by entity ids.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    /// Two points are the same place.
    Coincident(Id, Id),
    /// A point lies on a line (anywhere along it), a circle or an arc's
    /// circle.
    PointOn {
        /// The point.
        point: Id,
        /// The line, circle or arc.
        entity: Id,
    },
    /// Two points lie along an axis of the sketch from each other: what
    /// shows as horizontal or vertical, depending on the view.
    Along {
        /// One point.
        a: Id,
        /// The other.
        b: Id,
        /// The axis they are along.
        axis: SketchAxis,
    },
    /// Two lines are parallel.
    Parallel(Id, Id),
    /// Two lines are perpendicular.
    Perpendicular(Id, Id),
    /// Two lines lie on one line.
    Collinear(Id, Id),
    /// A line and a circle (or arc), or two circles, touch.
    Tangent {
        /// One of them.
        a: Id,
        /// The other.
        b: Id,
        /// For two circles: one inside the other.
        internal: bool,
    },
    /// A spline runs on smoothly from a line, an arc (or circle) or
    /// another spline where it ends on it: in the same direction and
    /// with the same curvature (G2).
    Smooth {
        /// The spline.
        spline: Id,
        /// What it runs on from.
        other: Id,
    },
    /// Two lines are as long, or two circles (or arcs) as large.
    Equal(Id, Id),
    /// A point is the middle of a line.
    Midpoint {
        /// The point.
        point: Id,
        /// The line.
        line: Id,
    },
    /// Two points mirror each other in a line.
    Symmetric {
        /// One point.
        a: Id,
        /// Its mirror image.
        b: Id,
        /// The mirror line.
        line: Id,
    },
    /// Dimension: the distance between two points.
    Distance(Id, Id),
    /// Dimension: the distance between two points along an axis of the
    /// sketch (horizontal or vertical, depending on the view).
    DistanceAlong {
        /// One point.
        a: Id,
        /// The other.
        b: Id,
        /// The axis it is measured along.
        axis: SketchAxis,
    },
    /// Dimension: the distance of a point from a line; as a diameter
    /// (see [`Dimension::diameter`]) twice that — the diameter, about the
    /// line as an axis, of what the point sweeps turned about it (a
    /// revolve's profile), shown with Ø.
    PointLine {
        /// The point.
        point: Id,
        /// The line.
        line: Id,
    },
    /// Dimension: the distance of a point from the side of a circle or
    /// an arc: its near side, or (`far`) the one beyond its middle.
    PointRound {
        /// The point.
        point: Id,
        /// The circle or arc.
        round: Id,
        /// To the far side, not the near one.
        far: bool,
    },
    /// Dimension: the distance of a line from the side of a circle or
    /// an arc: its near side, or (`far`) the one beyond its middle — a
    /// slot's whole length, from its end line to the far side of its
    /// other end.
    LineRound {
        /// The line.
        line: Id,
        /// The circle or arc.
        round: Id,
        /// To the far side, not the near one.
        far: bool,
    },
    /// Dimension: the gap between two circles or arcs where they are
    /// nearest: between two beside each other, or (one inside the
    /// other) the wall between them.
    Gap(Id, Id),
    /// Dimension: the size of a circle or an arc: its radius, or its
    /// diameter where the dimension is one (see [`Dimension::diameter`]).
    Size(Id),
    /// Dimension: the angle from the first line to the second,
    /// counter-clockwise, in degrees.
    Angle(Id, Id),
    /// Dimension: curve `b` is curve `a` moved a distance to its side: a
    /// parallel line, or a concentric arc or circle. The pairs of one
    /// offset (one `group`) share the distance: only the first shows it.
    Offset {
        /// The original.
        a: Id,
        /// The copy.
        b: Id,
        /// The copy is to the right of the line (not the left), or inside
        /// the circle (not outside).
        flip: bool,
        /// The constraint whose value all pairs of the offset share.
        group: Id,
    },
}

impl Kind {
    /// Whether it is a dimension: a constraint with a value.
    pub fn is_dimension(&self) -> bool {
        matches!(
            self,
            Kind::Distance(..)
                | Kind::DistanceAlong { .. }
                | Kind::PointLine { .. }
                | Kind::PointRound { .. }
                | Kind::LineRound { .. }
                | Kind::Gap(..)
                | Kind::Size(_)
                | Kind::Angle(..)
                | Kind::Offset { .. }
        )
    }

    /// The points and the entities it names.
    pub fn operands(&self) -> (Vec<Id>, Vec<Id>) {
        match *self {
            Kind::Coincident(a, b) | Kind::Distance(a, b) => (vec![a, b], vec![]),
            Kind::Along { a, b, .. } | Kind::DistanceAlong { a, b, .. } => (vec![a, b], vec![]),
            Kind::PointOn { point, entity } => (vec![point], vec![entity]),
            Kind::Midpoint { point, line } | Kind::PointLine { point, line } => {
                (vec![point], vec![line])
            }
            Kind::PointRound { point, round, .. } => (vec![point], vec![round]),
            Kind::LineRound { line, round, .. } => (vec![], vec![line, round]),
            Kind::Gap(a, b) => (vec![], vec![a, b]),
            Kind::Parallel(a, b)
            | Kind::Perpendicular(a, b)
            | Kind::Collinear(a, b)
            | Kind::Equal(a, b)
            | Kind::Angle(a, b)
            | Kind::Offset { a, b, .. }
            | Kind::Smooth {
                spline: a,
                other: b,
            }
            | Kind::Tangent { a, b, .. } => (vec![], vec![a, b]),
            Kind::Symmetric { a, b, line } => (vec![a, b], vec![line]),
            Kind::Size(e) => (vec![], vec![e]),
        }
    }
}

/// The tolerance of a dimension, for the drawing made from it later.
pub use workpiece_core::world::Tolerance as DimTolerance;

/// What a dimension measures is its constraint's kind; this is its
/// value, whether it holds that value or only measures, whether it is
/// a diameter, where its text is, and its tolerance.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Dimension {
    /// The value: a length (a diameter, for one), or an angle in
    /// degrees.
    pub value: f64,
    /// Whether it is driven: it takes no part in solving and only
    /// measures (its value follows the sketch), shown in brackets.
    /// Else it is driving: the sketch is made to have its value.
    #[serde(default)]
    pub driven: bool,
    /// Whether it gives a diameter, twice what its kind measures: of a
    /// circle or an arc (`Kind::Size`), or about a line as the axis
    /// (`Kind::PointLine`). It stays one until this is changed.
    #[serde(default)]
    pub diameter: bool,
    /// The formula over the part's parameters that gives the value, if
    /// one does: each build values it anew.
    #[serde(default)]
    pub formula: Option<String>,
    /// Where its text sits, in the plane's coordinates.
    pub label: Point2,
    /// Its tolerance.
    #[serde(default)]
    pub tolerance: DimTolerance,
    /// What it is called among what is said of the model
    /// (`{sketch}/{name}`), if it is given a name; else it is called by
    /// its constraint's number (`{sketch}/d{id}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A constraint, with its value if it is a dimension.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Constraint {
    /// What it says.
    pub kind: Kind,
    /// The value and its label, for a dimension.
    #[serde(default)]
    pub dimension: Option<Dimension>,
}

/// A sketch: its plane and everything drawn on it.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    workpiece_core::Arguments,
)]
pub struct SketchArgs {
    /// The plane it is drawn on.
    pub plane: Option<Reference<DatumPlane>>,
    /// The points, by id.
    #[serde(default)]
    pub points: BTreeMap<Id, Point>,
    /// The entities, by id.
    #[serde(default)]
    pub entities: BTreeMap<Id, Entity>,
    /// The constraints and dimensions, by id. Earlier ones win when they
    /// contradict.
    #[serde(default)]
    pub constraints: BTreeMap<Id, Constraint>,
    /// The next free id.
    #[serde(default)]
    pub next: Id,
}

/// Something of a sketch that can be pointed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Item {
    /// A point.
    Point(Id),
    /// An entity.
    Entity(Id),
    /// A constraint or a dimension.
    Constraint(Id),
}

impl SketchArgs {
    /// A fresh id.
    pub fn fresh(&mut self) -> Id {
        self.next += 1;
        self.next
    }

    /// Adds a point at `at`.
    pub fn add_point(&mut self, at: Point2) -> Id {
        let id = self.fresh();
        let point = Point {
            at,
            fixed: false,
            tolerance: DEFAULT_TOLERANCE,
        };
        self.points.insert(id, point);
        id
    }

    /// Adds an entity of `shape`.
    pub fn add(&mut self, shape: Shape) -> Id {
        let id = self.fresh();
        let entity = Entity {
            shape,
            construction: false,
            tolerance: DEFAULT_TOLERANCE,
            projected: None,
            auto: false,
            section: false,
            axis: None,
        };
        self.entities.insert(id, entity);
        id
    }

    /// Adds a line from `a` to `b`.
    pub fn add_line(&mut self, a: Id, b: Id) -> Id {
        self.add(Shape::Line(a, b))
    }

    /// Adds the constraint `kind`.
    pub fn constrain(&mut self, kind: Kind) -> Id {
        let id = self.fresh();
        let constraint = Constraint {
            kind,
            dimension: None,
        };
        self.constraints.insert(id, constraint);
        id
    }

    /// Adds the dimension `kind` with `value`, its text at `label`.
    pub fn dimension(&mut self, kind: Kind, value: f64, label: Point2) -> Id {
        let id = self.fresh();
        let dimension = Some(Dimension {
            value,
            driven: false,
            diameter: false,
            formula: None,
            label,
            tolerance: DimTolerance::General,
            name: None,
        });
        self.constraints.insert(id, Constraint { kind, dimension });
        id
    }

    /// Adds the dimension `kind` with `value`, its text at `label`, as a
    /// diameter if `diameter` (where that applies, see
    /// [`Self::diameter_applies`]).
    pub fn dimension_of(&mut self, kind: Kind, value: f64, label: Point2, diameter: bool) -> Id {
        let id = self.dimension(kind, value, label);
        if let Some(d) = self.dimension_mut(id) {
            d.diameter = diameter && Self::diameter_applies(&kind);
        }
        id
    }

    /// The value and the looks of dimension `id`, to change.
    pub fn dimension_mut(&mut self, id: Id) -> Option<&mut Dimension> {
        self.constraints
            .get_mut(&id)
            .and_then(|c| c.dimension.as_mut())
    }

    /// Whether a dimension of `kind` can be a diameter: of a circle or
    /// an arc, or from a point or a line to a line taken as the axis.
    pub fn diameter_applies(kind: &Kind) -> bool {
        matches!(kind, Kind::Size(_) | Kind::PointLine { .. })
    }

    /// Whether constraint `id` is a driven dimension.
    pub fn is_driven(&self, id: Id) -> bool {
        self.constraints
            .get(&id)
            .and_then(|c| c.dimension.as_ref())
            .is_some_and(|d| d.driven)
    }

    /// Makes dimension `id` driven (it only measures), or driving (it
    /// holds its value): with the value it has now, which is what a
    /// driven one measures. A driven one has no formula.
    pub fn set_driven(&mut self, id: Id, driven: bool) {
        if let Some(d) = self.dimension_mut(id) {
            d.driven = driven;
            if driven {
                d.formula = None;
            }
        }
        self.refresh_driven();
    }

    /// Makes dimension `id` a diameter, or not (where that applies): its
    /// value doubles or halves with it, so the sketch stays as it is.
    pub fn set_diameter(&mut self, id: Id, diameter: bool) {
        let Some(c) = self.constraints.get_mut(&id) else {
            return;
        };
        let applies = Self::diameter_applies(&c.kind);
        if let Some(d) = c.dimension.as_mut()
            && applies
            && d.diameter != diameter
        {
            d.diameter = diameter;
            d.value *= if diameter { 2.0 } else { 0.5 };
            // (A formula said the other value.)
            d.formula = None;
        }
    }

    /// Where point `id` is (the origin if there is none).
    pub fn at(&self, id: Id) -> Point2 {
        self.points.get(&id).map_or(Point2::ZERO, |p| p.at)
    }

    /// The points that only projected curves have — where the model
    /// is, not something the sketch says — each with whether it is an
    /// end of such a curve (a centre, a line's or an arc's end, a
    /// spline's first or last point) or one of a spline's control points
    /// between its ends. Those between are neither shown nor pointed at:
    /// a curve traced off the model has hundreds. The ends are pointed
    /// at, and shown when they are.
    pub fn projected_points(&self) -> BTreeMap<Id, bool> {
        let mut found: BTreeMap<Id, bool> = BTreeMap::new();
        let mut own: BTreeSet<Id> = BTreeSet::new();
        for (&id, entity) in &self.entities {
            let points = self.points_of(id);
            if entity.projected.is_none() {
                own.extend(points);
                continue;
            }
            let between = matches!(entity.shape, Shape::Spline { .. });
            let last = points.len().saturating_sub(1);
            for (k, point) in points.into_iter().enumerate() {
                let end = !between || k == 0 || k == last;
                let known = found.entry(point).or_insert(end);
                *known = *known || end;
            }
        }
        found.retain(|point, _| !own.contains(point));
        found
    }

    /// The points entity `id` goes through.
    pub fn points_of(&self, id: Id) -> Vec<Id> {
        match self.entities.get(&id).map(|e| &e.shape) {
            Some(Shape::Point(p)) => vec![*p],
            Some(Shape::Line(a, b)) => vec![*a, *b],
            Some(Shape::Circle { center, .. }) => vec![*center],
            Some(Shape::Arc { center, start, end }) => vec![*center, *start, *end],
            Some(Shape::Spline { control, .. }) => control.clone(),
            Some(Shape::Axis { through, .. }) => vec![*through],
            None => Vec::new(),
        }
    }

    /// The end points of line `id`; of a rotation axis, the point it
    /// passes through and that point moved one along it.
    pub fn line(&self, id: Id) -> Option<(Point2, Point2)> {
        match self.entities.get(&id)?.shape {
            Shape::Line(a, b) => Some((self.at(a), self.at(b))),
            Shape::Axis { through, direction } => {
                let at = self.at(through);
                Some((at, at + direction))
            }
            _ => None,
        }
    }

    /// The center point's id, the center and the radius of circle or arc
    /// `id`.
    pub fn circle(&self, id: Id) -> Option<(Id, Point2, f64)> {
        match self.entities.get(&id)?.shape {
            Shape::Circle { center, radius } => Some((center, self.at(center), radius)),
            Shape::Arc { center, start, .. } => {
                let c = self.at(center);
                Some((center, c, c.distance(self.at(start))))
            }
            _ => None,
        }
    }

    /// Whether constraint `id` shows: all do but the pairs of an offset
    /// after its first, which share that one's distance.
    pub fn shows(&self, id: Id) -> bool {
        match self.constraints.get(&id).map(|c| c.kind) {
            Some(Kind::Offset { group, .. }) => {
                group == id || !self.constraints.contains_key(&group)
            }
            Some(_) => true,
            None => false,
        }
    }

    /// Whether entity `id` is a line.
    pub fn is_line(&self, id: Id) -> bool {
        self.line(id).is_some()
    }

    /// Whether entity `id` cannot be moved: it is projected.
    pub fn is_projected(&self, id: Id) -> bool {
        self.entities
            .get(&id)
            .is_some_and(|e| e.projected.is_some())
    }

    /// Whether entity `id` is one of the sketch plane's axes.
    pub fn is_axis(&self, id: Id) -> bool {
        self.entities.get(&id).is_some_and(|e| e.axis.is_some())
    }

    /// Whether entity `id` is a rotation axis.
    pub fn is_rotation_axis(&self, id: Id) -> bool {
        self.entities
            .get(&id)
            .is_some_and(|e| matches!(e.shape, Shape::Axis { .. }))
    }

    /// Whether entity `id` is a line without end: an axis of the plane,
    /// or a rotation axis. It has no length, and is met anywhere along
    /// it.
    pub fn is_endless(&self, id: Id) -> bool {
        self.is_axis(id) || self.is_rotation_axis(id)
    }

    /// The axis of the plane `axis` is, if the sketch has it.
    pub fn plane_axis(&self, axis: SketchAxis) -> Option<Id> {
        self.entities
            .iter()
            .find(|(_, e)| e.axis == Some(axis))
            .map(|(&id, _)| id)
    }

    /// Adds a rotation axis through point `through`, along `direction`.
    pub fn add_rotation_axis(&mut self, through: Id, direction: Point2) -> Id {
        let direction = direction.try_normalize().unwrap_or(Point2::Y);
        let id = self.add(Shape::Axis { through, direction });
        if let Some(e) = self.entities.get_mut(&id) {
            e.construction = true;
        }
        id
    }
    /// Whether point `id` is only there to give an axis its direction:
    /// it does not show, and nothing snaps to it.
    pub fn is_axis_end(&self, id: Id) -> bool {
        self.entities
            .values()
            .any(|e| matches!((e.axis, &e.shape), (Some(_), Shape::Line(_, end)) if *end == id))
    }

    /// The entities that go through point `id`.
    pub fn entities_at(&self, id: Id) -> Vec<Id> {
        self.entities
            .keys()
            .copied()
            .filter(|&e| self.points_of(e).contains(&id))
            .collect()
    }

    /// Removes `items`, and with them what cannot be without: the
    /// entities through a removed point, the points only a removed entity
    /// went through, and the constraints on anything removed.
    pub fn remove(&mut self, items: &[Item]) {
        for item in items {
            match *item {
                Item::Constraint(id) => drop(self.constraints.remove(&id)),
                Item::Entity(id) => self.remove_entity(id),
                // The axes and their origin stay.
                Item::Point(id) if self.entities_at(id).iter().any(|&e| self.is_axis(e)) => {}
                Item::Point(id) => {
                    for entity in self.entities_at(id) {
                        self.remove_entity(entity);
                    }
                    if self.points.contains_key(&id) {
                        self.release(id);
                        self.points.remove(&id);
                    }
                }
            }
        }
        let (points, entities) = (&self.points, &self.entities);
        self.constraints.retain(|_, c| {
            let (p, e) = c.kind.operands();
            p.iter().all(|p| points.contains_key(p)) && e.iter().all(|e| entities.contains_key(e))
        });
    }

    fn remove_entity(&mut self, id: Id) {
        if self.is_axis(id) {
            return;
        }
        let points = self.points_of(id);
        self.entities.remove(&id);
        for p in points {
            if self.entities_at(p).is_empty() {
                self.release(p);
                self.points.remove(&p);
            }
        }
    }

    /// Before point `p` goes: what was joined to it (by coincidence)
    /// stays joined to each other, as the corner they make is still
    /// where it was.
    fn release(&mut self, p: Id) {
        let joined: Vec<Id> = self
            .constraints
            .values()
            .filter_map(|c| match c.kind {
                Kind::Coincident(a, b) if a == p => Some(b),
                Kind::Coincident(a, b) if b == p => Some(a),
                _ => None,
            })
            .collect();
        for pair in joined.windows(2) {
            self.constrain(Kind::Coincident(pair[0], pair[1]));
        }
    }

    /// The size of what is drawn: the diagonal of its bounding box, at
    /// least 1. Tolerances of solves scale with it.
    pub fn scale(&self) -> f64 {
        let mut lo = Point2::splat(f64::INFINITY);
        let mut hi = Point2::splat(f64::NEG_INFINITY);
        for p in self.points.values() {
            lo = lo.min(p.at);
            hi = hi.max(p.at);
        }
        let size = (hi - lo).length();
        if size.is_finite() { size.max(1.0) } else { 1.0 }
    }

    /// The middle of what is drawn (the origin for an empty sketch).
    pub fn center(&self) -> Point2 {
        if self.points.is_empty() {
            return Point2::ZERO;
        }
        let mut lo = Point2::splat(f64::INFINITY);
        let mut hi = Point2::splat(f64::NEG_INFINITY);
        for p in self.points.values() {
            lo = lo.min(p.at);
            hi = hi.max(p.at);
        }
        (lo + hi) / 2.0
    }
}
