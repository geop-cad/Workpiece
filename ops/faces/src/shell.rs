//! Shell: a body hollowed to a wall, open at the faces picked.

use std::collections::BTreeSet;

use anyhow::ensure;
use shapecraft_boolean::read;
use shapecraft_core::{Builder, Measure, Operation};

/// Where a body is opened, and how thick its wall is left.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct ShellArgs {
    /// The faces the body is open at; a body, picked itself, is
    /// hollowed with no opening.
    #[arg(label = "Open at", required)]
    pub faces: Vec<shapecraft_core::Reference<shapecraft_core::FaceOrBody>>,
    /// How thick the wall is.
    pub thickness: Measure,
}

impl Default for ShellArgs {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            thickness: Measure::new(1.0),
        }
    }
}

/// Hollows bodies: of each, what is within the thickness of its faces
/// stays as its wall, but for the faces picked, where it is left open.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Shell {
    /// The operation's id.
    pub id: String,
    /// Where it opens, and how thick the wall is.
    pub args: ShellArgs,
}

impl Operation for Shell {
    type Args = ShellArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &ShellArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut ShellArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        let args = &self.args;
        let thickness = (*builder).measure(&args.thickness, "the wall's thickness")?;
        ensure!(thickness > 0.0, "a wall has some thickness");
        ensure!(!args.faces.is_empty(), "pick the faces to open the body at");
        let picked: Vec<&str> = args.faces.iter().map(|r| r.name()).collect();
        // (Nothing is changed here: the bodies it makes of those it reads
        // are written by the builder.)
        let topo = &builder.tracked().topo;
        let mut out = shapecraft_core::Made::default();
        let mut found: BTreeSet<&str> = BTreeSet::new();
        for (body, name) in shapecraft_shape::bodies::solids_with(topo, &picked) {
            let solid = read(topo, body)?;
            let open: Vec<usize> = (0..solid.faces.len())
                .filter(|&f| {
                    let named = solid.faces[f].name.as_deref();
                    let also = solid.faces[f].also.iter().map(String::as_str);
                    let hit = named.into_iter().chain(also).find(|n| picked.contains(n));
                    found.extend(hit.and_then(|n| picked.iter().find(|p| **p == n)));
                    hit.is_some()
                })
                .collect();
            let whole = picked.iter().find(|p| **p == name);
            found.extend(whole);
            if open.is_empty() && whole.is_none() {
                continue;
            }
            // The wall's thickness, said once, at a face that stays (more
            // are added by hand where they are wanted).
            let stays = (0..solid.faces.len()).find(|f| !open.contains(f));
            if let (true, Some(face)) = (out.annotations.is_empty(), stays) {
                out.annotations.push(shapecraft_shape::on::held(
                    &self.id,
                    "thickness",
                    (&solid, face),
                    (-thickness, None),
                    &args.thickness,
                    thickness,
                ));
            }
            let hollow = shapecraft_shape::thick::shelled(&solid, thickness, &open)?;
            out.removed.push(name.clone());
            out = out.and(name.clone(), hollow);
        }
        if let Some(missing) = picked.iter().find(|p| !found.contains(*p)) {
            anyhow::bail!("there is no face or body {missing:?} to open");
        }
        Ok(out)
    }

    fn summary(&self) -> String {
        let n = self.args.faces.len();
        format!(
            "wall {} thick, open at {n} {}",
            self.args.thickness,
            if n == 1 { "pick" } else { "picks" }
        )
    }
}
