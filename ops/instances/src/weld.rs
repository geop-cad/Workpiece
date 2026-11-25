//! [`Weld`]: a bead along edges where the parts of an assembly meet.

use anyhow::Context;
use shapecraft_boolean::{Op, Solid, combine, read};
use shapecraft_core::math::Point2;
use shapecraft_core::world::topo::{BodyKind, Curve3, EdgeId};
use shapecraft_core::{Builder, Measure, Operation, Pose, Source, World};
use shapecraft_shape::swept::{Outline, circle, steady, swept};

/// The edges welded along, and how big the bead is.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct WeldArgs {
    /// The edges the bead runs along: of the placed parts, where they
    /// meet.
    #[arg(label = "Along", required)]
    pub edges: Vec<shapecraft_core::Reference<shapecraft_core::world::topo::EdgeId>>,
    /// The bead's size: how far it reaches from the edge.
    pub size: Measure,
}

impl Default for WeldArgs {
    fn default() -> Self {
        Self {
            edges: Vec::new(),
            size: Measure::new(3.0),
        }
    }
}

/// Lays a weld bead along edges of the placed parts: a round of the
/// bead's size about each edge, less what the parts take up — in the
/// corner between two plates, a quarter round filling it. The beads are
/// bodies of the assembly, named after the operation's id.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Weld {
    /// The operation's id; also the bead's name.
    pub id: String,
    /// The edges, and the bead's size.
    pub args: WeldArgs,
}

/// The edge `source` names, as a curve where `pose` (and the parts it
/// is placed in) put it.
fn edge_curve(world: &World, source: &Source, pose: Pose) -> anyhow::Result<Curve3> {
    match source {
        Source::Placed { instance, source } => {
            let placed = world
                .instances
                .get(instance)
                .with_context(|| format!("there is no placed part {instance:?}"))?;
            edge_curve(&placed.world, source, pose * placed.pose)
        }
        Source::Topology(name) => {
            let topo = &world.topo;
            let edge = topo
                .resolve::<EdgeId>(name)
                .and_then(|e| topo.edge(e))
                .with_context(|| format!("there is no edge {name:?}"))?;
            let curve = topo.curve(edge.curve).context("an edge without a curve")?;
            let piece = curve.trimmed(edge.t0, edge.t1)?;
            Ok(Curve3::new(
                piece.degree(),
                piece.knots().to_vec(),
                piece
                    .control_points()
                    .iter()
                    .map(|&p| pose.transform_point(p))
                    .collect(),
                Some(piece.weights().to_vec()),
            )?)
        }
        _ => anyhow::bail!("a bead runs along edges"),
    }
}

/// The solid bodies of everything placed in `world`, where they are.
fn placed_solids(world: &World, pose: Pose, own: bool, out: &mut Vec<Solid>) -> anyhow::Result<()> {
    if !own {
        for (id, body) in world.topo.bodies().iter() {
            if body.kind == BodyKind::Solid {
                out.push(read(&world.topo, id)?.moved(&|p| pose.transform_point(p))?);
            }
        }
    }
    for instance in world.instances.values() {
        placed_solids(&instance.world, pose * instance.pose, false, out)?;
    }
    Ok(())
}

impl Operation for Weld {
    type Args = WeldArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &WeldArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut WeldArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: what it makes is put in by the
        // builder.)
        let mut out = shapecraft_boolean::Out::default();
        anyhow::ensure!(!self.args.edges.is_empty(), "pick the edges to weld along");
        let size = (*builder).measure(&self.args.size, "the bead's size")?;
        anyhow::ensure!(size > 0.0, "a bead has some size");
        // The parts the edges are on are read for the edges; of every
        // part, how far it reaches; and the parts themselves only where
        // a bead could touch them.
        let on = self.args.edges.iter().filter_map(|e| e.instance());
        let world = builder.with_placed(on);
        let names = builder.placed_names();
        let mut beads = Vec::new();
        for (k, source) in self.args.edges.iter().enumerate() {
            let path = edge_curve(world, source, Pose::IDENTITY)?;
            let range = path.domain();
            let (from, way) = path.derivative_at(range.0);
            let up = way.normalize_or_zero().any_orthonormal_vector();
            let reach = from.distance(path.point_at(range.1)) + size;
            let (sides, points) = circle(Point2::ZERO, size);
            let at = steady(&points);
            let id = match k {
                0 => self.id.clone(),
                k => format!("{}/{}", self.id, k + 1),
            };
            let outline = Outline { sides, at: &at };
            let mut bead = swept(&id, &outline, &path, range, up, reach * 4.0)?;
            let (lo, hi) = bead.extent();
            let mut parts = Vec::new();
            for name in &names {
                let near = builder
                    .placed_reach(name)
                    .is_some_and(|(part_lo, part_hi)| {
                        !(lo.cmpgt(part_hi).any() || part_lo.cmpgt(hi).any())
                    });
                if !near {
                    continue;
                }
                if let Some(placed) = builder.placed(name) {
                    placed_solids(&placed.world, placed.pose, false, &mut parts)?;
                }
            }
            for part in &parts {
                let (part_lo, part_hi) = part.extent();
                if lo.cmpgt(part_hi).any() || part_lo.cmpgt(hi).any() || bead.is_empty() {
                    continue;
                }
                bead = combine(Op::Cut, &bead, part)
                    .context("fitting the bead to the parts")?
                    .solid;
            }
            anyhow::ensure!(!bead.is_empty(), "the bead would be inside the parts");
            beads.push((id, bead));
        }
        for (id, bead) in beads {
            out.write(&bead, &id);
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        format!("along {} edges, {}", self.args.edges.len(), self.args.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_core::Standalone;
    use shapecraft_core::math::Point3;
    use shapecraft_core::world::Instance;
    use std::sync::Arc;

    #[test]
    fn a_bead_fills_the_corner_between_two_parts() -> anyhow::Result<()> {
        // A plate 4 × 4 × 1 and a wall 1 × 4 × 3 standing on it: a bead
        // of 0.5 along the wall's foot is a quarter round, 4 long.
        let axes = [Point3::X, Point3::Y, Point3::Z];
        let part = |size: [f64; 3]| -> anyhow::Result<Arc<World>> {
            let mut world = World::new();
            let block = shapecraft_boolean::block("b", Point3::ZERO, axes, size)?;
            shapecraft_boolean::write(&mut world.topo, &block, "b")?;
            Ok(Arc::new(world))
        };
        let (plate, wall) = (part([4.0, 4.0, 1.0])?, part([1.0, 4.0, 3.0])?);
        // The wall's edge along y at x = 1, at its foot.
        let topo = &wall.topo;
        let foot = topo
            .edges()
            .iter()
            .find(|(_, e)| {
                let ends = [e.start, e.end].map(|v| topo.vertex(v).map(|v| v.point));
                let at = |p: Option<Point3>| p.is_some_and(|p| p.x == 1.0 && p.z == 0.0);
                at(ends[0]) && at(ends[1])
            })
            .and_then(|(id, _)| topo.name_of(id))
            .context("the foot")?
            .to_owned();
        let mut b = Standalone::new();
        for (name, world, z) in [("plate", &plate, 0.0), ("wall", &wall, 1.0)] {
            b.world.instances.insert(
                name.into(),
                Instance {
                    pose: Pose::from_translation(Point3::new(0.0, 0.0, z)),
                    world: world.clone(),
                    fixed: true,
                },
            );
        }
        Weld {
            id: "bead".into(),
            args: WeldArgs {
                edges: vec![Source::placed(&["wall".to_owned()], Source::Topology(foot)).into()],
                size: Measure::new(0.5),
            },
        }
        .apply(&mut b)?;
        assert_eq!(b.world.topo.validate(), Ok(()));
        let body = b.world.topo.resolve("bead").context("the bead")?;
        let bead = read(&b.world.topo, body)?;
        let volume = shapecraft_boolean::measure(&bead).volume;
        assert!(
            (volume - std::f64::consts::PI * 0.25).abs() < 1e-9,
            "{volume}"
        );
        Ok(())
    }
}
