//! Render: the step that is a picture of the model as it is where the
//! step stands in the part's program.

use anyhow::ensure;
use workpiece_core::{Builder, Choices, Made, Operation};

/// What lights the picture.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Sky {
    /// A studio: soft light from all around, and a key light.
    #[default]
    Studio,
    /// Outdoors: a blue sky and the sun.
    Outdoor,
    /// A dark room with one light.
    Dark,
}

impl Choices for Sky {
    const ALL: &'static [Self] = &[Sky::Studio, Sky::Outdoor, Sky::Dark];

    fn label(self) -> &'static str {
        match self {
            Sky::Studio => "Studio",
            Sky::Outdoor => "Outdoors",
            Sky::Dark => "Dark room",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Sky::Studio => "sky-studio",
            Sky::Outdoor => "sky-outdoor",
            Sky::Dark => "sky-dark",
        })
    }
}

/// From where the picture is taken.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct View {
    /// What the camera looks at.
    pub target: [f32; 3],
    /// How the camera is turned: a quaternion that turns its axes (x
    /// right, y up, z towards the eye) into the model's.
    pub rotation: [f32; 4],
    /// From the target to the eye.
    pub distance: f32,
    /// The vertical field of view, in radians.
    pub fov_y: f32,
    /// Whether it is a parallel projection, not a perspective.
    pub orthographic: bool,
}

/// The picture: from where it is taken, how large it is, and its
/// light.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct Shot {
    /// From where it is taken; none, until the view was turned to it.
    #[serde(default)]
    #[arg(hidden)]
    pub view: Option<View>,
    /// How many pixels wide the picture is.
    pub width: usize,
    /// How many pixels high.
    #[arg(row)]
    pub height: usize,
    /// How many times each pixel is sampled: more is less grainy, and
    /// takes longer.
    pub samples: usize,
    /// What lights it.
    #[serde(default)]
    #[arg(choice, label = "Light")]
    pub sky: Sky,
    /// Where the key light stands around the model: in degrees from
    /// the x axis towards y.
    #[arg(label = "Light from", icon = "f-direction")]
    pub sun_turn: f64,
    /// How high the key light stands, in degrees above the ground.
    #[arg(row, label = "Height")]
    pub sun_height: f64,
    /// How much lighter (or, below nothing, darker) the picture is, in
    /// stops: each doubles the light.
    pub exposure: f64,
    /// Whether the model stands on a floor that catches its shadow.
    #[arg(
        label = "Floor",
        options = "None|Shadow",
        icons = "floor-none|floor-shadow"
    )]
    pub ground: bool,
    /// The colour behind the model; none is a light grey.
    #[serde(default)]
    #[arg(field, label = "Backdrop")]
    pub backdrop: Option<[u8; 3]>,
}

impl Default for Shot {
    fn default() -> Self {
        Self {
            view: None,
            width: 1600,
            height: 1000,
            samples: 256,
            sky: Sky::Studio,
            sun_turn: -35.0,
            sun_height: 50.0,
            exposure: 0.0,
            ground: true,
            backdrop: None,
        }
    }
}

impl Shot {
    /// The colour behind the model.
    pub fn backdrop(&self) -> [u8; 3] {
        self.backdrop.unwrap_or([226, 228, 232])
    }
}

/// A picture of the model, with light, shadow and reflections. The
/// model is pictured as the steps before this one leave it; nothing of
/// the model follows from it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Render {
    /// The operation's id: what the picture is called.
    pub id: String,
    /// The picture.
    pub args: Shot,
}

impl Operation for Render {
    type Args = Shot;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &Shot {
        &self.args
    }

    fn args_mut(&mut self) -> &mut Shot {
        &mut self.args
    }

    fn make(&self, _: &dyn Builder) -> anyhow::Result<Made<'_>> {
        let shot = &self.args;
        ensure!(
            (16..=8192).contains(&shot.width) && (16..=8192).contains(&shot.height),
            "a picture is from 16 to 8192 pixels wide and high"
        );
        ensure!(shot.samples >= 1, "a picture takes at least one sample");
        Ok(Made::default())
    }

    fn summary(&self) -> String {
        let shot = &self.args;
        format!(
            "{} × {}, {}",
            shot.width,
            shot.height,
            shot.sky.label().to_lowercase()
        )
    }
}
