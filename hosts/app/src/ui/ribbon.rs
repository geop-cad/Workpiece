//! The tool bar: undo and redo as small icons beside the File menu, and
//! the tools as icons with their names below.

use eframe::egui;
use shapecraft_cad_ui::Message;
use shapecraft_cad_ui::view::{Icon, Ribbon, Tool};

use super::tip;
use crate::icons;
use crate::theme::color;

/// Size of a tool button, in points.
const TOOL_SIZE: egui::Vec2 = egui::vec2(64.0, 52.0);
/// Size of a tool's icon, in points.
const ICON_SIZE: f32 = 22.0;

/// How many small tools stand above each other.
const SMALL_ROWS: usize = 3;
/// The width of a small tool, and the size of its icon, in points.
const SMALL_WIDTH: f32 = 104.0;
const SMALL_ICON: f32 = 13.0;

fn is_history(tool: &Tool) -> bool {
    matches!(tool.icon, Icon::Undo | Icon::Redo)
}

/// Draws undo and redo as small icon buttons (for the menu bar).
pub fn history(ui: &mut egui::Ui, ribbon: &Ribbon, out: &mut Vec<Message>) {
    for tool in ribbon.tools.iter().filter(|t| is_history(t)) {
        let image = icons::widget(icons::image(tool.icon), 16.0, color::TEXT);
        let response = ui
            .add_enabled(tool.enabled, egui::Button::image(image).frame(false))
            .on_hover_text(tip(&tool.button.label, tool.button.hotkey, tool.hint))
            .on_disabled_hover_text(tip(&tool.button.label, tool.button.hotkey, "Nothing to do"));
        if response.clicked() {
            out.push(tool.button.click());
        }
    }
}

/// Draws the tools (but undo and redo) in a row, each an icon with its
/// name below.
pub fn show(ui: &mut egui::Ui, ribbon: &Ribbon, out: &mut Vec<Message>) {
    // The tabs, as tabs: a strip in the window's own grey, the tab that
    // shows a white card with rounded shoulders standing on the tools
    // it holds, the others names on the strip.
    const TAB_HEIGHT: f32 = 26.0;
    let strip = {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), TAB_HEIGHT),
            egui::Sense::hover(),
        );
        // (Out to the panel's edges, past its margin.)
        rect.expand2(egui::vec2(10.0, 0.0))
    };
    let painter = ui.painter().clone();
    painter.rect_filled(strip, 0.0, color::CANVAS);
    painter.line_segment(
        [strip.left_bottom(), strip.right_bottom()],
        egui::Stroke::new(1.0, color::BORDER),
    );
    let font = egui::FontId::proportional(12.5);
    let mut left = strip.left() + 10.0;
    for tab in &ribbon.tabs {
        let name = painter.layout_no_wrap(tab.button.label.clone(), font.clone(), color::TEXT);
        let rect = egui::Rect::from_min_size(
            egui::pos2(left, strip.top() + 3.0),
            egui::vec2(name.size().x + 24.0, TAB_HEIGHT - 3.0),
        );
        left = rect.right() + 2.0;
        let response = ui
            .interact(
                rect,
                ui.id().with(("tab", &tab.button.label)),
                egui::Sense::click(),
            )
            .on_hover_text(tip(
                &tab.button.label,
                tab.button.hotkey,
                "Show this tab's tools",
            ));
        let corners = egui::CornerRadius {
            nw: 6,
            ne: 6,
            sw: 0,
            se: 0,
        };
        let text = if tab.active {
            // Open at the bottom: one with the tools below.
            painter.rect_filled(rect, corners, color::PANEL);
            let edge = egui::Stroke::new(1.0, color::BORDER);
            painter.line_segment(
                [rect.left_bottom(), rect.left_top() + egui::vec2(0.0, 5.0)],
                edge,
            );
            painter.line_segment(
                [rect.right_bottom(), rect.right_top() + egui::vec2(0.0, 5.0)],
                edge,
            );
            painter.line_segment(
                [
                    rect.left_top() + egui::vec2(5.0, 0.0),
                    rect.right_top() - egui::vec2(5.0, 0.0),
                ],
                egui::Stroke::new(2.0, color::ACCENT),
            );
            painter.line_segment(
                [
                    rect.left_bottom() + egui::vec2(1.0, 0.0),
                    rect.right_bottom() - egui::vec2(1.0, 0.0),
                ],
                egui::Stroke::new(1.5, color::PANEL),
            );
            color::TEXT
        } else if response.hovered() {
            painter.rect_filled(rect.shrink2(egui::vec2(0.0, 2.0)), 5.0, color::ACTIVE);
            color::TEXT
        } else {
            color::TEXT_WEAK
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &tab.button.label,
            font.clone(),
            text,
        );
        if response.clicked() && !tab.active {
            out.push(tab.button.click());
        }
    }
    ui.add_space(2.0);
    // The tools, a thin line between two groups of them; scrolled
    // sideways where the window is too narrow for them all.
    egui::ScrollArea::horizontal()
        .id_salt("ribbon-tools")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let tools: Vec<&Tool> = ribbon.tools.iter().filter(|t| !is_history(t)).collect();
                let mut group = None;
                let mut k = 0;
                while k < tools.len() {
                    let tool = tools[k];
                    if group.is_some_and(|g| g != tool.group) {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(9.0, TOOL_SIZE.y),
                            egui::Sense::hover(),
                        );
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.center().x, rect.top() + 8.0),
                                egui::pos2(rect.center().x, rect.bottom() - 8.0),
                            ],
                            egui::Stroke::new(1.0, color::TEXT_WEAK.gamma_multiply(0.35)),
                        );
                    }
                    group = Some(tool.group);
                    if !tool.small {
                        let hint = tip(&tool.button.label, tool.button.hotkey, tool.hint);
                        if tool_button(ui, tool).on_hover_text(hint).clicked() {
                            out.push(tool.button.click());
                        }
                        k += 1;
                        continue;
                    }
                    // The small ones of a group: three above each other.
                    let mut column = vec![tool];
                    while column.len() < SMALL_ROWS
                        && let Some(next) = tools.get(k + column.len())
                        && next.small
                        && next.group == tool.group
                    {
                        column.push(next);
                    }
                    k += column.len();
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for tool in column {
                            let hint = tip(&tool.button.label, tool.button.hotkey, tool.hint);
                            if small_button(ui, tool).on_hover_text(hint).clicked() {
                                out.push(tool.button.click());
                            }
                        }
                    });
                }
            });
        });
}

/// One small tool: its icon and its name beside it, a third of the
/// ribbon's height.
fn small_button(ui: &mut egui::Ui, tool: &Tool) -> egui::Response {
    let size = egui::vec2(SMALL_WIDTH, TOOL_SIZE.y / SMALL_ROWS as f32);
    let sense = if tool.enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let fill = if response.is_pointer_button_down_on() {
            color::ACTIVE
        } else if response.hovered() && tool.enabled {
            color::HOVER
        } else {
            egui::Color32::TRANSPARENT
        };
        painter.rect_filled(rect, 4.0, fill);
        let tint = if tool.enabled {
            color::TEXT
        } else {
            color::TEXT_WEAK.gamma_multiply(0.6)
        };
        let icon = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 4.0 + SMALL_ICON / 2.0, rect.center().y),
            egui::vec2(SMALL_ICON, SMALL_ICON),
        );
        icons::widget(icons::image(tool.icon), SMALL_ICON, tint).paint_at(ui, icon);
        painter.text(
            egui::pos2(icon.right() + 5.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &tool.button.label,
            egui::FontId::proportional(11.5),
            tint,
        );
    }
    response
}

/// One tool: its icon, its name below, a rounded hover background.
fn tool_button(ui: &mut egui::Ui, tool: &Tool) -> egui::Response {
    let sense = if tool.enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    // As wide as its name needs, where that is longer than most.
    let name = ui.painter().layout_no_wrap(
        tool.button.label.clone(),
        egui::FontId::proportional(11.5),
        color::TEXT,
    );
    let size = egui::vec2(TOOL_SIZE.x.max(name.size().x + 8.0), TOOL_SIZE.y);
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let fill = if response.is_pointer_button_down_on() {
            color::ACTIVE
        } else if response.hovered() && tool.enabled {
            color::HOVER
        } else {
            egui::Color32::TRANSPARENT
        };
        painter.rect_filled(rect, 6.0, fill);
        let tint = if tool.enabled {
            color::TEXT
        } else {
            color::TEXT_WEAK.gamma_multiply(0.6)
        };
        let icon = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 6.0 + ICON_SIZE / 2.0),
            egui::vec2(ICON_SIZE, ICON_SIZE),
        );
        icons::widget(icons::image(tool.icon), ICON_SIZE, tint).paint_at(ui, icon);
        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 5.0),
            egui::Align2::CENTER_BOTTOM,
            &tool.button.label,
            egui::FontId::proportional(11.5),
            tint,
        );
    }
    response
}
