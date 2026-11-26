//! Surface texture: how rough a face may be, and whether it is machined
//! (ISO 21920-1; drawn as ISO 1302 has it).

use anyhow::Context;
use workpiece_core::world::topo::FaceId;
use workpiece_core::world::{Annotation, Note, Removal};
use workpiece_core::{Builder, Operation, Reference};

/// Whether material is removed, as the dialog chooses it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct Process(pub Removal);

impl workpiece_core::Choices for Process {
    const ALL: &'static [Self] = &[
        Process(Removal::Any),
        Process(Removal::Required),
        Process(Removal::Prohibited),
    ];

    fn label(self) -> &'static str {
        match self.0 {
            Removal::Any => "Any process",
            Removal::Required => "Machined",
            Removal::Prohibited => "Not machined",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self.0 {
            Removal::Any => "texture-any",
            Removal::Required => "texture-machined",
            Removal::Prohibited => "texture-unmachined",
        })
    }
}

/// The face, its requirement and the process.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct SurfaceTextureArgs {
    /// The face it is of.
    pub feature: Option<Reference<FaceId>>,
    /// The requirement: a parameter and its limit, `Ra 1,6`.
    #[arg(label = "Requirement")]
    pub text: String,
    /// Whether material is to be removed.
    #[arg(choice, label = "Process")]
    pub removal: Process,
    /// Where its symbol stands, if that was said: a click beside the
    /// model, with the face picked, puts it there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[arg(hidden)]
    pub at: Option<[f64; 3]>,
}

impl Default for SurfaceTextureArgs {
    fn default() -> Self {
        Self {
            feature: None,
            text: "Ra 3,2".into(),
            removal: Process(Removal::Required),
            at: None,
        }
    }
}

/// Says how rough a face may be.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SurfaceTexture {
    /// The operation's id.
    pub id: String,
    /// The face and its requirement.
    pub args: SurfaceTextureArgs,
}

impl Operation for SurfaceTexture {
    type Args = SurfaceTextureArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &SurfaceTextureArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut SurfaceTextureArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let args = &self.args;
        let text = args.text.trim();
        anyhow::ensure!(
            !text.is_empty(),
            "say the requirement: a parameter and its limit, Ra 1,6"
        );
        let world = builder.tracked();
        let feature = args.feature.as_ref().context("pick the face")?;
        let (solid, face) = workpiece_shape::on::face(&world.topo, feature.name())?;
        let (on, out) = workpiece_shape::on::middle(&solid, face);
        let note = Annotation {
            sketch: None,
            note: Note::Texture {
                text: text.to_owned(),
                removal: args.removal.0,
                on,
                out,
            },
            at: match args.at {
                Some(at) => workpiece_core::math::Point3::from_array(at),
                None => on + out * (workpiece_shape::on::size(&solid) * 0.15),
            },
        };
        Ok(workpiece_core::Made {
            annotations: vec![(self.id.clone(), note)],
            ..Default::default()
        })
    }

    fn visualize_edit(
        &self,
        context: &workpiece_core::EditContext<'_>,
        (): &(),
    ) -> workpiece_core::EditVisualization<workpiece_core::Edit<SurfaceTextureArgs, ()>> {
        let about = self.args.feature.as_ref().and_then(|feature| {
            let (solid, face) =
                workpiece_shape::on::face(&context.world.topo, feature.name()).ok()?;
            Some(workpiece_shape::on::middle(&solid, face).0)
        });
        crate::placed(&self.args, context, about, |args, at| SurfaceTextureArgs {
            at: Some(at),
            ..args.clone()
        })
    }

    fn summary(&self) -> String {
        let how = match self.args.removal.0 {
            Removal::Any => "",
            Removal::Required => ", machined",
            Removal::Prohibited => ", not machined",
        };
        format!("surface {}{how}", self.args.text.trim())
    }
}
