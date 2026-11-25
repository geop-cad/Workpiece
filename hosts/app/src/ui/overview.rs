//! The overview: the focused part's bodies, sketches and datums, each
//! with an eye to show or hide it.

use eframe::egui;
use shapecraft_cad_ui::Message;
use shapecraft_cad_ui::view::{Item, Overview};

use crate::icons;
use crate::theme::color;

/// Draws the overview.
pub fn show(ui: &mut egui::Ui, overview: &Overview, out: &mut Vec<Message>) {
    if overview.part.is_none() {
        return;
    };
    ui.strong("Contents");
    egui::ScrollArea::vertical()
        .id_salt("overview")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            section(ui, "Bodies", &overview.bodies, true, out);
            if !overview.faces.is_empty() {
                section(ui, "Faces", &overview.faces, true, out);
            }
            section(ui, "Sketches", &overview.sketches, false, out);
            section(ui, "Datums", &overview.datums, false, out);
            // How it is displayed besides what the view's own bar has:
            // a joint run along its travel, its parts apart.
            if !overview.joints.is_empty() || overview.assembly {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Display")
                        .size(11.5)
                        .color(color::TEXT_WEAK),
                );
            }
            let mut display = overview.display.clone();
            if !overview.joints.is_empty() {
                // A joint run along its travel, between its limits.
                ui.horizontal(|ui| {
                    ui.label("Motion");
                    let (mut which, mut along) = display.motion.unwrap_or((0, 0.5));
                    which = which.min(overview.joints.len() - 1);
                    let mut on = display.motion.is_some();
                    egui::ComboBox::from_id_salt("motion-joint")
                        .selected_text(&overview.joints[which])
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for (k, name) in overview.joints.iter().enumerate() {
                                if ui.selectable_value(&mut which, k, name).clicked() {
                                    on = true;
                                }
                            }
                        });
                    let slider = ui
                        .add(egui::Slider::new(&mut along, 0.0..=1.0).show_value(false))
                        .on_hover_text("Run the joint from one limit to the other: the parts are drawn where that leaves them (right-click: as they are)");
                    if slider.changed() {
                        on = true;
                    }
                    if slider.secondary_clicked() {
                        on = false;
                    }
                    display.motion = on.then_some((which, along));
                });
            }
            if overview.assembly {
                ui.horizontal(|ui| {
                    ui.label("Explode");
                    ui.add(egui::Slider::new(&mut display.explode, 0.0..=2.0).show_value(false))
                        .on_hover_text("Draw the placed parts apart, away from their middle");
                });
            }
            if display != overview.display {
                out.push(Message::SetDisplay(display));
            }
            // What is placed in it, counted.
            if !overview.placed.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Bill of materials")
                        .size(11.5)
                        .color(color::TEXT_WEAK),
                );
                for (part, count) in &overview.placed {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{count} ×")).monospace());
                        ui.label(part);
                    });
                }
            }
            // The assembly as a whole, measured.
            if !overview.whole.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("Assembly (in {})", overview.unit.label()))
                        .size(11.5)
                        .color(color::TEXT_WEAK),
                );
                for reading in &overview.whole {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&reading.label).color(color::TEXT_WEAK));
                        ui.label(egui::RichText::new(&reading.value).monospace());
                    });
                }
            }
            // What is selected, measured.
            if !overview.measured.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("Selection (in {})", overview.unit.label()))
                        .size(11.5)
                        .color(color::TEXT_WEAK),
                );
                for reading in &overview.measured {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&reading.label).color(color::TEXT_WEAK));
                        ui.label(egui::RichText::new(&reading.value).monospace());
                    });
                }
            }
        });
}

/// A section of the overview. Its names, with `alone`, can be clicked to
/// show one of them alone (and again, to show them all).
fn section(ui: &mut egui::Ui, title: &str, items: &[Item], alone: bool, out: &mut Vec<Message>) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(title)
            .size(11.5)
            .color(color::TEXT_WEAK),
    );
    if items.is_empty() {
        ui.weak("  none");
    }
    for item in items {
        ui.horizontal(|ui| {
            let (icon, tint) = if item.visible {
                (icons::EYE, color::TEXT)
            } else {
                (icons::EYE_OFF, color::TEXT_WEAK)
            };
            let hint = match (item.visible, item.auto_hidden) {
                (true, _) => "Hide",
                (false, true) => "Hidden because later steps use it; click to show",
                (false, false) => "Show",
            };
            let eye = egui::Button::image(icons::widget(icon, 14.0, tint)).frame(false);
            if ui.add(eye).on_hover_text(hint).clicked() {
                out.push(item.toggle.click());
            }
            let text = egui::RichText::new(&item.name);
            let text = if item.visible {
                text
            } else {
                text.color(color::TEXT_WEAK).italics()
            };
            if alone && items.len() > 1 {
                let name = ui
                    .add(egui::Label::new(text).sense(egui::Sense::click()))
                    .on_hover_text("Double-click: show this body alone (again: show all)");
                if name.double_clicked() {
                    out.push(item.isolate.click());
                } else if name.clicked() {
                    out.push(Message::SelectStep(Some(vec![item.name.clone()])));
                }
            } else {
                // A click selects it, as its step in the timeline: a
                // tool started then takes it (an extrusion its sketch).
                let name = ui
                    .add(egui::Label::new(text).sense(egui::Sense::click()))
                    .on_hover_text("Click to select: the next tool takes it");
                if name.clicked() {
                    out.push(Message::SelectStep(Some(vec![item.name.clone()])));
                }
            }
            // A body: what it is made of, where that shows.
            if let Some(colour) = item.colour {
                super::materials::swatch(ui, colour);
            }
            if let Some(material) = &item.material {
                ui.weak(material);
            }
        });
    }
}
