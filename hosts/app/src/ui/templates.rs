//! The sheet templates panel: what new drawings start from. The sheet
//! is drawn as it will look: its views are dragged to where they
//! stand, and its title block is a table whose borders are dragged,
//! whose cells are split and merged, and into which the blocks of what
//! a part says of itself are dropped.

use eframe::egui;
use workpiece_ops_drawing::sheet::{Beside, Format, Projection, SheetView, Side, Style, ViewKind};
use workpiece_ops_drawing::template::{Cell, Content};
use workpiece_ops_drawing::{Template, Templates};
use workpiece_ui::Message;
use workpiece_ui::view::SheetTemplates;

use super::materials::field;
use crate::theme::color;

/// A number typed into a field: what it is made, if it is changed.
fn number(ui: &mut egui::Ui, id: egui::Id, now: f64, width: f32) -> Option<f64> {
    field(ui, id, &format!("{now}"), width).and_then(|t| t.trim().parse::<f64>().ok())
}

/// What a view of a template is of, as it is called.
fn kind_label(view: &SheetView) -> String {
    match &view.kind {
        ViewKind::Base { side } => side.label().to_owned(),
        ViewKind::Named { view } => format!("Camera view {view}"),
        ViewKind::Projected { of, beside } => format!("{beside:?} of {of}"),
        _ => "—".to_owned(),
    }
}

/// What of a template is selected in its picture.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Picked {
    #[default]
    Nothing,
    View(usize),
    Cell(usize),
}

/// How much of the sheet the picture shows.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Zoom {
    /// All of it: where the views stand.
    #[default]
    Sheet,
    /// Its title block, large enough to work on its cells.
    Block,
}

/// Which template the panel shows, and what of it is selected.
#[derive(Default)]
pub struct TemplatesPanel {
    selected: usize,
    picked: Picked,
    zoom: Zoom,
}

/// The margin between a sheet's edge and its frame, in millimetres.
const MARGIN: f64 = 10.0;
/// The size a view is drawn at in the picture (what it will take up
/// depends on the part), in millimetres.
const VIEW: [f64; 2] = [64.0, 46.0];
/// How far a projected view is drawn from what it is projected from.
const APART: f64 = 78.0;
/// The least a cell is wide or high.
const LEAST: f64 = 3.0;

/// The width and height of a title block: as far as its cells reach.
fn block_size(block: &[Cell]) -> [f64; 2] {
    block.iter().fold([0.0, 0.0], |m, c| {
        [m[0].max(c.at[0] + c.size[0]), m[1].max(c.at[1] + c.size[1])]
    })
}

/// Where view `n` of `template` is drawn: a projected one beside what
/// it is projected from.
fn view_at(template: &Template, n: usize, depth: usize) -> [f64; 2] {
    let view = &template.views[n];
    let ViewKind::Projected { of, beside } = &view.kind else {
        return view.at;
    };
    let from = template.views.iter().position(|v| v.id == *of);
    let Some(from) = from.filter(|_| depth < 8) else {
        return view.at;
    };
    let [x, y] = view_at(template, from, depth + 1);
    match beside {
        Beside::Right => [x + APART, y],
        Beside::Left => [x - APART, y],
        Beside::Above => [x, y + APART * 0.8],
        Beside::Below => [x, y - APART * 0.8],
    }
}

/// Every edge of the cells of `block` at `old` (across x, or across y)
/// moved to `new`, as far as that leaves every cell its least size.
fn move_border(block: &mut [Cell], axis: usize, old: f64, new: f64) {
    let same = |a: f64, b: f64| (a - b).abs() < 1e-6;
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for c in block.iter() {
        let (start, end) = (c.at[axis], c.at[axis] + c.size[axis]);
        if same(end, old) {
            lo = lo.max(start + LEAST);
        }
        if same(start, old) {
            hi = hi.min(end - LEAST);
        }
    }
    if lo > hi {
        return;
    }
    let new = new.clamp(lo, hi);
    for c in block.iter_mut() {
        let (start, end) = (c.at[axis], c.at[axis] + c.size[axis]);
        if same(end, old) {
            c.size[axis] = new - start;
        } else if same(start, old) {
            c.at[axis] = new;
            c.size[axis] = end - new;
        }
    }
}

/// The cell beside cell `n` of `block` towards `way` (0 right, 1 left,
/// 2 up, 3 down) that makes one rectangle with it.
fn neighbour(block: &[Cell], n: usize, way: usize) -> Option<usize> {
    let same = |a: f64, b: f64| (a - b).abs() < 1e-6;
    let c = &block[n];
    let (along, across) = if way < 2 { (0, 1) } else { (1, 0) };
    block.iter().position(|o| {
        let lined = same(o.at[across], c.at[across]) && same(o.size[across], c.size[across]);
        let touches = if way.is_multiple_of(2) {
            same(o.at[along], c.at[along] + c.size[along])
        } else {
            same(o.at[along] + o.size[along], c.at[along])
        };
        lined && touches
    })
}

impl TemplatesPanel {
    /// Draws the panel as a window over the view; what is changed is
    /// sent as it is changed.
    pub fn show(&mut self, ctx: &egui::Context, shown: &SheetTemplates, out: &mut Vec<Message>) {
        let mut open = true;
        egui::Window::new("Sheet templates")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(egui::pos2(150.0, 70.0))
            .show(ctx, |ui| self.contents(ui, shown, out));
        if !open {
            out.push(Message::ShowPanel(None));
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, shown: &SheetTemplates, out: &mut Vec<Message>) {
        let mut templates = shown.templates.clone();
        self.selected = self.selected.min(templates.len().saturating_sub(1));
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(170.0);
                ui.label(
                    egui::RichText::new(format!(
                        "What new drawings start from; kept in {} beside the parts",
                        shown.file
                    ))
                    .size(11.0)
                    .color(color::TEXT_WEAK),
                );
                ui.add_space(4.0);
                for (k, template) in templates.iter().enumerate() {
                    if ui
                        .selectable_label(self.selected == k, &template.name)
                        .clicked()
                    {
                        self.selected = k;
                        self.picked = Picked::Nothing;
                    }
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("A new template").clicked() {
                        let name = Templates {
                            templates: templates.clone(),
                        }
                        .fresh_name();
                        templates.push(Template::three_views(&name, Format::A3, false));
                        self.selected = templates.len() - 1;
                        self.picked = Picked::Nothing;
                    }
                    let any = !templates.is_empty();
                    if ui
                        .add_enabled(any, egui::Button::new("Copy"))
                        .on_hover_text("Another like the one chosen")
                        .clicked()
                    {
                        let mut copy = templates[self.selected].clone();
                        copy.name = (2..)
                            .map(|n| format!("{} {n}", copy.name))
                            .find(|name| templates.iter().all(|t| t.name != *name))
                            .unwrap_or_default();
                        templates.push(copy);
                        self.selected = templates.len() - 1;
                    }
                    if ui
                        .add_enabled(templates.len() > 1, egui::Button::new("Remove"))
                        .clicked()
                    {
                        templates.remove(self.selected);
                        self.selected = self.selected.min(templates.len().saturating_sub(1));
                        self.picked = Picked::Nothing;
                    }
                });
            });
            ui.separator();
            let taken: Vec<String> = templates.iter().map(|t| t.name.clone()).collect();
            let k = self.selected;
            if let Some(template) = templates.get_mut(k) {
                ui.vertical(|ui| {
                    self.sheet_row(ui, k, template, &taken);
                    ui.add_space(4.0);
                    self.picture(ui, template);
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(230.0);
                    self.inspector(ui, k, template, shown);
                });
            }
        });
        if templates != shown.templates {
            out.push(Message::SetTemplates { templates });
        }
    }

    /// The row above the picture: the template's name and its sheet,
    /// and how much of it the picture shows.
    fn sheet_row(
        &mut self,
        ui: &mut egui::Ui,
        k: usize,
        template: &mut Template,
        taken: &[String],
    ) {
        ui.horizontal(|ui| {
            let id = egui::Id::new(("template-name", k));
            if let Some(name) = field(ui, id, &template.name, 170.0) {
                let name = name.trim();
                if !name.is_empty() && taken.iter().all(|t| t != name) {
                    template.name = name.to_owned();
                }
            }
            egui::ComboBox::from_id_salt(("template-format", k))
                .selected_text(template.format.label())
                .width(46.0)
                .show_ui(ui, |ui| {
                    for format in Format::ALL {
                        ui.selectable_value(&mut template.format, format, format.label());
                    }
                });
            let lies = if template.upright {
                "Portrait"
            } else {
                "Landscape"
            };
            egui::ComboBox::from_id_salt(("template-upright", k))
                .selected_text(lies)
                .width(84.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut template.upright, false, "Landscape");
                    ui.selectable_value(&mut template.upright, true, "Portrait");
                });
            let angle = match template.projection {
                Projection::FirstAngle => "First angle",
                Projection::ThirdAngle => "Third angle",
            };
            egui::ComboBox::from_id_salt(("template-projection", k))
                .selected_text(angle)
                .width(88.0)
                .show_ui(ui, |ui| {
                    let p = &mut template.projection;
                    ui.selectable_value(p, Projection::FirstAngle, "First angle");
                    ui.selectable_value(p, Projection::ThirdAngle, "Third angle");
                });
            // How its dimensions stand: what dragging them snaps to.
            let spacing = &mut template.spacing;
            ui.add(
                egui::DragValue::new(&mut spacing.gap)
                    .speed(0.1)
                    .range(0.0..=20.0)
                    .prefix("Gap ")
                    .suffix(" mm"),
            )
            .on_hover_text("How far an extension line starts off the outline: where dragging its end snaps to");
            ui.add(
                egui::DragValue::new(&mut spacing.pitch)
                    .speed(0.1)
                    .range(1.0..=50.0)
                    .prefix("Pitch ")
                    .suffix(" mm"),
            )
            .on_hover_text("How far apart dimension lines stand stacked over each other, and the first off the outline: where dragging a dimension snaps to");
            ui.add_space(12.0);
            for (zoom, label, hint) in [
                (
                    Zoom::Sheet,
                    "Whole sheet",
                    "Drag the views to where they stand",
                ),
                (
                    Zoom::Block,
                    "Title block",
                    "Work on the cells of the title block",
                ),
            ] {
                if ui
                    .selectable_label(self.zoom == zoom, label)
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.zoom = zoom;
                    self.picked = Picked::Nothing;
                }
            }
        });
    }

    /// The sheet as it will look, to work on.
    fn picture(&mut self, ui: &mut egui::Ui, template: &mut Template) {
        let [w, h] = template.size();
        let [bw, bh] = block_size(&template.block);
        // The block's lower left corner on the sheet.
        let (bx, by) = (w - MARGIN - bw, MARGIN);
        // What of the sheet shows, in millimetres.
        let (x0, y0, x1, y1) = match self.zoom {
            Zoom::Sheet => (0.0, 0.0, w, h),
            Zoom::Block => (bx - 6.0, by - 6.0, w - MARGIN + 6.0, by + bh + 8.0),
        };
        let (wide, high) = (640.0_f32, 452.0_f32);
        let scale = (wide / (x1 - x0) as f32).min(high / (y1 - y0) as f32);
        let size = egui::vec2(wide, high);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        // (Around the whole sheet, the desk it lies on; close to its
        // title block, only the sheet.)
        let around = match self.zoom {
            Zoom::Sheet => color::CANVAS,
            Zoom::Block => color::PANEL,
        };
        painter.rect_filled(rect, 4.0, around);
        // (Centred in the space there is.)
        let shown = egui::vec2((x1 - x0) as f32, (y1 - y0) as f32) * scale;
        let origin = rect.center() + egui::vec2(-shown.x, shown.y) / 2.0;
        let at = |x: f64, y: f64| origin + egui::vec2((x - x0) as f32, -((y - y0) as f32)) * scale;
        let boxed =
            |x: f64, y: f64, w: f64, h: f64| egui::Rect::from_two_pos(at(x, y), at(x + w, y + h));
        let thin = egui::Stroke::new(1.0, color::TEXT);
        let weak = egui::Stroke::new(1.0, color::BORDER);
        painter.rect_filled(boxed(0.0, 0.0, w, h), 0.0, color::PANEL);
        painter.rect_stroke(boxed(0.0, 0.0, w, h), 0.0, weak, egui::StrokeKind::Inside);
        let frame = boxed(MARGIN, MARGIN, w - 2.0 * MARGIN, h - 2.0 * MARGIN);
        painter.rect_stroke(frame, 0.0, thin, egui::StrokeKind::Inside);

        // The views, where the whole sheet shows.
        if self.zoom == Zoom::Sheet {
            for n in 0..template.views.len() {
                let [cx, cy] = view_at(template, n, 0);
                let area = boxed(cx - VIEW[0] / 2.0, cy - VIEW[1] / 2.0, VIEW[0], VIEW[1]);
                let projected = matches!(template.views[n].kind, ViewKind::Projected { .. });
                let id = ui.id().with(("template-view-box", n));
                let response = ui.interact(area, id, egui::Sense::click_and_drag());
                if response.clicked() || response.drag_started() {
                    self.picked = Picked::View(n);
                }
                if response.dragged() && !projected {
                    let by = response.drag_delta() / scale;
                    let view = &mut template.views[n];
                    view.at[0] = (view.at[0] + by.x as f64).clamp(MARGIN, w - MARGIN);
                    view.at[1] = (view.at[1] - by.y as f64).clamp(MARGIN, h - MARGIN);
                }
                let response = if projected {
                    response
                        .on_hover_text("In line with the view it is projected from: drag that one")
                } else {
                    response.on_hover_cursor(egui::CursorIcon::Grab)
                };
                let on = self.picked == Picked::View(n);
                let ink = if on { color::ACCENT } else { color::TEXT_WEAK };
                let fill = if on || response.hovered() {
                    color::ACCENT.gamma_multiply(0.10)
                } else {
                    color::HOVER
                };
                if let ViewKind::Projected { of, .. } = &template.views[n].kind
                    && let Some(from) = template.views.iter().position(|v| v.id == *of)
                {
                    let [fx, fy] = view_at(template, from, 0);
                    painter.line_segment(
                        [at(fx, fy), at(cx, cy)],
                        egui::Stroke::new(1.0, color::BORDER),
                    );
                }
                painter.rect_filled(area, 3.0, fill);
                painter.rect_stroke(
                    area,
                    3.0,
                    egui::Stroke::new(1.0, ink),
                    egui::StrokeKind::Inside,
                );
                let view = &template.views[n];
                painter.text(
                    area.center() - egui::vec2(0.0, 7.0),
                    egui::Align2::CENTER_CENTER,
                    kind_label(view),
                    egui::FontId::proportional(12.5),
                    color::TEXT,
                );
                painter.text(
                    area.center() + egui::vec2(0.0, 9.0),
                    egui::Align2::CENTER_CENTER,
                    &view.id,
                    egui::FontId::proportional(10.5),
                    color::TEXT_WEAK,
                );
            }
        }

        // The title block: its cells.
        let dropping = egui::DragAndDrop::has_payload_of_type::<Content>(ui.ctx());
        for n in 0..template.block.len() {
            let cell = &template.block[n];
            let area = boxed(bx + cell.at[0], by + cell.at[1], cell.size[0], cell.size[1]);
            let id = ui.id().with(("template-cell-box", n));
            let response = ui.interact(area, id, egui::Sense::click());
            if response.clicked() {
                // (Small as it is on the whole sheet, it is worked on
                // large.)
                self.zoom = Zoom::Block;
                self.picked = Picked::Cell(n);
            }
            if let Some(dropped) = response.dnd_release_payload::<Content>() {
                template.block[n].content = (*dropped).clone();
                self.picked = Picked::Cell(n);
            }
            let cell = &template.block[n];
            let on = self.picked == Picked::Cell(n);
            let over = response.hovered() || (dropping && response.contains_pointer());
            if on || over {
                let tint = if dropping { 0.22 } else { 0.10 };
                painter.rect_filled(area, 0.0, color::ACCENT.gamma_multiply(tint));
            }
            painter.rect_stroke(area, 0.0, thin, egui::StrokeKind::Middle);
            if self.zoom == Zoom::Block {
                painter.text(
                    area.left_top() + egui::vec2(3.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    &cell.label,
                    egui::FontId::proportional((1.6 * scale).clamp(7.0, 11.0)),
                    color::TEXT_WEAK,
                );
                let (says, ink) = match &cell.content {
                    Content::Text(text) => (text.clone(), color::TEXT),
                    Content::Property(name) => (format!("‹{name}›"), color::ACCENT),
                };
                let letters = (cell.height as f32 * scale).clamp(8.0, 26.0);
                painter.with_clip_rect(area.intersect(rect)).text(
                    area.left_bottom() + egui::vec2(4.0, -3.0),
                    egui::Align2::LEFT_BOTTOM,
                    says,
                    egui::FontId::proportional(letters),
                    ink,
                );
            }
            if on {
                let stroke = egui::Stroke::new(2.0, color::ACCENT);
                painter.rect_stroke(area, 0.0, stroke, egui::StrokeKind::Inside);
            }
        }

        // Its borders, to drag: every cell edge on a border goes with it.
        if self.zoom == Zoom::Block {
            let pointer = ui.ctx().pointer_latest_pos();
            for n in 0..template.block.len() {
                for (axis, far) in [(0, true), (0, false), (1, true), (1, false)] {
                    let cell = &template.block[n];
                    let coord = cell.at[axis] + if far { cell.size[axis] } else { 0.0 };
                    if coord < 1e-6 {
                        // (The block's own lower left corner stays.)
                        continue;
                    }
                    let grip = 3.0 / scale as f64;
                    let area = if axis == 0 {
                        boxed(
                            bx + coord - grip,
                            by + cell.at[1] + grip,
                            2.0 * grip,
                            cell.size[1] - 2.0 * grip,
                        )
                    } else {
                        boxed(
                            bx + cell.at[0] + grip,
                            by + coord - grip,
                            cell.size[0] - 2.0 * grip,
                            2.0 * grip,
                        )
                    };
                    let id = ui.id().with(("template-border", n, axis, far));
                    let cursor = if axis == 0 {
                        egui::CursorIcon::ResizeHorizontal
                    } else {
                        egui::CursorIcon::ResizeVertical
                    };
                    let response = ui
                        .interact(area, id, egui::Sense::drag())
                        .on_hover_cursor(cursor);
                    if response.dragged()
                        && let Some(p) = pointer
                    {
                        ui.ctx().set_cursor_icon(cursor);
                        let to = p - origin;
                        let mm = if axis == 0 {
                            x0 + (to.x / scale) as f64 - bx
                        } else {
                            y0 - (to.y / scale) as f64 - by
                        };
                        // (To half millimetres.)
                        let new = (mm * 2.0).round() / 2.0;
                        move_border(&mut template.block, axis, coord, new);
                    }
                    if response.hovered() || response.dragged() {
                        let stroke = egui::Stroke::new(2.0, color::ACCENT);
                        let cell = &template.block[n];
                        let now = cell.at[axis] + if far { cell.size[axis] } else { 0.0 };
                        let line = if axis == 0 {
                            [
                                at(bx + now, by + cell.at[1]),
                                at(bx + now, by + cell.at[1] + cell.size[1]),
                            ]
                        } else {
                            [
                                at(bx + cell.at[0], by + now),
                                at(bx + cell.at[0] + cell.size[0], by + now),
                            ]
                        };
                        painter.line_segment(line, stroke);
                    }
                }
            }
        }
        let hint = match self.zoom {
            Zoom::Sheet => "Drag a view to where it stands · click the title block to work on it",
            Zoom::Block => "Click a cell · drag a border · drop a block from the right into a cell",
        };
        ui.label(egui::RichText::new(hint).size(11.0).color(color::TEXT_WEAK));
    }

    /// What is selected, to change; and what there is to add.
    fn inspector(
        &mut self,
        ui: &mut egui::Ui,
        k: usize,
        template: &mut Template,
        shown: &SheetTemplates,
    ) {
        let heading = |ui: &mut egui::Ui, text: &str| {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).size(11.5).color(color::TEXT_WEAK));
        };
        let [w, h] = template.size();
        match self.zoom {
            Zoom::Sheet => {
                heading(ui, "Views a drawing starts with");
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button("+ View")
                        .on_hover_text("Another view of the part")
                        .clicked()
                    {
                        let id = fresh_view(template);
                        template.views.push(SheetView {
                            id,
                            kind: ViewKind::Base { side: Side::Front },
                            at: [w * 0.5, h * 0.55],
                            scale: None,
                            style: Style::HiddenRemoved,
                            bodies: Vec::new(),
                            cut_only: false,
                            half: None,
                            section: None,
                            caption_at: None,
                        });
                        self.picked = Picked::View(template.views.len() - 1);
                    }
                });
                let Picked::View(n) = self.picked else {
                    ui.add_space(4.0);
                    ui.weak("Click a view in the picture to change what it shows.");
                    return;
                };
                if n >= template.views.len() {
                    self.picked = Picked::Nothing;
                    return;
                }
                heading(ui, &format!("View {}", template.views[n].id));
                let ids: Vec<String> = template.views.iter().map(|v| v.id.clone()).collect();
                let view = &mut template.views[n];
                let own = view.id.clone();
                ui.horizontal(|ui| {
                    ui.label("Shows");
                    egui::ComboBox::from_id_salt(("template-view", k, n))
                        .selected_text(kind_label(view))
                        .width(150.0)
                        .height(360.0)
                        .show_ui(ui, |ui| {
                            for side in Side::ALL {
                                let kind = ViewKind::Base { side };
                                if ui
                                    .selectable_label(view.kind == kind, side.label())
                                    .clicked()
                                {
                                    view.kind = kind;
                                }
                            }
                            for name in &shown.views {
                                let kind = ViewKind::Named { view: name.clone() };
                                let label = format!("Camera view {name}");
                                if ui.selectable_label(view.kind == kind, label).clicked() {
                                    view.kind = kind;
                                }
                            }
                        });
                });
                match &view.kind {
                    ViewKind::Projected { of, .. } => {
                        ui.weak(format!("Projected from {of}: it keeps in line with it."));
                    }
                    _ => {
                        ui.horizontal(|ui| {
                            ui.label("At");
                            let x = number(
                                ui,
                                egui::Id::new(("template-view-x", k, n)),
                                view.at[0].round(),
                                44.0,
                            );
                            let y = number(
                                ui,
                                egui::Id::new(("template-view-y", k, n)),
                                view.at[1].round(),
                                44.0,
                            );
                            if let Some(x) = x {
                                view.at[0] = x.clamp(0.0, w);
                            }
                            if let Some(y) = y {
                                view.at[1] = y.clamp(0.0, h);
                            }
                            ui.weak("mm");
                        });
                    }
                }
                heading(ui, "A view projected from it");
                let mut added = None;
                ui.horizontal_wrapped(|ui| {
                    for beside in [Beside::Left, Beside::Right, Beside::Above, Beside::Below] {
                        let there = template.views.iter().any(|v| {
                            matches!(&v.kind, ViewKind::Projected { of, beside: b } if *of == own && *b == beside)
                        });
                        let button = egui::Button::new(format!("{beside:?}"));
                        if ui.add_enabled(!there, button).clicked() {
                            added = Some(beside);
                        }
                    }
                });
                let _ = ids;
                if let Some(beside) = added {
                    let id = fresh_view(template);
                    let at = template.views[n].at;
                    template.views.push(SheetView {
                        id,
                        kind: ViewKind::Projected {
                            of: own.clone(),
                            beside,
                        },
                        at,
                        scale: None,
                        style: Style::HiddenRemoved,
                        bodies: Vec::new(),
                        cut_only: false,
                        half: None,
                        section: None,
                        caption_at: None,
                    });
                }
                ui.add_space(8.0);
                if ui.button("Remove this view").clicked() {
                    let id = template.views.remove(n).id;
                    // (What was projected from it goes with it.)
                    template.views.retain(
                        |v| !matches!(&v.kind, ViewKind::Projected { of, .. } if *of == id),
                    );
                    self.picked = Picked::Nothing;
                }
            }
            Zoom::Block => {
                heading(
                    ui,
                    "Blocks: drag one into a cell, or click it for the selected cell",
                );
                let mut put = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    let blocks = std::iter::once(("Text".to_owned(), Content::Text(String::new())))
                        .chain(
                            shown
                                .properties
                                .iter()
                                .map(|p| (format!("‹{p}›"), Content::Property(p.clone()))),
                        );
                    for (label, content) in blocks {
                        let property = matches!(content, Content::Property(_));
                        let text = egui::RichText::new(label).size(12.0).color(if property {
                            color::ACCENT
                        } else {
                            color::TEXT
                        });
                        let chip = egui::Button::new(text)
                            .fill(color::PANEL)
                            .stroke(egui::Stroke::new(1.0, color::BORDER))
                            .sense(egui::Sense::click_and_drag());
                        let response = ui.add(chip).on_hover_cursor(egui::CursorIcon::Grab);
                        if response.drag_started() {
                            egui::DragAndDrop::set_payload(ui.ctx(), content.clone());
                        }
                        if response.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        }
                        if response.clicked() {
                            put = Some(content);
                        }
                    }
                });
                if let (Some(content), Picked::Cell(n)) = (put, self.picked)
                    && let Some(cell) = template.block.get_mut(n)
                {
                    cell.content = content;
                }
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button("+ Row")
                        .on_hover_text("A row across the top of the block")
                        .clicked()
                    {
                        let [bw, bh] = block_size(&template.block);
                        template.block.push(Cell {
                            at: [0.0, bh],
                            size: [bw.max(60.0), 9.0],
                            label: "Cell".to_owned(),
                            content: Content::Text(String::new()),
                            height: 3.5,
                        });
                        self.picked = Picked::Cell(template.block.len() - 1);
                    }
                    if ui
                        .button("ISO 7200")
                        .on_hover_text("The title block of ISO 7200 again")
                        .clicked()
                    {
                        template.block = workpiece_ops_drawing::template::iso_block();
                        self.picked = Picked::Nothing;
                    }
                });
                let Picked::Cell(n) = self.picked else {
                    ui.add_space(4.0);
                    ui.weak("Click a cell in the picture to change it.");
                    return;
                };
                if n >= template.block.len() {
                    self.picked = Picked::Nothing;
                    return;
                }
                heading(ui, "The selected cell");
                let id = |what: &str| egui::Id::new(("template-cell", what, k, n));
                let cell = &mut template.block[n];
                egui::Grid::new(("template-cell-grid", k))
                    .num_columns(2)
                    .spacing([8.0, 5.0])
                    .show(ui, |ui| {
                        ui.label("Label");
                        if let Some(label) = field(ui, id("label"), &cell.label, 140.0) {
                            cell.label = label;
                        }
                        ui.end_row();
                        ui.label("Says");
                        match &mut cell.content {
                            Content::Text(text) => {
                                if let Some(typed) = field(ui, id("text"), text, 140.0) {
                                    *text = typed;
                                }
                            }
                            Content::Property(name) => {
                                let text = format!("‹{name}› of the part");
                                ui.label(egui::RichText::new(text).color(color::ACCENT));
                            }
                        }
                        ui.end_row();
                        ui.label("Letters");
                        ui.horizontal(|ui| {
                            if let Some(typed) = number(ui, id("t"), cell.height, 40.0)
                                && typed > 0.0
                            {
                                cell.height = typed;
                            }
                            ui.weak("mm high");
                        });
                        ui.end_row();
                        ui.label("Size");
                        ui.horizontal(|ui| {
                            let [wide, high] = &mut cell.size;
                            for (what, value) in [("w", wide), ("h", high)] {
                                if let Some(typed) = number(ui, id(what), *value, 40.0)
                                    && typed >= LEAST
                                {
                                    *value = typed;
                                }
                            }
                            ui.weak("mm");
                        });
                        ui.end_row();
                    });
                heading(ui, "Split it");
                let mut split = None;
                ui.horizontal(|ui| {
                    if ui.button("Left | Right").clicked() {
                        split = Some(0);
                    }
                    if ui.button("Top / Bottom").clicked() {
                        split = Some(1);
                    }
                });
                if let Some(axis) = split
                    && template.block[n].size[axis] >= 2.0 * LEAST
                {
                    let half = (template.block[n].size[axis]).round() / 2.0;
                    let mut other = template.block[n].clone();
                    other.label = "Cell".to_owned();
                    other.content = Content::Text(String::new());
                    let whole = template.block[n].size[axis];
                    if axis == 0 {
                        // The cell keeps the left; the new one is right of it.
                        template.block[n].size[0] = half;
                        other.at[0] += half;
                        other.size[0] = whole - half;
                    } else {
                        // The cell keeps the top; the new one is below it.
                        other.size[1] = whole - half;
                        template.block[n].at[1] += whole - half;
                        template.block[n].size[1] = half;
                    }
                    template.block.push(other);
                }
                heading(ui, "Merge it with the cell");
                let mut merged = None;
                ui.horizontal_wrapped(|ui| {
                    for (way, label) in ["right of it", "left of it", "above", "below"]
                        .into_iter()
                        .enumerate()
                    {
                        let with = neighbour(&template.block, n, way);
                        if ui
                            .add_enabled(with.is_some(), egui::Button::new(label))
                            .clicked()
                        {
                            merged = with.map(|with| (way, with));
                        }
                    }
                });
                if let Some((way, with)) = merged {
                    let other = template.block[with].clone();
                    let cell = &mut template.block[n];
                    let axis = if way < 2 { 0 } else { 1 };
                    cell.at[axis] = cell.at[axis].min(other.at[axis]);
                    cell.size[axis] += other.size[axis];
                    template.block.remove(with);
                    self.picked = Picked::Cell(if with < n { n - 1 } else { n });
                }
                ui.add_space(8.0);
                if ui.button("Remove this cell").clicked() {
                    template.block.remove(n);
                    self.picked = Picked::Nothing;
                }
            }
        }
    }
}

/// A name no view of `template` has yet.
fn fresh_view(template: &Template) -> String {
    (1..)
        .map(|n| format!("view{n}"))
        .find(|id| template.views.iter().all(|v| v.id != *id))
        .unwrap_or_default()
}
