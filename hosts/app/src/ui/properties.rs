//! The properties panel: what the part says of itself, and which
//! properties parts have.

use eframe::egui;
use workpiece_part::{Kind, Property, Schema};
use workpiece_ui::Message;
use workpiece_ui::view::PartProperties;

use super::materials::field;
use crate::theme::color;

/// The kinds a property can be of, a fresh one of each.
fn kinds() -> [Kind; 6] {
    [
        Kind::Text,
        Kind::Integer,
        Kind::Number,
        Kind::Date,
        Kind::Boolean,
        Kind::Choice(Vec::new()),
    ]
}

/// The field that says `property`, showing `value`: what it is made
/// to say, if it is changed.
pub fn value(ui: &mut egui::Ui, id: egui::Id, property: &Property, value: &str) -> Option<String> {
    match &property.kind {
        Kind::Choice(of) => {
            let mut now = value.to_owned();
            egui::ComboBox::from_id_salt(id)
                .selected_text(if now.is_empty() { "—" } else { now.as_str() })
                .width(150.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut now, String::new(), "—");
                    for choice in of {
                        ui.selectable_value(&mut now, choice.clone(), choice);
                    }
                });
            (now != value).then_some(now)
        }
        Kind::Boolean => {
            let mut now = value.to_owned();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut now, "yes".to_owned(), "Yes");
                ui.selectable_value(&mut now, "no".to_owned(), "No");
                ui.selectable_value(&mut now, String::new(), "—");
            });
            (now != value).then_some(now)
        }
        kind => {
            let typed = field(ui, id, value, 150.0)?;
            // (What is not of the property's kind is not taken.)
            kind.check(&typed).is_ok().then_some(typed)
        }
    }
}

/// Which side of the panel shows.
#[derive(Default)]
pub struct PropertiesPanel {
    /// Whether the properties themselves are being defined.
    defining: bool,
}

impl PropertiesPanel {
    /// Draws the panel as a window over the view; what is changed is
    /// sent as it is changed.
    pub fn show(&mut self, ctx: &egui::Context, shown: &PartProperties, out: &mut Vec<Message>) {
        let mut open = true;
        egui::Window::new("Properties")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(egui::pos2(340.0, 150.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.defining, false, "Of this part");
                    ui.selectable_value(&mut self.defining, true, "Define properties");
                });
                ui.separator();
                if self.defining {
                    define(ui, shown, out);
                } else {
                    values(ui, shown, out);
                }
            });
        if !open {
            out.push(Message::ShowPanel(None));
        }
    }
}

/// What the part says of each property.
fn values(ui: &mut egui::Ui, shown: &PartProperties, out: &mut Vec<Message>) {
    if !shown.part {
        ui.weak("No part is open");
        return;
    }
    egui::Grid::new("part-properties")
        .num_columns(3)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            // The unit of length the part's numbers are in.
            ui.label("Unit");
            let mut unit = shown.unit;
            egui::ComboBox::from_id_salt("part-unit")
                .selected_text(unit.label())
                .width(150.0)
                .show_ui(ui, |ui| {
                    for each in workpiece_core::Unit::ALL {
                        ui.selectable_value(&mut unit, each, each.label());
                    }
                });
            if unit != shown.unit {
                out.push(Message::SetUnit(unit));
            }
            ui.weak("of its lengths (exports say so)");
            ui.end_row();
            for (k, property) in shown.schema.iter().enumerate() {
                let star = if property.required { " *" } else { "" };
                ui.label(format!("{}{star}", property.name));
                let said = shown.said.get(&property.name).map_or("", String::as_str);
                let id = egui::Id::new(("part-property", k));
                let now = if said.is_empty() {
                    &property.default
                } else {
                    said
                };
                if let Some(value) = value(ui, id, property, now) {
                    out.push(Message::SetProperty {
                        name: property.name.clone(),
                        value,
                    });
                }
                let what = match (&property.kind, said.is_empty()) {
                    (Kind::Date, _) => "a day, as 2026-11-22",
                    (_, true) if !property.default.is_empty() => "as by default",
                    _ => "",
                };
                ui.weak(what);
                ui.end_row();
            }
        });
    for problem in &shown.problems {
        ui.colored_label(color::ERROR, problem);
    }
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Title blocks of the part's drawings say these")
            .size(11.0)
            .color(color::TEXT_WEAK),
    );
}

/// The properties parts have, to change.
fn define(ui: &mut egui::Ui, shown: &PartProperties, out: &mut Vec<Message>) {
    let mut schema = shown.schema.clone();
    ui.label(
        egui::RichText::new(format!(
            "Kept in {} beside the parts, for all of them",
            shown.file
        ))
        .size(11.0)
        .color(color::TEXT_WEAK),
    );
    ui.add_space(4.0);
    let mut gone = None;
    egui::Grid::new("schema")
        .num_columns(5)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            for heading in ["Name", "Is", "By default", "Required", ""] {
                ui.weak(heading);
            }
            ui.end_row();
            let taken: Vec<String> = schema.iter().map(|p| p.name.clone()).collect();
            for (k, property) in schema.iter_mut().enumerate() {
                let id = egui::Id::new(("schema-name", k));
                if let Some(name) = field(ui, id, &property.name, 110.0) {
                    let name = name.trim();
                    if !name.is_empty() && taken.iter().all(|t| t != name) {
                        property.name = name.to_owned();
                    }
                }
                ui.vertical(|ui| {
                    egui::ComboBox::from_id_salt(("schema-kind", k))
                        .selected_text(property.kind.label())
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for kind in kinds() {
                                let on = std::mem::discriminant(&kind)
                                    == std::mem::discriminant(&property.kind);
                                if ui.selectable_label(on, kind.label()).clicked() && !on {
                                    property.kind = kind;
                                }
                            }
                        });
                    if let Kind::Choice(of) = &mut property.kind {
                        let id = egui::Id::new(("schema-choices", k));
                        let typed = field(ui, id, &of.join(", "), 100.0);
                        if let Some(typed) = typed {
                            *of = typed
                                .split(',')
                                .map(|c| c.trim().to_owned())
                                .filter(|c| !c.is_empty())
                                .collect();
                        }
                    }
                });
                let id = egui::Id::new(("schema-default", k));
                let by_default = property.default.clone();
                if let Some(default) = value(ui, id, property, &by_default) {
                    property.default = default;
                }
                ui.checkbox(&mut property.required, "");
                if ui
                    .small_button("x")
                    .on_hover_text("Parts have it no more")
                    .clicked()
                {
                    gone = Some(k);
                }
                ui.end_row();
            }
        });
    if let Some(k) = gone {
        schema.remove(k);
    }
    if ui.button("+ Property").clicked() {
        let name = Schema {
            properties: schema.clone(),
        }
        .fresh_name();
        schema.push(Property {
            name,
            ..Property::default()
        });
    }
    if schema != shown.schema {
        out.push(Message::SetSchema { properties: schema });
    }
}
