//! The part's parameters and its table of variants: a grid with a column per
//! parameter and a row per variant (the first row is the part itself). A cell left empty takes the
//! part's own value.

use eframe::egui;
use shapecraft_cad_part::{Parameter, Variant};
use shapecraft_cad_ui::Message;
use shapecraft_cad_ui::view::Table;
use shapecraft_core::{Hotkey, Measure};

use super::tip;
use crate::icons;
use crate::theme::color;

/// The keys that add a parameter and a variant.
pub const ADD_PARAMETER: Hotkey = Hotkey::ctrl('P');
/// See [`ADD_PARAMETER`].
pub const ADD_VARIANT: Hotkey = Hotkey::ctrl_shift('P');

/// `table` with one more parameter.
pub fn with_parameter(table: &Table) -> Message {
    let mut table = table.clone();
    table.parameters.push(Parameter {
        name: table.fresh_parameter("p"),
        value: Measure::new(1.0),
    });
    table.store()
}

/// `table` with one more variant.
pub fn with_variant(table: &Table) -> Message {
    let mut table = table.clone();
    table.variants.push(Variant {
        name: table.fresh_variant("variant"),
        ..Variant::default()
    });
    table.store()
}

/// A text field that keeps what is typed to itself until it is left (or
/// Enter is pressed): then it gives the new text, if it changed.
fn cell(ui: &mut egui::Ui, id: egui::Id, current: &str, width: f32, hint: &str) -> Option<String> {
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| current.to_owned());
    let edit = egui::TextEdit::singleline(&mut text)
        .desired_width(width)
        .hint_text(hint);
    let response = ui.add(edit);
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text));
        return None;
    }
    ui.data_mut(|d| d.remove::<String>(id));
    (response.lost_focus() && text != current).then_some(text)
}

/// Draws the table; edits store it whole.
pub fn show(ui: &mut egui::Ui, table: &Table, out: &mut Vec<Message>) {
    ui.horizontal(|ui| {
        ui.strong("Parameters");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hint = tip(
                "Add variant",
                Some(ADD_VARIANT),
                "Another row of the table: the part with other values",
            );
            if ui.small_button("+ Variant").on_hover_text(hint).clicked() {
                out.push(with_variant(table));
            }
            let hint = tip(
                "Add parameter",
                Some(ADD_PARAMETER),
                "A named number for formulas: enter =name in a field",
            );
            if ui.small_button("+ Parameter").on_hover_text(hint).clicked() {
                out.push(with_parameter(table));
            }
        });
    });
    if table.parameters.is_empty() && table.variants.is_empty() {
        ui.weak("  none");
        return;
    }
    let no_values = Default::default();
    let values = table.values.as_ref().unwrap_or(&no_values);
    let mut edited = table.clone();
    let mut changed = false;
    egui::ScrollArea::both()
        .id_salt("table")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("parameters")
                .spacing([6.0, 3.0])
                .show(ui, |ui| {
                    // The header: the parameters' names.
                    ui.label("");
                    for (p, parameter) in table.parameters.iter().enumerate() {
                        let id = ui.id().with(("parameter", p));
                        ui.horizontal(|ui| {
                            if let Some(name) = cell(ui, id, &parameter.name, 72.0, "name")
                                && !name.trim().is_empty()
                            {
                                edited.parameters[p].name = name.trim().to_owned();
                                changed = true;
                            }
                            if ui
                                .small_button("×")
                                .on_hover_text("Remove this parameter")
                                .clicked()
                            {
                                edited.parameters.remove(p);
                                changed = true;
                            }
                        });
                    }
                    ui.end_row();
                    // Clicking a row's eye shows the part as that variant.
                    let shown =
                        |ui: &mut egui::Ui, variant: Option<&str>, out: &mut Vec<Message>| {
                            let on = table.shown.as_deref() == variant;
                            let icon = if on { icons::EYE } else { icons::EYE_OFF };
                            let tint = if on { color::ACCENT } else { color::TEXT_WEAK };
                            let button =
                                egui::Button::image(icons::widget(icon, 13.0, tint)).frame(false);
                            if ui
                                .add(button)
                                .on_hover_text("Show the part like this")
                                .clicked()
                            {
                                out.push(Message::ShowVariant(variant.map(str::to_owned)));
                            }
                        };
                    // The part itself: its own values.
                    ui.horizontal(|ui| {
                        shown(ui, None, out);
                        ui.label(egui::RichText::new("Default").color(color::TEXT_WEAK));
                    });
                    for (p, parameter) in table.parameters.iter().enumerate() {
                        let id = ui.id().with(("parameter", p, "default"));
                        let text = parameter.value.to_string();
                        if let Some(text) =
                            cell(ui, id, &text, 72.0, "").filter(|t| !t.trim().is_empty())
                        {
                            edited.parameters[p].value = Measure::parse(&text, values);
                            changed = true;
                        }
                    }
                    ui.end_row();
                    // Then a row per variant; an empty cell shows what it
                    // takes from the part.
                    for (v, variant) in table.variants.iter().enumerate() {
                        ui.horizontal(|ui| {
                            shown(ui, Some(&variant.name), out);
                            let id = ui.id().with(("variant", v));
                            if let Some(name) = cell(ui, id, &variant.name, 56.0, "name")
                                && !name.trim().is_empty()
                            {
                                edited.variants[v].name = name.trim().to_owned();
                                changed = true;
                            }
                            if ui
                                .small_button("×")
                                .on_hover_text("Remove this variant")
                                .clicked()
                            {
                                edited.variants.remove(v);
                                changed = true;
                            }
                        });
                        for (p, parameter) in table.parameters.iter().enumerate() {
                            let now = values
                                .get(&parameter.name)
                                .map(|v| format!("= {v}"))
                                .unwrap_or_default();
                            let text = variant
                                .values
                                .get(&parameter.name)
                                .map(Measure::to_string)
                                .unwrap_or_default();
                            let id = ui.id().with(("parameter", p, "variant", v));
                            if let Some(text) = cell(ui, id, &text, 72.0, &now) {
                                let values_of = &mut edited.variants[v].values;
                                if text.trim().is_empty() {
                                    values_of.remove(&parameter.name);
                                } else {
                                    values_of.insert(
                                        parameter.name.clone(),
                                        Measure::parse(&text, values),
                                    );
                                }
                                changed = true;
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    if let Err(error) = &table.values {
        ui.colored_label(color::ERROR, error);
    }
    if changed {
        out.push(edited.store());
    }
}
