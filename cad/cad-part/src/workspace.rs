//! The [`Workspace`]: the parts of a project, by path, and building them.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use workpiece_core::{Cache, Parts, World};
use workpiece_ops_step::{ImportStep, ImportStepArgs};

use crate::{Cancel, Engine, Part};

/// Why a part could not be found or built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceError {
    /// No part has this path.
    #[error("no part at {0:?}")]
    UnknownPart(String),
    /// A part contains itself; the paths from the first occurrence of the
    /// part back to it.
    #[error("part contains itself: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
}

/// The parts of a project, by path.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Workspace {
    /// The parts, by path.
    pub parts: BTreeMap<String, Part>,
    /// The materials the parts are made of.
    pub library: crate::Library,
    /// What new drawings start from: the sheet templates.
    pub sheets: workpiece_ops_drawing::Templates,
    /// The properties the parts have, and what each is.
    pub schema: crate::Schema,
}

impl Workspace {
    /// A workspace holding `parts`.
    pub fn new(parts: BTreeMap<String, Part>) -> Self {
        Self {
            parts,
            library: crate::Library::default(),
            sheets: workpiece_ops_drawing::Templates::default(),
            schema: crate::Schema::default(),
        }
    }

    /// The part at `path`.
    pub fn part(&self, path: &str) -> Result<&Part, WorkspaceError> {
        self.parts
            .get(path)
            .ok_or_else(|| WorkspaceError::UnknownPart(path.into()))
    }

    /// What the parts the part at `path` places are called, by the names
    /// they are placed under: the title the placed part gives itself
    /// (its property `Title`), else its file's name — what a parts list
    /// says.
    pub fn placed_names(&self, path: &str) -> BTreeMap<String, String> {
        let Ok(part) = self.part(path) else {
            return BTreeMap::new();
        };
        let folder = std::path::Path::new(path).parent();
        part.program
            .iter()
            .filter_map(|op| {
                let (file, _) = op.placed()?;
                let beside = folder.map(|f| f.join(file).to_string_lossy().into_owned());
                let placed = self
                    .parts
                    .get(file)
                    .or_else(|| beside.and_then(|b| self.parts.get(&b)));
                let name = placed
                    .and_then(|p| p.properties.get("Title").filter(|t| !t.is_empty()).cloned())
                    .unwrap_or_else(|| {
                        std::path::Path::new(file)
                            .file_stem()
                            .map_or_else(|| file.to_owned(), |n| n.to_string_lossy().into_owned())
                    });
                Some((op.id().to_owned(), name))
            })
            .collect()
    }

    /// The part at `path`, for editing.
    pub fn part_mut(&mut self, path: &str) -> Result<&mut Part, WorkspaceError> {
        self.parts
            .get_mut(path)
            .ok_or_else(|| WorkspaceError::UnknownPart(path.into()))
    }

    /// The part at `path`, or the part a file there that is no part
    /// file stands for: a STEP file is a part whose one step imports it,
    /// so that it is placed, built and kept like any other.
    pub fn resolve(&self, path: &str) -> Result<Cow<'_, Part>, WorkspaceError> {
        if let Some(part) = self.parts.get(path) {
            return Ok(Cow::Borrowed(part));
        }
        if is_step(path) {
            return Ok(Cow::Owned(Part::new(vec![
                ImportStep {
                    id: "step".into(),
                    args: ImportStepArgs { path: path.into() },
                }
                .into(),
            ])));
        }
        Err(WorkspaceError::UnknownPart(path.into()))
    }

    /// Builds the part at `path`, and every part it places.
    pub fn build(&self, path: &str) -> anyhow::Result<Arc<World>> {
        self.build_where(path, &|_| true)
    }

    /// Builds the part at `path` up to (not including) step `steps`: the
    /// world its first `steps` operations make, as it is while step
    /// `steps` is edited. The parts it places are built whole.
    pub fn build_steps(&self, path: &str, steps: usize) -> anyhow::Result<Arc<World>> {
        self.build_where(path, &|i| i < steps)
    }

    /// Builds the part at `path` with only the steps whose positions
    /// `keep` takes. Leaving out a step that places a part always works:
    /// no step can refer into a placed part.
    pub fn build_where(
        &self,
        path: &str,
        keep: &dyn Fn(usize) -> bool,
    ) -> anyhow::Result<Arc<World>> {
        Ok(self.build_cached(path, keep)?.0)
    }

    /// [`build_where`](Self::build_where), also returning the part's
    /// cache as the build leaves it: what its operations worked out, to
    /// store with the part so the next build starts there.
    pub fn build_cached(
        &self,
        path: &str,
        keep: &dyn Fn(usize) -> bool,
    ) -> anyhow::Result<(Arc<World>, Cache)> {
        self.build_variant(path, None, keep)
    }

    /// [`build_cached`](Self::build_cached) of `variant` of the part (the
    /// part as it is by default, for `None`). A variant starts from the
    /// part's cache like the part itself; the cache it leaves is that
    /// variant's, not the part's.
    ///
    /// This builds everything anew, on this thread; an [`Engine`] that is
    /// kept builds only what changed.
    pub fn build_variant(
        &self,
        path: &str,
        variant: Option<&str>,
        keep: &dyn Fn(usize) -> bool,
    ) -> anyhow::Result<(Arc<World>, Cache)> {
        let built =
            Engine::inline().build(&Arc::new(self.clone()), path, variant, keep, &Cancel::new())?;
        Ok((built.world, built.cache))
    }
}

/// Whether `path` names a file that is no part file but is read as a
/// part: a STEP file, or a mesh (STL).
pub fn is_step(path: &str) -> bool {
    let lower = path.trim().to_ascii_lowercase();
    lower.ends_with(".step")
        || lower.ends_with(".stp")
        || workpiece_ops_step::is_mesh(path)
        || workpiece_ops_step::is_iges(path)
}

impl Parts for Workspace {
    fn variants(&self, path: &str) -> Vec<String> {
        self.parts
            .get(path)
            .map(|part| part.variants.iter().map(|v| v.name.clone()).collect())
            .unwrap_or_default()
    }

    fn materials(&self) -> Vec<String> {
        self.library.names()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnyOperation;
    use workpiece_ops_instances::{AddPart, AddPartArgs};

    /// A part placing each listed part, under its own path.
    fn places(paths: &[&str]) -> Part {
        let add = |path: &&str| -> AnyOperation {
            AddPart {
                id: (*path).into(),
                args: AddPartArgs {
                    part: (*path).into(),
                    variant: None,
                    hold: Default::default(),
                    bodies: Vec::new(),
                },
            }
            .into()
        };
        Part {
            program: paths.iter().map(add).collect(),
            ..Default::default()
        }
    }

    fn workspace(parts: &[(&str, &[&str])]) -> Workspace {
        Workspace::new(
            parts
                .iter()
                .map(|(path, placed)| ((*path).into(), places(placed)))
                .collect(),
        )
    }

    #[test]
    fn parts_are_built_into_their_own_world_and_shared() -> anyhow::Result<()> {
        let ws = workspace(&[
            ("bolt", &[]),
            ("plate", &["bolt"]),
            ("frame", &["plate", "bolt"]),
        ]);
        let frame = ws.build("frame")?;
        let plate = &frame.instances["plate"].world;
        assert!(plate.instances.contains_key("bolt"));
        assert!(!plate.instances.contains_key("plate"));
        // The bolt is built once and shared by both instances.
        assert!(Arc::ptr_eq(
            &frame.instances["bolt"].world,
            &plate.instances["bolt"].world
        ));
        Ok(())
    }

    #[test]
    fn cycles_and_unknown_parts_fail() {
        let ws = workspace(&[
            ("a", &["b"]),
            ("b", &["c"]),
            ("c", &["b"]),
            ("d", &["nope"]),
            ("loop", &["loop"]),
        ]);
        let cycle = |path: &str| {
            ws.build(path)
                .expect_err("cycle")
                .downcast_ref::<WorkspaceError>()
                .cloned()
        };
        assert_eq!(
            cycle("a"),
            Some(WorkspaceError::Cycle(vec![
                "b".into(),
                "c".into(),
                "b".into()
            ]))
        );
        assert_eq!(
            cycle("loop"),
            Some(WorkspaceError::Cycle(vec!["loop".into(), "loop".into()]))
        );
        assert_eq!(cycle("d"), Some(WorkspaceError::UnknownPart("nope".into())));
        assert_eq!(
            cycle("nope"),
            Some(WorkspaceError::UnknownPart("nope".into()))
        );
    }

    #[test]
    fn prefixes_build_the_first_steps() -> anyhow::Result<()> {
        let ws = workspace(&[("bolt", &[]), ("pair", &["bolt", "bolt2"]), ("bolt2", &[])]);
        assert!(ws.build_steps("pair", 0)?.instances.is_empty());
        assert_eq!(ws.build_steps("pair", 1)?.instances.len(), 1);
        assert_eq!(ws.build_steps("pair", 9)?.instances.len(), 2);
        // A part still may not contain itself.
        let ws = workspace(&[("loop", &["loop"])]);
        assert!(ws.build_steps("loop", 1).is_err());
        Ok(())
    }
}

#[cfg(test)]
mod variant_tests {
    use super::*;
    use crate::{AnyOperation, Parameter, Variant};
    use workpiece_core::math::Point3;
    use workpiece_core::world::ORIGIN;
    use workpiece_core::{Measure, Reference};
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};
    use workpiece_ops_instances::{AddPart, AddPartArgs};

    /// A block as wide as its `width`, twice as deep, in sizes S and L;
    /// and a part placing it as it is, and as L.
    fn workspace() -> Workspace {
        let no = BTreeMap::new();
        let formula = |text: &str| Measure::parse(text, &no);
        let block = Part {
            program: vec![
                AddBox {
                    id: "body".into(),
                    args: AddBoxArgs {
                        frame: Some(Reference::datum(ORIGIN)),
                        size: [formula("=width"), formula("=depth"), Measure::new(1.0)],
                    },
                }
                .into(),
            ],
            parameters: vec![
                Parameter {
                    name: "width".into(),
                    value: Measure::new(2.0),
                },
                Parameter {
                    name: "depth".into(),
                    value: formula("=width * 2"),
                },
            ],
            variants: ["S", "L"]
                .into_iter()
                .zip([1.0, 5.0])
                .map(|(name, width)| Variant {
                    name: name.into(),
                    values: BTreeMap::from([("width".to_owned(), Measure::new(width))]),
                })
                .collect(),
            ..Part::default()
        };
        let place = |id: &str, variant: Option<&str>| -> AnyOperation {
            AddPart {
                id: id.into(),
                args: AddPartArgs {
                    part: "block".into(),
                    variant: variant.map(str::to_owned),
                    hold: Default::default(),
                    bodies: Vec::new(),
                },
            }
            .into()
        };
        let pair = Part::new(vec![
            place("plain", None),
            place("large", Some("L")),
            place("big", Some("L")),
        ]);
        Workspace::new(BTreeMap::from([
            ("block".into(), block),
            ("pair".into(), pair),
        ]))
    }

    /// The largest x and y of the world's vertices.
    fn reach(world: &World) -> Point3 {
        let vertices = world.topo.vertices().iter().map(|(_, v)| v.point);
        vertices.fold(Point3::ZERO, Point3::max)
    }

    #[test]
    fn variants_build_with_their_own_parameters() -> anyhow::Result<()> {
        let ws = workspace();
        assert_eq!(ws.variants("block"), ["S", "L"]);
        let block = ws.part("block")?;
        assert_eq!(block.resolve(None)?["depth"], 4.0);
        assert_eq!(block.resolve(Some("L"))?["depth"], 10.0);
        assert!(block.resolve(Some("XL")).is_err());

        let pair = ws.build("pair")?;
        let plain = reach(&pair.instances["plain"].world);
        let large = reach(&pair.instances["large"].world);
        assert!(
            plain.abs_diff_eq(Point3::new(1.0, 2.0, 0.5), 1e-12),
            "{plain}"
        );
        assert!(
            large.abs_diff_eq(Point3::new(2.5, 5.0, 0.5), 1e-12),
            "{large}"
        );
        // The same variant is built once.
        assert!(Arc::ptr_eq(
            &pair.instances["large"].world,
            &pair.instances["big"].world
        ));
        assert!(!Arc::ptr_eq(
            &pair.instances["large"].world,
            &pair.instances["plain"].world
        ));
        // The part itself can be built as a variant, too.
        let (small, _) = ws.build_variant("block", Some("S"), &|_| true)?;
        assert!(reach(&small).abs_diff_eq(Point3::new(0.5, 1.0, 0.5), 1e-12));
        Ok(())
    }

    #[test]
    fn parts_with_tables_round_trip_through_text() -> anyhow::Result<()> {
        let ws = workspace();
        let block = ws.part("block")?;
        let text = block.to_text()?;
        assert!(text.contains("\"formula\": \"width * 2\""), "{text}");
        assert_eq!(&Part::from_text(&text)?, block);
        // Files from before parameters still read.
        let old = Part::from_text(r#"{"program": []}"#)?;
        assert!(old.parameters.is_empty() && old.variants.is_empty());
        Ok(())
    }
}

// Parts are built on worker threads.
const _: fn() = || {
    fn shared<T: Send + Sync>() {}
    shared::<Workspace>();
};
