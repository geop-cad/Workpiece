//! [`ImportStep`]: the solids of a STEP file, as bodies.

use anyhow::Context;
use shapecraft_core::{Builder, Operation};

/// Which file to import.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    shapecraft_core::Arguments,
)]
pub struct ImportStepArgs {
    /// The path of the STEP file.
    #[arg(label = "STEP, IGES, STL or 3MF file")]
    pub path: String,
}

/// Adds the solids of a STEP file: the first is the body `{id}`, the
/// others `{id}/body2`, `{id}/body3`, …; their faces, edges and vertices
/// are named after their body. Lengths are in millimetres, whatever
/// unit the file uses. A file with a solid that cannot be read is
/// refused, with the reason.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportStep {
    /// The operation's id; also the name of the first body.
    pub id: String,
    /// The file.
    pub args: ImportStepArgs,
}

impl Operation for ImportStep {
    type Args = ImportStepArgs;
    type EditState = ();

    fn files(&self) -> Vec<&str> {
        vec![&self.args.path]
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &ImportStepArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut ImportStepArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing of the world is read or changed here: the bodies the
        // file holds are written by the builder.)
        let _ = builder;
        let mut out = shapecraft_boolean::Out::default();
        let path = self.args.path.trim();
        anyhow::ensure!(
            !path.is_empty(),
            "enter the path of a STEP, IGES, STL or 3MF file"
        );
        let bytes =
            shapecraft_core::files::read(path).with_context(|| format!("reading {path}"))?;
        // An IGES file: its trimmed surfaces, sewn.
        if crate::is_iges(path) {
            let found =
                crate::import_iges(&bytes, &self.id).with_context(|| format!("reading {path}"))?;
            if let Some(why) = found.refused.first() {
                anyhow::bail!(
                    "{path}: {} of its faces cannot be read: {why}",
                    found.refused.len()
                );
            }
            for (k, (body, closed)) in found.bodies.iter().enumerate() {
                let name = match k {
                    0 => self.id.clone(),
                    k => format!("{}/body{}", self.id, k + 1),
                };
                if *closed {
                    out.write(body, &name);
                } else {
                    out.write_sheet(body, &name);
                }
            }
            return Ok(out.made);
        }
        // A mesh: the solid its triangles bound.
        if crate::is_mesh(path) {
            let solids = crate::import_mesh(&bytes, path, &self.id)
                .with_context(|| format!("reading {path}"))?;
            for (k, solid) in solids.iter().enumerate() {
                let name = match k {
                    0 => self.id.clone(),
                    k => format!("{}/body{}", self.id, k + 1),
                };
                out.write(solid, &name);
            }
            return Ok(out.made);
        }
        let found = crate::import(&bytes).with_context(|| format!("reading {path}"))?;
        if let Some(why) = found.refused.first() {
            anyhow::bail!(
                "{path}: {} of its solids cannot be read: {why}",
                found.refused.len()
            );
        }
        anyhow::ensure!(!found.bodies.is_empty(), "{path} has no solids");
        for (k, body) in found.bodies.iter().enumerate() {
            let name = if k == 0 {
                self.id.clone()
            } else {
                format!("{}/body{}", self.id, k + 1)
            };
            out.write(&body.solid, &name);
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let op = self;
        let file = std::path::Path::new(&op.args.path).file_name();
        file.map_or_else(
            || "(no file)".to_owned(),
            |f| f.to_string_lossy().into_owned(),
        )
    }
}
