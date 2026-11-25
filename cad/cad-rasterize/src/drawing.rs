//! A [`Drawing`]: what is drawn of a world and the parts placed in it,
//! as scenes with poses.
//!
//! Each world is rasterized on its own, in its own coordinates, into a
//! [`Scene`] that is kept by the world's [stamp](World::stamp); a drawing
//! lists those scenes with where each is placed. Moving a part of an
//! assembly makes a drawing with the same scenes and other poses: nothing
//! is rasterized again, and whoever draws it has nothing new to upload.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, PoisonError};

use workpiece_core::world::{Visibility, World};
use workpiece_core::{Pose, Source};

use crate::world::Style;
use crate::{Pick, PickId, Scene};

/// One world's scene, placed.
#[derive(Debug, Clone)]
pub struct Placed {
    /// The world's own things, in its coordinates. Its picks are as of a
    /// world on its own: without instances.
    pub scene: Arc<Scene>,
    /// Where the world is.
    pub pose: Pose,
    /// The instances it is in, from the drawn world down.
    pub instance: Vec<String>,
    /// What is added to the scene's pick ids to make them the
    /// drawing's.
    pub base: PickId,
}

/// Scenes with poses: a world and everything placed in it.
#[derive(Debug, Clone, Default)]
pub struct Drawing {
    /// The scenes, the drawn world's own first.
    pub items: Vec<Placed>,
}

impl Drawing {
    /// Whether there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.items.iter().all(|i| i.scene.is_empty())
    }

    /// The id of `pick` in the drawing, if it shows it.
    pub fn index_of(&self, pick: &Pick) -> Option<PickId> {
        let Pick::Entity { instance, source } = pick else {
            return None;
        };
        let item = self.items.iter().find(|i| i.instance == *instance)?;
        let local = Pick::Entity {
            instance: Vec::new(),
            source: source.clone(),
        };
        Some(item.base + item.scene.index_of(&local)?)
    }

    /// Everything the drawing shows that can be picked.
    pub fn picks(&self) -> Vec<Pick> {
        let mut out = Vec::new();
        for item in &self.items {
            for pick in item.scene.picks() {
                if let Pick::Entity { source, .. } = pick {
                    out.push(Pick::Entity {
                        instance: item.instance.clone(),
                        source: source.clone(),
                    });
                }
            }
        }
        out
    }

    /// Adds the scenes of `other`, after this one's.
    pub fn extend(&mut self, other: Drawing) {
        let mut base = self.next_base();
        for mut item in other.items {
            item.base = base;
            base += item.scene.picks().len() as PickId;
            self.items.push(item);
        }
    }

    fn next_base(&self) -> PickId {
        self.items
            .last()
            .map_or(0, |i| i.base + i.scene.picks().len() as PickId)
    }

    /// Whether `source` of the instance at `instance` is drawn.
    pub fn shows(&self, instance: &[String], source: &Source) -> bool {
        self.index_of(&Pick::Entity {
            instance: instance.to_vec(),
            source: source.clone(),
        })
        .is_some()
    }
}

/// The scenes kept: by what world they are of, how it is drawn and what
/// of it is hidden.
#[derive(Debug, Default)]
pub struct Scenes {
    kept: Mutex<HashMap<(u64, Style, u64), Arc<Scene>>>,
}

/// How many scenes are kept before the oldest are let go (all of them:
/// what is still drawn is rasterized again).
const KEPT: usize = 2048;

impl Scenes {
    /// None kept yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The scene of `world` on its own.
    fn of(&self, world: &World, style: Style, visibility: &Visibility) -> Arc<Scene> {
        let make = || {
            let mut scene = Scene::default();
            crate::world::own(&mut scene, world, style, visibility);
            Arc::new(scene)
        };
        // A world no engine built has no stamp: nothing to keep it by.
        if world.stamp == 0 {
            return make();
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{visibility:?}").hash(&mut hasher);
        let key = (world.stamp, style, hasher.finish());
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(scene) = kept.get(&key) {
            return scene.clone();
        }
        if kept.len() >= KEPT {
            kept.clear();
        }
        let scene = make();
        kept.insert(key, scene.clone());
        scene
    }

    /// The drawing of `world`, placed by `pose`, leaving out what
    /// `visibility` hides of it. What it places shows as parts do in an
    /// assembly: their bodies, without the datums and sketches they were
    /// built from.
    pub fn draw(
        &self,
        world: &World,
        pose: Pose,
        style: Style,
        visibility: &Visibility,
    ) -> Drawing {
        let mut drawing = Drawing::default();
        self.add(
            &mut drawing,
            world,
            pose,
            style,
            visibility,
            &mut Vec::new(),
        );
        drawing
    }

    fn add(
        &self,
        drawing: &mut Drawing,
        world: &World,
        pose: Pose,
        style: Style,
        visibility: &Visibility,
        instance: &mut Vec<String>,
    ) {
        let scene = self.of(world, style, visibility);
        let base = drawing.next_base();
        drawing.items.push(Placed {
            scene,
            pose,
            instance: instance.clone(),
            base,
        });
        for (name, placed) in world.shown_instances(visibility) {
            instance.push(name.clone());
            self.add(
                drawing,
                &placed.world,
                pose * placed.pose,
                style,
                &Visibility::placed(&placed.world, &visibility.shades),
                instance,
            );
            instance.pop();
        }
    }
}
