//! Draft: faces of a body leaned by an angle, as a part that has to
//! come out of a mould needs them.

use std::collections::BTreeSet;

use anyhow::{Context, ensure};
use workpiece_boolean::read;
use workpiece_core::world::DatumPlane;
use workpiece_core::{Builder, Measure, Operation, Reference};

/// Which faces lean, about what, and by how much.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct DraftArgs {
    /// The faces that are drafted.
    #[arg(required)]
    pub faces: Vec<workpiece_core::Reference<workpiece_core::world::topo::FaceId>>,
    /// The neutral plane: the faces turn about where they meet it, and
    /// the pull is along its normal.
    #[arg(label = "Neutral plane")]
    pub neutral: Option<Reference<DatumPlane>>,
    /// The angle, in degrees: into the body as the faces rise along the
    /// pull.
    #[arg(label = "Angle °")]
    pub angle: Measure,
    /// Whether the pull is against the plane's normal.
    #[arg(flip, row)]
    pub flip: bool,
    /// Whether the faces lean the same way on both sides of the plane
    /// (a part made in two halves of a mould, parted there).
    #[arg(
        label = "Sides",
        options = "One way|Both ways from the plane",
        icons = "draft-one|draft-both"
    )]
    pub both: bool,
}

impl Default for DraftArgs {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            neutral: None,
            angle: Measure::new(3.0),
            flip: false,
            both: false,
        }
    }
}

/// Leans faces of bodies by an angle from the pull direction, each
/// about where it meets the neutral plane. The faces beside them are
/// trimmed to them or grow to meet them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Draft {
    /// The operation's id.
    pub id: String,
    /// What leans, about what, by how much.
    pub args: DraftArgs,
}

impl Operation for Draft {
    type Args = DraftArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &DraftArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut DraftArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let args = &self.args;
        ensure!(!args.faces.is_empty(), "pick the faces to draft");
        let plane = args.neutral.as_ref().context("pick the neutral plane")?;
        let plane = builder.tracked().resolve(plane)?;
        let angle = (*builder)
            .measure(&args.angle, "the draft angle")?
            .to_radians();
        let pull = if args.flip {
            -plane.normal()
        } else {
            plane.normal()
        };
        let picked: Vec<&str> = args.faces.iter().map(|r| r.name()).collect();
        // (Nothing is changed here: the bodies it makes of those it reads
        // are written by the builder.)
        let topo = &builder.tracked().topo;
        let mut out = workpiece_core::Made::default();
        let mut found: BTreeSet<&str> = BTreeSet::new();
        for (body, name) in workpiece_shape::bodies::solids_with(topo, &picked) {
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
            let made = workpiece_shape::thick::drafted(
                &solid,
                &faces,
                pull,
                plane.origin(),
                angle,
                args.both,
            )?;
            // Its angle is a size of the model: said at the first face
            // drafted that leans off the pull, where it crosses the
            // neutral plane, between the pull and the face as it leans
            // now.
            for &face in &faces {
                if !out.annotations.is_empty() {
                    break;
                }
                use workpiece_core::world::{Annotation, Measured};
                let (on, way) = workpiece_shape::on::middle(&solid, face);
                if let Some(across) = (way - pull * way.dot(pull)).try_normalize() {
                    let at = on - pull * (on - plane.origin()).dot(pull);
                    let measured = Measured::Angle {
                        at,
                        first: pull,
                        second: pull * libm::cos(angle) - across * libm::sin(angle),
                    };
                    let reach = workpiece_shape::on::size(&solid) * 0.15;
                    let held = Annotation::held(measured, angle.to_degrees(), &args.angle, reach);
                    out.annotations.push((format!("{}/angle", self.id), held));
                }
            }
            out.removed.push(name.clone());
            out = out.and(name.clone(), made);
        }
        if let Some(missing) = picked.iter().find(|p| !found.contains(*p)) {
            anyhow::bail!("there is no face {missing:?} to draft");
        }
        Ok(out)
    }

    fn summary(&self) -> String {
        let n = self.args.faces.len();
        format!(
            "{}° on {n} {}",
            self.args.angle,
            if n == 1 { "pick" } else { "picks" }
        )
    }
}
