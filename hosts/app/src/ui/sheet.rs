//! The drawing workbench: a sheet of the part on a canvas — its views
//! dragged where they should stand, what is said of the model put on
//! it and moved — with the tools that add views and dimensions, the
//! tools of a sketch to draw on it by hand, and a panel for the sheet
//! itself and its title block. What the pointer picks is picked as in
//! the 3D view: every candidate under it in one list (handles first,
//! then what stands in a view, what is missing, cutting lines, views),
//! the current one highlighted and named with "i/n"; the right button
//! goes on to the next, the left confirms it or drags it.

use eframe::egui;
use workpiece_core::handles::{CanvasEvent, CanvasEventKind, CanvasShape, CanvasView, Tone};
use workpiece_core::math::Point2;
use workpiece_ops_drawing::Sheet;
use workpiece_ops_drawing::lay::{Laid, LaidView, Placing, scale_text};
use workpiece_ops_drawing::sheet::{
    Beside, Format, OwnDimension, Projection, SCALES, SheetView, Side, Style, ViewKind,
};
use workpiece_ops_drawing::template::Content;
use workpiece_ui::Message;
use workpiece_ui::view::{
    EditDialog, Hand, Hovering, Mark, PickMode, SheetPick, SheetShown, Sheeted,
    describe_sheet_pick, sheet_candidates, sheet_marks,
};

use crate::theme::color;

/// The colour of what is not on the sheet yet, shown as what it would
/// be.
const PREVIEW: egui::Color32 = egui::Color32::from_rgb(0, 110, 220);

/// What of a view was clicked for a dimension of the sheet's own: the
/// view, the points in its own coordinates, and whether they are the
/// middle and a point of something round.
#[derive(Debug, Clone, PartialEq)]
struct Picked {
    view: String,
    points: Vec<Point2>,
    round: bool,
}

/// What a click on the sheet does.
#[derive(Debug, Clone, PartialEq)]
enum Tool {
    /// Selects, and drags what is pressed.
    Select,
    /// A section: the corners of its cutting line clicked one after the
    /// other in a view, as a chain of lines is drawn in a sketch; a
    /// double click (or Enter) ends it. The view, and the corners so
    /// far on the sheet.
    Section(Option<String>, Vec<Point2>),
    /// A detail of the view clicked, about where it is clicked.
    Detail,
    /// A dimension of the sheet's own, as in a sketch: what it is of
    /// is clicked — two points, a line, or something round — then,
    /// beside it, where it stands.
    Dimension(Option<Picked>),
}

/// What is selected on the sheet.
#[derive(Debug, Clone, PartialEq)]
enum Selected {
    View(String),
    Note {
        index: usize,
        kind: Placing,
    },
    /// The title block.
    Block,
}

/// What is dragged.
#[derive(Debug, Clone, PartialEq)]
enum Grip {
    /// A view or a note, as a whole.
    Whole(Selected),
    /// A corner of the cutting line of the section `view`.
    Corner { view: String, index: usize },
    /// The middle of what the detail `view` shows.
    Middle { view: String },
    /// How far around its middle the detail `view` shows.
    Rim { view: String },
    /// Where the extension line `k` of the sheet's note `index` starts.
    Start { index: usize, k: usize },
    /// Where the leader `k` of the sheet's words `index` ends.
    Tip { index: usize, k: usize },
    /// The end `k` of the sheet's line `index`.
    End { index: usize, k: usize },
    /// The caption of `view`: where it stood from the view's middle.
    Caption { view: String, from: [f64; 2] },
    /// The cutting line of the section `view` as a whole: its corners
    /// as they were, on the sheet.
    Line { view: String, path: Vec<Point2> },
}

/// What is being dragged: from where the pointer was on the sheet, and
/// the sheet as it was.
struct Drag {
    what: Grip,
    from: Point2,
    sheet: Sheet,
}

/// The canvas of the drawing workbench, and what it keeps between
/// frames: how the sheet is looked at, the tool in hand, what is
/// selected and dragged.
pub struct SheetPanel {
    /// Pixels to a millimetre; nothing until the sheet was fitted to
    /// the canvas once.
    zoom: Option<f32>,
    /// Where the sheet's lower left corner is, in pixels from the
    /// canvas's upper left.
    pan: egui::Vec2,
    tool: Tool,
    selected: Option<Selected>,
    drag: Option<Drag>,
    /// Whether what is said of the model and is not on the sheet shows
    /// as what it would be.
    show_missing: bool,
    /// Whether the sheet is drawn on by hand: the tools are a sketch's,
    /// and the pointer is the sketch's.
    drawing: bool,
    /// The text field open on the sheet (a drawn dimension's value):
    /// what it is for, its text, and why that was not taken.
    entry: Option<(String, String, Option<String>)>,
    /// The kind of file the sheet was asked to be written as: the
    /// application asks where to.
    pub export: Option<&'static str>,
    /// The cell of the title block that was pointed at on the sheet:
    /// its field in the dialog takes the keyboard.
    cell: Option<usize>,
    /// What is under the pointer, and which of it is current.
    hovering: Hovering,
    /// The points of the views the pointer snapped to last, latest
    /// first, while a cutting line is drawn or a corner of one dragged:
    /// what the next point snaps level with, as in the sketcher.
    remembered: Vec<Point2>,
}

impl Default for SheetPanel {
    fn default() -> Self {
        Self {
            zoom: None,
            pan: egui::Vec2::ZERO,
            tool: Tool::Select,
            selected: None,
            drag: None,
            show_missing: false,
            drawing: false,
            entry: None,
            export: None,
            cell: None,
            hovering: Hovering::default(),
            remembered: Vec::new(),
        }
    }
}

/// The colour of the candidate under the pointer.
const HOVERED: egui::Color32 = egui::Color32::from_rgb(240, 130, 20);

/// Draws the marks of the candidate under the pointer.
fn mark_pick(
    painter: &egui::Painter,
    marks: &[Mark],
    to_screen: impl Fn(Point2) -> egui::Pos2,
    zoom: f32,
) {
    let ink = egui::Stroke::new(2.0, HOVERED);
    for mark in marks {
        match mark {
            Mark::Box([lo, hi]) => {
                let r = egui::Rect::from_two_pos(to_screen(*lo), to_screen(*hi)).expand(2.0);
                painter.rect_stroke(r, 2.0, ink, egui::StrokeKind::Outside);
            }
            Mark::Lines(lines) => {
                for (a, b) in lines {
                    painter.line_segment([to_screen(*a), to_screen(*b)], ink);
                }
            }
            Mark::Handle(at) => {
                let r = egui::Rect::from_center_size(to_screen(*at), egui::vec2(10.0, 10.0));
                painter.rect_filled(r, 1.0, egui::Color32::WHITE);
                painter.rect_stroke(r, 1.0, ink, egui::StrokeKind::Inside);
            }
            Mark::Ring(at, r) => {
                painter.circle_stroke(to_screen(*at), (*r as f32 * zoom).max(5.0), ink);
            }
        }
    }
}

/// What confirming `pick` selects.
fn selected_by(pick: &SheetPick, sheet: &Sheet) -> Option<Selected> {
    let note = |index: usize, kind: Placing| Some(Selected::Note { index, kind });
    match pick {
        SheetPick::Text { index, kind } | SheetPick::Note { index, kind } => {
            note(*index, kind.clone())
        }
        SheetPick::Start { index, .. } => note(*index, Placing::Note),
        SheetPick::Tip { index, .. } => note(*index, Placing::Words),
        SheetPick::End { index, .. } => {
            note(*index, Placing::Line(sheet.lines.get(*index)?.of.clone()))
        }
        SheetPick::Corner { view, .. }
        | SheetPick::Arrow { view, .. }
        | SheetPick::CutLine { view }
        | SheetPick::Middle { view }
        | SheetPick::Rim { view }
        | SheetPick::Circle { view }
        | SheetPick::Caption { view }
        | SheetPick::View { view } => Some(Selected::View(view.clone())),
        SheetPick::Cell { .. } => Some(Selected::Block),
        SheetPick::Missing { .. }
        | SheetPick::Point { .. }
        | SheetPick::Round { .. }
        | SheetPick::Edge { .. } => None,
    }
}

/// What a drag that starts on `pick` drags.
fn grip_of(pick: &SheetPick, shown: &SheetShown) -> Option<Grip> {
    let laid = &shown.laid;
    Some(match pick {
        SheetPick::Text { index, kind } | SheetPick::Note { index, kind } => {
            Grip::Whole(Selected::Note {
                index: *index,
                kind: kind.clone(),
            })
        }
        SheetPick::Start { index, k } => Grip::Start {
            index: *index,
            k: *k,
        },
        SheetPick::Corner { view, index } | SheetPick::Arrow { view, index } => Grip::Corner {
            view: view.clone(),
            index: *index,
        },
        SheetPick::Tip { index, k } => Grip::Tip {
            index: *index,
            k: *k,
        },
        SheetPick::End { index, k } => Grip::End {
            index: *index,
            k: *k,
        },
        SheetPick::Middle { view } | SheetPick::Circle { view } => {
            Grip::Middle { view: view.clone() }
        }
        SheetPick::Rim { view } => Grip::Rim { view: view.clone() },
        SheetPick::Caption { view } => {
            let laid_view = laid_view(laid, view)?;
            let [lo, hi] = laid_view.caption?;
            let from = shown
                .sheet
                .view(view)
                .and_then(|v| v.caption_at)
                .unwrap_or(((lo + hi) / 2.0 - laid_view.anchor).to_array());
            Grip::Caption {
                view: view.clone(),
                from,
            }
        }
        SheetPick::CutLine { view } if shown.movable.contains(view) => {
            let cut = laid.cuts.iter().find(|c| c.view == *view)?;
            Grip::Line {
                view: view.clone(),
                path: cut.path.clone(),
            }
        }
        SheetPick::View { view } => Grip::Whole(Selected::View(view.clone())),
        _ => return None,
    })
}

/// The message that makes `sheet` the shown sheet.
fn set(shown: &SheetShown, sheet: Sheet, settled: bool) -> Message {
    let set = shown.set(sheet);
    // (A drag that goes on is one change.)
    match settled {
        true => Message::Batch(vec![set, Message::EndEdit]),
        false => set,
    }
}

/// Where a new view about as large as `half` stands, starting at
/// `at` and going on by `step` until it is clear of the views there
/// are.
fn clear_of(laid: &Laid, mut at: Point2, half: Point2, step: Point2) -> Point2 {
    let step = step.normalize_or_zero() * 25.0;
    for _ in 0..40 {
        let taken = laid.views.iter().any(|v| {
            (at.x - v.centre.x).abs() < half.x + v.half.x + 12.0
                && (at.y - v.centre.y).abs() < half.y + v.half.y + 12.0
        });
        if !taken || step == Point2::ZERO {
            break;
        }
        at += step;
    }
    at
}

/// The scale twice `scale` or more that drawings use: what a detail is
/// drawn at.
fn enlarged(scale: [u32; 2]) -> [u32; 2] {
    let now = f64::from(scale[0]) / f64::from(scale[1]);
    SCALES
        .iter()
        .find(|s| f64::from(s[0]) / f64::from(s[1]) >= now * 2.0 - 1e-9)
        .copied()
        .unwrap_or([100, 1])
}

/// The point `p` of the sheet in the own coordinates of `view`.
fn own_of(view: &LaidView, p: Point2) -> Point2 {
    (p - view.anchor) / view.scale + view.origin
}

/// The point `p` of the own coordinates of `view` on the sheet.
fn sheet_of(view: &LaidView, p: Point2) -> Point2 {
    view.anchor + (p - view.origin) * view.scale
}

/// The view of `laid` called `id`.
fn laid_view<'a>(laid: &'a Laid, id: &str) -> Option<&'a LaidView> {
    laid.views.iter().find(|v| v.id == id)
}

/// How far `p` is from the line from `a` to `b`, and the point of the
/// line nearest to it.
fn off_line(p: Point2, a: Point2, b: Point2) -> (f64, Point2) {
    let d = b - a;
    let t = if d.length_squared() > 0.0 {
        ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let foot = a + d * t;
    (foot.distance(p), foot)
}

/// `p` snapped as the sketcher snaps (its own code): to the ends of
/// the lines of the views (of the view `only`, if it is said) and the
/// middles of what is round in them, where two lines cross, the middle
/// of a line, onto a line or a circle, or level with `last` across or
/// up; `reach` is how near, in millimetres. Where it lands, and whether
/// that is on something of the model.
fn snap(
    laid: &Laid,
    p: Point2,
    reach: f64,
    only: Option<&str>,
    last: Option<Point2>,
    remembered: &[Point2],
) -> workpiece_ops_sketch::tools::Snapped {
    let views = || {
        laid.views
            .iter()
            .filter(|v| !v.detail && only.is_none_or(|id| id == v.id))
    };
    // (Only what is near: the sketcher looks at every line it has.)
    let near = |a: Point2, b: Point2| off_line(p, a, b).0 <= reach * 3.0;
    let lines: Vec<(Point2, Point2)> = views()
        .flat_map(|v| v.lines.iter().copied())
        .filter(|(a, b)| near(*a, *b))
        .collect();
    let rounds: Vec<(Point2, f64)> = views()
        .flat_map(|v| v.rounds.iter().copied())
        .filter(|(c, r)| (c.distance(p) - r).abs() <= reach * 3.0 || c.distance(p) <= reach * 3.0)
        .collect();
    let points: Vec<Point2> = views()
        .flat_map(|v| v.snaps.iter().copied())
        .filter(|s| s.distance(p) <= reach * 3.0)
        .collect();
    workpiece_ops_sketch::tools::Snappable::new(&lines, &rounds, &points).snap(
        p,
        &snap_view(reach),
        last,
        remembered,
    )
}

/// The sheet as the sketcher's snapping sees it: `reach` is how near it
/// snaps, in millimetres of the sheet.
fn snap_view(reach: f64) -> CanvasView {
    CanvasView {
        pixel: reach / SNAP_PX,
        right: Point2::X,
        up: Point2::Y,
        middle: Point2::ZERO,
    }
}

/// The text of the note `index` (one of the sheet's own, with `own`)
/// moved to `at` (from its view's middle), snapped as the sheet's
/// spacing has it: along its dimension line to the line's middle and
/// ends, and its line's height to the stacking pitch off the outline
/// and off the other lengths of its view.
fn snap_text(
    laid: &Laid,
    index: usize,
    kind: &Placing,
    at: &mut [f64; 2],
    sheet: &Sheet,
    reach: f64,
) {
    let Some(note) = laid
        .notes
        .iter()
        .find(|n| n.preview.is_none() && n.kind == *kind && n.index == index)
    else {
        return;
    };
    let (Some(linear), Some(view)) = (note.linear, laid_view(laid, &note.view)) else {
        return;
    };
    let others: Vec<_> = laid
        .notes
        .iter()
        .filter(|n| n.preview.is_none() && n.view == note.view)
        .filter(|n| !(n.kind == *kind && n.index == index))
        .filter_map(|n| n.linear)
        .collect();
    let free = view.anchor + Point2::from_array(*at);
    let snapped = linear.text_at(free, sheet.spacing.pitch, &others, reach);
    *at = (snapped - view.anchor).to_array();
}

/// How many pixels near the sketcher snaps (its reach, as it has it).
const SNAP_PX: f64 = 8.0;

/// Where the cutting line of the section `view` of `sheet` runs on the
/// sheet, in the view it is drawn in; or the middle and the reach of
/// what a detail shows.
fn anchors(shown: &SheetShown, laid: &Laid, view: &SheetView) -> Vec<(Grip, Point2)> {
    match &view.kind {
        ViewKind::Section { .. } => {
            // (Only a line the part keeps as a sketch of nothing else
            // is dragged here.)
            if !shown.movable.contains(&view.id) {
                return Vec::new();
            }
            let Some(cut) = laid.cuts.iter().find(|c| c.view == view.id) else {
                return Vec::new();
            };
            cut.path
                .iter()
                .enumerate()
                .map(|(index, p)| {
                    let grip = Grip::Corner {
                        view: view.id.clone(),
                        index,
                    };
                    (grip, *p)
                })
                .collect()
        }
        ViewKind::Detail {
            of, centre, radius, ..
        } => {
            let Some(parent) = laid_view(laid, of) else {
                return Vec::new();
            };
            let middle = sheet_of(parent, Point2::from_array(*centre));
            let rim = middle - Point2::new(1.0, 1.0).normalize() * (radius * parent.scale);
            let id = || view.id.clone();
            vec![
                (Grip::Middle { view: id() }, middle),
                (Grip::Rim { view: id() }, rim),
            ]
        }
        _ => Vec::new(),
    }
}

impl SheetPanel {
    /// Whether a tool of the workbench is in hand (not selecting): Escape
    /// and Enter are its own then.
    pub fn holds_keys(&self) -> bool {
        self.tool != Tool::Select && !self.drawing
    }

    /// The tools of the workbench, in place of the ribbon's. While the
    /// sheet is drawn on by hand, the tools of a sketch (`dialog`'s).
    pub fn tools(
        &mut self,
        ui: &mut egui::Ui,
        shown: &SheetShown,
        dialog: Option<&EditDialog>,
        out: &mut Vec<Message>,
        choose_file: &mut Option<Vec<usize>>,
    ) {
        let sheet = &shown.sheet;
        let tool = |ui: &mut egui::Ui, on: bool, label: &str, hint: &str| {
            let text = egui::RichText::new(label).size(12.0).color(if on {
                color::ACCENT
            } else {
                color::TEXT
            });
            ui.add_sized([0.0, 30.0], egui::Button::new(text).selected(on))
                .on_hover_text(hint)
                .clicked()
        };
        if self.drawing
            && let Some(dialog) = dialog
        {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if tool(
                    ui,
                    false,
                    "< Sheet",
                    "Back to the sheet's own tools: views, sections, dimensions",
                ) {
                    self.drawing = false;
                }
                ui.separator();
                super::edit_dialog::show_groups(ui, dialog, out, choose_file);
            });
            return;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.set_min_height(52.0);
            if tool(ui, false, "✔ Done", "Back to the model (the drawing stays a step of the part)") {
                out.push(Message::FinishEdit);
            }
            ui.separator();
            // A view of the model by itself, from a side.
            ui.menu_button(egui::RichText::new("+ View").size(12.0), |ui| {
                for side in Side::ALL {
                    if ui.button(side.label()).clicked() {
                        let mut new = sheet.clone();
                        let [w, h] = new.size();
                        new.views.push(SheetView {
                            id: new.fresh_view("view"),
                            kind: ViewKind::Base { side },
                            at: [w * 0.5, h * 0.6],
                            scale: None,
                            style: Style::HiddenRemoved,
                            bodies: Vec::new(),
                            cut_only: false,
                            half: None,
                            section: None,
                            caption_at: None,
                        });
                        out.push(set(shown, new, true));
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("A view of the model from a side, to drag where it should stand");
            // What the part itself says of how it is looked at and cut:
            // its camera views and its sections.
            if !shown.views.is_empty() || !shown.sections.is_empty() {
                ui.menu_button(egui::RichText::new("+ Of the part").size(12.0), |ui| {
                    for name in &shown.views {
                        if ui.button(format!("Camera view {name}")).clicked() {
                            let mut new = sheet.clone();
                            let [w, h] = new.size();
                            new.views.push(SheetView {
                                id: new.fresh_view("view"),
                                kind: ViewKind::Named { view: name.clone() },
                                at: [w * 0.5, h * 0.6],
                                scale: None,
                                style: Style::HiddenRemoved,
                                bodies: Vec::new(),
                                cut_only: false,
                                half: None,
                                section: None,
                                caption_at: None,
                            });
                            out.push(set(shown, new, true));
                            ui.close();
                        }
                    }
                    // (Its cutting line shows in the selected view, if
                    // one is.)
                    let of = match &self.selected {
                        Some(Selected::View(id)) => Some(id.clone()),
                        _ => None,
                    };
                    for name in &shown.sections {
                        if ui.button(format!("Section {name}")).clicked() {
                            let mut new = sheet.clone();
                            let [w, h] = new.size();
                            new.views.push(SheetView {
                                id: new.fresh_view("section"),
                                kind: ViewKind::Section {
                                    of: of.clone(),
                                    section: name.clone(),
                                    letter: new.fresh_letter(),
                                    turn: 0.0,
                                },
                                at: [w * 0.6, h * 0.5],
                                scale: None,
                                style: Style::HiddenRemoved,
                                bodies: Vec::new(),
                                cut_only: false,
                                half: None,
                                section: None,
                                caption_at: None,
                            });
                            out.push(set(shown, new, true));
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("A view as a camera view of the part looks at it, or cut open as a section of the part cuts it (its line shows in the selected view)");
            }
            // A view projected from the selected one.
            let of = match &self.selected {
                Some(Selected::View(id)) => shown.laid.views.iter().find(|v| v.id == *id && !v.detail),
                _ => None,
            };
            ui.add_enabled_ui(of.is_some(), |ui| {
                ui.menu_button(egui::RichText::new("Projected").size(12.0), |ui| {
                    let Some(of) = of else {
                        return;
                    };
                    let reach = of.half.x.max(of.half.y) * 2.0 + 30.0;
                    for (beside, label) in [
                        (Beside::Right, "To the right"),
                        (Beside::Left, "To the left"),
                        (Beside::Above, "Above"),
                        (Beside::Below, "Below"),
                    ] {
                        if ui.button(label).clicked() {
                            let step = match beside {
                                Beside::Right => Point2::new(reach, 0.0),
                                Beside::Left => Point2::new(-reach, 0.0),
                                Beside::Above => Point2::new(0.0, reach),
                                Beside::Below => Point2::new(0.0, -reach),
                            };
                            let mut new = sheet.clone();
                            new.views.push(SheetView {
                                id: new.fresh_view("view"),
                                kind: ViewKind::Projected {
                                    of: of.id.clone(),
                                    beside,
                                },
                                at: clear_of(&shown.laid, of.anchor + step, of.half, step)
                                    .to_array(),
                                scale: None,
                                style: Style::HiddenRemoved,
                                bodies: Vec::new(),
                                cut_only: false,
                                half: None,
                                section: None,
                                caption_at: None,
                            });
                            out.push(set(shown, new, true));
                            ui.close();
                        }
                    }
                    ui.separator();
                    // Seen along the sheet, askew: what lies at an
                    // angle in the view is seen true.
                    if ui
                        .button("Auxiliary (at an angle)")
                        .on_hover_text("The model seen along a direction of the selected view: its angle is set in the dialog at the right")
                        .clicked()
                    {
                        let step = Point2::new(1.0, 1.0).normalize() * reach;
                        let mut new = sheet.clone();
                        new.views.push(SheetView {
                            id: new.fresh_view("view"),
                            kind: ViewKind::Auxiliary {
                                of: of.id.clone(),
                                angle: 225.0,
                            },
                            at: clear_of(&shown.laid, of.anchor + step, of.half, step).to_array(),
                            scale: None,
                            style: Style::HiddenRemoved,
                            bodies: Vec::new(),
                            cut_only: false,
                            half: None,
                            section: None,
                            caption_at: None,
                        });
                        out.push(set(shown, new, true));
                        ui.close();
                    }
                })
                .response
                .on_hover_text("A view beside the selected one, in line with it: the model turned a quarter (select a view first)");
            });
            let mut pick = |ui: &mut egui::Ui, this: Tool, label: &str, hint: &str| {
                let on = std::mem::discriminant(&self.tool) == std::mem::discriminant(&this);
                if tool(ui, on, label, hint) {
                    self.tool = if on { Tool::Select } else { this };
                }
            };
            pick(ui, Tool::Section(None, Vec::new()), "✂ Section", "Draw the cutting line in a view, corner by corner as a chain of lines in a sketch (it snaps as the sketcher does: to the view's ends, middles, crossings and centres, onto its lines, and level with the corner before); a double click, Enter or Escape ends it. One piece cuts straight through; several make an offset section. The cut is seen from the left of the line as it is drawn");
            pick(ui, Tool::Detail, "🔍 Detail", "Click in a view what to show larger");
            pick(ui, Tool::Dimension(None), "↔ Dimension", "A dimension of the sheet's own, as in a sketch: click what it is of — two points, a line, or a circle — then click beside it where it stands (beside two points: up and down; above or below: across; Shift: straight between them)");
            if tool(ui, false, "Draw", "Draw on the sheet by hand with the tools of a sketch: lines, circles, arcs, splines, lettering, with their constraints and dimensions; they snap to the views") {
                self.drawing = true;
                self.tool = Tool::Select;
            }
            ui.separator();
            let missing = shown.laid.missing.len();
            // (Centre lines and datum traces are offered whether or not
            // a dimension is missing.)
            let label = if self.show_missing {
                format!("👁 Hide {missing} missing")
            } else if missing == 0 {
                "✔ All dimensions placed · 👁 Show lines".to_owned()
            } else {
                format!("👁 Show {missing} missing")
            };
            let hint = "What is said of the model — its dimensions, datums and tolerances — and is not on the sheet yet, and the centre lines, datum planes, axes and points each view could show, in blue as what they would be: in the selected view, or (with none selected) in every view that sees them. Click one to put it on the sheet (the right button goes on to the next under the pointer)";
            if tool(ui, self.show_missing, &label, hint) {
                self.show_missing = !self.show_missing;
            }
            ui.separator();
            ui.menu_button(egui::RichText::new("⎙ Export").size(12.0), |ui| {
                for (label, kind) in [("PDF…", "pdf"), ("DXF…", "dxf"), ("SVG…", "svg")] {
                    if ui.button(label).clicked() {
                        self.export = Some(kind);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Write the sheet as a file, true to size");
        });
    }

    /// The canvas: the sheet, looked at closer with the wheel and moved
    /// with the right or middle button.
    pub fn show(&mut self, ui: &mut egui::Ui, shown: &SheetShown, out: &mut Vec<Message>) {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let laid = &shown.laid;
        let [w, h] = laid.size.map(|s| s as f32);
        // Fitted to the canvas the first time.
        let zoom = *self.zoom.get_or_insert_with(|| {
            let zoom = ((rect.width() - 40.0) / w)
                .min((rect.height() - 40.0) / h)
                .max(0.1);
            self.pan = egui::vec2(
                (rect.width() - w * zoom) / 2.0,
                (rect.height() + h * zoom) / 2.0,
            );
            zoom
        });
        let pan = self.pan;
        let to_screen =
            move |p: Point2| rect.min + pan + egui::vec2(p.x as f32 * zoom, -(p.y as f32) * zoom);
        let to_sheet = move |p: egui::Pos2| {
            let d = p - rect.min - pan;
            Point2::new(f64::from(d.x / zoom), f64::from(-d.y / zoom))
        };
        // The wheel looks closer where the pointer is; the right or the
        // middle button moves the sheet.
        if let Some(pointer) = response.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let factor = (scroll * 0.004).exp();
                let new = (zoom * factor).clamp(0.2, 60.0);
                let at = pointer - rect.min;
                self.pan = at - (at - self.pan) * (new / zoom);
                self.zoom = Some(new);
            }
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            self.pan += response.drag_delta();
        }
        // What is missing shows in the selected view, or in all.
        let wanted = self.show_missing.then(|| match &self.selected {
            Some(Selected::View(id)) => Some(id.clone()),
            _ => None,
        });
        if wanted != shown.previews {
            out.push(Message::PreviewNotes(wanted));
        }
        paint(ui, rect, laid, &to_screen, zoom);
        let painter = ui.painter_at(rect);
        let pointer = response.hover_pos().map(to_sheet);
        let reach = f64::from(9.0 / zoom);
        if self.drawing
            && let Some(hand) = &shown.hand
        {
            self.draw_by_hand(ui, &response, shown, hand, pointer, &to_screen, zoom, out);
            return;
        }
        // What is selected, outlined.
        let outline = |lo: Point2, hi: Point2, colour: egui::Color32| {
            let r = egui::Rect::from_two_pos(to_screen(lo), to_screen(hi)).expand(3.0);
            painter.rect_stroke(
                r,
                2.0,
                egui::Stroke::new(1.5, colour),
                egui::StrokeKind::Outside,
            );
        };
        let mut grips: Vec<(Grip, Point2)> = Vec::new();
        match &self.selected {
            Some(Selected::View(id)) => {
                if let Some(v) = laid_view(laid, id) {
                    outline(v.centre - v.half, v.centre + v.half, color::ACCENT);
                }
                // Where it cuts, or what it shows larger: to be dragged.
                if let Some(view) = shown.sheet.view(id) {
                    grips = anchors(shown, laid, view);
                }
            }
            Some(Selected::Note { index, kind }) => {
                if let Some(n) = laid
                    .notes
                    .iter()
                    .find(|n| n.preview.is_none() && n.index == *index && n.kind == *kind)
                {
                    if n.path.is_empty() {
                        outline(n.rect[0], n.rect[1], color::ACCENT);
                    } else {
                        // (A line: itself, marked.)
                        for (a, b) in &n.path {
                            painter.line_segment(
                                [to_screen(*a), to_screen(*b)],
                                egui::Stroke::new(2.5, color::ACCENT),
                            );
                        }
                    }
                    // Where the leader of words ends; the ends of a
                    // line: to be dragged.
                    if n.kind == Placing::Words {
                        for (k, tip) in n.tips.iter().enumerate() {
                            grips.push((Grip::Tip { index: *index, k }, *tip));
                        }
                    }
                    if let (Placing::Line(_), [(a, b)]) = (&n.kind, &n.path[..]) {
                        grips.push((
                            Grip::End {
                                index: *index,
                                k: 0,
                            },
                            *a,
                        ));
                        grips.push((
                            Grip::End {
                                index: *index,
                                k: 1,
                            },
                            *b,
                        ));
                    }
                    // Where its extension lines start: to be dragged.
                    if let (Placing::Note, Some(linear), Some(placed)) =
                        (kind, n.linear, shown.sheet.notes.get(*index))
                    {
                        let gap = shown.sheet.spacing.gap;
                        for k in 0..2 {
                            let start = placed.starts[k].unwrap_or(gap);
                            let end = linear.from[k].distance(linear.line[k]);
                            let at = linear.from[k] + linear.ways[k] * start.min(end);
                            grips.push((Grip::Start { index: *index, k }, at));
                        }
                    }
                }
            }
            Some(Selected::Block) => {
                // (All of its cells.)
                let corners = laid.cells.iter().flat_map(|c| c.rect);
                let (lo, hi) = corners.fold(
                    (
                        Point2::new(f64::MAX, f64::MAX),
                        Point2::new(f64::MIN, f64::MIN),
                    ),
                    |(lo, hi), p| {
                        (
                            Point2::new(lo.x.min(p.x), lo.y.min(p.y)),
                            Point2::new(hi.x.max(p.x), hi.y.max(p.y)),
                        )
                    },
                );
                if lo.x <= hi.x {
                    outline(lo, hi, color::ACCENT);
                }
            }
            None => {}
        }
        for (_, at) in &grips {
            let r = egui::Rect::from_center_size(to_screen(*at), egui::vec2(8.0, 8.0));
            painter.rect_filled(r, 1.0, egui::Color32::WHITE);
            painter.rect_stroke(
                r,
                1.0,
                egui::Stroke::new(1.5, color::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        // Everything under the pointer that the tool in hand picks, the
        // one to pick first first (as in the 3D view): the current one
        // highlighted and named with how manyth it is; the right button
        // goes on to the next, the left confirms it (or drags it).
        let of = Sheeted {
            sheet: &shown.sheet,
            laid,
            movable: &shown.movable,
        };
        // (While the button is down, what was under it when it went
        // down stays: that is what a drag grabs.)
        if self.drag.is_none() && !ui.input(|i| i.pointer.primary_down()) {
            let mode = match &self.tool {
                Tool::Select => Some(PickMode::Select {
                    missing: self.show_missing,
                }),
                Tool::Section(None, _) | Tool::Detail => Some(PickMode::Views),
                Tool::Section(Some(_), _) => None,
                Tool::Dimension(picked) => {
                    let ready = picked
                        .as_ref()
                        .is_some_and(|p| p.round || p.points.len() >= 2);
                    (!ready).then(|| PickMode::Measure {
                        only: picked.as_ref().map(|p| p.view.as_str()),
                    })
                }
            };
            let list = match (mode, pointer) {
                (Some(mode), Some(p)) => sheet_candidates(&of, mode, p, reach),
                _ => Vec::new(),
            };
            self.hovering.update(pointer, list, f64::from(3.0 / zoom));
        }
        if response.clicked_by(egui::PointerButton::Secondary) {
            self.hovering.cycle();
        }
        let current = self.hovering.current().cloned();
        if self.drag.is_none()
            && let (Some(pick), Some(at)) = (&current, response.hover_pos())
        {
            mark_pick(&painter, &sheet_marks(&of, pick), to_screen, zoom);
            let text = describe_sheet_pick(&of, pick);
            let n = self.hovering.list.len();
            let text = match n > 1 {
                true => format!("{text}   {}/{n}", self.hovering.current + 1),
                false => text,
            };
            // (Over the dialogs, as a tooltip is.)
            let top = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("sheet candidate"),
            ));
            let galley = top.layout_no_wrap(text, egui::FontId::proportional(12.0), color::TEXT);
            let at = at + egui::vec2(14.0, 14.0);
            let bg = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(6.0, 3.0));
            top.rect_filled(bg, 4.0, color::PANEL.gamma_multiply(0.92));
            top.rect_stroke(
                bg,
                4.0,
                egui::Stroke::new(1.0, color::BORDER),
                egui::StrokeKind::Inside,
            );
            top.galley(at, galley, color::TEXT);
        }
        // The view nearest to a point: what a cutting line that starts
        // beside a view is of.
        let near_view = |p: Point2| {
            laid.views
                .iter()
                .filter(|v| !v.detail)
                .map(|v| {
                    let off = ((p - v.centre).abs() - v.half).max(Point2::ZERO).length();
                    (off, v)
                })
                .filter(|(off, _)| *off <= 40.0)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, v)| v)
        };
        // (Points are remembered only while a cutting line is drawn,
        // or a corner of one or a detail's middle dragged.)
        if self.drag.is_none() && !matches!(self.tool, Tool::Section(..)) {
            self.remembered.clear();
        }
        match self.tool.clone() {
            Tool::Select => {
                if response.clicked() {
                    match &current {
                        // Something that is missing: it is put on the
                        // sheet where it shows.
                        Some(SheetPick::Missing { index }) => {
                            if let Some(note) = laid.notes.get(*index)
                                && let Some((name, at)) = &note.preview
                            {
                                out.push(match &note.kind {
                                    Placing::Line(of) => Message::PlaceLine {
                                        view: note.view.clone(),
                                        of: of.clone(),
                                    },
                                    _ => Message::PlaceNote {
                                        note: name.clone(),
                                        view: Some(note.view.clone()),
                                        at: Some(*at),
                                    },
                                });
                            }
                        }
                        // A cell of the title block: what it says is
                        // typed in the dialog.
                        Some(SheetPick::Cell { index }) => {
                            self.cell = Some(*index);
                            self.selected = Some(Selected::Block);
                        }
                        Some(pick) => self.selected = selected_by(pick, &shown.sheet),
                        None => self.selected = None,
                    }
                }
                self.select(
                    &response,
                    shown,
                    pointer,
                    reach,
                    current.as_ref(),
                    (&painter, &to_screen),
                    out,
                );
            }
            Tool::Section(view, mut points) => {
                let only = view.as_deref();
                // Snapped as the sketcher snaps: also level with the
                // points hovered before, remembered as it goes.
                let last = points.last().copied();
                let snapped = pointer.map(|p| snap(laid, p, reach, only, last, &self.remembered));
                // The line so far, and on to the pointer.
                let ink = egui::Stroke::new(1.5, color::ACCENT);
                let mut shown_points: Vec<egui::Pos2> =
                    points.iter().map(|p| to_screen(*p)).collect();
                if let Some(snapped) = &snapped {
                    shown_points.push(to_screen(snapped.at));
                    snapped.remember(&mut self.remembered);
                    hand_shapes(&painter, &snapped.cues(&snap_view(reach)), &to_screen);
                }
                let snapped = snapped.map(|s| (s.at, s.on));
                if shown_points.len() >= 2 {
                    painter.add(egui::Shape::dashed_line(&shown_points, ink, 8.0, 3.0));
                }
                for p in &points {
                    painter.circle_filled(to_screen(*p), 3.0, color::ACCENT);
                }
                // Enter, or Escape as in the sketcher, ends the chain
                // with the corners it has (Escape with fewer than two
                // drops it).
                let ends = ui
                    .input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
                let mut done = ends;
                if (response.clicked() || response.double_clicked())
                    && let Some((at, _)) = snapped
                {
                    // (The view it is drawn in: the one picked under
                    // the pointer, else the nearest.)
                    let picked = match &current {
                        Some(SheetPick::View { view }) => Some(view.clone()),
                        _ => None,
                    };
                    let view = view
                        .clone()
                        .or(picked)
                        .or_else(|| near_view(at).map(|v| v.id.clone()));
                    if let Some(view) = view {
                        if points.last().is_none_or(|last| last.distance(at) > 1e-6) {
                            points.push(at);
                        }
                        done |= response.double_clicked();
                        self.tool = Tool::Section(Some(view), points.clone());
                    }
                }
                if done
                    && points.len() >= 2
                    && let Tool::Section(Some(id), _) = &self.tool
                    && let Some(view) = laid_view(laid, id)
                {
                    let line: Vec<[f64; 2]> =
                        points.iter().map(|p| own_of(view, *p).to_array()).collect();
                    // Where it stands: on the side it is looked at
                    // from — where the eye is, in first angle the view
                    // stands opposite.
                    let way = (points[1] - points[0]).normalize_or_zero();
                    let first = shown.sheet.projection == Projection::FirstAngle;
                    let side = way.perp() * if first { -1.0 } else { 1.0 };
                    let reach = view.half.x.max(view.half.y) * 2.0 + 35.0;
                    let step = side * reach;
                    // The part gets the line as a sketch and a section
                    // along it; the sheet, a view of that section.
                    out.push(Message::CutSection {
                        view: view.id.clone(),
                        line,
                        at: clear_of(laid, view.anchor + step, view.half, step).to_array(),
                    });
                    self.selected = None;
                    self.tool = Tool::Select;
                }
            }
            Tool::Detail => {
                if let Some(p) = pointer.filter(|_| response.clicked())
                    && let Some(SheetPick::View { view }) = &current
                    && let Some(view) = laid_view(laid, view).filter(|v| !v.detail)
                {
                    let at = snap(laid, p, reach, Some(&view.id), None, &[]).at;
                    let mut new = shown.sheet.clone();
                    let scale = enlarged(
                        new.view(&view.id)
                            .and_then(|v| v.scale)
                            .unwrap_or(new.scale),
                    );
                    new.views.push(SheetView {
                        id: new.fresh_view("detail"),
                        kind: ViewKind::Detail {
                            of: view.id.clone(),
                            centre: own_of(view, at).to_array(),
                            radius: (view.half.x.min(view.half.y) * 0.3).max(4.0) / view.scale,
                            letter: new.fresh_letter(),
                            letter_at: None,
                        },
                        at: clear_of(
                            laid,
                            view.centre
                                + Point2::new(view.half.x * 2.0 + 40.0, -view.half.y - 30.0),
                            Point2::splat(20.0),
                            Point2::new(1.0, -0.4),
                        )
                        .to_array(),
                        scale: Some(scale),
                        style: Style::HiddenRemoved,
                        bodies: Vec::new(),
                        cut_only: false,
                        half: None,
                        section: None,
                        caption_at: None,
                    });
                    self.selected = new.views.last().map(|v| Selected::View(v.id.clone()));
                    out.push(set(shown, new, true));
                    self.tool = Tool::Select;
                }
            }
            Tool::Dimension(picked) => {
                self.dimension(
                    ui,
                    &response,
                    shown,
                    picked,
                    pointer,
                    current.as_ref(),
                    &to_screen,
                    out,
                );
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.tool = Tool::Select;
        }
        // What cannot be drawn is said at the bottom.
        if let Some(problem) = laid.problems.first() {
            painter.text(
                rect.left_bottom() + egui::vec2(12.0, -12.0),
                egui::Align2::LEFT_BOTTOM,
                problem,
                egui::FontId::proportional(13.0),
                color::ERROR,
            );
        }
    }

    /// The dimension tool, as a sketch's: clicks on what it is of add
    /// to what is picked, a click beside it places the dimension.
    #[allow(clippy::too_many_arguments)]
    fn dimension(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        shown: &SheetShown,
        picked: Option<Picked>,
        pointer: Option<Point2>,
        current: Option<&SheetPick>,
        to_screen: &dyn Fn(Point2) -> egui::Pos2,
        out: &mut Vec<Message>,
    ) {
        let laid = &shown.laid;
        let painter = ui.painter_at(response.rect);
        let ink = egui::Stroke::new(1.5, color::ACCENT);
        // What is under the pointer: a point first, then something
        // round by its outline, then a line (the current candidate).
        enum Under<'a> {
            Point(&'a LaidView, Point2),
            Round(&'a LaidView, Point2, f64),
            Line(&'a LaidView, Point2, Point2),
        }
        let under = current.and_then(|pick| match pick {
            SheetPick::Point { view, at } => laid_view(laid, view).map(|v| Under::Point(v, *at)),
            SheetPick::Round {
                view,
                centre,
                radius,
            } => laid_view(laid, view).map(|v| Under::Round(v, *centre, *radius)),
            SheetPick::Edge { view, ends } => {
                laid_view(laid, view).map(|v| Under::Line(v, ends[0], ends[1]))
            }
            _ => None,
        });
        let ready = picked
            .as_ref()
            .is_some_and(|p| p.round || p.points.len() >= 2);
        // What is picked shows; and what the pointer would pick.
        if let Some(picked) = &picked
            && let Some(view) = laid_view(laid, &picked.view)
        {
            let on_sheet: Vec<Point2> = picked.points.iter().map(|p| sheet_of(view, *p)).collect();
            if picked.round && on_sheet.len() == 2 {
                painter.circle_stroke(
                    to_screen(on_sheet[0]),
                    on_sheet[0].distance(on_sheet[1]) as f32
                        * (to_screen(Point2::X).x - to_screen(Point2::ZERO).x),
                    ink,
                );
            } else {
                for p in &on_sheet {
                    painter.circle_filled(to_screen(*p), 3.5, color::ACCENT);
                }
            }
            if ready && let Some(p) = pointer {
                let middle = on_sheet.iter().sum::<Point2>() / on_sheet.len().max(1) as f64;
                painter.add(egui::Shape::dashed_line(
                    &[to_screen(middle), to_screen(p)],
                    egui::Stroke::new(1.0, color::ACCENT),
                    5.0,
                    3.0,
                ));
            }
        }
        if !response.clicked() {
            return;
        }
        let Some(p) = pointer else {
            return;
        };
        match (under, picked) {
            // On something: it is picked.
            (Some(Under::Point(view, s)), picked) if !ready => {
                let mut picked = picked.unwrap_or_else(|| Picked {
                    view: view.id.clone(),
                    points: Vec::new(),
                    round: false,
                });
                picked.points.push(own_of(view, s));
                self.tool = Tool::Dimension(Some(picked));
            }
            (Some(Under::Round(view, c, r)), None) => {
                self.tool = Tool::Dimension(Some(Picked {
                    view: view.id.clone(),
                    points: vec![own_of(view, c), own_of(view, c + Point2::X * r)],
                    round: true,
                }));
            }
            (Some(Under::Line(view, a, b)), None) => {
                self.tool = Tool::Dimension(Some(Picked {
                    view: view.id.clone(),
                    points: vec![own_of(view, a), own_of(view, b)],
                    round: false,
                }));
            }
            // Beside what is picked: the dimension stands there. Where
            // it stands says what it measures: beside two points up
            // and down, above or below them across.
            (_, Some(picked)) if ready => {
                let Some(view) = laid_view(laid, &picked.view) else {
                    return;
                };
                let (a, b) = (
                    sheet_of(view, picked.points[0]),
                    sheet_of(view, picked.points[1]),
                );
                let (lo, hi) = (a.min(b), a.max(b));
                let beside = p.x < lo.x || p.x > hi.x;
                let above = p.y < lo.y || p.y > hi.y;
                let upright = if ui.input(|i| i.modifiers.shift) {
                    None
                } else if beside && !above {
                    Some(true)
                } else if above && !beside {
                    Some(false)
                } else if (hi.y - lo.y).min(hi.x - lo.x) > 1e-6 {
                    // (Off a corner of a line that lies askew: along
                    // it.)
                    None
                } else {
                    Some((hi.y - lo.y) > (hi.x - lo.x))
                };
                let mut new = shown.sheet.clone();
                new.own.push(OwnDimension {
                    view: picked.view.clone(),
                    from: picked.points[0].to_array(),
                    to: picked.points[1].to_array(),
                    upright,
                    round: picked.round,
                    at: (p - view.anchor).to_array(),
                });
                out.push(set(shown, new, true));
                // (The tool stays in hand for the next.)
                self.tool = Tool::Dimension(None);
            }
            _ => {}
        }
    }

    /// Drawing on the sheet by hand: the pointer is the sketch's, which
    /// lies on the sheet; what it points at snaps to the views.
    #[allow(clippy::too_many_arguments)]
    fn draw_by_hand(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        shown: &SheetShown,
        hand: &Hand,
        pointer: Option<Point2>,
        to_screen: &dyn Fn(Point2) -> egui::Pos2,
        zoom: f32,
        out: &mut Vec<Message>,
    ) {
        let rect = response.rect;
        let painter = ui.painter_at(rect);
        let view = CanvasView {
            pixel: f64::from(1.0 / zoom),
            right: Point2::X,
            up: Point2::Y,
            middle: Point2::ZERO,
        };
        hand_shapes(&painter, &(hand.draw)(&view), to_screen);
        let reach = f64::from(7.0 / zoom);
        let add = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
        let mut send = |kind: CanvasEventKind, p: Point2| {
            let snapped = snap(&shown.laid, p, reach, None, None, &[]);
            let (at, on_model) = (snapped.at, snapped.on);
            if on_model && kind == CanvasEventKind::Hover {
                painter.circle_stroke(to_screen(at), 5.0, egui::Stroke::new(1.0, PREVIEW));
            }
            let event = CanvasEvent {
                kind,
                at,
                add,
                view,
            };
            if let Some(message) = (hand.on_event)(event) {
                out.push(message);
                if !matches!(kind, CanvasEventKind::Hover | CanvasEventKind::Drag) {
                    out.push(Message::EndEdit);
                }
            }
        };
        let primary = egui::PointerButton::Primary;
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .map(|p| {
                let d = p - to_screen(Point2::ZERO);
                Point2::new(f64::from(d.x / zoom), f64::from(-d.y / zoom))
            })
            .or(pointer);
        if let Some(p) = pointer {
            if response.drag_started_by(primary) {
                send(CanvasEventKind::Press, origin.unwrap_or(p));
            }
            if response.dragged_by(primary) {
                send(CanvasEventKind::Drag, p);
            } else if response.drag_stopped_by(primary) {
                send(CanvasEventKind::Release, p);
            } else if response.double_clicked() {
                send(CanvasEventKind::DoubleClick, p);
            } else if response.clicked() {
                send(CanvasEventKind::Click, p);
            } else if !response.dragged() {
                send(CanvasEventKind::Hover, p);
            }
        }
        // The field a drawn dimension's value is typed in, where the
        // dimension is.
        let Some(input) = &hand.input else {
            self.entry = None;
            return;
        };
        let fresh = self.entry.as_ref().map(|e| &e.0) != Some(&input.key);
        if fresh {
            self.entry = Some((input.key.clone(), input.text.clone(), None));
        }
        let Some((_, text, error)) = &mut self.entry else {
            return;
        };
        let mut entered = false;
        egui::Area::new(egui::Id::new("sheet entry"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(to_screen(input.at))
            .constrain_to(rect)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::same(3))
                    .show(ui, |ui| {
                        let edit = egui::TextEdit::singleline(text)
                            .desired_width(96.0)
                            .horizontal_align(egui::Align::Center);
                        let mut output = edit.show(ui);
                        if fresh {
                            output.response.request_focus();
                            let all = egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(text.chars().count()),
                            );
                            output.state.cursor.set_char_range(Some(all));
                            output.state.store(ui.ctx(), output.response.id);
                        }
                        if output.response.changed() {
                            *error = None;
                        }
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let away = ui.input(|i| i.pointer.any_pressed());
                        entered = output.response.lost_focus() && (enter || away);
                        if entered && enter {
                            output.response.request_focus();
                        }
                        if let Some(error) = error {
                            ui.colored_label(color::ERROR, error.as_str());
                        }
                    });
            });
        if entered {
            match input.enter(text) {
                Ok(message) => out.extend([message, Message::EndEdit]),
                Err(why) => *error = Some(why),
            }
        }
    }

    /// Selecting, dragging and deleting.
    #[allow(clippy::too_many_arguments)]
    fn select(
        &mut self,
        response: &egui::Response,
        shown: &SheetShown,
        pointer: Option<Point2>,
        reach: f64,
        current: Option<&SheetPick>,
        (painter, to_screen): (&egui::Painter, &dyn Fn(Point2) -> egui::Pos2),
        out: &mut Vec<Message>,
    ) {
        let laid = &shown.laid;
        // (A corner of a cutting line, a detail's middle: dragged, they
        // snap as the sketcher's points do, level with points hovered
        // on the way, and show it so.)
        let mut remembered = std::mem::take(&mut self.remembered);
        let mut snapped_at = |p: Point2, only: &str, other: Option<Point2>| {
            let snapped = snap(laid, p, reach, Some(only), other, &remembered);
            snapped.remember(&mut remembered);
            hand_shapes(painter, &snapped.cues(&snap_view(reach)), to_screen);
            snapped.at
        };
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(p) = pointer
        {
            // What was under the pointer where the button went down: it
            // is selected, and it (or the part of it grabbed) dragged —
            // from there.
            let from = response
                .ctx
                .input(|i| i.pointer.press_origin())
                .map_or(p, |o| {
                    let d = o - response.rect.min - self.pan;
                    let zoom = self.zoom.unwrap_or(1.0);
                    Point2::new(f64::from(d.x / zoom), f64::from(-d.y / zoom))
                });
            if let Some(pick) = current {
                if let Some(selected) = selected_by(pick, &shown.sheet) {
                    self.selected = Some(selected);
                }
                self.drag = grip_of(pick, shown).map(|what| Drag {
                    what,
                    from,
                    sheet: shown.sheet.clone(),
                });
            }
        }
        if let (Some(drag), Some(p)) = (&self.drag, pointer) {
            let by = (p - drag.from).to_array();
            let mut new = drag.sheet.clone();
            let moved = |at: &mut [f64; 2]| {
                at[0] += by[0];
                at[1] += by[1];
            };
            // (Where a corner or a middle is dragged to snaps as where
            // it was first put did.)
            let parent_of = |id: &str| {
                let of = match &drag.sheet.view(id)?.kind {
                    ViewKind::Section { of, .. } => of.as_ref()?,
                    ViewKind::Detail { of, .. } => of,
                    _ => return None,
                };
                laid_view(laid, of)
            };
            match &drag.what {
                Grip::Whole(Selected::View(id)) => {
                    if let Some(view) = new.views.iter_mut().find(|v| v.id == *id) {
                        moved(&mut view.at);
                    }
                }
                Grip::Whole(Selected::Note { index, kind }) => match kind {
                    Placing::Note => {
                        if let Some(note) = new.notes.get_mut(*index) {
                            moved(&mut note.at);
                            snap_text(laid, *index, kind, &mut note.at, &drag.sheet, reach);
                        }
                    }
                    Placing::Own => {
                        if let Some(own) = new.own.get_mut(*index) {
                            moved(&mut own.at);
                            snap_text(laid, *index, kind, &mut own.at, &drag.sheet, reach);
                        }
                    }
                    Placing::Words => {
                        if let Some(words) = new.words.get_mut(*index) {
                            moved(&mut words.at);
                        }
                    }
                    // (A line stands where what it is of is.)
                    Placing::Line(_) => {}
                    // A balloon: where it is put; one put by itself is
                    // put by hand from now on, where it is dropped.
                    Placing::Balloon(part) => {
                        if let Some(balloon) = new.balloons.get_mut(*index) {
                            moved(&mut balloon.at);
                        } else if let Some(laid_note) = laid.notes.iter().find(|n| {
                            n.preview.is_none() && n.kind == Placing::Balloon(part.clone())
                        }) && let Some(view) = laid_view(laid, &laid_note.view)
                        {
                            new.balloons.push(workpiece_ops_drawing::sheet::Balloon {
                                part: part.clone(),
                                view: view.id.clone(),
                                at: (p - view.anchor).to_array(),
                                to: None,
                            });
                        }
                    }
                },
                Grip::Tip { index, k } => {
                    // The leader's end, snapped to the edges of the
                    // words' view: a point of that view.
                    if let Some(words) = new.words.get_mut(*index)
                        && let Some(view) = words.view.as_deref().and_then(|v| laid_view(laid, v))
                    {
                        let at = snap(laid, p, reach, Some(&view.id), None, &[]).at;
                        if let Some(to) = words.to.get_mut(*k) {
                            *to = workpiece_ops_drawing::sheet::Target::View(
                                own_of(view, at).to_array(),
                            );
                        }
                    }
                }
                Grip::End { index, k } => {
                    // How much farther the end runs on: along the line.
                    let piece = laid
                        .notes
                        .iter()
                        .find(|n| {
                            n.preview.is_none()
                                && matches!(n.kind, Placing::Line(_))
                                && n.index == *index
                        })
                        .and_then(|n| n.path.first().copied());
                    if let (Some((a, b)), Some(line)) = (piece, new.lines.get_mut(*index)) {
                        let out = match k {
                            0 => (a - b).normalize_or_zero(),
                            _ => (b - a).normalize_or_zero(),
                        };
                        let was = drag.sheet.lines.get(*index).map_or(0.0, |l| l.ends[*k]);
                        line.ends[*k] = was + (p - drag.from).dot(out);
                    }
                }
                Grip::Start { index, k } => {
                    let laid_note = laid.notes.iter().find(|n| {
                        n.preview.is_none() && n.kind == Placing::Note && n.index == *index
                    });
                    if let (Some(laid_note), Some(note)) = (laid_note, new.notes.get_mut(*index))
                        && let Some(linear) = laid_note.linear
                    {
                        let lines: Vec<(Point2, Point2)> = laid_view(laid, &laid_note.view)
                            .map(|v| v.lines.clone())
                            .unwrap_or_default();
                        let gap = drag.sheet.spacing.gap;
                        note.starts[*k] = Some(linear.start_at(*k, p, gap, &lines, reach));
                    }
                }
                // (The title block stands where its template has it.)
                Grip::Whole(Selected::Block) => {}
                Grip::Caption { view, from } => {
                    if let Some(v) = new.views.iter_mut().find(|v| v.id == *view) {
                        v.caption_at = Some([from[0] + by[0], from[1] + by[1]]);
                    }
                }
                Grip::Line { view, path } => {
                    // The line is the part's: its sketch is drawn anew.
                    if let Some(parent) = parent_of(view)
                        && let Some(cut) = laid.cuts.iter().find(|c| c.view == *view)
                    {
                        let d = p - drag.from;
                        let moved: Vec<Point2> = path.iter().map(|q| *q + d).collect();
                        let done = response.drag_stopped();
                        if moved != cut.path || done {
                            out.push(Message::MoveSectionLine {
                                view: view.clone(),
                                line: moved
                                    .iter()
                                    .map(|q| own_of(parent, *q).to_array())
                                    .collect(),
                                settled: done,
                            });
                        }
                    }
                }
                Grip::Corner { view, index } => {
                    // The line is the part's: its sketch is drawn anew.
                    if let Some(parent) = parent_of(view)
                        && let Some(cut) = laid.cuts.iter().find(|c| c.view == *view)
                    {
                        let mut path = cut.path.clone();
                        let other = match *index {
                            0 => path.get(1),
                            k => path.get(k - 1),
                        }
                        .copied();
                        let at = snapped_at(p, &parent.id, other);
                        if let Some(corner) = path.get_mut(*index) {
                            *corner = at;
                        }
                        let done = response.drag_stopped();
                        if path != cut.path || done {
                            out.push(Message::MoveSectionLine {
                                view: view.clone(),
                                line: path.iter().map(|q| own_of(parent, *q).to_array()).collect(),
                                settled: done,
                            });
                        }
                    }
                }
                Grip::Middle { view } => {
                    if let Some(parent) = parent_of(view)
                        && let Some(ViewKind::Detail { centre, .. }) = new
                            .views
                            .iter_mut()
                            .find(|v| v.id == *view)
                            .map(|v| &mut v.kind)
                    {
                        let at = snapped_at(p, &parent.id, None);
                        *centre = own_of(parent, at).to_array();
                    }
                }
                Grip::Rim { view } => {
                    if let Some(parent) = parent_of(view)
                        && let Some(ViewKind::Detail { centre, radius, .. }) = new
                            .views
                            .iter_mut()
                            .find(|v| v.id == *view)
                            .map(|v| &mut v.kind)
                    {
                        let middle = sheet_of(parent, Point2::from_array(*centre));
                        *radius = (middle.distance(p) / parent.scale).max(1e-6);
                    }
                }
            }
            let done = response.drag_stopped();
            if new != shown.sheet || done {
                out.push(set(shown, new, done));
            }
            if done {
                self.drag = None;
            }
        }
        // Delete takes what is selected off the sheet.
        let delete = response
            .ctx
            .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        if delete && response.ctx.memory(|m| m.focused().is_none()) {
            let mut new = shown.sheet.clone();
            match self.selected.take() {
                Some(Selected::View(id)) => new = new.without_view(&id),
                Some(Selected::Note { index, kind }) => {
                    let list_len = match kind {
                        Placing::Note => new.notes.len(),
                        Placing::Own => new.own.len(),
                        Placing::Words => new.words.len(),
                        Placing::Line(_) => new.lines.len(),
                        Placing::Balloon(_) => new.balloons.len(),
                    };
                    if index < list_len {
                        match kind {
                            Placing::Note => drop(new.notes.remove(index)),
                            Placing::Own => drop(new.own.remove(index)),
                            Placing::Words => drop(new.words.remove(index)),
                            Placing::Line(_) => drop(new.lines.remove(index)),
                            Placing::Balloon(_) => drop(new.balloons.remove(index)),
                        }
                    }
                }
                _ => {}
            }
            if new != shown.sheet {
                out.push(set(shown, new, true));
            }
        }
        self.remembered = remembered;
    }

    /// The dialog of the drawing, where every step's dialog is: what
    /// is selected on the sheet, to change. With nothing selected the
    /// sheet itself; a view; or the title block.
    pub fn inspector(
        &mut self,
        ctx: &egui::Context,
        area: egui::Rect,
        shown: &SheetShown,
        out: &mut Vec<Message>,
    ) {
        if self.drawing {
            return;
        }
        // (What is gone from the sheet is not selected.)
        if let Some(Selected::View(id)) = &self.selected
            && shown.sheet.view(id).is_none()
        {
            self.selected = None;
        }
        let sheet = &shown.sheet;
        let mut new = sheet.clone();
        let title = match &self.selected {
            Some(Selected::View(id)) => shown
                .laid
                .views
                .iter()
                .find(|v| v.id == *id)
                .map_or(id.clone(), |v| v.label.clone()),
            Some(Selected::Block) => "Title block".to_owned(),
            Some(Selected::Note { index, kind }) => match kind {
                Placing::Note => sheet
                    .notes
                    .get(*index)
                    .map_or("Dimension".to_owned(), |n| n.note.clone()),
                Placing::Own => "Sheet dimension".to_owned(),
                Placing::Words => match sheet.words.get(*index) {
                    Some(w) if w.dimension.is_some() => "Words of a dimension".to_owned(),
                    Some(w) if !w.to.is_empty() => "Words on a leader".to_owned(),
                    _ => "Words".to_owned(),
                },
                Placing::Line(of) => match of {
                    workpiece_ops_drawing::sheet::LineOf::Datum(_) => "Datum line".to_owned(),
                    workpiece_ops_drawing::sheet::LineOf::Axis(_) => "Centre line".to_owned(),
                    workpiece_ops_drawing::sheet::LineOf::Pitch(_) => "Pitch circle".to_owned(),
                    workpiece_ops_drawing::sheet::LineOf::Path(_) => "Path".to_owned(),
                },
                Placing::Balloon(_) => "Balloon".to_owned(),
            },
            None => "Sheet".to_owned(),
        };
        let kind = match &self.selected {
            Some(Selected::View(_)) => "View",
            Some(Selected::Note {
                kind: Placing::Note,
                ..
            }) => "Dimension",
            Some(Selected::Note {
                kind: Placing::Words,
                ..
            }) => "Words",
            Some(Selected::Note {
                kind: Placing::Line(_),
                ..
            }) => "Line",
            Some(Selected::Block | Selected::Note { .. }) => "",
            None => "Format",
        };
        super::edit_dialog::window(ctx, area, |ui| {
            ui.horizontal(|ui| {
                ui.heading(title);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(kind);
                });
            });
            ui.separator();
            match self.selected.clone() {
                Some(Selected::View(id)) => self.of_view(ui, shown, &mut new, &id, out),
                Some(Selected::Block) => self.of_block(ui, shown, &mut new, out),
                Some(Selected::Note { index, kind }) => {
                    Self::of_note(ui, shown, &mut new, index, &kind);
                    ui.add_space(4.0);
                    ui.weak("Drag it to where it should stand; Delete takes it off the sheet");
                }
                None => {
                    Self::of_sheet(ui, shown, &mut new);
                    ui.add_space(4.0);
                    ui.weak("Click a view or the title block to change it");
                }
            }
        });
        if new != *sheet {
            // (What is typed is one change until the field is left.)
            let typing = ctx.memory(|m| m.focused().is_some());
            out.push(set(shown, new, !typing));
        }
    }

    /// What is selected of what stands on the sheet, with its own
    /// fields: words their text, height and leader; a line what it is of
    /// and how far its ends run on; a balloon its item.
    fn of_note(
        ui: &mut egui::Ui,
        shown: &SheetShown,
        new: &mut Sheet,
        index: usize,
        kind: &Placing,
    ) {
        egui::Grid::new("note-inspector")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| match kind {
                Placing::Words => {
                    let Some(words) = new.words.get_mut(index) else {
                        return;
                    };
                    ui.label("Text");
                    ui.add(
                        egui::TextEdit::multiline(&mut words.text)
                            .desired_rows(1)
                            .desired_width(170.0),
                    );
                    ui.end_row();
                    ui.label("Height");
                    egui::ComboBox::from_id_salt("words-height")
                        .selected_text(format!("{} mm", words.height))
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            // (The heights of ISO 3098.)
                            for h in [2.5, 3.5, 5.0, 7.0, 10.0] {
                                ui.selectable_value(&mut words.height, h, format!("{h} mm"));
                            }
                        });
                    ui.end_row();
                    if let Some(dimension) = &words.dimension {
                        ui.label("Belongs to");
                        ui.label(dimension);
                        ui.end_row();
                    }
                    ui.label("Leaders");
                    ui.vertical(|ui| {
                        let mut gone = None;
                        for (k, to) in words.to.iter().enumerate() {
                            let said = match to {
                                workpiece_ops_drawing::sheet::Target::Model(p) => {
                                    format!("to ({:.4}, {:.4}, {:.4})", p[0], p[1], p[2])
                                }
                                workpiece_ops_drawing::sheet::Target::View(p) => {
                                    format!("to ({:.4}, {:.4}) of the view", p[0], p[1])
                                }
                            };
                            ui.horizontal(|ui| {
                                ui.label(said);
                                if ui.small_button("✕").on_hover_text("Take this leader off").clicked() {
                                    gone = Some(k);
                                }
                            });
                        }
                        if let Some(k) = gone {
                            words.to.remove(k);
                        }
                        if words.view.is_some()
                            && ui
                                .button("Add a leader")
                                .on_hover_text("A leader from the words to the view's middle: drag its end to what the words say something of (ISO 128-22: an arrowhead on an outline, a dot within a face)")
                                .clicked()
                        {
                            words.to.push(workpiece_ops_drawing::sheet::Target::View([0.0, 0.0]));
                        }
                        if words.view.is_none() {
                            ui.weak("none: words on the sheet itself have no leader");
                        }
                    });
                    ui.end_row();
                    ui.label("Stands by");
                    ui.label(words.view.as_deref().unwrap_or("the sheet"));
                    ui.end_row();
                }
                Placing::Line(of) => {
                    let Some(line) = new.lines.get_mut(index) else {
                        return;
                    };
                    ui.label("Of");
                    ui.label(of.label());
                    ui.end_row();
                    ui.label("Line type");
                    ui.label(match of {
                        workpiece_ops_drawing::sheet::LineOf::Datum(_) => {
                            "Long-dashed dotted narrow (ISO 128-2, 04.1): a datum's trace"
                        }
                        _ => "Long-dashed dotted narrow (ISO 128-2, 04.1): a centre line",
                    });
                    ui.end_row();
                    ui.label("In");
                    ui.label(line.view.as_deref().unwrap_or("every view that shows it"));
                    ui.end_row();
                    for (k, name) in ["Runs on at its start", "Runs on at its end"]
                        .iter()
                        .enumerate()
                    {
                        ui.label(*name);
                        ui.add(
                            egui::DragValue::new(&mut line.ends[k])
                                .speed(0.1)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                    }
                    ui.label("Drawn");
                    ui.horizontal(|ui| {
                        let mut shown = !line.hidden;
                        if ui.checkbox(&mut shown, "shown").changed() {
                            line.hidden = !shown;
                        }
                        ui.checkbox(&mut line.symmetry, "symmetry marks")
                            .on_hover_text("ISO 128-2: two short parallel lines across each end of a line of symmetry");
                    });
                    ui.end_row();
                    ui.label("Words at its end");
                    let mut words = line.label.clone().unwrap_or_default();
                    if ui
                        .add(egui::TextEdit::singleline(&mut words).hint_text("CL SYM").desired_width(150.0))
                        .changed()
                    {
                        line.label = (!words.is_empty()).then_some(words);
                    }
                    ui.end_row();
                }
                Placing::Balloon(part) => {
                    ui.label("Part");
                    ui.label(part);
                    ui.end_row();
                    let by_hand = new.balloons.iter().position(|b| b.part == *part);
                    ui.label("Placed");
                    match by_hand {
                        Some(k) => {
                            if ui.button("Put it by itself").clicked() {
                                new.balloons.remove(k);
                            }
                        }
                        None => {
                            ui.label("by itself");
                        }
                    }
                    ui.end_row();
                }
                Placing::Note | Placing::Own => {
                    let laid = shown
                        .laid
                        .notes
                        .iter()
                        .find(|n| n.kind == *kind && n.index == index);
                    if let Some(view) = laid.map(|n| n.view.clone()) {
                        ui.label("In");
                        ui.label(view);
                        ui.end_row();
                    }
                }
            });
    }

    /// A selected view: what it shows, how it is drawn, at what scale.
    fn of_view(
        &mut self,
        ui: &mut egui::Ui,
        shown: &SheetShown,
        new: &mut Sheet,
        id: &str,
        out: &mut Vec<Message>,
    ) {
        let Some(view) = new.views.iter_mut().find(|v| v.id == id) else {
            return;
        };
        let id = &id.to_owned();
        egui::Grid::new("view-inspector")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                // What it looks at the model from: a side, or a camera
                // view of the part.
                if matches!(view.kind, ViewKind::Base { .. } | ViewKind::Named { .. }) {
                    ui.label("Shows");
                    let now = match &view.kind {
                        ViewKind::Base { side } => side.label().to_owned(),
                        ViewKind::Named { view } => view.clone(),
                        _ => String::new(),
                    };
                    egui::ComboBox::from_id_salt("view-shows")
                        .height(360.0)
                        .selected_text(now)
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for side in Side::ALL {
                                let on = view.kind == ViewKind::Base { side };
                                if ui.selectable_label(on, side.label()).clicked() {
                                    view.kind = ViewKind::Base { side };
                                }
                            }
                            if !shown.views.is_empty() {
                                ui.separator();
                                ui.weak("The part's camera views");
                            }
                            for name in &shown.views {
                                let kind = ViewKind::Named { view: name.clone() };
                                if ui.selectable_label(view.kind == kind, name).clicked() {
                                    view.kind = kind;
                                }
                            }
                        })
                        .response
                        .on_hover_text("The side the model is seen from, or a camera view of the part (a Camera view step)");
                    ui.end_row();
                }
                ui.label("Lines");
                egui::ComboBox::from_id_salt("view-style")
                    .selected_text(view.style.label())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for style in Style::ALL {
                            ui.selectable_value(&mut view.style, style, style.label());
                        }
                    });
                ui.end_row();
                let own = matches!(
                    view.kind,
                    ViewKind::Base { .. }
                        | ViewKind::Named { .. }
                        | ViewKind::Detail { .. }
                        | ViewKind::Section { .. }
                );
                if own {
                    // (A section of a view is drawn at that view's
                    // scale where it has none of its own.)
                    let none = match &view.kind {
                        ViewKind::Section { of: Some(_), .. } => "As the view it is of",
                        _ => "The sheet's scale",
                    };
                    ui.label("Scale");
                    ui.horizontal(|ui| {
                        let now = view.scale.map_or(none.to_owned(), scale_text);
                        egui::ComboBox::from_id_salt("view-scale")
                            .height(480.0)
                            .selected_text(now)
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut view.scale, None, none);
                                for scale in SCALES {
                                    ui.selectable_value(&mut view.scale, Some(scale), scale_text(scale));
                                }
                                let custom = view.scale.is_some_and(|s| !SCALES.contains(&s));
                                if ui.selectable_label(custom, "Custom…").clicked() && !custom {
                                    view.scale = Some([3, 1]);
                                }
                            })
                            .response
                            .on_hover_text("The scale it is drawn at; one not the sheet's is said in its caption (ISO 5455): A–A (2:1)");
                        // A scale of its own: as two whole numbers.
                        if let Some(scale) = view.scale.as_mut().filter(|s| !SCALES.contains(s)) {
                            ui.add(egui::DragValue::new(&mut scale[0]).range(1..=1000).speed(0.1));
                            ui.label(":");
                            ui.add(egui::DragValue::new(&mut scale[1]).range(1..=1000).speed(0.1));
                        }
                    });
                    ui.end_row();
                }
                match &mut view.kind {
                    ViewKind::Detail { radius, letter, .. } => {
                        ui.label("Reach");
                        ui.add(egui::DragValue::new(radius).speed(0.05).range(1e-6..=1e9));
                        ui.end_row();
                        ui.label("Letter");
                        ui.add(egui::TextEdit::singleline(letter).desired_width(30.0));
                        ui.end_row();
                    }
                    ViewKind::Section {
                        section,
                        letter,
                        of,
                        ..
                    } => {
                        // The view its cutting line shows in: one that
                        // sees the cut edge on.
                        let able: Vec<&str> = shown
                            .laid
                            .views
                            .iter()
                            .find(|v| v.id == *id)
                            .map(|v| v.cut_shows_in.iter().map(String::as_str).collect())
                            .unwrap_or_default();
                        let label_of = |v: &str| {
                            shown
                                .laid
                                .views
                                .iter()
                                .find(|l| l.id == v)
                                .map_or(v.to_owned(), |l| format!("{} ({v})", l.label))
                        };
                        ui.label("Show cutting line in");
                        ui.horizontal(|ui| {
                            let chosen_id = egui::Id::new(("cut-shows-in", id.as_str()));
                            let mut chosen: Option<String> = ui
                                .data(|d| d.get_temp::<Option<String>>(chosen_id))
                                .flatten()
                                .or_else(|| of.clone())
                                .or_else(|| able.first().map(|v| (*v).to_owned()));
                            let now = chosen.as_deref().map_or("no view".to_owned(), label_of);
                            egui::ComboBox::from_id_salt("view-cut-in")
                                .selected_text(now)
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    for v in &able {
                                        let on = chosen.as_deref() == Some(*v);
                                        if ui.selectable_label(on, label_of(v)).clicked() {
                                            chosen = Some((*v).to_owned());
                                        }
                                    }
                                    if able.is_empty() {
                                        ui.weak("No view sees the cut edge on");
                                    }
                                })
                                .response
                                .on_hover_text("The views that see the cut edge on: where its cutting line, with its arrows and letters, can show");
                            ui.data_mut(|d| d.insert_temp(chosen_id, chosen.clone()));
                            let shows = chosen.is_some() && *of == chosen;
                            let text = if shows { "Shown" } else { "Place" };
                            if ui
                                .add_enabled(chosen.is_some() && !shows, egui::Button::new(text))
                                .on_hover_text("Put the cutting line into that view (the section then stands in line with it, at its scale unless it has its own)")
                                .clicked()
                            {
                                *of = chosen;
                            }
                        });
                        ui.end_row();
                        ui.label("Section");
                        egui::ComboBox::from_id_salt("view-section")
                            .selected_text(section.as_str())
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for name in &shown.sections {
                                    ui.selectable_value(section, name.clone(), name);
                                }
                            })
                            .response
                            .on_hover_text("The section of the part (a Section step) it cuts the model by");
                        ui.end_row();
                        ui.label("Letter");
                        ui.add(egui::TextEdit::singleline(letter).desired_width(30.0));
                        ui.end_row();
                        // A half section: one half of it cut, the other
                        // seen uncut (or left out), divided by a centre
                        // line (ISO 128-3).
                        ui.label("Half");
                        let now = match view.half {
                            None => "Whole".to_owned(),
                            Some(h) if h.only => format!("{} only", h.cut.label()),
                            Some(h) => format!("{} cut", h.cut.label()),
                        };
                        egui::ComboBox::from_id_salt("view-half")
                            .selected_text(now)
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut view.half, None, "Whole");
                                for only in [false, true] {
                                    for cut in workpiece_ops_drawing::sheet::HalfSide::ALL {
                                        let half = Some(workpiece_ops_drawing::sheet::Half { cut, only });
                                        let text = match only {
                                            true => format!("{} only", cut.label()),
                                            false => format!("{} cut", cut.label()),
                                        };
                                        ui.selectable_value(&mut view.half, half, text);
                                    }
                                }
                            })
                            .response
                            .on_hover_text("A half section: only one half of the view (from its middle line) cut, the other half seen as it is — or, \"only\", not drawn at all");
                        ui.end_row();
                        // Which side it shows: the part's section takes
                        // away the other side.
                        ui.label("Side");
                        if ui
                            .button("↔ Show the other side")
                            .on_hover_text("Turn the section round: the part's Section step takes away the other side, and its arrows point the other way")
                            .clicked()
                        {
                            out.push(Message::FlipSection { view: id.clone() });
                        }
                        ui.end_row();
                    }
                    ViewKind::Auxiliary { angle, .. } => {
                        ui.label("Looks along");
                        ui.add(egui::DragValue::new(angle).speed(1.0).suffix("°"));
                        ui.end_row();
                    }
                    _ => {}
                }
            });
        let hint = match &view.kind {
            ViewKind::Detail { .. } => "Drag its middle or its circle in the view it is of",
            ViewKind::Section { .. } if shown.movable.contains(id) => {
                "Drag the corners of its cutting line in the view it is of; the part's sketch of the line follows"
            }
            ViewKind::Section { .. } => {
                "Where it cuts is the part's: edit the section step (or its sketch) to move it"
            }
            _ => "Drag it on the sheet; Delete takes it off",
        };
        ui.add_space(4.0);
        ui.weak(hint);
    }

    /// The sheet itself: its size, its scale, what it is made like.
    fn of_sheet(ui: &mut egui::Ui, shown: &SheetShown, new: &mut Sheet) {
        egui::Grid::new("sheet-inspector")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("Size");
                egui::ComboBox::from_id_salt("sheet-format")
                    .selected_text(new.format.label())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for format in Format::ALL {
                            ui.selectable_value(&mut new.format, format, format.label());
                        }
                    });
                ui.end_row();
                ui.label("Lies");
                let lies = |upright| if upright { "Portrait" } else { "Landscape" };
                egui::ComboBox::from_id_salt("sheet-upright")
                    .selected_text(lies(new.upright))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for upright in [false, true] {
                            ui.selectable_value(&mut new.upright, upright, lies(upright));
                        }
                    });
                ui.end_row();
                ui.label("Scale");
                egui::ComboBox::from_id_salt("sheet-scale")
                    .height(360.0)
                    .selected_text(scale_text(new.scale))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for scale in SCALES {
                            ui.selectable_value(&mut new.scale, scale, scale_text(scale));
                        }
                    });
                ui.end_row();
                ui.label("Projection");
                let name = |projection| match projection {
                    Projection::FirstAngle => "First angle",
                    Projection::ThirdAngle => "Third angle",
                };
                egui::ComboBox::from_id_salt("sheet-projection")
                    .selected_text(name(new.projection))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for projection in [Projection::FirstAngle, Projection::ThirdAngle] {
                            ui.selectable_value(&mut new.projection, projection, name(projection));
                        }
                    })
                    .response
                    .on_hover_text(
                        "First angle is what ISO drawings use: the view from the left stands to the right",
                    );
                ui.end_row();
                ui.label("General tolerance");
                ui.horizontal(|ui| {
                    let mut text = new.general_tolerance.clone().unwrap_or_default();
                    let edited = ui
                        .add(egui::TextEdit::singleline(&mut text).desired_width(118.0))
                        .on_hover_text(
                            "What the title block says holds of every size it gives no tolerance of: ISO 2768 for machined parts, ISO 8062-3 for castings, or words of your own; empty for none",
                        )
                        .changed();
                    egui::ComboBox::from_id_salt("sheet-general-tolerance")
                        .selected_text("")
                        .width(24.0)
                        .show_ui(ui, |ui| {
                            for said in workpiece_ops_drawing::sheet::GENERAL_TOLERANCES
                                .into_iter()
                                .chain(["None"])
                            {
                                if ui.selectable_label(text == said, said).clicked() {
                                    new.general_tolerance =
                                        (said != "None").then(|| said.to_owned());
                                }
                            }
                        });
                    if edited {
                        // (As typed: a space at its end is kept while
                        // more is typed.)
                        new.general_tolerance = (!text.is_empty()).then_some(text);
                    }
                });
                ui.end_row();
                ui.label("Fits");
                let fits = |with: bool| if with { "Class (deviations)" } else { "Class only" };
                egui::ComboBox::from_id_salt("sheet-fits")
                    .selected_text(fits(new.numbers.fit_deviations))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for with in [false, true] {
                            ui.selectable_value(&mut new.numbers.fit_deviations, with, fits(with));
                        }
                    })
                    .response
                    .on_hover_text(
                        "How a tolerance class is written: Ø20 H7, or with its ISO 286 deviations after it in brackets, Ø20 H7 (+0,021/0)",
                    );
                ui.end_row();
                ui.label("Centre lines");
                egui::ComboBox::from_id_salt("sheet-centres")
                    .selected_text(new.centres.label())
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for centres in workpiece_ops_drawing::sheet::Centres::ALL {
                            ui.selectable_value(&mut new.centres, centres, centres.label());
                        }
                    })
                    .response
                    .on_hover_text(
                        "Which centre lines the sheet draws by itself; the others, and the traces of datums, are put on from \"Show missing\"",
                    );
                ui.end_row();
                ui.label("Parts");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut new.parts_list, "Parts list")
                        .on_hover_text("A parts list (ISO 7573) over the title block: of the parts placed in the model, each kind an item with its number, how many, its name and its material");
                    ui.checkbox(&mut new.auto_balloons, "Balloons")
                        .on_hover_text("A balloon (ISO 6433) for each item, put by itself in the first view that shows it; drag one to put it by hand");
                });
                ui.end_row();
                ui.label("Line widths");
                let wide = new.drafting.wide(new.format);
                egui::ComboBox::from_id_salt("sheet-widths")
                    .selected_text(format!("{wide} / {}", wide / 2.0))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        // (The series of ISO 128-2, wide and narrow 2:1.)
                        for w in [0.25, 0.35, 0.5, 0.7, 1.0, 1.4] {
                            if ui
                                .selectable_label(wide == w, format!("{w} / {}", w / 2.0))
                                .clicked()
                            {
                                new.drafting.wide = Some(w);
                            }
                        }
                    })
                    .response
                    .on_hover_text(
                        "The wide lines (visible edges) and the narrow ones (dimensions, hatching, centre lines), ISO 128-2",
                    );
                ui.end_row();
                if !shown.templates.is_empty() {
                    ui.label("Template");
                    egui::ComboBox::from_id_salt("sheet-template")
                        .selected_text(if new.template.is_empty() {
                            "—"
                        } else {
                            new.template.as_str()
                        })
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for template in &shown.templates {
                                let on = new.template == template.name;
                                if ui.selectable_label(on, &template.name).clicked() {
                                    // (Its sheet and its title block;
                                    // the views stay.)
                                    new.template.clone_from(&template.name);
                                    new.format = template.format;
                                    new.upright = template.upright;
                                    new.projection = template.projection;
                                    new.block.clone_from(&template.block);
                                }
                            }
                        })
                        .response
                        .on_hover_text(
                            "Make the sheet and its title block like a template's (the views stay)",
                        );
                    ui.end_row();
                }
            });
    }

    /// The title block: what each of its cells says.
    fn of_block(
        &mut self,
        ui: &mut egui::Ui,
        shown: &SheetShown,
        new: &mut Sheet,
        out: &mut Vec<Message>,
    ) {
        if new.block.is_empty() {
            ui.weak("The sheet has no title block: a sheet template gives it one");
        }
        for (index, cell) in new.block.iter_mut().enumerate() {
            let laid = shown.laid.cells.get(index);
            let id = egui::Id::new(("title-cell", index));
            if self.cell == Some(index) {
                ui.ctx().memory_mut(|m| m.request_focus(id));
                self.cell = None;
            }
            ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                    egui::vec2(110.0, 18.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(110.0);
                        ui.add(egui::Label::new(cell.label.as_str()).truncate());
                    },
                );
                        match &mut cell.content {
                            Content::Text(text) => {
                                ui.add(egui::TextEdit::singleline(text).id(id).desired_width(140.0));
                            }
                            Content::Property(name) => {
                                let value = laid.map_or("", |c| c.value.as_str());
                                let property = shown.schema.iter().find(|p| p.name == *name);
                                let given = workpiece_ops_drawing::template::BUILT_IN
                                    .contains(&name.as_str())
                                    && !matches!(name.as_str(), "Name" | "Title");
                                if given {
                                    // (The sheet's or the part's own: not
                                    // said here.)
                                    ui.add_enabled(
                                        false,
                                        egui::TextEdit::singleline(&mut value.to_owned())
                                            .id(id)
                                            .desired_width(140.0),
                                    )
                                    .on_disabled_hover_text(format!(
                                        "{name} is the sheet's or the part's own: it follows by itself"
                                    ));
                                } else if let Some(workpiece_part::Kind::Choice(of)) =
                                    property.map(|p| &p.kind)
                                {
                                    let mut now = value.to_owned();
                                    egui::ComboBox::from_id_salt(id)
                                        .selected_text(now.as_str())
                                        .width(140.0)
                                        .show_ui(ui, |ui| {
                                            for choice in of {
                                                ui.selectable_value(&mut now, choice.clone(), choice);
                                            }
                                        });
                                    if now != value {
                                        out.push(Message::SetProperty {
                                            name: name.clone(),
                                            value: now,
                                        });
                                    }
                                } else {
                                    // (Typed as text; taken when the field
                                    // is left, if it is of its kind.)
                                    let typed = ui.data_mut(|d| {
                                        d.get_temp::<String>(id).unwrap_or_else(|| value.to_owned())
                                    });
                                    let mut now = typed.clone();
                                    let field = ui.add(
                                        egui::TextEdit::singleline(&mut now).id(id).desired_width(140.0),
                                    );
                                    if field.has_focus() || field.lost_focus() {
                                        ui.data_mut(|d| d.insert_temp(id, now.clone()));
                                    }
                                    if field.lost_focus() {
                                        ui.data_mut(|d| d.remove::<String>(id));
                                        if now != value {
                                            out.push(Message::SetProperty {
                                                name: name.clone(),
                                                value: now,
                                            });
                                        }
                                    }
                                    field.on_hover_text(format!(
                                        "The part's property {name}: what every sheet and table of the part says"
                                    ));
                                }
                            }
                        }
                    });
        }
    }
}

/// The colour of a tone of what a sketch draws, on paper.
fn tone_colour(tone: Tone) -> egui::Color32 {
    match tone {
        Tone::Free => egui::Color32::from_rgb(20, 90, 200),
        Tone::Constrained => egui::Color32::from_rgb(12, 14, 18),
        Tone::Construction => egui::Color32::from_rgb(130, 136, 146),
        Tone::Projected => egui::Color32::from_rgb(140, 70, 170),
        Tone::Hover => egui::Color32::from_rgb(240, 140, 0),
        Tone::Selected => egui::Color32::from_rgb(225, 95, 0),
        Tone::Conflict => egui::Color32::from_rgb(210, 30, 30),
        Tone::Dimension => egui::Color32::from_rgb(40, 60, 90),
        Tone::Preview => egui::Color32::from_rgb(60, 130, 230),
        Tone::Badge => egui::Color32::from_rgb(40, 130, 90),
    }
}

/// Paints what a sketch lying on the sheet draws.
fn hand_shapes(
    painter: &egui::Painter,
    shapes: &[CanvasShape],
    to_screen: &dyn Fn(Point2) -> egui::Pos2,
) {
    for shape in shapes {
        match shape {
            CanvasShape::Polyline {
                points,
                tone,
                width,
                dashed,
            } => {
                let points: Vec<egui::Pos2> = points.iter().map(|p| to_screen(*p)).collect();
                let pen = egui::Stroke::new(*width, tone_colour(*tone));
                if *dashed {
                    painter.add(egui::Shape::dashed_line(&points, pen, 6.0, 4.0));
                } else {
                    painter.add(egui::Shape::line(points, pen));
                }
            }
            CanvasShape::Dot { at, tone, size } => {
                painter.circle_filled(to_screen(*at), size / 2.0, tone_colour(*tone));
            }
            CanvasShape::Arrowhead {
                tip,
                direction,
                tone,
            } => {
                let tip = to_screen(*tip);
                let way = egui::vec2(direction.x as f32, -(direction.y as f32)).normalized();
                let across = way.rot90() * (11.0 * 7.5_f32.to_radians().tan());
                let back = tip - way * 11.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![tip, back + across, back - across],
                    tone_colour(*tone),
                    egui::Stroke::NONE,
                ));
            }
            CanvasShape::Text {
                at,
                along,
                text,
                upper,
                lower,
                tone,
                badge,
            } => {
                let ink = tone_colour(*tone);
                let angle = -(along.y.atan2(along.x) as f32);
                let (sin, cos) = angle.sin_cos();
                let turned =
                    |v: egui::Vec2| egui::vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
                let size = if *badge { 10.0 } else { 12.5 };
                let main =
                    painter.layout_no_wrap(text.clone(), egui::FontId::proportional(size), ink);
                let small = |t: &String| {
                    painter.layout_no_wrap(t.clone(), egui::FontId::proportional(9.0), ink)
                };
                let (up, low) = (small(upper), small(lower));
                let wide = main.size().x + up.size().x.max(low.size().x);
                let centre = to_screen(*at);
                let start = centre - turned(egui::vec2(wide / 2.0, main.size().y / 2.0));
                if *badge {
                    let bg = egui::Rect::from_center_size(centre, main.size()).expand(2.0);
                    painter.rect_filled(bg, 2.0, egui::Color32::from_white_alpha(230));
                    painter.rect_stroke(
                        bg,
                        2.0,
                        egui::Stroke::new(0.7, ink),
                        egui::StrokeKind::Inside,
                    );
                }
                let after = main.size().x + 1.0;
                let high = main.size().y;
                painter.add(egui::epaint::TextShape::new(start, main, ink).with_angle(angle));
                for (galley, down) in [(up, -2.0), (low, high - 10.0)] {
                    painter.add(
                        egui::epaint::TextShape::new(
                            start + turned(egui::vec2(after, down)),
                            galley,
                            ink,
                        )
                        .with_angle(angle),
                    );
                }
            }
        }
    }
}

/// Paints the sheet: the paper, then what is laid out on it.
fn paint(
    ui: &egui::Ui,
    rect: egui::Rect,
    laid: &Laid,
    to_screen: &dyn Fn(Point2) -> egui::Pos2,
    zoom: f32,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(214, 217, 222));
    let paper = egui::Rect::from_two_pos(
        to_screen(Point2::ZERO),
        to_screen(Point2::from_array(laid.size)),
    );
    painter.rect_filled(
        paper.translate(egui::vec2(3.0, 3.0)),
        0.0,
        egui::Color32::from_black_alpha(40),
    );
    painter.rect_filled(paper, 0.0, egui::Color32::WHITE);
    // (The filled triangles as they are, not feathered: the feathering
    // of a sliver juts out of it in spikes.)
    let mut filled = egui::Mesh::default();
    for fill in &laid.fills {
        let [r, g, b] = fill.colour;
        let colour = egui::Color32::from_rgb(r, g, b);
        let at = filled.vertices.len() as u32;
        for corner in fill.corners {
            filled.colored_vertex(to_screen(corner), colour);
        }
        filled.add_triangle(at, at + 1, at + 2);
    }
    painter.add(filled);
    let ink = egui::Color32::from_rgb(12, 14, 18);
    // Lines that run on from one another are drawn as one: a curve of
    // many short pieces is one stroke of the pen.
    let mut k = 0;
    while k < laid.strokes.len() {
        let pen_of = laid.strokes[k].pen;
        let mut points = vec![to_screen(laid.strokes[k].a), to_screen(laid.strokes[k].b)];
        let mut last = laid.strokes[k].b;
        k += 1;
        while let Some(next) = laid.strokes.get(k)
            && next.pen == pen_of
            && next.preview == laid.strokes[k - 1].preview
            && next.a.distance(last) < 1e-9
        {
            // (Not a point a hair from the one before — a piece of a
            // hatch line cut by many triangles: its way is lost in
            // rounding, and the path's corner there would be drawn as
            // a spike across the sheet.)
            let to = to_screen(next.b);
            if points.last().is_none_or(|p| p.distance(to) > 0.05) {
                points.push(to);
            }
            last = next.b;
            k += 1;
        }
        // (Nor a line a hair long by itself: drawn, it is a dot.)
        if points.len() == 2 && points[0].distance(points[1]) <= 0.05 {
            continue;
        }
        let ink = if laid.strokes[k - 1].preview {
            PREVIEW
        } else {
            ink
        };
        let pen = egui::Stroke::new((pen_of.width(laid.wide) as f32 * zoom).max(0.7), ink);
        // (A chain line's long dashes fitted to its length.)
        let length: f64 = points
            .windows(2)
            .map(|w| f64::from(w[0].distance(w[1]) / zoom))
            .sum();
        let dashes = pen_of.fitted_dashes(laid.wide, length);
        if dashes.is_empty() {
            painter.add(egui::Shape::line(points, pen));
            continue;
        }
        // Dash by dash along it.
        let (mut left, mut d) = ((dashes[0] as f32 * zoom).max(0.5), 0);
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = a.distance(b);
            if length <= 0.0 {
                continue;
            }
            let way = (b - a) / length;
            let mut at = 0.0_f32;
            while at < length {
                let step = left.min(length - at);
                if d % 2 == 0 {
                    painter.line_segment([a + way * at, a + way * (at + step)], pen);
                }
                at += step;
                left -= step;
                if left <= 1e-6 {
                    d += 1;
                    left = (dashes[d % dashes.len()] as f32 * zoom).max(0.5);
                }
            }
        }
    }
    for text in &laid.texts {
        // (The capitals of the interface's letters are about 0.7 of
        // their size high.)
        let cap = text.height as f32 * zoom;
        let size = cap / 0.7;
        if size < 2.0 {
            continue;
        }
        let ink = if text.preview { PREVIEW } else { ink };
        let mut galley =
            painter.layout_no_wrap(text.text.clone(), egui::FontId::proportional(size), ink);
        // (No wider than the sheet lays it out — Helvetica, as the PDF
        // has it — so that what follows it on its line is not written
        // over it.)
        let room = workpiece_ops_drawing::lay::text_width(&text.text, text.height) as f32 * zoom;
        if galley.size().x > room * 1.01 && room > 0.0 {
            let smaller = size * room / galley.size().x;
            galley =
                painter.layout_no_wrap(text.text.clone(), egui::FontId::proportional(smaller), ink);
        }
        // Where its upper left corner is, turned about the point it is
        // set by: on the screen up is down, and a turn the other way.
        let angle = -(text.angle as f32);
        let (sin, cos) = angle.sin_cos();
        let baseline = galley.size().y * 0.80;
        let dx = -(text.anchor[0] as f32) * galley.size().x;
        let dy = -(baseline - text.anchor[1] as f32 * cap);
        let at = to_screen(text.at) + egui::vec2(dx * cos - dy * sin, dx * sin + dy * cos);
        painter.add(egui::epaint::TextShape::new(at, galley, ink).with_angle(angle));
    }
}
