//! The [`Mate`] operation: a joint between two connectors.

use anyhow::Context;
use glam::DQuat;
use shapecraft_core::widgets::{
    Choice, ConnectorInput, HBox, Label, Mood, NumericInput, Picked, Scope,
};
use shapecraft_core::world::{Connector, Joint, JointKind, Limits, World};
use shapecraft_core::{Builder, EditContext, Operation, Pose, Source};

use crate::connector::{self, Snap};

/// A connector as a step keeps it: what it is on, and how it sits there.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Anchor {
    /// The placed parts it is in, from this part down; empty for
    /// something of this part itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance: Vec<String>,
    /// What it is on, in the last of those parts.
    pub source: Source,
    /// How it sits there.
    pub snap: Snap,
}

impl Anchor {
    /// The world it is in, and where that world is in the first of its
    /// instances.
    fn world<'a>(&self, world: &'a World) -> anyhow::Result<(&'a World, Pose)> {
        let mut within = world;
        let mut pose = Pose::IDENTITY;
        for (depth, name) in self.instance.iter().enumerate() {
            let instance = within
                .instances
                .get(name)
                .with_context(|| format!("no placed part {name}"))?;
            if depth > 0 {
                pose = pose * instance.pose;
            }
            within = &instance.world;
        }
        Ok((within, pose))
    }

    /// The connector it is, in `world`.
    pub fn connector(&self, world: &World) -> anyhow::Result<Connector> {
        let (within, pose) = self.world(world)?;
        Ok(Connector {
            instance: self.instance.first().cloned(),
            frame: pose * connector::frame(within, &self.source, &self.snap)?,
        })
    }

    /// What it is on, in words.
    pub fn describe(&self) -> String {
        let on = self.source.name();
        let how = match &self.snap {
            Snap::Vertex | Snap::Datum => "",
            Snap::EdgeMiddle => " (middle)",
            Snap::EdgeEnd { .. } => " (end)",
            Snap::ArcCentre => " (centre)",
            Snap::FaceCentre => " (middle)",
            Snap::FaceAxis => " (axis)",
            Snap::FaceEdge { .. } => " (at an edge)",
            Snap::FaceVertex { .. } => " (at a corner)",
        };
        match self.instance.last() {
            Some(part) => format!("{part}: {on}{how}"),
            None => format!("{on}{how}"),
        }
    }

    /// The connector the pointer offers on what was picked.
    pub fn picked(picked: &Picked) -> Option<Anchor> {
        let snap = connector::infer(&picked.within, &picked.source, picked.at, picked.near)?;
        Some(Anchor {
            instance: picked.instance.clone(),
            // A frame's axis stands for the frame.
            source: match &picked.source {
                Source::FrameAxis { frame, .. } => Source::Datum(frame.clone()),
                source => source.clone(),
            },
            snap,
        })
    }

    /// The same connector, told from inside the first of its instances:
    /// a connector on a placed part, as that part's own step keeps it.
    #[must_use]
    pub fn inside(mut self) -> Anchor {
        if !self.instance.is_empty() {
            self.instance.remove(0);
        }
        self
    }

    /// Its frame in the coordinates of `world`, the world its instances
    /// are told from.
    pub fn frame_in(&self, world: &World) -> anyhow::Result<Pose> {
        let mut within = world;
        let mut pose = Pose::IDENTITY;
        for name in &self.instance {
            let instance = within
                .instances
                .get(name)
                .with_context(|| format!("no placed part {name}"))?;
            pose = pose * instance.pose;
            within = &instance.world;
        }
        Ok(pose * connector::frame(within, &self.source, &self.snap)?)
    }
}

/// The arguments of [`Mate`].
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    shapecraft_core::Arguments,
)]
pub struct MateArgs {
    /// How the second connector may move against the first.
    #[arg(choice, label = "Joint")]
    pub kind: JointKind,
    /// The first connector.
    #[arg(with = "connector_fields")]
    pub a: Option<Anchor>,
    /// The second: the part that comes to the first when the joint is
    /// made.
    pub b: Option<Anchor>,
    /// Whether the second connector faces the first: its z against the
    /// first's.
    #[serde(default)]
    pub flip: bool,
    /// The least and greatest angle around the axis, in degrees.
    #[serde(default)]
    pub angle: Option<[f64; 2]>,
    /// The least and greatest distance along the axis.
    #[serde(default)]
    pub distance: Option<[f64; 2]>,
}

/// A joint between two connectors on placed parts. Where the parts end
/// up is worked out after the part's last step, with all its joints.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Mate {
    /// The step's id: the joint's name.
    pub id: String,
    /// The arguments.
    pub args: MateArgs,
}

impl Mate {
    /// A joint of `kind` called `id`, its connectors still to pick.
    pub fn of(id: impl Into<String>, kind: JointKind) -> Self {
        Self {
            id: id.into(),
            args: MateArgs {
                kind,
                ..MateArgs::default()
            },
        }
    }

    /// The joint it makes in `world`.
    pub fn joint(&self, world: &World) -> anyhow::Result<Joint> {
        let a = self.args.a.as_ref().context("pick the first connector")?;
        let b = self.args.b.as_ref().context("pick the second connector")?;
        let (a, mut b) = (a.connector(world)?, b.connector(world)?);
        anyhow::ensure!(
            a.instance != b.instance,
            "a joint connects two parts, not a part to itself"
        );
        b.frame = self.args.facing(b.frame);
        Ok(Joint {
            name: self.id.clone(),
            kind: self.args.kind,
            a,
            b,
            limits: self.args.limits(),
        })
    }
}

impl Operation for Mate {
    type Args = MateArgs;
    type EditState = ();

    fn settles(&self) -> bool {
        true
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &MateArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut MateArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        Ok(shapecraft_core::Made {
            joints: vec![{
                // (It reads the parts its connectors are on.)
                let on = [&self.args.a, &self.args.b];
                let on = on.into_iter().flatten().filter_map(|a| a.instance.first());
                self.joint(builder.with_placed(on.map(String::as_str)))?
            }],
            ..Default::default()
        })
    }

    fn references(&self) -> Vec<Source> {
        // What a connector is on inside a placed part is not made by a
        // step of this part; what is on this part itself is.
        [&self.args.a, &self.args.b]
            .into_iter()
            .flatten()
            .filter(|a| a.instance.is_empty())
            .map(|a| a.source.clone())
            .collect()
    }

    fn references_mut(&mut self) -> Vec<&mut Source> {
        [&mut self.args.a, &mut self.args.b]
            .into_iter()
            .flatten()
            .filter(|a| a.instance.is_empty())
            .map(|a| &mut a.source)
            .collect()
    }

    fn summary(&self) -> String {
        let op = self;
        let on = |a: &Option<crate::Anchor>| {
            a.as_ref()
                .and_then(|a| a.instance.first().cloned())
                .unwrap_or_else(|| "…".to_owned())
        };
        format!(
            "{} joint of {} and {}",
            op.args.kind.label().to_lowercase(),
            on(&op.args.a),
            on(&op.args.b)
        )
    }
}

/// The fields of how a joint moves, after its connectors: which way
/// they face, and the limits its kind can have.
pub fn motion_fields(args: &MateArgs) -> Vec<shapecraft_core::Widget<MateArgs>> {
    let mut fields: Vec<shapecraft_core::Widget<MateArgs>> = vec![
        Choice::new(
            "Facing",
            vec!["The same way".to_owned(), "Each other".to_owned()],
            usize::from(args.flip),
            {
                let args = args.clone();
                move |i| MateArgs {
                    flip: i == 1,
                    ..args.clone()
                }
            },
        )
        .into(),
    ];
    // The limits the joint's kind can have.
    let limit = |label: &str,
                 unit: [f64; 2],
                 value: Option<[f64; 2]>,
                 set: fn(&mut MateArgs, Option<[f64; 2]>)| {
        let with = {
            let args = args.clone();
            move |limits: Option<[f64; 2]>| {
                let mut args = args.clone();
                set(&mut args, limits);
                args
            }
        };
        let mut row = vec![
            Choice::new(
                label,
                vec!["Free".to_owned(), "Limited".to_owned()],
                usize::from(value.is_some()),
                {
                    let with = with.clone();
                    move |i| with((i == 1).then_some(value.unwrap_or(unit)))
                },
            )
            .into(),
        ];
        if let Some([lo, hi]) = value {
            let (least, most) = (with.clone(), with);
            row.push(NumericInput::new("from", lo, move |x| least(Some([x, hi]))).into());
            row.push(NumericInput::new("to", hi, move |x| most(Some([lo, x]))).into());
        }
        HBox::new(row).into()
    };
    if args.kind.turns() {
        fields.push(limit("Angle", [-90.0, 90.0], args.angle, |a, l| {
            a.angle = l;
        }));
    }
    if args.kind.slides() {
        fields.push(limit("Travel", [0.0, 10.0], args.distance, |a, l| {
            a.distance = l;
        }));
    }
    fields
}

/// The fields of a joint, after its kind: its two connectors (called
/// `labels`, each taking what its `scopes` say; the second is the part
/// that moves, and what is picked for it is kept as told from inside
/// that part where its scope is one part), which way they face, and the
/// limits the kind can have.
pub fn joint_fields(
    args: &MateArgs,
    labels: [&str; 2],
    scopes: [Scope; 2],
) -> Vec<shapecraft_core::Widget<MateArgs>> {
    let pick = |first: bool, inside: bool| {
        let args = args.clone();
        move |picked: Picked| {
            let anchor = Anchor::picked(&picked).map(|a| if inside { a.inside() } else { a });
            let mut args = args.clone();
            if first {
                args.a = anchor.or(args.a);
            } else {
                args.b = anchor.or(args.b);
            }
            args
        }
    };
    let said = |anchor: &Option<Anchor>| anchor.as_ref().map(Anchor::describe);
    let inside = |scope: &Scope| matches!(scope, Scope::Inside(_));
    let [first, second] = scopes;
    let mut fields: Vec<shapecraft_core::Widget<MateArgs>> = vec![
        ConnectorInput::new(labels[0], said(&args.a), pick(true, inside(&first)))
            .within(first)
            .into(),
        ConnectorInput::new(labels[1], said(&args.b), pick(false, inside(&second)))
            .within(second)
            .into(),
    ];
    fields.extend(motion_fields(args));
    if args.a.is_none() || args.b.is_none() {
        fields.push(
            Label::new(
                "",
                "Point at a vertex, an edge or a face: the joint is made where the pointer \
                 shows its frame.",
                Mood::Plain,
            )
            .into(),
        );
    }
    fields
}

impl MateArgs {
    /// The limits, as a joint has them.
    pub fn limits(&self) -> Limits {
        let ordered = |[lo, hi]: [f64; 2]| (lo.min(hi), lo.max(hi));
        Limits {
            angle: self.angle.map(|l| {
                let (lo, hi) = ordered(l);
                (lo.to_radians(), hi.to_radians())
            }),
            distance: self.distance.map(ordered),
        }
    }

    /// The second connector's frame `frame`, turned to face the first
    /// if that is asked for.
    pub fn facing(&self, frame: Pose) -> Pose {
        if self.flip {
            frame * Pose::from_rotation(DQuat::from_rotation_x(std::f64::consts::PI))
        } else {
            frame
        }
    }
}

/// The fields of its two connectors, and of what the joint lets go.
fn connector_fields(
    args: &MateArgs,
    _: &EditContext<'_>,
) -> Vec<shapecraft_core::Widget<MateArgs>> {
    joint_fields(args, ["First", "Second"], [Scope::Any, Scope::Any])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly;
    use shapecraft_core::math::Point3;
    use shapecraft_core::world::{Datum, DatumFrame, Instance};
    use shapecraft_core::{Axis, Standalone};
    use std::sync::Arc;

    /// A part with a frame "peg" at `at`.
    fn part(at: Point3) -> anyhow::Result<Arc<World>> {
        let mut world = World::new();
        world.datums.insert(
            "peg".into(),
            Datum::Frame(DatumFrame::from_pose(Pose::from_translation(at))?),
        );
        Ok(Arc::new(world))
    }

    fn anchor(instance: &str) -> Anchor {
        Anchor {
            instance: vec![instance.into()],
            source: Source::Datum("peg".into()),
            snap: Snap::Datum,
        }
    }

    #[test]
    fn a_mate_makes_a_joint_that_brings_the_parts_together() -> anyhow::Result<()> {
        let mut b = Standalone::new();
        for (name, at, pose, fixed) in [
            ("base", Point3::new(0.0, 0.0, 2.0), Pose::IDENTITY, true),
            (
                "lid",
                Point3::new(1.0, 0.0, 0.0),
                Pose::from_translation(Point3::new(7.0, 8.0, 9.0)),
                false,
            ),
        ] {
            b.world.instances.insert(
                name.into(),
                Instance {
                    pose,
                    world: part(at)?,
                    fixed,
                },
            );
        }
        let mut mate = Mate::of("hinge", JointKind::Revolute);
        assert!(mate.apply(&mut b).is_err(), "no connectors yet");
        mate.args.a = Some(anchor("base"));
        mate.args.b = Some(anchor("lid"));
        mate.args.angle = Some([-30.0, 30.0]);
        mate.apply(&mut b)?;
        assert_eq!(b.world.joints.len(), 1);
        assert_eq!(
            b.world.joints[0].limits.angle,
            Some((-(30.0_f64).to_radians(), 30.0_f64.to_radians()))
        );

        assembly::settle(&mut b.world, &mut b.cache);
        // The lid's peg is on the base's, its z along the base's.
        let lid = b.world.instances["lid"].pose;
        let peg = lid.transform_point(Point3::new(1.0, 0.0, 0.0));
        assert!(peg.abs_diff_eq(Point3::new(0.0, 0.0, 2.0), 1e-7), "{peg}");
        assert!(lid.axis(Axis::Z).abs_diff_eq(Point3::Z, 1e-7));
        // Where it ended up is kept, and found again.
        assert!(b.cache.get("lid/pose").is_some());
        assert!(b.cache.get("hinge/made").is_some());
        // Driven, the hinge is held where it is told to be: 20° one way
        // and 20° the other are 40° apart. Past its limits it is not
        // driven.
        let mut drive = crate::Drive::new("drive");
        drive.args.joint = Some("hinge".into());
        drive.args.at = shapecraft_core::Measure::new(45.0);
        assert!(drive.apply(&mut b).is_err(), "past the limits");
        drive.args.at = shapecraft_core::Measure::new(20.0);
        drive.apply(&mut b)?;
        assembly::settle(&mut b.world, &mut b.cache);
        let one = b.world.instances["lid"].pose.axis(Axis::X);
        let back = (-20.0_f64).to_radians();
        b.world.joints[0].limits.angle = Some((back, back));
        assembly::settle(&mut b.world, &mut b.cache);
        let other = b.world.instances["lid"].pose.axis(Axis::X);
        let between = one.dot(other).clamp(-1.0, 1.0).acos().to_degrees();
        assert!((between - 40.0).abs() < 1e-5, "{between}");
        Ok(())
    }
}
