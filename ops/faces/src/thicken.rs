//! Thicken: faces or a whole surface given a thickness, into a solid.

use anyhow::{Context, ensure};
use shapecraft_boolean::{Op, Solid, combine, read, read_sheet};
use shapecraft_core::world::topo::BodyKind;
use shapecraft_core::{Builder, Measure, Operation};

use shapecraft_shape::bodies::{self, Combine};

/// Which side of the faces the thickness goes to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ThickenSide {
    /// The side the faces' normals point to (out of a solid).
    #[default]
    Front,
    /// The other side.
    Back,
    /// Both: the thickness to each.
    Both,
}

impl ThickenSide {
    const ALL: [ThickenSide; 3] = [ThickenSide::Front, ThickenSide::Back, ThickenSide::Both];

    fn label(self) -> &'static str {
        match self {
            ThickenSide::Front => "Front",
            ThickenSide::Back => "Back",
            ThickenSide::Both => "Both sides",
        }
    }
}

/// What is thickened, by how much, and what it becomes.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct ThickenArgs {
    /// The faces thickened: faces of any body, or whole surface bodies
    /// (which are used up).
    #[arg(required)]
    pub faces: Vec<shapecraft_core::Reference<shapecraft_core::FaceOrBody>>,
    /// How thick.
    #[arg(label = "Thickness")]
    pub distance: Measure,
    /// To which side.
    #[serde(default)]
    #[arg(choice)]
    pub side: ThickenSide,
    /// Whether, where two faces part as they are thickened, they run on
    /// to a sharp corner (else the gap is rounded).
    #[serde(default)]
    #[arg(label = "Corners", options = "Rounded|Sharp")]
    pub sharp: bool,
    /// What the solid does to the bodies there are.
    #[serde(default)]
    #[arg(choice, label = "Result")]
    pub combine: Combine,
    /// The body it is combined with; `None` for every solid body.
    #[serde(default)]
    #[arg(
        label = "With",
        empty = "All bodies",
        when = "self.combine != Combine::NewBody"
    )]
    pub target: Option<shapecraft_core::Reference<shapecraft_core::world::topo::BodyId>>,
}

impl Default for ThickenArgs {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            distance: Measure::new(1.0),
            side: ThickenSide::Front,
            sharp: false,
            combine: Combine::NewBody,
            target: None,
        }
    }
}

/// Gives faces a thickness: the solid of everything within the distance
/// of them, on the side picked. Named after the operation's id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Thicken {
    /// The operation's id; also the name of the body it makes.
    pub id: String,
    /// What is thickened and how.
    pub args: ThickenArgs,
}

impl Operation for Thicken {
    type Args = ThickenArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &ThickenArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut ThickenArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = shapecraft_shape::bodies::Out::default();
        let args = &self.args;
        let distance = (*builder).measure(&args.distance, "the thickness")?;
        ensure!(distance > 0.0, "a solid has some thickness");
        ensure!(
            !args.faces.is_empty(),
            "pick the faces or the surface to thicken"
        );
        let (distance, both) = match args.side {
            ThickenSide::Front => (distance, false),
            ThickenSide::Back => (-distance, false),
            ThickenSide::Both => (distance, true),
        };
        let picked: Vec<&str> = args.faces.iter().map(|r| r.name()).collect();
        let topo = &builder.tracked().topo;
        let mut found = vec![false; picked.len()];
        let mut whole: Option<Solid> = None;
        let mut used_up = Vec::new();
        // (Only the bodies that what was picked is of are read.)
        let mut owners: Vec<_> = picked
            .iter()
            .filter_map(|p| topo.owner(topo.referred(p)?))
            .collect();
        owners.sort();
        owners.dedup();
        let all: Vec<_> = owners
            .into_iter()
            .filter_map(|id| Some((id, topo.body(id)?.kind, topo.name_of(id)?.to_owned())))
            .collect();
        for (body, kind, name) in all {
            let it = match kind {
                BodyKind::Solid => read(topo, body),
                BodyKind::Sheet => read_sheet(topo, body),
                _ => continue,
            };
            let Ok(it) = it else {
                continue;
            };
            let all_of_it = picked.iter().position(|p| *p == name);
            let faces: Vec<usize> = (0..it.faces.len())
                .filter(|&f| {
                    let named = it.faces[f].name.as_deref();
                    let also = it.faces[f].also.iter().map(String::as_str);
                    let hit = named
                        .into_iter()
                        .chain(also)
                        .find_map(|n| picked.iter().position(|p| *p == n));
                    if let Some(k) = hit {
                        found[k] = true;
                    }
                    hit.is_some() || all_of_it.is_some()
                })
                .collect();
            if let Some(k) = all_of_it {
                found[k] = true;
                if kind == BodyKind::Sheet {
                    used_up.push(body);
                }
            }
            if faces.is_empty() {
                continue;
            }
            if out.made.annotations.is_empty() {
                let thick = distance.abs();
                out.made.annotations.push(shapecraft_shape::on::held(
                    &self.id,
                    "distance",
                    (&it, faces[0]),
                    (distance, None),
                    &args.distance,
                    thick,
                ));
            }
            let made = shapecraft_shape::thick::thicken(&it, &faces, distance, both, args.sharp)
                .with_context(|| format!("thickening {name}"))?;
            whole = Some(match whole {
                None => made,
                Some(sofar) => combine(Op::Union, &sofar, &made)?.solid,
            });
        }
        if let Some(k) = found.iter().position(|f| !f) {
            anyhow::bail!("there is no face or surface {:?} to thicken", picked[k]);
        }
        let tool = whole.context("nothing to thicken")?;
        for body in used_up {
            out.remove(topo, body);
        }
        let targets = bodies::targets(topo, &self.id, &args.target)?;
        bodies::settle_into(topo, &mut out, &self.id, tool, args.combine, &targets)?;
        Ok(out.made)
    }

    fn summary(&self) -> String {
        format!(
            "{} thick, {}",
            self.args.distance,
            self.args.side.label().to_lowercase()
        )
    }
}

impl shapecraft_core::Choices for ThickenSide {
    const ALL: &'static [Self] = &Self::ALL;

    fn label(self) -> &'static str {
        Self::label(self)
    }
}
