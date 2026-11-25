//! Delete face: a feature's faces taken off a body, and the faces
//! around it closed over where it was — a hole or a pocket filled, a
//! boss removed.

use std::collections::BTreeSet;

use anyhow::{Context, ensure};
use workpiece_boolean::read;
use workpiece_core::{Builder, Operation};
pub use workpiece_shape::heal::faces_deleted;

/// The faces to delete.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    workpiece_core::Arguments,
)]
pub struct DeleteFaceArgs {
    /// The faces deleted: all of a hole, a pocket or a boss.
    #[arg(required)]
    pub faces: Vec<workpiece_core::Reference<workpiece_core::world::topo::FaceId>>,
}

/// Deletes faces of bodies and closes the faces around them over where
/// they were.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeleteFace {
    /// The operation's id.
    pub id: String,
    /// The faces.
    pub args: DeleteFaceArgs,
}

impl Operation for DeleteFace {
    type Args = DeleteFaceArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &DeleteFaceArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut DeleteFaceArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let args = &self.args;
        ensure!(!args.faces.is_empty(), "pick the faces to delete");
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
            let made = faces_deleted(&solid, &faces)
                .with_context(|| format!("deleting faces of {name}"))?;
            out.removed.push(name.clone());
            out = out.and(name.clone(), made);
        }
        if let Some(missing) = picked.iter().find(|p| !found.contains(*p)) {
            anyhow::bail!("there is no face {missing:?} to delete");
        }
        Ok(out)
    }

    fn summary(&self) -> String {
        let n = self.args.faces.len();
        format!("{n} {}", if n == 1 { "face" } else { "faces" })
    }
}
