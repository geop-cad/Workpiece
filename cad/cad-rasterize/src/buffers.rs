//! The primitives a scene is drawn and picked from.
//!
//! Positions are `f32`, in the coordinates of the part being shown. Widths
//! and sizes are in pixels: lines and vertices keep their size on screen
//! however far away they are.

use std::collections::HashMap;

use glam::Vec3;
use workpiece_core::Source;
use workpiece_core::handles::GizmoPart;

/// Linear RGBA.
pub type Color = [f32; 4];

/// The index of a [`Pick`] in its [`Scene`].
pub type PickId = u32;

/// What a primitive shows, for picking and highlighting.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pick {
    /// Something a reference can point at: `source` in the world of the
    /// instance at `instance` (instance names from the shown part down;
    /// empty for the shown part itself).
    Entity {
        /// The path of instances it is in.
        instance: Vec<String>,
        /// The datum, frame axis or named topology.
        source: Source,
    },
    /// One of the edited operation's handles, or one part of it.
    Handle {
        /// The handle's name.
        name: String,
        /// The grabbed part of a gizmo; `None` for other handles.
        part: Option<GizmoPart>,
    },
}

/// A triangle of a face or a filled plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    /// The corners.
    pub positions: [Vec3; 3],
    /// The unit normal at each corner.
    pub normals: [Vec3; 3],
    /// The color; translucent when alpha is below 1.
    pub color: Color,
    /// What it shows, if it can be picked.
    pub pick: Option<PickId>,
    /// The texture drawn over its color: which one, by its number (0:
    /// none), and the length of one repeat of its pattern.
    pub texture: [f32; 2],
}

/// A straight line segment, `width` pixels wide at each end (0 at one end
/// makes an arrowhead).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    /// The start.
    pub a: Vec3,
    /// The end.
    pub b: Vec3,
    /// The width in pixels at `a` and at `b`.
    pub width: [f32; 2],
    /// The color.
    pub color: Color,
    /// What it shows, if it can be picked.
    pub pick: Option<PickId>,
}

/// A vertex: a round marker `size` pixels across.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// The center.
    pub position: Vec3,
    /// The diameter in pixels.
    pub size: f32,
    /// The color.
    pub color: Color,
    /// What it shows, if it can be picked.
    pub pick: Option<PickId>,
}

/// Text at a point of the scene, written along a direction in it: a
/// dimension's value, a constraint's badge. The host draws it over the
/// scene, upright.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// Its center.
    pub position: Vec3,
    /// The direction of its baseline, in the scene (unit).
    pub along: Vec3,
    /// The text.
    pub text: String,
    /// Small text set above the line after it: an upper deviation.
    pub upper: String,
    /// Small text set below the line after it: a lower deviation.
    pub lower: String,
    /// The color.
    pub color: Color,
    /// In a small box: a constraint's badge.
    pub badge: bool,
}

/// Triangles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriangleBuffer {
    /// The triangles, in drawing order.
    pub triangles: Vec<Triangle>,
}

/// Lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineBuffer {
    /// The lines, in drawing order.
    pub lines: Vec<Line>,
}

/// Vertices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VertexBuffer {
    /// The vertices, in drawing order.
    pub vertices: Vec<Vertex>,
}

/// What a scene draws over everything else of it, whatever is in
/// front: datum frames, which are what everything is placed from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Front {
    /// Filled planes.
    pub triangles: Vec<Triangle>,
    /// Axes and outlines.
    pub lines: Vec<Line>,
    /// Origins.
    pub vertices: Vec<Vertex>,
}

/// Everything to draw, with what each primitive shows.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    /// Faces and filled planes.
    pub triangles: TriangleBuffer,
    /// Edges, axes, arrows and rings.
    pub lines: LineBuffer,
    /// Vertices and origins.
    pub vertices: VertexBuffer,
    /// Texts.
    pub labels: Vec<Label>,
    /// What shows in front of all of the above.
    pub front: Front,
    /// What the primitives show, by [`PickId`].
    picks: Vec<Pick>,
    /// `picks`, inverted.
    ids: HashMap<Pick, PickId>,
}

impl Scene {
    /// The id of `pick`, added if new; `None` passes through (for
    /// primitives that cannot be picked).
    pub fn pick_id(&mut self, pick: Option<Pick>) -> Option<PickId> {
        let pick = pick?;
        if let Some(&id) = self.ids.get(&pick) {
            return Some(id);
        }
        let id = PickId::try_from(self.picks.len()).ok()?;
        self.ids.insert(pick.clone(), id);
        self.picks.push(pick);
        Some(id)
    }

    /// The id of `pick`, if the scene shows it.
    pub fn index_of(&self, pick: &Pick) -> Option<PickId> {
        self.ids.get(pick).copied()
    }

    /// Everything the scene's primitives show, by [`PickId`].
    pub fn picks(&self) -> &[Pick] {
        &self.picks
    }

    /// What the primitive with `id` shows.
    pub fn pick(&self, id: PickId) -> Option<&Pick> {
        self.picks.get(usize::try_from(id).ok()?)
    }

    /// Whether there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.triangles.triangles.is_empty()
            && self.lines.lines.is_empty()
            && self.vertices.vertices.is_empty()
            && self.labels.is_empty()
            && self.front == Front::default()
    }
}
