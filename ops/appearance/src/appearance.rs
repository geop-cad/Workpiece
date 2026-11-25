//! Appearance: the step that says what bodies and faces are made of and
//! what colour they have.

use anyhow::ensure;
use shapecraft_core::world::Look;
use shapecraft_core::{Builder, FaceOrBody, Made, Operation, Reference};

/// What gets a look, and the look.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    shapecraft_core::Arguments,
)]
pub struct AppearanceArgs {
    /// The bodies and the faces it is given to.
    #[arg(required, label = "Of")]
    pub of: Vec<Reference<FaceOrBody>>,
    /// The material they are made of, by its name in the library; none
    /// leaves them of what they are made of.
    #[serde(default)]
    #[arg(one_of = "context.parts.materials()", empty = "As it is")]
    pub material: Option<String>,
    /// Their colour, over their material's.
    #[serde(default)]
    #[arg(field)]
    pub colour: Option<[u8; 3]>,
}

/// Gives bodies and faces a material or a colour of their own: a face
/// over its body's, a body over the part's. Nothing of the shape
/// changes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Appearance {
    /// The operation's id.
    pub id: String,
    /// What gets which look.
    pub args: AppearanceArgs,
}

impl Operation for Appearance {
    type Args = AppearanceArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AppearanceArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AppearanceArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<Made<'_>> {
        let args = &self.args;
        ensure!(
            !args.of.is_empty(),
            "pick the bodies or the faces to give a look"
        );
        let topo = &builder.tracked().topo;
        let look = Look {
            material: args.material.clone(),
            colour: args.colour,
        };
        let mut made = Made::default();
        for picked in &args.of {
            let name = picked.name();
            ensure!(
                topo.referred(name).is_some(),
                "there is no body or face {name:?}"
            );
            made.looks.push((name.to_owned(), look.clone()));
        }
        Ok(made)
    }

    fn summary(&self) -> String {
        let args = &self.args;
        let what = match (&args.material, args.colour) {
            (Some(material), Some(_)) => format!("{material}, coloured"),
            (Some(material), None) => material.clone(),
            (None, Some([r, g, b])) => format!("#{r:02x}{g:02x}{b:02x}"),
            (None, None) => "as it is".to_owned(),
        };
        format!("{what}, of {}", args.of.len())
    }
}
