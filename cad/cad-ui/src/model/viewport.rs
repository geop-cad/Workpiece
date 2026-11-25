//! The model's side of the viewport: the camera, what is built and
//! rasterized for the active part, and what is under the pointer.

use std::sync::Arc;

use shapecraft_cad_part::{Cancel, Engine, Workspace};
use shapecraft_cad_rasterize::{Drawing, Scenes, Style};
use shapecraft_core::world::Visibility;
use shapecraft_core::{Pose, World};

use super::Camera;
use super::camera::Transition;
use super::focus::locate;
use crate::picking::Candidate;

/// What the viewport shows: the focused part built up to the edited step
/// (or the scrubber, or its end) in color, and the parts around it as
/// ghosts.
#[derive(Debug, Clone, Default)]
pub struct Shown {
    /// The focused part, pickable, in the active part's coordinates.
    pub scene: Arc<Drawing>,
    /// The parts the focused part is placed in, up to where it is placed.
    pub ghosts: Arc<Drawing>,
    /// The world the edited step is applied to, in the focused part's
    /// coordinates: what its references resolve in. Without an edit, the
    /// shown world.
    pub before: Arc<World>,
    /// The focused part's world as shown, in its own coordinates: what is
    /// picked.
    pub world: Arc<World>,
    /// What of it is hidden.
    pub visibility: Visibility,
    /// Where the focused part sits in the active part.
    pub pose: Pose,
    /// Why something could not be built.
    pub error: Option<String>,
    /// The focused part's cache as the build left it: what its operations
    /// worked out, to keep with the part.
    pub cache: Option<shapecraft_core::Cache>,
    /// The references of the part's steps that lost what they named
    /// and were found again: its program follows them.
    pub repaired: Vec<shapecraft_cad_part::Repair>,
    /// The bodies of each part the focused part places, by its path
    /// and variant: what a dialog offers to take along of it.
    pub placed: std::collections::BTreeMap<(String, Option<String>), Vec<String>>,
}

/// Where the pointer is in the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerAt {
    /// Pixels from the viewport's top left.
    pub position: [f32; 2],
    /// The viewport's size in pixels.
    pub size: [f32; 2],
}

/// A placed part being moved by the pointer.
#[derive(Debug, Clone, PartialEq)]
pub struct Moving {
    /// The path of the part whose assembly is solved: the one the moved
    /// instance is placed in.
    pub part: String,
    /// The instances from the shown world down to that assembly.
    pub within: Vec<String>,
    /// The instance moved.
    pub instance: String,
    /// The point of it the pointer holds, in its coordinates.
    pub local: shapecraft_core::math::Point3,
    /// Where that point was when it was grabbed, in the view's
    /// coordinates: the pointer moves it in the plane through there
    /// that faces the view.
    pub grabbed: shapecraft_core::math::Point3,
}

/// The viewport's state.
#[derive(Debug, Clone, Default)]
pub struct Viewport {
    /// The camera, as it is now.
    pub camera: Camera,
    /// Where the camera is going, while it is on its way to a new view.
    pub transition: Option<Transition>,
    /// What is built and shown.
    pub shown: Shown,
    /// The pointer, while it is over the viewport.
    pub pointer: Option<PointerAt>,
    /// The viewport's size in pixels.
    pub size: [f32; 2],
    /// What is under the pointer and may be picked, in picking order.
    pub candidates: Vec<Candidate>,
    /// The current candidate, cycled with the right mouse button.
    pub candidate: usize,
    /// The handle being dragged.
    pub dragging: Option<Candidate>,
    /// What was under the pointer when the left button went down, and
    /// where: what a drag grabs, and where it starts.
    pub pressed: Option<(Candidate, PointerAt)>,
    /// The placed part the pointer is moving.
    pub moving: Option<Moving>,
}

impl Viewport {
    /// The current candidate under the pointer.
    pub fn hovered(&self) -> Option<&Candidate> {
        self.candidates.get(self.candidate)
    }
}

/// Builds what the viewport shows of the part at `active`, focused on the
/// placed part at `focus`: up to the step `editing` (its last id, in the
/// focused part) with and without it, or up to `scrub` steps. Without
/// `preview`, the edited step is not applied. The active part shows as
/// its `variant`.
///
/// The ghosts are everything else: each part the focus is placed in,
/// whole but for the step placing it, and what the focused part's later
/// steps add (what they build that the shown world, by name, has not).
#[allow(clippy::too_many_arguments)]
pub(crate) fn shown(
    scenes: &Scenes,
    engine: &Engine,
    cancel: &Cancel,
    workspace: &Arc<Workspace>,
    active: &str,
    focus: &[String],
    editing: Option<&str>,
    scrub: Option<usize>,
    preview: bool,
    variant: Option<&str>,
    visibility: &dyn Fn(&str, &World) -> Visibility,
    display: super::display::Display,
) -> Shown {
    let mut shown = Shown::default();
    let mut errors = Vec::new();
    let located = match locate(workspace, active, focus) {
        Ok(located) => located,
        Err(e) => {
            shown.error = Some(format!("{e:#}"));
            return shown;
        }
    };
    // The focused part shows as the variant it is placed as; the active
    // part itself as the variant asked for.
    let variant = located
        .variant
        .as_deref()
        .or(variant.filter(|_| focus.is_empty()));
    let mut ghosts = Drawing::default();
    let ghost = |ghosts: &mut Drawing, world: &World, pose, hide: &Visibility| {
        ghosts.extend(scenes.draw(world, pose, Style::Ghost, hide));
    };
    for context in &located.contexts {
        let placing = context.steps;
        match engine.build(workspace, &context.part, None, &|i| i != placing, cancel) {
            Ok(built) => ghost(&mut ghosts, &built.world, context.pose, &Visibility::ALL),
            Err(e) => errors.push(format!("{e:#}")),
        }
    }

    let program = workspace
        .part(&located.part)
        .map(|p| &p.program[..])
        .unwrap_or_default();
    let edited = editing.and_then(|id| program.iter().position(|op| op.id() == id));
    let steps = edited.unwrap_or_else(|| scrub.unwrap_or(program.len()).min(program.len()));
    let mut first = None;
    let mut repaired = Vec::new();
    let before = match engine.build(workspace, &located.part, variant, &|i| i < steps, cancel) {
        Ok(built) => {
            first = Some(built.cache);
            repaired = built.repaired;
            built.world
        }
        Err(e) => {
            errors.push(format!("{e:#}"));
            Arc::new(World::new())
        }
    };
    // The build that shows what is edited also leaves what it worked out.
    let mut cache = None;
    let after = match edited {
        Some(step) if preview => {
            match engine.build(workspace, &located.part, variant, &|i| i <= step, cancel) {
                Ok(built) => {
                    cache = Some(built.cache);
                    built.world
                }
                Err(e) => {
                    errors.push(format!("{e:#}"));
                    before.clone()
                }
            }
        }
        _ => before.clone(),
    };
    // The steps after what is shown, as ghosts of what they add.
    let shown_steps = edited.map_or(steps, |step| step + 1);
    if shown_steps < program.len() && preview {
        match engine
            .build(workspace, &located.part, variant, &|_| true, cancel)
            .map(|b| b.world)
        {
            Ok(end) => ghost(&mut ghosts, &end, located.pose, &Visibility::of(&after)),
            Err(e) => errors.push(format!("{e:#}")),
        }
    }

    // The bodies of the parts it places, each built whole (as its
    // build placed it, so kept already).
    let mut placed = std::collections::BTreeMap::new();
    for (path, variant) in program.iter().filter_map(|op| op.placed()) {
        let key = (path.to_owned(), variant.map(str::to_owned));
        if placed.contains_key(&key) {
            continue;
        }
        if let Ok(built) = engine.build(workspace, path, variant, &|_| true, cancel) {
            let topo = &built.world.topo;
            let names = topo.bodies().iter().filter_map(|(id, _)| topo.name_of(id));
            placed.insert(key, names.map(str::to_owned).collect());
        }
    }

    let visibility = visibility(&located.part, &after);
    // (Drawn cut open or apart, if it is displayed so: what is picked
    // and measured is the world as it is.)
    // (A section that is edited shows as it cuts: its offset is
    // dragged to where the inside shows.)
    let cutting = editing.filter(|id| edited.is_some() && after.sections.contains_key(*id));
    let display = match cutting {
        Some(id) => super::display::Display {
            cut: Some(id.to_owned()),
            ..display.clone()
        },
        None => display.clone(),
    };
    let scene = scenes.draw(
        &display.of(&after),
        located.pose,
        Style::Normal,
        &visibility,
    );

    Shown {
        scene: Arc::new(scene),
        ghosts: Arc::new(ghosts),
        before,
        world: after,
        visibility,
        pose: located.pose,
        error: (!errors.is_empty()).then(|| errors.join("\n")),
        // A variant's solution is not the part's.
        cache: cache
            .or(first.filter(|_| edited.is_none()))
            .filter(|_| variant.is_none()),
        repaired,
        placed,
    }
}
