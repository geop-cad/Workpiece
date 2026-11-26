use anyhow::Context;
use std::collections::BTreeMap;

use workpiece_core::{Builder, Cache, Measure, Parameters};

use crate::AnyOperation;

/// A named number of a part, for the formulas of its operations'
/// arguments.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Parameter {
    /// Its name, as formulas write it.
    pub name: String,
    /// Its value: a number, or a formula over the parameters before it.
    pub value: Measure,
}

/// A variant of a part: a row of its table, giving some of its parameters
/// other values.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Variant {
    /// Its name, as placing the part chooses it.
    pub name: String,
    /// The parameters it sets, by name; the others keep the part's own
    /// values.
    #[serde(default)]
    pub values: BTreeMap<String, Measure>,
}

/// A part: a program of operations. Applying it to an empty world gives
/// the part's geometry; [`Workspace::build`](crate::Workspace::build) does
/// that.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Part {
    /// The operations, in the order they are applied.
    pub program: Vec<AnyOperation>,
    /// The part's parameters, in order: each may use the ones before it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<Parameter>,
    /// The part's table: its variants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<Variant>,
    /// The unit of length its numbers are in.
    #[serde(default, skip_serializing_if = "workpiece_core::Unit::is_default")]
    pub unit: workpiece_core::Unit,
    /// What the part is made of, where nothing else is said: a
    /// material, by its name in the library (see
    /// [`Library`](crate::Library)). Steps give single bodies and faces
    /// other materials and colours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// What the part says of itself, by property (see
    /// [`Schema`](crate::Schema)): what it does not say is as the
    /// property is by default.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub properties: std::collections::BTreeMap<String, String>,
    /// The script the part was made of, where a script made it (the
    /// browser's assistant keeps the Python it wrote here): to read and
    /// copy, and to start a change from. Nothing builds from it; the
    /// part may have been changed by hand since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Where the text of what is said of the model stands on it, by the
    /// annotation's name, where the user stood it elsewhere than it
    /// stands by itself: how far from the point it is about (see
    /// `Annotation::origin`), so that it goes along when that moves.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub note_places: std::collections::BTreeMap<String, [f64; 3]>,
    /// Where the operations start solving from: what they worked out the
    /// last time the part was built (see [`Cache`]). An input of the
    /// build like the program: it is saved with the part, since from
    /// another cache the part could build to another solution.
    #[serde(default, skip_serializing_if = "Cache::is_empty")]
    pub cache: Cache,
    /// A small picture of the part, taken when it was last saved: what
    /// lists of parts show of it. It is no part of the part (see
    /// [`Pictured`](crate::Pictured)): none, for a part never pictured.
    #[serde(default, skip_serializing_if = "crate::Pictured::is_none")]
    pub preview: crate::Pictured,
}

impl Part {
    /// A part with `program`, never built.
    pub fn new(program: Vec<AnyOperation>) -> Self {
        Self {
            program,
            ..Self::default()
        }
    }

    /// The values of the parameters: of `variant`, or the part's own.
    /// Fails for a variant the part does not have, or a parameter whose
    /// formula cannot be valued (it may use the parameters before it).
    pub fn resolve(&self, variant: Option<&str>) -> anyhow::Result<Parameters> {
        let overrides = match variant {
            Some(name) => Some(
                &self
                    .variants
                    .iter()
                    .find(|v| v.name == name)
                    .with_context(|| format!("no variant {name:?}"))?
                    .values,
            ),
            None => None,
        };
        let mut values = Parameters::new();
        for parameter in &self.parameters {
            let measure = overrides
                .and_then(|o| o.get(&parameter.name))
                .unwrap_or(&parameter.value);
            let value = measure
                .resolve(&values)
                .with_context(|| format!("parameter {:?}", parameter.name))?;
            values.insert(parameter.name.clone(), value);
        }
        Ok(values)
    }

    /// Applies each operation in order to the world being built. Fails on
    /// the first operation that fails.
    pub fn apply(&self, builder: &mut dyn Builder) -> anyhow::Result<()> {
        self.apply_steps(builder, self.program.len())
    }

    /// Applies the first `steps` operations (all, if there are fewer).
    pub fn apply_steps(&self, builder: &mut dyn Builder, steps: usize) -> anyhow::Result<()> {
        self.apply_where(builder, &|i| i < steps)
    }

    /// Applies the operations whose positions `keep` takes, in order.
    pub fn apply_where(
        &self,
        builder: &mut dyn Builder,
        keep: &dyn Fn(usize) -> bool,
    ) -> anyhow::Result<()> {
        for op in self
            .program
            .iter()
            .enumerate()
            .filter(|(i, _)| keep(*i))
            .map(|(_, op)| op)
        {
            op.apply(builder)
                .with_context(|| format!("operation {:?}", op.id()))?;
        }
        Ok(())
    }

    /// What the part is, in a word: the same for two parts that build
    /// the same — whose steps, parameters, variants, unit and material
    /// are the same — whatever their caches hold. A preview says which
    /// part it is a picture of by it.
    pub fn stamp(&self) -> String {
        let mut stamp = crate::preview::Stamp::default();
        let what = (
            &self.program,
            &self.parameters,
            &self.variants,
            &self.unit,
            &self.material,
        );
        // (Writing to a hasher does not fail.)
        let _ = serde_json::to_writer(&mut stamp, &what);
        format!("{:016x}", stamp.0)
    }

    /// The part's preview, if it has one of the part as it is now.
    pub fn fresh_preview(&self) -> Option<&crate::Preview> {
        self.preview.0.as_ref().filter(|p| p.of == self.stamp())
    }

    /// The part as file contents (JSON).
    pub fn to_text(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// The part in file contents written by [`to_text`](Self::to_text).
    pub fn from_text(text: &str) -> anyhow::Result<Self> {
        // (Its steps are read by the operations there are.)
        crate::registry();
        Ok(serde_json::from_str(text)?)
    }

    /// Puts the part placed by the step `id` at `pose`: where it is is
    /// kept in the cache, not among the step's arguments.
    #[must_use]
    pub fn placing(mut self, id: &str, pose: &workpiece_core::Pose) -> Self {
        self.cache
            .replace(id, workpiece_ops_instances::assembly::kept(pose));
        self
    }

    /// The paths of the parts this part places, in order, without repeats:
    /// what has to be loaded with it.
    pub fn placed_parts(&self) -> Vec<String> {
        let mut parts: Vec<String> = Vec::new();
        for op in &self.program {
            if let Some((path, _)) = op.placed()
                && !parts.iter().any(|p| p == path)
            {
                parts.push(path.to_owned());
            }
        }
        parts
    }

    /// The operation with this id.
    pub fn operation(&self, id: &str) -> Option<&AnyOperation> {
        self.program.iter().find(|op| op.id() == id)
    }

    /// The operation with this id, for changing it.
    pub fn operation_mut(&mut self, id: &str) -> Option<&mut AnyOperation> {
        self.program.iter_mut().find(|op| op.id() == id)
    }

    /// The first operation that refers to something an operation at or
    /// after it makes, with that operation: `None` for a program in an
    /// order it can be built in.
    fn out_of_order(&self) -> Option<(&str, &str)> {
        self.program.iter().enumerate().find_map(|(i, op)| {
            let references = op.references();
            let maker = self.program[i..]
                .iter()
                .find(|later| references.iter().any(|s| s.made_by(later.id())))?;
            Some((op.id(), maker.id()))
        })
    }

    /// Moves the operation `id` to position `to` of the program (the
    /// last, if there are fewer). Fails if that puts an operation before
    /// one it refers to.
    pub fn move_operation(&mut self, id: &str, to: usize) -> anyhow::Result<()> {
        let from = self
            .program
            .iter()
            .position(|op| op.id() == id)
            .with_context(|| format!("no operation {id:?}"))?;
        let op = self.program.remove(from);
        let to = to.min(self.program.len());
        self.program.insert(to, op);
        if let Some((user, maker)) = self.out_of_order() {
            anyhow::bail!("{user} uses {maker}, so it has to come after it");
        }
        Ok(())
    }

    /// Removes the operation `id`, with what it left in the cache. Fails
    /// if another operation refers to something it makes.
    pub fn remove_operation(&mut self, id: &str) -> anyhow::Result<()> {
        let at = self
            .program
            .iter()
            .position(|op| op.id() == id)
            .with_context(|| format!("no operation {id:?}"))?;
        let users: Vec<&str> = self
            .program
            .iter()
            .filter(|op| op.id() != id && op.references().iter().any(|s| s.made_by(id)))
            .map(AnyOperation::id)
            .collect();
        anyhow::ensure!(
            users.is_empty(),
            "{id} is used by {}: remove or repoint {} first",
            users.join(", "),
            if users.len() == 1 { "it" } else { "them" }
        );
        self.program.remove(at);
        self.cache.replace(id, []);
        Ok(())
    }

    /// Renames the operation `id` to `name`; the operations that refer to
    /// what it makes follow, and its cache goes with it. Fails for a name
    /// that is empty, has a `/` (names of what an operation makes are
    /// `"{id}/…"`), or is taken.
    pub fn rename_operation(&mut self, id: &str, name: &str) -> anyhow::Result<()> {
        let name = name.trim();
        if name == id {
            return Ok(());
        }
        anyhow::ensure!(!name.is_empty(), "a step needs a name");
        anyhow::ensure!(!name.contains('/'), "a step's name cannot have a / in it");
        anyhow::ensure!(
            self.operation(name).is_none(),
            "there is a step called {name:?} already"
        );
        self.operation_mut(id)
            .with_context(|| format!("no operation {id:?}"))?
            .set_id(name);
        for op in &mut self.program {
            op.follow_rename(id, name);
        }
        self.cache.rename(id, name);
        Ok(())
    }

    /// An id not used by any operation yet: `prefix` followed by the
    /// smallest free number from 1.
    pub fn fresh_id(&self, prefix: &str) -> String {
        (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|id| self.operation(id).is_none())
            .expect("unbounded range")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workspace;
    use std::collections::BTreeMap;
    use workpiece_core::math::Point3;
    use workpiece_core::world::Datum;
    use workpiece_core::{Pose, Reference, Widget, World};
    use workpiece_ops_datum::{AddFrame, AddPlane, AddPlaneArgs, PlaneMode};
    use workpiece_ops_instances::{AddPart, AddPartArgs};

    /// A frame, and a plane offset from that frame's XY plane.
    fn part() -> Part {
        Part {
            program: vec![
                AddFrame::at("frame", Pose::from_translation(Point3::Z)).into(),
                AddPlane {
                    id: "plane".into(),
                    args: AddPlaneArgs {
                        mode: PlaneMode::Offset,
                        references: vec![
                            (Reference::<()>::frame_axis("frame", workpiece_core::Axis::Z).source)
                                .into(),
                        ],
                        distance: 2.0,
                        ..AddPlaneArgs::default()
                    },
                }
                .into(),
            ],
            ..Default::default()
        }
    }

    /// A part placing `part()` twice.
    fn pair() -> Part {
        let add = |id: &str| -> AnyOperation {
            AddPart {
                id: id.into(),
                args: AddPartArgs {
                    part: "single".into(),
                    variant: None,
                    hold: Default::default(),
                    bodies: Vec::new(),
                },
            }
            .into()
        };
        let at = |x: f64| Pose::from_translation(Point3::new(x, 0.0, 0.0));
        Part::new(vec![add("left"), add("right")])
            .placing("left", &at(-1.0))
            .placing("right", &at(1.0))
    }

    #[test]
    fn building_applies_the_program_in_order() -> anyhow::Result<()> {
        let ws = Workspace::new(BTreeMap::from([("single".into(), part())]));
        let world = ws.build("single")?;
        let Some(Datum::Plane(p)) = world.datums.get("plane") else {
            anyhow::bail!("no plane");
        };
        assert!(p.origin().abs_diff_eq(Point3::new(0.0, 0.0, 3.0), 1e-12));

        let mut broken = part();
        broken.program.swap(0, 1);
        let ws = Workspace::new(BTreeMap::from([("single".into(), broken)]));
        let err = ws.build("single").expect_err("plane needs the frame first");
        assert!(format!("{err:#}").contains("operation \"plane\""));
        Ok(())
    }

    #[test]
    fn parts_place_other_parts() -> anyhow::Result<()> {
        let ws = Workspace::new(BTreeMap::from([
            ("single".into(), part()),
            ("pair".into(), pair()),
        ]));
        let pair = ws.build("pair")?;
        let right = &pair.instances["right"];
        assert!(right.world.datums.contains_key("plane"));
        assert!(right.pose.translation().abs_diff_eq(Point3::X, 1e-12));
        Ok(())
    }

    #[test]
    fn edits_produce_the_edited_operation() -> anyhow::Result<()> {
        let part = part();
        let op = &part.program[0];
        let world = std::sync::Arc::new(World::new());
        let cache = workpiece_core::Cache::default();
        let context = workpiece_core::EditContext::standalone(&world, &cache);
        let Widget::VBox(fields) = op.visualize_edit(&context, None).dialog else {
            anyhow::bail!("expected the fields");
        };
        let Some(Widget::PoseInput(pose)) = fields.children.into_iter().nth(2) else {
            anyhow::bail!("expected the pose fields");
        };
        let edit = pose.set_position(workpiece_core::Axis::X, 5.0);
        let edited = edit.and_then(|e| e.op);
        let Some(frame) = edited.as_ref().and_then(|op| op.downcast_ref::<AddFrame>()) else {
            anyhow::bail!("expected the edited frame");
        };
        assert_eq!(frame.id, "frame");
        assert!(
            frame
                .args
                .pose
                .translation()
                .abs_diff_eq(Point3::new(5.0, 0.0, 1.0), 1e-12)
        );
        Ok(())
    }

    #[test]
    fn fresh_ids_skip_used_ones() {
        assert_eq!(part().fresh_id("frame"), "frame1");
        let mut p = part();
        p.program
            .push(AddFrame::at("frame1", Pose::IDENTITY).into());
        assert_eq!(p.fresh_id("frame"), "frame2");
    }

    #[test]
    fn parts_round_trip_through_text() -> anyhow::Result<()> {
        let mut p = part();
        p.program.push(
            AddPart {
                id: "bolt1".into(),
                args: AddPartArgs {
                    part: "bolt.wpc".into(),
                    variant: None,
                    hold: Default::default(),
                    bodies: Vec::new(),
                },
            }
            .into(),
        );
        let at = Pose::from_translation(Point3::new(1.0, 2.0, 3.0));
        let p = p.placing("bolt1", &at);
        let text = p.to_text()?;
        let back = Part::from_text(&text)?;
        assert_eq!(back.placed_parts(), ["bolt.wpc"]);
        // Where the part is comes back with the part's cache.
        let placed = workpiece_ops_instances::assembly::placed(&back.cache, "bolt1");
        assert!(
            placed
                .translation()
                .abs_diff_eq(Point3::new(1.0, 2.0, 3.0), 1e-12)
        );
        assert_eq!(back.program[..2], p.program[..2]);
        Ok(())
    }
}
