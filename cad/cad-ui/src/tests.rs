//! The model and view driven by messages, as the app drives them.

use crate::model::display::Display;
use std::collections::BTreeMap;

use glam::Vec3;
use workpiece_core::math::{Axis, Point3};
use workpiece_core::{Pose, Source, Widget};
use workpiece_ops_basic_shapes::AddBox;
use workpiece_ops_datum::AddFrame;
use workpiece_ops_instances::{AddPart, AddPartArgs};
use workpiece_part::{AnyOperation, Part, Workspace};
use workpiece_rasterize::Pick;

use crate::view::{ProgramStep, View};
use crate::{CameraMove, Candidate, Message, Model, Pointer};

const SIZE: [f32; 2] = [800.0, 600.0];

fn frame(id: &str, x: f64) -> AnyOperation {
    AddFrame::at(id, Pose::from_translation(Point3::new(x, 0.0, 0.0))).into()
}

fn place(id: &str, part: &str) -> AnyOperation {
    AddPart {
        id: id.into(),
        args: AddPartArgs {
            part: part.into(),
            variant: None,
            hold: Default::default(),
            bodies: Vec::new(),
        },
    }
    .into()
}

/// `x` along the x axis: where a placed part is put.
fn along(x: f64) -> Pose {
    Pose::from_translation(Point3::new(x, 0.0, 0.0))
}

/// `m` seen as the tests look at it: parts a few units across, from
/// close by, their datums one unit long.
fn small(mut m: Model) -> Model {
    m.viewport.camera.distance = 6.0;
    m.datum_size = Some(1.0);
    m
}

/// Two empty parts, `bolt` and `nut`.
pub(crate) fn model() -> Model {
    small(Model::new(Workspace::new(BTreeMap::from([
        ("bolt".into(), Part::default()),
        ("nut".into(), Part::default()),
    ]))))
}

/// `bolt` places `nut`, which holds a frame; `bolt` is open.
fn assembly() -> anyhow::Result<Model> {
    let mut m = small(Model::new(Workspace::new(BTreeMap::from([
        (
            "bolt".into(),
            Part::new(vec![frame("f", 0.0), place("nut1", "nut")]).placing("nut1", &along(5.0)),
        ),
        (
            "nut".into(),
            Part {
                program: vec![frame("g", 1.0)],
                ..Default::default()
            },
        ),
    ]))));
    m.update(Message::Open("bolt".into()))?;
    Ok(m)
}

/// Clicks the ribbon's tool `label`, on whichever tab it is. (A box,
/// which has no tool, is added as its tool would.)
fn click(m: &mut Model, label: &str) -> anyhow::Result<()> {
    if label == "Box" {
        let part = m.focused_part().expect("a part");
        let id = m.workspace.part(&part)?.fresh_id("box");
        let op = workpiece_ops_basic_shapes::AddBox {
            id,
            args: Default::default(),
        };
        return m.update(Message::AddOperation {
            part,
            op: op.into(),
        });
    }
    for tab in crate::Tab::ALL {
        let view = View::new(m);
        if let Some(tool) = view.ribbon.tools.iter().find(|t| t.button.label == label) {
            return m.update(scaled(tool.button.click()));
        }
        m.update(Message::SetTab(tab))?;
    }
    let view = View::new(m);
    let Some(tool) = view.ribbon.tools.iter().find(|t| t.button.label == label) else {
        anyhow::bail!("no tool {label:?}");
    };
    m.update(scaled(tool.button.click()))
}

/// `message`, a tool's click, with the step it adds brought down to the
/// size of the tests' parts, which are a few units across: tools start
/// at sizes for parts ten times that.
fn scaled(message: Message) -> Message {
    let Message::AddOperation { part, mut op } = message else {
        return message;
    };
    if let Some(mesh) = op.downcast_ref::<workpiece_ops_mesh::Mesh>() {
        op = workpiece_ops_mesh::Mesh::starting(mesh.id.clone(), 2.0, mesh.args.smooth).into();
    } else if let Some(fillet) = op.downcast_mut::<workpiece_ops_blend::Fillet>() {
        fillet.args.size = workpiece_core::Measure::new(0.1);
    } else if let Some(extrude) = op.downcast_mut::<workpiece_ops_sweep::Extrude>() {
        extrude.args.distance = workpiece_core::Measure::new(1.0);
    } else if let Some(hole) = op.downcast_mut::<workpiece_ops_features::Hole>() {
        hole.args.diameter = workpiece_core::Measure::new(0.2);
        hole.args.depth = workpiece_core::Measure::new(0.3);
    }
    Message::AddOperation { part, op }
}

fn program(m: &Model, part: &str) -> Vec<String> {
    m.workspace.parts[part]
        .program
        .iter()
        .map(|op| op.id().to_owned())
        .collect()
}

/// Where `p` is on screen.
fn screen(m: &Model, p: Vec3) -> [f32; 2] {
    let clip = m
        .viewport
        .camera
        .view_proj(SIZE[0] / SIZE[1])
        .project_point3(p);
    [
        (clip.x + 1.0) / 2.0 * SIZE[0],
        (1.0 - clip.y) / 2.0 * SIZE[1],
    ]
}

/// Picks the frame field's reference by clicking the origin frame's center,
/// as a user would; picking starts by itself when a step opens.
fn pick_origin(m: &mut Model) -> anyhow::Result<()> {
    assert!(m.picking.is_some(), "picking the frame");
    hover(m, Vec3::ZERO)?;
    m.update(Message::Pointer(Pointer::Click { add: false }))
}

fn hover(m: &mut Model, p: Vec3) -> anyhow::Result<()> {
    let position = screen(m, p);
    m.update(Message::Pointer(Pointer::Moved {
        position,
        size: SIZE,
    }))
}

#[test]
fn adding_opens_the_dialog_and_cancelling_removes_the_step() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    click(&mut m, "Box")?;
    assert_eq!(m.editing, ["box1"]);
    let dialog = View::new(&m)
        .edit_dialog
        .ok_or_else(|| anyhow::anyhow!("no dialog"))?;
    assert_eq!(dialog.title, "box1");
    m.update(Message::CancelEdit)?;
    assert!(m.editing.is_empty());
    assert!(program(&m, "bolt").is_empty());
    assert!(!m.can_redo(), "a cancelled edit cannot be redone");

    click(&mut m, "Frame")?;
    m.update(Message::FinishEdit)?;
    assert_eq!(program(&m, "bolt"), ["frame1"]);
    m.update(Message::Undo)?;
    assert!(program(&m, "bolt").is_empty());
    Ok(())
}

#[test]
fn a_part_made_by_a_script_replaces_the_open_one_as_one_undo_step() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    click(&mut m, "Frame")?;
    m.update(Message::FinishEdit)?;
    let mut made = Part::new(vec![frame("a", 1.0), place("p", "made_nut")]);
    made.script = Some("from workpiece import Part".into());
    let placed = BTreeMap::from([("made_nut".to_owned(), Part::new(vec![frame("n", 0.0)]))]);
    m.update(Message::ReplacePart {
        part: Box::new(made.clone()),
        placed,
    })?;
    assert_eq!(program(&m, "bolt"), ["a", "p"]);
    assert_eq!(m.workspace.part("bolt")?.script, made.script);
    assert!(m.workspace.part("made_nut").is_ok());
    m.update(Message::Undo)?;
    assert_eq!(
        program(&m, "bolt"),
        ["frame1"],
        "one undo step takes it all back"
    );
    assert!(m.workspace.part("made_nut").is_err());
    m.update(Message::Redo)?;
    assert_eq!(program(&m, "bolt"), ["a", "p"]);
    Ok(())
}

#[test]
fn edits_of_one_step_are_one_undo_step_until_ended() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    click(&mut m, "Frame")?;
    let x = |m: &Model| match m.workspace.parts["bolt"].program[0]
        .downcast_ref::<workpiece_ops_datum::AddFrame>()
    {
        Some(f) => f.args.pose.translation().x,
        None => f64::NAN,
    };
    let set_x = |m: &Model, v: f64| -> anyhow::Result<_> {
        let dialog = View::new(m)
            .edit_dialog
            .ok_or_else(|| anyhow::anyhow!("no dialog"))?;
        let Widget::VBox(fields) = dialog.fields() else {
            anyhow::bail!("expected the fields");
        };
        let Some(Widget::PoseInput(pose)) = fields.children.get(2) else {
            anyhow::bail!("expected the pose fields");
        };
        pose.set_position(Axis::X, v)
            .ok_or_else(|| anyhow::anyhow!("no edit"))
    };
    for v in 1..=3 {
        let message = set_x(&m, f64::from(v))?;
        m.update(message)?;
    }
    m.update(Message::EndEdit)?;
    let message = set_x(&m, 9.0)?;
    m.update(message)?;
    assert_eq!(x(&m), 9.0);
    m.update(Message::Undo)?;
    assert_eq!(x(&m), 3.0);
    m.update(Message::Undo)?;
    assert_eq!(x(&m), 0.0);
    Ok(())
}

#[test]
fn placed_parts_are_tree_nodes_and_edit_in_context() -> anyhow::Result<()> {
    let mut m = assembly()?;
    // What is placed in it is counted: its bill of materials.
    assert_eq!(View::new(&m).overview.placed, [("nut".to_owned(), 1)]);
    let steps = View::new(&m).timeline.steps;
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[1].children.as_deref(), Some(&[][..]), "collapsed");
    m.update(Message::Toggle(vec!["nut1".into()]))?;
    let steps = View::new(&m).timeline.steps;
    let children: &[ProgramStep] = steps[1].children.as_deref().unwrap_or_default();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].path, ["nut1", "g"]);

    // Editing the nut's frame from inside the bolt: the bolt up to the
    // nut is a ghost, the nut is in color, placed where the bolt puts it.
    m.update(Message::Edit(vec!["nut1".into(), "g".into()]))?;
    assert_eq!(m.focus, ["nut1"]);
    let shown = &m.viewport.shown;
    assert!(shown.error.is_none(), "{:?}", shown.error);
    assert!(!shown.ghosts.is_empty());
    assert!(shown.ghosts.picks().is_empty());
    assert!(
        shown
            .pose
            .translation()
            .abs_diff_eq(Point3::new(5.0, 0.0, 0.0), 1e-12)
    );
    assert!(
        !shown.before.datums.contains_key("g"),
        "before the edited step"
    );
    let g = Pick::Entity {
        instance: Vec::new(),
        source: Source::Datum("g".into()),
    };
    assert!(shown.scene.index_of(&g).is_some());
    // The dialog's edits go to the nut's definition.
    let dialog = View::new(&m)
        .edit_dialog
        .ok_or_else(|| anyhow::anyhow!("no dialog"))?;
    let Widget::VBox(fields) = dialog.fields() else {
        anyhow::bail!("expected the fields");
    };
    let Some(Widget::PoseInput(pose)) = fields.children.get(2) else {
        anyhow::bail!("expected the pose fields");
    };
    let Some(Message::ReplaceOperation { part, .. }) = pose.set_angle(Axis::Z, 45.0) else {
        anyhow::bail!("expected an edit");
    };
    assert_eq!(part, "nut");
    Ok(())
}

#[test]
fn placing_a_part_opens_its_file_field() -> anyhow::Result<()> {
    let mut m = assembly()?;
    click(&mut m, "Add part")?;
    let dialog = View::new(&m)
        .edit_dialog
        .ok_or_else(|| anyhow::anyhow!("no dialog"))?;
    let Widget::VBox(fields) = dialog.fields() else {
        anyhow::bail!("expected fields");
    };
    let Some(Widget::FileInput(file)) = fields.children.first() else {
        anyhow::bail!("expected the file field");
    };
    assert_eq!(file.extension, "wpc");
    // No part chosen yet: the step cannot be built.
    assert!(m.viewport.shown.error.is_some());
    m.update(file.set("nut"))?;
    assert!(
        m.viewport.shown.error.is_none(),
        "{:?}",
        m.viewport.shown.error
    );
    Ok(())
}

#[test]
fn a_placed_part_brings_the_bodies_ticked() -> anyhow::Result<()> {
    use anyhow::Context;
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};
    let cube = |id: &str, frame: &str, size: f64| -> AnyOperation {
        AddBox {
            id: id.into(),
            args: AddBoxArgs {
                frame: Some(workpiece_core::Reference::datum(frame)),
                size: [size; 3].map(workpiece_core::Measure::new),
            },
        }
        .into()
    };
    // `pair` has two bodies, a cube of 1 and one of 2 beside it;
    // `bolt` places it.
    let pair = Part::new(vec![
        frame("there", 3.0),
        cube("a", "origin", 1.0),
        cube("b", "there", 2.0),
    ]);
    let mut m = small(Model::new(Workspace::new(BTreeMap::from([
        ("pair".into(), pair),
        ("bolt".into(), Part::new(vec![place("p", "pair")])),
    ]))));
    m.update(Message::Open("bolt".into()))?;
    let bodies = |m: &Model| {
        let placed = &m.viewport.shown.world.instances["p"].world;
        let topo = &placed.topo;
        let mut names: Vec<String> = topo
            .bodies()
            .iter()
            .filter_map(|(id, _)| topo.name_of(id).map(str::to_owned))
            .collect();
        names.sort();
        names
    };
    assert_eq!(bodies(&m), ["a", "b"]);
    let whole = m.viewport.shown.world.instances["p"].world.stamp;
    // Its dialog ticks each of them; the one left out stays behind.
    m.update(Message::Edit(vec!["p".into()]))?;
    let dialog = View::new(&m).edit_dialog.context("no dialog")?;
    let Widget::VBox(fields) = dialog.fields() else {
        anyhow::bail!("expected fields");
    };
    let tick = |name: &str| {
        fields.children.iter().find_map(|w| match w {
            Widget::Choice(c) if c.label == name => Some(c),
            _ => None,
        })
    };
    let b = tick("b").context("a tick for b")?;
    assert_eq!(b.selected, 0);
    m.update(b.set(1).context("left out")?)?;
    m.update(Message::FinishEdit)?;
    let Some(AddPart { args, .. }) = m.workspace.parts["bolt"].program[0].downcast_ref::<AddPart>()
    else {
        anyhow::bail!("the step");
    };
    assert_eq!(args.bodies, ["a"]);
    assert_eq!(bodies(&m), ["a"]);
    // What comes along is that body whole: the cube of 1.
    let volume = |m: &Model| -> anyhow::Result<f64> {
        let placed = &m.viewport.shown.world.instances["p"].world;
        let topo = &placed.topo;
        let body = topo.resolve::<workpiece_core::world::topo::BodyId>("a");
        let body = body.context("the body a")?;
        Ok(workpiece_boolean::measure(&workpiece_boolean::read(topo, body)?).volume)
    };
    assert!((volume(&m)? - 1.0).abs() < 1e-9);
    // (Not drawn as the whole part is: it is another world.)
    assert_ne!(m.viewport.shown.world.instances["p"].world.stamp, whole);
    // The part it is of still builds it: changed there, it is changed
    // here.
    m.update(Message::ReplaceOperation {
        part: "pair".into(),
        op: cube("a", "origin", 1.5),
    })?;
    m.update(Message::FinishEdit)?;
    let placed = m.viewport.shown.world.instances["p"].world.clone();
    let (_, hi) = placed.extent().context("a body")?;
    assert!((hi.x - 0.75).abs() < 1e-9, "{hi:?}");
    Ok(())
}

#[test]
fn used_datums_hide_until_shown() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    let datum = |m: &Model, name: &str| {
        let overview = View::new(m).overview;
        overview
            .datums
            .into_iter()
            .find(|i| i.name == name)
            .map(|i| (i.visible, i.auto_hidden))
    };
    click(&mut m, "Frame")?;
    m.update(Message::FinishEdit)?;
    assert_eq!(datum(&m, "frame1"), Some((true, false)));
    // A box standing on the frame hides it; the origin, used or not,
    // always shows.
    click(&mut m, "Box")?;
    let frame1 = AnyOperation::from(workpiece_ops_basic_shapes::AddBox {
        id: "box1".into(),
        args: workpiece_ops_basic_shapes::AddBoxArgs {
            frame: Some(workpiece_core::Reference::datum("frame1")),
            ..Default::default()
        },
    });
    m.update(Message::ReplaceOperation {
        part: "bolt".into(),
        op: frame1,
    })?;
    m.update(Message::FinishEdit)?;
    assert_eq!(datum(&m, "frame1"), Some((false, true)));
    assert!(m.viewport.shown.visibility.datums.contains("frame1"));
    assert_eq!(datum(&m, "origin"), Some((true, false)));
    // Back in time, before the box, nothing uses the frame yet; forward
    // again, the box does.
    m.update(Message::Scrub(Some(1)))?;
    assert_eq!(datum(&m, "frame1"), Some((true, false)));
    assert!(!m.viewport.shown.visibility.datums.contains("frame1"));
    m.update(Message::Scrub(None))?;
    assert_eq!(datum(&m, "frame1"), Some((false, true)));
    // The user overrides that, and hides the box.
    m.update(Message::SetVisible {
        name: "frame1".into(),
        visible: true,
    })?;
    assert_eq!(datum(&m, "frame1"), Some((true, false)));
    m.update(Message::SetVisible {
        name: "box1".into(),
        visible: false,
    })?;
    assert!(m.viewport.shown.visibility.bodies.contains("box1"));
    // The part's numbers are inches from now on; undone, millimetres.
    m.update(Message::SetUnit(workpiece_core::Unit::Inch))?;
    assert_eq!(View::new(&m).overview.unit.label(), "in");
    m.update(Message::Undo)?;
    assert_eq!(m.unit(), workpiece_core::Unit::Millimetre);
    // The part is made of brass from now on: its bodies show as that.
    m.update(Message::SetPartMaterial(Some("Brass".into())))?;
    assert_eq!(m.viewport.shown.world.material.as_deref(), Some("Brass"));
    assert_eq!(
        View::new(&m).overview.bodies[0].colour,
        Some([201, 170, 92])
    );
    let overview = View::new(&m).overview;
    assert_eq!(
        overview
            .bodies
            .iter()
            .map(|i| (i.name.as_str(), i.visible))
            .collect::<Vec<_>>(),
        [("box1", false)]
    );
    Ok(())
}

#[test]
fn files_load_save_and_close() -> anyhow::Result<()> {
    let mut m = Model::default();
    let nut = Part {
        program: vec![frame("g", 1.0)],
        ..Default::default()
    };
    let bolt = Part::new(vec![place("nut1", "nut.wpc")]).placing("nut1", &along(1.0));
    m.update(Message::Load {
        parts: BTreeMap::from([("bolt.wpc".into(), bolt), ("nut.wpc".into(), nut)]),
        open: Some("bolt.wpc".into()),
    })?;
    assert_eq!(m.active_part.as_deref(), Some("bolt.wpc"));
    assert!(!m.is_dirty("bolt.wpc"));
    // A file opened is one opened recently.
    assert_eq!(m.settings.recent, ["bolt.wpc"]);
    click(&mut m, "Frame")?;
    m.update(Message::FinishEdit)?;
    assert!(m.is_dirty("bolt.wpc"));
    m.update(Message::Saved("bolt.wpc".into()))?;
    assert!(!m.is_dirty("bolt.wpc"));
    // Saving as another file; then closing drops it but keeps the nut
    // only while something places it.
    m.update(Message::SaveAs("bolt2.wpc".into()))?;
    assert!(m.is_dirty("bolt2.wpc"), "never written");
    assert!(!m.workspace.parts.contains_key("bolt.wpc"));
    m.update(Message::Close)?;
    assert_eq!(m.active_part, None);
    m.update(Message::New("new.wpc".into()))?;
    assert_eq!(m.active_part.as_deref(), Some("new.wpc"));
    assert!(m.is_dirty("new.wpc"), "never saved");
    assert!(m.update(Message::New("new.wpc".into())).is_err());
    m.update(Message::Close)?;
    assert!(!m.workspace.parts.contains_key("bolt2.wpc"));
    assert!(m.workspace.parts.contains_key("nut.wpc"));
    // Recent files, the latest first: a new part, never written, is
    // none; one saved or opened is.
    assert_eq!(m.settings.recent, ["bolt.wpc"]);
    m.update(Message::Open("nut.wpc".into()))?;
    assert_eq!(m.settings.recent, ["nut.wpc", "bolt.wpc"]);
    m.update(Message::Open("nut.wpc".into()))?;
    assert_eq!(m.settings.recent, ["nut.wpc", "bolt.wpc"], "once each");
    Ok(())
}

#[test]
fn steps_go_in_at_the_scrubber_and_an_edit_comes_back_to_it() -> anyhow::Result<()> {
    let mut m = model();
    m.workspace.part_mut("bolt")?.program = vec![frame("a", 0.0), frame("b", 1.0), frame("c", 2.0)];
    m.update(Message::Open("bolt".into()))?;
    // Scrubbed back to after the first step: a step added goes in
    // there, and the scrubber after it.
    m.update(Message::Scrub(Some(1)))?;
    click(&mut m, "Frame")?;
    let added = program(&m, "bolt")[1].clone();
    assert_eq!(program(&m, "bolt"), ["a", added.as_str(), "b", "c"]);
    assert_eq!(m.scrub, Some(2));
    m.update(Message::FinishEdit)?;
    assert_eq!((m.scrub, m.shown_steps()), (Some(2), 2));
    // A step after the scrubber edited: shown while it is, and when the
    // edit is done the scrubber is where it was.
    m.update(Message::Edit(vec!["c".into()]))?;
    assert_eq!(m.shown_steps(), 4);
    m.update(Message::FinishEdit)?;
    assert_eq!((m.scrub, m.shown_steps()), (Some(2), 2));
    // A step added there and cancelled: the scrubber stays.
    click(&mut m, "Frame")?;
    assert_eq!((program(&m, "bolt").len(), m.scrub), (5, Some(3)));
    m.update(Message::CancelEdit)?;
    assert_eq!((program(&m, "bolt").len(), m.scrub), (4, Some(2)));
    // With all shown, at the end.
    m.update(Message::Scrub(None))?;
    click(&mut m, "Pattern")?;
    let last = m.workspace.parts["bolt"].program.last().expect("a step");
    let pattern = last
        .downcast_ref::<workpiece_ops_bodies::Pattern>()
        .expect("the pattern, last");
    // (Four of what is repeated, to begin with.)
    assert_eq!((pattern.args.count, m.scrub), (4, None));
    Ok(())
}

#[test]
fn the_scrubber_shows_fewer_steps() -> anyhow::Result<()> {
    let mut m = assembly()?;
    let shown = |m: &Model| m.viewport.shown.before.instances.len();
    assert_eq!(shown(&m), 1);
    m.update(Message::Scrub(Some(1)))?;
    assert_eq!(shown(&m), 0);
    let timeline = View::new(&m).timeline;
    assert_eq!(timeline.scrubber.map(|s| (s.at, s.of)), Some((1, 2)));
    assert!(timeline.steps[1].rolled_back);
    m.update(Message::Focus(vec!["nut1".into()]))?;
    assert_eq!(m.scrub, None, "focusing shows the whole part");
    assert!(!m.viewport.shown.ghosts.is_empty());
    m.update(Message::Focus(Vec::new()))?;
    assert!(m.viewport.shown.ghosts.is_empty());
    assert!(
        m.update(Message::Focus(vec!["f".into()])).is_err(),
        "f places nothing"
    );
    Ok(())
}

#[test]
fn hovering_finds_candidates_and_clicking_selects_or_picks() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    m.update(Message::Camera(CameraMove::LookFrom([0.3, -0.2, 1.0])))?;
    m.update(Message::Tick(1.0))?;
    click(&mut m, "Box")?;
    pick_origin(&mut m)?;
    assert_eq!(m.picking, None, "nothing more to pick");
    m.update(Message::FinishEdit)?;

    // Over the middle of the top: the top face (the origin frame's z
    // arrow pokes out through it, too).
    hover(&mut m, Vec3::new(0.2, 0.15, 0.5))?;
    let top = Candidate::Reference {
        instance: Vec::new(),
        source: Source::Topology("box1/top".into()),
    };
    assert_eq!(m.viewport.hovered(), Some(&top));
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(m.selection, std::slice::from_ref(&top));
    let n = m.viewport.candidates.len();
    assert!(n >= 2, "the bottom is under the top");
    m.update(Message::Pointer(Pointer::Cycle))?;
    assert_ne!(m.viewport.hovered(), Some(&top));
    // Moving starts again at the first candidate.
    hover(&mut m, Vec3::new(0.21, 0.15, 0.5))?;
    assert_eq!(m.viewport.hovered(), Some(&top));

    // Editing an offset plane and picking its plane: only planes are
    // candidates, and clicking sets the field.
    // Opening a plane starts picking into its references: nothing is
    // preselected.
    m.update(Message::ClearSelection)?;
    click(&mut m, "Plane")?;
    assert_eq!(m.picking, Some(vec![0]), "the reference box");
    hover(&mut m, Vec3::new(0.2, 0.15, 0.5))?;
    assert_eq!(m.viewport.hovered(), Some(&top));
    assert!(
        m.viewport.candidates.iter().all(|p| match p {
            Candidate::Reference {
                source: Source::Topology(n),
                ..
            } => !n.contains("edge"),
            _ => true,
        }),
        "edges are no planes"
    );
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(m.picking, Some(vec![0]), "the box keeps picking");
    let Some(op) =
        m.workspace.parts["bolt"].program[1].downcast_ref::<workpiece_ops_datum::AddPlane>()
    else {
        anyhow::bail!("expected the plane");
    };
    assert_eq!(op.args.references, [Source::Topology("box1/top".into())]);
    assert_eq!(op.args.mode, workpiece_ops_datum::PlaneMode::Offset);
    // A frame, though it can stand at a pose alone, starts picking its
    // references as well.
    m.update(Message::FinishEdit)?;
    m.update(Message::ClearSelection)?;
    click(&mut m, "Frame")?;
    assert_eq!(m.picking, Some(vec![0]), "the frame's reference box");
    hover(&mut m, Vec3::new(0.2, 0.15, 0.5))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    let Some(op) =
        m.workspace.parts["bolt"].program[2].downcast_ref::<workpiece_ops_datum::AddFrame>()
    else {
        anyhow::bail!("expected the frame");
    };
    // (On the plane just made there.)
    assert_eq!(op.args.references, [Source::Datum("plane1".into())]);
    assert_eq!(op.args.mode, workpiece_ops_datum::FrameMode::OnPlane);
    Ok(())
}

#[test]
fn dragging_a_handle_edits_the_step() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    m.update(Message::Camera(CameraMove::LookFrom([0.0, -1.0, 0.0])))?;
    m.update(Message::Tick(1.0))?;
    click(&mut m, "Box")?;
    pick_origin(&mut m)?;
    // The z arrow's knob at the top's centre.
    hover(&mut m, Vec3::new(0.0, 0.0, 0.5))?;
    let z = Candidate::Handle {
        name: "z".into(),
        part: None,
    };
    assert_eq!(m.viewport.hovered(), Some(&z));
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(&mut m, Vec3::new(0.0, 0.0, 1.0))?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let Some(AddBox { args, .. }) = m.workspace.parts["bolt"].program[0].downcast_ref::<AddBox>()
    else {
        anyhow::bail!("expected the box");
    };
    assert!((args.size[2].value - 2.0).abs() < 1e-3, "{}", args.size[2]);
    assert_eq!(args.size[Axis::X.index()].value, 1.0);
    // One undo step for the drag, one for adding the box.
    m.update(Message::Undo)?;
    assert_eq!(program(&m, "bolt"), ["box1"]);
    Ok(())
}

#[test]
fn editing_in_context_ghosts_what_comes_after() -> anyhow::Result<()> {
    let mut m = assembly()?;
    // The nut gets a second frame after the one edited.
    m.update(Message::Focus(vec!["nut1".into()]))?;
    m.update(Message::AddOperation {
        part: "nut".into(),
        op: frame("h", 2.0),
    })?;
    m.update(Message::FinishEdit)?;
    m.update(Message::Edit(vec!["nut1".into(), "g".into()]))?;
    // Ghosts: the bolt without the nut (its origin and f), and the nut's
    // later frame h — not what is shown in color.
    let origins = |m: &Model| -> usize {
        let ghosts = &m.viewport.shown.ghosts;
        ghosts
            .items
            .iter()
            .map(|i| i.scene.front.vertices.len())
            .sum()
    };
    assert_eq!(origins(&m), 3);
    // Without the preview, neither the edited step nor what follows it
    // is built; the bolt around stays.
    m.update(Message::SetPreview(false))?;
    assert_eq!(origins(&m), 2);
    assert!(!m.viewport.shown.world.datums.contains_key("g"));
    Ok(())
}

#[test]
fn without_the_preview_edits_do_not_rebuild() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    click(&mut m, "Box")?;
    m.update(Message::SetPreview(false))?;
    let scene = m.viewport.shown.scene.clone();
    // The width field: its value, and what setting it to 3 sends.
    let field = |m: &Model| -> anyhow::Result<_> {
        let dialog = View::new(m)
            .edit_dialog
            .ok_or_else(|| anyhow::anyhow!("no dialog"))?;
        assert!(!dialog.preview);
        let Widget::VBox(fields) = dialog.fields() else {
            anyhow::bail!("expected fields");
        };
        let Some(Widget::HBox(sizes)) = fields.children.get(1) else {
            anyhow::bail!("expected the sizes");
        };
        let Some(Widget::NumericInput(width)) = sizes.children.first() else {
            anyhow::bail!("expected the width");
        };
        Ok((width.value, width.set(3.0)))
    };
    let message = field(&m)?.1.ok_or_else(|| anyhow::anyhow!("no edit"))?;
    m.update(message)?;
    assert!(
        std::sync::Arc::ptr_eq(&scene, &m.viewport.shown.scene),
        "not rebuilt"
    );
    assert_eq!(field(&m)?.0, 3.0, "the dialog shows the edit");
    m.update(Message::FinishEdit)?;
    assert!(
        !std::sync::Arc::ptr_eq(&scene, &m.viewport.shown.scene),
        "rebuilt on closing"
    );
    Ok(())
}

#[test]
fn a_drag_grabs_what_was_pressed() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    click(&mut m, "Frame")?;
    m.update(Message::Camera(CameraMove::LookFrom([0.0, -1.0, 0.0])))?;
    m.update(Message::Tick(1.0))?;
    // Press on the gizmo's center, which moves freely.
    hover(&mut m, Vec3::ZERO)?;
    let free = Candidate::Handle {
        name: "pose".into(),
        part: Some(workpiece_core::handles::GizmoPart::Free),
    };
    assert_eq!(m.viewport.hovered(), Some(&free));
    m.update(Message::Pointer(Pointer::Press))?;
    // The drag only starts once the pointer has moved (here onto the x
    // arrow), but grabs the center, and moves from where it was pressed.
    hover(&mut m, Vec3::new(0.3, 0.0, 0.0))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    assert_eq!(m.viewport.dragging, Some(free));
    hover(&mut m, Vec3::new(0.3, 0.0, 0.5))?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let Some(f) =
        m.workspace.parts["bolt"].program[0].downcast_ref::<workpiece_ops_datum::AddFrame>()
    else {
        anyhow::bail!("expected the frame");
    };
    let t = f.args.pose.translation();
    assert!((t.x - 0.3).abs() < 1e-3 && (t.z - 0.5).abs() < 1e-3, "{t}");
    Ok(())
}

/// A part with a box, open.
fn with_box() -> anyhow::Result<Model> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    m.update(Message::Camera(CameraMove::LookFrom([0.3, -0.2, 1.0])))?;
    m.update(Message::Tick(1.0))?;
    click(&mut m, "Box")?;
    pick_origin(&mut m)?;
    m.update(Message::FinishEdit)?;
    Ok(m)
}

fn select(m: &mut Model, at: Vec3, add: bool) -> anyhow::Result<()> {
    hover(m, at)?;
    m.update(Message::Pointer(Pointer::Click { add }))
}

#[test]
fn the_selection_becomes_the_references_and_picks_the_mode() -> anyhow::Result<()> {
    let mut m = with_box()?;
    // The top, then (Ctrl) the bottom: two planes.
    select(&mut m, Vec3::new(0.2, 0.15, 0.5), false)?;
    m.update(Message::Pointer(Pointer::Cycle))?; // From the top to the bottom.
    m.update(Message::Pointer(Pointer::Click { add: true }))?;
    assert_eq!(m.selection.len(), 2);
    click(&mut m, "Plane")?;
    assert!(m.selection.is_empty(), "the selection went into the step");
    let Some(op) =
        m.workspace.parts["bolt"].program[1].downcast_ref::<workpiece_ops_datum::AddPlane>()
    else {
        anyhow::bail!("expected the plane");
    };
    assert_eq!(op.args.mode, workpiece_ops_datum::PlaneMode::Midplane);
    assert_eq!(op.args.references.len(), 2);
    // The modes two planes do not fit are greyed out.
    let dialog = View::new(&m)
        .edit_dialog
        .ok_or_else(|| anyhow::anyhow!("no dialog"))?;
    let Widget::VBox(fields) = dialog.fields() else {
        anyhow::bail!("expected fields");
    };
    let Some(Widget::Choice(modes)) = fields.children.get(1) else {
        anyhow::bail!("expected the modes");
    };
    let enabled: Vec<_> = modes
        .options
        .iter()
        .zip(&modes.enabled)
        .filter(|(_, e)| **e)
        .map(|(o, _)| o.as_str())
        .collect();
    assert_eq!(enabled, ["Midplane"]);
    m.update(Message::FinishEdit)?;
    let Some(workpiece_core::world::Datum::Plane(mid)) =
        m.viewport.shown.world.datums.get("plane1")
    else {
        anyhow::bail!("no midplane: {:?}", m.viewport.shown.error);
    };
    assert!(
        mid.origin().z.abs() < 1e-9,
        "halfway between top and bottom"
    );
    Ok(())
}

#[test]
fn a_hole_goes_where_its_face_is_clicked_and_is_then_placed() -> anyhow::Result<()> {
    use workpiece_ops_features::{Hole, HolePlace};
    let mut m = with_box()?;
    let hole = |m: &Model| -> Hole {
        m.workspace.parts["bolt"].program[1]
            .downcast_ref::<Hole>()
            .expect("the hole")
            .clone()
    };
    // The dialog opens picking the face, the first field of "Where".
    click(&mut m, "Hole")?;
    assert_eq!(m.picking, Some(vec![0, 0]), "picking the face");
    // The top clicked: the hole is there at once, where it was clicked.
    let at = Vec3::new(0.2, 0.15, 0.5);
    select(&mut m, at, false)?;
    let args = hole(&m).args;
    assert_eq!(args.face, Some(Source::Topology("box1/top".into()).into()));
    assert_eq!(args.place, HolePlace::Point);
    let on: workpiece_core::world::DatumPlane =
        m.viewport
            .shown
            .world
            .resolve(&workpiece_core::Reference::new(Source::Topology(
                "box1/top".into(),
            )))?;
    let clicked = on.origin() + on.x_dir() * args.at[0].value + on.y_dir() * args.at[1].value;
    assert!(
        (clicked - Point3::new(0.2, 0.15, 0.5)).length() < 0.02,
        "{clicked:?}"
    );
    m.update(Message::Tick(1.0))?;
    let shown = &m.viewport.shown;
    assert!(shown.error.is_none(), "{:?}", shown.error);
    assert!(
        shown.world.annotations.contains_key("hole1/callout"),
        "the hole previews"
    );
    // Then how it is placed, by the placement's pictures: off two
    // edges, whose fields come in its place.
    let dialog = View::new(&m)
        .edit_dialog
        .ok_or_else(|| anyhow::anyhow!("no dialog"))?;
    let Widget::VBox(fields) = dialog.fields() else {
        anyhow::bail!("expected fields");
    };
    let Some(Widget::Group(where_)) = fields.children.first() else {
        anyhow::bail!("expected where");
    };
    assert_eq!(where_.label, "Where");
    let Some(Widget::Choice(place)) = where_.children.get(1) else {
        anyhow::bail!("expected the placement");
    };
    assert!(place.pictured(), "radio buttons with icons");
    assert_eq!(place.options, ["Point", "Two edges", "Axis", "Sketch"]);
    if let Some(message) = place.set(1) {
        m.update(message)?;
    }
    assert_eq!(hole(&m).args.place, HolePlace::Offsets);
    let dialog = View::new(&m)
        .edit_dialog
        .ok_or_else(|| anyhow::anyhow!("no dialog"))?;
    let Widget::VBox(fields) = dialog.fields() else {
        anyhow::bail!("expected fields");
    };
    let Some(Widget::Group(where_)) = fields.children.first() else {
        anyhow::bail!("expected where");
    };
    let labels: Vec<String> = where_
        .children
        .iter()
        .filter_map(|w| match w {
            Widget::ReferenceInput(r) => Some(r.label.clone()),
            Widget::HBox(row) => match row.children.first() {
                Some(Widget::ReferenceInput(r)) => Some(r.label.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(labels, ["On", "From", "And", "In"]);
    // Until they are picked, the hole stays where the face was clicked;
    // picked, their distances are where it is: it stays there.
    let centre = |m: &mut Model| -> anyhow::Result<Point3> {
        m.update(Message::Tick(1.0))?;
        let shown = &m.viewport.shown;
        anyhow::ensure!(shown.error.is_none(), "{:?}", shown.error);
        match shown
            .world
            .annotations
            .get("hole1/callout")
            .map(|a| &a.note)
        {
            Some(workpiece_core::world::Note::Hole { centre, .. }) => Ok(*centre),
            _ => anyhow::bail!("no hole"),
        }
    };
    assert!((centre(&mut m)? - clicked).length() < 1e-9);
    for (k, side) in [(0, "box1/right"), (1, "box1/back")] {
        let dialog = View::new(&m)
            .edit_dialog
            .ok_or_else(|| anyhow::anyhow!("no dialog"))?;
        let Widget::VBox(fields) = dialog.fields() else {
            anyhow::bail!("expected fields");
        };
        let Some(Widget::Group(where_)) = fields.children.first() else {
            anyhow::bail!("expected where");
        };
        let Some(Widget::HBox(row)) = where_.children.get(2 + k) else {
            anyhow::bail!("expected a plane and its distance");
        };
        let Some(Widget::ReferenceInput(input)) = row.children.first() else {
            anyhow::bail!("expected the plane");
        };
        m.update(input.set(Source::Topology(side.into())))?;
    }
    let args = hole(&m).args;
    assert!(args.first.is_some() && args.second.is_some());
    assert!((centre(&mut m)? - clicked).length() < 1e-3);
    // The knob at the hole moves it over the face, as a gizmo's centre
    // does: both distances follow.
    let was = centre(&mut m)?;
    hover(&mut m, was.as_vec3())?;
    let knob = Candidate::Handle {
        name: "place".into(),
        part: Some(workpiece_core::handles::GizmoPart::Free),
    };
    assert_eq!(m.viewport.hovered(), Some(&knob));
    let to = Point3::new(-0.1, -0.2, 0.5);
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(&mut m, Vec3::new(0.05, 0.0, 0.5))?;
    hover(&mut m, to.as_vec3())?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    assert!(
        (centre(&mut m)? - to).length() < 1e-2,
        "{:?}",
        centre(&mut m)?
    );
    let moved = hole(&m).args;
    assert!(moved.first_offset != args.first_offset && moved.second_offset != args.second_offset);
    Ok(())
}

#[test]
fn ctrl_clicks_toggle_the_selection_set() -> anyhow::Result<()> {
    let mut m = with_box()?;
    let top = Vec3::new(0.2, 0.15, 0.5);
    select(&mut m, top, true)?;
    select(&mut m, top, true)?;
    assert!(m.selection.is_empty(), "added, then removed");
    select(&mut m, top, false)?;
    assert_eq!(m.selection.len(), 1);
    // A plain click on nothing clears it.
    select(&mut m, Vec3::new(30.0, 30.0, 30.0), false)?;
    assert!(m.selection.is_empty());
    Ok(())
}

#[test]
fn the_context_menu_offers_what_fits() -> anyhow::Result<()> {
    let mut m = with_box()?;
    select(&mut m, Vec3::new(0.2, 0.15, 0.5), false)?;
    m.update(Message::ContextMenu(Some([10.0, 10.0])))?;
    let labels = |m: &Model| -> Vec<String> {
        let menu = View::new(m).context_menu;
        menu.into_iter()
            .flat_map(|c| c.groups)
            .flatten()
            .map(|i| i.button.label)
            .collect()
    };
    // One face: what is made of one — a sketch on it, a plane off it,
    // a frame on it, and what works on faces; an axis is not.
    let offered = labels(&m);
    for wanted in ["Sketch", "Plane", "Frame", "Shell", "Clear selection"] {
        assert!(
            offered.iter().any(|l| l == wanted),
            "{wanted} of {offered:?}"
        );
    }
    assert!(!offered.iter().any(|l| l == "Axis"), "{offered:?}");
    // Nothing selected: what one starts with, several of them.
    m.update(Message::ClearSelection)?;
    assert_eq!(
        labels(&m),
        ["Sketch", "Plane", "Axis", "Frame", "Curve through points"]
    );
    // While editing: closing the dialog.
    click(&mut m, "Frame")?;
    m.update(Message::ContextMenu(Some([10.0, 10.0])))?;
    assert_eq!(labels(&m), ["Done", "Cancel"]);
    Ok(())
}

#[test]
fn handles_drag_while_picking() -> anyhow::Result<()> {
    let mut m = with_box()?;
    select(&mut m, Vec3::new(0.2, 0.15, 0.5), false)?;
    click(&mut m, "Plane")?;
    m.update(Message::Camera(CameraMove::LookFrom([1.0, -1.0, 0.3])))?;
    m.update(Message::Tick(1.0))?;
    // The offset fits, so nothing more is needed; pick on anyway.
    m.update(Message::Pick(Some(vec![0])))?;
    assert!(m.picking.is_some());
    // The distance arrow's knob, one up from the top's plane (anchored
    // where the world's origin falls on it): a handle, first.
    hover(&mut m, Vec3::new(0.0, 0.0, 1.5))?;
    let distance = Candidate::Handle {
        name: "distance".into(),
        part: None,
    };
    assert_eq!(m.viewport.hovered(), Some(&distance));
    // Clicking it picks nothing; dragging it drags.
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    m.update(Message::Pointer(Pointer::Press))?;
    hover(&mut m, Vec3::new(0.0, 0.0, 2.0))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(&mut m, Vec3::new(-0.5, -0.5, 2.5))?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let Some(op) =
        m.workspace.parts["bolt"].program[1].downcast_ref::<workpiece_ops_datum::AddPlane>()
    else {
        anyhow::bail!("expected the plane");
    };
    assert_eq!(op.args.references.len(), 1, "nothing picked");
    assert!(
        (op.args.distance - 2.0).abs() < 1e-3,
        "{}",
        op.args.distance
    );
    Ok(())
}

#[test]
fn new_steps_say_nothing_wrong_until_changed() -> anyhow::Result<()> {
    let mut m = with_box()?;
    click(&mut m, "Plane")?;
    assert!(m.viewport.shown.error.is_some(), "it lacks its plane");
    let dialog = |m: &Model| View::new(m).edit_dialog.and_then(|d| d.error);
    assert_eq!(dialog(&m), None, "not yet said");
    assert_eq!(View::new(&m).viewport.error, None);
    // Picking the top makes it whole; removing it again (a change) leaves
    // it lacking, and now that is said.
    hover(&mut m, Vec3::new(0.2, 0.15, 0.5))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert!(m.viewport.shown.error.is_none());
    let view = View::new(&m);
    let Some(Widget::VBox(fields)) = view.edit_dialog.as_ref().map(|d| d.fields()) else {
        anyhow::bail!("expected the fields");
    };
    let Some(Widget::ReferenceList(references)) = fields.children.first() else {
        anyhow::bail!("expected the references");
    };
    m.update(references.remove(0))?;
    assert!(dialog(&m).is_some(), "now said");
    // Reopened steps say what is wrong at once.
    m.update(Message::FinishEdit)?;
    m.update(Message::Edit(vec!["plane1".into()]))?;
    assert!(dialog(&m).is_some());
    Ok(())
}

/// A model with a sketch on the origin's XY plane open, the view facing
/// it.
fn sketching() -> anyhow::Result<Model> {
    sketching_with(model())
}

fn sketching_with(mut m: Model) -> anyhow::Result<Model> {
    m.update(Message::Open("bolt".into()))?;
    hover(&mut m, Vec3::new(0.3, 0.3, 0.0))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(
        m.selected_sources(),
        [Source::FrameAxis {
            frame: "origin".into(),
            axis: Axis::Z
        }]
    );
    click(&mut m, "Sketch")?;
    Ok(m)
}

fn press_key(m: &mut Model, key: workpiece_core::Hotkey) -> anyhow::Result<()> {
    let view = View::new(m);
    let keys = view.hotkeys();
    let Some((_, press)) = keys.iter().find(|(k, _)| *k == key) else {
        anyhow::bail!("nothing on {key}");
    };
    let message = view
        .press(press)
        .ok_or_else(|| anyhow::anyhow!("{key} does nothing"))?;
    m.update(scaled(message))?;
    m.update(Message::EndEdit)
}

fn click_at(m: &mut Model, x: f32, y: f32) -> anyhow::Result<()> {
    hover(m, Vec3::new(x, y, 0.0))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))
}

fn sketch(m: &Model) -> &workpiece_ops_sketch::SketchArgs {
    match m.workspace.parts["bolt"].program[0].downcast_ref::<workpiece_ops_sketch::Sketch>() {
        Some(sketch) => &sketch.args,
        None => panic!("expected a sketch, found another step"),
    }
}

#[test]
fn sketches_open_in_the_ribbon_and_the_view_flies_to_face_them() -> anyhow::Result<()> {
    let mut m = sketching()?;
    assert_eq!(m.editing, ["sketch1"]);
    assert_eq!(m.picking, None, "the selected plane was taken");
    // The view is on its way to facing the plane, and gets there in time.
    let view = View::new(&m);
    assert!(view.viewport.animating);
    let dialog = view.edit_dialog.as_ref().expect("the sketch's ribbon");
    assert_eq!(dialog.placement, workpiece_core::Placement::Ribbon);
    let before = m.viewport.camera;
    m.update(Message::Tick(0.1))?;
    assert_ne!(m.viewport.camera, before);
    assert!(m.viewport.transition.is_some());
    m.update(Message::Tick(1.0))?;
    assert!(m.viewport.transition.is_none());
    // By the smallest turn: seen from above as it was, and what was
    // nearest to upright (the far side of the view, −x) is up; nothing
    // is turned on its head.
    assert!(m.viewport.camera.back().abs_diff_eq(Vec3::Z, 1e-5));
    assert!(m.viewport.camera.up().abs_diff_eq(-Vec3::X, 1e-5));
    // Looked at from below, it is faced from below.
    m.update(Message::Camera(CameraMove::LookFrom([0.3, 1.0, -2.0])))?;
    m.update(Message::Tick(1.0))?;
    m.update(Message::Camera(CameraMove::FacePlane))?;
    m.update(Message::Tick(1.0))?;
    assert!(m.viewport.camera.back().abs_diff_eq(-Vec3::Z, 1e-5));
    assert!(m.viewport.camera.up().abs_diff_eq(Vec3::Y, 1e-5));
    // Turning the view by hand ends a flight where it is.
    m.update(Message::Camera(CameraMove::Home))?;
    m.update(Message::Tick(0.1))?;
    m.update(Message::Camera(CameraMove::Zoom(10.0)))?;
    assert!(m.viewport.transition.is_none());
    Ok(())
}

#[test]
fn a_sketch_on_a_side_face_is_faced_upright() -> anyhow::Result<()> {
    let mut m = with_box()?;
    m.update(Message::Camera(CameraMove::LookFrom([-1.0, 0.2, 0.3])))?;
    m.update(Message::Tick(1.0))?;
    select(&mut m, Vec3::new(-0.5, 0.1, 0.1), false)?;
    assert!(matches!(m.selected_sources()[..], [Source::Topology(_)]));
    click(&mut m, "Sketch")?;
    m.update(Message::Tick(1.0))?;
    // The face looks along −x, its v is up: the view shows the model
    // upright, whatever way round the view was before.
    assert!(m.viewport.camera.back().abs_diff_eq(-Vec3::X, 1e-5));
    assert!(m.viewport.camera.up().abs_diff_eq(Vec3::Z, 1e-5));
    m.update(Message::Camera(CameraMove::LookFrom([-1.0, -0.2, 0.3])))?;
    m.update(Message::Tick(1.0))?;
    m.update(Message::Camera(CameraMove::FacePlane))?;
    m.update(Message::Tick(1.0))?;
    assert!(m.viewport.camera.up().abs_diff_eq(Vec3::Z, 1e-5));
    Ok(())
}

#[test]
fn the_canvas_takes_the_pointer_and_hotkeys_choose_tools() -> anyhow::Result<()> {
    let mut m = sketching()?;
    m.update(Message::Tick(1.0))?;
    // R is the rectangle here (the ribbon's own tools are away).
    press_key(&mut m, workpiece_core::Hotkey::key('R'))?;
    click_at(&mut m, 0.5, 0.5)?;
    // Only the origin and the two axes.
    assert_eq!(
        sketch(&m).entities.len(),
        3,
        "one click is no rectangle yet"
    );
    click_at(&mut m, 1.5, 1.0)?;
    let lines = |m: &Model| {
        sketch(m)
            .entities
            .keys()
            .filter(|&&e| sketch(m).is_line(e) && !sketch(m).is_axis(e))
            .count()
    };
    assert_eq!(lines(&m), 4);
    // The canvas shows it, with a badge for each level line; the world's
    // own drawing of the sketch is hidden while it is edited.
    let view = View::new(&m);
    assert_eq!(
        view.viewport
            .handles
            .labels
            .iter()
            .filter(|l| l.badge)
            .count(),
        4
    );
    assert!(m.viewport.shown.visibility.profiles.contains("sketch1"));
    assert_eq!(m.viewport.shown.world.profiles["sketch1"].edges.len(), 4);
    // What the build worked out is kept with the part, as the start of
    // its next build; undo takes the rectangle back in one step.
    assert!(
        m.workspace.parts["bolt"]
            .cache
            .get("sketch1/args")
            .is_some()
    );
    m.update(Message::Undo)?;
    assert_eq!(lines(&m), 0);
    m.update(Message::Redo)?;
    assert_eq!(lines(&m), 4);

    // Dragging a corner moves it, as one undo step.
    press_key(
        &mut m,
        workpiece_core::Hotkey::named(workpiece_core::Key::Escape),
    )?;
    hover(&mut m, Vec3::new(1.5, 1.0, 0.0))?;
    m.update(Message::Pointer(Pointer::Press))?;
    hover(&mut m, Vec3::new(1.6, 1.1, 0.0))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(&mut m, Vec3::new(2.0, 1.5, 0.0))?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let moved = |m: &Model, x: f64, y: f64| {
        sketch(m)
            .points
            .values()
            .any(|p| p.at.abs_diff_eq(glam::DVec2::new(x, y), 1e-3))
    };
    assert!(moved(&m, 2.0, 1.5) && moved(&m, 0.5, 0.5) && moved(&m, 2.0, 0.5));
    m.update(Message::Undo)?;
    assert!(moved(&m, 1.5, 1.0));

    // Two lines selected: the menu offers what fits them.
    click_at(&mut m, 1.0, 0.5)?;
    hover(&mut m, Vec3::new(1.5, 0.75, 0.0))?;
    m.update(Message::Pointer(Pointer::Click { add: true }))?;
    m.update(Message::ContextMenu(Some([10.0, 10.0])))?;
    let view = View::new(&m);
    let menu = view.context_menu.as_ref().expect("the menu");
    let offered: Vec<&str> = menu.groups[0]
        .iter()
        .map(|i| i.button.label.as_str())
        .collect();
    assert!(
        offered.contains(&"Perpendicular") && offered.contains(&"Dimension"),
        "{offered:?}"
    );
    assert!(
        menu.groups
            .iter()
            .flatten()
            .all(|i| i.button.hotkey.is_some())
    );
    // Enter finishes the sketch; its profile shows in the world.
    press_key(
        &mut m,
        workpiece_core::Hotkey::named(workpiece_core::Key::Enter),
    )?;
    assert!(m.editing.is_empty());
    assert!(!m.viewport.shown.visibility.profiles.contains("sketch1"));
    assert_eq!(View::new(&m).overview.sketches.len(), 1);
    Ok(())
}

#[test]
fn every_tool_has_a_hotkey_of_its_own() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    let view = View::new(&m);
    // (A menu's button only opens it: its tools have the keys.)
    let tools: Vec<&crate::view::Tool> = view
        .ribbon
        .tools
        .iter()
        .flat_map(|t| {
            if t.menu.is_empty() {
                vec![t]
            } else {
                t.menu.iter().collect()
            }
        })
        .collect();
    assert!(tools.iter().all(|t| t.button.hotkey.is_some()));
    let mut keys: Vec<_> = tools.iter().filter_map(|t| t.button.hotkey).collect();
    keys.extend(
        [view.viewport.home.hotkey, view.viewport.projection.hotkey]
            .into_iter()
            .flatten(),
    );
    let count = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), count, "no key twice");
    // E adds an extrusion, whose dialog's buttons have keys too.
    press_key(&mut m, workpiece_core::Hotkey::key('E'))?;
    assert_eq!(m.editing, ["extrude1"]);
    let view = View::new(&m);
    let dialog = view.edit_dialog.as_ref().expect("the dialog");
    assert_eq!(
        dialog.cancel.hotkey,
        Some(workpiece_core::Hotkey::named(workpiece_core::Key::Escape))
    );
    press_key(
        &mut m,
        workpiece_core::Hotkey::named(workpiece_core::Key::Escape),
    )?;
    assert!(m.editing.is_empty() && program(&m, "bolt").is_empty());
    Ok(())
}

#[test]
fn parameters_drive_steps_and_variants_show_and_place() -> anyhow::Result<()> {
    use workpiece_core::Measure;
    use workpiece_part::{Parameter, Variant};

    let mut m = model();
    m.update(Message::Open("nut".into()))?;
    click(&mut m, "Box")?;
    pick_origin(&mut m)?;
    // A parameter, and a variant that makes it bigger.
    let mut table = View::new(&m).table.expect("the table");
    table.parameters.push(Parameter {
        name: table.fresh_parameter("p"),
        value: Measure::new(2.0),
    });
    table.variants.push(Variant {
        name: "L".into(),
        values: BTreeMap::from([("p1".to_owned(), Measure::new(6.0))]),
    });
    m.update(table.store())?;
    assert_eq!(
        View::new(&m).table.expect("the table").values,
        Ok(BTreeMap::from([("p1".to_owned(), 2.0)]))
    );
    // The box's width becomes a formula, entered in its field.
    let view = View::new(&m);
    let dialog = view.edit_dialog.as_ref().expect("the dialog");
    let mut width = None;
    dialog.fields().visit(&mut |w| {
        if let Widget::NumericInput(input) = w
            && input.label == "Width"
        {
            width = input.enter("= p1 * 2");
        }
    });
    m.update(width.expect("the width takes formulas"))?;
    m.update(Message::FinishEdit)?;
    let reach = |m: &Model| {
        let vertices = m
            .viewport
            .shown
            .world
            .topo
            .vertices()
            .iter()
            .map(|(_, v)| v.point.x);
        vertices.fold(0.0, f64::max)
    };
    assert!((reach(&m) - 2.0).abs() < 1e-12);
    let step = &View::new(&m).timeline.steps[0];
    assert!(
        step.description.starts_with("=p1 * 2 × 1 × 1"),
        "{}",
        step.description
    );
    // Shown as the variant, it is three times as wide; the file is not.
    m.update(Message::ShowVariant(Some("L".into())))?;
    assert!((reach(&m) - 6.0).abs() < 1e-12);
    assert!(m.update(Message::ShowVariant(Some("XL".into()))).is_err());
    m.update(Message::ShowVariant(None))?;

    // Placing the part offers its variants.
    m.update(Message::Open("bolt".into()))?;
    click(&mut m, "Add part")?;
    let view = View::new(&m);
    let Some(Widget::FileInput(file)) = view.edit_dialog.as_ref().and_then(|d| d.fields().at(&[0]))
    else {
        anyhow::bail!("expected the file field");
    };
    m.update(file.set("nut"))?;
    let view = View::new(&m);
    let dialog = view.edit_dialog.as_ref().expect("the dialog");
    let Some(Widget::Choice(variants)) = dialog.fields().at(&[1]) else {
        anyhow::bail!("expected the variants");
    };
    assert_eq!(variants.options, ["Default", "L"]);
    m.update(variants.set(1).expect("L can be chosen"))?;
    let placed = &m.viewport.shown.world.instances["part1"].world;
    let wide = placed
        .topo
        .vertices()
        .iter()
        .map(|(_, v)| v.point.x)
        .fold(0.0, f64::max);
    assert!((wide - 6.0).abs() < 1e-12);
    let step = &View::new(&m).timeline.steps[0];
    assert_eq!(step.description, "places nut [L]");
    // Taking the table back is one undo step in the part it is of.
    m.update(Message::FinishEdit)?;
    m.update(Message::Open("nut".into()))?;
    let mut table = View::new(&m).table.expect("the table");
    table.parameters[0].name = "width".into();
    m.update(table.store())?;
    assert!(
        m.error().is_some_and(|e| e.contains("no parameter \"p1\"")),
        "{:?}",
        m.error()
    );
    m.update(Message::Undo)?;
    assert_eq!(m.error(), None);
    Ok(())
}

#[test]
fn steps_move_rename_and_go_unless_that_breaks_what_uses_them() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    for op in [frame("f", 0.0), frame("g", 1.0)] {
        m.update(Message::AddOperation {
            part: "bolt".into(),
            op,
        })?;
        m.update(Message::FinishEdit)?;
    }
    // A box on the frame `f`.
    let on_f = AddBox {
        id: "box1".into(),
        args: workpiece_ops_basic_shapes::AddBoxArgs {
            frame: Some(workpiece_core::Reference::datum("f")),
            ..Default::default()
        },
    };
    m.update(Message::AddOperation {
        part: "bolt".into(),
        op: on_f.into(),
    })?;
    m.update(Message::FinishEdit)?;
    assert_eq!(program(&m, "bolt"), ["f", "g", "box1"]);
    let path = |id: &str| vec![id.to_owned()];

    // Steps move within the program, but not before what they use.
    m.update(Message::MoveOperation {
        path: path("g"),
        to: 2,
    })?;
    assert_eq!(program(&m, "bolt"), ["f", "box1", "g"]);
    let refused = m
        .update(Message::MoveOperation {
            path: path("box1"),
            to: 0,
        })
        .expect_err("before its frame");
    assert!(
        format!("{refused:#}").contains("box1 uses f"),
        "{refused:#}"
    );
    assert_eq!(program(&m, "bolt"), ["f", "box1", "g"]);
    m.update(Message::Undo)?;
    assert_eq!(program(&m, "bolt"), ["f", "g", "box1"]);

    // A rename takes the steps that use it along.
    m.update(Message::RenameOperation {
        path: path("f"),
        name: "base".into(),
    })?;
    assert_eq!(program(&m, "bolt"), ["base", "g", "box1"]);
    let part = m.workspace.part("bolt")?;
    assert_eq!(
        part.operation("box1").map(AnyOperation::references),
        Some(vec![Source::Datum("base".into())])
    );
    assert!(m.error().is_none(), "{:?}", m.error());
    for name in ["g", "", "a/b"] {
        let taken = m.update(Message::RenameOperation {
            path: path("base"),
            name: name.into(),
        });
        assert!(taken.is_err(), "{name:?}");
    }

    // The selected step goes with Delete, unless something uses it.
    m.update(Message::SelectStep(Some(path("base"))))?;
    let view = View::new(&m);
    assert!(view.timeline.steps[0].selected);
    let delete = workpiece_core::Hotkey::named(workpiece_core::Key::Delete);
    let refused = press_key(&mut m, delete).expect_err("in use");
    assert!(
        format!("{refused:#}").contains("base is used by box1"),
        "{refused:#}"
    );
    m.update(Message::SelectStep(Some(path("g"))))?;
    press_key(&mut m, delete)?;
    assert_eq!(program(&m, "bolt"), ["base", "box1"]);
    assert_eq!(m.step, None);
    m.update(Message::RemoveOperation(path("box1")))?;
    m.update(Message::RemoveOperation(path("base")))?;
    assert!(program(&m, "bolt").is_empty());
    Ok(())
}

/// A sketch with a circle of radius 1 at `x`, dimensioned.
fn dimensioned(id: &str, x: f64) -> AnyOperation {
    use workpiece_ops_sketch::{Kind, Shape};
    let mut sketch = workpiece_ops_sketch::Sketch::new(id);
    sketch.args.plane = Some(workpiece_core::Reference::frame_axis("origin", Axis::Z));
    let center = sketch
        .args
        .add_point(workpiece_core::math::Point2::new(x, 0.0));
    let circle = sketch.args.add(Shape::Circle {
        center,
        radius: 1.0,
    });
    sketch.args.dimension(
        Kind::Size(circle),
        1.0,
        workpiece_core::math::Point2::new(x + 1.5, 1.0),
    );
    sketch.into()
}

#[test]
fn sketch_dimensions_lie_in_their_plane_and_are_drawn_once_while_edited() -> anyhow::Result<()> {
    let mut m = small(Model::new(Workspace::new(BTreeMap::from([(
        "bolt".into(),
        Part::new(vec![dimensioned("a", 0.0), dimensioned("b", 4.0)]),
    )]))));
    m.update(Message::Open("bolt".into()))?;
    m.update(Message::SetTab(crate::Tab::Annotate))?;
    let names = |m: &Model| -> Vec<String> {
        let notes = View::new(m).viewport.notes;
        notes.into_iter().map(|n| n.name).collect()
    };
    // Both sketches' dimensions, as the part is looked at.
    let all = names(&m);
    assert!(all.iter().any(|n| n.starts_with("a/")), "{all:?}");
    assert!(all.iter().any(|n| n.starts_with("b/")), "{all:?}");
    // Drawn in the plane (z = 0) whichever way the view is turned.
    for turn in [[0.0, 0.0], [140.0, 70.0], [-60.0, -30.0]] {
        m.update(Message::Camera(CameraMove::Orbit(turn)))?;
        m.update(Message::Tick(10.0))?;
        for note in View::new(&m).viewport.notes {
            assert!(!note.strokes.is_empty());
            let points = note
                .strokes
                .iter()
                .flat_map(|(a, b)| [*a, *b])
                .chain(note.fills.iter().flatten().copied())
                .chain(note.texts.iter().map(|t| t.at));
            for p in points {
                assert!(p.z.abs() < 1e-4, "{} leaves the plane: {p:?}", note.name);
            }
        }
    }
    // While one is edited the sketcher draws its dimensions (once): no
    // note is drawn over them, of it or of anything else.
    m.update(Message::Edit(vec!["a".into()]))?;
    assert!(names(&m).is_empty(), "{:?}", names(&m));
    m.update(Message::FinishEdit)?;
    m.update(Message::SetTab(crate::Tab::Annotate))?;
    assert!(!names(&m).is_empty());
    Ok(())
}

#[test]
fn the_normal_view_faces_the_sketch_again() -> anyhow::Result<()> {
    let mut m = sketching()?;
    m.update(Message::Tick(10.0))?;
    let facing = m.viewport.camera;
    m.update(Message::Camera(CameraMove::Orbit([120.0, 60.0])))?;
    assert_ne!(m.viewport.camera, facing);
    // Only while a step with a plane is edited is there such a view.
    assert!(View::new(&m).viewport.normal.is_some());
    press_key(&mut m, workpiece_core::Hotkey::key('Q'))?;
    m.update(Message::Tick(10.0))?;
    let back = m.viewport.camera.back();
    assert!(back.abs_diff_eq(facing.back(), 1e-4), "{back:?}");
    m.update(Message::FinishEdit)?;
    assert!(View::new(&m).viewport.normal.is_none());
    Ok(())
}

#[test]
fn a_sketch_on_a_plane_cuts_the_model_open_there() -> anyhow::Result<()> {
    // Drawing a sketch on a plane cuts the model at that plane (away
    // from the view), and leaving it puts the display back.
    let mut m = sketching()?;
    m.update(Message::Tick(10.0))?;
    let plane = m.display.plane.clone().expect("cut at the sketch plane");
    assert!(m.display.cut.is_none());
    // The side the view looks from is what is cut away.
    let back = m.viewport.camera.back().as_dvec3();
    assert!(
        plane
            .y
            .dot(m.viewport.shown.pose.inverse().transform_vector(back))
            > 0.5
    );
    // The user can put it away meanwhile; it stays so while sketching.
    m.update(Message::SetDisplay(Display::default()))?;
    m.update(Message::Tick(1.0))?;
    assert!(m.display.plane.is_none());
    m.update(Message::FinishEdit)?;
    assert_eq!(m.display, Display::default());
    // What was chosen before comes back.
    let mut m = sketching()?;
    m.update(Message::FinishEdit)?;
    let id = m
        .workspace
        .part("bolt")?
        .program
        .last()
        .unwrap()
        .id()
        .to_owned();
    m.update(Message::SetDisplay(Display {
        explode: 0.5,
        ..Display::default()
    }))?;
    m.update(Message::Edit(vec![id]))?;
    m.update(Message::Tick(10.0))?;
    assert!(m.display.plane.is_some() && m.display.explode == 0.5);
    m.update(Message::FinishEdit)?;
    assert!(m.display.plane.is_none() && m.display.explode == 0.5);
    // With the setting off nothing changes.
    let mut start = model();
    start.settings.sketch_section = false;
    let mut m = sketching_with(start)?;
    m.update(Message::Tick(10.0))?;
    assert_eq!(m.display, Display::default());
    Ok(())
}

#[test]
fn tabs_hold_the_tools_and_sketches_extrude_into_bodies() -> anyhow::Result<()> {
    let mut m = sketching()?;
    m.update(Message::Tick(1.0))?;
    // A rectangle, drawn in the sketch.
    press_key(&mut m, workpiece_core::Hotkey::key('R'))?;
    click_at(&mut m, 0.5, 0.5)?;
    click_at(&mut m, 1.5, 1.0)?;
    m.update(Message::FinishEdit)?;
    // Modeling has extrude, revolve and combine; surfacing no combine;
    // assembly adding parts (STEP files among them, each with its joint),
    // making new ones in place, and mechanisms.
    let labels = |m: &Model| -> Vec<String> {
        let view = View::new(m);
        let tools = view.ribbon.tools.iter();
        tools.map(|t| t.button.label.clone()).collect()
    };
    let has = |m: &Model, label: &str| labels(m).iter().any(|l| l == label);
    assert!(has(&m, "Extrude") && has(&m, "Revolve") && has(&m, "Combine"));
    assert!(!has(&m, "Add part"));
    press_key(&mut m, workpiece_core::Hotkey::ctrl('2'))?;
    assert_eq!(m.tab, crate::Tab::Surfacing);
    assert!(has(&m, "Extrude") && !has(&m, "Combine"));
    press_key(&mut m, workpiece_core::Hotkey::ctrl('3'))?;
    assert_eq!(
        labels(&m),
        [
            "Undo",
            "Redo",
            "Add part",
            "New part",
            "Mechanism",
            "Standard part",
            "Pattern part",
            "Mirror part",
            "Drive",
            "Cut parts",
            "Weld"
        ]
    );
    assert!(!has(&m, "Import STEP"));
    press_key(&mut m, workpiece_core::Hotkey::ctrl('1'))?;

    // Extrude takes the sketch just drawn and still picks: a click on
    // it (off the arrow in its middle) takes it for good, and it makes a
    // body of it.
    press_key(&mut m, workpiece_core::Hotkey::key('E'))?;
    assert_eq!(m.editing, ["extrude1"]);
    assert_eq!(m.picking, Some(vec![0]));
    click_at(&mut m, 0.7, 0.6)?;
    assert_eq!(m.picking, None);
    assert!(m.error().is_none(), "{:?}", m.error());
    m.update(Message::FinishEdit)?;
    let world = &m.viewport.shown.world;
    let body = world.topo.resolve("extrude1").expect("the body");
    assert_eq!(world.topo.body_faces(body).map(|f| f.len()), Some(6));
    // As a surface, it is a sheet: drawn in the surfaces' color.
    press_key(&mut m, workpiece_core::Hotkey::ctrl('2'))?;
    press_key(&mut m, workpiece_core::Hotkey::key('E'))?;
    assert_eq!(m.editing, ["surface1"]);
    // The first sketch is used by now, and shows again to be picked.
    click_at(&mut m, 0.5, 0.75)?;
    assert_eq!(m.picking, None);
    m.update(Message::FinishEdit)?;
    let world = &m.viewport.shown.world;
    let sheet = world.topo.resolve("surface1").expect("the sheet");
    let kind = world.topo.body(sheet).map(|b| b.kind);
    assert_eq!(kind, Some(workpiece_core::world::topo::BodyKind::Sheet));
    let scene = &m.viewport.shown.scene;
    let surface = workpiece_rasterize::palette::SURFACE;
    let triangles = scene
        .items
        .iter()
        .flat_map(|i| &i.scene.triangles.triangles);
    assert!(triangles.into_iter().any(|t| t.color == surface));
    Ok(())
}

#[test]
fn sweeps_pick_sketches_and_bodies_and_drag_their_distance() -> anyhow::Result<()> {
    let mut m = sketching()?;
    m.update(Message::Tick(1.0))?;
    press_key(&mut m, workpiece_core::Hotkey::key('R'))?;
    click_at(&mut m, 0.5, 0.5)?;
    click_at(&mut m, 1.5, 1.0)?;
    m.update(Message::FinishEdit)?;
    m.update(Message::Camera(CameraMove::Home))?;
    m.update(Message::Tick(1.0))?;
    press_key(&mut m, workpiece_core::Hotkey::key('E'))?;
    let extrude = |m: &Model| match m.workspace.parts["bolt"]
        .program
        .last()
        .and_then(|op| op.downcast_ref::<workpiece_ops_sweep::Extrude>())
    {
        Some(extrude) => extrude.args.clone(),
        None => panic!("expected an extrusion, found another step"),
    };
    // The sketch just drawn, which nothing uses yet, is taken; the field
    // still picks sketches (by a curve or inside an area), so a click
    // takes another.
    assert_eq!(
        extrude(&m).sketch,
        Some(Source::Profile("sketch1".into()).into())
    );
    assert_eq!(m.picking, Some(vec![0]));
    assert!(m.error().is_none(), "{:?}", m.error());
    hover(&mut m, Vec3::new(1.0, 0.75, 0.0))?;
    let sketch = Candidate::Reference {
        instance: Vec::new(),
        source: Source::Profile("sketch1".into()),
    };
    assert_eq!(m.viewport.hovered(), Some(&sketch));
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(m.picking, None);
    assert_eq!(
        extrude(&m).sketch,
        Some(Source::Profile("sketch1".into()).into())
    );

    // The arrow at the middle of the area drags the distance, though the
    // sketch's plane takes the pointer everywhere else.
    hover(&mut m, Vec3::new(1.0, 0.75, 1.0))?;
    let arrow = Candidate::Handle {
        name: "distance".into(),
        part: None,
    };
    assert_eq!(m.viewport.hovered(), Some(&arrow));
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(&mut m, Vec3::new(1.0, 0.75, 2.0))?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let distance = extrude(&m).distance.value;
    assert!((distance - 2.0).abs() < 1e-3, "{distance}");
    // Dragged through the sketch, it turns around.
    hover(&mut m, Vec3::new(1.0, 0.75, 2.0))?;
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(&mut m, Vec3::new(1.0, 0.75, -0.5))?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let args = extrude(&m);
    assert!(args.flip && (args.distance.value - 0.5).abs() < 1e-3);
    // The button beside the distance turns it back.
    press_key(&mut m, workpiece_core::Hotkey::key('X'))?;
    assert!(!extrude(&m).flip);
    // Off the arrow, a click still picks the area under it.
    click_at(&mut m, 0.7, 0.6)?;
    assert_eq!(extrude(&m).areas.len(), 1);
    assert!(m.error().is_none(), "{:?}", m.error());
    m.update(Message::FinishEdit)?;

    // Combining asks for a body, picked by any of its faces.
    press_key(&mut m, workpiece_core::Hotkey::key('J'))?;
    assert_eq!(m.picking, Some(vec![0]));
    hover(&mut m, Vec3::new(1.0, 0.75, 0.5))?;
    let body = Candidate::Reference {
        instance: Vec::new(),
        source: Source::Topology("extrude1".into()),
    };
    assert_eq!(m.viewport.hovered(), Some(&body));
    // It shows outlined, with the handles.
    assert!(!m.handle_scene().lines.lines.is_empty());
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    let Some(combine) = m.workspace.parts["bolt"]
        .program
        .last()
        .and_then(|op| op.downcast_ref::<workpiece_ops_bodies::CombineBodies>())
    else {
        anyhow::bail!("expected the combination");
    };
    assert_eq!(
        combine.args.target,
        Some(Source::Topology("extrude1".into()).into())
    );
    // Next, the body to combine it with.
    assert_eq!(m.picking, Some(vec![2]));
    Ok(())
}

#[test]
fn a_revolve_takes_the_last_sketch_and_its_rotation_axis() -> anyhow::Result<()> {
    let revolve = |m: &Model| match m.workspace.parts["bolt"]
        .program
        .last()
        .and_then(|op| op.downcast_ref::<workpiece_ops_sweep::Revolve>())
    {
        Some(revolve) => revolve.args.clone(),
        None => panic!("expected a revolution, found another step"),
    };
    // A rectangle beside a rotation axis, both drawn in the sketch.
    let mut m = sketching()?;
    m.update(Message::Tick(1.0))?;
    press_key(&mut m, workpiece_core::Hotkey::key('R'))?;
    click_at(&mut m, 0.5, 0.5)?;
    click_at(&mut m, 1.5, 1.0)?;
    press_key(&mut m, workpiece_core::Hotkey::shift('Z'))?;
    click_at(&mut m, 2.0, 0.0)?;
    click_at(&mut m, 2.0, 1.0)?;
    m.update(Message::FinishEdit)?;
    // Revolve takes the sketch and its one rotation axis (a datum of the
    // sketch's), and still picks the sketch.
    click(&mut m, "Revolve")?;
    let args = revolve(&m);
    assert_eq!(args.sketch, Some(Source::Profile("sketch1".into()).into()));
    let axes = workpiece_ops_sweep::drawn_axes(&m.viewport.shown.world, "sketch1");
    assert_eq!(axes.len(), 1);
    assert_eq!(args.axis, Some(Source::Datum(axes[0].clone()).into()));
    assert_eq!(m.picking, Some(vec![0]), "still picking the sketch");
    assert!(m.error().is_none(), "{:?}", m.error());
    // Another axis may be picked in its place: the field takes axes,
    // datum axes and straight edges, in the view.
    let view = View::new(&m);
    let dialog = view.edit_dialog.as_ref().expect("the dialog");
    let mut takes = None;
    dialog.fields().visit(&mut |w| {
        if let Widget::ReferenceInput(input) = w
            && input.label == "Rotation axis"
        {
            takes = Some(input.kind);
        }
    });
    assert_eq!(takes, Some(workpiece_core::ReferenceKind::Axis));
    m.update(Message::FinishEdit)?;
    // Used now, the sketch is not taken again: the next revolution asks
    // for one.
    click(&mut m, "Revolve")?;
    assert_eq!(revolve(&m).sketch, None);
    assert_eq!(m.picking, Some(vec![0]));
    Ok(())
}

/// An assembly of two blocks: a base that is fixed, and an arm hinged to
/// it around the z axis at the origin.
fn hinged() -> anyhow::Result<Model> {
    use workpiece_core::world::JointKind;
    use workpiece_ops_basic_shapes::{AddBox, AddBoxArgs};
    use workpiece_ops_instances::connector::Snap;
    use workpiece_ops_instances::{Anchor, Mate};
    let block = |size: [f64; 3]| {
        Part::new(vec![
            AddBox {
                id: "body".into(),
                args: AddBoxArgs {
                    frame: Some(workpiece_core::Reference::datum(
                        workpiece_core::world::ORIGIN,
                    )),
                    size: size.map(workpiece_core::Measure::new),
                },
            }
            .into(),
        ])
    };
    let place = |id: &str, part: &str, fixed: bool| -> AnyOperation {
        AddPart {
            id: id.into(),
            args: AddPartArgs {
                part: part.into(),
                variant: None,
                hold: if fixed {
                    workpiece_ops_instances::Hold::fixed()
                } else {
                    Default::default()
                },
                bodies: Vec::new(),
            },
        }
        .into()
    };
    let origin = |instance: &str| {
        Some(Anchor {
            instance: vec![instance.into()],
            source: Source::Datum(workpiece_core::world::ORIGIN.into()),
            snap: Snap::Datum,
        })
    };
    let mut hinge = Mate::of("hinge", JointKind::Revolute);
    hinge.args.a = origin("base");
    hinge.args.b = origin("arm");
    let assembly = Part::new(vec![
        place("base", "base.wpc", true),
        place("arm", "arm.wpc", false),
        hinge.into(),
    ]);
    let mut m = small(Model::new(Workspace::new(BTreeMap::from([
        ("base.wpc".into(), block([2.0, 2.0, 0.5])),
        ("arm.wpc".into(), block([6.0, 0.6, 0.6])),
        ("hinged.wpc".into(), assembly),
    ]))));
    m.update(Message::Open("hinged.wpc".into()))?;
    m.update(Message::Tick(1.0))?;
    Ok(m)
}

#[test]
fn parts_are_dragged_as_far_as_their_joints_let_them() -> anyhow::Result<()> {
    let mut m = hinged()?;
    let arm = |m: &Model| m.viewport.shown.world.instances["arm"].pose;
    assert!(arm(&m).translation().length() < 1e-9);
    let drawn = m.viewport.shown.scene.clone();
    let arm_scene = |m: &Model| {
        let scene = &m.viewport.shown.scene;
        let item = scene.items.iter().find(|i| i.instance == ["arm"]);
        item.map(|i| i.scene.clone()).expect("the arm is drawn")
    };
    let meshes = arm_scene(&m);

    // Grab the arm near its far end and pull it around.
    let grab = Point3::new(2.7, 0.0, 0.3);
    let to = Point3::new(0.0, 2.7, 0.3);
    hover(&mut m, grab.as_vec3())?;
    let Some(Candidate::Reference { instance, .. }) = m.viewport.hovered() else {
        anyhow::bail!("nothing of the arm under the pointer");
    };
    assert_eq!(instance, &["arm".to_owned()]);
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    assert!(m.viewport.moving.is_some());
    for step in 1..=20 {
        let at = grab.lerp(to, f64::from(step) / 20.0);
        hover(&mut m, at.as_vec3())?;
    }
    m.update(Message::Pointer(Pointer::DragEnd))?;
    assert!(m.viewport.moving.is_none());

    // The hinge held: the arm turned, about where it was pulled to.
    let pose = arm(&m);
    assert!(pose.translation().length() < 1e-6, "{pose:?}");
    let along = pose.axis(Axis::X);
    assert!(along.y > 0.5, "the arm did not turn: {along}");
    assert!(pose.axis(Axis::Z).abs_diff_eq(Point3::Z, 1e-6));
    // The base, fixed, stayed.
    assert_eq!(
        m.viewport.shown.world.instances["base"].pose,
        Pose::IDENTITY
    );
    // Where the arm is belongs to the assembly's part, not to its steps.
    let part = m.workspace.part("hinged.wpc")?;
    assert!(part.cache.get("arm/pose").is_some());
    // Nothing was rasterized again: the same scenes, at other poses.
    assert!(!std::sync::Arc::ptr_eq(&drawn, &m.viewport.shown.scene));
    assert!(std::sync::Arc::ptr_eq(&meshes, &arm_scene(&m)));
    // The whole drag is one step to undo.
    m.update(Message::Undo)?;
    assert!(arm(&m).translation().length() < 1e-9);
    assert!(arm(&m).axis(Axis::X).abs_diff_eq(Point3::X, 1e-9));
    Ok(())
}

#[test]
fn parts_build_beside_the_interface_and_show_when_done() -> anyhow::Result<()> {
    let mut m = hinged()?;
    m.engine = workpiece_part::Engine::threaded(2);
    m.background = true;
    let before = m.viewport.shown.scene.clone();
    // A change asks for a build and returns at once: what was shown
    // stays until the build is done.
    let wait = |m: &mut Model| {
        for _ in 0..2000 {
            if !m.poll() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the build never finished");
    };
    m.update(Message::Scrub(Some(1)))?;
    assert!(m.is_building() || m.viewport.shown.world.instances.len() == 1);
    wait(&mut m);
    assert!(!m.is_building());
    assert_eq!(m.viewport.shown.world.instances.len(), 1);
    assert!(!std::sync::Arc::ptr_eq(&before, &m.viewport.shown.scene));
    // A build that is overtaken is dropped: only the last one shows.
    m.update(Message::Scrub(Some(2)))?;
    m.update(Message::Scrub(None))?;
    wait(&mut m);
    assert_eq!(m.viewport.shown.world.instances.len(), 2);
    assert_eq!(m.viewport.shown.world.joints.len(), 1);
    Ok(())
}

#[test]
fn a_joint_is_made_by_pointing_at_the_two_parts() -> anyhow::Result<()> {
    use workpiece_core::world::JointKind;
    use workpiece_ops_instances::Mate;
    let mut m = hinged()?;
    // Without the hinge, and with the arm lying somewhere else.
    m.update(Message::RemoveOperation(vec!["hinge".into()]))?;
    let away = Pose::from_translation(Point3::new(1.0, 4.0, 2.0));
    m.update(Message::Keep {
        part: "hinged.wpc".into(),
        id: "arm".into(),
        kept: workpiece_ops_instances::assembly::kept(&away),
    })?;
    assert_eq!(m.viewport.shown.world.instances["arm"].pose, away);

    m.update(Message::AddOperation {
        part: "hinged.wpc".into(),
        op: Mate::of("joint1", JointKind::Fastened).into(),
    })?;
    // It asks for its first connector right away.
    let picking = |m: &Model| {
        let view = View::new(m);
        let dialog = view.edit_dialog.as_ref()?;
        let field = dialog.fields().at(m.picking.as_ref()?)?;
        match field {
            Widget::ConnectorInput(input) => Some(input.label.clone()),
            _ => None,
        }
    };
    assert_eq!(picking(&m).as_deref(), Some("First"));
    // The top of the base, away from its edges: the face's middle is
    // offered, drawn as a frame.
    hover(&mut m, Vec3::new(0.7, -0.7, 0.25))?;
    let frame = m.handle_scene();
    assert_eq!(frame.front.lines.len(), 3, "the offered connector's axes");
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(picking(&m).as_deref(), Some("Second"));
    // The top of the arm, where it lies now.
    hover(&mut m, Vec3::new(1.5, 4.1, 2.3))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(m.picking, None);
    m.update(Message::FinishEdit)?;
    assert_eq!(m.error(), None);

    // The arm came to the base: the middle of its top on the middle of
    // the base's, facing the same way.
    let world = &m.viewport.shown.world;
    assert_eq!(world.joints.len(), 1);
    let arm = world.instances["arm"].pose;
    let top = arm.transform_point(Point3::new(0.0, 0.0, 0.3));
    assert!(top.abs_diff_eq(Point3::new(0.0, 0.0, 0.25), 1e-6), "{top}");
    assert!(arm.axis(Axis::Z).abs_diff_eq(Point3::Z, 1e-6));
    Ok(())
}

#[test]
fn a_drag_moves_the_first_part_up_that_is_not_fixed() -> anyhow::Result<()> {
    let mut m = hinged()?;
    // The hinged pair, placed in an assembly above it.
    m.update(Message::Load {
        parts: BTreeMap::from([(
            "top.wpc".to_owned(),
            Part::new(vec![
                AddPart {
                    id: "sub".into(),
                    args: AddPartArgs {
                        part: "hinged.wpc".into(),
                        variant: None,
                        hold: Default::default(),
                        bodies: Vec::new(),
                    },
                }
                .into(),
            ]),
        )]),
        open: Some("top.wpc".into()),
    })?;
    m.update(Message::Tick(1.0))?;
    let grab = Point3::new(2.7, 0.0, 0.3);
    let start = |m: &mut Model| -> anyhow::Result<()> {
        hover(m, grab.as_vec3())?;
        m.update(Message::Pointer(Pointer::Press))?;
        m.update(Message::Pointer(Pointer::DragStart))
    };
    // The arm is free in its own assembly: that is where it moves.
    start(&mut m)?;
    let moving = m.viewport.moving.clone().expect("the arm is grabbed");
    assert_eq!(moving.part, "hinged.wpc");
    assert_eq!(moving.within, ["sub"]);
    assert_eq!(moving.instance, "arm");
    hover(&mut m, Vec3::new(1.5, 2.0, 0.3))?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    assert!(
        m.workspace
            .part("hinged.wpc")?
            .cache
            .get("arm/pose")
            .is_some()
    );
    assert!(m.workspace.part("top.wpc")?.cache.get("sub/pose").is_none());
    m.update(Message::Undo)?;

    // Fixed in its own assembly, it takes that whole assembly along.
    {
        let part = m.workspace.part_mut("hinged.wpc")?;
        let Some(add) = part.program[1].downcast_mut::<workpiece_ops_instances::AddPart>() else {
            anyhow::bail!("expected the arm's step");
        };
        add.args.hold = workpiece_ops_instances::Hold::fixed();
    }
    m.update(Message::Open("top.wpc".into()))?;
    start(&mut m)?;
    let moving = m.viewport.moving.clone().expect("the pair is grabbed");
    assert_eq!(moving.part, "top.wpc");
    assert!(moving.within.is_empty());
    assert_eq!(moving.instance, "sub");
    hover(&mut m, (grab + Point3::new(1.0, 0.5, 0.0)).as_vec3())?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let sub = m.viewport.shown.world.instances["sub"].pose;
    assert!(sub.translation().length() > 0.5, "{sub:?}");
    Ok(())
}

#[test]
fn a_part_is_added_with_its_joint_and_previewed_where_it_goes() -> anyhow::Result<()> {
    use workpiece_core::world::JointKind;
    let mut m = hinged()?;
    // The base alone; the arm is added again: a reference on each of
    // the two, and then the constraint between them.
    m.update(Message::RemoveOperation(vec!["hinge".into()]))?;
    m.update(Message::RemoveOperation(vec!["arm".into()]))?;
    m.update(Message::AddOperation {
        part: "hinged.wpc".into(),
        op: AddPart {
            id: "arm".into(),
            args: AddPartArgs {
                part: "arm.wpc".into(),
                variant: None,
                hold: workpiece_ops_instances::Hold::to_pick(),
                bodies: Vec::new(),
            },
        }
        .into(),
    })?;
    let picking = |m: &Model| {
        let view = View::new(m);
        let field = view
            .edit_dialog
            .as_ref()?
            .fields()
            .at(m.picking.as_ref()?)?;
        match field {
            Widget::ConnectorInput(input) => Some(input.label.clone()),
            _ => None,
        }
    };
    // First what it goes onto: only what is not the new part itself.
    assert_eq!(picking(&m).as_deref(), Some("Onto"));
    hover(&mut m, Vec3::new(0.7, -0.7, 0.25))?;
    let Some(Candidate::Reference { instance, .. }) = m.viewport.hovered() else {
        anyhow::bail!("nothing of the base under the pointer");
    };
    assert_eq!(instance, &["base".to_owned()]);
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    // Then the connector on the part: pointing at it shows the part
    // where the joint would bring it, before anything is picked.
    assert_eq!(picking(&m).as_deref(), Some("With"));
    assert!(m.joint_preview().is_empty());
    hover(&mut m, Vec3::new(2.5, 0.1, 0.3))?;
    let preview = m.joint_preview();
    assert!(!preview.is_empty(), "the part is previewed");
    let ghost = preview.items[0]
        .pose
        .transform_point(Point3::new(0.0, 0.0, 0.3));
    assert!(
        ghost.abs_diff_eq(Point3::new(0.0, 0.0, 0.25), 1e-6),
        "{ghost}"
    );
    assert!(View::new(&m).viewport.ghosts.items.len() > m.viewport.shown.ghosts.items.len());
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    // With the pair complete, the constraints that fit it are offered
    // as a drop-down, and one of them is chosen already.
    let view = View::new(&m);
    let fields = &view.edit_dialog.as_ref().expect("the dialog").fields();
    let mut constraint = None;
    fields.visit(&mut |w| {
        if let Widget::Choice(c) = w
            && c.label == "Constraint"
        {
            constraint = Some((c.shown, c.options.clone(), c.pictured()));
        }
    });
    let (shown, options, pictured) = constraint.expect("the constraint's choice");
    assert_eq!(shown, workpiece_core::widgets::Shown::DropDown);
    assert!(options.len() > 2 && pictured, "{options:?}");
    let added = m.workspace.part("hinged.wpc")?;
    let added = added.operation("arm").expect("the step");
    let hold = &added.downcast_ref::<AddPart>().expect("a part").args.hold;
    assert_eq!(hold.mates[0].kind, JointKind::Fastened);
    m.update(Message::FinishEdit)?;
    assert_eq!(m.error(), None);
    // It lies where it was previewed, held by a joint named after it.
    let world = &m.viewport.shown.world;
    assert_eq!(world.joints.len(), 1);
    assert_eq!(world.joints[0].name, "arm");
    let top = world.instances["arm"]
        .pose
        .transform_point(Point3::new(0.0, 0.0, 0.3));
    assert!(top.abs_diff_eq(Point3::new(0.0, 0.0, 0.25), 1e-6), "{top}");
    Ok(())
}

#[test]
fn a_new_part_is_made_in_place_and_worked_on_there() -> anyhow::Result<()> {
    let mut m = hinged()?;
    m.update(Message::NewPartHere(Some("cover.wpc".into())))?;
    assert!(m.workspace.parts.contains_key("cover.wpc"));
    // Added to the assembly, and focused: the next step goes into it.
    let assembly = m.workspace.part("hinged.wpc")?;
    assert!(assembly.operation("cover").is_some());
    assert_eq!(m.focus, ["cover"]);
    assert_eq!(m.focused_part().as_deref(), Some("cover.wpc"));
    // One step to undo.
    m.update(Message::Undo)?;
    assert!(!m.workspace.parts.contains_key("cover.wpc"));
    assert!(m.workspace.part("hinged.wpc")?.operation("cover").is_none());
    Ok(())
}

#[test]
fn a_mechanism_ties_two_joints_pointed_at_in_the_view() -> anyhow::Result<()> {
    use workpiece_core::world::CouplingKind;
    use workpiece_ops_instances::{Mechanism, MechanismArgs};
    let mut m = hinged()?;
    m.update(Message::AddOperation {
        part: "hinged.wpc".into(),
        op: Mechanism::new("mesh").into(),
    })?;
    // It asks for its first joint at once: the joints that turn are
    // drawn where they are, the hinge at the base's origin.
    let picking = |m: &Model| {
        let view = View::new(m);
        let field = view
            .edit_dialog
            .as_ref()?
            .fields()
            .at(m.picking.as_ref()?)?;
        match field {
            Widget::JointInput(input) => Some(input.label.clone()),
            _ => None,
        }
    };
    assert_eq!(picking(&m).as_deref(), Some("First joint"));
    let quiet = m.handle_scene().front.lines.len();
    assert!(quiet > 20, "the hinge's glyph");
    // Pointed at, it is the candidate, named at the pointer, and
    // highlighted; clicked, it is the mechanism's first joint.
    hover(&mut m, Vec3::new(0.0, 0.0, 0.0))?;
    assert!(matches!(
        m.viewport.hovered(),
        Some(Candidate::Joint { name, .. }) if name == "hinge"
    ));
    let view = View::new(&m);
    let said = view.viewport.candidate.map(|c| c.2);
    assert_eq!(said.as_deref(), Some("Revolute joint hinge"));
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    let mechanism = |m: &Model| -> Option<MechanismArgs> {
        let part = m.workspace.part("hinged.wpc").ok()?;
        let op = part.operation("mesh")?;
        Some(op.downcast_ref::<Mechanism>()?.args.clone())
    };
    assert_eq!(mechanism(&m).and_then(|a| a.a).as_deref(), Some("hinge"));
    // On to the second; the same hinge twice is refused.
    assert_eq!(picking(&m).as_deref(), Some("Second joint"));
    hover(&mut m, Vec3::new(0.0, 0.0, 0.0))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    let args = mechanism(&m).expect("the mechanism");
    assert_eq!(args.b.as_deref(), Some("hinge"));
    assert!(matches!(args.kind, CouplingKind::Gear { .. }));
    assert!(
        m.error().is_some_and(|e| e.contains("two joints")),
        "{:?}",
        m.error()
    );
    Ok(())
}

#[test]
fn parts_are_dragged_while_they_are_placed_and_fixed_ones_stay() -> anyhow::Result<()> {
    let mut m = hinged()?;
    // Another arm is added, lying off to the side: it asks for what it
    // goes onto, and has no gizmo — it is dragged.
    m.update(Message::AddOperation {
        part: "hinged.wpc".into(),
        op: AddPart {
            id: "arm2".into(),
            args: AddPartArgs {
                part: "arm.wpc".into(),
                variant: None,
                hold: workpiece_ops_instances::Hold::to_pick(),
                bodies: Vec::new(),
            },
        }
        .into(),
    })?;
    m.update(Message::Keep {
        part: "hinged.wpc".into(),
        id: "arm2".into(),
        kept: workpiece_ops_instances::assembly::kept(&Pose::from_translation(Point3::new(
            0.0, 3.0, 0.0,
        ))),
    })?;
    assert!(m.picking.is_some());
    assert!(
        m.session()
            .is_some_and(|s| s.visualization.handles.is_empty())
    );
    let pose = |m: &Model, name: &str| m.viewport.shown.world.instances[name].pose;
    let drag = |m: &mut Model, from: Point3, to: Point3| -> anyhow::Result<()> {
        hover(m, from.as_vec3())?;
        m.update(Message::Pointer(Pointer::Press))?;
        m.update(Message::Pointer(Pointer::DragStart))?;
        for step in 1..=10 {
            hover(m, from.lerp(to, f64::from(step) / 10.0).as_vec3())?;
        }
        m.update(Message::Pointer(Pointer::DragEnd))
    };
    // Free, it goes where it is pulled; the step is still edited.
    drag(
        &mut m,
        Point3::new(2.0, 3.0, 0.3),
        Point3::new(2.0, 4.5, 0.3),
    )?;
    // (Its point under the pointer follows the pointer, in the plane
    // through where it was grabbed that faces the view.)
    let moved = pose(&m, "arm2").translation();
    assert!(moved.distance(Point3::new(0.0, 3.0, 0.0)) > 0.8, "{moved}");
    assert!(moved.y > 3.8, "{moved}");
    assert_eq!(m.editing, ["arm2"]);
    // The base is fixed: pulled, it stays where it is.
    drag(
        &mut m,
        Point3::new(-0.7, -0.7, 0.25),
        Point3::new(-2.0, -2.0, 0.25),
    )?;
    assert_eq!(pose(&m, "base"), Pose::IDENTITY);
    Ok(())
}

#[test]
fn edges_are_picked_into_a_fillet_and_rounded() -> anyhow::Result<()> {
    let mut m = with_box()?;
    click(&mut m, "Fillet")?;
    // The dialog opens asking for edges, and takes them one by one.
    assert_eq!(m.editing, ["fillet1"]);
    assert_eq!(m.picking, Some(vec![0]));
    let fillet = |m: &Model| match m.workspace.parts["bolt"]
        .program
        .last()
        .and_then(|op| op.downcast_ref::<workpiece_ops_blend::Fillet>())
    {
        Some(fillet) => fillet.args.clone(),
        None => panic!("expected a fillet, found another step"),
    };
    hover(&mut m, Vec3::new(0.5, 0.1, 0.5))?;
    let is_edge = |c: &Candidate| {
        let topo = &m.viewport.shown.world.topo;
        matches!(c, Candidate::Reference { source: Source::Topology(name), .. }
            if topo.resolve::<workpiece_core::world::topo::EdgeId>(name).is_some())
    };
    let edge = m
        .viewport
        .candidates
        .iter()
        .position(is_edge)
        .ok_or_else(|| anyhow::anyhow!("no edge under the pointer"))?;
    for _ in 0..edge {
        m.update(Message::Pointer(Pointer::Cycle))?;
    }
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(fillet(&m).edges.len(), 1, "{:?}", m.viewport.candidates);
    // The round shows while the step is edited, and the list goes on
    // taking edges.
    assert_eq!(m.viewport.shown.error, None);
    let rounded = |m: &Model| {
        let topo = &m.viewport.shown.world.topo;
        topo.faces()
            .iter()
            .filter(|(id, _)| topo.name_of(*id).is_some_and(|n| n.starts_with("fillet1/")))
            .count()
    };
    assert_eq!(rounded(&m), 1);
    assert_eq!(m.picking, Some(vec![0]));
    // The edge it rounds is gone from what shows, and can still be
    // pointed at (to take it out again); the edges beside it, shorter
    // now, are what they were.
    let picked = fillet(&m).edges[0].name().to_owned();
    let offered = |m: &Model, name: &str| {
        m.viewport.candidates.iter().any(
            |c| matches!(c, Candidate::Reference { source: Source::Topology(n), .. } if n == name),
        )
    };
    hover(&mut m, Vec3::new(0.5, 0.1, 0.5))?;
    assert!(offered(&m, &picked));
    hover(&mut m, Vec3::new(0.5, 0.5, 0.0))?;
    assert!(m.viewport.candidates.iter().any(|c| {
        matches!(c, Candidate::Reference { source: Source::Topology(n), .. }
            if m.viewport.shown.world.topo.resolve::<workpiece_core::world::topo::EdgeId>(n).is_some())
    }));
    m.update(Message::FinishEdit)?;
    assert_eq!(rounded(&m), 1);
    let step = View::new(&m)
        .timeline
        .steps
        .last()
        .cloned()
        .expect("the step");
    assert_eq!(step.description, "radius 0.1 on 1 pick");
    Ok(())
}

#[test]
fn the_model_is_drawn_cut_open_without_being_changed() -> anyhow::Result<()> {
    use crate::model::display::Display;
    use std::sync::Arc;
    use workpiece_core::math::Point3;
    use workpiece_core::world::{Instance, World};
    // A 2 x 3 x 4 block cut halfway along x: half of it is drawn.
    let mut world = World::new();
    let axes = [Point3::X, Point3::Y, Point3::Z];
    let block = workpiece_boolean::block("b", Point3::ZERO, axes, [2.0, 3.0, 4.0])?;
    workpiece_boolean::write(&mut world.topo, &block, "b")?;
    let half = workpiece_core::world::Cutting::plane(Point3::new(1.0, 0.0, 0.0), Point3::X);
    world.sections.insert("half".into(), half);
    let world = Arc::new(world);
    let volume = |w: &World| -> anyhow::Result<f64> {
        let body = w.topo.bodies().ids().next().expect("a body");
        Ok(workpiece_boolean::measure(&workpiece_boolean::read(&w.topo, body)?).volume)
    };
    let cut = Display {
        cut: Some("half".into()),
        explode: 0.0,
        ..Display::default()
    };
    let drawn = cut.of(&world);
    assert_eq!(drawn.topo.validate(), Ok(()));
    assert!((volume(&drawn)? - 12.0).abs() < 1e-9);
    assert!((volume(&world)? - 24.0).abs() < 1e-9);
    // Two of them placed 10 apart: drawn apart, they are 20 apart.
    let mut both = World::new();
    for (name, x) in [("left", -5.0), ("right", 5.0)] {
        both.instances.insert(
            name.into(),
            Instance {
                pose: workpiece_core::Pose::from_translation(Point3::new(x, 0.0, 0.0)),
                world: world.clone(),
                fixed: false,
            },
        );
    }
    let apart = Display {
        explode: 1.0,
        ..Display::default()
    }
    .of(&Arc::new(both));
    let x = |name: &str| apart.instances[name].pose.translation().x;
    assert!((x("right") - x("left") - 20.0).abs() < 1e-9);
    Ok(())
}

#[test]
fn an_assembly_is_measured_as_a_whole() -> anyhow::Result<()> {
    use std::sync::Arc;
    use workpiece_core::math::Point3;
    use workpiece_core::world::{Instance, World};
    // Three 2 x 2 x 2 blocks, the second 1 along x from the first (4 of
    // each is in the other) and the third well apart: 24 in all.
    let mut part = World::new();
    let axes = [Point3::X, Point3::Y, Point3::Z];
    let block = workpiece_boolean::block("b", Point3::ZERO, axes, [2.0; 3])?;
    workpiece_boolean::write(&mut part.topo, &block, "b")?;
    let lowest = part
        .topo
        .vertices()
        .iter()
        .find(|(_, v)| v.point.length() < 1e-12)
        .map(|(id, _)| id)
        .expect("the corner at the origin");
    let corner_name = part.topo.name_of(lowest).expect("its name").to_owned();
    let part = Arc::new(part);
    let mut both = World::new();
    for (name, x) in [("left", 0.0), ("right", 1.0), ("far", 6.0)] {
        both.instances.insert(
            name.into(),
            Instance {
                pose: workpiece_core::Pose::from_translation(Point3::new(x, 0.0, 0.0)),
                world: part.clone(),
                fixed: false,
            },
        );
    }
    let assembly = Arc::new(both);
    let readings = crate::view::inspect::of_assembly(&assembly);
    let says = |label: &str| {
        let found = readings.iter().find(|r| r.label == label);
        found.map(|r| r.value.clone())
    };
    assert_eq!(says("Volume of all").as_deref(), Some("24"));
    // (The third is 3 from the second.)
    assert_eq!(says("Least clearance").as_deref(), Some("≈ 3 (far, right)"));
    assert_eq!(says("left in right").as_deref(), Some("4"));
    // The low corners of the first and of the third, selected each in
    // its part: 6 apart, as the parts are placed.
    let corner = |part: &str| crate::picking::Candidate::Reference {
        instance: vec![part.to_owned()],
        source: workpiece_core::Source::Topology(corner_name.clone()),
    };
    let between = crate::view::inspect::readings(&assembly, &[corner("left"), corner("far")], None);
    let distance = between.iter().find(|r| r.label == "Distance");
    assert_eq!(distance.map(|r| r.value.as_str()), Some("6"));
    Ok(())
}

#[test]
fn models_are_drawn_in_three_views_with_what_is_hidden_apart() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    use workpiece_core::world::World;
    use workpiece_ops_drawing::quick::ThreeViews as Drawing;
    // A 2 x 3 x 4 block: in each view its outline, four sides long, in
    // view; nothing of it is hidden but what lies right behind that.
    let axes = [Point3::X, Point3::Y, Point3::Z];
    let mut world = World::new();
    let block = workpiece_boolean::block("b", Point3::ZERO, axes, [2.0, 3.0, 4.0])?;
    workpiece_boolean::write(&mut world.topo, &block, "b")?;
    let drawing = Drawing::of(&world, "bracket", None)?;
    let long = |name: &str, behind: bool| -> f64 {
        let view = drawing
            .views
            .iter()
            .find(|v| v.name == name)
            .expect("the view");
        let lines = view.lines.iter().filter(|l| l.2 == behind);
        lines.map(|(a, b, _)| a.distance(*b)).sum()
    };
    // (Each side of the outline is two edges, one behind the other.)
    assert!((long("FRONT", false) + long("FRONT", true) - 2.0 * 12.0).abs() < 1e-9);
    assert!(
        (long("FRONT", false) - 12.0).abs() < 1e-9,
        "{}",
        long("FRONT", false)
    );
    // A small block behind it, seen from the front: all of it hidden.
    let small = workpiece_boolean::block("s", Point3::new(0.5, 5.0, 1.0), axes, [1.0; 3])?;
    workpiece_boolean::write(&mut world.topo, &small, "s")?;
    let drawing = Drawing::of(&world, "bracket", None)?;
    let long = |name: &str, behind: bool| -> f64 {
        let view = drawing
            .views
            .iter()
            .find(|v| v.name == name)
            .expect("the view");
        let lines = view.lines.iter().filter(|l| l.2 == behind);
        lines.map(|(a, b, _)| a.distance(*b)).sum()
    };
    assert!(
        (long("FRONT", false) - 12.0).abs() < 1e-9,
        "{}",
        long("FRONT", false)
    );
    assert!(long("FRONT", true) > 12.0 + 7.9);
    // A square hole, 0.5 wide, down through the big block: open in the
    // section through the middle, where its two walls show, in view.
    let big = workpiece_boolean::read(&world.topo, world.topo.resolve("b").expect("the block"))?;
    let rod = workpiece_boolean::block("r", Point3::new(0.75, 1.25, -1.0), axes, [0.5, 0.5, 6.0])?;
    let drilled = workpiece_boolean::combine(workpiece_boolean::Op::Cut, &big, &rod)?.solid;
    let mut holed = World::new();
    workpiece_boolean::write(&mut holed.topo, &drilled, "b")?;
    let section = Drawing::of(&holed, "holed", None)?;
    // (Upright lines inside the outline: the hole's walls. From the
    // front they are hidden; in the section they are in view.)
    let walls = |name: &str, hidden: bool| -> f64 {
        let view = section
            .views
            .iter()
            .find(|v| v.name == name)
            .expect("the view");
        let inside = view.lines.iter().filter(|(a, b, behind)| {
            let across = a.x - view.corner.x;
            *behind == hidden && (a.x - b.x).abs() < 1e-9 && across > 0.1 && across < 1.9
        });
        inside.map(|(a, b, _)| a.distance(*b)).sum()
    };
    assert!(walls("FRONT", false) < 1e-9 && walls("FRONT", true) > 15.9);
    assert!(
        walls("SECTION", false) > 15.9,
        "{}",
        walls("SECTION", false)
    );
    // Cut where it is asked for instead — square to z, halfway up: seen
    // from above, as wide and deep as the block.
    let across = Drawing::of(&holed, "holed", Some((workpiece_core::math::Axis::Z, 0.5)))?;
    assert!(
        across
            .dxf(workpiece_core::Unit::Millimetre)
            .contains("square to z, 50% along")
    );
    let cut = across
        .views
        .iter()
        .find(|v| v.name == "SECTION")
        .expect("the section");
    assert_eq!((cut.size.x, cut.size.y), (2.0, 3.0));
    // A washer seen from above: its two diameters are written.
    let washer = workpiece_ops_standard::standard(
        "w",
        workpiece_ops_standard::StandardKind::Washer,
        "M10",
        0.0,
    )?;
    let mut round = World::new();
    workpiece_boolean::write(&mut round.topo, &washer, "w")?;
    let ring = Drawing::of(&round, "washer", None)?;
    let top = ring
        .views
        .iter()
        .find(|v| v.name == "TOP")
        .expect("the view from above");
    let mut said: Vec<&str> = top.notes.iter().map(|n| n.1.as_str()).collect();
    said.sort_unstable();
    assert_eq!(said, ["DIA 10.5", "DIA 20"]);
    // The sketches' dimensions are a table beside the title.
    let mut drawing = drawing;
    drawing.dimensions = vec![("s".into(), vec!["12 ±0.1".into(), "Ø6".into()])];
    assert!(
        drawing
            .dxf(workpiece_core::Unit::Millimetre)
            .contains("s: 12 +/-0.1, DIA 6")
    );
    // As files: a DXF with layers for what is hidden, a PDF of one page.
    let text = drawing.dxf(workpiece_core::Unit::Millimetre);
    assert!(text.contains("$INSUNITS\n70\n4\n") && text.ends_with("0\nEOF\n"));
    assert!(text.contains("FRONT_HIDDEN") && text.contains("FRONT 2 x 4 mm"));
    assert!(text.contains("TITLE") && text.contains("bracket") && text.contains("SECTION A-A"));
    let pdf = drawing.pdf(workpiece_core::Unit::Millimetre);
    assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
    if let Ok(to) = std::env::var("WORKPIECE_PDF") {
        std::fs::write(to, &pdf)?;
    }
    assert!(Drawing::of(&World::new(), "nothing", None).is_err());
    Ok(())
}

#[test]
fn a_3mf_file_written_reads_back_as_the_solid() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    let mut world = workpiece_core::world::World::new();
    let axes = [Point3::X, Point3::Y, Point3::Z];
    let block = workpiece_boolean::block("b", Point3::ZERO, axes, [2.0, 3.0, 4.0])?;
    workpiece_boolean::write(&mut world.topo, &block, "b")?;
    let file =
        workpiece_rasterize::tessellate::three_mf(&world.topo, workpiece_core::Unit::Millimetre);
    let back = workpiece_ops_step::import_3mf(&file, "mesh")?;
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].faces.len(), 6);
    assert!((workpiece_boolean::measure(&back[0]).volume - 24.0).abs() < 1e-9);
    Ok(())
}

#[test]
fn a_plate_with_a_hole_is_cut_hole_first() -> anyhow::Result<()> {
    use workpiece_core::math::Point3;
    // A plate 6 x 4 x 1 with a square hole 1 wide: two outlines, the
    // hole's (4 around) before the plate's (20 around).
    let axes = [Point3::X, Point3::Y, Point3::Z];
    let plate = workpiece_boolean::block("p", Point3::ZERO, axes, [6.0, 4.0, 1.0])?;
    let hole = workpiece_boolean::block("h", Point3::new(1.0, 1.0, -1.0), axes, [1.0, 1.0, 3.0])?;
    let holed = workpiece_boolean::combine(workpiece_boolean::Op::Cut, &plate, &hole)?.solid;
    let mut world = workpiece_core::world::World::new();
    workpiece_boolean::write(&mut world.topo, &holed, "p")?;
    let code = crate::cam::cut_path(&world, workpiece_core::Unit::Millimetre, 800.0)?;
    assert!(code.contains("G21") && code.ends_with("M2\n"));
    // How long each outline is, in the order they are cut.
    let mut long = Vec::new();
    let mut at: Option<(f64, f64)> = None;
    for line in code.lines() {
        let xy = || -> Option<(f64, f64)> {
            let mut words = line.split_whitespace().skip(1);
            let x = words.next()?.strip_prefix('X')?.parse().ok()?;
            let y = words.next()?.strip_prefix('Y')?.parse().ok()?;
            Some((x, y))
        };
        if line.starts_with("G0 ") {
            long.push(0.0);
            at = xy();
        } else if line.starts_with("G1 ")
            && let (Some(from), Some(to), Some(last)) = (at, xy(), long.last_mut())
        {
            *last += (to.0 - from.0).hypot(to.1 - from.1);
            at = Some(to);
        }
    }
    assert_eq!(long.len(), 2);
    assert!(
        (long[0] - 4.0).abs() < 1e-3 && (long[1] - 20.0).abs() < 1e-3,
        "{long:?}"
    );
    assert_eq!(code.matches("M3").count(), 2);
    // Milled instead, in passes 0.4 deep: three times around each
    // outline, the tool kept to the waste side, down to the bottom.
    let milled = crate::cam::mill_path(&world, workpiece_core::Unit::Millimetre, 600.0, 0.5, 0.4)?;
    assert_eq!(milled.matches("G42 D1").count(), 6);
    assert_eq!(milled.matches("G40").count(), 6);
    assert!(milled.contains("G1 Z0.0000") && milled.ends_with("M5\nM2\n"));
    Ok(())
}

#[test]
fn a_joint_is_run_along_its_travel() -> anyhow::Result<()> {
    use crate::model::display::{Display, drivable};
    use std::sync::Arc;
    use workpiece_core::math::{Axis, Point3};
    use workpiece_core::world::{Connector, Instance, Joint, JointKind, Limits, World};
    // A lid hinged to a base, to turn half a radian either way: run
    // from one limit to the other, it turns by one radian.
    let mut world = World::new();
    for (name, fixed) in [("base", true), ("lid", false)] {
        world.instances.insert(
            name.into(),
            Instance {
                pose: workpiece_core::Pose::IDENTITY,
                world: Arc::new(World::new()),
                fixed,
            },
        );
    }
    let at = |name: &str| Connector {
        instance: Some(name.to_owned()),
        frame: workpiece_core::Pose::from_translation(Point3::new(1.0, 0.0, 0.0)),
    };
    world.joints.push(Joint {
        name: "hinge".into(),
        kind: JointKind::Revolute,
        a: at("base"),
        b: at("lid"),
        limits: Limits {
            angle: Some((-0.5, 0.5)),
            distance: None,
        },
    });
    let world = Arc::new(world);
    assert_eq!(drivable(&world), ["hinge"]);
    let lid_at = |along: f64| {
        let display = Display {
            motion: Some((0, along)),
            ..Display::default()
        };
        display.of(&world).instances["lid"].pose.axis(Axis::X)
    };
    let turned = lid_at(0.0).dot(lid_at(1.0)).clamp(-1.0, 1.0).acos();
    assert!((turned - 1.0).abs() < 1e-6, "{turned}");
    Ok(())
}

#[test]
fn sheet_metal_has_a_tab_of_its_own() -> anyhow::Result<()> {
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    let tools = |m: &Model| -> Vec<String> {
        let ribbon = View::new(m).ribbon;
        ribbon
            .tools
            .iter()
            .map(|t| t.button.label.clone())
            .collect()
    };
    // Modeling has neither of them.
    let modeling = tools(&m);
    assert!(modeling.contains(&"Extrude".to_owned()), "{modeling:?}");
    assert!(!modeling.iter().any(|t| t == "Flange" || t == "Unfold"));
    // The tab, by its key, has both, and what a plate is made with.
    press_key(&mut m, workpiece_core::Hotkey::ctrl('4'))?;
    assert_eq!(m.tab, crate::Tab::SheetMetal);
    let sheet = tools(&m);
    for tool in ["Sketch", "Extrude", "Flange", "Unfold", "Hole"] {
        assert!(sheet.contains(&tool.to_owned()), "no {tool} in {sheet:?}");
    }
    // A flange is added from there, and edited with the dialog its
    // arguments give.
    click(&mut m, "Flange")?;
    assert_eq!(program(&m, "bolt"), ["flange1"]);
    assert_eq!(m.editing.last().map(String::as_str), Some("flange1"));
    Ok(())
}

#[test]
fn a_reference_that_lost_its_name_is_found_again_and_said() -> anyhow::Result<()> {
    use workpiece_ops_blend::{Fillet, FilletArgs};
    let round = |edge: &str| -> AnyOperation {
        Fillet {
            id: "round".into(),
            args: FilletArgs {
                edges: vec![Source::Topology(edge.to_owned()).into()],
                ..Default::default()
            },
        }
        .into()
    };
    let mut m = with_box()?;
    m.update(Message::AddOperation {
        part: "bolt".into(),
        op: round("box1/edge0"),
    })?;
    m.update(Message::FinishEdit)?;
    assert_eq!(m.error(), None);
    assert_eq!(m.notice(), None);
    // Where the edge is was kept with the part.
    let kept = m.workspace.parts["bolt"].cache.get("round@refs/box1/edge0");
    let kept = kept.expect("where the edge is").to_vec();
    // The edge under a name that is no more, as after a change upstream.
    let part = m.workspace.part_mut("bolt")?;
    let last = part.program.len() - 1;
    part.program[last] = round("box1/gone");
    part.cache
        .replace("round@refs", [("box1/gone".to_owned(), kept)]);
    if let Ok(path) = std::env::var("WORKPIECE_DUMP_LOST") {
        std::fs::write(path, m.workspace.parts["bolt"].to_text()?)?;
    }
    m.update(Message::Open("bolt".into()))?;
    // It is found where it was: the step refers to it again, and the
    // user is told.
    assert_eq!(m.error(), None);
    let Some(fillet) = m.workspace.parts["bolt"].program[last].downcast_ref::<Fillet>() else {
        anyhow::bail!("expected the fillet");
    };
    assert_eq!(fillet.args.edges[0].name(), "box1/edge0");
    let notice = m.notice().expect("a notice");
    assert!(
        notice.contains("box1/gone") && notice.contains("box1/edge0"),
        "{notice}"
    );
    Ok(())
}

/// The candidate under the pointer that is a face, brought to the front.
fn face_under(m: &mut Model, at: Vec3) -> anyhow::Result<()> {
    hover(m, at)?;
    let topo = &m.viewport.shown.world.topo;
    let face = m
        .viewport
        .candidates
        .iter()
        .position(|c| {
            matches!(c, Candidate::Reference { source: Source::Topology(name), .. }
            if topo.resolve::<workpiece_core::world::topo::FaceId>(name).is_some())
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no face under the pointer: {:?} picking {:?}",
                m.viewport.candidates,
                m.picking
            )
        })?;
    for _ in 0..face {
        m.update(Message::Pointer(Pointer::Cycle))?;
    }
    Ok(())
}

#[test]
fn tools_take_what_is_selected() -> anyhow::Result<()> {
    let mut m = with_box()?;
    face_under(&mut m, Vec3::new(0.2, 0.15, 0.5))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    // A shell opens with the face picked as where it is open.
    click(&mut m, "Shell")?;
    let shell = m.workspace.parts["bolt"]
        .program
        .last()
        .and_then(|op| op.downcast_ref::<workpiece_ops_faces::Shell>())
        .expect("a shell");
    assert_eq!(shell.args.faces.len(), 1);
    Ok(())
}

#[test]
fn a_plane_picked_for_a_sketch_gets_one_drawn_first() -> anyhow::Result<()> {
    let mut m = with_box()?;
    click(&mut m, "Extrude")?;
    assert_eq!(m.editing, ["extrude1"]);
    assert!(m.picking.is_some(), "picking the sketch");
    // A face where the sketch is asked for: a sketch on it, before the
    // extrusion, and it is drawn first.
    face_under(&mut m, Vec3::new(0.2, 0.15, 0.5))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(program(&m, "bolt"), ["box1", "sketch1", "extrude1"]);
    assert_eq!(m.editing, ["sketch1"]);
    let sketch_of = |m: &Model| {
        m.workspace.parts["bolt"]
            .operation("extrude1")
            .and_then(|op| op.downcast_ref::<workpiece_ops_sweep::Extrude>())
            .and_then(|e| e.args.sketch.as_ref().map(|s| s.name().to_owned()))
    };
    assert_eq!(sketch_of(&m).as_deref(), Some("sketch1"));
    // Done drawing: on with the extrusion.
    m.update(Message::FinishEdit)?;
    assert_eq!(m.editing, ["extrude1"]);
    // Cancelling it now takes only it away: the sketch stays.
    m.update(Message::CancelEdit)?;
    assert!(m.editing.is_empty());
    assert_eq!(program(&m, "bolt"), ["box1", "sketch1"]);
    // Cancelling the sketch instead goes back to the extrusion as it
    // was, still asking.
    click(&mut m, "Extrude")?;
    face_under(&mut m, Vec3::new(0.2, 0.15, 0.5))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(m.editing, ["sketch2"]);
    m.update(Message::CancelEdit)?;
    assert_eq!(m.editing, ["extrude1"]);
    assert_eq!(program(&m, "bolt"), ["box1", "sketch1", "extrude1"]);
    Ok(())
}

#[test]
fn a_part_is_made_of_a_material_and_steps_give_faces_their_own() -> anyhow::Result<()> {
    use workpiece_core::world::Look;
    use workpiece_core::world::topo::FaceId;
    use workpiece_ops_appearance::{Appearance, AppearanceArgs};
    let mut m = with_box()?;
    // The materials are the library's; their panel shows when asked
    // for, whatever the tab.
    assert!(View::new(&m).materials.is_none());
    m.update(Message::ShowPanel(Some(crate::Panel::Materials)))?;
    let materials = View::new(&m).materials.expect("the materials");
    assert!(materials.library.iter().any(|x| x.name == "Oak"));
    m.update(Message::ShowPanel(None))?;
    // Of oak: all of it, and it weighs by oak's density (a cubic
    // millimetre at 750 kg/m³).
    assert!(m.density().is_none());
    m.update(Message::SetPartMaterial(Some("Oak".into())))?;
    let density = m.density().expect("a density");
    assert!((density - 750.0e-9).abs() < 1e-15, "{density}");
    let look = |m: &Model, face: &str| {
        let world = &m.viewport.shown.world;
        let face = world.topo.resolve::<FaceId>(face).expect("the face");
        world.face_look(face, Some("box1"))
    };
    assert_eq!(look(&m, "box1/top").material.as_deref(), Some("Oak"));
    // A step gives the top another material and a colour: it stands
    // among the part's steps, and the other faces stay as they are.
    let part = m.focused_part().expect("a part");
    m.update(Message::AddOperation {
        part,
        op: Appearance {
            id: "appearance1".into(),
            args: AppearanceArgs {
                of: vec![workpiece_core::Reference::topology("box1/top")],
                material: Some("Brass".into()),
                colour: Some([200, 0, 0]),
            },
        }
        .into(),
    })?;
    // Its dialog offers the library's materials, and a colour.
    let dialog = View::new(&m).edit_dialog.expect("its dialog");
    let mut choose = None;
    dialog.fields().visit(&mut |widget| {
        if let workpiece_core::Widget::Choice(choice) = widget
            && choice.label == "Material"
        {
            let copper = choice.options.iter().position(|o| o == "Copper");
            choose = copper.map(|i| (choice.on_change)(i));
        }
    });
    m.update(choose.expect("copper is offered"))?;
    m.update(Message::FinishEdit)?;
    assert_eq!(program(&m, "bolt"), ["box1", "appearance1"]);
    assert_eq!(look(&m, "box1/top").material.as_deref(), Some("Copper"));
    // (And back to brass, by the step's arguments.)
    let part = m.focused_part().expect("a part");
    m.update(Message::ReplaceOperation {
        part,
        op: Appearance {
            id: "appearance1".into(),
            args: AppearanceArgs {
                of: vec![workpiece_core::Reference::topology("box1/top")],
                material: Some("Brass".into()),
                colour: Some([200, 0, 0]),
            },
        }
        .into(),
    })?;
    let painted = Look {
        material: Some("Brass".into()),
        colour: Some([200, 0, 0]),
    };
    assert_eq!(look(&m, "box1/top"), painted);
    assert_eq!(look(&m, "box1/bottom").material.as_deref(), Some("Oak"));
    assert_eq!(look(&m, "box1/bottom").colour, None);
    // A material changed in the library is changed for the part; and
    // that can be undone.
    let mut library = m.workspace.library.materials.clone();
    for material in library.iter_mut().filter(|x| x.name == "Oak") {
        material.density = 800.0;
    }
    m.update(Message::SetLibrary { materials: library })?;
    assert!((m.density().expect("a density") - 800.0e-9).abs() < 1e-15);
    m.update(Message::Undo)?;
    assert!((m.density().expect("a density") - 750.0e-9).abs() < 1e-15);
    Ok(())
}

#[test]
fn a_picture_is_a_step_taken_from_where_the_view_is() -> anyhow::Result<()> {
    use workpiece_ops_appearance::{Render, Shot};
    let mut m = with_box()?;
    assert!(View::new(&m).picture.is_none());
    let part = m.focused_part().expect("a part");
    m.update(Message::AddOperation {
        part,
        op: Render {
            id: "render1".into(),
            args: Shot::default(),
        }
        .into(),
    })?;
    // While it is edited, the picture shows: what is traced is the
    // box, and no view is taken yet.
    let picture = View::new(&m).picture.expect("the picture");
    assert!(picture.scene.bounds().is_some());
    assert!(!picture.taken);
    // The view as it is becomes the picture's; turned, it is another.
    m.update(Message::TakeView)?;
    let picture = View::new(&m).picture.expect("the picture");
    assert!(picture.taken);
    assert_eq!(
        picture.shot.view.map(|v| v.distance),
        Some(m.viewport.camera.distance)
    );
    m.update(Message::Camera(CameraMove::LookFrom([1.0, 0.2, 0.4])))?;
    m.update(Message::Tick(1.0))?;
    assert!(!View::new(&m).picture.expect("the picture").taken);
    // Done, the model shows again; edited again, the view goes back
    // to where the picture was taken from.
    m.update(Message::FinishEdit)?;
    assert!(View::new(&m).picture.is_none());
    m.update(Message::Edit(vec!["render1".into()]))?;
    m.update(Message::Tick(1.0))?;
    assert!(View::new(&m).picture.expect("the picture").taken);
    Ok(())
}

#[test]
fn moving_things_on_a_sheet_builds_and_sees_nothing_again() -> anyhow::Result<()> {
    let mut m = with_box()?;
    click(&mut m, "Drawing")?;
    let asked = View::new(&m).new_drawing.expect("asked");
    m.update(Message::NewDrawing(Some(asked.templates[0].clone())))?;
    let shown = View::new(&m).sheet.expect("the sheet");
    let (applied, kept) = (m.engine.applied(), m.sheets_kept.len());
    // A view dragged across the sheet, step by step.
    for k in 1..=5 {
        let shown = View::new(&m).sheet.expect("the sheet");
        let mut sheet = shown.sheet.clone();
        sheet.views[0].at[0] += 2.0 * f64::from(k);
        m.update(shown.set(sheet))?;
        assert!(View::new(&m).sheet.is_some());
    }
    // Only the drawing's own step was applied again, never one before
    // it; and what its views see was kept, not worked out anew.
    assert!(
        m.engine.applied() - applied <= 5,
        "{}",
        m.engine.applied() - applied
    );
    assert_eq!(m.sheets_kept.len(), kept);
    drop(shown);
    Ok(())
}

#[test]
fn a_drawing_shows_the_model_and_what_is_said_of_it() -> anyhow::Result<()> {
    use workpiece_core::world::Tolerance;
    use workpiece_ops_annotate::{
        DatumFeature, DatumFeatureArgs, Dimension, DimensionArgs, DimensionKind,
        GeometricTolerance, GeometricToleranceArgs, Of,
    };
    use workpiece_ops_drawing::sheet::{Beside, SheetView, Style, ViewKind};
    let mut m = with_box()?;
    let face = |name: &str| Source::Topology(format!("box1/{name}"));
    let add = |m: &mut Model, op: AnyOperation| -> anyhow::Result<()> {
        m.update(Message::AddOperation {
            part: "bolt".into(),
            op,
        })?;
        m.update(Message::FinishEdit)
    };
    // Said of the box: how high it is, that its bottom is datum A, and
    // how flat its top is.
    add(
        &mut m,
        Dimension {
            id: "height".into(),
            args: DimensionArgs {
                kind: DimensionKind::Auto,
                of: vec![face("top").into(), face("bottom").into()],
                tolerance: Tolerance::Symmetric(0.1),
                ..Default::default()
            },
        }
        .into(),
    )?;
    add(
        &mut m,
        DatumFeature {
            id: "datum1".into(),
            args: DatumFeatureArgs {
                feature: Some(face("bottom").into()),
                letter: "A".into(),
                at: None,
            },
        }
        .into(),
    )?;
    add(
        &mut m,
        GeometricTolerance {
            id: "flat".into(),
            args: GeometricToleranceArgs {
                feature: Some(face("top").into()),
                characteristic: Of(workpiece_core::world::Characteristic::Parallelism),
                value: workpiece_core::Measure::new(0.05),
                diametral: false,
                datums: "A".into(),
                at: None,
            },
        }
        .into(),
    )?;
    // They show on the model where what is said of it is looked at —
    // the Annotate tab, not the others — and stand where they are
    // dragged to.
    m.update(Message::SetTab(crate::Tab::Modeling))?;
    assert!(View::new(&m).viewport.notes.is_empty());
    m.update(Message::SetTab(crate::Tab::Annotate))?;
    let notes = View::new(&m).viewport.notes;
    assert_eq!(notes.len(), 3);
    let height = notes.iter().find(|n| n.name == "height").expect("it");
    assert_eq!(height.text, "1 ±0,1");
    // (As a drawing has it: two extension lines and the dimension
    // line, an arrowhead at each end, the value along the line.)
    assert!(height.strokes.len() >= 3 && height.fills.len() == 2);
    assert_eq!(height.texts.len(), 1);
    assert_eq!(height.texts[0].text, "1 ±0,1");
    let to = [3.0, 0.5, 2.0];
    m.update(Message::MoveNote {
        name: "height".into(),
        at: to,
        settled: true,
    })?;
    let notes = View::new(&m).viewport.notes;
    let height = notes.iter().find(|n| n.name == "height").expect("it");
    assert_eq!(height.at, glam::Vec3::new(3.0, 0.5, 2.0));
    assert_eq!(m.workspace.parts["bolt"].note_places["height"], to);
    // A drawing is a step: an A3 with the model from the front, from
    // the left and from above, framed and titled; all that is said of
    // the model is missing on it.
    assert!(View::new(&m).sheet.is_none());
    // (What it starts from is asked first: nothing, or the views of a
    // sheet template.)
    click(&mut m, "Drawing")?;
    let asked = View::new(&m).new_drawing.expect("asked");
    assert_eq!(asked.templates[0], "ISO A3, three views");
    m.update(Message::NewDrawing(Some(asked.templates[0].clone())))?;
    assert!(View::new(&m).new_drawing.is_none());
    let shown = View::new(&m).sheet.expect("the sheet");
    assert_eq!(shown.id, "drawing1");
    assert_eq!(shown.laid.size, [420.0, 297.0]);
    assert_eq!(shown.laid.views.len(), 3);
    assert!(shown.laid.problems.is_empty(), "{:?}", shown.laid.problems);
    let said = |laid: &workpiece_ops_drawing::lay::Laid, what: &str| {
        laid.texts.iter().any(|t| t.text == what)
    };
    assert!(said(&shown.laid, "Legal owner") && said(&shown.laid, "bolt"));
    assert_eq!(shown.laid.missing.len(), 3);
    assert!(shown.laid.notes.is_empty());
    // Asked for, what is missing shows as what it would be, in every
    // view that sees it true: the view from above sees the height end
    // on, the front view sees it true.
    m.update(Message::PreviewNotes(Some(None)))?;
    let shown = View::new(&m).sheet.expect("the sheet");
    let previews = |laid: &workpiece_ops_drawing::lay::Laid, name: &str| -> Vec<String> {
        let of = laid
            .notes
            .iter()
            .filter(|n| n.preview.as_ref().is_some_and(|preview| preview.0 == name));
        of.map(|n| n.view.clone()).collect()
    };
    let height = previews(&shown.laid, "height");
    assert!(height.contains(&"front".to_owned()) && !height.contains(&"top".to_owned()));
    assert!(shown.laid.strokes.iter().any(|s| s.preview));
    // (In one view only, if one is asked for.)
    m.update(Message::PreviewNotes(Some(Some("left".into()))))?;
    let shown = View::new(&m).sheet.expect("the sheet");
    assert_eq!(previews(&shown.laid, "height"), ["left"]);
    m.update(Message::PreviewNotes(None))?;
    assert!(
        m.update(Message::PlaceNote {
            note: "height".into(),
            view: Some("top".into()),
            at: None,
        })
        .is_err()
    );
    // Each put on the sheet, where it fits by itself: nothing is
    // missing, and they are drawn as a drawing says them.
    for note in ["height", "datum1", "flat"] {
        m.update(Message::PlaceNote {
            note: note.into(),
            view: None,
            at: None,
        })?;
    }
    let shown = View::new(&m).sheet.expect("the sheet");
    assert!(shown.laid.missing.is_empty());
    assert_eq!(shown.laid.notes.len(), 3);
    // (No line runs through the value of the dimension.)
    for note in shown.laid.notes.iter().filter(|n| n.index == 0) {
        let middle = (note.rect[0] + note.rect[1]) / 2.0;
        let through = shown.laid.strokes.iter().any(|s| {
            let d = s.b - s.a;
            let t = ((middle - s.a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
            (s.a + d * t).distance(middle) < 0.3
        });
        assert!(!through, "a line through the text of a note");
    }
    assert!(shown.laid.problems.is_empty(), "{:?}", shown.laid.problems);
    assert!(said(&shown.laid, "1 ±0,1") && said(&shown.laid, "A") && said(&shown.laid, "0,05"));
    // A detail of the front view, twice as large.
    let mut sheet = shown.sheet.clone();
    let lines = shown.laid.strokes.len();
    // (Something drawn on the sheet by hand: a line.)
    let mut hand = sheet.hand();
    let (a, b) = (
        hand.args
            .add_point(workpiece_core::math::Point2::new(20.0, 250.0)),
        hand.args
            .add_point(workpiece_core::math::Point2::new(60.0, 250.0)),
    );
    hand.args.add_line(a, b);
    sheet.drawn = Some(hand.args);
    sheet.views.push(SheetView {
        id: "detail1".into(),
        kind: ViewKind::Detail {
            of: "front".into(),
            centre: [0.4, 0.4],
            radius: 0.2,
            letter: "B".into(),
            letter_at: None,
        },
        at: [60.0, 60.0],
        scale: Some([2, 1]),
        style: Style::HiddenRemoved,
        bodies: Vec::new(),
        cut_only: false,
        half: None,
        section: None,
        caption_at: None,
    });
    sheet.views.push(SheetView {
        id: "back".into(),
        kind: ViewKind::Projected {
            of: "left".into(),
            beside: Beside::Right,
        },
        at: [380.0, 200.0],
        scale: None,
        style: Style::Shaded,
        bodies: Vec::new(),
        cut_only: false,
        half: None,
        section: None,
        caption_at: None,
    });
    m.update(shown.set(sheet))?;
    m.update(Message::EndEdit)?;
    // A section through the front view, hatched where it cuts: its
    // line is drawn in that view, and is the part's — a sketch of it
    // on the plane the view looks onto, and a section along that,
    // before the drawing.
    m.update(Message::CutSection {
        view: "front".into(),
        line: vec![[0.0, -1.0], [0.0, 1.0]],
        at: [330.0, 100.0],
    })?;
    // (And one whose cutting line steps aside half way: an offset
    // section.)
    m.update(Message::CutSection {
        view: "front".into(),
        line: vec![[-0.2, -1.0], [-0.2, 0.0], [0.2, 0.0], [0.2, 1.0]],
        // (Clear of the title block, which a view may not stand on.)
        at: [170.0, 40.0],
    })?;
    let steps: Vec<&str> = m.workspace.parts["bolt"]
        .program
        .iter()
        .map(|op| op.id())
        .collect();
    let at = |id: &str| steps.iter().position(|s| *s == id).expect("the step");
    assert!(at("cut1") < at("section1") && at("section2") < at("drawing1"));
    let world = m.viewport.shown.world.clone();
    assert_eq!(world.sections["section1"].path.len(), 2);
    assert_eq!(world.sections["section2"].path.len(), 4);
    let shown = View::new(&m).sheet.expect("the sheet");
    assert_eq!(shown.sections, ["section1", "section2"]);
    assert_eq!(shown.laid.cuts.len(), 2);
    // The line dragged on the sheet draws the part's sketch anew: the
    // section follows.
    assert!(shown.movable.contains(&"section1".to_owned()));
    m.update(Message::MoveSectionLine {
        view: "section1".into(),
        line: vec![[0.1, -1.0], [0.1, 1.0]],
        settled: true,
    })?;
    let moved = &m.viewport.shown.world.sections["section1"];
    assert_ne!(*moved, world.sections["section1"]);
    assert_eq!(moved.towards(), world.sections["section1"].towards());
    // Turned round from the sheet: the part's section shows the other
    // side, and the view with it.
    let towards = moved.towards();
    m.update(Message::FlipSection {
        view: "section1".into(),
    })?;
    let flipped = &m.viewport.shown.world.sections["section1"];
    assert!((flipped.towards() + towards).length() < 1e-9);
    // The model itself shows cut open so, if asked.
    let whole = View::new(&m).sheet.is_some();
    assert!(whole);
    let shown = View::new(&m).sheet.expect("the sheet");
    assert_eq!(shown.laid.views.len(), 7);
    assert!(shown.laid.problems.is_empty(), "{:?}", shown.laid.problems);
    assert!(shown.laid.strokes.len() > lines);
    assert!(said(&shown.laid, "A–A") && said(&shown.laid, "B (2:1)") && said(&shown.laid, "C–C"));
    assert!(!shown.laid.fills.is_empty());
    let drawn = shown.laid.strokes.iter().any(|s| {
        s.pen == workpiece_ops_drawing::lay::Pen::Wide
            && s.a.distance(workpiece_core::math::Point2::new(20.0, 250.0)) < 1e-9
            && s.b.distance(workpiece_core::math::Point2::new(60.0, 250.0)) < 1e-9
    });
    assert!(drawn, "what is drawn by hand is on the sheet");
    // (The sketch that draws on it lies on the sheet, with its tools.)
    assert!(shown.hand.is_some());
    // As other files: line for line.
    assert!(shown.laid.dxf().contains("\nLINE\n8\nVISIBLE\n"));
    assert!(shown.laid.svg().starts_with("<svg") && shown.laid.svg().contains("A–A"));
    // As a file: a PDF of the sheet's size.
    let pdf = shown.laid.pdf();
    assert!(pdf.starts_with(b"%PDF-1.4"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/MediaBox [0 0 1190.551 841.890]"), "the A3");
    // A view taken away takes what was made of it and stood in it.
    let without = shown.sheet.without_view("front");
    assert!(without.views.is_empty() && without.notes.is_empty());
    // And it is the part's: undone, the section and the detail are
    // gone; the model shows again when the sheet is closed.
    for _ in 0..5 {
        m.update(Message::Undo)?;
    }
    assert_eq!(View::new(&m).sheet.expect("the sheet").laid.views.len(), 3);
    assert!(m.workspace.parts["bolt"].operation("section1").is_none());
    m.update(Message::FinishEdit)?;
    assert!(View::new(&m).sheet.is_none());
    let part = &m.workspace.parts["bolt"];
    let step = part.operation("drawing1").expect("the step");
    assert_eq!(step.summary(), "A3, 50:1, 3 views");
    // Edited again, it shows again, with what stands on it.
    m.update(Message::Edit(vec!["drawing1".into()]))?;
    assert_eq!(View::new(&m).sheet.expect("the sheet").laid.notes.len(), 3);
    Ok(())
}

/// How long a sheet of the part file `WORKPIECE_SHEET_OF` takes to lay out:
/// run by hand, `WORKPIECE_SHEET_OF=part.wpc cargo test --release -p
/// workpiece-ui a_sheet_is_timed -- --ignored --nocapture`.
#[test]
#[ignore = "run by hand, with a part file"]
fn a_sheet_is_timed() -> anyhow::Result<()> {
    let path = std::env::var("WORKPIECE_SHEET_OF")?;
    let part = Part::from_text(&std::fs::read_to_string(&path)?)?;
    let mut m = Model::new(Workspace::new(BTreeMap::from([("it".to_owned(), part)])));
    m.update(Message::Open("it".into()))?;
    let started = std::time::Instant::now();
    m.update(Message::NewDrawing(Some("ISO A3, three views".into())))?;
    let shown = View::new(&m).sheet.expect("the sheet");
    eprintln!(
        "{:?}: {} lines, {} missing, problems {:?}",
        started.elapsed(),
        shown.laid.strokes.len(),
        shown.laid.missing.len(),
        shown.laid.problems
    );
    if let Ok(out) = std::env::var("WORKPIECE_SHEET_PDF") {
        // Everything that is missing, put where it fits by itself.
        m.update(Message::PreviewNotes(Some(None)))?;
        let shown = View::new(&m).sheet.expect("the sheet");
        let mut names: Vec<String> = shown
            .laid
            .notes
            .iter()
            .filter_map(|n| n.preview.as_ref().map(|p| p.0.clone()))
            .collect();
        names.dedup();
        for note in names {
            let _ = m.update(Message::PlaceNote {
                note,
                view: None,
                at: None,
            });
        }
        m.update(Message::PreviewNotes(None))?;
        let shown = View::new(&m).sheet.expect("the sheet");
        std::fs::write(out, shown.laid.pdf())?;
    }
    Ok(())
}

/// The mesh being edited, and the state of its edit session.
fn mesh(m: &Model) -> (workpiece_ops_mesh::MeshArgs, workpiece_ops_mesh::MeshState) {
    let op = m.workspace.parts["bolt"]
        .program
        .last()
        .and_then(|op| op.downcast_ref::<workpiece_ops_mesh::Mesh>())
        .expect("a mesh");
    let state = m
        .edit_state
        .as_ref()
        .and_then(|s| s.downcast_ref::<workpiece_ops_mesh::MeshState>())
        .cloned()
        .unwrap_or_default();
    (op.args.clone(), state)
}

#[test]
fn meshes_are_edited_by_their_faces_edges_and_points() -> anyhow::Result<()> {
    use workpiece_ops_mesh::{Item, Mode};
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    click(&mut m, "SubD")?;
    assert_eq!(m.tab, crate::Tab::Modeling);
    assert_eq!(m.editing, ["subd1"]);
    // A box to start from, which makes something round.
    let (args, state) = mesh(&m);
    assert_eq!((args.mesh.faces.len(), args.smooth), (6, true));
    assert!(state.picked.is_empty());
    assert!(m.viewport.shown.world.topo.lookup("subd1").is_some());
    // The face on top is picked by a click on it, and extruded.
    hover(&mut m, Vec3::new(0.2, 0.1, 1.0))?;
    assert!(matches!(mesh(&m).1.hover, Some(Item::Face(_))));
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    let (args, state) = mesh(&m);
    let [Item::Face(top)] = state.picked[..] else {
        anyhow::bail!("expected a face picked, not {:?}", state.picked);
    };
    assert!(args.mesh.normal(top).z > 0.9);
    press_key(&mut m, workpiece_core::Hotkey::key('E'))?;
    let (args, state) = mesh(&m);
    assert_eq!(args.mesh.faces.len(), 10);
    assert!(args.mesh.center(top).z > 1.5);
    assert!(state.last.is_some(), "its distance can still be typed");
    // Its gizmo moves it on: the arrow up, dragged (free of snapping:
    // Alt is held).
    m.update(Message::SuspendSnaps(true))?;
    let at = args.mesh.center(top).as_vec3();
    hover(&mut m, at + Vec3::Z * 0.2)?;
    assert!(matches!(
        m.viewport.hovered(),
        Some(Candidate::Handle { .. })
    ));
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(&mut m, at + Vec3::Z * 0.7)?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let (moved, _) = mesh(&m);
    let up = moved.mesh.center(top).z - args.mesh.center(top).z;
    assert!((up - 0.5).abs() < 0.05, "{up}");
    m.update(Message::SuspendSnaps(false))?;
    // A frame dragged open beside the mesh and over it picks what is
    // inside: in the mode of points, all of them.
    press_key(&mut m, workpiece_core::Hotkey::key('1'))?;
    assert_eq!(mesh(&m).1.mode, Mode::Vertex);
    m.update(Message::Pointer(Pointer::Moved {
        position: [5.0, 5.0],
        size: SIZE,
    }))?;
    m.update(Message::Pointer(Pointer::Press))?;
    // (A drag starts once the pointer has moved.)
    m.update(Message::Pointer(Pointer::Moved {
        position: [9.0, 9.0],
        size: SIZE,
    }))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    m.update(Message::Pointer(Pointer::Moved {
        position: [SIZE[0] - 5.0, SIZE[1] - 5.0],
        size: SIZE,
    }))?;
    assert!(mesh(&m).1.band.is_some());
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let (args, state) = mesh(&m);
    // (All that show: the top is above the view.)
    let showing = (0..args.mesh.points.len())
        .filter(|&v| {
            let [x, y] = screen(&m, args.mesh.points[v].as_vec3());
            x > 9.0 && y > 9.0 && x < SIZE[0] - 5.0 && y < SIZE[1] - 5.0
        })
        .count();
    assert!(showing >= 8);
    assert_eq!(state.picked.len(), showing);
    assert!(state.band.is_none());
    // One undo step for each: the move, the extrusion, the step.
    m.update(Message::Undo)?;
    m.update(Message::Undo)?;
    assert_eq!(mesh(&m).0.mesh.faces.len(), 6);
    m.update(Message::Redo)?;
    m.update(Message::FinishEdit)?;
    assert!(m.viewport.shown.world.topo.lookup("subd1").is_some());
    Ok(())
}

#[test]
fn a_mesh_is_stretched_by_its_arrows_and_cut_by_a_loop() -> anyhow::Result<()> {
    use workpiece_ops_mesh::Item;
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    click(&mut m, "Mesh")?;
    assert!(!mesh(&m).0.smooth);
    hover(&mut m, Vec3::new(0.2, 0.1, 1.0))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    // An arrow along x on the far side of the gizmo, half as far out as
    // the face reaches, dragged twice as far out (free of snapping).
    m.update(Message::SuspendSnaps(true))?;
    let reach = 2.0_f32.sqrt() / 2.0;
    hover(&mut m, Vec3::new(-reach, 0.0, 1.0))?;
    let Some(Candidate::Handle { name, .. }) = m.viewport.hovered().cloned() else {
        anyhow::bail!("expected an arrow under the pointer");
    };
    assert_eq!(name, "scale-0");
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(&mut m, Vec3::new(-2.0 * reach, 0.0, 1.0))?;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    let (args, state) = mesh(&m);
    let [Item::Face(top)] = state.picked[..] else {
        anyhow::bail!("expected the face still picked");
    };
    let wide = args.mesh.faces[top]
        .iter()
        .map(|&v| args.mesh.points[v].x.abs())
        .fold(0.0, f64::max);
    assert!((wide - 2.0).abs() < 0.05, "{wide}");
    // The loop cut tool: a click on an edge cuts around, across it.
    press_key(&mut m, workpiece_core::Hotkey::key('R'))?;
    hover(&mut m, Vec3::new(0.3, 1.0, -1.0))?;
    assert!(matches!(mesh(&m).1.hover, Some(Item::Edge(..))));
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    let (args, state) = mesh(&m);
    assert_eq!(args.mesh.faces.len(), 10);
    assert_eq!(state.picked.len(), 4);
    assert!(matches!(state.picked[0], Item::Edge(..)));
    m.update(Message::FinishEdit)?;
    assert!(m.viewport.shown.world.topo.lookup("mesh1").is_some());
    Ok(())
}

#[test]
fn a_part_has_views_and_sections_of_its_own_and_shows_cut_open() -> anyhow::Result<()> {
    use crate::model::display::Display;
    use workpiece_ops_drawing::{Along, CameraView, CameraViewArgs, Looking, Section, SectionArgs};
    let mut m = with_box()?;
    let add = |m: &mut Model, op: AnyOperation| {
        m.update(Message::AddOperation {
            part: "bolt".into(),
            op,
        })
    };
    // A camera view from above: the part has it by name, and the
    // camera is sent to it.
    add(
        &mut m,
        CameraView {
            id: "above".into(),
            args: CameraViewArgs {
                looking: Looking::Side(workpiece_ops_drawing::sheet::Side::Top),
                ..CameraViewArgs::default()
            },
        }
        .into(),
    )?;
    m.update(Message::FinishEdit)?;
    assert_eq!(m.viewport.shown.world.views["above"].towards, Point3::Z);
    assert_eq!(View::new(&m).viewport.views, ["above"]);
    m.update(Message::LookFrom("above".into()))?;
    let to = m.viewport.transition.expect("on its way").to;
    assert!((to.rotation * glam::Vec3::Z).distance(glam::Vec3::Z) < 1e-6);
    // One that looks as the view does takes the view once it rests.
    add(
        &mut m,
        CameraView {
            id: "mine".into(),
            args: CameraViewArgs::default(),
        }
        .into(),
    )?;
    assert!(View::new(&m).viewport.takes_view);
    m.update(Message::TakeView)?;
    assert!(!View::new(&m).viewport.takes_view);
    m.update(Message::FinishEdit)?;
    let mine = m.viewport.shown.world.views["mine"];
    assert!(mine.towards.distance(Point3::Z) < 1e-6 && mine.at.is_some());
    // A section along a plane of the origin: the model shows cut open
    // by it — drawn otherwise than whole — and whole again.
    add(
        &mut m,
        Section {
            id: "half".into(),
            args: SectionArgs {
                along: Along::Plane,
                plane: Some(workpiece_core::Reference::frame_axis("origin", Axis::X)),
                offset: workpiece_core::Measure::new(0.5),
                ..SectionArgs::default()
            },
        }
        .into(),
    )?;
    m.update(Message::FinishEdit)?;
    assert_eq!(View::new(&m).viewport.sections, ["half"]);
    let whole = View::new(&m).viewport.scene;
    let cut = Display {
        cut: Some("half".into()),
        ..Display::default()
    };
    m.update(Message::SetDisplay(cut))?;
    let opened = View::new(&m).viewport.scene;
    assert!(!std::sync::Arc::ptr_eq(
        &whole.items[0].scene,
        &opened.items[0].scene
    ));
    m.update(Message::SetDisplay(Display::default()))?;
    let again = View::new(&m).viewport.scene;
    assert!(std::sync::Arc::ptr_eq(
        &whole.items[0].scene,
        &again.items[0].scene
    ));
    Ok(())
}

#[test]
fn parts_say_properties_and_title_blocks_read_them() -> anyhow::Result<()> {
    let mut m = with_box()?;
    // What parts say of themselves is of a kind: a day is a day.
    let set = |m: &mut Model, name: &str, value: &str| {
        m.update(Message::SetProperty {
            name: name.into(),
            value: value.into(),
        })
    };
    assert!(set(&mut m, "Date", "tomorrow").is_err());
    set(&mut m, "Date", "2026-11-22")?;
    set(&mut m, "Part number", "P-0042")?;
    m.update(Message::ShowPanel(Some(crate::Panel::Properties)))?;
    let shown = View::new(&m).properties.expect("the panel");
    assert_eq!(shown.said["Part number"], "P-0042");
    assert!(shown.problems.is_empty());
    // A drawing's title block says them, and what is by default; an
    // empty sheet has no views.
    m.update(Message::NewDrawing(None))?;
    let shown = View::new(&m).sheet.expect("the sheet");
    assert!(shown.laid.views.is_empty());
    let cell = |laid: &workpiece_ops_drawing::lay::Laid, label: &str| {
        let cell = laid.cells.iter().find(|c| c.label == label).expect("it");
        cell.value.clone()
    };
    assert_eq!(cell(&shown.laid, "Identification number"), "P-0042");
    assert_eq!(cell(&shown.laid, "Date of issue"), "2026-11-22");
    assert_eq!(cell(&shown.laid, "Rev."), "A");
    assert_eq!(cell(&shown.laid, "Title"), "bolt");
    // The properties parts have are the workspace's, as its sheet
    // templates are.
    let mut properties = m.workspace.schema.properties.clone();
    properties.retain(|p| p.name != "Revision");
    m.update(Message::SetSchema { properties })?;
    let shown = View::new(&m).sheet.expect("the sheet");
    assert_eq!(cell(&shown.laid, "Rev."), "");
    let mut templates = m.workspace.sheets.templates.clone();
    templates[0].block.truncate(1);
    m.update(Message::SetTemplates { templates })?;
    m.update(Message::FinishEdit)?;
    m.update(Message::NewDrawing(Some("ISO A3, three views".into())))?;
    let shown = View::new(&m).sheet.expect("the sheet");
    assert_eq!((shown.laid.cells.len(), shown.laid.views.len()), (1, 3));
    Ok(())
}

#[test]
fn parts_keep_building_beside_the_interface_and_a_build_can_be_stopped() -> anyhow::Result<()> {
    let mut m = assembly()?;
    m.engine = workpiece_part::Engine::threaded(2);
    m.background = true;
    // Another part opened builds where the first did.
    m.update(Message::Open("nut".into()))?;
    assert!(m.background && m.engine.pool().is_some());
    // A build that is stopped shows nothing new, and says so; the next
    // change builds again.
    m.update(Message::StopBuild)?;
    assert!(!m.is_building());
    m.update(Message::Open("bolt".into()))?;
    while m.poll() {
        std::thread::yield_now();
    }
    assert!(m.viewport.shown.error.is_none());
    assert!(!m.viewport.shown.world.instances.is_empty());
    Ok(())
}

#[test]
fn a_part_is_pictured_as_it_is_and_is_no_other_part_for_it() -> anyhow::Result<()> {
    let mut m = with_box()?;
    let path = m.active_part.clone().expect("a part");
    m.update(Message::Saved(path.clone()))?;
    assert!(m.workspace.part(&path)?.preview.is_none());
    m.take_preview(&path)?;
    // A picture of the box: of the part as it is, and no change of it.
    let part = m.workspace.part(&path)?;
    let taken = part.fresh_preview().expect("a preview").clone();
    assert!(!m.is_dirty(&path));
    let (width, height, rgba) = crate::preview::pixels(&taken).expect("a picture");
    assert_eq!(
        (width, height),
        (crate::preview::WIDTH, crate::preview::HEIGHT)
    );
    let corner = &rgba[..3];
    assert!(
        rgba.as_chunks::<4>().0.iter().any(|p| &p[..3] != corner),
        "the box shows"
    );
    // It comes back from the file, and is not taken again while the
    // part stays what it was.
    let read = workpiece_part::Part::from_text(&part.to_text()?)?;
    assert_eq!(read.fresh_preview().map(|p| p.png()), Some(taken.png()));
    m.take_preview(&path)?;
    assert_eq!(
        m.workspace.part(&path)?.preview.0.as_ref().map(|p| &p.of),
        Some(&taken.of)
    );
    // Another part, once a step is added: its preview is of what it was.
    click(&mut m, "Box")?;
    pick_origin(&mut m)?;
    m.update(Message::FinishEdit)?;
    assert!(m.workspace.part(&path)?.fresh_preview().is_none());
    m.take_preview(&path)?;
    let again = m.workspace.part(&path)?.fresh_preview().expect("a new one");
    assert_ne!(again.of, taken.of);
    Ok(())
}

/// A sketch on the z = 0 plane with a dimension of every kind: distance,
/// horizontal and vertical, radius, diameter, diameter about an axis,
/// angle, and one that is driven.
fn all_dimensions(id: &str) -> AnyOperation {
    use workpiece_core::math::Point2;
    use workpiece_ops_sketch::{Kind, Shape, SketchAxis};
    let mut sketch = workpiece_ops_sketch::Sketch::new(id);
    let a = &mut sketch.args;
    a.plane = Some(workpiece_core::Reference::frame_axis("origin", Axis::Z));
    let (p0, p1, p2) = (
        a.add_point(Point2::new(0.0, 0.0)),
        a.add_point(Point2::new(4.0, 0.0)),
        a.add_point(Point2::new(4.0, 3.0)),
    );
    let (l0, l1) = (a.add(Shape::Line(p0, p1)), a.add(Shape::Line(p1, p2)));
    let centre = |a: &mut workpiece_ops_sketch::SketchArgs, x: f64, y: f64| {
        let c = a.add_point(Point2::new(x, y));
        a.add(Shape::Circle {
            center: c,
            radius: 1.5,
        })
    };
    let (c1, c2) = (centre(a, 8.0, 2.0), centre(a, 8.0, -3.0));
    a.dimension(Kind::Distance(p0, p1), 4.0, Point2::new(2.0, -1.5));
    a.dimension(
        Kind::DistanceAlong {
            a: p0,
            b: p2,
            axis: SketchAxis::U,
        },
        4.0,
        Point2::new(1.0, 4.5),
    );
    a.dimension(
        Kind::DistanceAlong {
            a: p0,
            b: p2,
            axis: SketchAxis::V,
        },
        3.0,
        Point2::new(-2.0, 1.5),
    );
    a.dimension(Kind::Size(c1), 1.5, Point2::new(9.6, 3.5));
    a.dimension_of(Kind::Size(c2), 3.0, Point2::new(9.8, -3.8), true);
    a.dimension_of(
        Kind::PointLine {
            point: p2,
            line: l0,
        },
        6.0,
        Point2::new(6.0, 0.5),
        true,
    );
    a.dimension(Kind::Angle(l0, l1), 90.0, Point2::new(5.5, 1.0));
    let driven = a.dimension(Kind::Distance(p0, p2), 5.0, Point2::new(1.0, 2.5));
    a.set_driven(driven, true);
    sketch.into()
}

/// A model of `all_dimensions`, on the Annotate tab, seen from `back`.
fn dimensioned_from(back: [f32; 3], perspective: bool) -> anyhow::Result<Model> {
    let mut m = small(Model::new(Workspace::new(BTreeMap::from([(
        "bolt".into(),
        Part::new(vec![all_dimensions("sk")]),
    )]))));
    m.update(Message::Open("bolt".into()))?;
    m.update(Message::SetTab(crate::Tab::Annotate))?;
    m.viewport.size = SIZE;
    m.viewport.camera.distance = 60.0;
    m.viewport.camera.orthographic = !perspective;
    m.update(Message::Camera(CameraMove::LookFrom(back)))?;
    m.update(Message::Tick(10.0))?;
    Ok(m)
}

#[test]
fn sketch_dimensions_are_in_their_plane_and_bounded_from_every_camera() -> anyhow::Result<()> {
    // Square on, edge-on (along an axis, along the diagonal, with the
    // plane a hair off), exactly horizontal, from behind.
    let cameras = [
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [1.0, 0.3, 0.0],
        [1.0, 0.2, 1e-4],
        [0.0, 1.0, -1e-6],
        [1.0, 1.0, 1.0],
        [0.3, -1.0, -0.2],
    ];
    for perspective in [false, true] {
        for back in cameras {
            let m = dimensioned_from(back, perspective)?;
            let vp = m.viewport.camera.view_proj(SIZE[0] / SIZE[1]);
            // How far from the middle of the view the sketch reaches.
            let reach = [(-5.0, -5.0), (-5.0, 5.0), (5.0, -5.0), (5.0, 5.0)]
                .iter()
                .map(|&(x, y)| {
                    let c = vp.project_point3(Vec3::new(x, y, 0.0));
                    c.x.abs().max(c.y.abs())
                })
                .fold(0.05f32, f32::max);
            let notes = View::new(&m).viewport.notes;
            assert_eq!(notes.len(), 8, "{back:?}");
            for note in notes {
                let points: Vec<Vec3> = note
                    .strokes
                    .iter()
                    .flat_map(|(a, b)| [*a, *b])
                    .chain(note.fills.iter().flatten().copied())
                    .chain(note.texts.iter().flat_map(|t| [t.at, t.towards]))
                    .chain(note.grip)
                    .collect();
                assert!(!note.strokes.is_empty() || !note.texts.is_empty());
                for p in points {
                    let who = format!("{} from {back:?} (perspective {perspective})", note.name);
                    assert!(
                        p.is_finite() && p.z.abs() < 1e-4,
                        "{who} leaves the plane: {p:?}"
                    );
                    let c = vp.project_point3(p);
                    // (Some room for what is a few pixels long.)
                    assert!(
                        c.is_finite() && c.x.abs().max(c.y.abs()) < reach * 3.0 + 0.2,
                        "{who} reaches {c:?}, the sketch {reach}"
                    );
                }
                // (Dragged, it moves in its sketch.)
                assert!(note.movable);
            }
        }
    }
    Ok(())
}

#[test]
fn a_sketch_dimension_in_3d_is_the_sketchers_layout_set_in_the_plane() -> anyhow::Result<()> {
    use workpiece_core::handles::{CanvasShape, CanvasView, Tone};
    use workpiece_core::math::{Point2, Point3};
    let m = dimensioned_from([0.4, -1.0, 1.2], false)?;
    let world = &m.viewport.shown.world;
    let camera = m.viewport.camera;
    let notes = View::new(&m).viewport.notes;
    let mut diameters = 0;
    for (name, note) in &world.annotations {
        let of = note.sketch.as_ref().expect("a sketch's");
        let args = of
            .geometry
            .get::<workpiece_ops_sketch::SketchArgs>()
            .expect("its sketch");
        let c = &args.constraints[&of.constraint];
        let label = Point2::new(
            (note.at - of.origin).dot(of.x),
            (note.at - of.origin).dot(of.y),
        );
        let pixel = m.pixel(note.at);
        let view = CanvasView::through(
            of.x,
            of.y,
            camera.right().as_dvec3(),
            camera.up().as_dvec3(),
            pixel,
            label,
        );
        let wanted = workpiece_ops_sketch::layout::dimension(args, c, &view, Tone::Dimension);
        let place = |p: Point2| (of.origin + of.x * p.x + of.y * p.y).as_vec3();
        let strokes: Vec<(Vec3, Vec3)> = wanted
            .iter()
            .filter_map(|s| match s {
                CanvasShape::Polyline { points, .. } => Some(points),
                _ => None,
            })
            .flat_map(|p| p.windows(2).map(|w| (place(w[0]), place(w[1]))))
            .collect();
        let shown = notes.iter().find(|n| &n.name == name).expect("drawn");
        assert_eq!(shown.strokes, strokes, "{name}");
        assert_eq!(
            shown.fills.len(),
            wanted
                .iter()
                .filter(|s| matches!(s, CanvasShape::Arrowhead { .. }))
                .count(),
            "{name}"
        );
        assert_eq!(shown.texts.len(), 1, "{name}");
        // A diameter about an axis is the whole of it: the line across
        // and the two lines to it from what is measured.
        if matches!(c.kind, workpiece_ops_sketch::Kind::PointLine { .. })
            && c.dimension.as_ref().is_some_and(|d| d.diameter)
        {
            diameters += 1;
            assert_eq!(
                shown.strokes.len(),
                3,
                "{name}: two extension lines and the line across"
            );
            let _: Point3 = of.origin;
        }
    }
    assert_eq!(diameters, 1);
    Ok(())
}

#[test]
fn what_is_behind_the_camera_is_cut_and_what_is_edge_on_stays_flat() {
    use crate::view::{clip_polygon, project};
    let camera = crate::model::Camera {
        orthographic: false,
        distance: 10.0,
        ..Default::default()
    };
    let vp = camera.view_proj(1.5);
    let eye = camera.eye();
    let behind = eye + camera.back() * 5.0;
    let front = camera.target;
    // A line from in front to behind the eye ends where the near plane
    // is, finite; one wholly behind it is gone.
    let cut = clip_polygon(vp, &[front, behind]);
    assert_eq!(cut.len(), 2);
    assert!(
        cut.iter()
            .all(|p| p.is_finite() && p.abs().max_element() < 400.0),
        "{cut:?}"
    );
    assert!(clip_polygon(vp, &[behind, behind + camera.back()]).is_empty());
    assert!(project(vp, behind).is_none() && project(vp, front).is_some());
    // A triangle through the eye's plane is cut to a polygon.
    let tri = clip_polygon(vp, &[front, behind, front + camera.right() * 3.0]);
    assert!(
        tri.len() >= 3 && tri.iter().all(|p| p.is_finite()),
        "{tri:?}"
    );
    // A triangle seen edge-on has no area, and no point of it is lost
    // or thrown out.
    let edge = [Vec3::ZERO, camera.right() * 2.0, camera.right() * 1.0];
    let flat = clip_polygon(vp, &edge);
    assert_eq!(flat.len(), 3);
    assert!(flat.iter().all(|p| p.y.abs() < 1e-5));
}

#[test]
fn what_is_selected_in_a_sketch_shows_in_its_panel_and_its_keys_win() -> anyhow::Result<()> {
    let mut m = sketching()?;
    m.update(Message::Tick(1.0))?;
    // Nothing selected: no panel; X is what the tools draw next.
    let view = View::new(&m);
    assert!(
        view.edit_dialog
            .as_ref()
            .expect("a ribbon")
            .inspector()
            .is_none()
    );
    press_key(&mut m, workpiece_core::Hotkey::key('R'))?;
    click_at(&mut m, 0.5, 0.5)?;
    click_at(&mut m, 1.5, 1.0)?;
    // What is drawn is not selected: still no panel.
    let selection = |m: &Model| {
        m.edit_state
            .as_ref()
            .and_then(|s| s.downcast_ref::<workpiece_ops_sketch::SketchState>())
            .map(|s| s.selection.clone())
            .unwrap_or_default()
    };
    assert!(selection(&m).is_empty());
    let inspector = |m: &Model| {
        View::new(m)
            .edit_dialog
            .and_then(|d| d.inspector().map(|_| ()))
            .is_some()
    };
    assert!(!inspector(&m));
    // A line of it selected: its panel, with a Construction mark whose
    // X switches the line, not the tools' mode.
    press_key(
        &mut m,
        workpiece_core::Hotkey::named(workpiece_core::Key::Escape),
    )?;
    click_at(&mut m, 1.0, 0.5)?;
    assert_eq!(selection(&m).len(), 1);
    let view = View::new(&m);
    let panel = view
        .edit_dialog
        .as_ref()
        .and_then(|d| d.inspector())
        .expect("a panel");
    let mut marks = Vec::new();
    panel.visit(&mut |w| {
        if let workpiece_core::Widget::Check(c) = w {
            marks.push(c.label.clone());
        }
    });
    assert!(marks.contains(&"Construction".to_owned()), "{marks:?}");
    press_key(&mut m, workpiece_core::Hotkey::key('X'))?;
    let drawn: Vec<bool> = sketch(&m)
        .entities
        .iter()
        .filter(|(e, _)| sketch(&m).is_line(**e) && !sketch(&m).is_axis(**e))
        .map(|(_, e)| e.construction)
        .collect();
    assert_eq!(drawn.iter().filter(|c| **c).count(), 1, "{drawn:?}");
    // D with something selected: the dimension tool, from nothing.
    assert_eq!(selection(&m).len(), 1);
    press_key(&mut m, workpiece_core::Hotkey::key('D'))?;
    assert!(selection(&m).is_empty());
    Ok(())
}

#[test]
fn a_sketch_dimension_dragged_on_the_annotate_tab_moves_in_its_sketch() -> anyhow::Result<()> {
    use workpiece_core::math::Point2;
    // The sketch's dimensions show on the Annotate tab, and are dragged
    // there as well: where their text stands is the sketch's own.
    let mut m = dimensioned_from([0.0, 0.0, 1.0], false)?;
    let notes = View::new(&m).viewport.notes;
    let first = notes
        .iter()
        .find(|n| n.name.starts_with("sk/"))
        .expect("one");
    assert!(first.movable);
    let name = first.name.clone();
    // (Dropped a little off the sketch's plane: it lands in it.)
    m.update(Message::MoveNote {
        name: name.clone(),
        at: [2.5, -2.25, 0.3],
        settled: true,
    })?;
    let part = &m.workspace.parts["bolt"];
    let sketch = part
        .operation("sk")
        .and_then(|op| op.downcast_ref::<workpiece_ops_sketch::Sketch>())
        .expect("the sketch");
    let labels: Vec<Point2> = sketch
        .args
        .constraints
        .values()
        .filter_map(|c| c.dimension.as_ref().map(|d| d.label))
        .collect();
    assert!(
        labels
            .iter()
            .any(|l| l.distance(Point2::new(2.5, -2.25)) < 1e-9),
        "{labels:?}"
    );
    assert!(!part.note_places.contains_key(&name));
    // The view shows it there: drawn by the sketch, rebuilt.
    let notes = View::new(&m).viewport.notes;
    let moved = notes.iter().find(|n| n.name == name).expect("it");
    assert!(
        moved.at.distance(glam::Vec3::new(2.5, -2.25, 0.0)) < 1e-4,
        "{}",
        moved.at
    );
    // Undone in one step.
    m.update(Message::Undo)?;
    let part = &m.workspace.parts["bolt"];
    let sketch = part
        .operation("sk")
        .and_then(|op| op.downcast_ref::<workpiece_ops_sketch::Sketch>())
        .expect("the sketch");
    assert!(
        sketch
            .args
            .constraints
            .values()
            .filter_map(|c| c.dimension.as_ref())
            .all(|d| d.label.distance(Point2::new(2.5, -2.25)) > 1e-9)
    );
    Ok(())
}

#[test]
fn a_mode_that_brings_up_a_picker_picks_into_it_at_once() -> anyhow::Result<()> {
    let mut m = with_box()?;
    // A curve through points opens picking its points.
    click(&mut m, "Curve through points")?;
    let points = m.picking.clone().expect("picking the points");
    let field = |m: &Model, path: &[usize]| -> Option<String> {
        let view = View::new(m);
        match view.edit_dialog.as_ref()?.fields().at(path)? {
            Widget::ReferenceInput(input) => Some(input.label.clone()),
            Widget::ReferenceList(list) => Some(list.label.clone()),
            _ => None,
        }
    };
    let choose = |m: &mut Model, end: &str, option: usize| -> anyhow::Result<()> {
        let view = View::new(m);
        let mut message = None;
        view.edit_dialog
            .as_ref()
            .expect("the dialog")
            .fields()
            .visit(&mut |w| {
                if let Widget::Choice(c) = w
                    && c.label.starts_with(end)
                {
                    message = c.set(option);
                }
            });
        m.update(message.expect("the end's way"))
    };
    // Its start made tangent: picking goes to what it is tangent to.
    choose(&mut m, "Start", 1)?;
    let picking = m.picking.clone().expect("picking");
    assert_ne!(picking, points);
    assert_eq!(field(&m, &picking).as_deref(), Some("Edge or axis"));
    // Picked, the focus goes on as it does (back to nothing left to
    // pick, here: the points' box does not have to be filled first).
    select(&mut m, Vec3::new(0.5, 0.0, 0.5), false)?;
    let curve = m.workspace.parts["bolt"].program[1]
        .downcast_ref::<workpiece_ops_surface::CurveThrough>()
        .expect("the curve")
        .clone();
    assert!(curve.args.start.to.is_some(), "the edge picked");
    // The end made tangent too, with the start's picker filled: the
    // end's picker is the one picked into.
    choose(&mut m, "End", 1)?;
    let picking = m.picking.clone().expect("picking");
    assert_eq!(field(&m, &picking).as_deref(), Some("Edge or axis"));
    let start = View::new(&m);
    let mut inputs = Vec::new();
    start
        .edit_dialog
        .as_ref()
        .expect("dialog")
        .fields()
        .visit(&mut |w| {
            if let Widget::ReferenceInput(i) = w {
                inputs.push(i.value.is_some());
            }
        });
    assert_eq!(inputs, [true, false]);
    Ok(())
}

/// Drags the point of the mesh being edited at `from` (in the mode of
/// points) to where `to` shows; what it snapped to.
fn drag_point(
    m: &mut Model,
    from: Vec3,
    to: Vec3,
) -> anyhow::Result<Option<workpiece_core::handles::Snap>> {
    hover(m, from)?;
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(m, to)?;
    let snapped = m.viewport.snapped;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    Ok(snapped)
}

#[test]
fn a_dragged_point_snaps_to_points_middles_and_the_grid() -> anyhow::Result<()> {
    use workpiece_core::handles::{SnapKind, SnapKinds};
    let mut m = with_box()?;
    click(&mut m, "SubD")?;
    press_key(&mut m, workpiece_core::Hotkey::key('1'))?;
    // The mesh's corner onto a corner of the box: there, exactly.
    let corner = Point3::new(1.0, -1.0, 1.0);
    let at = |m: &Model, p: Point3| {
        let (args, _) = mesh(m);
        args.mesh.points.iter().position(|q| q.distance(p) < 1e-9)
    };
    let v = at(&m, corner).expect("the corner");
    let target = Point3::new(0.5, -0.5, 0.5);
    let snapped = drag_point(&mut m, corner.as_vec3(), target.as_vec3())?;
    assert_eq!(snapped.map(|s| s.kind), Some(SnapKind::Point));
    assert!(mesh(&m).0.mesh.points[v].distance(target) < 1e-9);
    // On to the middle of an edge of the box.
    let middle = Point3::new(0.5, 0.0, 0.5);
    let snapped = drag_point(&mut m, target.as_vec3(), middle.as_vec3())?;
    assert_eq!(snapped.map(|s| s.kind), Some(SnapKind::Middle));
    assert!(mesh(&m).0.mesh.points[v].distance(middle) < 1e-9);
    // With the middles off, not to the middle: onto the edge there.
    let kinds = SnapKinds::default().with(SnapKind::Middle, false);
    m.update(Message::SetSnaps(kinds))?;
    let back = Point3::new(0.0, -0.5, 0.5);
    let snapped = drag_point(&mut m, middle.as_vec3(), back.as_vec3())?;
    assert_eq!(snapped.map(|s| s.kind), Some(SnapKind::Edge));
    // The gizmo's arrow, with only the grid on: the point goes up to a
    // whole step of z.
    let grid = SnapKinds {
        on: true,
        kinds: [false; 8],
    }
    .with(SnapKind::Grid, true);
    m.update(Message::SetSnaps(grid))?;
    let was = mesh(&m).0.mesh.points[v];
    hover(&mut m, was.as_vec3())?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    assert_eq!(mesh(&m).1.picked, [workpiece_ops_mesh::Item::Vertex(v)]);
    let arrow = was.as_vec3() + Vec3::Z * 0.2;
    hover(&mut m, arrow)?;
    assert!(matches!(
        m.viewport.hovered(),
        Some(Candidate::Handle {
            part: Some(workpiece_core::handles::GizmoPart::Translate(Axis::Z)),
            ..
        })
    ));
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    hover(&mut m, arrow + Vec3::Z * 0.37)?;
    let snapped = m.viewport.snapped;
    m.update(Message::Pointer(Pointer::DragEnd))?;
    assert_eq!(snapped.map(|s| s.kind), Some(SnapKind::Grid));
    let now = mesh(&m).0.mesh.points[v];
    let step = workpiece_core::handles::snap::grid_step(m.pixel(now));
    assert!((now.x - was.x).abs() < 1e-9 && (now.y - was.y).abs() < 1e-9);
    let steps = now.z / step;
    assert!(
        (steps - steps.round()).abs() < 1e-6,
        "{} in steps of {step}",
        now.z
    );
    assert!(now.z > was.z + 0.2);
    Ok(())
}

#[test]
fn the_gizmo_scales_all_at_once_by_its_outer_ring() -> anyhow::Result<()> {
    use workpiece_core::handles::GizmoPart;
    let mut m = model();
    m.update(Message::Open("bolt".into()))?;
    click(&mut m, "SubD")?;
    hover(&mut m, Vec3::new(0.2, 0.1, 1.0))?;
    m.update(Message::Pointer(Pointer::Click { add: false }))?;
    let (args, state) = mesh(&m);
    let [workpiece_ops_mesh::Item::Face(top)] = state.picked[..] else {
        anyhow::bail!("expected the top picked");
    };
    let middle = args.mesh.center(top);
    let reach = |a: &workpiece_ops_mesh::MeshArgs| {
        let p = a.mesh.faces[top][0];
        a.mesh.points[p].distance(middle)
    };
    // The ring, out beyond the arrows, round the middle as seen.
    let [x, y] = screen(&m, middle.as_vec3());
    let ring = 1.25 * 90.0;
    let at = |r: f32| Pointer::Moved {
        position: [x - r, y],
        size: SIZE,
    };
    m.update(Message::SuspendSnaps(true))?;
    m.update(Message::Pointer(at(ring)))?;
    assert!(matches!(
        m.viewport.hovered(),
        Some(Candidate::Handle {
            part: Some(GizmoPart::Scale),
            ..
        })
    ));
    m.update(Message::Pointer(Pointer::Press))?;
    m.update(Message::Pointer(Pointer::DragStart))?;
    m.update(Message::Pointer(at(ring * 1.5)))?;
    let factor = m.viewport.gizmo_drag.map(|d| d.factor);
    m.update(Message::Pointer(Pointer::DragEnd))?;
    assert!(factor.is_some_and(|f| (f - 1.5).abs() < 0.01), "{factor:?}");
    let (scaled, state) = mesh(&m);
    let by = reach(&scaled) / reach(&args);
    assert!((by - 1.5).abs() < 0.01, "{by}");
    // It can be typed on: the factor of all three axes.
    let last = state.last.expect("the scale, to type");
    assert!((last.by.x - 1.5).abs() < 0.01 && (last.by.z - 1.5).abs() < 0.01);
    Ok(())
}

#[test]
fn a_subd_line_is_a_curve_later_steps_take() -> anyhow::Result<()> {
    use workpiece_core::{Measure, Operation, Reference};
    use workpiece_ops_mesh::{Mesh, MeshArgs, Poly};
    use workpiece_ops_sweep::{Extrude, ExtrudeArgs};
    let mut b = workpiece_core::Standalone::new();
    // Four points, three lines: one smooth curve.
    let mut mesh = Poly::default();
    for p in [
        [0.0, 0.0, 0.0],
        [3.0, 0.0, 0.0],
        [3.0, 3.0, 0.0],
        [6.0, 3.0, 0.0],
    ] {
        mesh.points.push(Point3::from_array(p));
    }
    mesh.lines = [(0, 1), (1, 2), (2, 3)].into();
    Mesh {
        id: "subd1".into(),
        args: MeshArgs {
            mesh,
            smooth: true,
            ..MeshArgs::default()
        },
    }
    .apply(&mut b)?;
    // Swept up into a surface by a later step.
    Extrude {
        id: "wall".into(),
        args: ExtrudeArgs {
            curves: vec![Reference::topology("subd1/curves/e0")],
            direction: Some(Reference::frame_axis("origin", Axis::Z)),
            distance: Measure::new(2.0),
            surface: true,
            ..ExtrudeArgs::default()
        },
    }
    .apply(&mut b)?;
    let topo = &b.world.topo;
    let wall = topo.body_called("wall").expect("the swept surface");
    assert!(!topo.body_faces(wall).unwrap_or_default().is_empty());
    Ok(())
}
