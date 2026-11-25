//! The [`Mechanism`] operation: two joints whose travels go together.

use workpiece_core::widgets::{Choice, Label, Mood, NumericInput};
use workpiece_core::world::{Coupling, CouplingKind};
use workpiece_core::{Builder, EditContext, Operation};

/// The arguments of [`Mechanism`].
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct MechanismArgs {
    /// How the second joint follows the first.
    #[arg(with = "fields_of")]
    pub kind: CouplingKind,
    /// The first joint: the name of the step that made it (a placed
    /// part's, for the joint it was added with).
    pub a: Option<String>,
    /// The second.
    pub b: Option<String>,
}

impl Default for MechanismArgs {
    fn default() -> Self {
        Self {
            kind: CouplingKind::Gear { ratio: -1.0 },
            a: None,
            b: None,
        }
    }
}

/// A mechanism between two joints of the assembly: gears that turn at a
/// fixed ratio, a pinion on a rack, a screw in its nut, two slides that
/// travel together. The joints say how each part may move; the mechanism
/// takes away one of those freedoms by tying two travels to each other.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Mechanism {
    /// The step's id: the mechanism's name.
    pub id: String,
    /// The arguments.
    pub args: MechanismArgs,
}

/// The kinds, as they are offered: each with what it starts as.
const KINDS: [CouplingKind; 4] = [
    CouplingKind::Gear { ratio: -1.0 },
    CouplingKind::RackAndPinion { radius: 1.0 },
    CouplingKind::Screw { lead: 1.0 },
    CouplingKind::Linear { ratio: 1.0 },
];

impl Mechanism {
    /// A mechanism called `id`, its joints still to choose.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            args: MechanismArgs::default(),
        }
    }
}

impl Operation for Mechanism {
    type Args = MechanismArgs;
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

    fn args(&self) -> &MechanismArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut MechanismArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let (Some(a), Some(b)) = (&self.args.a, &self.args.b) else {
            anyhow::bail!("choose the two joints the mechanism is between");
        };
        let (wants_a, wants_b) = self.args.kind.turns();
        for (name, turns) in [(a, wants_a), (b, wants_b)] {
            let joint = builder
                .joint(name)
                .ok_or_else(|| anyhow::anyhow!("there is no joint {name}"))?;
            let can = if turns {
                joint.kind.turns()
            } else {
                joint.kind.slides()
            };
            anyhow::ensure!(
                can,
                "the joint {name} does not {}: a {} needs one that does",
                if turns { "turn" } else { "slide" },
                self.args.kind.label().to_lowercase()
            );
        }
        anyhow::ensure!(
            a != b || matches!(self.args.kind, CouplingKind::Screw { .. }),
            "a mechanism is between two joints (only a screw is one joint's turning and sliding)"
        );
        Ok(workpiece_core::Made {
            couplings: vec![Coupling {
                name: self.id.clone(),
                kind: self.args.kind,
                a: a.clone(),
                b: b.clone(),
            }],
            ..Default::default()
        })
    }

    fn summary(&self) -> String {
        let op = self;
        let joint = |j: &Option<String>| j.clone().unwrap_or_else(|| "…".to_owned());
        format!(
            "{} of {} and {}",
            op.args.kind.label().to_lowercase(),
            joint(&op.args.a),
            joint(&op.args.b)
        )
    }
}

/// The fields of a mechanism: what it is, the two joints it ties (of
/// those that can be its first, and its second), its ratio, and what
/// that means.
fn fields_of(
    args: &MechanismArgs,
    context: &EditContext<'_>,
) -> Vec<workpiece_core::Widget<MechanismArgs>> {
    let args = args.clone();
    let same = |k: &CouplingKind| std::mem::discriminant(k) == std::mem::discriminant(&args.kind);
    let mut fields = vec![
        Choice::new(
            "Mechanism",
            KINDS.iter().map(|k| k.label().to_owned()).collect(),
            KINDS.iter().position(same).unwrap_or(0),
            {
                let args = args.clone();
                move |i| MechanismArgs {
                    kind: KINDS[i],
                    ..args.clone()
                }
            },
        )
        .with_icons(
            ["mech-gear", "mech-rack", "mech-screw", "mech-linear"]
                .map(Some)
                .to_vec(),
        )
        .into(),
    ];
    // The joints of the assembly so far that can be its first, and
    // its second.
    let (wants_a, wants_b) = args.kind.turns();
    let joints = |turns: bool| -> Vec<String> {
        context
            .world
            .joints
            .iter()
            .filter(|j| {
                if turns {
                    j.kind.turns()
                } else {
                    j.kind.slides()
                }
            })
            .map(|j| j.name.clone())
            .collect()
    };
    let what = |turns: bool| if turns { "turns" } else { "slides" };
    for (first, label, turns) in [
        (true, "First joint", wants_a),
        (false, "Second joint", wants_b),
    ] {
        let chosen = if first { &args.a } else { &args.b };
        let mut options = vec![format!("Choose a joint that {}…", what(turns))];
        let names = joints(turns);
        options.extend(names.iter().cloned());
        let selected = chosen
            .as_ref()
            .and_then(|c| names.iter().position(|n| n == c))
            .map_or(0, |i| i + 1);
        let with = {
            let args = args.clone();
            move |i: usize| {
                let joint = (i > 0).then(|| names[i - 1].clone());
                let mut args = args.clone();
                if first {
                    args.a = joint;
                } else {
                    args.b = joint;
                }
                args
            }
        };
        fields.push(Choice::new(label, options, selected, with).into());
    }
    let number = |label: &str, value: f64, kind: fn(f64) -> CouplingKind| {
        let args = args.clone();
        NumericInput::new(label, value, move |x| MechanismArgs {
            kind: kind(x),
            ..args.clone()
        })
        .into()
    };
    fields.push(match args.kind {
        CouplingKind::Gear { ratio } => {
            number("Ratio", ratio, |ratio| CouplingKind::Gear { ratio })
        }
        CouplingKind::RackAndPinion { radius } => number("Pitch radius", radius, |radius| {
            CouplingKind::RackAndPinion { radius }
        }),
        CouplingKind::Screw { lead } => number("Lead", lead, |lead| CouplingKind::Screw { lead }),
        CouplingKind::Linear { ratio } => {
            number("Ratio", ratio, |ratio| CouplingKind::Linear { ratio })
        }
    });
    let says = match args.kind {
        CouplingKind::Gear { .. } => {
            "The second joint turns this many times for each turn of the first; against it, \
             for a negative ratio, as two gears in mesh do."
        }
        CouplingKind::RackAndPinion { .. } => {
            "The second joint slides this far for every radian the first turns."
        }
        CouplingKind::Screw { .. } => {
            "The second joint slides this far for every turn of the first. Both may be one \
             cylindrical joint: a screw in its nut."
        }
        CouplingKind::Linear { .. } => {
            "The second joint slides this many times as far as the first."
        }
    };
    fields.push(Label::new("", says, Mood::Plain).into());
    fields
}
