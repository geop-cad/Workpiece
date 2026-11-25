//! Mirror: features — what steps did to a body: what an extrusion
//! added, a hole took away, a fillet rounded — or whole bodies, done
//! again as their mirror image in a plane.

use anyhow::{Context, ensure};
use workpiece_boolean::{Op, combine, read};
use workpiece_core::math::Point3;
use workpiece_core::world::DatumPlane;
use workpiece_core::world::topo::{BodyId, FaceId};
use workpiece_core::{Builder, Operation, Reference};
use workpiece_shape::bodies::Combine;

use crate::features::{Change, Features, Move, steps_of};
use crate::pattern::Repeat;

/// What becomes of the mirror image of a body: a body of its own, or
/// one with the body.
const MIRRORS_BECOME: [Combine; 2] = [Combine::NewBody, Combine::Join];

/// What is mirrored, and the plane it is mirrored in.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    workpiece_core::Arguments,
)]
#[serde(default)]
pub struct MirrorArgs {
    /// Whether features or whole bodies are mirrored.
    #[arg(choice, label = "", group = "Mirror")]
    pub of: Repeat,
    /// Faces of the features that are mirrored: one of each is enough.
    /// Features with others between them are mirrored with those.
    #[arg(
        required,
        label = "Faces of the features",
        when = "self.of == Repeat::Features",
        group = "Mirror"
    )]
    pub features: Vec<Reference<FaceId>>,
    /// The bodies that are mirrored.
    #[arg(
        required,
        label = "Bodies",
        when = "self.of == Repeat::Bodies",
        group = "Mirror"
    )]
    pub bodies: Vec<Reference<BodyId>>,
    /// The plane they are mirrored in: a datum plane or a flat face.
    #[arg(label = "In", group = "Mirror")]
    pub plane: Option<Reference<DatumPlane>>,
    /// Of bodies: whether each mirror image is a body of its own or one
    /// with the body.
    #[arg(
        label = "",
        among = "MIRRORS_BECOME",
        when = "self.of == Repeat::Bodies",
        group = "Mirror"
    )]
    pub combine: Combine,
}

/// Mirrors features or bodies in a plane. Features: what the steps that
/// made the picked faces (and the steps between them) added to the
/// bodies and took from them is added and taken again as the mirror
/// image; what they make there is named under `{id}/`. Bodies: the
/// mirror image of each, a body of its own (`{id}`, or `{id}/1`, `{id}/2`
/// … of several) or joined with it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Mirror {
    /// The operation's id.
    pub id: String,
    /// What is mirrored, and the plane.
    pub args: MirrorArgs,
}

impl Mirror {
    /// The features mirrored: see [`Mirror`].
    fn features(
        &self,
        builder: &dyn Builder,
        reflect: &dyn Fn(Point3) -> Point3,
        out: &mut workpiece_shape::bodies::Out,
    ) -> anyhow::Result<()> {
        let args = &self.args;
        ensure!(
            !args.features.is_empty(),
            "pick a face of each feature to mirror"
        );
        let features = Features::of(builder, &args.features)?;
        let topo = &builder.tracked().topo;
        let mut done = false;
        for Change {
            name,
            existed,
            added,
            taken,
        } in &features.changes
        {
            let Some(body) = crate::arrange::named_solid(topo, name) else {
                continue;
            };
            // The steps themselves done again on the body's mirror image
            // (in the world they were done in, so their planes, senses
            // and directions are untouched), the outcome mirrored back:
            // a cut then takes what its tool meets there. Steps that
            // place themselves by the body's own faces do the same
            // again on the mirror image: of those, what they added and
            // took is mirrored instead.
            if *existed {
                let by = Move {
                    back: reflect,
                    there: reflect,
                    mirrored: true,
                };
                let again = features
                    .redone(builder, name, &read(topo, body)?, &by, &self.id)
                    .with_context(|| format!("doing the features mirrored on {name}"))?;
                if let Some(again) = again {
                    out.remove(topo, body);
                    out.write(&again, name);
                    done = true;
                    continue;
                }
            }
            let mut result = read(topo, body)?;
            for (op, piece, what) in [
                (Op::Union, added, "adding"),
                (Op::Cut, taken, "taking away"),
            ] {
                if piece.is_empty() {
                    continue;
                }
                let copy = piece.mapped(reflect, true, 1.0)?;
                let copy = crate::arrange::instance(copy, &self.id);
                result = combine(op, &result, &copy)
                    .with_context(|| format!("{what} the mirror image at {name}"))?
                    .solid;
            }
            out.remove(topo, body);
            if !result.is_empty() {
                out.write(&result, name);
            }
            done = true;
        }
        ensure!(
            done,
            "the features picked changed no body: nothing to mirror"
        );
        // (The holes mirrored: their callouts say twice as many.)
        out.made.annotations = crate::features::callouts_times(builder, &args.features, 2);
        Ok(())
    }

    /// The bodies mirrored: see [`Mirror`].
    fn bodies(
        &self,
        builder: &dyn Builder,
        reflect: &dyn Fn(Point3) -> Point3,
        out: &mut workpiece_shape::bodies::Out,
    ) -> anyhow::Result<()> {
        let args = &self.args;
        ensure!(!args.bodies.is_empty(), "pick the bodies to mirror");
        ensure!(
            MIRRORS_BECOME.contains(&args.combine),
            "the mirror image of a body is a body of its own or joined with it"
        );
        let topo = &builder.tracked().topo;
        let several = args.bodies.len() > 1;
        for (k, body) in args.bodies.iter().enumerate() {
            let name = body.name();
            let id = topo
                .resolve::<BodyId>(name)
                .with_context(|| format!("there is no body {name:?}"))?;
            let sheet =
                topo.body(id).map(|b| b.kind) == Some(workpiece_core::world::topo::BodyKind::Sheet);
            let solid = if sheet {
                workpiece_boolean::read_sheet(topo, id)?
            } else {
                read(topo, id)?
            };
            let called = if several {
                format!("{}/{}", self.id, k + 1)
            } else {
                self.id.clone()
            };
            let image = crate::arrange::instance(solid.mapped(reflect, true, 1.0)?, &called);
            match args.combine {
                Combine::Join if !sheet => {
                    let joined = combine(Op::Union, &solid, &image)
                        .with_context(|| format!("joining {name} and its mirror image"))?
                        .solid;
                    out.remove(topo, id);
                    out.write(&joined, name);
                }
                _ if sheet => out.write_sheet(&image, &called),
                _ => out.write(&image, &called),
            }
        }
        Ok(())
    }
}

impl Operation for Mirror {
    type Args = MirrorArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &MirrorArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut MirrorArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let mut out = workpiece_shape::bodies::Out::default();
        let plane = self
            .args
            .plane
            .as_ref()
            .context("pick a plane to mirror in")?;
        let plane = builder.tracked().resolve(plane)?;
        let (o, n) = (plane.origin(), plane.normal());
        let reflect = move |p: Point3| p - n * (2.0 * (p - o).dot(n));
        match self.args.of {
            Repeat::Features => self.features(builder, &reflect, &mut out)?,
            Repeat::Bodies => self.bodies(builder, &reflect, &mut out)?,
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let args = &self.args;
        match args.of {
            Repeat::Features => {
                let steps = steps_of(&args.features);
                let of = if steps.is_empty() {
                    "(nothing picked)".to_owned()
                } else {
                    steps.join(", ")
                };
                format!("{of} mirrored")
            }
            Repeat::Bodies => {
                let names: Vec<&str> = args.bodies.iter().map(|b| b.name()).collect();
                let of = if names.is_empty() {
                    "(nothing picked)".to_owned()
                } else {
                    names.join(", ")
                };
                let how = if args.combine == Combine::Join {
                    "joined"
                } else {
                    "as a body"
                };
                format!("{of} mirrored, {how}")
            }
        }
    }
}
