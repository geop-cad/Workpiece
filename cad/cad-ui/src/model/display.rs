//! How the model is displayed without being changed: cut open by a
//! plane, its placed parts drawn apart.

use std::sync::Arc;

use workpiece_core::Pose;
use workpiece_core::math::Point3;
use workpiece_core::world::World;
use workpiece_core::world::topo::BodyKind;
pub(crate) use workpiece_shape::cut::corners;

/// What the model's faces are painted to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Analysis {
    /// Nothing: they show as they are.
    #[default]
    None,
    /// Stripes mirrored in them, which break where faces do not run
    /// on smoothly into each other.
    Zebra,
    /// How tightly they bend: from blue where they are flat to red.
    Curvature,
    /// How they lean from a pull along z: green where they have draft,
    /// yellow where they have little, red where they have none or are
    /// undercut — seen from above.
    Draft,
}

/// A way of displaying a world that is not the world's own.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Display {
    /// Cut open as a section of the part cuts it: a step of the part,
    /// by its name.
    pub cut: Option<String>,
    /// Cut open by a plane that no step of the part has (the plane a
    /// sketch is drawn on): what is on the side its normal points to
    /// is cut away. Shown instead of `cut`.
    pub plane: Option<workpiece_core::world::Cutting>,
    /// How far apart the placed parts are drawn: each this many times as
    /// far from the middle of them all as it is.
    pub explode: f64,
    /// What the faces are painted to show.
    pub analysis: Analysis,
    /// A joint run along its travel: which of the joints that have
    /// limits (as [`drivable`] lists them), and how far from its low
    /// limit to its high one (0 to 1). The parts are drawn where that
    /// leaves them.
    pub motion: Option<(usize, f64)>,
}

/// A box around the solid bodies of `world` (and of what is placed in
/// it), as `pose` places it.
fn extent(world: &World, pose: Pose, into: &mut Option<(Point3, Point3)>) {
    let topo = &world.topo;
    for (id, body) in topo.bodies().iter() {
        if body.kind != BodyKind::Solid {
            continue;
        }
        let Ok(solid) = workpiece_boolean::read(topo, id) else {
            continue;
        };
        let (lo, hi) = solid.extent();
        for corner in corners(lo, hi) {
            let p = pose.transform_point(corner);
            *into = Some(into.map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
        }
    }
    for instance in world.instances.values() {
        extent(&instance.world, pose * instance.pose, into);
    }
}

/// The joints of `world` that have limits to their travel, by their
/// place among its joints.
fn limited(world: &World) -> Vec<usize> {
    let has = |j: &workpiece_core::world::Joint| {
        (j.kind.turns() && j.limits.angle.is_some())
            || (j.kind.slides() && j.limits.distance.is_some())
    };
    (0..world.joints.len())
        .filter(|&k| has(&world.joints[k]))
        .collect()
}

/// The names of the joints of `world` that can be run along their
/// travel: those with limits.
pub fn drivable(world: &World) -> Vec<String> {
    let joints = limited(world).into_iter();
    joints.map(|k| world.joints[k].name.clone()).collect()
}

/// Stamps `world`, and what is placed in it, as what it is cut open
/// by `how`: what is drawn of a world is kept by its stamp, and the
/// world cut open is drawn otherwise than whole.
fn restamp(world: &mut World, how: &str) {
    use std::hash::{Hash, Hasher};
    let stamp = |of: u64| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (of, how).hash(&mut hasher);
        // (None is what no engine built: never kept.)
        hasher.finish().max(1)
    };
    if world.stamp != 0 {
        world.stamp = stamp(world.stamp);
    }
    for instance in world.instances.values_mut() {
        if instance.world.stamp != 0 {
            restamp(Arc::make_mut(&mut instance.world), how);
        }
    }
}

impl Display {
    /// Whether the world is displayed as it is.
    pub fn is_plain(&self) -> bool {
        self.cut.is_none() && self.plane.is_none() && self.explode == 0.0 && self.motion.is_none()
    }

    /// `world` as it is displayed.
    pub fn of(&self, world: &Arc<World>) -> Arc<World> {
        if self.is_plain() {
            return world.clone();
        }
        let mut shown = (**world).clone();
        if let Some((which, along)) = self.motion
            && let Some(&at) = limited(&shown).get(which)
        {
            // The joint held at that place of its travel, and the parts
            // where the joints then leave them.
            let joint = &mut shown.joints[at];
            let held = |(lo, hi): (f64, f64)| {
                let at = lo + (hi - lo) * along.clamp(0.0, 1.0);
                Some((at, at))
            };
            match joint.limits.angle.filter(|_| joint.kind.turns()) {
                Some(range) => joint.limits.angle = held(range),
                None => joint.limits.distance = joint.limits.distance.and_then(held),
            }
            let solved = workpiece_ops_instances::assembly::solve(&shown, None);
            for (name, pose) in solved.poses {
                if let Some(instance) = shown.instances.get_mut(&name) {
                    instance.pose = pose;
                }
            }
        }
        if self.explode != 0.0 && !shown.instances.is_empty() {
            // Each part away from the middle of them all.
            let middles: Vec<(String, Point3)> = shown
                .instances
                .iter()
                .map(|(name, instance)| {
                    let mut around = None;
                    extent(&instance.world, instance.pose, &mut around);
                    let middle =
                        around.map_or(instance.pose.translation(), |(lo, hi)| (lo + hi) / 2.0);
                    (name.clone(), middle)
                })
                .collect();
            let all = middles.iter().map(|(_, m)| *m).sum::<Point3>() / middles.len() as f64;
            for (name, middle) in middles {
                if let Some(instance) = shown.instances.get_mut(&name) {
                    let by = (middle - all) * self.explode;
                    instance.pose = Pose::from_translation(by) * instance.pose;
                }
            }
        }
        let named = self.cut.as_ref().and_then(|name| world.sections.get(name));
        if let Some(cutting) = self.plane.as_ref().or(named) {
            let mut around = None;
            extent(&shown, Pose::IDENTITY, &mut around);
            if let Some((lo, hi)) = around {
                let far = (hi - lo).length() * 4.0 + lo.length() + hi.length() + 1.0;
                let far = far + cutting.origin.length();
                shown = workpiece_ops_drawing::opened(&shown, cutting, far);
                restamp(&mut shown, &format!("{cutting:?}"));
            }
        }
        Arc::new(shown)
    }
}
