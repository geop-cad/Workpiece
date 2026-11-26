//! What a built world looks like: its faces, edges and vertices, its
//! datums, and the worlds of its instances, each at its pose.
//!
//! Reference geometry is as large as its world says
//! ([`World::datum_size`]), times [`FRAME_SIZE`], [`PLANE_SIZE`] or
//! [`AXIS_SIZE`]: it grows with the model. Widths are in pixels.

use glam::Vec3;
use workpiece_core::math::{Axis, Point3};
use workpiece_core::world::topo::{BodyKind, EntityId, Topo};
use workpiece_core::world::visibility::shown_topology;
use workpiece_core::world::{Datum, DatumFrame, Profile, Visibility, World};
use workpiece_core::{Pose, Source};

use crate::palette::{self, AXES, EDGE, MODEL, POINT, REFERENCE, SKETCH, SURFACE};
use crate::{Color, Line, Pick, Scene, Triangle, Vertex, tessellate};

/// Length of a datum frame's axis arrows, in datum sizes.
pub const FRAME_SIZE: f64 = 1.0;
/// Half the side of the square a datum plane is drawn as, in datum
/// sizes.
pub const PLANE_SIZE: f64 = 1.0;
/// Half the length a datum axis is drawn with, in datum sizes.
pub const AXIS_SIZE: f64 = 2.0;

/// Widths and sizes, in pixels.
const EDGE_WIDTH: f32 = 1.6;
const AXIS_WIDTH: f32 = 2.2;
const ARROW_WIDTH: f32 = 9.0;
const VERTEX_SIZE: f32 = 6.0;
const ORIGIN_SIZE: f32 = 9.0;
const SKETCH_WIDTH: f32 = 1.6;
const SKETCH_POINT: f32 = 4.5;
/// How far along a frame's axes its inset planes reach, as fractions of
/// [`FRAME_SIZE`].
pub const INSET: (f64, f64) = (0.2, 0.5);
/// Opacity of filled planes.
const PLANE_ALPHA: f32 = 0.22;

/// How a world is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Style {
    /// In color, and pickable.
    #[default]
    Normal,
    /// Faded and translucent, and not pickable: context around what is
    /// edited.
    Ghost,
}

/// Adds `world`, placed by `pose`, to `scene`, leaving out what
/// `visibility` hides of it (of what it places, the bodies show).
pub fn world(scene: &mut Scene, world: &World, pose: Pose, style: Style, visibility: &Visibility) {
    Painter {
        scene,
        pose,
        style,
        instance: Vec::new(),
        front: false,
        size: visibility.datum_size.unwrap_or_else(|| world.datum_size()),
    }
    .world(world, visibility);
}

/// Adds what `world` itself has to `scene`, in the world's own
/// coordinates: not what is placed in it. Leaves out what `visibility`
/// hides.
pub fn own(scene: &mut Scene, world: &World, style: Style, visibility: &Visibility) {
    Painter {
        scene,
        pose: Pose::IDENTITY,
        style,
        instance: Vec::new(),
        front: false,
        size: visibility.datum_size.unwrap_or_else(|| world.datum_size()),
    }
    .own(world, visibility);
}

/// Adds primitives for one world (and, recursively, its instances).
struct Painter<'a> {
    scene: &'a mut Scene,
    pose: Pose,
    style: Style,
    /// The instance path of the world being added.
    instance: Vec<String>,
    /// Whether what is added goes in front of everything else.
    front: bool,
    /// The size the datums of the world being added are drawn at.
    size: f64,
}

fn f32v(p: Point3) -> Vec3 {
    p.as_vec3()
}

impl Painter<'_> {
    fn color(&self, color: Color) -> Color {
        match self.style {
            Style::Normal => color,
            Style::Ghost => palette::ghost(color),
        }
    }

    fn pick(&mut self, source: Source) -> Option<crate::PickId> {
        let pick = (self.style == Style::Normal).then(|| Pick::Entity {
            instance: self.instance.clone(),
            source,
        });
        self.scene.pick_id(pick)
    }

    fn at(&self, p: Point3) -> Vec3 {
        f32v(self.pose.transform_point(p))
    }

    fn line(&mut self, a: Point3, b: Point3, width: [f32; 2], color: Color, pick: Option<u32>) {
        let line = Line {
            a: self.at(a),
            b: self.at(b),
            width,
            color: self.color(color),
            pick,
        };
        if self.front {
            self.scene.front.lines.push(line);
        } else {
            self.scene.lines.lines.push(line);
        }
    }

    fn vertex(&mut self, p: Point3, size: f32, color: Color, pick: Option<u32>) {
        let vertex = Vertex {
            position: self.at(p),
            size,
            color: self.color(color),
            pick,
        };
        if self.front {
            self.scene.front.vertices.push(vertex);
        } else {
            self.scene.vertices.vertices.push(vertex);
        }
    }

    /// The square with corners `corners`, filled and outlined.
    fn quad(&mut self, corners: [Point3; 4], color: Color, pick: Option<u32>) {
        let normal = (corners[1] - corners[0]).cross(corners[3] - corners[0]);
        let normal = f32v(self.pose.transform_vector(normal.normalize_or_zero()));
        let fill = self.color(palette::with_alpha(color, PLANE_ALPHA * color[3]));
        let [a, b, c, d] = corners.map(|p| self.at(p));
        for positions in [[a, b, c], [a, c, d]] {
            let triangle = Triangle {
                positions,
                normals: [normal; 3],
                color: fill,
                pick,
                texture: [0.0, 1.0],
            };
            if self.front {
                self.scene.front.triangles.push(triangle);
            } else {
                self.scene.triangles.triangles.push(triangle);
            }
        }
        for k in 0..4 {
            self.line(corners[k], corners[(k + 1) % 4], [1.2; 2], color, pick);
        }
    }

    /// A shaft from `from` to `to`, with an arrowhead at `to`.
    fn arrow(&mut self, from: Point3, to: Point3, color: Color, pick: Option<u32>) {
        let head = from + (to - from) * 0.8;
        self.line(from, head, [AXIS_WIDTH; 2], color, pick);
        self.line(head, to, [ARROW_WIDTH, 0.0], color, pick);
    }

    fn own(&mut self, world: &World, visibility: &Visibility) {
        self.topo(world, visibility);
        // (In front of everything while they are to be picked.)
        for (name, datum) in world.shown_datums(visibility) {
            self.front = visibility.datums_in_front;
            self.datum(name, datum);
        }
        self.front = visibility.sketches_in_front;
        for (_, profile) in world.shown_profiles(visibility) {
            self.profile(profile);
        }
        self.front = false;
    }

    fn world(&mut self, world: &World, visibility: &Visibility) {
        self.own(world, visibility);
        for (name, instance) in world.shown_instances(visibility) {
            let outer = self.pose;
            self.pose = outer * instance.pose;
            self.instance.push(name.clone());
            let outer_size = std::mem::replace(&mut self.size, instance.world.datum_size());
            let placed = Visibility::placed(&instance.world, &visibility.shades);
            self.world(&instance.world, &placed);
            self.instance.pop();
            self.size = outer_size;
            self.pose = outer;
        }
    }

    /// A sketch's curves and their ends, in its plane.
    fn profile(&mut self, profile: &Profile) {
        let pose = profile.plane.pose();
        let place = |p: workpiece_core::math::Point2| pose.transform_point(p.extend(0.0));
        for edge in &profile.edges {
            let points = edge.curve.polyline(edge.t0, edge.t1);
            for pair in points.windows(2) {
                self.line(
                    place(pair[0]),
                    place(pair[1]),
                    [SKETCH_WIDTH; 2],
                    SKETCH,
                    None,
                );
            }
        }
        for (index, vertex) in profile.vertices.iter().enumerate() {
            // Where a circle closes on itself is no end to show: only
            // ends of open edges, and points placed on their own.
            let mut at = profile
                .edges
                .iter()
                .filter(|e| e.start == index || e.end == index)
                .peekable();
            let seam = at.peek().is_some() && at.all(|e| e.start == e.end);
            if !seam {
                self.vertex(place(vertex.point), SKETCH_POINT, SKETCH, None);
            }
        }
    }

    fn named(&mut self, topo: &Topo, id: impl Into<EntityId>) -> Option<u32> {
        let name = topo.name_of(id)?.to_owned();
        self.pick(Source::Topology(name))
    }

    fn topo(&mut self, world: &World, visibility: &Visibility) {
        let topo = &world.topo;
        let shown = shown_topology(topo, visibility);
        // Surfaces (sheets) show in a color of their own.
        let mut of_sheets = std::collections::BTreeSet::new();
        for (body, _) in topo
            .bodies()
            .iter()
            .filter(|(_, b)| b.kind == BodyKind::Sheet)
        {
            of_sheets.extend(topo.body_faces(body).unwrap_or_default());
        }
        // A face shows as what it is made of does — as was said of
        // it, or of its body, or of the part — in the colour it was
        // given over that.
        // (From what a screen shows to what light adds up as.)
        let linear = |c: u8| libm::powf(f32::from(c) / 255.0, 2.2);
        let colour_of = |[r, g, b]: [u8; 3]| [linear(r), linear(g), linear(b), 1.0];
        let mut body_of = std::collections::BTreeMap::new();
        for (body, _) in topo.bodies().iter() {
            for face in topo.body_faces(body).unwrap_or_default() {
                body_of.insert(face, body);
            }
        }
        for &id in &shown.faces {
            let body = body_of.get(&id).and_then(|b| topo.name_of(*b));
            let look = world.face_look(id, body);
            let shade = look
                .material
                .as_ref()
                .and_then(|m| visibility.shades.get(m));
            let texture = match shade {
                Some(shade) if shade.texture != 0 => {
                    [f32::from(shade.texture), shade.scale as f32 / 1000.0]
                }
                _ => [0.0, 1.0],
            };
            let color = self.color(match look.colour.or(shade.map(|s| s.colour)) {
                Some(colour) => colour_of(colour),
                None if of_sheets.contains(&id) => SURFACE,
                None => MODEL,
            });
            let Some(mesh) = tessellate::face(topo, id) else {
                continue;
            };
            let pick = self.named(topo, id);
            for (positions, normals) in mesh.triangles.iter().copied() {
                let triangle = Triangle {
                    positions: positions.map(|p| self.at(p)),
                    normals: normals.map(|n| f32v(self.pose.transform_vector(n))),
                    color,
                    pick,
                    texture,
                };
                self.scene.triangles.triangles.push(triangle);
            }
        }
        for &id in &shown.edges {
            let Some((edge, curve)) = topo.edge(id).and_then(|e| Some((e, topo.curve(e.curve)?)))
            else {
                continue;
            };
            let pick = self.named(topo, id);
            for pair in curve.polyline(edge.t0, edge.t1).windows(2) {
                self.line(pair[0], pair[1], [EDGE_WIDTH; 2], EDGE, pick);
            }
        }
        for &id in &shown.vertices {
            if let Some(v) = topo.vertex(id) {
                let pick = self.named(topo, id);
                self.vertex(v.point, VERTEX_SIZE, POINT, pick);
            }
        }
        // A thread that is said of a face, not cut into it: a helix on
        // the face, of no more turns than can be told apart.
        for thread in world.threads.values() {
            let face = topo.resolve::<workpiece_core::world::topo::FaceId>(&thread.face);
            if face.is_some_and(|id| !shown.faces.contains(&id)) || thread.pitch <= 0.0 {
                continue;
            }
            const TURNS: f64 = 48.0;
            const PIECES: usize = 16;
            let turns = (thread.length / thread.pitch).min(TURNS);
            let e1 = thread.axis.any_orthonormal_vector();
            let e2 = thread.axis.cross(e1) * if thread.left { -1.0 } else { 1.0 };
            // (A little off the face, to the side that is seen.)
            let off = if thread.internal { 0.99 } else { 1.01 };
            let radius = thread.crests() / 2.0 * off;
            let pieces = (turns * PIECES as f64).ceil().max(1.0) as usize;
            let at = |k: usize| {
                let along = k as f64 / pieces as f64;
                let angle = along * turns * std::f64::consts::TAU;
                thread.origin
                    + thread.axis * (thread.length * along)
                    + (e1 * libm::cos(angle) + e2 * libm::sin(angle)) * radius
            };
            for k in 0..pieces {
                self.line(at(k), at(k + 1), [1.0; 2], crate::palette::THREAD, None);
            }
        }
    }

    fn datum(&mut self, name: &str, datum: &Datum) {
        match datum {
            Datum::Frame(frame) => self.frame(name, frame),
            Datum::Plane(plane) => {
                let pick = self.pick(Source::Datum(name.into()));
                let (o, x, y) = (plane.origin(), plane.x_dir(), plane.y_dir());
                let s = PLANE_SIZE * self.size;
                let corners = [
                    o - x * s - y * s,
                    o + x * s - y * s,
                    o + x * s + y * s,
                    o - x * s + y * s,
                ];
                self.quad(corners, REFERENCE, pick);
            }
            Datum::Axis(axis) => {
                let pick = self.pick(Source::Datum(name.into()));
                let (o, d) = (axis.origin(), axis.direction());
                self.line(
                    o - d * (AXIS_SIZE * self.size),
                    o + d * (AXIS_SIZE * self.size),
                    [AXIS_WIDTH; 2],
                    REFERENCE,
                    pick,
                );
            }
        }
    }

    /// Arrows along the axes, a small plane inset between each pair of
    /// them, and the origin: in front of everything, so a frame is never
    /// lost inside what is built on it.
    fn frame(&mut self, name: &str, frame: &DatumFrame) {
        self.front = true;
        let o = frame.origin();
        for axis in Axis::ALL {
            let pick = self.pick(Source::FrameAxis {
                frame: name.into(),
                axis,
            });
            let color = AXES[axis.index()];
            self.arrow(
                o,
                o + frame.direction(axis) * (FRAME_SIZE * self.size),
                color,
                pick,
            );
            // The plane normal to `axis`, between the other two.
            let (u, v) = (
                frame.direction(axis.next()) * (FRAME_SIZE * self.size),
                frame.direction(axis.next().next()) * (FRAME_SIZE * self.size),
            );
            let (lo, hi) = INSET;
            let corners = [
                o + u * lo + v * lo,
                o + u * hi + v * lo,
                o + u * hi + v * hi,
                o + u * lo + v * hi,
            ];
            self.quad(corners, color, pick);
        }
        let pick = self.pick(Source::Datum(name.into()));
        self.vertex(o, ORIGIN_SIZE, POINT, pick);
        self.front = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_core::world::{Instance, ORIGIN};
    use workpiece_core::{Operation, Standalone};
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};

    fn boxed() -> anyhow::Result<World> {
        let mut b = Standalone::new();
        AddBox {
            id: "box".into(),
            args: AddBoxArgs {
                frame: Some(workpiece_core::Reference::datum(ORIGIN)),
                ..AddBoxArgs::default()
            },
        }
        .apply(&mut b)?;
        Ok(b.world)
    }

    #[test]
    fn a_box_is_written_as_a_mesh_of_its_corners() -> anyhow::Result<()> {
        let file = crate::tessellate::three_mf(&boxed()?.topo, workpiece_core::Unit::Millimetre);
        if let Ok(to) = std::env::var("WORKPIECE_3MF") {
            std::fs::write(to, &file)?;
        }
        let text = String::from_utf8_lossy(&file);
        assert_eq!(text.matches("<vertex ").count(), 8);
        assert_eq!(text.matches("<triangle ").count(), 12);
        Ok(())
    }

    fn entity(source: Source) -> Pick {
        Pick::Entity {
            instance: Vec::new(),
            source,
        }
    }

    #[test]
    fn boxes_show_faces_edges_and_vertices() -> anyhow::Result<()> {
        let mut scene = Scene::default();
        world(
            &mut scene,
            &boxed()?,
            Pose::IDENTITY,
            Style::Normal,
            &Visibility::ALL,
        );
        // Each face is one bilinear cell: two triangles. Plus the origin
        // frame's three inset planes.
        let faces = scene
            .triangles
            .triangles
            .iter()
            .filter(|t| t.color == MODEL)
            .count();
        assert_eq!(faces, 12);
        let top = scene.index_of(&entity(Source::Topology("box/top".into())));
        assert!(top.is_some());
        assert!(
            scene
                .index_of(&entity(Source::Topology("box/edge7".into())))
                .is_some()
        );
        assert!(
            scene
                .index_of(&entity(Source::Topology("box/vertex0".into())))
                .is_some()
        );
        let x = Source::FrameAxis {
            frame: ORIGIN.into(),
            axis: Axis::X,
        };
        assert!(scene.index_of(&entity(x)).is_some());
        Ok(())
    }

    #[test]
    fn instances_are_placed_and_ghosts_not_picked() -> anyhow::Result<()> {
        let mut outer = World::new();
        outer.instances.insert(
            "b1".into(),
            Instance {
                pose: Pose::from_translation(Point3::new(10.0, 0.0, 0.0)),
                world: std::sync::Arc::new(boxed()?),
                fixed: false,
            },
        );
        let mut scene = Scene::default();
        world(
            &mut scene,
            &outer,
            Pose::IDENTITY,
            Style::Normal,
            &Visibility::ALL,
        );
        let top = Pick::Entity {
            instance: vec!["b1".into()],
            source: Source::Topology("box/top".into()),
        };
        let id = scene.index_of(&top);
        assert!(id.is_some());
        let moved = scene
            .triangles
            .triangles
            .iter()
            .filter(|t| t.pick == id)
            .all(|t| t.positions.iter().all(|p| p.x > 9.0));
        assert!(moved);

        let mut ghosts = Scene::default();
        world(
            &mut ghosts,
            &outer,
            Pose::IDENTITY,
            Style::Ghost,
            &Visibility::ALL,
        );
        assert!(ghosts.picks().is_empty());
        assert!(!ghosts.is_empty());
        Ok(())
    }
}

#[cfg(test)]
mod pole_tests {
    use super::*;
    use workpiece_core::{Operation, Reference, Standalone};
    use workpiece_ops_basic_shapes::{AddCylinder, AddCylinderArgs};

    /// A cylinder's rims are closed edges and its seams hidden: no vertex
    /// is a corner, so none shows. A box shows its eight corners.
    #[test]
    fn only_corners_show_as_vertices() -> anyhow::Result<()> {
        let vertices = |world: &World| {
            let mut scene = Scene::default();
            world_of(&mut scene, world);
            scene.vertices.vertices.len()
        };
        let mut b = Standalone::new();
        let frame = Some(Reference::datum(workpiece_core::world::ORIGIN));
        AddCylinder {
            id: "pin".into(),
            args: AddCylinderArgs {
                frame: frame.clone(),
                ..Default::default()
            },
        }
        .apply(&mut b)?;
        b.world.datums.clear();
        assert_eq!(vertices(&b.world), 0);

        let mut b = Standalone::new();
        workpiece_ops_basic_shapes::AddBox {
            id: "box".into(),
            args: workpiece_ops_basic_shapes::AddBoxArgs {
                frame,
                ..Default::default()
            },
        }
        .apply(&mut b)?;
        b.world.datums.clear();
        assert_eq!(vertices(&b.world), 8);
        Ok(())
    }

    fn world_of(scene: &mut Scene, w: &World) {
        world(scene, w, Pose::IDENTITY, Style::Normal, &Visibility::ALL);
    }

    /// A cap placed off the origin: at its centre (a pole) rounding noise
    /// once made random normals.
    #[test]
    fn caps_are_flat_away_from_the_origin() -> anyhow::Result<()> {
        let mut b = Standalone::new();
        let frame = DatumFrame::from_pose(Pose::from_translation(Point3::new(0.5, 0.0, 0.15)))?;
        b.world.datums.insert("top".into(), frame.into());
        let args = AddCylinderArgs {
            frame: Some(Reference::datum("top")),
            radius: 0.25.into(),
            height: 1.0.into(),
        };
        AddCylinder {
            id: "pin".into(),
            args,
        }
        .apply(&mut b)?;
        let mut scene = Scene::default();
        world(
            &mut scene,
            &b.world,
            Pose::IDENTITY,
            Style::Normal,
            &Visibility::ALL,
        );
        let on_cap = |t: &&Triangle| t.positions.iter().all(|p| (p.z - 1.15).abs() < 1e-3);
        let caps: Vec<_> = scene.triangles.triangles.iter().filter(on_cap).collect();
        assert!(!caps.is_empty());
        assert!(
            caps.iter()
                .flat_map(|t| t.normals)
                .all(|n| n.z.abs() > 0.99)
        );
        Ok(())
    }
}
