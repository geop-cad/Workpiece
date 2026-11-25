//! The timeline: the active part's steps as a tree, the focus, and the
//! scrubber.
//!
//! A click selects a step, a double click edits it, and a second, slow
//! click on the selected step renames it (Enter or a click elsewhere
//! takes the name, Escape drops it). A step is dragged to another place
//! in its program, and removed with the × at the end of its row (or
//! Delete, for the selected step).

use eframe::egui;
use workpiece_ui::Message;
use workpiece_ui::view::{Icon, ProgramStep, REMOVE_STEP, Timeline};

use super::tip;
use crate::icons;
use crate::theme::color;

const ROW_HEIGHT: f32 = 26.0;
const INDENT: f32 = 16.0;

/// What the timeline remembers between frames.
#[derive(Clone, Default)]
struct State {
    /// The step being renamed, what is typed, and whether the field just
    /// opened (it takes the keyboard, its text selected).
    renaming: Option<(Vec<String>, String, bool)>,
    /// A selected step that was clicked again, and when: renamed if no
    /// second click makes a double click of it.
    clicked: Option<(Vec<String>, f64)>,
}

/// Draws the timeline.
pub fn show(ui: &mut egui::Ui, timeline: &Timeline, out: &mut Vec<Message>) {
    let Some(part) = &timeline.part else {
        ui.weak("No part open");
        return;
    };
    let memory = ui.id().with("timeline state");
    let mut state: State = ui.data(|d| d.get_temp(memory)).unwrap_or_default();
    if let Some((path, at)) = &state.clicked {
        let now = ui.input(|i| i.time);
        let wait = ui.ctx().options(|o| o.input_options.max_double_click_delay);
        if now - at > wait {
            let name = path.last().cloned().unwrap_or_default();
            state.renaming = Some((path.clone(), name, true));
            state.clicked = None;
        } else {
            ui.ctx().request_repaint();
        }
    }
    // The focus as breadcrumbs: each leads back out.
    ui.horizontal_wrapped(|ui| {
        let mut crumb = |ui: &mut egui::Ui, label: &str, path: Vec<String>, current: bool| {
            let text = egui::RichText::new(label).strong();
            let text = if current {
                text.color(color::ACCENT)
            } else {
                text
            };
            if ui.add(egui::Button::new(text).frame(false)).clicked() && !current {
                out.push(Message::Focus(path));
            }
        };
        crumb(ui, part, Vec::new(), timeline.focus.is_empty());
        for n in 1..=timeline.focus.len() {
            ui.weak("›");
            let current = n == timeline.focus.len();
            crumb(
                ui,
                &timeline.focus[n - 1],
                timeline.focus[..n].to_vec(),
                current,
            );
        }
    });
    ui.separator();

    let mut focus_rows: Vec<egui::Rect> = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            rows(ui, &timeline.steps, 0, out, &mut focus_rows, &mut state);
            if let Some(scrubber) = timeline.scrubber {
                scrub_bar(ui, scrubber.at, scrubber.of, &focus_rows, out);
            }
        });
    ui.data_mut(|d| d.insert_temp(memory, state));
}

/// Draws `steps` and their expanded children; the rectangle they cover.
/// Each step of the focused part adds the rectangle of its row *and* its
/// expanded children to `focus_rows`, so the scrubber goes below them.
fn rows(
    ui: &mut egui::Ui,
    steps: &[ProgramStep],
    depth: usize,
    out: &mut Vec<Message>,
    focus_rows: &mut Vec<egui::Rect>,
    state: &mut State,
) -> egui::Rect {
    let mut all = egui::Rect::NOTHING;
    // Each step's rectangle, with its children; and the step dragged.
    let mut rects: Vec<egui::Rect> = Vec::new();
    let mut moved: Option<(usize, bool)> = None;
    for (index, step) in steps.iter().enumerate() {
        let (mut rect, response) = row(ui, step, depth, out, state);
        if step.expanded
            && let Some(children) = &step.children
        {
            rect = rect.union(rows(ui, children, depth + 1, out, focus_rows, state));
        }
        if step.in_focus {
            focus_rows.push(rect);
        }
        let primary = egui::PointerButton::Primary;
        if response.dragged_by(primary) || response.drag_stopped_by(primary) {
            moved = Some((index, response.drag_stopped()));
        }
        rects.push(rect);
        all = all.union(rect);
    }
    // A dragged step goes between the steps of its program nearest the
    // pointer: a line shows where.
    let pointer = ui.input(|i| i.pointer.interact_pos());
    if let (Some((from, dropped)), Some(pointer)) = (moved, pointer) {
        state.clicked = None;
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let boundary = |k: usize| match rects.get(k) {
            Some(r) => r.top(),
            None => all.bottom(),
        };
        let nearest = (0..=rects.len())
            .min_by(|&a, &b| {
                (boundary(a) - pointer.y)
                    .abs()
                    .total_cmp(&(boundary(b) - pointer.y).abs())
            })
            .unwrap_or(from);
        let to = if nearest > from { nearest - 1 } else { nearest };
        if to != from {
            let y = boundary(nearest);
            let left = rects[from].left() + 4.0 + depth as f32 * INDENT;
            let stroke = egui::Stroke::new(2.0, color::TEXT);
            ui.painter().line_segment(
                [egui::pos2(left, y), egui::pos2(all.right() - 4.0, y)],
                stroke,
            );
            ui.painter().circle_stroke(egui::pos2(left, y), 3.0, stroke);
            if dropped {
                let path = steps[from].path.clone();
                out.push(Message::MoveOperation { path, to });
            }
        }
    }
    all
}

/// One step's row: its rectangle, and what the pointer does with it.
fn row(
    ui: &mut egui::Ui,
    step: &ProgramStep,
    depth: usize,
    out: &mut Vec<Message>,
    state: &mut State,
) -> (egui::Rect, egui::Response) {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::click_and_drag());
    // Over the row or one of its buttons.
    let over = ui.rect_contains_pointer(rect);
    let painter = ui.painter_at(rect);
    if response.dragged_by(egui::PointerButton::Primary) {
        painter.rect_filled(rect, 5.0, color::ACCENT.gamma_multiply(0.3));
    } else if step.editing {
        painter.rect_filled(rect, 5.0, color::ACCENT.gamma_multiply(0.14));
    } else if step.selected {
        painter.rect_filled(rect, 5.0, color::ACCENT.gamma_multiply(0.22));
    } else if step.focused {
        painter.rect_stroke(
            rect.shrink(0.5),
            5.0,
            egui::Stroke::new(1.0, color::ACCENT),
            egui::StrokeKind::Inside,
        );
    } else if over {
        painter.rect_filled(rect, 5.0, color::HOVER);
    }
    let dim = |c: egui::Color32| {
        if step.rolled_back {
            c.gamma_multiply(0.4)
        } else {
            c
        }
    };
    let mut x = rect.left() + 4.0 + depth as f32 * INDENT;
    let center_y = rect.center().y;

    // The disclosure arrow of a placed part.
    if step.children.is_some() {
        let arrow =
            egui::Rect::from_center_size(egui::pos2(x + 7.0, center_y), egui::vec2(14.0, 14.0));
        let image = icons::widget(icons::CHEVRON, 12.0, dim(color::TEXT_WEAK)).rotate(
            if step.expanded {
                std::f32::consts::FRAC_PI_2
            } else {
                0.0
            },
            egui::vec2(0.5, 0.5),
        );
        image.paint_at(ui, arrow.shrink(1.0));
        let toggle = ui.interact(arrow, response.id.with("toggle"), egui::Sense::click());
        if toggle.clicked() {
            out.push(Message::Toggle(step.path.clone()));
        }
    }
    x += 16.0;
    let icon_rect =
        egui::Rect::from_center_size(egui::pos2(x + 8.0, center_y), egui::vec2(16.0, 16.0));
    icons::widget(icons::image(step.icon), 16.0, dim(color::TEXT)).paint_at(ui, icon_rect);
    x += 22.0;
    let renaming = matches!(&state.renaming, Some((path, ..)) if *path == step.path);
    if renaming {
        rename(
            ui,
            step,
            rect.with_min_x(x - 4.0)
                .shrink2(egui::vec2(48.0, 2.0))
                .with_min_x(x - 4.0),
            out,
            state,
        );
    } else {
        let title = painter.text(
            egui::pos2(x, center_y),
            egui::Align2::LEFT_CENTER,
            &step.title,
            egui::FontId::proportional(13.5),
            dim(color::TEXT),
        );
        painter.text(
            egui::pos2(title.right() + 8.0, center_y),
            egui::Align2::LEFT_CENTER,
            &step.description,
            egui::FontId::proportional(12.0),
            dim(color::TEXT_WEAK),
        );
    }

    // The buttons at the end of the row are always there (so the pointer
    // can reach them); they show while the pointer is over the row,
    // themselves included.
    let mut right = rect.right() - 14.0;
    let button = egui::Rect::from_center_size(egui::pos2(right, center_y), egui::vec2(20.0, 20.0));
    let remove = ui.interact(button, response.id.with("remove"), egui::Sense::click());
    if over && !renaming {
        let tint = if remove.hovered() {
            color::ERROR
        } else {
            color::TEXT_WEAK
        };
        ui.painter().text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::proportional(16.0),
            tint,
        );
    }
    let hint = "Delete removes the selected step";
    if remove
        .on_hover_text(tip("Delete step", Some(REMOVE_STEP), hint))
        .clicked()
    {
        out.push(Message::RemoveOperation(step.path.clone()));
    }
    right -= 22.0;

    // A placed part can be focused.
    if step.children.is_some() {
        let button =
            egui::Rect::from_center_size(egui::pos2(right, center_y), egui::vec2(20.0, 20.0));
        let focus = ui.interact(button, response.id.with("focus"), egui::Sense::click());
        if over || step.focused {
            let tint = if focus.hovered() || step.focused {
                color::ACCENT
            } else {
                color::TEXT_WEAK
            };
            icons::widget(icons::image(Icon::Focus), 16.0, tint).paint_at(ui, button.shrink(2.0));
        }
        if focus.on_hover_text("Focus this part").clicked() {
            out.push(Message::Focus(step.path.clone()));
        }
    }
    if response.double_clicked() {
        state.clicked = None;
        state.renaming = None;
        out.push(Message::Edit(step.path.clone()));
    } else if response.clicked() {
        if step.selected {
            // Again, slowly: its name is typed over.
            let now = ui.input(|i| i.time);
            state.clicked = Some((step.path.clone(), now));
        } else {
            state.clicked = None;
            out.push(Message::SelectStep(Some(step.path.clone())));
        }
    }
    (rect, response)
}

/// The field a step's new name is typed in, in place of its name.
fn rename(
    ui: &mut egui::Ui,
    step: &ProgramStep,
    rect: egui::Rect,
    out: &mut Vec<Message>,
    state: &mut State,
) {
    let Some((_, text, fresh)) = &mut state.renaming else {
        return;
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let mut output = egui::TextEdit::singleline(text)
        .desired_width(rect.width())
        .show(&mut child);
    if std::mem::take(fresh) {
        output.response.request_focus();
        let all = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text.chars().count()),
        );
        output.state.cursor.set_char_range(Some(all));
        output.state.store(ui.ctx(), output.response.id);
    }
    if output.response.lost_focus() {
        // Enter, or a click elsewhere, takes the name; Escape drops it.
        let taken = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.pointer.any_pressed());
        let name = text.trim().to_owned();
        if taken && !name.is_empty() && name != step.title {
            out.push(Message::RenameOperation {
                path: step.path.clone(),
                name,
            });
        }
        state.renaming = None;
    }
}

/// The bar after the last shown step of the focused part, dragged to go
/// back in time.
fn scrub_bar(ui: &mut egui::Ui, at: usize, of: usize, rows: &[egui::Rect], out: &mut Vec<Message>) {
    // Boundary `k` lies above row `k` (below the last row for `k = of`).
    let boundary = |k: usize| -> Option<f32> {
        match rows.get(k) {
            Some(r) => Some(r.top()),
            None => rows.last().map(|r| r.bottom()),
        }
    };
    let Some(y) = boundary(at) else {
        return;
    };
    let Some(first) = rows.first() else {
        return;
    };
    let line = egui::Rect::from_min_max(
        egui::pos2(first.left(), y - 4.0),
        egui::pos2(first.right(), y + 4.0),
    );
    let response = ui
        .interact(line, ui.id().with("scrubber"), egui::Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeVertical)
        .on_hover_text("Drag to go back in time");
    let painter = ui.painter();
    let active = response.hovered() || response.dragged();
    let stroke = egui::Stroke::new(if active { 3.0 } else { 2.0 }, color::ACCENT);
    painter.line_segment(
        [
            egui::pos2(line.left() + 4.0, y),
            egui::pos2(line.right() - 4.0, y),
        ],
        stroke,
    );
    painter.circle_filled(egui::pos2(line.left() + 4.0, y), 4.0, color::ACCENT);
    if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let nearest = (0..=of.min(rows.len()))
            .filter_map(|k| Some((k, (boundary(k)? - pointer.y).abs())))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k);
        if let Some(k) = nearest
            && k != at
        {
            out.push(Message::Scrub((k < of).then_some(k)));
        }
    }
}
