//! Datum feature: a face named A, B, C… that the part is measured from.

use anyhow::Context;
use shapecraft_core::world::topo::FaceId;
use shapecraft_core::world::{Annotation, Note, World};
use shapecraft_core::{Builder, Operation, Reference};

/// The face, and its letter.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct DatumFeatureArgs {
    /// The face that is the datum feature.
    pub feature: Option<Reference<FaceId>>,
    /// Its letter: `A`, `B`, `C`.
    pub letter: String,
    /// Where its letter stands, if that was said: a click beside the
    /// model, with the face picked, puts it there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[arg(hidden)]
    pub at: Option<[f64; 3]>,
}

impl Default for DatumFeatureArgs {
    fn default() -> Self {
        Self {
            feature: None,
            letter: "A".into(),
            at: None,
        }
    }
}

impl DatumFeatureArgs {
    /// A datum with the first letter no datum of `world` has yet (but I,
    /// O and Q, which are not used: ISO 5459).
    pub fn next(world: &World) -> Self {
        let taken: Vec<&str> = world
            .annotations
            .values()
            .filter_map(|a| match &a.note {
                Note::Datum { letter, .. } => Some(letter.as_str()),
                _ => None,
            })
            .collect();
        let letter = ('A'..='Z')
            .filter(|c| !matches!(c, 'I' | 'O' | 'Q'))
            .map(String::from)
            .find(|l| !taken.contains(&l.as_str()))
            .unwrap_or_else(|| "A".into());
        Self {
            feature: None,
            letter,
            at: None,
        }
    }
}

/// Names a face a datum feature.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DatumFeature {
    /// The operation's id.
    pub id: String,
    /// The face and its letter.
    pub args: DatumFeatureArgs,
}

impl Operation for DatumFeature {
    type Args = DatumFeatureArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &DatumFeatureArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut DatumFeatureArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let args = &self.args;
        let letter = args.letter.trim();
        anyhow::ensure!(
            !letter.is_empty() && letter.chars().all(|c| c.is_ascii_uppercase()),
            "a datum's letter is a capital: A, B, C"
        );
        let world = builder.tracked();
        let taken = world
            .annotations
            .values()
            .any(|a| matches!(&a.note, Note::Datum { letter: other, .. } if other == letter));
        anyhow::ensure!(!taken, "there is a datum {letter} already");
        let feature = args.feature.as_ref().context("pick the face")?;
        let (solid, face) = shapecraft_shape::on::face(&world.topo, feature.name())?;
        let (on, out) = shapecraft_shape::on::middle(&solid, face);
        let note = Annotation {
            sketch: None,
            note: Note::Datum {
                letter: letter.to_owned(),
                on,
                out,
            },
            at: match args.at {
                Some(at) => shapecraft_core::math::Point3::from_array(at),
                None => on + out * (shapecraft_shape::on::size(&solid) * 0.12),
            },
        };
        Ok(shapecraft_core::Made {
            annotations: vec![(self.id.clone(), note)],
            ..Default::default()
        })
    }

    fn visualize_edit(
        &self,
        context: &shapecraft_core::EditContext<'_>,
        (): &(),
    ) -> shapecraft_core::EditVisualization<shapecraft_core::Edit<DatumFeatureArgs, ()>> {
        let about = self.args.feature.as_ref().and_then(|feature| {
            let (solid, face) =
                shapecraft_shape::on::face(&context.world.topo, feature.name()).ok()?;
            Some(shapecraft_shape::on::middle(&solid, face).0)
        });
        crate::placed(&self.args, context, about, |args, at| DatumFeatureArgs {
            at: Some(at),
            ..args.clone()
        })
    }

    fn summary(&self) -> String {
        let of = shapecraft_core::reference::described(&self.args.feature);
        format!("{} is {of}", self.args.letter)
    }
}
