//! What every step that places something has in common: where the
//! placed thing is (in the part's cache), whether it is fixed, the mates
//! that hold it — each a pair of references and the constraint between
//! them — and the fields that say so.

use std::sync::Arc;

use workpiece_core::hotkey::Hotkey;
use workpiece_core::widgets::{
    Choice, ConnectorInput, Group, HBox, Label, Mood, NumericInput, Picked, PoseInput, Scope,
    ToolButton,
};
use workpiece_core::world::{Connector, Instance, Joint, JointKind, Limits};
use workpiece_core::{Builder, Edit, EditContext, Made, Pose, Widget, World};

use crate::connector::Snap;
use crate::{Anchor, MateArgs};

/// What a reference of a mate stands for: what the mate can hold on
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
    /// An axis and a plane: the centre of a round edge of a face (the
    /// rim of a hole), z the face's normal.
    Round,
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
            Snap::FaceEdge {
                at: crate::connector::EdgeAt::Centre,
                ..
            } => Stands::Round,
            Snap::FaceCentre | Snap::FaceEdge { .. } | Snap::FaceVertex { .. } => Stands::Plane,
            Snap::Datum => Stands::Frame,
        }
    }

    /// Whether a mate of `kind` can hold on to it.
    fn takes(self, kind: JointKind) -> bool {
        use JointKind::{Ball, Cylindrical, Fastened, Planar, Revolute, Slider};
        match self {
            Stands::Frame => true,
            Stands::Point => matches!(kind, Fastened | Ball),
            Stands::Axis => matches!(kind, Fastened | Revolute | Slider | Cylindrical),
            Stands::Plane => matches!(kind, Fastened | Revolute | Slider | Planar),
            Stands::Round => !matches!(kind, Ball),
        }
    }
}

/// The constraint a mate of `kind` is, as the mate's drop-down offers
/// it: the joint, and what it holds the two references to.
pub fn constraint_label(kind: JointKind) -> &'static str {
    match kind {
        JointKind::Fastened => "Fastened: one frame",
        JointKind::Revolute => "Revolute: concentric, coincident",
        JointKind::Slider => "Slider: along the axis",
        JointKind::Cylindrical => "Cylindrical: concentric",
        JointKind::Planar => "Planar: on the plane",
        JointKind::Ball => "Ball: coincident points",
    }
}

/// One mate of something placed: a reference on what is in the assembly
/// already, one on what is placed, which way each looks, and the
/// constraint between them (a joint of a kind). It holds once both
/// references are picked.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mating {
    /// The reference on what is in the assembly already.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onto: Option<Anchor>,
    /// The reference on what is placed, told from inside it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with: Option<Anchor>,
    /// The constraint between the two: how what is placed may move.
    pub kind: JointKind,
    /// Whether the reference on what is there is turned over: its z the
    /// other way.
    #[serde(skip_serializing_if = "is_false")]
    pub flip_onto: bool,
    /// Whether the reference on what is placed is turned over: its z
    /// the other way (two faces that lie against each other have one of
    /// them turned over).
    #[serde(skip_serializing_if = "is_false")]
    pub flip_with: bool,
    /// How far off the reference on what is there it is held: along
    /// that reference's own x and y (in its plane, for a face) and z
    /// (its normal), after the mate lines the two up.
    #[serde(skip_serializing_if = "is_zero")]
    pub offset: [f64; 3],
    /// The least and greatest angle around the mate's axis, in degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<[f64; 2]>,
    /// The least and greatest distance along the mate's axis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<[f64; 2]>,
}

/// Half a turn about x: a frame turned over, its z the other way.
fn turned_over(frame: Pose) -> Pose {
    frame * Pose::from_rotation(glam::DQuat::from_rotation_x(std::f64::consts::PI))
}

impl Mating {
    /// A mate of `kind`, `onto` a reference of what is there `with` one
    /// of what is placed.
    pub fn new(kind: JointKind, onto: Anchor, with: Anchor) -> Mating {
        Mating {
            onto: Some(onto),
            with: Some(with),
            kind,
            ..Mating::default()
        }
    }

    /// Its two references, once both are picked.
    pub fn pair(&self) -> Option<(&Anchor, &Anchor)> {
        self.onto.as_ref().zip(self.with.as_ref())
    }

    /// Where it holds the reference of what is placed, given the frame
    /// `frame` of the one on what is there: that frame moved by the
    /// offset, and turned over if that is asked for.
    pub fn onto_frame(&self, frame: Pose) -> Pose {
        let moved = frame * Pose::from_translation(glam::DVec3::from_array(self.offset));
        if self.flip_onto {
            turned_over(moved)
        } else {
            moved
        }
    }

    /// The frame `frame` of the reference on what is placed, turned
    /// over if that is asked for.
    pub fn with_frame(&self, frame: Pose) -> Pose {
        if self.flip_with {
            turned_over(frame)
        } else {
            frame
        }
    }

    /// The limits, as a joint has them.
    pub fn limits(&self) -> Limits {
        self.joint_args().limits()
    }

    /// The same as the arguments of a joint (for the fields of what it
    /// lets go).
    fn joint_args(&self) -> MateArgs {
        MateArgs {
            kind: self.kind,
            a: self.onto.clone(),
            b: self.with.clone(),
            flip: self.flip_with,
            angle: self.angle,
            distance: self.distance,
        }
    }

    /// The constraints that fit its two references, in the order they
    /// are offered: none until both are picked.
    pub fn fitting(&self) -> Vec<JointKind> {
        let Some((onto, with)) = self.pair() else {
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

    /// The constraint its two references are most likely picked for:
    /// two axes (or rims) turn in each other, anything else is fastened.
    fn likely(&self) -> JointKind {
        let axis = |a: &Option<Anchor>| {
            matches!(
                a.as_ref().map(Stands::of),
                Some(Stands::Axis | Stands::Round)
            )
        };
        if axis(&self.onto) && axis(&self.with) {
            JointKind::Revolute
        } else {
            JointKind::Fastened
        }
    }

    /// The same after a reference was picked (both were there `before`,
    /// or not): a pair that is just complete gets its constraint — that
    /// of `seat`, if it is the seat it started from, or the likely one —
    /// and a constraint that no longer fits the pair becomes one that
    /// does.
    fn settled(mut self, before: bool, seat: Option<&Mating>) -> Mating {
        let fitting = self.fitting();
        if fitting.is_empty() {
            return self;
        }
        let seated = seat.filter(|s| s.with.is_some() && s.with == self.with);
        let mut prefer = None;
        if !before {
            if let Some(seat) = seated {
                // A seat says how it sits.
                self.flip_with = seat.flip_with;
                prefer = Some(seat.kind);
            } else {
                // Two faces just paired lie against each other, as parts
                // that are put together do: each looks at the other.
                let plane = |a: &Option<Anchor>| {
                    matches!(
                        a.as_ref().map(Stands::of),
                        Some(Stands::Plane | Stands::Round)
                    )
                };
                if plane(&self.onto) && plane(&self.with) {
                    self.flip_with = true;
                }
            }
        }
        if !before || !fitting.contains(&self.kind) {
            let likely = prefer.unwrap_or_else(|| self.likely());
            self.kind = if fitting.contains(&likely) {
                likely
            } else {
                fitting[0]
            };
        }
        self
    }
}

/// How what a step places is held: fixed where it is put or not, and
/// by its mates.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hold {
    /// Whether it stays where it is put: mates and drags move what is
    /// connected to it, never it.
    #[serde(skip_serializing_if = "is_false")]
    pub fixed: bool,
    /// Its mates, each a pair of references and the constraint between
    /// them; one that lacks a reference holds nothing yet.
    pub mates: Vec<Mating>,
}

impl Hold {
    /// Something that stays where it is put.
    pub fn fixed() -> Hold {
        Hold {
            fixed: true,
            mates: Vec::new(),
        }
    }

    /// Something that is dragged anywhere, with one mate to pick: what a
    /// step that places something starts as.
    pub fn to_pick() -> Hold {
        Hold {
            fixed: false,
            mates: vec![Mating::default()],
        }
    }

    /// Something one mate of `kind` holds, `onto` a reference of what is
    /// there `with` one of its own.
    pub fn joint(kind: JointKind, onto: Anchor, with: Anchor) -> Hold {
        Hold {
            fixed: false,
            mates: vec![Mating::new(kind, onto, with)],
        }
    }

    /// Whether a mate with both its references holds it.
    pub fn mated(&self) -> bool {
        self.mates.iter().any(|m| m.pair().is_some())
    }

    /// What a step that places something says of how it is held, after
    /// its name.
    pub fn said(&self) -> String {
        let made: Vec<&Mating> = self.mates.iter().filter(|m| m.pair().is_some()).collect();
        let mates = match made[..] {
            [] => String::new(),
            [one] => format!(", {} joint", one.kind.label().to_lowercase()),
            _ => format!(", {} mates", made.len()),
        };
        if self.fixed {
            format!(", fixed{mates}")
        } else {
            mates
        }
    }
}

/// The name of the joint the mate `k` (from 0) of what the step `id`
/// places makes: the step's own for the first, then `id/2`, `id/3`.
pub fn joint_name(id: &str, k: usize) -> String {
    match k {
        0 => id.to_owned(),
        _ => format!("{id}/{}", k + 1),
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Whether an offset is none.
fn is_zero(offset: &[f64; 3]) -> bool {
    offset.iter().all(|x| *x == 0.0)
}

/// Puts `world` into `made` as the instance `id` — where the part's
/// cache has it — held as `hold` says: fixed or not, and with a joint
/// for each of its mates that has both its references (the first on
/// what is there already, the second on what is placed, told from
/// inside it).
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
    for (k, mate) in hold.mates.iter().enumerate() {
        let Some((onto, own)) = mate.pair() else {
            continue;
        };
        anyhow::ensure!(
            onto.instance.first().map(String::as_str) != Some(id),
            "mate {} of {id} connects it to itself",
            k + 1
        );
        made.joints.push(Joint {
            name: joint_name(id, k),
            kind: mate.kind,
            a: {
                let world = builder.with_placed(onto.instance.first().map(String::as_str));
                let mut a = onto.connector(world)?;
                a.frame = mate.onto_frame(a.frame);
                a
            },
            b: Connector {
                instance: Some(id.to_owned()),
                frame: mate.with_frame(own.frame_in(&world)?),
            },
            limits: mate.limits(),
        });
    }
    // Where the part's cache has it; its joints may still move it,
    // after the last step.
    made.instances.push((
        id.to_owned(),
        Some(Instance {
            pose,
            world,
            fixed: hold.fixed,
        }),
    ));
    Ok(())
}

/// The fields that say how what the step `step` places is held: whether
/// it is fixed where it is put, then each of its mates — a reference on
/// what is there already and one on what is placed, each with a button
/// that turns it over, and the constraint between the two, of those
/// that fit the pair — with a button to add another. `seat` is the mate
/// a standard part starts with: its constraint and which way it looks
/// once what it goes onto is picked.
pub fn hold_fields(hold: &Hold, step: &str, seat: Option<&Mating>) -> Vec<Widget<Hold>> {
    let mut fields: Vec<Widget<Hold>> = vec![
        Choice::new(
            "Held",
            vec!["By its mates".to_owned(), "Fixed".to_owned()],
            usize::from(hold.fixed),
            {
                let hold = hold.clone();
                move |i| Hold {
                    fixed: i == 1,
                    ..hold.clone()
                }
            },
        )
        .with_icon("constraint-fixed")
        .with_icons(vec![Some("constraint-free"), Some("constraint-fixed")])
        .into(),
    ];
    for (k, mate) in hold.mates.iter().enumerate() {
        let with_mate = {
            let hold = hold.clone();
            move |mate: Mating| {
                let mut hold = hold.clone();
                hold.mates[k] = mate;
                hold
            }
        };
        let mut rows: Vec<Widget<Hold>> = mate_fields(mate, k, step, seat.filter(|_| k == 0))
            .into_iter()
            .map(|field| field.map(with_mate.clone()))
            .collect();
        // (The last mate goes too: what is placed is then dragged
        // anywhere.)
        let remove = {
            let hold = hold.clone();
            move || {
                let mut hold = hold.clone();
                hold.mates.remove(k);
                hold
            }
        };
        rows.push(
            ToolButton::new(
                "mate-remove",
                "Remove this mate",
                Hotkey::shift('X'),
                remove,
            )
            .with_hint("Take this mate away: what it held is free again")
            .into(),
        );
        fields.push(Group::new(format!("Mate {}", k + 1), rows).into());
    }
    let add = {
        let hold = hold.clone();
        move || {
            let mut hold = hold.clone();
            hold.mates.push(Mating::default());
            hold
        }
    };
    fields.push(
        ToolButton::new("mate-add", "Add a mate", Hotkey::shift('M'), add)
            .with_hint("Another pair of references and a constraint between them: two mates hold what one leaves free")
            .into(),
    );
    if !hold.mated() && !hold.fixed {
        fields.push(
            Label::new(
                "",
                "Drag it where it goes, or join it: point at a vertex, an edge or a face, \
                 first of what it goes onto, then of the part itself.",
                Mood::Plain,
            )
            .into(),
        );
    }
    fields
}

/// The fields of the mate `k`: its two references, each with the button
/// that turns it over; the constraint between them; how far off the
/// first it is; and what the constraint lets go.
fn mate_fields(mate: &Mating, k: usize, step: &str, seat: Option<&Mating>) -> Vec<Widget<Mating>> {
    let before = mate.pair().is_some();
    let pick = |onto: bool| {
        let (mate, seat) = (mate.clone(), seat.cloned());
        move |picked: Picked| {
            let mut mate = mate.clone();
            let Some(anchor) = Anchor::picked(&picked) else {
                return mate;
            };
            if onto {
                mate.onto = Some(anchor);
            } else {
                mate.with = Some(anchor.inside());
            }
            mate.settled(before, seat.as_ref())
        }
    };
    let flip = |onto: bool| {
        let mate = mate.clone();
        move || {
            let mut mate = mate.clone();
            if onto {
                mate.flip_onto = !mate.flip_onto;
            } else {
                mate.flip_with = !mate.flip_with;
            }
            mate
        }
    };
    let said = |anchor: &Option<Anchor>| anchor.as_ref().map(Anchor::describe);
    let mut fields: Vec<Widget<Mating>> = vec![
        HBox::new(vec![
            ConnectorInput::new("Onto", said(&mate.onto), pick(true))
                .within(Scope::Outside(step.to_owned()))
                .of_pair(k)
                .into(),
            ToolButton::new("mate-flip", "Flip", Hotkey::shift('F'), flip(true))
                .with_hint("Turn this reference over: its z the other way")
                .active(mate.flip_onto)
                .into(),
        ])
        .into(),
        HBox::new(vec![
            ConnectorInput::new("With", said(&mate.with), pick(false))
                .within(Scope::Inside(step.to_owned()))
                .of_pair(k)
                .into(),
            ToolButton::new("mate-flip", "Flip", Hotkey::key('F'), flip(false))
                .with_hint("Turn the part's reference over: the part the other way round")
                .active(mate.flip_with)
                .into(),
        ])
        .into(),
    ];
    // The constraints that fit the pair (every one, while it is not
    // complete; one that is kept and does not fit still shows).
    let mut kinds = mate.fitting();
    if kinds.is_empty() {
        kinds = JointKind::ALL.to_vec();
    }
    if !kinds.contains(&mate.kind) {
        kinds.push(mate.kind);
    }
    let chosen = kinds.iter().position(|c| *c == mate.kind).unwrap_or(0);
    let labels = kinds
        .iter()
        .map(|kind| constraint_label(*kind).to_owned())
        .collect();
    let icons = kinds
        .iter()
        .map(|kind| workpiece_core::Choices::icon(*kind))
        .collect();
    let with_kind = {
        let mate = mate.clone();
        move |i: usize| Mating {
            kind: kinds[i],
            ..mate.clone()
        }
    };
    fields.push(
        Choice::new("Constraint", labels, chosen, with_kind)
            .with_icon("joint")
            .with_icons(icons)
            .drop_down()
            .into(),
    );
    // How far off the first reference it is held, in that reference's
    // own frame.
    if mate.onto.is_some() {
        let [u, v, w] = mate.offset;
        let along = |i: usize| {
            let mate = mate.clone();
            move |x: f64| {
                let mut mate = mate.clone();
                mate.offset[i] = x;
                mate
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
    // What the constraint lets go (which way the pair faces is the
    // flip buttons').
    for field in crate::mate::motion_fields(&mate.joint_args())
        .into_iter()
        .skip(1)
    {
        let mate = mate.clone();
        fields.push(field.map(move |joint: MateArgs| Mating {
            angle: joint.angle,
            distance: joint.distance,
            ..mate.clone()
        }));
    }
    fields
}

/// The field that says where something placed is, while no mate holds
/// it: where it is belongs to the part's cache, not to the step's
/// arguments — set by hand here, or by dragging the part in the view.
pub fn pose_field<A: 'static, S: 'static>(
    id: &str,
    context: &EditContext<'_>,
    hold: &Hold,
) -> Option<Widget<Edit<A, S>>> {
    if hold.mated() {
        return None;
    }
    let with_pose = |pose: Pose| crate::assembly::kept(&pose);
    // Where it is now: where it was put or dragged.
    let now = crate::assembly::placed(context.cache, id);
    let place: Widget<workpiece_core::Kept> = PoseInput::new("Pose", now, with_pose).into();
    Some(place.map(Edit::kept))
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
    fn the_constraints_that_fit_a_pair_are_offered_and_one_is_chosen() {
        use JointKind::{Ball, Cylindrical, Fastened, Planar, Revolute, Slider};
        // Nothing fits until both references are there.
        let mut mate = Mating {
            onto: Some(anchor(Snap::FaceAxis)),
            ..Mating::default()
        };
        assert!(mate.fitting().is_empty());
        assert_eq!(mate.clone().settled(false, None), mate);
        // Two axes: what turns and slides about one, and it turns.
        mate.with = Some(anchor(Snap::ArcCentre));
        assert_eq!(mate.fitting(), [Fastened, Revolute, Slider, Cylindrical]);
        let mate = mate.settled(false, None);
        assert_eq!(mate.kind, Revolute);
        // A face against an axis: a planar mate no longer fits, and
        // becomes the likely one.
        let faces = Mating {
            with: Some(anchor(Snap::FaceCentre)),
            kind: Planar,
            ..mate.clone()
        };
        assert_eq!(faces.fitting(), [Fastened, Revolute, Slider]);
        assert_eq!(faces.settled(true, None).kind, Fastened);
        // Two faces just paired face each other; picked again later,
        // they stay as they were set. A seat says itself how it sits.
        let pair = Mating {
            onto: Some(anchor(Snap::FaceCentre)),
            with: Some(anchor(Snap::FaceCentre)),
            ..Mating::default()
        };
        assert!(pair.clone().settled(false, None).flip_with);
        assert!(!pair.clone().settled(true, None).flip_with);
        let seat = Mating {
            with: pair.with.clone(),
            kind: Revolute,
            ..Mating::default()
        };
        let seated = pair.settled(false, Some(&seat));
        assert_eq!((seated.kind, seated.flip_with), (Revolute, false));
        // A vertex is held fastened or as a ball.
        let point = Mating {
            with: Some(anchor(Snap::Vertex)),
            ..mate
        };
        assert_eq!(point.fitting(), [Fastened, Ball]);
        let hold = Hold {
            fixed: false,
            mates: vec![point],
        };
        let fields = hold_fields(&hold, "part", None);
        let Some(Widget::Group(group)) = fields.get(1) else {
            panic!("expected the first mate");
        };
        let Some(Widget::Choice(choice)) = group.children.get(2) else {
            panic!("expected the constraint's choice");
        };
        // (The revolute mate it was still shows, though it fits no more.)
        assert_eq!(choice.options.len(), 3);
        let chosen = choice.set(1).expect("a ball");
        assert_eq!(chosen.mates[0].kind, Ball);
        // Mates are added and taken away.
        let Some(Widget::ToolButton(add)) = fields.get(2) else {
            panic!("expected the button that adds a mate");
        };
        assert_eq!(add.click().map(|h| h.mates.len()), Some(2));
        let Some(Widget::ToolButton(remove)) = group.children.last() else {
            panic!("expected the button that takes it away");
        };
        assert_eq!(remove.click().map(|h| h.mates.len()), Some(0));
    }
}
