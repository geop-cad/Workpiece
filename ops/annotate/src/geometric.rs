//! Geometric tolerance: how true a face is, by itself or from datums.

use anyhow::Context;
use shapecraft_core::world::topo::FaceId;
use shapecraft_core::world::{Annotation, Characteristic, Note};
use shapecraft_core::{Builder, Measure, Operation, Reference};

/// A characteristic as the dialog chooses it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct Of(pub Characteristic);

impl shapecraft_core::Choices for Of {
    const ALL: &'static [Self] = &[
        Of(Characteristic::Straightness),
        Of(Characteristic::Flatness),
        Of(Characteristic::Circularity),
        Of(Characteristic::Cylindricity),
        Of(Characteristic::LineProfile),
        Of(Characteristic::SurfaceProfile),
        Of(Characteristic::Parallelism),
        Of(Characteristic::Perpendicularity),
        Of(Characteristic::Angularity),
        Of(Characteristic::Position),
        Of(Characteristic::Concentricity),
        Of(Characteristic::Symmetry),
        Of(Characteristic::CircularRunout),
        Of(Characteristic::TotalRunout),
    ];

    fn label(self) -> &'static str {
        self.0.label()
    }
}

/// The face, what of it is toleranced, and how closely.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct GeometricToleranceArgs {
    /// The face that is toleranced.
    pub feature: Option<Reference<FaceId>>,
    /// What of it.
    #[arg(choice, label = "Of")]
    pub characteristic: Of,
    /// The width of the zone the face has to lie in.
    #[arg(label = "Tolerance")]
    pub value: Measure,
    /// Whether the zone is round: the tolerance is its diameter.
    #[serde(default)]
    #[arg(label = "Zone", options = "Between planes|Round (⌀)")]
    pub diametral: bool,
    /// The datums it is taken from, in order: their letters, apart.
    #[serde(default)]
    #[arg(label = "Datums", when = "!self.characteristic.0.of_form()")]
    pub datums: String,
    /// Where its frame stands, if that was said: a click beside the
    /// model, with the face picked, puts it there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[arg(hidden)]
    pub at: Option<[f64; 3]>,
}

impl Default for GeometricToleranceArgs {
    fn default() -> Self {
        Self {
            feature: None,
            characteristic: Of(Characteristic::Flatness),
            value: Measure::new(0.05),
            diametral: false,
            datums: String::new(),
            at: None,
        }
    }
}

/// Says how true a face has to be.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeometricTolerance {
    /// The operation's id.
    pub id: String,
    /// The face and its tolerance.
    pub args: GeometricToleranceArgs,
}

impl GeometricToleranceArgs {
    /// The letters of its datums.
    fn letters(&self) -> Vec<String> {
        self.datums
            .split(|c: char| !c.is_ascii_alphabetic())
            .filter(|l| !l.is_empty())
            .map(str::to_uppercase)
            .collect()
    }
}

impl Operation for GeometricTolerance {
    type Args = GeometricToleranceArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &GeometricToleranceArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut GeometricToleranceArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let args = &self.args;
        let value = (*builder).measure(&args.value, "the tolerance")?;
        anyhow::ensure!(value > 0.0, "a tolerance is more than nothing");
        let of = args.characteristic.0;
        let datums = if of.of_form() {
            Vec::new()
        } else {
            args.letters()
        };
        anyhow::ensure!(
            of.of_form()
                || !datums.is_empty()
                || matches!(
                    of,
                    Characteristic::LineProfile | Characteristic::SurfaceProfile
                ),
            "{} is from a datum: say which",
            of.label().to_lowercase()
        );
        anyhow::ensure!(datums.len() <= 3, "there are three datums at most");
        let world = builder.tracked();
        for letter in &datums {
            let known = world
                .annotations
                .values()
                .any(|a| matches!(&a.note, Note::Datum { letter: l, .. } if l == letter));
            anyhow::ensure!(known, "there is no datum {letter}");
        }
        let feature = args.feature.as_ref().context("pick the face")?;
        let (solid, face) = shapecraft_shape::on::face(&world.topo, feature.name())?;
        let (on, out) = shapecraft_shape::on::middle(&solid, face);
        let note = Annotation {
            sketch: None,
            note: Note::Geometric {
                characteristic: of,
                value,
                diametral: args.diametral,
                datums,
                on,
                out,
            },
            at: match args.at {
                Some(at) => shapecraft_core::math::Point3::from_array(at),
                None => on + out * (shapecraft_shape::on::size(&solid) * 0.18),
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
    ) -> shapecraft_core::EditVisualization<shapecraft_core::Edit<GeometricToleranceArgs, ()>> {
        let about = self.args.feature.as_ref().and_then(|feature| {
            let (solid, face) =
                shapecraft_shape::on::face(&context.world.topo, feature.name()).ok()?;
            Some(shapecraft_shape::on::middle(&solid, face).0)
        });
        crate::placed(&self.args, context, about, |args, at| {
            GeometricToleranceArgs {
                at: Some(at),
                ..args.clone()
            }
        })
    }

    fn summary(&self) -> String {
        let a = &self.args;
        let zone = if a.diametral { "⌀" } else { "" };
        let from = a.letters().join(" ");
        let of = a.characteristic.0;
        if of.of_form() || from.is_empty() {
            format!("{} {zone}{}", of.label().to_lowercase(), a.value)
        } else {
            format!(
                "{} {zone}{} from {from}",
                of.label().to_lowercase(),
                a.value
            )
        }
    }
}
