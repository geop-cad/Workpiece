//! Move face: faces of a finished body pushed or pulled — along their
//! own normals, or all one way — with the body following.

use std::collections::BTreeSet;

use anyhow::{Context, ensure};
use shapecraft_boolean::read;
use shapecraft_core::world::DatumAxis;
use shapecraft_core::{Builder, Measure, Operation, Reference};

/// Which way the faces go.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum FaceWay {
    /// Each off itself, along its own normal: a flat face moves, a hole
    /// widens, a boss thickens.
    #[default]
    Offset,
    /// All along one axis, as they are (a round carried with the face
    /// it runs into).
    Along,
    /// Replaced by a plane: the body runs on to it, or is cut back to
    /// it.
    ToPlane,
}

/// The faces, which way they go, and how far.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct MoveFaceArgs {
    /// The faces moved.
    #[arg(required)]
    pub faces: Vec<shapecraft_core::Reference<shapecraft_core::world::topo::FaceId>>,
    /// Off themselves, or all along an axis.
    #[serde(default)]
    #[arg(choice)]
    pub way: FaceWay,
    /// The axis they are moved along, for [`FaceWay::Along`].
    #[serde(default)]
    #[arg(when = "self.way == FaceWay::Along")]
    pub axis: Option<Reference<DatumAxis>>,
    /// The plane the faces are replaced by, for [`FaceWay::ToPlane`].
    #[serde(default)]
    #[arg(when = "self.way == FaceWay::ToPlane")]
    pub plane: Option<Reference<shapecraft_core::world::DatumPlane>>,
    /// How far: out of the body, or along the axis.
    #[arg(when = "self.way != FaceWay::ToPlane")]
    pub distance: Measure,
    /// Whether they go the other way: into the body, or against the
    /// axis.
    #[serde(default)]
    #[arg(flip, row, when = "self.way != FaceWay::ToPlane")]
    pub flip: bool,
}

impl Default for MoveFaceArgs {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            way: FaceWay::Offset,
            axis: None,
            plane: None,
            distance: Measure::new(1.0),
            flip: false,
        }
    }
}

/// Pushes or pulls faces of bodies: what the faces pass through is
/// added to the body where they move out of it and taken from it where
/// they move in, and the faces beside them run on or are cut back to
/// meet them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MoveFace {
    /// The operation's id.
    pub id: String,
    /// The faces, which way, and how far.
    pub args: MoveFaceArgs,
}

impl Operation for MoveFace {
    type Args = MoveFaceArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &MoveFaceArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut MoveFaceArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let args = &self.args;
        ensure!(!args.faces.is_empty(), "pick the faces to move");
        let distance = (*builder).measure(&args.distance, "the distance")?;
        ensure!(distance > 0.0, "by some distance");
        let distance = if args.flip { -distance } else { distance };
        let plane = match args.way {
            FaceWay::ToPlane => {
                let plane = args
                    .plane
                    .as_ref()
                    .context("pick the plane to replace them by")?;
                Some(builder.tracked().resolve(plane)?)
            }
            _ => None,
        };
        let along = match args.way {
            FaceWay::Offset | FaceWay::ToPlane => None,
            FaceWay::Along => {
                let axis = args.axis.as_ref().context("pick the axis to move along")?;
                Some(builder.tracked().resolve(axis)?.direction() * distance)
            }
        };
        let picked: Vec<&str> = args.faces.iter().map(|r| r.name()).collect();
        // (Nothing is changed here: the bodies it makes of those it reads
        // are written by the builder.)
        let topo = &builder.tracked().topo;
        let mut out = shapecraft_core::Made::default();
        let mut found: BTreeSet<&str> = BTreeSet::new();
        for (body, name) in shapecraft_shape::bodies::solids_with(topo, &picked) {
            let solid = read(topo, body)?;
            let faces: Vec<usize> = (0..solid.faces.len())
                .filter(|&f| {
                    let named = solid.faces[f].name.as_deref();
                    let also = solid.faces[f].also.iter().map(String::as_str);
                    let hit = named.into_iter().chain(also).find(|n| picked.contains(n));
                    found.extend(hit.and_then(|n| picked.iter().find(|p| **p == n)));
                    hit.is_some()
                })
                .collect();
            if faces.is_empty() {
                continue;
            }
            if out.annotations.is_empty() && plane.is_none() {
                let (by, way) = match along {
                    Some(by) => (by.length(), by.try_normalize()),
                    None => (distance, None),
                };
                out.annotations.push(shapecraft_shape::on::held(
                    &self.id,
                    "distance",
                    (&solid, faces[0]),
                    (by, way),
                    &args.distance,
                    distance.abs(),
                ));
            }
            let made = match (along, &plane) {
                (_, Some(plane)) => shapecraft_shape::thick::faces_to_plane(
                    &solid,
                    &faces,
                    plane.origin(),
                    plane.normal(),
                ),
                (Some(by), _) => shapecraft_shape::thick::faces_moved(&solid, &faces, by),
                (None, _) => shapecraft_shape::thick::faces_offset(&solid, &faces, distance),
            }
            .with_context(|| format!("moving faces of {name}"))?;
            ensure!(!made.is_empty(), "nothing would be left of {name}");
            out.removed.push(name.clone());
            out = out.and(name.clone(), made);
        }
        if let Some(missing) = picked.iter().find(|p| !found.contains(*p)) {
            anyhow::bail!("there is no face {missing:?} to move");
        }
        Ok(out)
    }

    fn summary(&self) -> String {
        let a = &self.args;
        let n = a.faces.len();
        let faces = if n == 1 { "face" } else { "faces" };
        let way = match (a.way, a.flip) {
            (FaceWay::Offset, false) => "out by",
            (FaceWay::Offset, true) => "in by",
            (FaceWay::Along, _) => "along an axis by",
            (FaceWay::ToPlane, _) => return format!("{n} {faces} replaced by a plane"),
        };
        format!("{n} {faces} {way} {}", a.distance)
    }
}

impl shapecraft_core::Choices for FaceWay {
    const ALL: &'static [Self] = &[FaceWay::Offset, FaceWay::Along, FaceWay::ToPlane];

    fn label(self) -> &'static str {
        match self {
            FaceWay::Offset => "Off themselves",
            FaceWay::Along => "Along an axis",
            FaceWay::ToPlane => "Replace by a plane",
        }
    }
}
