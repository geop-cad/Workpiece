//! [`CutParts`]: a cut through the parts of an assembly once they are
//! put together.

use std::sync::Arc;

use anyhow::Context;
use shapecraft_boolean::{Op, Solid, combine, read, remove, write};
use shapecraft_core::math::Point3;
use shapecraft_core::world::topo::BodyKind;
use shapecraft_core::{Builder, Operation, Pose, World};

/// The bodies that are cut away from the placed parts.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    shapecraft_core::Arguments,
)]
pub struct CutPartsArgs {
    /// Bodies of the assembly itself: what they take up is cut out of
    /// every placed part they run into, and they go.
    #[arg(label = "Cut away", required)]
    pub tools: Vec<shapecraft_core::Reference<shapecraft_core::world::topo::BodyId>>,
}

/// Cuts bodies of the assembly out of the parts placed in it: a hole
/// drilled through several parts at once, a slot milled across them.
/// The parts are cut as they are placed here; their own files stay as
/// they are.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CutParts {
    /// The operation's id.
    pub id: String,
    /// What is cut away.
    pub args: CutPartsArgs,
}

/// `world` (placed at `pose`) with `tools` cut out of its solid bodies
/// and of what is placed in it; `None` if they run into none of it.
fn cut(world: &World, pose: Pose, tools: &[Solid]) -> anyhow::Result<Option<World>> {
    let back = pose.inverse();
    let mut out = None;
    let bodies: Vec<_> = world
        .topo
        .bodies()
        .iter()
        .filter(|(_, b)| b.kind == BodyKind::Solid)
        .map(|(id, _)| id)
        .collect();
    for body in bodies {
        let Some(name) = world.topo.name_of(body).map(str::to_owned) else {
            continue;
        };
        let was = read(&world.topo, body)?;
        let (lo, hi) = was.extent();
        let mut now = was.clone();
        let mut changed = false;
        for tool in tools {
            // (The tool, where this part's own coordinates have it.)
            let tool = tool.moved(&|p| back.transform_point(p))?;
            let (tool_lo, tool_hi) = tool.extent();
            if lo.cmpgt(tool_hi).any() || tool_lo.cmpgt(hi).any() || now.is_empty() {
                continue;
            }
            now = combine(Op::Cut, &now, &tool)
                .with_context(|| format!("cutting {name}"))?
                .solid;
            changed = true;
        }
        if changed {
            let out = out.get_or_insert_with(|| world.clone());
            remove(&mut out.topo, body);
            if !now.is_empty() {
                write(&mut out.topo, &now, &name)?;
            }
        }
    }
    for (name, instance) in &world.instances {
        if let Some(inner) = cut(&instance.world, pose * instance.pose, tools)? {
            let out = out.get_or_insert_with(|| world.clone());
            if let Some(placed) = out.instances.get_mut(name) {
                placed.world = Arc::new(inner);
            }
        }
    }
    Ok(out)
}

impl Operation for CutParts {
    type Args = CutPartsArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &CutPartsArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut CutPartsArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: what it makes is put in by the
        // builder.)
        let mut out = shapecraft_boolean::Out::default();
        anyhow::ensure!(!self.args.tools.is_empty(), "pick the bodies to cut away");
        // Of every placed part, how far it reaches is read; the part
        // itself only where what is cut away could run into it.
        let world = builder.tracked();
        let mut tools = Vec::new();
        let mut used = Vec::new();
        for tool in &self.args.tools {
            let name = tool.name();
            let body = world
                .topo
                .resolve(name)
                .with_context(|| format!("there is no body {name:?} in the assembly itself"))?;
            tools.push(read(&world.topo, body)?);
            used.push(body);
        }
        let mut any = false;
        let reaches: Vec<_> = tools.iter().map(Solid::extent).collect();
        for name in builder.placed_names() {
            let near = builder.placed_reach(&name).is_some_and(|(lo, hi)| {
                let apart = |(tool_lo, tool_hi): &(Point3, Point3)| {
                    lo.cmpgt(*tool_hi).any() || tool_lo.cmpgt(hi).any()
                };
                !reaches.iter().all(apart)
            });
            if !near {
                continue;
            }
            let Some(instance) = builder.placed(&name) else {
                continue;
            };
            if let Some(inner) = cut(&instance.world, instance.pose, &tools)
                .with_context(|| format!("cutting through {name}"))?
            {
                any = true;
                let cut = shapecraft_core::world::Instance {
                    world: Arc::new(inner),
                    ..instance.clone()
                };
                out.made.instances.push((name, Some(cut)));
            }
        }
        anyhow::ensure!(any, "what is cut away runs into none of the placed parts");
        for body in used {
            out.remove(&world.topo, body);
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        match self.args.tools.len() {
            1 => "1 body cut out of the parts".to_owned(),
            n => format!("{n} bodies cut out of the parts"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::Standalone;
    use shapecraft_core::math::Point3;
    use shapecraft_core::world::Instance;

    #[test]
    fn a_body_is_cut_out_of_the_parts_it_runs_into() -> anyhow::Result<()> {
        // Two 2 × 2 × 2 blocks side by side along x, a third far off;
        // a 4 × 1 × 1 bar through the first two takes 2 out of each.
        let axes = [Point3::X, Point3::Y, Point3::Z];
        let mut part = World::new();
        let block = shapecraft_boolean::block("b", Point3::ZERO, axes, [2.0; 3])?;
        write(&mut part.topo, &block, "b")?;
        let part = Arc::new(part);
        let mut b = Standalone::new();
        for (name, x) in [("left", 0.0), ("right", 2.0), ("far", 20.0)] {
            b.world.instances.insert(
                name.into(),
                Instance {
                    pose: Pose::from_translation(Point3::new(x, 0.0, 0.0)),
                    world: part.clone(),
                    fixed: true,
                },
            );
        }
        let bar =
            shapecraft_boolean::block("bar", Point3::new(0.0, 0.5, 0.5), axes, [4.0, 1.0, 1.0])?;
        write(&mut b.world.topo, &bar, "bar")?;
        let op = CutParts {
            id: "slot".into(),
            args: CutPartsArgs {
                tools: vec![shapecraft_core::Source::Topology("bar".into()).into()],
            },
        };
        op.apply(&mut b)?;
        let volume = |name: &str| -> anyhow::Result<f64> {
            let world = &b.world.instances[name].world;
            assert_eq!(world.topo.validate(), Ok(()));
            let body = world.topo.bodies().ids().next().expect("a body");
            Ok(shapecraft_boolean::measure(&read(&world.topo, body)?).volume)
        };
        assert!((volume("left")? - 6.0).abs() < 1e-9);
        assert!((volume("right")? - 6.0).abs() < 1e-9);
        assert!((volume("far")? - 8.0).abs() < 1e-9);
        // The far one is the part as it was; the bar is used up.
        assert!(Arc::ptr_eq(&b.world.instances["far"].world, &part));
        assert_eq!(b.world.topo.bodies().iter().count(), 0);
        Ok(())
    }
}
