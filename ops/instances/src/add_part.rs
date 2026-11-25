//! [`AddPart`]: another part, built and placed at a pose.

use workpiece_core::math::Point3;
use workpiece_core::widgets::{Choice, FileInput, VBox};
use workpiece_core::{Builder, EditVisualization, Operation};

/// The extension of part files.
pub const PART_EXTENSION: &str = "workpiece";

/// Which part to place, and where.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    workpiece_core::Arguments,
)]
pub struct AddPartArgs {
    /// The placed part's path in the workspace.
    #[arg(with = "fields_of")]
    pub part: String,
    /// Which variant of the part to place (a row of its table); `None`
    /// for the part as it is by default.
    #[serde(default)]
    #[arg(hidden)]
    pub variant: Option<String>,
    /// How it is held: free, fixed, or by a joint between a reference
    /// on what is in the assembly already and one of its own.
    #[serde(default)]
    #[arg(hidden)]
    pub hold: crate::placing::Hold,
    /// Which of the part's bodies come along, by name; none named, all
    /// of them. (One part's bodies are made parts of their own so: a
    /// part that places it and takes one.)
    #[serde(default)]
    #[arg(hidden)]
    pub bodies: Vec<String>,
}

impl AddPartArgs {
    /// `world`, the part built, with only the bodies that come along.
    pub fn taken(
        &self,
        world: std::sync::Arc<workpiece_core::World>,
    ) -> anyhow::Result<std::sync::Arc<workpiece_core::World>> {
        if self.bodies.is_empty() {
            return Ok(world);
        }
        let topo = &world.topo;
        let named = |id| topo.name_of(id).map(str::to_owned);
        let all: Vec<_> = topo
            .bodies()
            .iter()
            .map(|(id, _)| (id, named(id)))
            .collect();
        if let Some(missing) = self
            .bodies
            .iter()
            .find(|b| !all.iter().any(|(_, n)| n.as_ref() == Some(*b)))
        {
            anyhow::bail!("the part {} has no body {missing:?}", self.part);
        }
        let mut only = (*world).clone();
        let mut gone = Vec::new();
        for (id, name) in all {
            if !name.as_ref().is_some_and(|n| self.bodies.contains(n)) {
                workpiece_boolean::remove(&mut only.topo, id);
                gone.extend(name);
            }
        }
        // What is said of what is gone goes with it: looks and threads
        // of its faces and of it, its mass, and what is noted on it.
        let topo = &only.topo;
        only.looks.retain(|name, _| topo.lookup(name).is_some());
        only.threads.retain(|_, t| topo.lookup(&t.face).is_some());
        for mass in only.masses.values_mut() {
            mass.bodies.retain(|b| !gone.contains(&b.body));
        }
        only.masses.retain(|_, m| !m.bodies.is_empty());
        let reach = reach(topo);
        only.annotations
            .retain(|_, a| feature_points(&a.note).iter().all(|p| within(reach, *p)));
        // It is another world than the part's whole: what is kept of
        // a world by its stamp (how it is drawn, picked) is not this.
        if only.stamp != 0 {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (only.stamp, &self.bodies).hash(&mut h);
            only.stamp = h.finish().max(1);
        }
        Ok(std::sync::Arc::new(only))
    }
}

/// The box around the bodies of `topo` (around their faces' control
/// points and their vertices), if it has any.
fn reach(topo: &workpiece_core::world::topo::Topo) -> Option<(Point3, Point3)> {
    let mut points: Vec<Point3> = topo.vertices().iter().map(|(_, v)| v.point).collect();
    for (_, face) in topo.faces().iter() {
        if let Some(surface) = topo.surface(face.surface) {
            points.extend(surface.control_points().iter().copied());
        }
    }
    let first = *points.first()?;
    Some(
        points
            .iter()
            .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))),
    )
}

/// Whether `p` is in the box `reach` (give or take a little).
fn within(reach: Option<(Point3, Point3)>, p: Point3) -> bool {
    reach.is_some_and(|(lo, hi)| {
        let slack = 1e-6 * (1.0 + (hi - lo).length());
        p.cmpge(lo - Point3::splat(slack)).all() && p.cmple(hi + Point3::splat(slack)).all()
    })
}

/// The points of the model an annotation is of (not where its text
/// stands): what it is noted on lies there.
fn feature_points(note: &workpiece_core::world::Note) -> Vec<Point3> {
    use workpiece_core::world::{Measured, Note};
    let of = |m: &Measured| match *m {
        Measured::Length { from, to, .. } => vec![from, to],
        Measured::Radial { centre, .. }
        | Measured::Diameter { centre, .. }
        | Measured::Radius { centre, .. } => vec![centre],
        Measured::Angle { at, .. } => vec![at],
    };
    match note {
        Note::Dimension { measured, .. } => of(measured),
        Note::Datum { on, .. } | Note::Geometric { on, .. } => vec![*on],
        Note::Hole { centre, .. } => vec![*centre],
    }
}

/// Builds another part of the workspace and places it, as an instance
/// named after the operation's id, at a pose.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddPart {
    /// The operation's id; also the new instance's name.
    pub id: String,
    /// The part and its pose.
    pub args: AddPartArgs,
}

impl Operation for AddPart {
    type Args = AddPartArgs;
    type EditState = ();

    fn settles(&self) -> bool {
        true
    }

    fn placed(&self) -> Option<(&str, Option<&str>)> {
        Some((&self.args.part, self.args.variant.as_deref()))
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &AddPartArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut AddPartArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: what it makes is put in by the
        // builder.)
        let mut out = workpiece_boolean::Out::default();
        let world = builder.build(&self.args.part, self.args.variant.as_deref())?;
        let world = self.args.taken(world)?;
        crate::placing::place(&mut out.made, builder, &self.id, world, &self.args.hold)?;
        Ok(out.made)
    }

    fn visualize_edit(
        &self,
        context: &workpiece_core::EditContext<'_>,
        (): &(),
    ) -> EditVisualization<workpiece_core::Edit<AddPartArgs, ()>> {
        // The dialog chooses the part's file and its variant and edits its
        // pose; the gizmo moves and turns it.
        let args = self.args.clone();
        // The fields the arguments give; where it is, and the gizmo, are
        // the cache's.
        let fields = workpiece_core::Arguments::fields(&args, context);
        let (place, handles) = crate::placing::pose_edit(&self.id, context, &self.args.hold);
        let mut all: Vec<workpiece_core::Widget<workpiece_core::Edit<AddPartArgs, ()>>> = fields
            .into_iter()
            .map(|field| field.map(workpiece_core::Edit::args))
            .collect();
        all.extend(place);
        EditVisualization::new(VBox::new(all), handles)
    }

    fn summary(&self) -> String {
        let op = self;
        let fixed = op.args.hold.constraint.said();
        let taken = match op.args.bodies.len() {
            0 => String::new(),
            _ => format!(" ({})", op.args.bodies.join(", ")),
        };
        match &op.args.variant {
            Some(variant) => format!("places {} [{variant}]{taken}{fixed}", op.args.part),
            None => format!("places {}{taken}{fixed}", op.args.part),
        }
    }
}

/// The fields that say what part it is, which of its variants, and
/// how it is held: its two references and the constraint between them.
fn fields_of(
    args: &AddPartArgs,
    context: &workpiece_core::EditContext<'_>,
) -> Vec<workpiece_core::Widget<AddPartArgs>> {
    let args = args.clone();
    let with_part = {
        let args = args.clone();
        // Another part has other variants.
        move |part| AddPartArgs {
            part,
            variant: None,
            bodies: Vec::new(),
            ..args.clone()
        }
    };
    let mut fields: Vec<workpiece_core::Widget<AddPartArgs>> =
        vec![FileInput::new("Part", args.part.clone(), PART_EXTENSION, with_part).into()];
    // The part as it is by default, or one of its variants.
    let variants = context.parts.variants(&args.part);
    if !variants.is_empty() || args.variant.is_some() {
        let mut options = vec!["Default".to_owned()];
        options.extend(variants.iter().cloned());
        let known = args
            .variant
            .as_ref()
            .map(|v| variants.iter().position(|o| o == v));
        let selected = match known {
            None => 0,
            Some(Some(i)) => i + 1,
            // A variant the part no longer has still shows.
            Some(None) => {
                options.push(args.variant.clone().unwrap_or_default());
                options.len() - 1
            }
        };
        let with_variant = {
            let (args, options) = (args.clone(), options.clone());
            move |i: usize| AddPartArgs {
                variant: (i > 0).then(|| options[i].clone()),
                ..args.clone()
            }
        };
        fields.push(Choice::new("Variant", options, selected, with_variant).into());
    }
    // Which of its bodies come along, each ticked or not (where it has
    // more than one): all of them ticked is none named.
    let bodies = context.parts.bodies(&args.part, args.variant.as_deref());
    if bodies.len() > 1 {
        let taken = match args.bodies.len() {
            0 => "all".to_owned(),
            n => format!("{n} of {}", bodies.len()),
        };
        let label = workpiece_core::widgets::Label::new("Bodies", taken, Default::default());
        fields.push(label.into());
        for body in &bodies {
            let taken = args.bodies.is_empty() || args.bodies.contains(body);
            let tick = {
                let (args, bodies, body) = (args.clone(), bodies.clone(), body.clone());
                move |i: usize| {
                    let was: Vec<String> = match args.bodies.is_empty() {
                        true => bodies.clone(),
                        false => args.bodies.clone(),
                    };
                    let now: Vec<String> = bodies
                        .iter()
                        .filter(|b| match **b == body {
                            true => i == 0,
                            false => was.contains(b),
                        })
                        .cloned()
                        .collect();
                    AddPartArgs {
                        bodies: if now.len() == bodies.len() {
                            Vec::new()
                        } else {
                            now
                        },
                        ..args.clone()
                    }
                }
            };
            let options = vec!["Taken".to_owned(), "Left out".to_owned()];
            fields.push(Choice::new(body.clone(), options, usize::from(!taken), tick).into());
        }
    }
    // How it is held: a reference on what is there, one of its own,
    // and the constraint between them.
    for field in crate::placing::hold_fields(&args.hold, context.step, None) {
        let args = args.clone();
        fields.push(field.map(move |hold| AddPartArgs {
            hold,
            ..args.clone()
        }));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use workpiece_core::handles::Handle;
    use workpiece_core::math::Point3;
    use workpiece_core::{Pose, World};

    /// A builder that knows one part, `bolt`, with an empty world.
    struct OneBolt(World, workpiece_core::Cache);

    impl Builder for OneBolt {
        fn world(&self) -> &World {
            &self.0
        }

        fn world_mut(&mut self) -> &mut World {
            &mut self.0
        }

        fn cache(&self) -> &workpiece_core::Cache {
            &self.1
        }

        fn cache_mut(&mut self) -> &mut workpiece_core::Cache {
            &mut self.1
        }

        fn parameters(&self) -> &workpiece_core::Parameters {
            &workpiece_core::measure::NO_PARAMETERS
        }

        fn build(&self, path: &str, _: Option<&str>) -> anyhow::Result<Arc<World>> {
            anyhow::ensure!(path == "bolt", "no part {path:?}");
            Ok(Arc::new(World::new()))
        }
    }

    fn add(id: &str, part: &str) -> AddPart {
        AddPart {
            id: id.into(),
            args: AddPartArgs {
                part: part.into(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn parts_are_placed_where_the_cache_has_them() -> anyhow::Result<()> {
        let mut b = OneBolt(World::new(), Default::default());
        let right = Pose::from_translation(Point3::X);
        b.1.replace("right", crate::assembly::kept(&right));
        add("left", "bolt").apply(&mut b)?;
        add("right", "bolt").apply(&mut b)?;
        assert_eq!(b.0.instances.len(), 2);
        assert_eq!(b.0.instances["left"].pose, Pose::IDENTITY);
        let placed = &b.0.instances["right"];
        assert!(placed.pose.translation().abs_diff_eq(Point3::X, 1e-12));

        assert!(add("right", "bolt").apply(&mut b).is_err());
        assert!(add("other", "nope").apply(&mut b).is_err());
        assert_eq!(b.0.instances.len(), 2);
        Ok(())
    }

    #[test]
    fn what_is_said_of_the_bodies_left_out_stays_behind() -> anyhow::Result<()> {
        use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};
        let mut b = OneBolt(World::new(), Default::default());
        for (id, size) in [("a", 1.0), ("b", 2.0)] {
            let args = AddBoxArgs {
                frame: Some(workpiece_core::Reference::datum("origin")),
                size: [size; 3].map(workpiece_core::Measure::new),
            };
            AddBox {
                id: id.into(),
                args,
            }
            .apply(&mut b)?;
        }
        let look = workpiece_core::world::Look {
            colour: Some([255, 0, 0]),
            ..Default::default()
        };
        b.0.looks.insert("a".into(), look.clone());
        b.0.looks.insert("b".into(), look);
        let args = AddPartArgs {
            part: "pair".into(),
            bodies: vec!["a".into()],
            ..Default::default()
        };
        let only = args.taken(Arc::new(b.0.clone()))?;
        assert_eq!(only.topo.bodies().len(), 1);
        assert_eq!(only.looks.keys().collect::<Vec<_>>(), ["a"]);
        // A body it does not have is said so.
        let wrong = AddPartArgs {
            bodies: vec!["c".into()],
            ..args
        };
        assert!(wrong.taken(Arc::new(b.0)).is_err());
        Ok(())
    }

    #[test]
    fn the_gizmo_moves_the_part_in_the_cache() -> anyhow::Result<()> {
        let op = add("b", "bolt");
        let world = Arc::new(World::new());
        let cache = workpiece_core::Cache::default();
        let context = workpiece_core::EditContext::standalone(&world, &cache);
        let edit = op.visualize_edit(&context, &());
        let Some(Handle::Gizmo(g)) = edit.handles.get("pose") else {
            anyhow::bail!("expected a gizmo");
        };
        let moved = Pose::from_translation(Point3::Y);
        let edit = (g.on_drag)(moved);
        // The arguments stay; what the step keeps is where the part is.
        assert!(edit.args.is_none());
        let mut cache = workpiece_core::Cache::default();
        cache.replace("b", edit.kept.expect("where the part is"));
        assert_eq!(crate::assembly::placed(&cache, "b"), moved);
        Ok(())
    }
}
