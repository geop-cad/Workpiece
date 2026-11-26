//! Thread: a thread on a cylindrical face, of a rod or of a hole — said
//! of it (cosmetic), or cut into it as a helical V groove.

use anyhow::Context;
use workpiece_boolean::read;
use workpiece_core::{Builder, Measure, Operation};
use workpiece_shape::thread::{Cylinder, cylinder};
use workpiece_shape::threaded;

use crate::norm;

/// The face threaded, and the thread's size.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
#[serde(default)]
pub struct ThreadArgs {
    /// Whether the thread is cut into the face, not only said of it.
    #[arg(
        label = "Thread",
        options = "Cosmetic|Modelled",
        icons = "thread-cosmetic|thread-modelled"
    )]
    pub modelled: bool,
    /// The cylindrical face: a rod's side, or a hole's wall.
    pub face: Option<workpiece_core::Reference<workpiece_core::world::topo::FaceId>>,
    /// Whether its pitch and depth are entered, not taken from the
    /// standard for the face's diameter.
    #[arg(
        label = "Size",
        options = "ISO metric|Custom",
        icons = "size-standard|size-custom"
    )]
    pub custom: bool,
    /// How far the thread advances in a turn.
    #[arg(when = "self.custom")]
    pub pitch: Measure,
    /// How deep the groove is cut into the face.
    #[arg(row, when = "self.custom")]
    pub depth: Measure,
    /// Whether it is a left-handed thread.
    #[arg(
        label = "Hand",
        options = "Right-handed|Left-handed",
        icons = "hand-right|hand-left"
    )]
    pub left: bool,
}

impl Default for ThreadArgs {
    fn default() -> Self {
        Self {
            modelled: false,
            face: None,
            custom: false,
            pitch: Measure::new(1.0),
            depth: Measure::new(0.61),
            left: false,
        }
    }
}

/// Threads a cylindrical face. A cosmetic thread leaves the face as it
/// is and is said of it: shown on the model, and drawn the way
/// drawings show threads (ISO 6410). A modelled one is cut into it: a
/// V groove with flanks 60° apart, along a helix the length of the
/// face.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Thread {
    /// The operation's id.
    pub id: String,
    /// The face and the thread's size.
    pub args: ThreadArgs,
}

/// What a thread of the major diameter `major` and `pitch` is called:
/// by its size's name where it is a standard one's coarse thread.
pub fn designation(major: f64, pitch: f64) -> String {
    let round = |x: f64| (x * 1000.0).round() / 1000.0;
    match norm::SIZES
        .iter()
        .find(|s| (s.diameter - major).abs() < 1e-6 * major.max(1.0))
    {
        Some(size) => size.designation(round(pitch)),
        None => format!("M{}×{}", round(major), round(pitch)),
    }
}

impl Operation for Thread {
    type Args = ThreadArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &ThreadArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut ThreadArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let args = &self.args;
        let picked = args
            .face
            .as_ref()
            .context("pick the face to thread")?
            .name();
        // (Nothing is changed here: the body it makes of the one it reads
        // is written by the builder.)
        let topo = &builder.tracked().topo;
        for (body, name) in workpiece_shape::bodies::solids_with(topo, &[picked]) {
            let solid = read(topo, body)?;
            let face = solid.faces.iter().position(|f| {
                f.name.as_deref() == Some(picked) || f.also.iter().any(|n| n == picked)
            });
            let Some(face) = face else {
                continue;
            };
            let Cylinder {
                origin,
                axis,
                radius,
                reach: (from, to),
                hole,
            } = cylinder(&solid, face)?;
            // Its pitch and depth: as entered, or the coarse thread of
            // the size the face is of — a rod as wide as the thread, a
            // hole as wide as the drill for its tap.
            let (pitch, depth) = if args.custom {
                (
                    (*builder).measure(&args.pitch, "the pitch")?,
                    (*builder).measure(&args.depth, "the depth")?,
                )
            } else {
                let across = radius * 2.0;
                let size = if hole {
                    // (The size whose tap's drill is nearest.)
                    let off = |s: &&norm::Size| {
                        libm::log(norm::tap_drill(s.diameter, s.coarse) / across).abs()
                    };
                    norm::SIZES.iter().min_by(|a, b| off(a).total_cmp(&off(b)))
                } else {
                    norm::size_for(across)
                };
                let size = size.context("the face is narrower than the smallest thread, M1")?;
                // (A hole about as wide as the drill for the tap is
                // threaded out to the thread's diameter.)
                let out = (size.diameter - across) / 2.0;
                let depth = if hole && (0.35..=0.75).contains(&(out / size.coarse)) {
                    out
                } else {
                    norm::thread_depth(size.coarse)
                };
                (size.coarse, depth)
            };
            anyhow::ensure!(
                pitch > 0.0 && depth > 0.0,
                "a thread has a pitch and a depth"
            );
            if args.modelled {
                let made = threaded(&solid, face, pitch, depth, args.left)?;
                let mut out = workpiece_core::Made::body(name.clone(), made);
                out.removed.push(name);
                return Ok(out);
            }
            anyhow::ensure!(
                hole || depth < radius,
                "the thread is deeper than the rod is thick"
            );
            let (major, minor) = if hole {
                (radius * 2.0 + depth * 2.0, radius * 2.0)
            } else {
                (radius * 2.0, radius * 2.0 - depth * 2.0)
            };
            let said = workpiece_core::world::Thread {
                face: picked.to_owned(),
                origin: origin + axis * from,
                axis,
                length: to - from,
                major,
                minor,
                pitch,
                left: args.left,
                internal: hole,
                designation: designation(major, pitch),
                // (A hole's thread starts at its mouth; a rod's runs
                // from wherever the face does.)
                ends: [!hole, true],
            };
            // What it is, on a leader (ISO 6410-2): `M16`, `M16×1.5`,
            // `-LH` after a left-handed one — to a rod's outline where
            // it is seen from the side, to a hole's rim seen end on.
            let middle = said.origin + axis * (said.length / 2.0);
            let hand = if args.left { "-LH" } else { "" };
            let callout = workpiece_core::world::Annotation {
                at: middle + axis.any_orthonormal_vector() * (radius * 3.0),
                sketch: None,
                note: workpiece_core::world::Note::Hole {
                    centre: if hole { said.origin } else { middle },
                    axis,
                    radius,
                    lines: vec![vec![workpiece_core::world::Callout::Text(format!(
                        "{}{hand}",
                        said.designation
                    ))]],
                    outside: !hole,
                },
            };
            return Ok(workpiece_core::Made {
                threads: vec![(self.id.clone(), said)],
                annotations: vec![(format!("{}/callout", self.id), callout)],
                ..Default::default()
            });
        }
        anyhow::bail!("there is no face {picked:?} to thread")
    }

    fn summary(&self) -> String {
        let a = &self.args;
        let kind = if a.modelled { "modelled" } else { "cosmetic" };
        let hand = if a.left { ", left-handed" } else { "" };
        if a.custom {
            format!("{kind}, pitch {}, {} deep{hand}", a.pitch, a.depth)
        } else {
            format!("{kind}, ISO metric{hand}")
        }
    }
}
