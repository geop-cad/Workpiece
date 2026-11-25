//! Instances: other parts' worlds placed into this one.

use std::sync::Arc;

use crate::math::Point3;
use crate::pose::Pose;
use crate::world::World;

/// Another part's built world, placed at a pose. The world is shared:
/// every instance of the same part within one build points at the same
/// world.
#[derive(Debug, Clone)]
pub struct Instance {
    /// Where the instanced world's global frame sits in this world.
    pub pose: Pose,
    /// The instanced part's built world.
    pub world: Arc<World>,
    /// Whether it stays where it is put: joints move what they connect
    /// to it, never it.
    pub fixed: bool,
}

/// A box around all the bodies of `world` (and of what is placed in
/// it), as `pose` places them: its least and greatest corner. No less
/// than they reach — it is around the control points of their faces,
/// which they lie within — and nothing, for a world without bodies.
fn reach_of(world: &World, pose: Pose, sofar: &mut Option<(Point3, Point3)>) {
    let topo = &world.topo;
    let mut take = |p: Point3| {
        let p = pose.transform_point(p);
        *sofar = Some(match *sofar {
            Some((lo, hi)) => (lo.min(p), hi.max(p)),
            None => (p, p),
        });
    };
    for (_, face) in topo.faces().iter() {
        if let Some(surface) = topo.surface(face.surface) {
            surface.control_points().iter().for_each(|p| take(*p));
        }
    }
    for (_, vertex) in topo.vertices().iter() {
        take(vertex.point);
    }
    for instance in world.instances.values() {
        reach_of(&instance.world, pose * instance.pose, sofar);
    }
}

impl World {
    /// A box around its bodies and those of what is placed in it (see
    /// [`reach_of`]), in its own coordinates: no less than they reach.
    pub fn reach(&self) -> Option<(Point3, Point3)> {
        let mut reach = None;
        reach_of(self, Pose::IDENTITY, &mut reach);
        reach
    }
}

impl Instance {
    /// A box around the part where it is placed (see [`reach_of`]):
    /// what it could touch lies within it.
    pub fn reach(&self) -> Option<(Point3, Point3)> {
        let mut reach = None;
        reach_of(&self.world, self.pose, &mut reach);
        reach
    }
}
