//! Split: a body cut in two by a plane, or by faces that go through it.

use anyhow::Context;
use shapecraft_boolean::{Solid, read, read_sheet, stitched};
use shapecraft_core::widgets::ReferenceList;
use shapecraft_core::world::topo::{BodyId, BodyKind, Topo};
use shapecraft_core::world::{DatumPlane, World};
use shapecraft_core::{
    Anything, Builder, EditContext, Operation, Reference, ReferenceKind, Widget,
};
use shapecraft_shape::{halves, halves_by};

/// Which side of the plane is kept.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Keep {
    /// Both: the body becomes two.
    #[default]
    Both,
    /// What is in front of the plane (the way its normal points).
    Front,
    /// What is behind it.
    Behind,
}

impl Keep {
    const ALL: [Keep; 3] = [Keep::Both, Keep::Front, Keep::Behind];

    fn label(self) -> &'static str {
        match self {
            Keep::Both => "Both sides",
            Keep::Front => "In front",
            Keep::Behind => "Behind",
        }
    }
}

/// What is split, by what.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    shapecraft_core::Arguments,
)]
pub struct SplitArgs {
    /// The body split; `None` for every solid body the plane goes
    /// through.
    #[arg(label = "Body", empty = "All bodies")]
    pub target: Option<shapecraft_core::Reference<shapecraft_core::world::topo::BodyId>>,
    /// What it is split by: a datum plane or a flat face (the body is
    /// cut all the way along its plane), or faces of any shape that go
    /// all the way through the body — a surface, or some of its faces
    /// that hang together.
    #[arg(with = "by_field")]
    pub by: Vec<Reference<Anything>>,
    /// Which side is kept.
    #[serde(default)]
    #[arg(choice)]
    pub keep: Keep,
}

fn by_field(args: &SplitArgs, _: &EditContext<'_>) -> Vec<Widget<SplitArgs>> {
    let kinds = vec![
        ReferenceKind::Plane,
        ReferenceKind::Face,
        ReferenceKind::Body,
    ];
    let set = shapecraft_shape::ui::set(args, |a: &mut SplitArgs, by| {
        a.by = Reference::each(by);
    });
    vec![
        ReferenceList::new("Split by", kinds, Reference::sources(&args.by), set)
            .required()
            .into(),
    ]
}

/// What a body is split by.
enum Cutter {
    /// A plane: through a point, across a normal.
    Plane(DatumPlane),
    /// Faces, as a sheet.
    Faces(Solid),
}

/// The surface called `name`, if there is one.
fn sheet(topo: &Topo, name: &str) -> Option<BodyId> {
    let body = topo.resolve::<BodyId>(name)?;
    (topo.body(body)?.kind == BodyKind::Sheet).then_some(body)
}

/// What `by` says to split by, in `world`.
fn cutter(world: &World, by: &[Reference<Anything>]) -> anyhow::Result<Cutter> {
    anyhow::ensure!(!by.is_empty(), "pick the plane or the face to split by");
    // One flat thing is its plane, however far the thing itself goes.
    if let [only] = by
        && let Ok(plane) = world.resolve(&Reference::<DatumPlane>::new(only.source.clone()))
        && sheet(&world.topo, only.name()).is_none()
    {
        return Ok(Cutter::Plane(plane));
    }
    let mut sheets = Vec::new();
    for each in by {
        let name = each.name();
        if let Some(body) = sheet(&world.topo, name) {
            sheets.push(read_sheet(&world.topo, body)?);
            continue;
        }
        let (of, face) = shapecraft_shape::on::face(&world.topo, name)
            .with_context(|| format!("{name} is neither a plane nor a face"))?;
        sheets.push(shapecraft_shape::faces_of(&of, &[face]));
    }
    if let [only] = &sheets[..] {
        return Ok(Cutter::Faces(only.clone()));
    }
    let reach = sheets
        .iter()
        .map(|s| {
            let (lo, hi) = s.extent();
            (hi - lo).length()
        })
        .fold(0.0, f64::max);
    let (all, _) = stitched(&sheets, reach * 1e-7)?;
    Ok(Cutter::Faces(all))
}

/// Cuts bodies in two by a plane or by faces. What is in front keeps
/// the body's name; what is behind becomes a body named after the
/// operation's id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Split {
    /// The operation's id; also the name of the body behind the plane.
    pub id: String,
    /// What is split, by what.
    pub args: SplitArgs,
}

impl Operation for Split {
    type Args = SplitArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &SplitArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut SplitArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = shapecraft_shape::bodies::Out::default();
        let args = &self.args;
        let cutter = cutter(builder.tracked(), &args.by)?;
        let topo = &builder.tracked().topo;
        let targets = shapecraft_shape::bodies::targets(topo, &self.id, &args.target)?;
        let mut split = 0;
        for (body, name) in targets {
            let solid = read(topo, body)?;
            let [front, behind] = match &cutter {
                Cutter::Plane(plane) => halves(&solid, plane.origin(), plane.normal()),
                Cutter::Faces(sheet) => halves_by(&solid, sheet),
            }
            .with_context(|| format!("splitting {name}"))?;
            if front.is_empty() || behind.is_empty() {
                continue;
            }
            out.remove(topo, body);
            let other = if split == 0 {
                self.id.clone()
            } else {
                format!("{}/b{split}", self.id)
            };
            match args.keep {
                Keep::Both => {
                    out.write(&front, &name);
                    out.write(&behind, &other);
                }
                Keep::Front => {
                    out.write(&front, &name);
                }
                Keep::Behind => {
                    out.write(&behind, &name);
                }
            }
            split += 1;
        }
        anyhow::ensure!(split > 0, "what it is split by goes through no body");
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let what = self.args.target.as_ref().map_or("every body", |r| r.name());
        format!("{what}, keeping {}", self.args.keep.label().to_lowercase())
    }
}

impl shapecraft_core::Choices for Keep {
    const ALL: &'static [Self] = &Self::ALL;

    fn label(self) -> &'static str {
        Self::label(self)
    }
}
