//! The edit session of a mesh: what is picked in it, the tool in use,
//! and what the pointer does — picking, moving, cutting and drawing.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use workpiece_core::handles::snap::grid_in_plane;
use workpiece_core::handles::{CanvasEventKind, SpaceEvent, SpaceView};
use workpiece_core::math::Point3;
use workpiece_core::{Edit, Pose, Ray};

use crate::MeshArgs;
use crate::ops::{self, Done};
use crate::poly::{Edge, Item, Poly, edge};
use crate::prims::Primitive;

/// What an edit of a mesh changes.
pub type MeshEdit = Edit<MeshArgs, MeshState>;

/// What is picked: points, edges or faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Mode {
    /// Points.
    Vertex,
    /// Edges.
    Edge,
    /// Faces.
    #[default]
    Face,
}

impl Mode {
    /// The mode that picks things like `item`.
    pub fn of(item: &Item) -> Mode {
        match item {
            Item::Vertex(_) => Mode::Vertex,
            Item::Edge(..) => Mode::Edge,
            Item::Face(_) => Mode::Face,
        }
    }
}

/// What the pointer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Tool {
    /// Picks, and moves, turns and scales what is picked by its gizmo.
    #[default]
    Select,
    /// Cuts faces from point to point clicked on their edges.
    Knife,
    /// Cuts a loop of edges around the mesh, across the edge clicked.
    LoopCut,
    /// Draws a face, corner by corner.
    Draw,
    /// Draws lines, point by point: a curve of the subdivision; one
    /// click, a point by itself.
    Line,
}

/// What was done last, with the numbers it was done with: they can
/// still be changed, and it is done again from the mesh as it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Last {
    /// What was done.
    pub what: What,
    /// The mesh it was done to.
    pub base: Arc<Poly>,
    /// What was picked in it.
    pub picked: Vec<Item>,
    /// A length: how far, how wide, how thick.
    pub amount: f64,
    /// A second length: how deep.
    pub depth: f64,
    /// A count: of cuts, of pieces.
    pub count: usize,
    /// A second count.
    pub turn: usize,
    /// A choice: each face by itself.
    pub each: bool,
    /// A vector: how far along x, y and z.
    pub by: Point3,
    /// The frame it is done in: where the gizmo was.
    pub frame: Pose,
}

/// What can be done again with other numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum What {
    /// [`ops::extrude`].
    Extrude,
    /// [`ops::inset`].
    Inset,
    /// [`ops::bevel`].
    Bevel,
    /// [`ops::loop_cut`] across the edge.
    LoopCut(Edge),
    /// [`ops::solidify`].
    Thicken,
    /// [`ops::weld`].
    Weld,
    /// [`ops::relax`].
    Relax,
    /// [`ops::inflate`].
    Inflate,
    /// A mesh to start from, added.
    Add(Primitive),
    /// What is picked moved.
    Move,
    /// What is picked stretched about the frame, along its axes.
    Scale,
}

impl Last {
    /// `what` done to `base` with `picked`, the numbers as they are
    /// when nothing is said.
    pub fn new(what: What, base: &Poly, picked: &[Item]) -> Self {
        let edge = base.mean_edge();
        let (amount, count) = match what {
            What::Extrude => (edge, 0),
            What::Inset => (edge * 0.2, 0),
            What::Bevel => (edge * 0.15, 0),
            What::LoopCut(_) => (0.0, 1),
            What::Thicken => (edge * 0.2, 0),
            What::Weld => (base.size() * 1e-4, 0),
            What::Relax => (0.5, 1),
            What::Inflate => (edge * 0.1, 0),
            What::Add(kind) => (ADD_SIZE, kind.pieces()),
            What::Move | What::Scale => (0.0, 0),
        };
        Self {
            what,
            base: Arc::new(base.clone()),
            picked: picked.to_vec(),
            amount,
            depth: 0.0,
            count,
            turn: 0,
            each: false,
            by: if what == What::Scale {
                Point3::ONE
            } else {
                Point3::ZERO
            },
            frame: Pose::IDENTITY,
        }
    }

    /// It, done.
    pub fn run(&self, args: &MeshArgs, falloff: f64) -> Done {
        let (base, picked) = (&*self.base, &self.picked[..]);
        match self.what {
            What::Extrude => ops::extrude(base, picked, self.amount),
            What::Inset => ops::inset(base, picked, self.amount, self.depth, self.each),
            What::Bevel => ops::bevel(base, picked, self.amount),
            What::LoopCut(across) => ops::loop_cut(base, across, self.count, self.amount),
            What::Thicken => ops::solidify(base, picked, self.amount),
            What::Weld => ops::weld(base, picked, self.amount),
            What::Inflate => ops::inflate(base, picked, self.amount),
            What::Relax => {
                let mut made = base.clone();
                for _ in 0..self.count.clamp(1, 50) {
                    made = ops::relax(&made, picked, self.amount)?.0;
                }
                Ok((made, picked.to_vec()))
            }
            What::Add(kind) => {
                let mut made = base.clone();
                let new = kind.mesh(self.amount, self.count, self.by);
                let (points, faces) = (made.points.len(), made.faces.len());
                made.points.extend(new.points);
                made.faces.extend(
                    new.faces
                        .iter()
                        .map(|r| r.iter().map(|v| v + points).collect()),
                );
                let picked = (faces..made.faces.len()).map(Item::Face).collect();
                Ok((made, picked))
            }
            What::Move => {
                let by = self.by;
                let moved = moved(base, picked, args.mirror, falloff, &|p| p + by);
                Ok((moved, picked.to_vec()))
            }
            What::Scale => {
                let (frame, by) = (self.frame, self.by);
                let back = frame.inverse();
                let stretch = |p: Point3| frame.transform_point(back.transform_point(p) * by);
                let moved = moved(base, picked, args.mirror, falloff, &stretch);
                Ok((moved, picked.to_vec()))
            }
        }
    }
}

/// How large what is added is, in millimetres: a box's edge, a plane's
/// side, the diameter of a cylinder, a cone, a sphere, a torus.
pub const ADD_SIZE: f64 = 10.0;

/// A point the pointer is at, for a tool that cuts or draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cursor {
    /// A point of the mesh.
    Vertex(usize),
    /// A point on an edge: how far along it, from its lower point.
    OnEdge(Edge, f64),
    /// A point of the plane that is drawn in.
    Free(Point3),
}

impl Cursor {
    /// Where it is.
    pub fn at(&self, poly: &Poly) -> Option<Point3> {
        match *self {
            Cursor::Vertex(v) => poly.points.get(v).copied(),
            Cursor::OnEdge((a, b), t) => Some(poly.points.get(a)?.lerp(*poly.points.get(b)?, t)),
            Cursor::Free(p) => Some(p),
        }
    }
}

/// A frame dragged open over what is to be picked: its corners on the
/// screen, and where they are in the scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    /// Where the button went down, on the screen.
    pub from: [f64; 2],
    /// Where the pointer is.
    pub to: [f64; 2],
    /// The corners, in the scene.
    pub corners: [Point3; 4],
}

/// The state of an edit session.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshState {
    /// What kind of thing is picked.
    pub mode: Mode,
    /// The tool in use.
    pub tool: Tool,
    /// What is picked.
    pub picked: Vec<Item>,
    /// What is under the pointer.
    pub hover: Option<Item>,
    /// Where the gizmo is, once it was dragged; else it is in the
    /// middle of what is picked.
    pub gizmo: Option<Pose>,
    /// Whether the gizmo stands along the normal of what is picked
    /// (else along the world's axes).
    pub along_normal: bool,
    /// How far around what is picked the mesh moves with it, less
    /// the farther; none for zero.
    pub falloff: f64,
    /// What was done last and can be done again with other numbers.
    pub last: Option<Last>,
    /// The frame being dragged open.
    pub band: Option<Band>,
    /// What the button went down on, and where: dragging moves it.
    pub grab: Option<(Item, Point3)>,
    /// The point the knife last cut at.
    pub knife: Option<usize>,
    /// Where the pointer is, for the knife and the pen.
    pub cursor: Option<Cursor>,
    /// The corners of the face being drawn; the points of the lines
    /// being drawn.
    pub drawn: Vec<usize>,
    /// The way the eye looked when the pen was last moved: a face
    /// drawn beside no other is turned to face it.
    pub forward: Point3,
    /// Why what was asked last could not be done.
    pub note: Option<String>,
}

impl MeshState {
    /// The state with `tool` in use, nothing half done.
    #[must_use]
    pub fn with_tool(&self, tool: Tool) -> Self {
        Self {
            tool,
            hover: None,
            band: None,
            grab: None,
            knife: None,
            cursor: None,
            drawn: Vec::new(),
            note: None,
            ..self.clone()
        }
    }

    /// The state with `picked` picked, and nothing that hangs on what
    /// was.
    #[must_use]
    pub fn with_picked(&self, picked: Vec<Item>) -> Self {
        let mode = picked.first().map_or(self.mode, Mode::of);
        Self {
            mode,
            picked,
            gizmo: None,
            last: None,
            note: None,
            ..self.clone()
        }
    }
}

/// `poly` with what is picked moved by `to`, and the mesh around it
/// along with it as far as `falloff` reaches; the points in a mirror
/// plane stay in it.
pub fn moved(
    poly: &Poly,
    picked: &[Item],
    mirror: [bool; 3],
    falloff: f64,
    to: &dyn Fn(Point3) -> Point3,
) -> Poly {
    let points = poly.points_of(picked);
    let near = poly.size() * 1e-6;
    let mut made = poly.clone();
    let mut put = |v: usize, weight: f64| {
        let was = poly.points[v];
        let mut now = was.lerp(to(was), weight);
        for axis in 0..3 {
            if mirror[axis] && was[axis].abs() <= near {
                now[axis] = 0.0;
            }
        }
        made.points[v] = now;
    };
    for &v in &points {
        put(v, 1.0);
    }
    if falloff > 0.0 && !points.is_empty() {
        for v in 0..poly.points.len() {
            if points.contains(&v) {
                continue;
            }
            let far = points
                .iter()
                .map(|&p| poly.points[p].distance(poly.points[v]))
                .fold(f64::MAX, f64::min);
            if far < falloff {
                let t = 1.0 - far / falloff;
                put(v, t * t * (3.0 - 2.0 * t));
            }
        }
    }
    made
}

/// The middle of what is picked and how the gizmo stands there.
pub fn pivot(args: &MeshArgs, state: &MeshState) -> Option<Pose> {
    if let Some(pose) = state.gizmo {
        return Some(pose);
    }
    let poly = &args.mesh;
    let points = poly.points_of(&state.picked);
    if points.is_empty() {
        return None;
    }
    let middle = points.iter().map(|&v| poly.points[v]).sum::<Point3>() / points.len() as f64;
    if !state.along_normal {
        return Some(Pose::from_translation(middle));
    }
    Some(frame_at(poly, &state.picked, middle))
}

/// A frame at `middle` whose z is the way what is picked faces.
pub fn frame_at(poly: &Poly, picked: &[Item], middle: Point3) -> Pose {
    let faces = poly.faces_of(picked);
    let mut normal: Point3 = faces.iter().map(|&f| poly.area_normal(f)).sum();
    if normal.length() < 1e-12 {
        let normals = poly.point_normals();
        normal = poly.points_of(picked).iter().map(|&v| normals[v]).sum();
    }
    let Some(z) = normal.try_normalize() else {
        return Pose::from_translation(middle);
    };
    // (x along the first picked edge, if there is one to go by.)
    let along = picked.iter().find_map(|i| match *i {
        Item::Edge(a, b) => Some(poly.points[b] - poly.points[a]),
        Item::Face(f) => {
            let ring = &poly.faces[f];
            Some(poly.points[ring[1]] - poly.points[ring[0]])
        }
        Item::Vertex(_) => None,
    });
    let x = along
        .map(|d| d - z * d.dot(z))
        .and_then(Point3::try_normalize)
        .unwrap_or_else(|| z.any_orthonormal_vector());
    Pose::from_axes(middle, x, z.cross(x))
}

/// What of `kind` the ray points at: the face it goes into first, or
/// the point or edge it passes nearest that no face is in front of.
pub fn pick(poly: &Poly, ray: &Ray, mode: Mode) -> Option<(Item, Point3)> {
    let mut face: Option<(f64, usize)> = None;
    for (f, ring) in poly.faces.iter().enumerate() {
        for k in 1..ring.len().saturating_sub(1) {
            let [a, b, c] = [ring[0], ring[k], ring[k + 1]].map(|v| poly.points[v]);
            if let Some(t) = ray.hit_triangle(a, b, c)
                && face.is_none_or(|(least, _)| t < least)
            {
                face = Some((t, f));
            }
        }
    }
    // What a face hides: all behind it, but for what lies on it.
    let hidden = |t: f64| face.is_some_and(|(at, _)| t > at + ray.radius_at(at) * 4.0);
    match mode {
        Mode::Face => face.map(|(t, f)| (Item::Face(f), ray.at(t))),
        Mode::Vertex => {
            let mut best: Option<(f64, usize)> = None;
            for (v, &p) in poly.points.iter().enumerate() {
                let Some(t) = ray.hit_point(p) else { continue };
                if hidden(t) {
                    continue;
                }
                let off = p.distance(ray.at(t)) / ray.radius_at(t).max(1e-300);
                if best.is_none_or(|(least, _)| off < least) {
                    best = Some((off, v));
                }
            }
            best.map(|(_, v)| (Item::Vertex(v), poly.points[v]))
        }
        Mode::Edge => {
            let mut best: Option<(f64, Edge, Point3)> = None;
            for &(a, b) in &poly.all_edges() {
                let (pa, pb) = (poly.points[a], poly.points[b]);
                let Some(t) = ray.hit_segment(pa, pb) else {
                    continue;
                };
                if hidden(t) {
                    continue;
                }
                let on = nearest_on(pa, pb, ray);
                let off = on.distance(ray.at(t)) / ray.radius_at(t).max(1e-300);
                if best.is_none_or(|(least, ..)| off < least) {
                    best = Some((off, (a, b), on));
                }
            }
            best.map(|(_, (a, b), on)| (Item::Edge(a, b), on))
        }
    }
}

/// The point of the segment from `a` to `b` nearest the ray.
fn nearest_on(a: Point3, b: Point3, ray: &Ray) -> Point3 {
    let Some(dir) = (b - a).try_normalize() else {
        return a;
    };
    let s = ray.closest_on_line(a, dir).unwrap_or(0.0);
    a + dir * s.clamp(0.0, a.distance(b))
}

/// Where the knife would cut: at a point of the mesh, or on an edge
/// (in its middle, if the pointer is near that).
fn knife_at(poly: &Poly, ray: &Ray) -> Option<Cursor> {
    if let Some((Item::Vertex(v), _)) = pick(poly, ray, Mode::Vertex) {
        return Some(Cursor::Vertex(v));
    }
    let (Item::Edge(a, b), on) = pick(poly, ray, Mode::Edge)? else {
        return None;
    };
    let long = poly.points[a].distance(poly.points[b]);
    let mut t = if long > 0.0 {
        on.distance(poly.points[a]) / long
    } else {
        0.5
    };
    let middle = (poly.points[a] + poly.points[b]) / 2.0;
    if ray.hit_point(middle).is_some() {
        t = 0.5;
    }
    Some(Cursor::OnEdge((a, b), t.clamp(0.02, 0.98)))
}

/// The plane a face is drawn in: through `through`, facing along the
/// axis of the world the eye looks along most.
fn drawing_plane(
    view: &SpaceView,
    through: Point3,
    ray: &Ray,
    grid: Option<f64>,
) -> Option<Point3> {
    let forward = view.forward().to_array();
    let axis = (0..3).max_by(|&a, &b| forward[a].abs().total_cmp(&forward[b].abs()))?;
    let mut normal = Point3::ZERO;
    normal[axis] = 1.0;
    let t = ray.intersect_plane(through, normal)?;
    let at = ray.at(t);
    Some(grid.map_or(at, |step| grid_in_plane(at, normal, step)))
}

/// What the pointer's `event` does to the mesh being edited.
pub fn handle(args: &MeshArgs, state: &MeshState, event: SpaceEvent) -> Option<MeshEdit> {
    match state.tool {
        Tool::Select => select(args, state, event),
        Tool::Knife => knife(args, state, event),
        Tool::LoopCut => loop_cut(args, state, event),
        Tool::Draw => draw(args, state, event),
        Tool::Line => lines(args, state, event),
    }
}

/// Where a point the pointer places lands: what it snaps to, else where
/// its ray meets the plane through `through` with unit `normal` — on
/// the grid, while there is one.
pub fn placed(event: &SpaceEvent, through: Point3, normal: Point3) -> Option<Point3> {
    if let Some(snap) = event.snap {
        return Some(snap.at);
    }
    let t = event.ray.intersect_plane(through, normal)?;
    let at = event.ray.at(t);
    Some(match event.grid {
        Some(step) => grid_in_plane(at, normal, step),
        None => at,
    })
}

/// A new state, if it is another than `state`.
fn changed(state: &MeshState, next: MeshState) -> Option<MeshEdit> {
    (next != *state).then(|| Edit::state(next))
}

fn select(args: &MeshArgs, state: &MeshState, event: SpaceEvent) -> Option<MeshEdit> {
    let poly = &args.mesh;
    let (ray, view) = (&event.ray, &event.view);
    let under = || pick(poly, ray, state.mode);
    let mut next = state.clone();
    match event.kind {
        CanvasEventKind::Hover => {
            // (The button may be down: a drag starts with a move.)
            next.hover = under().map(|(item, _)| item);
            changed(state, next)
        }
        CanvasEventKind::Click => {
            let item = under().map(|(item, _)| item);
            let mut picked = state.picked.clone();
            match (item, event.add) {
                (Some(item), true) => {
                    if let Some(k) = picked.iter().position(|i| *i == item) {
                        picked.remove(k);
                    } else {
                        picked.push(item);
                    }
                }
                (Some(item), false) => picked = vec![item],
                (None, true) => {}
                (None, false) => picked.clear(),
            }
            next = next.with_picked(picked);
            next.band = None;
            next.grab = None;
            changed(state, next)
        }
        CanvasEventKind::DoubleClick => {
            let (item, _) = under()?;
            let more: Vec<Item> = match item {
                Item::Edge(a, b) => ops::edge_loop(poly, edge(a, b))
                    .into_iter()
                    .map(|(a, b)| Item::Edge(a, b))
                    .collect(),
                other => ops::items_on(poly, &ops::linked(poly, &[other]), other),
            };
            let mut picked = if event.add {
                state.picked.clone()
            } else {
                Vec::new()
            };
            for item in more {
                if !picked.contains(&item) {
                    picked.push(item);
                }
            }
            changed(state, next.with_picked(picked))
        }
        CanvasEventKind::Press => {
            next.grab = under();
            next.band = None;
            if next.grab.is_none() {
                let at = view.on_screen(ray, ray.at(1.0))?;
                next.band = Some(Band {
                    from: at,
                    to: at,
                    corners: [ray.at(1.0); 4],
                });
            }
            changed(state, next)
        }
        CanvasEventKind::Drag => {
            if let Some((item, at)) = state.grab {
                // What was grabbed moves in the plane facing the eye
                // (onto what it snaps to), with all that is picked if
                // it is of that.
                let to = placed(&event, at, -view.forward())?;
                if !state.picked.contains(&item) {
                    next = next.with_picked(vec![item]);
                }
                next.grab = Some((item, to));
                return Some(translate(args, &next, to - at));
            }
            let band = state.band?;
            let to = view.on_screen(ray, ray.at(1.0))?;
            // (As far off as the mesh is.)
            let middle = poly
                .extent()
                .map_or(Point3::ZERO, |(lo, hi)| (lo + hi) / 2.0);
            let depth = (middle - ray.start()).dot(view.forward()).max(1e-6);
            let corner = |x: f64, y: f64| view.at_screen(ray, [x, y], depth);
            let (a, b) = (band.from, to);
            next.band = Some(Band {
                from: a,
                to,
                corners: [
                    corner(a[0], a[1]),
                    corner(b[0], a[1]),
                    corner(b[0], b[1]),
                    corner(a[0], b[1]),
                ],
            });
            changed(state, next)
        }
        CanvasEventKind::Release => {
            next.grab = None;
            let Some(band) = next.band.take() else {
                return changed(state, next);
            };
            let lo = [band.from[0].min(band.to[0]), band.from[1].min(band.to[1])];
            let hi = [band.from[0].max(band.to[0]), band.from[1].max(band.to[1])];
            let inside = |p: Point3| {
                view.on_screen(ray, p)
                    .is_some_and(|s| (0..2).all(|k| s[k] >= lo[k] && s[k] <= hi[k]))
            };
            let points: BTreeSet<usize> = (0..poly.points.len())
                .filter(|&v| inside(poly.points[v]))
                .collect();
            let like = match state.mode {
                Mode::Vertex => Item::Vertex(0),
                Mode::Edge => Item::Edge(0, 0),
                Mode::Face => Item::Face(0),
            };
            let mut picked = if event.add {
                state.picked.clone()
            } else {
                Vec::new()
            };
            for item in ops::items_on(poly, &points, like) {
                // (A point of no face is no point to pick.)
                if !picked.contains(&item) {
                    picked.push(item);
                }
            }
            changed(state, next.with_picked(picked))
        }
    }
}

/// What is picked moved by `by`: the move it has made in all goes on
/// (and can be typed), from the mesh as it was before it.
pub fn translate(args: &MeshArgs, state: &MeshState, by: Point3) -> MeshEdit {
    let mut last = match &state.last {
        Some(last) if last.what == What::Move && last.picked == state.picked => last.clone(),
        _ => Last::new(What::Move, &args.mesh, &state.picked),
    };
    last.by += by;
    redo(args, state, last)
}

/// `last` done (again): the mesh it gives, and it kept to be done
/// again; or the state saying why it cannot be.
pub fn redo(args: &MeshArgs, state: &MeshState, last: Last) -> MeshEdit {
    match last.run(args, state.falloff) {
        Ok((mesh, picked)) => {
            let keeps_gizmo = matches!(last.what, What::Move | What::Scale);
            let mut next = state.with_picked(picked);
            if keeps_gizmo {
                next.gizmo = state.gizmo;
            }
            next.last = Some(last);
            Edit::both(
                MeshArgs {
                    mesh,
                    ..args.clone()
                },
                next,
            )
        }
        Err(why) => {
            let mut next = state.clone();
            next.note = Some(why);
            Edit::state(next)
        }
    }
}

/// `done` — what an operation gave — as an edit: the new mesh with
/// what is picked in it, or the state saying why it could not be done.
pub fn finish(args: &MeshArgs, state: &MeshState, done: Done) -> MeshEdit {
    match done {
        Ok((mesh, picked)) => Edit::both(
            MeshArgs {
                mesh,
                ..args.clone()
            },
            state.with_picked(picked),
        ),
        Err(why) => {
            let mut next = state.clone();
            next.note = Some(why);
            Edit::state(next)
        }
    }
}

/// The gizmo dragged to `to`: what is picked moves and turns with it.
pub fn drag_gizmo(args: &MeshArgs, state: &MeshState, to: Pose) -> MeshEdit {
    let Some(shown) = pivot(args, state) else {
        return Edit::state(state.clone());
    };
    let turned = to.rotation().angle_between(shown.rotation()) > 1e-9;
    if turned {
        // Turned about where it is.
        let by = to * shown.inverse();
        let mesh = moved(
            &args.mesh,
            &state.picked,
            args.mirror,
            state.falloff,
            &|p| by.transform_point(p),
        );
        let mut next = state.clone();
        next.gizmo = Some(to);
        next.last = None;
        next.note = None;
        return Edit::both(
            MeshArgs {
                mesh,
                ..args.clone()
            },
            next,
        );
    }
    let step = to.translation() - shown.translation();
    let mut next = state.clone();
    next.gizmo = Some(to);
    if step == Point3::ZERO {
        return Edit::state(next);
    }
    translate(args, &next, step)
}

/// What is picked scaled by `by` more in all directions about the
/// gizmo: by its outer ring. How far it is scaled in all goes on (and
/// can be typed), from the mesh as it was.
pub fn scale_all(args: &MeshArgs, state: &MeshState, by: f64) -> MeshEdit {
    let Some(pivot) = pivot(args, state) else {
        return Edit::state(state.clone());
    };
    let mut last = match &state.last {
        Some(last) if last.what == What::Scale && last.picked == state.picked => last.clone(),
        _ => {
            let mut last = Last::new(What::Scale, &args.mesh, &state.picked);
            last.frame = pivot;
            last
        }
    };
    last.by *= by;
    let mut next = state.clone();
    next.gizmo = Some(pivot);
    redo(args, &next, last)
}

fn knife(args: &MeshArgs, state: &MeshState, event: SpaceEvent) -> Option<MeshEdit> {
    let poly = &args.mesh;
    let mut next = state.clone();
    match event.kind {
        CanvasEventKind::Hover => {
            next.cursor = knife_at(poly, &event.ray);
            changed(state, next)
        }
        CanvasEventKind::Click => {
            let Some(cursor) = knife_at(poly, &event.ray) else {
                // (A click beside the mesh ends the cut.)
                next.knife = None;
                return changed(state, next);
            };
            let mut mesh = poly.clone();
            let at = match cursor {
                Cursor::Vertex(v) => v,
                Cursor::OnEdge(e, t) => {
                    let (cut, new) = ops::split_edges(poly, &BTreeMap::from([(e, vec![t])]));
                    mesh = cut;
                    new[&e][0]
                }
                Cursor::Free(_) => return None,
            };
            if let Some(from) = state.knife
                && from != at
            {
                ops::connect(&mut mesh, from, at);
            }
            next.knife = Some(at);
            next.cursor = Some(Cursor::Vertex(at));
            next.picked.clear();
            next.last = None;
            next.gizmo = None;
            Some(Edit::both(
                MeshArgs {
                    mesh,
                    ..args.clone()
                },
                next,
            ))
        }
        CanvasEventKind::DoubleClick => {
            next.knife = None;
            changed(state, next)
        }
        _ => None,
    }
}

fn loop_cut(args: &MeshArgs, state: &MeshState, event: SpaceEvent) -> Option<MeshEdit> {
    let poly = &args.mesh;
    let under = pick(poly, &event.ray, Mode::Edge).map(|(item, _)| item);
    match event.kind {
        CanvasEventKind::Hover => {
            let mut next = state.clone();
            next.hover = under;
            changed(state, next)
        }
        CanvasEventKind::Click => {
            let Item::Edge(a, b) = under? else {
                return None;
            };
            let mut next = state.clone();
            next.hover = None;
            let last = Last::new(What::LoopCut(edge(a, b)), poly, &[]);
            Some(redo(args, &next, last))
        }
        _ => None,
    }
}

/// The face being drawn closed: made, if it has three corners.
pub fn close_drawn(args: &MeshArgs, state: &MeshState) -> MeshEdit {
    let mut next = state.clone();
    let mut corners = std::mem::take(&mut next.drawn);
    corners.dedup();
    next.cursor = None;
    if corners.len() < 3 {
        return Edit::state(next);
    }
    let mut mesh = args.mesh.clone();
    let shared = {
        let have = mesh.edges();
        (0..corners.len())
            .any(|k| have.contains_key(&edge(corners[k], corners[(k + 1) % corners.len()])))
    };
    let f = mesh.add_face(corners);
    let mut next = next.with_picked(vec![Item::Face(f)]);
    next.tool = Tool::Draw;
    // (A face beside no other faces the eye: it was drawn seen from
    // its front.)
    if !shared && mesh.normal(f).dot(state.forward) > 0.0 {
        mesh.faces[f].reverse();
    }
    Edit::both(
        MeshArgs {
            mesh,
            ..args.clone()
        },
        next,
    )
}

fn draw(args: &MeshArgs, state: &MeshState, event: SpaceEvent) -> Option<MeshEdit> {
    let poly = &args.mesh;
    let (ray, view) = (&event.ray, &event.view);
    let at = || -> Option<Cursor> {
        if let Some((Item::Vertex(v), _)) = pick(poly, ray, Mode::Vertex) {
            return Some(Cursor::Vertex(v));
        }
        // (A point of no face is found too: the corners drawn so far.)
        for &v in &state.drawn {
            if ray.hit_point(poly.points[v]).is_some() {
                return Some(Cursor::Vertex(v));
            }
        }
        if let Some(snap) = event.snap {
            return Some(Cursor::Free(snap.at));
        }
        let through = state.drawn.last().map_or(Point3::ZERO, |&v| poly.points[v]);
        drawing_plane(view, through, ray, event.grid).map(Cursor::Free)
    };
    let mut next = state.clone();
    next.forward = view.forward();
    match event.kind {
        CanvasEventKind::Hover => {
            next.cursor = at();
            changed(state, next)
        }
        CanvasEventKind::Click => {
            let cursor = at()?;
            let mut mesh = poly.clone();
            let v = match cursor {
                Cursor::Vertex(v) => v,
                Cursor::Free(p) => {
                    mesh.points.push(p);
                    mesh.points.len() - 1
                }
                Cursor::OnEdge(..) => return None,
            };
            if state.drawn.last() == Some(&v) {
                return None;
            }
            if state.drawn.first() == Some(&v) && state.drawn.len() >= 3 {
                return Some(close_drawn(args, &next));
            }
            next.drawn.push(v);
            next.picked.clear();
            next.last = None;
            next.gizmo = None;
            Some(Edit::both(
                MeshArgs {
                    mesh,
                    ..args.clone()
                },
                next,
            ))
        }
        CanvasEventKind::DoubleClick => Some(close_drawn(args, &next)),
        _ => None,
    }
}

/// The line tool: each click a point, joined by a line to the one
/// before; on a point of the mesh, that point. A double-click ends the
/// run (one click alone leaves a point by itself).
fn lines(args: &MeshArgs, state: &MeshState, event: SpaceEvent) -> Option<MeshEdit> {
    let poly = &args.mesh;
    let (ray, view) = (&event.ray, &event.view);
    let at = || -> Option<Cursor> {
        if event.snap.is_none()
            && let Some((Item::Vertex(v), _)) = pick(poly, ray, Mode::Vertex)
        {
            return Some(Cursor::Vertex(v));
        }
        // (Onto a point of the mesh it snapped to: that point.)
        if let Some(snap) = event.snap
            && let Some(v) = poly
                .points
                .iter()
                .position(|p| p.distance(snap.at) <= poly.size() * 1e-9)
        {
            return Some(Cursor::Vertex(v));
        }
        if let Some(snap) = event.snap {
            return Some(Cursor::Free(snap.at));
        }
        let through = state.drawn.last().map_or(Point3::ZERO, |&v| poly.points[v]);
        drawing_plane(view, through, ray, event.grid).map(Cursor::Free)
    };
    let mut next = state.clone();
    match event.kind {
        CanvasEventKind::Hover => {
            next.cursor = at();
            changed(state, next)
        }
        CanvasEventKind::Click => {
            let cursor = at()?;
            let mut mesh = poly.clone();
            let v = match cursor {
                Cursor::Vertex(v) => v,
                Cursor::Free(p) => {
                    mesh.points.push(p);
                    mesh.points.len() - 1
                }
                Cursor::OnEdge(..) => return None,
            };
            if let Some(&last) = state.drawn.last() {
                if last == v {
                    return None;
                }
                mesh.lines.insert(edge(last, v));
                mesh.tidy();
            }
            next.drawn.push(v);
            next = next.with_picked(vec![Item::Vertex(v)]);
            next.tool = Tool::Line;
            Some(Edit::both(
                MeshArgs {
                    mesh,
                    ..args.clone()
                },
                next,
            ))
        }
        CanvasEventKind::DoubleClick => {
            next.drawn.clear();
            next.cursor = None;
            changed(state, next)
        }
        _ => None,
    }
}
