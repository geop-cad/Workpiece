//! Camera view: the step that is a named way of looking at the model —
//! from a side, as the view is turned, or exactly along a plane's
//! normal. The 3D view flies to it, and drawings draw the model from
//! it.

use anyhow::Context;
use shapecraft_core::world::{DatumPlane, Viewpoint};
use shapecraft_core::{Builder, Choices, Made, Operation, Reference};
use shapecraft_ops_appearance::View;

use crate::sheet::Side;

/// What says how a camera view looks at the model.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Looking {
    /// As the 3D view is turned while the step is edited.
    #[default]
    Current,
    /// Square onto a plane, a frame's plane or a flat face.
    Plane,
    /// From a side.
    Side(Side),
}

impl Choices for Looking {
    const ALL: &'static [Self] = &[
        Looking::Current,
        Looking::Plane,
        Looking::Side(Side::Front),
        Looking::Side(Side::Back),
        Looking::Side(Side::Left),
        Looking::Side(Side::Right),
        Looking::Side(Side::Top),
        Looking::Side(Side::Bottom),
        Looking::Side(Side::Isometric),
        Looking::Side(Side::IsometricFrontLeft),
        Looking::Side(Side::IsometricBackRight),
        Looking::Side(Side::IsometricBackLeft),
        Looking::Side(Side::Dimetric),
        Looking::Side(Side::IsometricBelowFrontRight),
        Looking::Side(Side::IsometricBelowFrontLeft),
        Looking::Side(Side::IsometricBelowBackRight),
        Looking::Side(Side::IsometricBelowBackLeft),
    ];

    fn label(self) -> &'static str {
        match self {
            Looking::Current => "As the view is",
            Looking::Plane => "Onto a plane",
            Looking::Side(side) => side.label(),
        }
    }
}

/// How a camera view looks at the model.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct CameraViewArgs {
    /// What says how it looks.
    #[arg(choice, label = "Looking")]
    pub looking: Looking,
    /// How the 3D view was turned; none, until it was turned to it.
    #[serde(default)]
    #[arg(hidden)]
    pub view: Option<View>,
    /// The plane it looks square onto: a datum plane, a frame's plane
    /// or a flat face. Its x is to the right.
    #[serde(default)]
    #[arg(when = "self.looking == Looking::Plane", label = "Onto")]
    pub plane: Option<Reference<DatumPlane>>,
    /// Whether it looks at the plane's back.
    #[serde(default)]
    #[arg(
        when = "self.looking == Looking::Plane",
        label = "Side",
        options = "Front|Back"
    )]
    pub back: bool,
    /// How far it is turned about the line it looks along, in degrees,
    /// counter-clockwise.
    #[serde(default)]
    #[arg(when = "self.looking != Looking::Current", label = "Turned")]
    pub turn: f64,
    /// Whether it is a parallel projection, not a perspective.
    #[arg(
        when = "self.looking != Looking::Current",
        label = "Projection",
        options = "Perspective|Parallel"
    )]
    pub orthographic: bool,
}

impl Default for CameraViewArgs {
    fn default() -> Self {
        Self {
            looking: Looking::Current,
            view: None,
            plane: None,
            back: false,
            turn: 0.0,
            orthographic: true,
        }
    }
}

/// The axis `axis` of a camera turned by the quaternion `q` (x, y, z,
/// w), in the model.
fn turned(q: [f32; 4], axis: [f64; 3]) -> shapecraft_core::math::Point3 {
    use shapecraft_core::math::Point3;
    let [x, y, z, w] = q.map(f64::from);
    let u = Point3::new(x, y, z);
    let v = Point3::from_array(axis);
    (v + u.cross(u.cross(v) + v * w) * 2.0).normalize_or_zero()
}

/// A named view of the model: a way of looking at it that the 3D view
/// flies to and drawings draw it from. Nothing of the model follows
/// from it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CameraView {
    /// The operation's id: what the view is called.
    pub id: String,
    /// How it looks.
    pub args: CameraViewArgs,
}

impl CameraView {
    /// How it looks at `world`.
    pub fn viewpoint(&self, world: &shapecraft_core::World) -> anyhow::Result<Viewpoint> {
        use shapecraft_core::math::Point3;
        let args = &self.args;
        let turn = args.turn.to_radians();
        let seen = match args.looking {
            Looking::Current => {
                let view = args
                    .view
                    .context("turn the view to how the model is to be looked at")?;
                Viewpoint {
                    right: turned(view.rotation, [1.0, 0.0, 0.0]),
                    up: turned(view.rotation, [0.0, 1.0, 0.0]),
                    towards: turned(view.rotation, [0.0, 0.0, 1.0]),
                    orthographic: view.orthographic,
                    at: Some((
                        Point3::from_array(view.target.map(f64::from)),
                        f64::from(view.distance),
                    )),
                }
            }
            Looking::Plane => {
                let plane =
                    world.resolve(args.plane.as_ref().context("pick the plane to look onto")?)?;
                let sign = if args.back { -1.0 } else { 1.0 };
                Viewpoint {
                    right: plane.x_dir() * sign,
                    up: plane.y_dir(),
                    towards: plane.normal() * sign,
                    orthographic: args.orthographic,
                    at: None,
                }
                .turned(turn)
            }
            Looking::Side(side) => {
                let [right, up, towards] = side.basis();
                Viewpoint {
                    right,
                    up,
                    towards,
                    orthographic: args.orthographic,
                    at: None,
                }
                .turned(turn)
            }
        };
        Ok(seen)
    }
}

impl Operation for CameraView {
    type Args = CameraViewArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &CameraViewArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut CameraViewArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<Made<'_>> {
        // (A view that was not turned to yet says nothing: it is as the
        // view is once it rests.)
        if self.args.looking == Looking::Current && self.args.view.is_none() {
            return Ok(Made::default());
        }
        let seen = self.viewpoint(builder.tracked())?;
        Ok(Made {
            views: vec![(self.id.clone(), seen)],
            ..Made::default()
        })
    }

    fn summary(&self) -> String {
        match self.args.looking {
            Looking::Plane => format!(
                "onto {}",
                shapecraft_core::reference::described(&self.args.plane)
            ),
            looking => looking.label().to_lowercase(),
        }
    }
}
