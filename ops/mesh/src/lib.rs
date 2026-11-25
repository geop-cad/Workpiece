//! Polygon meshes and subdivision surfaces.
//!
//! A [`Mesh`] step holds a control mesh ([`Poly`]): points, and faces
//! that are rings of them. It is edited by its points, edges and
//! faces — picked, moved with a gizmo, extruded, inset, bevelled, cut,
//! bridged, filled, dissolved — and makes a body of it: of flat faces,
//! or (smooth) of the subdivision surface the mesh controls, as NURBS
//! faces that every other step can work on. Its lines (free edges)
//! make curves the same way: the limit curve of their subdivision.
//!
//! * [`poly`]: the mesh.
//! * [`prims`]: the meshes to start from.
//! * [`ops`]: what is done to a mesh.
//! * [`subd`]: the smooth surface of a mesh, as bicubic patches.
//! * [`body`]: the body a mesh makes.
//! * [`session`]: the edit session — what is picked, the tool in use,
//!   what the pointer does.

pub mod body;
mod draw;
pub mod lines;
pub mod ops;
pub mod poly;
pub mod prims;
pub mod session;
pub mod subd;
mod ui;

use std::collections::BTreeMap;
use std::rc::Rc;

use workpiece_core::handles::{Gizmo, Handle, LinearHandle, SnapKind, SnapTargets, Space};
use workpiece_core::math::Point3;
use workpiece_core::{Axis, Builder, EditContext, EditVisualization, Operation, Source, Widget};

pub use poly::{Item, Poly};
pub use prims::Primitive;
pub use session::{MeshState, Mode, Tool};

use session::{Last, MeshEdit, What};

/// The name of the wire body of a mesh step's curves (its lines).
pub fn curves_name(id: &str) -> String {
    format!("{id}/curves")
}

/// The gizmo's size, in pixels.
const GIZMO_SIZE: f64 = 90.0;

/// What a mesh step is made of.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct MeshArgs {
    /// The control mesh.
    pub mesh: Poly,
    /// Whether the body is the smooth surface the mesh controls (else
    /// its flat faces).
    pub smooth: bool,
    /// How many more steps the mesh is subdivided by before the
    /// smooth surface is made of it: more, smaller faces.
    #[serde(default)]
    pub finer: usize,
    /// The planes through the origin the mesh is mirrored in: across
    /// x, y and z. What is edited is the one side; the body is whole.
    #[serde(default)]
    pub mirror: [bool; 3],
    /// Whether what it makes is a surface even where the mesh closes
    /// (made on the Surfacing tab); else a closed mesh makes a solid.
    #[serde(default)]
    pub surface: bool,
}

impl MeshArgs {
    /// The mesh with its mirror images.
    pub fn whole(&self) -> Poly {
        if self.mirror.contains(&true) {
            ops::mirrored(&self.mesh, self.mirror, self.mesh.size() * 1e-6)
        } else {
            self.mesh.clone()
        }
    }
}

/// A mesh is edited in the scene, by its own tools: it refers to
/// nothing and has no fields.
impl workpiece_core::Arguments for MeshArgs {
    fn references(&self) -> Vec<Source> {
        Vec::new()
    }

    fn references_mut(&mut self) -> Vec<&mut Source> {
        Vec::new()
    }

    fn fields(&self, _: &EditContext<'_>) -> Vec<Widget<Self>> {
        Vec::new()
    }
}

/// A mesh, and the body it makes: called after the step.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Mesh {
    /// The operation's id; also the body's name.
    pub id: String,
    /// The mesh and what is made of it.
    pub args: MeshArgs,
}

impl Mesh {
    /// A box `size` across to start from, about the origin: smooth —
    /// a subdivision surface — or of flat faces.
    pub fn starting(id: impl Into<String>, size: f64, smooth: bool) -> Self {
        Self {
            id: id.into(),
            args: MeshArgs {
                mesh: Primitive::Box.mesh(size, 1, Point3::ZERO),
                smooth,
                finer: 0,
                mirror: [false; 3],
                surface: false,
            },
        }
    }

    /// A surface to start from: a flat patch `size` across of three by
    /// three faces about the origin, smooth or of flat faces. It stays
    /// a surface (a sheet body) whatever it is made into.
    pub fn patch(id: impl Into<String>, size: f64, smooth: bool) -> Self {
        Self {
            id: id.into(),
            args: MeshArgs {
                mesh: Primitive::Plane.mesh(size, 3, Point3::ZERO),
                smooth,
                finer: 0,
                mirror: [false; 3],
                surface: true,
            },
        }
    }
}

impl Operation for Mesh {
    type Args = MeshArgs;
    type EditState = MeshState;

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &MeshArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut MeshArgs {
        &mut self.args
    }

    fn make(&self, _: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        let mut out = workpiece_boolean::io::Out::default();
        let whole = self.args.whole();
        // The curves of its lines: a wire body.
        if !whole.lines.is_empty() {
            let curves = lines::curves(&whole, self.args.smooth)?;
            out.made
                .bodies
                .push((curves_name(&self.id), Box::new(lines::Wire(curves))));
        }
        // (A mesh of no faces yet makes no other body.)
        if whole.faces.is_empty() {
            return Ok(out.made);
        }
        let pieces = body::bodies(&whole, self.args.smooth, self.args.finer, &self.id)?;
        for (k, piece) in pieces.iter().enumerate() {
            let name = if k == 0 {
                self.id.clone()
            } else {
                format!("{}/body{}", self.id, k + 1)
            };
            if piece.closed && !self.args.surface {
                out.write(&piece.solid, &name);
            } else {
                out.write_sheet(&piece.solid, &name);
            }
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let faces = self.args.mesh.faces.len();
        let curves = lines::runs(&self.args.mesh).len();
        let curves = match curves {
            0 => String::new(),
            1 => ", 1 curve".to_owned(),
            n => format!(", {n} curves"),
        };
        format!(
            "{faces} face{}{curves}, {}",
            if faces == 1 { "" } else { "s" },
            if self.args.smooth { "smooth" } else { "flat" }
        )
    }

    fn visualize_edit(
        &self,
        _: &EditContext<'_>,
        state: &MeshState,
    ) -> EditVisualization<MeshEdit> {
        let args = Rc::new(self.args.clone());
        // (What is picked may be gone: the mesh was undone under it.)
        let mut state = state.clone();
        state.picked.retain(|item| args.mesh.has(item));
        if state.hover.is_some_and(|item| !args.mesh.has(&item)) {
            state.hover = None;
        }
        let points = args.mesh.points.len();
        state.drawn.retain(|&v| v < points);
        state.knife = state.knife.filter(|&v| v < points);
        let state = Rc::new(state);
        let mut handles: BTreeMap<String, Handle<MeshEdit>> = BTreeMap::new();
        let anchor = args
            .mesh
            .extent()
            .map_or(Point3::ZERO, |(lo, hi)| (lo + hi) / 2.0);
        let space = {
            let (a, s) = (args.clone(), state.clone());
            let draw = move |view: &_| draw::shapes(&a, &s, view);
            let (a, s) = (args.clone(), state.clone());
            Space::new(anchor, draw, move |event| session::handle(&a, &s, event))
                .with_targets(snap_targets(&args.mesh, &state))
                .placing(matches!(state.tool, Tool::Draw | Tool::Line))
        };
        handles.insert("mesh".to_owned(), space.into());
        if let Some(pivot) = session::pivot(&args, &state) {
            // The gizmo moves and turns what is picked, and scales it
            // all at once by its outer ring; an arrow along each axis
            // stretches it, on the other side of the middle, out of the
            // gizmo's way.
            if state.tool == Tool::Select {
                let side = -1.0;
                let (a, s) = (args.clone(), state.clone());
                let (a2, s2) = (args.clone(), state.clone());
                let gizmo =
                    Gizmo::new(pivot, GIZMO_SIZE, move |to| session::drag_gizmo(&a, &s, to))
                        .with_scale(move |by| session::scale_all(&a2, &s2, by));
                handles.insert("gizmo".to_owned(), gizmo.into());
                // As long as what is picked reaches, times how far it
                // is stretched.
                let scaling = state
                    .last
                    .clone()
                    .filter(|l| l.what == What::Scale && l.picked == state.picked);
                let base = scaling.as_ref().map_or(&args.mesh, |l| &*l.base);
                let reach = base
                    .points_of(&state.picked)
                    .iter()
                    .map(|&v| base.points[v].distance(pivot.translation()))
                    .fold(0.0, f64::max);
                // (The arrows stand inside what they stretch.)
                let reach = if reach > 0.0 {
                    reach * 0.5
                } else {
                    base.mean_edge() * 0.5
                };
                let by = scaling.as_ref().map_or(Point3::ONE, |l| l.by);
                for axis in Axis::ALL {
                    let (a, s, scaling) = (args.clone(), state.clone(), scaling.clone());
                    let k = axis.index();
                    let handle = LinearHandle::new(
                        pivot.translation(),
                        pivot.axis(axis) * side,
                        reach * by[k],
                        move |value: f64| {
                            let mut last = scaling.clone().unwrap_or_else(|| {
                                let mut last = Last::new(What::Scale, &a.mesh, &s.picked);
                                last.frame = pivot;
                                last
                            });
                            last.by[k] = value / reach;
                            let mut state = (*s).clone();
                            state.gizmo = Some(pivot);
                            session::redo(&a, &state, last)
                        },
                    );
                    if let Some(handle) = handle {
                        handles.insert(format!("scale-{k}"), handle.into());
                    }
                }
            }
        }
        let session = ui::Session { args, state };
        EditVisualization::new(session.ribbon(), handles)
            .in_ribbon()
            .with_menu(session.menu())
    }
}

/// What of `poly` a point moved or placed in `state` snaps to: its
/// points, the middles of its edges and lines, the middles of its
/// faces, and its edges and lines — but for what moves (what is
/// picked and what is held, while what is picked is moved).
fn snap_targets(poly: &Poly, state: &MeshState) -> SnapTargets {
    let mut moving = std::collections::BTreeSet::new();
    if state.tool == Tool::Select {
        moving = poly.points_of(&state.picked);
        if let Some((item, _)) = state.grab {
            moving.extend(poly.points_of(&[item]));
        }
    }
    let mut targets = SnapTargets::default();
    for (v, &p) in poly.points.iter().enumerate() {
        if !moving.contains(&v) {
            targets.points.push((p, SnapKind::Point));
        }
    }
    for (a, b) in poly.all_edges() {
        if moving.contains(&a) || moving.contains(&b) {
            continue;
        }
        let (pa, pb) = (poly.points[a], poly.points[b]);
        targets.points.push(((pa + pb) / 2.0, SnapKind::Middle));
        targets.segments.push([pa, pb]);
    }
    for (f, ring) in poly.faces.iter().enumerate() {
        if !ring.iter().any(|v| moving.contains(v)) {
            targets.points.push((poly.center(f), SnapKind::Centre));
        }
    }
    targets
}

/// Tells `r` of this crate's operation: its kind, the tools that add
/// it, and its icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<Mesh>("subd");
    r.tool(Tool {
        tab: "Modeling",
        group: "Create",
        label: "SubD",
        icon: "subd",
        key: 'U',
        hint: "A subdivision surface: a mesh of a few faces, edited by its points, edges and faces, that controls a smooth surface — made a NURBS body every other tool works on",
        small: true,
        make: |cx| Mesh::starting((cx.fresh_id)("subd"), session::ADD_SIZE, true).into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Create",
        label: "SubD",
        icon: "subd",
        key: 'V',
        hint: "A subdivision surface: a patch of a few faces, edited by its points, edges and faces, that controls a smooth surface — a surface body (not a solid, even closed) every other tool works on",
        small: true,
        make: |cx| Mesh::patch((cx.fresh_id)("subd"), session::ADD_SIZE, true).into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Create",
        label: "Mesh",
        icon: "mesh",
        key: 'M',
        hint: "A polygon mesh, edited by its points, edges and faces: a surface body of flat faces",
        small: true,
        make: |cx| Mesh::patch((cx.fresh_id)("mesh"), session::ADD_SIZE, false).into(),
    });
    ui::icons(r);
}
