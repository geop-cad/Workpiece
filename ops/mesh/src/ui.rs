//! The mesh's ribbon and context menu: what is picked, the tools, what
//! is done to what is picked, and the numbers of what was done last.

use std::collections::BTreeSet;
use std::rc::Rc;

use workpiece_core::hotkey::{Hotkey, Key};
use workpiece_core::math::Point3;
use workpiece_core::widgets::{Choice, Group, Label, Mood, NumericInput, ToolButton};
use workpiece_core::{Edit, Widget};

use crate::MeshArgs;
use crate::ops::{self, Done};
use crate::poly::{Item, Poly};
use crate::prims::Primitive;
use crate::session::{self, Last, MeshEdit, MeshState, Mode, Tool, What};

/// What the ribbon is built from.
pub(crate) struct Session {
    pub(crate) args: Rc<MeshArgs>,
    pub(crate) state: Rc<MeshState>,
}

/// Something done to what is picked, at the press of a button.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Action {
    Extrude,
    Inset,
    Bevel,
    Subdivide,
    Connect,
    Bridge,
    Fill,
    GridFill,
    Delete,
    Dissolve,
    Merge,
    Collapse,
    Weld,
    Relax,
    Flatten,
    Crease,
    Flip,
    Outwards,
    Triangulate,
    Quads,
    Poke,
    Duplicate,
    Detach,
    Inflate,
    Thicken,
    All,
    Invert,
    Linked,
    Loop,
    Ring,
    Grow,
    Shrink,
    CloseFace,
    ApplyMirror,
}

/// An action's button: its icon, name, hotkey, and what it does.
fn action_info(action: Action) -> (&'static str, &'static str, Hotkey, &'static str) {
    match action {
        Action::Extrude => (
            "mesh-extrude",
            "Extrude",
            Hotkey::key('E'),
            "Push the picked faces out, leaving walls behind them; or give each picked edge a new face. Drag the gizmo or type the distance",
        ),
        Action::Inset => (
            "mesh-inset",
            "Inset",
            Hotkey::key('I'),
            "Set a rim into the picked faces; with a depth, what is left inside is pushed out or in",
        ),
        Action::Bevel => (
            "mesh-bevel",
            "Bevel",
            Hotkey::key('B'),
            "Cut the picked edges away at a slant: a face in place of each. On a subdivision surface, tightens the edge",
        ),
        Action::Subdivide => (
            "mesh-subdivide",
            "Subdivide",
            Hotkey::key('D'),
            "Cut the picked faces in quads about their middles, or the picked edges in two",
        ),
        Action::Connect => (
            "mesh-connect",
            "Connect",
            Hotkey::key('J'),
            "Join two picked points of a face by a new edge across it, or the middles of the picked edges",
        ),
        Action::Bridge => (
            "mesh-bridge",
            "Bridge",
            Hotkey::shift('B'),
            "Faces between two picked runs of edges that are as long, or between two picked faces",
        ),
        Action::Fill => (
            "mesh-fill",
            "Fill",
            Hotkey::key('F'),
            "A face around the picked points or edges: closes a hole, or joins two edges",
        ),
        Action::GridFill => (
            "mesh-grid-fill",
            "Grid fill",
            Hotkey::shift('F'),
            "Fill the hole the picked edges go around with a grid of quads",
        ),
        Action::Delete => (
            "delete",
            "Delete",
            Hotkey::named(Key::Delete),
            "Remove what is picked, and the faces at it: a hole is left",
        ),
        Action::Dissolve => (
            "mesh-dissolve",
            "Dissolve",
            Hotkey::key('X'),
            "Take out what is picked and make one face of those around it: no hole is left",
        ),
        Action::Merge => (
            "mesh-merge",
            "Merge",
            Hotkey::key('M'),
            "Make one point of the picked points, in their middle",
        ),
        Action::Collapse => (
            "mesh-collapse",
            "Collapse",
            Hotkey::shift('M'),
            "Make each picked edge (or run of edges) one point",
        ),
        Action::Weld => (
            "mesh-weld",
            "Weld",
            Hotkey::key('W'),
            "Make one of the points that lie closer together than a distance: of those picked, or of all",
        ),
        Action::Relax => (
            "mesh-relax",
            "Relax",
            Hotkey::shift('R'),
            "Even out what is picked (or all): each point moves towards the middle of those beside it",
        ),
        Action::Flatten => (
            "mesh-flatten",
            "Flatten",
            Hotkey::shift('T'),
            "Move the picked points into the plane they lie nearest",
        ),
        Action::Crease => (
            "mesh-crease",
            "Crease",
            Hotkey::key('C'),
            "Keep the picked edges sharp on the smooth surface; again: smooth",
        ),
        Action::Flip => (
            "mesh-flip",
            "Flip",
            Hotkey::key('N'),
            "Turn the picked faces (or all) to face the other way",
        ),
        Action::Outwards => (
            "mesh-outwards",
            "Outwards",
            Hotkey::shift('N'),
            "Turn all faces to face one way, out of what the mesh encloses",
        ),
        Action::Triangulate => (
            "mesh-triangulate",
            "Triangles",
            Hotkey::key('T'),
            "Cut the picked faces (or all) in triangles",
        ),
        Action::Quads => (
            "mesh-quads",
            "Quads",
            Hotkey::key('Q'),
            "Make quads of the picked triangles (or all), two by two",
        ),
        Action::Poke => (
            "mesh-poke",
            "Poke",
            Hotkey::shift('P'),
            "Cut the picked faces in triangles about their middles",
        ),
        Action::Duplicate => (
            "mesh-duplicate",
            "Duplicate",
            Hotkey::shift('D'),
            "Copy the picked faces in place; drag the gizmo to move the copy away",
        ),
        Action::Detach => (
            "mesh-detach",
            "Split off",
            Hotkey::key('Y'),
            "Cut the picked faces loose from those beside them, to move by themselves",
        ),
        Action::Inflate => (
            "mesh-inflate",
            "Inflate",
            Hotkey::shift('I'),
            "Move what is picked (or all) along the way the mesh faces there: out, or (below zero) in",
        ),
        Action::Thicken => (
            "mesh-thicken",
            "Thicken",
            Hotkey::shift('K'),
            "Give the picked faces (or all) a thickness: a second skin behind them, and walls around",
        ),
        Action::All => (
            "mesh-all",
            "All",
            Hotkey::ctrl('A'),
            "Pick everything; again: nothing",
        ),
        Action::Invert => (
            "mesh-invert",
            "Invert",
            Hotkey::ctrl('I'),
            "Pick what is not picked",
        ),
        Action::Linked => (
            "mesh-linked",
            "Linked",
            Hotkey::ctrl('L'),
            "Pick all that hangs together with what is picked (or double-click a face)",
        ),
        Action::Loop => (
            "mesh-loop",
            "Loop",
            Hotkey::key('L'),
            "Pick the loop of edges on from each picked edge, end to end (or double-click an edge)",
        ),
        Action::Ring => (
            "mesh-ring",
            "Ring",
            Hotkey::shift('L'),
            "Pick the ring of edges across the quads from each picked edge, side by side",
        ),
        Action::Grow => (
            "mesh-grow",
            "Grow",
            Hotkey::ctrl('G'),
            "Pick what is beside what is picked as well",
        ),
        Action::Shrink => (
            "mesh-shrink",
            "Shrink",
            Hotkey::ctrl_shift('G'),
            "Pick no more what is at the rim of what is picked",
        ),
        Action::CloseFace => (
            "mesh-fill",
            "Close face",
            Hotkey::named(Key::Enter),
            "Make the face of the corners drawn so far",
        ),
        Action::ApplyMirror => (
            "mesh-apply-mirror",
            "Apply mirror",
            Hotkey::ctrl('M'),
            "Make the mirror images part of the mesh, to be edited by themselves",
        ),
    }
}

/// A tool's button.
fn tool_info(tool: Tool) -> (&'static str, &'static str, Hotkey, &'static str) {
    match tool {
        Tool::Select => (
            "select",
            "Select",
            Hotkey::named(Key::Escape),
            "Click to pick (Ctrl adds), double-click an edge for its loop or a face for all that hangs together, drag over nothing to pick what is inside; drag the gizmo, or what is picked itself, to move and turn it; drag an arrow on its far side to stretch it",
        ),
        Tool::Scale => (
            "mesh-scale",
            "Scale",
            Hotkey::key('S'),
            "Stretch what is picked: drag an arrow, or type the factors (0 flattens along that axis)",
        ),
        Tool::Knife => (
            "mesh-knife",
            "Knife",
            Hotkey::key('K'),
            "Click point after point on edges and corners: faces are cut from one to the next. Double-click, or click beside the mesh, to end a cut",
        ),
        Tool::LoopCut => (
            "mesh-loop-cut",
            "Loop cut",
            Hotkey::key('R'),
            "Click an edge to cut a loop of new edges around the mesh across it; then type how many and where",
        ),
        Tool::Draw => (
            "mesh-draw",
            "Draw face",
            Hotkey::key('P'),
            "Click the corners of a new face — on points of the mesh, or beside it. Click the first again, double-click or press Enter to close it",
        ),
    }
}

/// What `action` does to `args` in `state`.
fn perform(args: &MeshArgs, state: &MeshState, action: Action) -> MeshEdit {
    let (poly, picked) = (&args.mesh, &state.picked[..]);
    let again = |what: What| session::redo(args, state, Last::new(what, poly, picked));
    let done = |done: Done| session::finish(args, state, done);
    let pick = |picked: Vec<Item>| Edit::state(state.with_picked(picked));
    let like = match state.mode {
        Mode::Vertex => Item::Vertex(0),
        Mode::Edge => Item::Edge(0, 0),
        Mode::Face => Item::Face(0),
    };
    let everything = || {
        let all: BTreeSet<usize> = (0..poly.points.len()).collect();
        ops::items_on(poly, &all, like)
    };
    match action {
        Action::Extrude => again(What::Extrude),
        Action::Inset => again(What::Inset),
        Action::Bevel => again(What::Bevel),
        Action::Thicken => again(What::Thicken),
        Action::Weld => again(What::Weld),
        Action::Relax => again(What::Relax),
        Action::Inflate => again(What::Inflate),
        Action::Detach => done(ops::detach(poly, picked)),
        Action::GridFill => again(What::GridFill),
        Action::Subdivide => done(ops::subdivide(poly, picked)),
        Action::Connect => done(ops::join(poly, picked)),
        Action::Bridge => done(ops::bridge(poly, picked)),
        Action::Fill => done(ops::fill(poly, picked)),
        Action::Delete => done(ops::delete(poly, picked)),
        Action::Dissolve => done(ops::dissolve(poly, picked)),
        Action::Merge => done(ops::merge(poly, picked, false)),
        Action::Collapse => done(ops::merge(poly, picked, true)),
        Action::Flatten => done(ops::flatten(poly, picked)),
        Action::Crease => done(ops::crease(poly, picked)),
        Action::Flip => done(ops::flip(poly, picked)),
        Action::Outwards => done(Ok((ops::outwards(poly), picked.to_vec()))),
        Action::Triangulate => done(ops::triangulate(poly, picked, false)),
        Action::Poke => done(ops::triangulate(poly, picked, true)),
        Action::Quads => done(ops::quads(poly, picked)),
        Action::Duplicate => done(ops::duplicate(poly, picked)),
        Action::All => {
            let all = everything();
            if picked.len() == all.len() {
                pick(Vec::new())
            } else {
                pick(all)
            }
        }
        Action::Invert => pick(
            everything()
                .into_iter()
                .filter(|item| !picked.contains(item))
                .collect(),
        ),
        Action::Linked => pick(ops::items_on(poly, &ops::linked(poly, picked), like)),
        Action::Loop | Action::Ring => {
            let mut found = Vec::new();
            for start in poly.edges_of(picked) {
                let more: Vec<(usize, usize)> = if action == Action::Loop {
                    ops::edge_loop(poly, start).into_iter().collect()
                } else {
                    ops::ring(poly, start).0
                };
                for (a, b) in more {
                    let item = Item::Edge(a.min(b), a.max(b));
                    if !found.contains(&item) {
                        found.push(item);
                    }
                }
            }
            if found.is_empty() {
                let mut next = state.clone();
                next.note = Some("Pick an edge to go on from".to_owned());
                return Edit::state(next);
            }
            pick(found)
        }
        Action::Grow => pick(ops::grown(poly, picked, true)),
        Action::Shrink => pick(ops::grown(poly, picked, false)),
        Action::CloseFace => session::close_drawn(args, state),
        Action::ApplyMirror => Edit::both(
            MeshArgs {
                mesh: args.whole(),
                mirror: [false; 3],
                ..args.clone()
            },
            state.with_picked(Vec::new()),
        ),
    }
}

/// The mesh without the points that are of no face: what a face left
/// half drawn leaves.
fn without_loose(poly: &Poly) -> Poly {
    let used: BTreeSet<usize> = poly.faces.iter().flatten().copied().collect();
    let loose: BTreeSet<usize> = (0..poly.points.len())
        .filter(|v| !used.contains(v))
        .collect();
    poly.without_points(&loose).0
}

impl Session {
    fn action(&self, action: Action) -> ToolButton<MeshEdit> {
        let (icon, label, hotkey, hint) = action_info(action);
        let (args, state) = (self.args.clone(), self.state.clone());
        ToolButton::new(icon, label, hotkey, move || perform(&args, &state, action)).with_hint(hint)
    }

    fn tool(&self, tool: Tool) -> ToolButton<MeshEdit> {
        let (icon, label, hotkey, hint) = tool_info(tool);
        let (args, state) = (self.args.clone(), self.state.clone());
        ToolButton::new(icon, label, hotkey, move || {
            let next = state.with_tool(tool);
            // (The corners of a face left half drawn go.)
            if state.tool == Tool::Draw && !state.drawn.is_empty() {
                let mesh = without_loose(&args.mesh);
                let mut next = next.with_picked(Vec::new());
                next.tool = tool;
                return Edit::both(
                    MeshArgs {
                        mesh,
                        ..(*args).clone()
                    },
                    next,
                );
            }
            Edit::state(next)
        })
        .with_hint(hint)
        .active(self.state.tool == tool)
    }

    fn mode(&self, mode: Mode) -> ToolButton<MeshEdit> {
        let (icon, label, key, hint, like) = match mode {
            Mode::Vertex => ("mesh-points", "Points", '1', "Pick points", Item::Vertex(0)),
            Mode::Edge => ("mesh-edges", "Edges", '2', "Pick edges", Item::Edge(0, 0)),
            Mode::Face => ("mesh-faces", "Faces", '3', "Pick faces", Item::Face(0)),
        };
        let (args, state) = (self.args.clone(), self.state.clone());
        ToolButton::new(icon, label, Hotkey::key(key), move || {
            // What is picked stays picked, as what it is in the new
            // mode: the points of it, or what lies between them.
            let points = args.mesh.points_of(&state.picked);
            let picked = ops::items_on(&args.mesh, &points, like);
            let mut next = state.with_picked(picked);
            next.mode = mode;
            next.hover = None;
            Edit::state(next)
        })
        .with_hint(hint)
        .active(self.state.mode == mode)
    }

    fn primitive(&self, kind: Primitive) -> ToolButton<MeshEdit> {
        let (icon, key) = match kind {
            Primitive::Box => ("mesh-box", '4'),
            Primitive::Grid => ("grid", '5'),
            Primitive::Cylinder => ("mesh-cylinder", '6'),
            Primitive::Cone => ("mesh-cone", '7'),
            Primitive::Sphere => ("mesh-sphere", '8'),
            Primitive::Torus => ("mesh-torus", '9'),
        };
        let (args, state) = (self.args.clone(), self.state.clone());
        ToolButton::new(icon, kind.label(), Hotkey::shift(key), move || {
            let mut last = Last::new(What::Add(kind), &args.mesh, &[]);
            // As large as what is there, about.
            last.amount = if args.mesh.faces.is_empty() {
                2.0
            } else {
                (args.mesh.size() / 2.0 * 100.0).round() / 100.0
            };
            session::redo(&args, &state.with_tool(Tool::Select), last)
        })
        .with_hint(format!(
            "Add a {} to the mesh; then type its size and how many pieces it is cut in",
            kind.label().to_lowercase()
        ))
    }

    fn toggle(
        &self,
        icon: &'static str,
        label: &str,
        hotkey: Hotkey,
        hint: &str,
        on: bool,
        set: impl Fn(&MeshArgs) -> MeshArgs + 'static,
    ) -> ToolButton<MeshEdit> {
        let args = self.args.clone();
        ToolButton::new(icon, label, hotkey, move || Edit::args(set(&args)))
            .with_hint(hint)
            .active(on)
    }

    /// The numbers of what was done last, to be changed.
    fn last_fields(&self) -> Option<Group<MeshEdit>> {
        let last = self.state.last.as_ref()?;
        let field = |label: &str, shown: f64, set: fn(&mut Last, f64)| -> Widget<MeshEdit> {
            let (args, state, last) = (self.args.clone(), self.state.clone(), last.clone());
            NumericInput::new(label, shown, move |x: f64| {
                let mut last = last.clone();
                set(&mut last, x);
                session::redo(&args, &state, last)
            })
            .into()
        };
        let xyz = |fields: &mut Vec<Widget<MeshEdit>>| {
            fields.push(field("X", last.by.x, |l, x| l.by.x = x));
            fields.push(field("Y", last.by.y, |l, x| l.by.y = x));
            fields.push(field("Z", last.by.z, |l, x| l.by.z = x));
        };
        let mut fields = Vec::new();
        let name = match last.what {
            What::Extrude => {
                fields.push(field("Distance", last.amount, |l, x| l.amount = x));
                "Extrude"
            }
            What::Inset => {
                fields.push(field("Thickness", last.amount, |l, x| l.amount = x));
                fields.push(field("Depth", last.depth, |l, x| l.depth = x));
                let (args, state, was) = (self.args.clone(), self.state.clone(), last.clone());
                let options = vec!["Together".to_owned(), "Each face".to_owned()];
                fields.push(
                    Choice::new("Faces", options, usize::from(last.each), move |k| {
                        let mut last = was.clone();
                        last.each = k == 1;
                        session::redo(&args, &state, last)
                    })
                    .into(),
                );
                "Inset"
            }
            What::Bevel => {
                fields.push(field("Width", last.amount, |l, x| l.amount = x.max(0.0)));
                "Bevel"
            }
            What::LoopCut(_) => {
                fields.push(field("Cuts", last.count as f64, |l, x| {
                    l.count = (x.round() as usize).clamp(1, 64);
                }));
                fields.push(field("Slide", last.amount, |l, x| l.amount = x));
                "Loop cut"
            }
            What::Thicken => {
                fields.push(field("Thickness", last.amount, |l, x| l.amount = x));
                "Thicken"
            }
            What::Weld => {
                fields.push(field("Distance", last.amount, |l, x| l.amount = x.max(0.0)));
                "Weld"
            }
            What::Inflate => {
                fields.push(field("Distance", last.amount, |l, x| l.amount = x));
                "Inflate"
            }
            What::Relax => {
                fields.push(field("Strength", last.amount, |l, x| {
                    l.amount = x.clamp(0.0, 1.0);
                }));
                fields.push(field("Repeats", last.count as f64, |l, x| {
                    l.count = (x.round() as usize).clamp(1, 50);
                }));
                "Relax"
            }
            What::GridFill => {
                let span = if last.count == 0 {
                    (last.base.edges_of(&last.picked).len() / 4).max(1)
                } else {
                    last.count
                };
                fields.push(field("Span", span as f64, |l, x| {
                    l.count = (x.round() as usize).max(1);
                }));
                fields.push(field("Turn", last.turn as f64, |l, x| {
                    l.turn = x.round().max(0.0) as usize;
                }));
                "Grid fill"
            }
            What::Add(kind) => {
                fields.push(field("Size", last.amount, |l, x| l.amount = x.abs()));
                fields.push(field("Pieces", last.count as f64, |l, x| {
                    l.count = (x.round() as usize).clamp(1, 64);
                }));
                xyz(&mut fields);
                kind.label()
            }
            What::Move => {
                xyz(&mut fields);
                "Move"
            }
            What::Scale => {
                xyz(&mut fields);
                fields.push(field(
                    "All",
                    (last.by.x + last.by.y + last.by.z) / 3.0,
                    |l, x| {
                        l.by = Point3::splat(x);
                    },
                ));
                "Scale"
            }
        };
        Some(Group::new(name, fields))
    }

    /// What the mesh is, and why what was asked last was not done.
    fn status(&self) -> Label {
        if let Some(note) = &self.state.note {
            return Label::new("Mesh", note.clone(), Mood::Warning);
        }
        let poly = &self.args.mesh;
        let open = poly.edges().values().filter(|s| s.len() == 1).count();
        let text = format!(
            "{} pts, {} faces, {}",
            poly.points.len(),
            poly.faces.len(),
            if open == 0 { "closed" } else { "open" }
        );
        Label::new("Mesh", text, Mood::Plain)
    }

    /// The ribbon: what is picked, the tools, what is done to the
    /// mesh, and how it is made a body.
    pub(crate) fn ribbon(&self) -> Widget<MeshEdit> {
        let group = |label: &str, tools: Vec<ToolButton<MeshEdit>>| -> Widget<MeshEdit> {
            Group::new(label, tools.into_iter().map(Widget::from).collect()).into()
        };
        let mut children: Vec<Widget<MeshEdit>> = vec![self.status().into()];
        let mut select = vec![
            self.tool(Tool::Select),
            self.mode(Mode::Vertex),
            self.mode(Mode::Edge),
            self.mode(Mode::Face),
        ];
        select.extend(
            [
                Action::All,
                Action::Invert,
                Action::Linked,
                Action::Loop,
                Action::Ring,
                Action::Grow,
                Action::Shrink,
            ]
            .map(|a| self.action(a)),
        );
        children.push(group("Select", select));
        let mut add: Vec<ToolButton<MeshEdit>> =
            Primitive::ALL.map(|kind| self.primitive(kind)).to_vec_of();
        add.push(self.tool(Tool::Draw));
        if self.state.tool == Tool::Draw && self.state.drawn.len() >= 3 {
            add.push(self.action(Action::CloseFace));
        }
        children.push(group("Add", add));
        let mut edit = vec![self.action(Action::Extrude), self.action(Action::Inset)];
        edit.push(self.action(Action::Bevel));
        edit.push(self.tool(Tool::LoopCut));
        edit.push(self.tool(Tool::Knife));
        edit.extend(
            [
                Action::Subdivide,
                Action::Connect,
                Action::Bridge,
                Action::Fill,
                Action::GridFill,
            ]
            .map(|a| self.action(a)),
        );
        children.push(group("Edit", edit));
        children.push(group(
            "Remove",
            [
                Action::Delete,
                Action::Dissolve,
                Action::Merge,
                Action::Collapse,
                Action::Weld,
            ]
            .map(|a| self.action(a))
            .to_vec_of(),
        ));
        let mut shape = vec![self.tool(Tool::Scale)];
        shape.extend(
            [
                Action::Relax,
                Action::Flatten,
                Action::Crease,
                Action::Duplicate,
                Action::Detach,
                Action::Inflate,
                Action::Thicken,
                Action::Flip,
                Action::Outwards,
                Action::Triangulate,
                Action::Quads,
                Action::Poke,
            ]
            .map(|a| self.action(a)),
        );
        children.push(group("Shape", shape));
        if let Some(last) = self.last_fields() {
            children.push(last.into());
        }
        // What is made of it.
        let mut surface = vec![self.toggle(
            "subd",
            "Smooth",
            Hotkey::key('H'),
            "The body is the smooth surface the mesh controls; off: its flat faces",
            self.args.smooth,
            |args| MeshArgs {
                smooth: !args.smooth,
                ..args.clone()
            },
        )];
        for (axis, (icon, label, key)) in [
            ("mesh-mirror-x", "Mirror X", 'X'),
            ("mesh-mirror-y", "Mirror Y", 'Y'),
            ("mesh-mirror-z", "Mirror Z", 'Z'),
        ]
        .into_iter()
        .enumerate()
        {
            surface.push(self.toggle(
                icon,
                label,
                Hotkey::shift(key),
                "Mirror the mesh in the plane through the origin across this axis: the side below it is cut away and is from now on the image of the other, which is edited; the points in the plane stay in it",
                self.args.mirror[axis],
                move |args| {
                    let mut mirror = args.mirror;
                    mirror[axis] = !mirror[axis];
                    // Mirrored, the mesh is its upper half: what is
                    // through the plane is cut along it, what is below
                    // is what the mirror makes.
                    let mesh = if mirror[axis] {
                        ops::upper_half(&args.mesh, axis, args.mesh.size() * 1e-6)
                    } else {
                        args.mesh.clone()
                    };
                    MeshArgs {
                        mesh,
                        mirror,
                        ..args.clone()
                    }
                },
            ));
        }
        if self.args.mirror.contains(&true) {
            surface.push(self.action(Action::ApplyMirror));
        }
        let mut surface: Vec<Widget<MeshEdit>> = surface.into_iter().map(Widget::from).collect();
        if self.args.smooth {
            let args = self.args.clone();
            surface.push(
                NumericInput::new("Finer", self.args.finer as f64, move |x: f64| {
                    Edit::args(MeshArgs {
                        finer: x.round().clamp(0.0, 3.0) as usize,
                        ..(*args).clone()
                    })
                })
                .into(),
            );
        }
        children.push(Group::new("Surface", surface).into());
        // How it moves.
        let state = self.state.clone();
        let axes = Choice::new(
            "Gizmo",
            vec!["World".to_owned(), "Normal".to_owned()],
            usize::from(self.state.along_normal),
            move |k| {
                let mut next = (*state).clone();
                next.along_normal = k == 1;
                next.gizmo = None;
                Edit::state(next)
            },
        );
        let state = self.state.clone();
        let snap = NumericInput::new("Step", self.state.snap, move |x: f64| {
            let mut next = (*state).clone();
            next.snap = x.max(0.0);
            Edit::state(next)
        });
        let state = self.state.clone();
        let falloff = NumericInput::new("Falloff", self.state.falloff, move |x: f64| {
            let mut next = (*state).clone();
            next.falloff = x.max(0.0);
            Edit::state(next)
        });
        children.push(Group::new("Move", vec![axes.into(), snap.into(), falloff.into()]).into());
        workpiece_core::widgets::HBox::new(children).into()
    }

    /// What the context menu offers for what is picked.
    pub(crate) fn menu(&self) -> Vec<ToolButton<MeshEdit>> {
        let actions: &[Action] = match self.state.picked.first() {
            None => &[Action::All],
            Some(Item::Vertex(_)) => &[
                Action::Connect,
                Action::Merge,
                Action::Fill,
                Action::Dissolve,
                Action::Delete,
                Action::Flatten,
            ],
            Some(Item::Edge(..)) => &[
                Action::Extrude,
                Action::Bevel,
                Action::Loop,
                Action::Ring,
                Action::Bridge,
                Action::Fill,
                Action::Crease,
                Action::Subdivide,
                Action::Dissolve,
                Action::Delete,
            ],
            Some(Item::Face(_)) => &[
                Action::Extrude,
                Action::Inset,
                Action::Subdivide,
                Action::Bridge,
                Action::Duplicate,
                Action::Dissolve,
                Action::Delete,
                Action::Flip,
            ],
        };
        actions.iter().map(|&a| self.action(a)).collect()
    }
}

/// An array as a vector.
trait ToVecOf<T> {
    fn to_vec_of(self) -> Vec<T>;
}

impl<T, const N: usize> ToVecOf<T> for [T; N] {
    fn to_vec_of(self) -> Vec<T> {
        self.into_iter().collect()
    }
}

/// Tells `r` of the icons of the mesh's tools.
pub(crate) fn icons(r: &mut workpiece_core::Registry) {
    r.icon("subd", include_str!("../icons/subd.svg"));
    r.icon("mesh", include_str!("../icons/mesh.svg"));
    r.icon("mesh-points", include_str!("../icons/mesh-points.svg"));
    r.icon("mesh-edges", include_str!("../icons/mesh-edges.svg"));
    r.icon("mesh-faces", include_str!("../icons/mesh-faces.svg"));
    r.icon("mesh-all", include_str!("../icons/mesh-all.svg"));
    r.icon("mesh-invert", include_str!("../icons/mesh-invert.svg"));
    r.icon("mesh-linked", include_str!("../icons/mesh-linked.svg"));
    r.icon("mesh-loop", include_str!("../icons/mesh-loop.svg"));
    r.icon("mesh-ring", include_str!("../icons/mesh-ring.svg"));
    r.icon("mesh-grow", include_str!("../icons/mesh-grow.svg"));
    r.icon("mesh-shrink", include_str!("../icons/mesh-shrink.svg"));
    r.icon("mesh-box", include_str!("../icons/mesh-box.svg"));
    r.icon("mesh-cylinder", include_str!("../icons/mesh-cylinder.svg"));
    r.icon("mesh-cone", include_str!("../icons/mesh-cone.svg"));
    r.icon("mesh-sphere", include_str!("../icons/mesh-sphere.svg"));
    r.icon("mesh-torus", include_str!("../icons/mesh-torus.svg"));
    r.icon("mesh-draw", include_str!("../icons/mesh-draw.svg"));
    r.icon("mesh-extrude", include_str!("../icons/mesh-extrude.svg"));
    r.icon("mesh-inset", include_str!("../icons/mesh-inset.svg"));
    r.icon("mesh-bevel", include_str!("../icons/mesh-bevel.svg"));
    r.icon("mesh-loop-cut", include_str!("../icons/mesh-loop-cut.svg"));
    r.icon("mesh-knife", include_str!("../icons/mesh-knife.svg"));
    r.icon(
        "mesh-subdivide",
        include_str!("../icons/mesh-subdivide.svg"),
    );
    r.icon("mesh-connect", include_str!("../icons/mesh-connect.svg"));
    r.icon("mesh-bridge", include_str!("../icons/mesh-bridge.svg"));
    r.icon("mesh-fill", include_str!("../icons/mesh-fill.svg"));
    r.icon(
        "mesh-grid-fill",
        include_str!("../icons/mesh-grid-fill.svg"),
    );
    r.icon("mesh-dissolve", include_str!("../icons/mesh-dissolve.svg"));
    r.icon("mesh-merge", include_str!("../icons/mesh-merge.svg"));
    r.icon("mesh-collapse", include_str!("../icons/mesh-collapse.svg"));
    r.icon("mesh-weld", include_str!("../icons/mesh-weld.svg"));
    r.icon("mesh-scale", include_str!("../icons/mesh-scale.svg"));
    r.icon("mesh-relax", include_str!("../icons/mesh-relax.svg"));
    r.icon("mesh-flatten", include_str!("../icons/mesh-flatten.svg"));
    r.icon("mesh-crease", include_str!("../icons/mesh-crease.svg"));
    r.icon(
        "mesh-duplicate",
        include_str!("../icons/mesh-duplicate.svg"),
    );
    r.icon("mesh-thicken", include_str!("../icons/mesh-thicken.svg"));
    r.icon("mesh-detach", include_str!("../icons/mesh-detach.svg"));
    r.icon("mesh-inflate", include_str!("../icons/mesh-inflate.svg"));
    r.icon("mesh-flip", include_str!("../icons/mesh-flip.svg"));
    r.icon("mesh-outwards", include_str!("../icons/mesh-outwards.svg"));
    r.icon(
        "mesh-triangulate",
        include_str!("../icons/mesh-triangulate.svg"),
    );
    r.icon("mesh-quads", include_str!("../icons/mesh-quads.svg"));
    r.icon("mesh-poke", include_str!("../icons/mesh-poke.svg"));
    r.icon("mesh-mirror-x", include_str!("../icons/mesh-mirror-x.svg"));
    r.icon("mesh-mirror-y", include_str!("../icons/mesh-mirror-y.svg"));
    r.icon("mesh-mirror-z", include_str!("../icons/mesh-mirror-z.svg"));
    r.icon(
        "mesh-apply-mirror",
        include_str!("../icons/mesh-apply-mirror.svg"),
    );
}
