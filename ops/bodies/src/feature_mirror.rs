//! Feature mirror: what steps did to a body — what an extrusion added,
//! a hole took away, a fillet rounded — done again as the mirror image
//! in a plane.

use anyhow::{Context, ensure};
use workpiece_boolean::{Op, combine, read};
use workpiece_core::math::Point3;
use workpiece_core::world::DatumPlane;
use workpiece_core::world::topo::FaceId;
use workpiece_core::{Builder, Operation, Reference};

use crate::features::{Change, Features, Move, steps_of};

/// The features, and the plane they are mirrored in.
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
pub struct FeatureMirrorArgs {
    /// Faces of the features that are mirrored: one of each is enough.
    /// Features with others between them are mirrored with those.
    #[arg(required, label = "Faces of the features")]
    pub features: Vec<Reference<FaceId>>,
    /// The plane they are mirrored in: a datum plane or a flat face.
    #[arg(label = "In")]
    pub plane: Option<Reference<DatumPlane>>,
}

/// Mirrors features: what the steps that made the picked faces (and the
/// steps between them) added to the bodies and took from them is added
/// and taken again as the mirror image in a plane. What they make there
/// is named under `{id}/`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FeatureMirror {
    /// The operation's id.
    pub id: String,
    /// The features, and the plane.
    pub args: FeatureMirrorArgs,
}

impl Operation for FeatureMirror {
    type Args = FeatureMirrorArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &FeatureMirrorArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut FeatureMirrorArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        ensure!(
            !args.features.is_empty(),
            "pick a face of each feature to mirror"
        );
        let plane = args.plane.as_ref().context("pick a plane to mirror in")?;
        let plane = builder.tracked().resolve(plane)?;
        let (o, n) = (plane.origin(), plane.normal());
        let reflect = |p: Point3| p - n * (2.0 * (p - o).dot(n));
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
                    back: &reflect,
                    there: &reflect,
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
                let copy = piece.mapped(&reflect, true, 1.0)?;
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
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let steps = steps_of(&self.args.features);
        let of = if steps.is_empty() {
            "(nothing picked)".to_owned()
        } else {
            steps.join(", ")
        };
        format!("{of} mirrored")
    }
}
