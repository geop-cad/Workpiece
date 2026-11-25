//! What every step that places something has in common: where the
//! placed thing is (in the part's cache), whether it is fixed or held
//! by a joint, and the fields and the gizmo that say so.

use std::collections::BTreeMap;
use std::sync::Arc;

use workpiece_core::handles::{Gizmo, Handle};
use workpiece_core::widgets::{
    Choice, ConnectorInput, HBox, Label, Mood, NumericInput, Picked, PoseInput, Scope,
};
use workpiece_core::world::{Connector, Instance, Joint, JointKind};
use workpiece_core::{Builder, Edit, EditContext, Kept, Made, Pose, Widget, World};

use crate::connector::Snap;
use crate::{Anchor, MateArgs};

/// Size of the edit gizmo, in pixels.
const GIZMO_SIZE: f64 = 90.0;

/// How something placed is held where it is put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Constraint {
    /// Not at all: it is dragged anywhere.
    #[default]
    Free,
    /// It stays where it is put: joints and drags move what is
    /// connected to it, never it.
    Fixed,
    /// A joint of this kind between its two references lets it move.
    Joint(JointKind),
}

impl Constraint {
    /// What it is called.
    pub fn label(self) -> &'static str {
        match self {
            Constraint::Free => "Free",
            Constraint::Fixed => "Fixed",
            Constraint::Joint(kind) => kind.label(),
        }
    }

    /// What a step that places something says of it after its name.
    pub fn said(self) -> String {
        match self {
            Constraint::Free => String::new(),
            Constraint::Fixed => ", fixed".to_owned(),
            Constraint::Joint(kind) => format!(", {} joint", kind.label().to_lowercase()),
        }
    }
}

/// What a reference of a joint stands for: what the joint can hold on
/// to there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stands {
    /// A point: a vertex.
    Point,
    /// An axis: an edge, the centre of an arc, the axis of a face that
    /// goes around one.
    Axis,
    /// A plane: a face, by its normal.
    Plane,
    /// A whole frame: a datum.
    Frame,
}

impl Stands {
    fn of(anchor: &Anchor) -> Stands {
        match anchor.snap {
            Snap::Vertex => Stands::Point,
            Snap::EdgeMiddle | Snap::EdgeEnd { .. } | Snap::ArcCentre | Snap::FaceAxis => {
                Stands::Axis
            }
            Snap::FaceCentre | Snap::FaceEdge { .. } | Snap::FaceVertex { .. } => Stands::Plane,
            Snap::Datum => Stands::Frame,
        }
    }

    /// Whether a joint of `kind` can hold on to it.
    fn takes(self, kind: JointKind) -> bool {
        use JointKind::{Ball, Cylindrical, Fastened, Planar, Revolute, Slider};
        match self {
            Stands::Frame => true,
            Stands::Point => matches!(kind, Fastened | Ball),
            Stands::Axis => matches!(kind, Fastened | Revolute | Slider | Cylindrical),
            Stands::Plane => matches!(kind, Fastened | Revolute | Slider | Planar),
        }
    }
}

/// How what a step places is held: a reference on what is there
/// already, one on what is placed, and the constraint between them.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Hold {
    /// The reference on what is in the assembly already.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onto: Option<Anchor>,
    /// The reference on what is placed, told from inside it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with: Option<Anchor>,
    /// How far off the reference on what is there it is held: along
    /// that reference's own x and y (in its plane, for a face) and z
    /// (its normal), after the joint lines the two up.
    #[serde(skip_serializing_if = "is_zero")]
    pub offset: [f64; 3],
    /// What holds it; a joint holds once both references are picked.
    pub constraint: Constraint,
    /// Whether the placed reference faces the other: its z against
    /// that one's.
    pub flip: bool,
    /// The least and greatest angle around a joint's axis, in degrees.
    pub angle: Option<[f64; 2]>,
    /// The least and greatest distance along a joint's axis.
    pub distance: Option<[f64; 2]>,
}

impl Hold {
    /// Something that stays where it is put.
    pub fn fixed() -> Hold {
        Hold {
            constraint: Constraint::Fixed,
            ..Hold::default()
        }
    }

    /// Something a joint of `kind` holds, `onto` a reference of what is
    /// there `with` one of its own.
    pub fn joint(kind: JointKind, onto: Anchor, with: Anchor) -> Hold {
        Hold {
            onto: Some(onto),
            with: Some(with),
            constraint: Constraint::Joint(kind),
            ..Hold::default()
        }
    }

    /// The joint it is, if a joint holds it: as the arguments of one.
    pub fn mate(&self) -> Option<MateArgs> {
        let Constraint::Joint(kind) = self.constraint else {
            return None;
        };
        Some(MateArgs {
            kind,
            a: self.onto.clone(),
            b: self.with.clone(),
            flip: self.flip,
            angle: self.angle,
            distance: self.distance,
        })
    }

    /// The placed reference's frame `frame`, turned to face the other
    /// if that is asked for.
    pub fn facing(&self, frame: Pose) -> Pose {
        if self.flip {
            frame * Pose::from_rotation(glam::DQuat::from_rotation_x(std::f64::consts::PI))
        } else {
            frame
        }
    }

    /// The frame `frame` of the reference on what is there, moved by
    /// its offset: where the joint holds the placed reference.
    pub fn shifted(&self, frame: Pose) -> Pose {
        frame * Pose::from_translation(glam::DVec3::from_array(self.offset))
    }

    /// The joints that fit its two references, in the order they are
    /// offered: none until both are picked.
    pub fn fitting(&self) -> Vec<JointKind> {
        let (Some(onto), Some(with)) = (&self.onto, &self.with) else {
            return Vec::new();
        };
        let (onto, with) = (Stands::of(onto), Stands::of(with));
        // (A point is held as a ball whatever it meets.)
        let point = onto == Stands::Point || with == Stands::Point;
        JointKind::ALL
            .into_iter()
            .filter(|kind| {
                (onto.takes(*kind) && with.takes(*kind)) || (point && *kind == JointKind::Ball)
            })
            .collect()
    }

    /// The joint its two references are most likely picked for: two
    /// axes turn in each other, anything else is fastened.
    fn likely(&self) -> JointKind {
        let axis = |a: &Option<Anchor>| a.as_ref().map(Stands::of) == Some(Stands::Axis);
        if axis(&self.onto) && axis(&self.with) {
            JointKind::Revolute
        } else {
            JointKind::Fastened
        }
    }

    /// The same after a reference was picked (both were there `before`,
    /// or not): a pair that is just complete gets its joint — `prefer`,
    /// or the likely one — and a joint that no longer fits the pair
    /// becomes one that does.
    fn settled(mut self, before: bool, prefer: Option<JointKind>) -> Hold {
        let fitting = self.fitting();
        if fitting.is_empty() {
            return self;
        }
        // Two faces just paired lie against each other, as parts that
        // are put together do: each looks at the other.
        let plane = |a: &Option<Anchor>| a.as_ref().map(Stands::of) == Some(Stands::Plane);
        if !before && plane(&self.onto) && plane(&self.with) {
            self.flip = true;
        }
        let stale = match self.constraint {
            Constraint::Joint(kind) => !fitting.contains(&kind),
            Constraint::Free => !before,
            Constraint::Fixed => false,
        };
        if stale {
            let likely = prefer.unwrap_or_else(|| self.likely());
            let kind = if fitting.contains(&likely) {
                likely
            } else {
                fitting[0]
            };
            self.constraint = Constraint::Joint(kind);
        }
        self
    }
}

/// Whether an offset is none.
fn is_zero(offset: &[f64; 3]) -> bool {
    offset.iter().all(|x| *x == 0.0)
}

/// Puts `world` into `made` as the instance `id` — where the part's
/// cache has it — held as `hold` says: fixed or not, and with its joint
/// once both its references are picked (the first on what is there
/// already, the second on what is placed, told from inside it).
pub fn place(
    made: &mut Made<'_>,
    builder: &dyn Builder,
    id: &str,
    world: Arc<World>,
    hold: &Hold,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !builder.peek().instances.contains_key(id),
        "instance {id:?} exists"
    );
    let pose = crate::assembly::placed(builder.cache(), id);
    if let Some(joint) = hold.mate()
        && let (Some(onto), Some(own)) = (&joint.a, &joint.b)
    {
        anyhow::ensure!(
            onto.instance.first().map(String::as_str) != Some(id),
            "the joint of {id} connects it to itself"
        );
        made.joints.push(Joint {
            name: id.to_owned(),
            kind: joint.kind,
            a: {
                let world = builder.with_placed(onto.instance.first().map(String::as_str));
                let mut a = onto.connector(world)?;
                a.frame = hold.shifted(a.frame);
                a
            },
            b: Connector {
                instance: Some(id.to_owned()),
                frame: joint.facing(own.frame_in(&world)?),
            },
            limits: joint.limits(),
        });
    }
    // Where the part's cache has it; its joints may still move it,
    // after the last step.
    let fixed = hold.constraint == Constraint::Fixed;
    made.instances
        .push((id.to_owned(), Some(Instance { pose, world, fixed })));
    Ok(())
}

/// The fields that say how what the step `step` places is held: a
/// reference on what is there already, one on what is placed, and then
/// the constraint between the two — free, fixed, or one of the joints
/// that fit the pair — with what that joint lets go. A pair that is
/// just complete gets a joint by itself: `prefer`, or the likely one.
pub fn hold_fields(hold: &Hold, step: &str, prefer: Option<JointKind>) -> Vec<Widget<Hold>> {
    let before = hold.onto.is_some() && hold.with.is_some();
    let pick = |onto: bool| {
        let hold = hold.clone();
        move |picked: Picked| {
            let mut hold = hold.clone();
            let Some(anchor) = Anchor::picked(&picked) else {
                return hold;
            };
            if onto {
                hold.onto = Some(anchor);
            } else {
                hold.with = Some(anchor.inside());
            }
            hold.settled(before, prefer)
        }
    };
    let said = |anchor: &Option<Anchor>| anchor.as_ref().map(Anchor::describe);
    let mut fields: Vec<Widget<Hold>> = vec![
        ConnectorInput::new("Onto", said(&hold.onto), pick(true))
            .within(Scope::Outside(step.to_owned()))
            .into(),
        ConnectorInput::new("With", said(&hold.with), pick(false))
            .within(Scope::Inside(step.to_owned()))
            .into(),
    ];
    // How far off the first reference it is held, in that reference's
    // own frame.
    if hold.onto.is_some() {
        let [u, v, w] = hold.offset;
        let along = |i: usize| {
            let hold = hold.clone();
            move |x: f64| {
                let mut hold = hold.clone();
                hold.offset[i] = x;
                hold
            }
        };
        fields.push(
            HBox::new(vec![
                NumericInput::new("Offset x", u, along(0)).into(),
                NumericInput::new("y", v, along(1)).into(),
                NumericInput::new("z (normal)", w, along(2)).into(),
            ])
            .into(),
        );
    }
    // Free, fixed, or a joint that fits the pair (one that is kept and
    // does not still shows).
    let mut constraints = vec![Constraint::Free, Constraint::Fixed];
    constraints.extend(hold.fitting().into_iter().map(Constraint::Joint));
    if !constraints.contains(&hold.constraint) {
        constraints.push(hold.constraint);
    }
    let chosen = constraints.iter().position(|c| *c == hold.constraint);
    let labels: Vec<String> = constraints
        .iter()
        .map(|c| match c {
            Constraint::Joint(kind) => format!("{} joint", kind.label()),
            other => other.label().to_owned(),
        })
        .collect();
    let icons = constraints
        .iter()
        .map(|c| match c {
            Constraint::Free => Some("constraint-free"),
            Constraint::Fixed => Some("constraint-fixed"),
            Constraint::Joint(kind) => workpiece_core::Choices::icon(*kind),
        })
        .collect::<Vec<_>>();
    let with_constraint = {
        let hold = hold.clone();
        move |i: usize| Hold {
            constraint: constraints[i],
            ..hold.clone()
        }
    };
    fields.push(
        Choice::new("Constraint", labels, chosen.unwrap_or(0), with_constraint)
            .with_icons(icons)
            .drop_down()
            .into(),
    );
    if let Some(joint) = hold.mate() {
        for field in crate::mate::motion_fields(&joint) {
            let hold = hold.clone();
            fields.push(field.map(move |joint: MateArgs| Hold {
                flip: joint.flip,
                angle: joint.angle,
                distance: joint.distance,
                ..hold.clone()
            }));
        }
    }
    if !before {
        fields.push(
            Label::new(
                "",
                "To join it, point at a vertex, an edge or a face: first of what it goes \
                 onto, then of the part itself.",
                Mood::Plain,
            )
            .into(),
        );
    }
    fields
}

/// The field that says where something placed is (none while a joint
/// says it), and its handles.
pub type PoseEdit<A, S> = (
    Option<Widget<Edit<A, S>>>,
    BTreeMap<String, Handle<Edit<A, S>>>,
);

/// The field and the gizmo that move what the step `id` places, held
/// as `hold` says: where it is belongs to the part's cache, not to the
/// step's arguments — set by hand here or by a drag. Neither shows
/// once a joint holds it: then the joint says where it is.
pub fn pose_edit<A: 'static, S: 'static>(
    id: &str,
    context: &EditContext<'_>,
    hold: &Hold,
) -> PoseEdit<A, S> {
    if hold.mate().is_some_and(|m| m.a.is_some() && m.b.is_some()) {
        return (None, BTreeMap::new());
    }
    let with_pose = |pose: Pose| crate::assembly::kept(&pose);
    // Where it is now: where it was put or dragged.
    let now = crate::assembly::placed(context.cache, id);
    let place: Widget<Kept> = PoseInput::new("Pose", now, with_pose).into();
    let gizmo: Handle<Kept> = Gizmo::new(now, GIZMO_SIZE, with_pose).into();
    (
        Some(place.map(Edit::kept)),
        BTreeMap::from([("pose".into(), gizmo.map(Edit::kept))]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::Source;

    fn anchor(snap: Snap) -> Anchor {
        Anchor {
            instance: Vec::new(),
            source: Source::Datum("d".into()),
            snap,
        }
    }

    #[test]
    fn the_joints_that_fit_a_pair_are_offered_and_one_is_chosen() {
        use JointKind::{Ball, Cylindrical, Fastened, Planar, Revolute, Slider};
        // Nothing but free and fixed until both references are there.
        let mut hold = Hold {
            onto: Some(anchor(Snap::FaceAxis)),
            ..Hold::default()
        };
        assert!(hold.fitting().is_empty());
        assert_eq!(hold.clone().settled(false, None), hold);
        // Two axes: what turns and slides about one, and it turns.
        hold.with = Some(anchor(Snap::ArcCentre));
        assert_eq!(hold.fitting(), [Fastened, Revolute, Slider, Cylindrical]);
        let hold = hold.settled(false, None);
        assert_eq!(hold.constraint, Constraint::Joint(Revolute));
        // A face against an axis: a planar joint no longer fits, and
        // becomes the likely one; a standard part is fastened.
        let faces = Hold {
            with: Some(anchor(Snap::FaceCentre)),
            constraint: Constraint::Joint(Planar),
            ..hold.clone()
        };
        assert_eq!(faces.fitting(), [Fastened, Revolute, Slider]);
        let settled = faces.clone().settled(true, None);
        assert_eq!(settled.constraint, Constraint::Joint(Fastened));
        // Two faces just paired face each other; picked again later,
        // they stay as they were set.
        let pair = Hold {
            onto: Some(anchor(Snap::FaceCentre)),
            with: Some(anchor(Snap::FaceCentre)),
            ..Hold::default()
        };
        assert!(pair.clone().settled(false, None).flip);
        assert!(!pair.settled(true, None).flip);
        // A vertex is held fastened or as a ball; a choice of free that
        // was made with the pair complete stays.
        let point = Hold {
            with: Some(anchor(Snap::Vertex)),
            constraint: Constraint::Free,
            ..hold
        };
        assert_eq!(point.fitting(), [Fastened, Ball]);
        assert_eq!(
            point.clone().settled(true, None).constraint,
            Constraint::Free
        );
        let fields = hold_fields(&point, "part", None);
        let Some(Widget::Choice(choice)) = fields.get(3) else {
            panic!("expected the constraint's choice");
        };
        assert_eq!(
            choice.options,
            ["Free", "Fixed", "Fastened joint", "Ball joint"]
        );
        assert_eq!(
            choice.set(3).map(|h| h.constraint),
            Some(Constraint::Joint(Ball))
        );
    }
}
