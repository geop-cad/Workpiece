//! The popup that edits a step.

use eframe::egui;
use shapecraft_cad_ui::Message;
use shapecraft_cad_ui::view::{EditDialog, describe_source};
use shapecraft_core::widgets::{Mood, ToolButton};
use shapecraft_core::{Axis, Widget};

use super::tip;
use crate::icons;
use crate::theme::color;

/// The least width of the popup, in points.
const DIALOG_WIDTH: f32 = 260.0;

/// Draws `dialog` at the top right of `area`, below the orientation cube.
/// A click on a file field asks for a file dialog for it: its widget path
/// lands in `choose_file`.
pub fn show(
    ctx: &egui::Context,
    area: egui::Rect,
    dialog: &EditDialog,
    out: &mut Vec<Message>,
    choose_file: &mut Option<Vec<usize>>,
) {
    window(ctx, area, |ui| {
        ui.horizontal(|ui| {
            ui.add(icons::widget(icons::image(dialog.icon), 20.0, color::TEXT));
            ui.heading(&dialog.title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (icon, hint) = if dialog.preview {
                    (
                        icons::EYE,
                        "On: the step shows applied as it changes. Turn off for large parts.",
                    )
                } else {
                    (
                        icons::EYE_OFF,
                        "Off: the step applies when the dialog closes.",
                    )
                };
                let hint = tip(
                    &dialog.toggle_preview.label,
                    dialog.toggle_preview.hotkey,
                    hint,
                );
                let tint = if dialog.preview {
                    color::ACCENT
                } else {
                    color::TEXT_WEAK
                };
                let button = egui::Button::image(icons::widget(icon, 16.0, tint)).frame(false);
                if ui.add(button).on_hover_text(hint).clicked() {
                    out.push(dialog.toggle_preview.click());
                }
            });
        });
        ui.separator();
        let mut fields = Fields {
            picking: dialog.picking.as_deref(),
            out: &mut *out,
            choose_file: &mut *choose_file,
        };
        fields.widget(ui, &dialog.fields, &mut Vec::new());
        if let Some(error) = &dialog.error {
            ui.add_space(4.0);
            ui.colored_label(color::ERROR, error);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let label = egui::RichText::new(&dialog.finish.label).color(egui::Color32::WHITE);
            let finish = egui::Button::new(label).fill(color::ACCENT);
            let hint = tip(
                &dialog.finish.label,
                dialog.finish.hotkey,
                "Keep the changes",
            );
            if ui.add(finish).on_hover_text(hint).clicked() {
                out.push(dialog.finish.click());
            }
            let hint = tip(
                &dialog.cancel.label,
                dialog.cancel.hotkey,
                "Undo the changes",
            );
            if ui
                .button(&dialog.cancel.label)
                .on_hover_text(hint)
                .clicked()
            {
                out.push(dialog.cancel.click());
            }
        });
    });
}

/// Draws the panel of what is selected in a step whose dialog is in the
/// ribbon (see [`EditDialog::inspector`]), where other steps' dialogs
/// show: at the top right of `area`.
pub fn inspector(
    ctx: &egui::Context,
    area: egui::Rect,
    dialog: &EditDialog,
    out: &mut Vec<Message>,
    choose_file: &mut Option<Vec<usize>>,
) {
    let Some(inspector) = &dialog.inspector else {
        return;
    };
    window(ctx, area, |ui| {
        let mut fields = Fields {
            // (Nothing in it is picked in the model.)
            picking: None,
            out: &mut *out,
            choose_file: &mut *choose_file,
        };
        // (Its own widget paths, apart from the ribbon's.)
        let mut path = vec![usize::MAX];
        let rows: &[Widget<Message>] = match inspector {
            Widget::VBox(b) => &b.children,
            other => std::slice::from_ref(other),
        };
        for (i, row) in rows.iter().enumerate() {
            path.push(i);
            if i == 0
                && let Widget::Label(title) = row
            {
                ui.heading(&title.text);
                ui.separator();
            } else {
                fields.widget(ui, row, &mut path);
            }
            path.pop();
        }
    });
}

/// The window every dialog of a step shows in: at the top right of
/// `area`, below the orientation cube.
pub fn window(ctx: &egui::Context, area: egui::Rect, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new("edit-step")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .fixed_pos(area.right_top() + egui::vec2(-12.0, 150.0))
        .pivot(egui::Align2::RIGHT_TOP)
        .show(ctx, |ui| {
            ui.set_min_width(DIALOG_WIDTH);
            contents(ui);
        });
}

/// Draws fields, collecting what the user does.
struct Fields<'a> {
    picking: Option<&'a [usize]>,
    out: &'a mut Vec<Message>,
    choose_file: &'a mut Option<Vec<usize>>,
}

impl Fields<'_> {
    /// The button beside a measure that gives it a tolerance, of the
    /// kind chosen; its numbers are then typed in the field.
    fn tolerance(
        &mut self,
        ui: &mut egui::Ui,
        input: &shapecraft_core::widgets::NumericInput<Message>,
    ) {
        use shapecraft_core::world::Tolerance;
        let Some(now) = &input.tolerance else {
            return;
        };
        let toleranced = *now != Tolerance::General;
        let text = egui::RichText::new("±").color(if toleranced {
            color::TEXT
        } else {
            color::TEXT_WEAK
        });
        let kinds = [
            ("None (general tolerance)", Tolerance::General),
            ("± Symmetric", Tolerance::Symmetric(0.1)),
            (
                "Deviations",
                Tolerance::Deviations {
                    upper: 0.1,
                    lower: -0.1,
                },
            ),
            (
                "Limits of size",
                Tolerance::Limits {
                    upper: 0.1,
                    lower: -0.1,
                },
            ),
            ("Fit (ISO 286)", Tolerance::Fit("H7".to_owned())),
        ];
        let same =
            |a: &Tolerance, b: &Tolerance| std::mem::discriminant(a) == std::mem::discriminant(b);
        ui.menu_button(text, |ui| {
            for (label, kind) in &kinds {
                if ui.selectable_label(same(now, kind), *label).clicked() {
                    if !same(now, kind) {
                        self.out.extend(input.tolerate(&now.as_kind(kind)));
                        self.out.push(Message::EndEdit);
                    }
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Its tolerance, for the drawing");
    }

    /// Draws `w` (at widget path `path`) and its children.
    fn widget(&mut self, ui: &mut egui::Ui, w: &Widget<Message>, path: &mut Vec<usize>) {
        let caption = |ui: &mut egui::Ui, text: &str| {
            ui.label(egui::RichText::new(text).color(color::TEXT_WEAK));
        };
        match w {
            Widget::Button(b) => {
                if ui.button(&b.label).clicked() {
                    self.out.push(b.click());
                }
            }
            Widget::NumericInput(input) => {
                ui.horizontal(|ui| {
                    caption(ui, &input.label);
                    // (A field that takes the keyboard is typed into,
                    // what it shows selected.)
                    let focus_id = ui.id().with(("focus", path.clone()));
                    let fresh = match &input.focus {
                        Some(key) => {
                            let was = ui.data_mut(|d| d.get_temp::<String>(focus_id));
                            ui.data_mut(|d| d.insert_temp(focus_id, key.clone()));
                            was.as_ref() != Some(key)
                        }
                        None => {
                            ui.data_mut(|d| d.remove::<String>(focus_id));
                            false
                        }
                    };
                    let written = input.written.clone().or_else(|| {
                        input.focus.as_ref().map(|_| {
                            let text = format!("{:.4}", input.value);
                            text.trim_end_matches('0').trim_end_matches('.').to_owned()
                        })
                    });
                    if let Some(shown) = &written {
                        // A formula or a toleranced number is text:
                        // typed, and entered on leaving the field. A
                        // plain number makes it one again.
                        let id = ui.id().with(("formula", path.clone()));
                        let shown = shown.clone();
                        let mut text = ui
                            .data_mut(|d| d.get_temp::<String>(id))
                            .unwrap_or_else(|| shown.clone());
                        let edit = egui::TextEdit::singleline(&mut text).desired_width(96.0);
                        let mut output = edit.show(ui);
                        if fresh {
                            // Typing replaces what was there.
                            output.response.request_focus();
                            let all = egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(text.chars().count()),
                            );
                            output.state.cursor.set_char_range(Some(all));
                            output.state.store(ui.ctx(), output.response.id);
                        }
                        let response = output.response.response.clone().on_hover_text(format!(
                            "= {}. A number or =formula, and its tolerance after it: 10 ±0.1, 10 +0.2/-0.1, 10 H7",
                            input.value
                        ));
                        if response.has_focus() {
                            ui.data_mut(|d| d.insert_temp(id, text));
                        } else {
                            ui.data_mut(|d| d.remove::<String>(id));
                            if response.lost_focus() && text != shown {
                                self.out.extend(input.enter(&text));
                                self.out.push(Message::EndEdit);
                            }
                        }
                        self.tolerance(ui, input);
                        return;
                    }
                    // A number is dragged or typed; typing =name / 2 where
                    // the field takes formulas makes it one.
                    let mut value = input.value;
                    let typed = std::cell::RefCell::new(None);
                    let mut field = egui::DragValue::new(&mut value).speed(0.01).max_decimals(4);
                    if input.on_text.is_some() {
                        field = field.custom_parser(|text| {
                            let number = text.trim().parse::<f64>().ok();
                            if number.is_none() {
                                *typed.borrow_mut() = Some(text.to_owned());
                            }
                            // What is no number leaves the value as it is.
                            number.or(Some(input.value))
                        });
                    }
                    let mut response = ui.add(field);
                    if input.on_text.is_some() {
                        response = response.on_hover_text(
                            "A number, or a formula over the part's parameters: =width / 2. With its tolerance after it: 10 ±0.1, 10 +0.2/-0.1, 10 H7",
                        );
                    }
                    if let Some(text) = typed.into_inner() {
                        self.out.extend(input.enter(&text));
                    } else if response.changed()
                        && let Some(message) = input.set(value)
                    {
                        self.out.push(message);
                    }
                    if response.drag_stopped() || response.lost_focus() {
                        self.out.push(Message::EndEdit);
                    }
                    self.tolerance(ui, input);
                });
            }
            Widget::Choice(choice) => {
                // The options as buttons in one rounded track, the
                // chosen one raised; options that cannot be chosen are
                // greyed out.
                use shapecraft_core::widgets::Shown;
                // (Beside others in a row, a drop-down says what it
                // chooses by itself.)
                let row = ui.layout().main_dir().is_horizontal();
                // (Buttons are for two or three short options that fit
                // one line; anything more is a list to choose from.)
                let letters: usize = choice.options.iter().map(|o| o.chars().count() + 3).sum();
                let listed =
                    choice.shown == Shown::DropDown || choice.options.len() > 3 || letters > 40;
                if !(row && listed) {
                    caption(ui, &choice.label);
                }
                if choice.shown == Shown::Searched {
                    self.searched(ui, choice, path);
                } else if listed {
                    let selected = choice
                        .options
                        .get(choice.selected)
                        .cloned()
                        .unwrap_or_default();
                    let width = if row {
                        (DIALOG_WIDTH - 24.0) / 2.0
                    } else {
                        DIALOG_WIDTH - 16.0
                    };
                    egui::ComboBox::from_id_salt(("dialog-choice", path.clone()))
                        .selected_text(selected)
                        .width(width)
                        .height(320.0)
                        .show_ui(ui, |ui| {
                            for (i, option) in choice.options.iter().enumerate() {
                                let enabled = choice.enabled.get(i).copied().unwrap_or(false);
                                let item = egui::Button::selectable(i == choice.selected, option);
                                let response = ui
                                    .add_enabled(enabled, item)
                                    .on_disabled_hover_text(choice.reason(i).unwrap_or_default());
                                if response.clicked() && i != choice.selected {
                                    self.out.extend(choice.set(i));
                                    self.out.push(Message::EndEdit);
                                }
                            }
                        });
                } else {
                    egui::Frame::new()
                        .fill(color::HOVER)
                        .stroke(egui::Stroke::new(1.0, color::BORDER))
                        .corner_radius(9)
                        .inner_margin(3)
                        .show(ui, |ui| {
                            ui.set_max_width(DIALOG_WIDTH - 8.0);
                            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                            ui.horizontal_wrapped(|ui| {
                                for (i, option) in choice.options.iter().enumerate() {
                                    let chosen = i == choice.selected;
                                    let enabled = choice.enabled.get(i).copied().unwrap_or(false);
                                    let text = egui::RichText::new(option);
                                    let button = if chosen {
                                        egui::Button::new(text.color(color::TEXT).strong())
                                            .fill(color::PANEL)
                                            .stroke(egui::Stroke::new(1.0, color::ACCENT))
                                    } else {
                                        egui::Button::new(text.color(color::TEXT_WEAK))
                                            .fill(egui::Color32::TRANSPARENT)
                                    };
                                    let why =
                                        choice.reason(i).unwrap_or("Not for these references");
                                    let response = ui
                                        .add_enabled(enabled, button.corner_radius(6))
                                        .on_disabled_hover_text(why);
                                    if response.clicked() && !chosen {
                                        self.out.extend(choice.set(i));
                                        self.out.push(Message::EndEdit);
                                    }
                                }
                            });
                        });
                }
            }
            Widget::ReferenceList(list) => {
                // The references as chips (× removes one) in a box; clicking
                // anywhere in the box turns picking on or off.
                let active = self.picking == Some(path.as_slice());
                caption(ui, &list.label);
                let builder = egui::UiBuilder::new().sense(egui::Sense::click());
                let boxed = ui.scope_builder(builder, |ui| {
                    let border = if active { color::ACCENT } else { color::BORDER };
                    let fill = if active {
                        color::ACCENT.gamma_multiply(0.06)
                    } else {
                        color::PANEL
                    };
                    egui::Frame::new()
                        .stroke(egui::Stroke::new(1.0, border))
                        .fill(fill)
                        .corner_radius(6)
                        .inner_margin(6)
                        .show(ui, |ui| {
                            ui.set_min_width(240.0);
                            if list.values.is_empty() {
                                ui.weak(if active {
                                    "Pick in the viewport…"
                                } else {
                                    "Click to pick"
                                });
                            }
                            ui.horizontal_wrapped(|ui| {
                                for (i, source) in list.values.iter().enumerate() {
                                    let chip = egui::Button::new(format!(
                                        "{}  ×",
                                        describe_source(source)
                                    ))
                                    .fill(color::HOVER)
                                    .corner_radius(10);
                                    if ui.add(chip).on_hover_text("Remove").clicked() {
                                        self.out.push(list.remove(i));
                                    }
                                }
                            });
                        });
                });
                let hint = if active {
                    "Picking: click points, axes or planes in the viewport (again to remove). Click here to stop."
                } else {
                    "Click to pick points, axes or planes in the viewport"
                };
                let response = boxed
                    .response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(hint);
                if response.clicked() {
                    self.out
                        .push(Message::Pick((!active).then(|| path.clone())));
                }
            }
            Widget::PoseInput(input) => {
                // Aligned columns: x, y, z; a row for the position, one for
                // the rotation's Euler angles.
                ui.label(egui::RichText::new(&input.label).color(color::TEXT_WEAK));
                egui::Grid::new(("pose", path.clone()))
                    .num_columns(4)
                    .spacing([6.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("");
                        for axis in ["X", "Y", "Z"] {
                            let header =
                                egui::Label::new(egui::RichText::new(axis).color(color::TEXT_WEAK));
                            ui.add_sized([64.0, 16.0], header);
                        }
                        ui.end_row();
                        let rows: [(&str, [f64; 3], f64, &str); 2] = [
                            ("Position", input.position(), 0.01, ""),
                            ("Rotation", input.angles(), 0.5, "°"),
                        ];
                        for (row, (label, values, speed, suffix)) in rows.into_iter().enumerate() {
                            caption(ui, label);
                            for (axis, value) in Axis::ALL.into_iter().zip(values) {
                                let mut v = value;
                                let field = egui::DragValue::new(&mut v)
                                    .speed(speed)
                                    .max_decimals(if row == 0 { 4 } else { 2 })
                                    .suffix(suffix);
                                let response = ui.add_sized([64.0, 20.0], field);
                                if response.changed() {
                                    let message = if row == 0 {
                                        input.set_position(axis, v)
                                    } else {
                                        input.set_angle(axis, v)
                                    };
                                    self.out.extend(message);
                                }
                                if response.drag_stopped() || response.lost_focus() {
                                    self.out.push(Message::EndEdit);
                                }
                            }
                            ui.end_row();
                        }
                    });
            }
            Widget::ReferenceInput(input) => {
                ui.horizontal(|ui| {
                    caption(ui, &input.label);
                    let active = self.picking == Some(path.as_slice());
                    let text = if active {
                        egui::RichText::new(format!("Pick a {}…", input.kind.name()))
                            .color(color::ACCENT)
                    } else {
                        // Empty, it says what that means, or asks.
                        let empty = || match &input.optional {
                            Some((empty, _)) => empty.clone(),
                            None => format!("Pick a {}", input.kind.name()),
                        };
                        match &input.value {
                            Some(source) => egui::RichText::new(describe_source(source)),
                            None => egui::RichText::new(empty()).color(color::TEXT_WEAK),
                        }
                    };
                    let button = egui::Button::new(text)
                        .selected(active)
                        .fill(color::HOVER)
                        .stroke(egui::Stroke::new(
                            1.0,
                            if active { color::ACCENT } else { color::BORDER },
                        ));
                    if ui
                        .add(button)
                        .on_hover_text("Click, then pick in the viewport")
                        .clicked()
                    {
                        self.out
                            .push(Message::Pick((!active).then(|| path.clone())));
                    }
                    // A field that may be empty is emptied with its ×.
                    if let Some(clear) = input.clear() {
                        let cross =
                            egui::Button::new(egui::RichText::new("×").color(color::TEXT_WEAK))
                                .frame(false);
                        if ui.add(cross).on_hover_text("Clear").clicked() {
                            self.out.extend([clear, Message::EndEdit]);
                        }
                    }
                });
            }
            Widget::ConnectorInput(input) => {
                ui.horizontal(|ui| {
                    caption(ui, &input.label);
                    let active = self.picking == Some(path.as_slice());
                    let text = match (&input.value, active) {
                        (_, true) => egui::RichText::new("Point at a part…").color(color::ACCENT),
                        (Some(on), false) => egui::RichText::new(on),
                        (None, false) => {
                            egui::RichText::new("Pick a connector").color(color::TEXT_WEAK)
                        }
                    };
                    let button = egui::Button::new(text)
                        .selected(active)
                        .fill(color::HOVER)
                        .stroke(egui::Stroke::new(
                            1.0,
                            if active { color::ACCENT } else { color::BORDER },
                        ));
                    let hint = "Click, then point at a vertex, an edge or a face of a placed \
                                part: the frame shown there is the connector";
                    if ui.add(button).on_hover_text(hint).clicked() {
                        self.out
                            .push(Message::Pick((!active).then(|| path.clone())));
                    }
                });
            }
            Widget::FileInput(input) => {
                ui.horizontal(|ui| {
                    caption(ui, &input.label);
                    let text = if input.value.is_empty() {
                        "Choose…"
                    } else {
                        input.value.as_str()
                    };
                    let image = icons::widget(icons::FOLDER, 14.0, color::TEXT);
                    if ui.add(egui::Button::image_and_text(image, text)).clicked() {
                        *self.choose_file = Some(path.clone());
                    }
                });
            }
            Widget::ToolButton(tool) => {
                let image = icons::widget(icons::named(tool.icon), 16.0, color::TEXT);
                let button = egui::Button::image_and_text(image, &tool.label).selected(tool.active);
                let response = ui
                    .add_enabled(tool.enabled(), button)
                    .on_hover_text(tip(&tool.label, Some(tool.hotkey), &tool.hint))
                    .on_disabled_hover_text(tool.disabled.as_deref().unwrap_or_default());
                if response.clicked() {
                    self.out.extend(tool.click());
                    self.out.push(Message::EndEdit);
                }
            }
            Widget::Group(group) => {
                caption(ui, &group.label);
                ui.vertical(|ui| self.children(ui, &group.children, path));
            }
            Widget::Label(label) if label.label.is_empty() => {
                // A note by itself: wrapped to the dialog's width.
                ui.scope(|ui| {
                    ui.set_max_width(DIALOG_WIDTH - 8.0);
                    let text = egui::RichText::new(&label.text).color(mood_color(label.mood));
                    ui.add(egui::Label::new(text).wrap());
                });
            }
            Widget::Label(label) => {
                ui.horizontal(|ui| {
                    caption(ui, &label.label);
                    ui.colored_label(mood_color(label.mood), &label.text);
                });
            }
            Widget::TextInput(input) => {
                ui.horizontal(|ui| {
                    caption(ui, &input.label);
                    let mut text = input.value.clone();
                    let response =
                        ui.add(egui::TextEdit::singleline(&mut text).desired_width(64.0));
                    if response.changed() {
                        self.out.push(input.set(text));
                    }
                    if response.lost_focus() {
                        self.out.push(Message::EndEdit);
                    }
                });
            }
            Widget::Check(check) => {
                let mut on = check.value;
                let hint = match &check.hotkey {
                    Some(key) => tip(&check.label, Some(*key), &check.hint),
                    None => check.hint.clone(),
                };
                let response = ui
                    .scope(|ui| {
                        // (Its box shows when it is off too.)
                        let stroke = egui::Stroke::new(1.0, color::TEXT_WEAK);
                        let visuals = &mut ui.visuals_mut().widgets;
                        for state in [&mut visuals.inactive, &mut visuals.hovered] {
                            state.bg_stroke = stroke;
                        }
                        ui.add_enabled(
                            check.disabled.is_none(),
                            egui::Checkbox::new(&mut on, &check.label),
                        )
                    })
                    .inner
                    .on_hover_text(hint)
                    .on_disabled_hover_text(check.disabled.clone().unwrap_or_default());
                if response.changed() {
                    self.out.extend(check.toggle());
                    self.out.push(Message::EndEdit);
                }
            }
            Widget::ColourInput(input) => {
                ui.horizontal(|ui| {
                    caption(ui, &input.label);
                    let was = input.value.unwrap_or([207, 210, 216]);
                    let mut colour = was;
                    ui.color_edit_button_srgb(&mut colour);
                    if colour != was {
                        self.out.push(input.set(Some(colour)));
                    }
                    if input.value.is_none() {
                        ui.weak("none");
                    } else if ui.small_button("×").on_hover_text("No colour").clicked() {
                        self.out.push(input.set(None));
                    }
                });
            }
            Widget::VBox(b) => {
                ui.vertical(|ui| self.children(ui, &b.children, path));
            }
            Widget::HBox(b) => {
                // In a row, a tool is its icon alone: a small button
                // beside the field it belongs to.
                ui.horizontal(|ui| {
                    for (i, child) in b.children.iter().enumerate() {
                        path.push(i);
                        match child {
                            Widget::ToolButton(tool) => self.small_tool(ui, tool),
                            other => self.widget(ui, other, path),
                        }
                        path.pop();
                    }
                });
            }
        }
    }

    /// A choice as a list that is searched by typing: the options that
    /// hold every word typed, to scroll through and click; Enter takes
    /// the first.
    fn searched(
        &mut self,
        ui: &mut egui::Ui,
        choice: &shapecraft_core::widgets::Choice<Message>,
        path: &[usize],
    ) {
        let id = egui::Id::new(("dialog-search", path.to_vec()));
        let mut search: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        let field = ui.add(
            egui::TextEdit::singleline(&mut search)
                .hint_text("Search…")
                .desired_width(DIALOG_WIDTH - 16.0),
        );
        let wanted = search.to_lowercase();
        let wanted: Vec<&str> = wanted.split_whitespace().collect();
        let found: Vec<usize> = (0..choice.options.len())
            .filter(|i| {
                let option = choice.options[*i].to_lowercase();
                wanted.iter().all(|word| option.contains(word))
            })
            .collect();
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let mut chosen = (entered && !wanted.is_empty())
            .then(|| found.first().copied())
            .flatten();
        ui.data_mut(|d| d.insert_temp(id, search));
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, color::BORDER))
            .fill(color::PANEL)
            .corner_radius(6)
            .inner_margin(4)
            .show(ui, |ui| {
                let width = DIALOG_WIDTH - 26.0;
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 1.0;
                let text = ui.text_style_height(&egui::TextStyle::Button);
                let row =
                    (text + 2.0 * ui.spacing().button_padding.y).max(ui.spacing().interact_size.y);
                // A list just shown, or one that is another list now
                // (searched, filtered), starts where what is chosen is
                // in it — in sight, not rows away — or at its top.
                let listed = (found.len(), found.first().copied(), found.last().copied());
                let seen = egui::Id::new(("dialog-search-listed", path.to_vec()));
                let mut area = egui::ScrollArea::vertical()
                    .id_salt(("dialog-search-list", path.to_vec()))
                    .max_height(168.0)
                    .min_scrolled_height(168.0)
                    .auto_shrink([false, false]);
                if ui.data(|d| d.get_temp(seen)) != Some(listed) {
                    ui.data_mut(|d| d.insert_temp(seen, listed));
                    let at = found.iter().position(|i| *i == choice.selected);
                    let offset = at.map_or(0.0, |k| (k as f32 - 2.0).max(0.0) * (row + 1.0));
                    area = area.vertical_scroll_offset(offset);
                }
                area.show_rows(ui, row, found.len(), |ui, rows| {
                    for i in &found[rows] {
                        let name = egui::RichText::new(&choice.options[*i]).size(12.0);
                        let item = egui::Button::selectable(*i == choice.selected, name)
                            .truncate()
                            .min_size(egui::vec2(width - 14.0, row));
                        if ui.add(item).clicked() {
                            chosen = Some(*i);
                        }
                    }
                });
                if found.is_empty() {
                    ui.weak("Nothing of that name");
                }
            });
        if let Some(i) = chosen.filter(|i| *i != choice.selected) {
            self.out.extend(choice.set(i));
            self.out.push(Message::EndEdit);
        }
    }

    /// A tool as a small icon button; its name, hotkey and what it does
    /// show when the pointer rests on it.
    fn small_tool(&mut self, ui: &mut egui::Ui, tool: &ToolButton<Message>) {
        let tint = if tool.active {
            color::ACCENT
        } else {
            color::TEXT
        };
        let image = icons::widget(icons::named(tool.icon), 14.0, tint);
        let button = egui::Button::image(image)
            .selected(tool.active)
            .fill(color::HOVER)
            .stroke(egui::Stroke::new(1.0, color::BORDER));
        let response = ui
            .add_enabled(tool.enabled(), button)
            .on_hover_text(tip(&tool.label, Some(tool.hotkey), &tool.hint))
            .on_disabled_hover_text(tool.disabled.as_deref().unwrap_or_default());
        if response.clicked() {
            self.out.extend(tool.click());
            self.out.push(Message::EndEdit);
        }
    }

    fn children(&mut self, ui: &mut egui::Ui, children: &[Widget<Message>], path: &mut Vec<usize>) {
        for (i, child) in children.iter().enumerate() {
            path.push(i);
            self.widget(ui, child, path);
            path.pop();
        }
    }
}

fn mood_color(mood: Mood) -> egui::Color32 {
    match mood {
        Mood::Plain => color::TEXT,
        Mood::Good => egui::Color32::from_rgb(46, 125, 80),
        Mood::Warning => egui::Color32::from_rgb(190, 120, 20),
        Mood::Error => color::ERROR,
    }
}

/// Size of a ribbon tool's icon button, in points.
const RIBBON_TOOL: f32 = 30.0;
/// The least width of a full-sized ribbon button, and its icon's size,
/// in points: as the main ribbon's tools.
const LARGE_WIDTH: f32 = 64.0;
const LARGE_ICON: f32 = 22.0;
/// The width a status in a ribbon wraps at, in points.
const RIBBON_NOTE: f32 = 120.0;
/// How many tools, and how many fields, stack in a ribbon group's column.
const TOOL_ROWS: usize = 2;
const FIELD_ROWS: usize = 3;
/// The height of a ribbon group: its rows of tools and its name.
const RIBBON_HEIGHT: f32 = TOOL_ROWS as f32 * (RIBBON_TOOL + 3.0) + 18.0;

/// Draws `dialog` as a ribbon, in place of the tools: its groups side by
/// side, each with its tools as icons and its fields stacked, and the
/// buttons that close it at the end. A click on a file field asks for a
/// file dialog, as in [`show`].
pub fn show_ribbon(
    ui: &mut egui::Ui,
    dialog: &EditDialog,
    out: &mut Vec<Message>,
    choose_file: &mut Option<Vec<usize>>,
) {
    ui.horizontal_top(|ui| {
        ui.set_min_height(RIBBON_HEIGHT);
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
        // Closing the ribbon comes first, so it never scrolls away.
        ui.vertical(|ui| {
            // As high as the tools' two rows, side by side.
            let size = egui::vec2(64.0, TOOL_ROWS as f32 * (RIBBON_TOOL + 3.0) - 3.0);
            ui.horizontal(|ui| {
                let label = egui::RichText::new(&dialog.finish.label)
                    .color(egui::Color32::WHITE)
                    .strong();
                let finish = egui::Button::new(label).fill(color::ACCENT);
                let hint = tip(
                    &dialog.finish.label,
                    dialog.finish.hotkey,
                    "Keep the changes",
                );
                if ui.add_sized(size, finish).on_hover_text(hint).clicked() {
                    out.push(dialog.finish.click());
                }
                let cancel = egui::Button::new(&dialog.cancel.label);
                let hint = tip(
                    &dialog.cancel.label,
                    dialog.cancel.hotkey,
                    "Undo the changes",
                );
                if ui.add_sized(size, cancel).on_hover_text(hint).clicked() {
                    out.push(dialog.cancel.click());
                }
            });
            ui.label(
                egui::RichText::new(&dialog.title)
                    .size(10.5)
                    .color(color::TEXT_WEAK),
            );
        });
        ui.separator();
        show_groups(ui, dialog, out, choose_file);
    });
}

/// The groups of `dialog` as a ribbon shows them, side by side, without
/// the buttons that close it: where the tools of a step stand among
/// others (those of a sketch, in the drawing workbench).
pub fn show_groups(
    ui: &mut egui::Ui,
    dialog: &EditDialog,
    out: &mut Vec<Message>,
    choose_file: &mut Option<Vec<usize>>,
) {
    egui::ScrollArea::horizontal()
        .id_salt("edit-ribbon")
        .show(ui, |ui| {
            let mut fields = Fields {
                picking: dialog.picking.as_deref(),
                out: &mut *out,
                choose_file: &mut *choose_file,
            };
            ui.horizontal_top(|ui| {
                ui.set_min_height(RIBBON_HEIGHT);
                for (i, group) in dialog.fields.children().iter().enumerate() {
                    if i > 0 {
                        ui.separator();
                    }
                    fields.ribbon_group(ui, group, &mut vec![i]);
                }
            });
        });
}

impl Fields<'_> {
    /// One group of a ribbon: its tools as icon buttons in columns, its
    /// fields stacked beside them, and its name below.
    fn ribbon_group(&mut self, ui: &mut egui::Ui, group: &Widget<Message>, path: &mut Vec<usize>) {
        let (label, children) = match group {
            Widget::Group(g) => (g.label.as_str(), &g.children[..]),
            other => ("", std::slice::from_ref(other)),
        };
        // Full-sized buttons where they stand; the small tools in columns
        // of two before the fields, stacked in columns of three.
        let large = |w: &Widget<Message>| match w {
            Widget::ToolButton(tool) => tool.large,
            Widget::ReferenceList(_) => true,
            _ => false,
        };
        let indexed: Vec<(usize, &Widget<Message>)> = children.iter().enumerate().collect();
        let (big, rest): (Vec<_>, Vec<_>) = indexed.into_iter().partition(|(_, w)| large(w));
        let (tools, others): (Vec<_>, Vec<_>) = rest
            .into_iter()
            .partition(|(_, w)| matches!(w, Widget::ToolButton(_)));
        ui.vertical(|ui| {
            ui.horizontal_top(|ui| {
                ui.set_min_height(TOOL_ROWS as f32 * (RIBBON_TOOL + 3.0));
                for column in tools.chunks(TOOL_ROWS) {
                    ui.vertical(|ui| {
                        for (_, tool) in column {
                            if let Widget::ToolButton(tool) = tool {
                                self.ribbon_tool(ui, tool);
                            }
                        }
                    });
                }
                for column in others.chunks(FIELD_ROWS) {
                    ui.vertical(|ui| {
                        ui.spacing_mut().button_padding = egui::vec2(6.0, 1.0);
                        ui.spacing_mut().interact_size.y = 18.0;
                        for (i, field) in column {
                            path.push(*i);
                            self.ribbon_field(ui, field, path);
                            path.pop();
                        }
                    });
                }
                for (i, widget) in big {
                    path.push(i);
                    self.ribbon_large(ui, widget, path);
                    path.pop();
                }
            });
            ui.label(
                egui::RichText::new(label)
                    .size(10.5)
                    .color(color::TEXT_WEAK),
            );
        });
    }

    /// A ribbon tool: its icon; its name, hotkey and what it does show
    /// when the pointer rests on it (or why it cannot be used now).
    fn ribbon_tool(&mut self, ui: &mut egui::Ui, tool: &ToolButton<Message>) {
        let enabled = tool.enabled();
        let sense = if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(RIBBON_TOOL, RIBBON_TOOL), sense);
        if ui.is_rect_visible(rect) {
            let fill = if tool.active {
                color::ACCENT.gamma_multiply(0.18)
            } else if response.is_pointer_button_down_on() {
                color::ACTIVE
            } else if response.hovered() && enabled {
                color::HOVER
            } else {
                egui::Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 6.0, fill);
            let tint = if !enabled {
                color::TEXT_WEAK.gamma_multiply(0.45)
            } else if tool.active {
                color::ACCENT
            } else {
                color::TEXT
            };
            let icon = egui::Rect::from_center_size(rect.center(), egui::vec2(20.0, 20.0));
            icons::widget(icons::named(tool.icon), 20.0, tint).paint_at(ui, icon);
        }
        let name = tip(&tool.label, Some(tool.hotkey), &tool.hint);
        let response = match &tool.disabled {
            Some(reason) => response.on_hover_text(format!("{name}\n\nNot now: {reason}")),
            None => response.on_hover_text(name),
        };
        if response.clicked() {
            self.out.extend(tool.click());
            self.out.push(Message::EndEdit);
        }
    }

    /// A full-sized ribbon button: its icon above its name, as high as
    /// two rows of small tools. A tool, or a box of references (picking
    /// on or off).
    fn ribbon_large(&mut self, ui: &mut egui::Ui, widget: &Widget<Message>, path: &[usize]) {
        let (icon, label, hint, enabled, active) = match widget {
            Widget::ToolButton(tool) => {
                let name = tip(&tool.label, Some(tool.hotkey), &tool.hint);
                let hint = match &tool.disabled {
                    Some(reason) => format!("{name}\n\nNot now: {reason}"),
                    None => name,
                };
                (
                    tool.icon,
                    tool.label.clone(),
                    hint,
                    tool.enabled(),
                    tool.active,
                )
            }
            Widget::ReferenceList(list) => {
                let active = self.picking == Some(path);
                let label = if active {
                    format!("{}: pick…", list.label)
                } else if list.values.is_empty() {
                    list.label.clone()
                } else {
                    format!("{} ({})", list.label, list.values.len())
                };
                let kinds: Vec<&str> = list.kinds.iter().map(|k| k.name()).collect();
                let hint = format!(
                    "Click, then pick in the model (again to remove): {}. Click here again to stop.",
                    kinds.join(", ")
                );
                ("project", label, hint, true, active)
            }
            _ => return,
        };
        let sense = if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let font = egui::FontId::proportional(11.5);
        let name = ui
            .painter()
            .layout_no_wrap(label.clone(), font.clone(), color::TEXT);
        let size = egui::vec2(
            LARGE_WIDTH.max(name.size().x + 10.0),
            TOOL_ROWS as f32 * (RIBBON_TOOL + 3.0) - 3.0,
        );
        let (rect, response) = ui.allocate_exact_size(size, sense);
        if ui.is_rect_visible(rect) {
            let fill = if active {
                color::ACCENT.gamma_multiply(0.18)
            } else if response.is_pointer_button_down_on() {
                color::ACTIVE
            } else if response.hovered() && enabled {
                color::HOVER
            } else {
                egui::Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 6.0, fill);
            let tint = if !enabled {
                color::TEXT_WEAK.gamma_multiply(0.45)
            } else if active {
                color::ACCENT
            } else {
                color::TEXT
            };
            let at = egui::pos2(rect.center().x, rect.top() + 8.0 + LARGE_ICON / 2.0);
            let icon_rect = egui::Rect::from_center_size(at, egui::vec2(LARGE_ICON, LARGE_ICON));
            icons::widget(icons::named(icon), LARGE_ICON, tint).paint_at(ui, icon_rect);
            ui.painter().text(
                egui::pos2(rect.center().x, rect.bottom() - 6.0),
                egui::Align2::CENTER_BOTTOM,
                &label,
                font,
                tint,
            );
        }
        if response.on_hover_text(hint).clicked() {
            match widget {
                Widget::ToolButton(tool) => {
                    self.out.extend(tool.click());
                    self.out.push(Message::EndEdit);
                }
                Widget::ReferenceList(_) => {
                    let active = self.picking == Some(path);
                    self.out
                        .push(Message::Pick((!active).then(|| path.to_vec())));
                }
                _ => {}
            }
        }
    }

    /// A field of a ribbon group, in one compact row.
    fn ribbon_field(&mut self, ui: &mut egui::Ui, field: &Widget<Message>, path: &mut Vec<usize>) {
        match field {
            Widget::Choice(choice) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&choice.label).color(color::TEXT_WEAK));
                    let selected = choice
                        .options
                        .get(choice.selected)
                        .cloned()
                        .unwrap_or_default();
                    egui::ComboBox::from_id_salt(("ribbon-choice", path.clone()))
                        .selected_text(selected)
                        .width(96.0)
                        .show_ui(ui, |ui| {
                            for (i, option) in choice.options.iter().enumerate() {
                                let enabled = choice.enabled.get(i).copied().unwrap_or(false);
                                let item = egui::Button::selectable(i == choice.selected, option);
                                let response = ui
                                    .add_enabled(enabled, item)
                                    .on_disabled_hover_text(choice.reason(i).unwrap_or_default());
                                if response.clicked() && i != choice.selected {
                                    self.out.extend(choice.set(i));
                                    self.out.push(Message::EndEdit);
                                }
                            }
                        });
                });
            }
            Widget::Label(label) if label.label.is_empty() => {
                // A status: wrapped narrow, so a long one does not push
                // the tools after it away.
                ui.scope(|ui| {
                    ui.set_max_width(RIBBON_NOTE);
                    let text = egui::RichText::new(&label.text).color(mood_color(label.mood));
                    ui.add(egui::Label::new(text).wrap());
                });
            }
            other => self.widget(ui, other, path),
        }
    }
}
