//! The materials panel: what the part is made of, and the library's
//! materials, each to be changed.

use eframe::egui;
use shapecraft_cad_part::{Material, Texture};
use shapecraft_cad_ui::Message;
use shapecraft_cad_ui::view::Materials;

use crate::theme::color;

/// A text field that keeps what is typed to itself until it is left.
pub fn field(ui: &mut egui::Ui, id: egui::Id, current: &str, width: f32) -> Option<String> {
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| current.to_owned());
    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(width));
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text));
        return None;
    }
    ui.data_mut(|d| d.remove::<String>(id));
    (response.lost_focus() && text != current).then_some(text)
}

/// A swatch of `colour`.
pub fn swatch(ui: &mut egui::Ui, [r, g, b]: [u8; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
}

/// The choice of what a part is made of, among `names`: the message
/// for another choice. The list is searched by typing: it opens with
/// the keyboard in its search field, shows the names that hold what is
/// typed, and Enter takes the first.
pub fn of_part(ui: &mut egui::Ui, now: Option<&str>, names: &[String]) -> Option<Message> {
    let mut chosen = None;
    let search_id = ui.id().with("part-material-search");
    let combo = egui::ComboBox::from_id_salt("part-material")
        .selected_text(now.unwrap_or("No material"))
        .width(120.0)
        .height(260.0)
        .show_ui(ui, |ui| {
            let mut search: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
            let field = ui.add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text("Search…")
                    .desired_width(150.0),
            );
            // (Asked for while nothing else has the keyboard: as the
            // list opens.)
            if !field.has_focus() && ui.memory(|m| m.focused().is_none()) {
                field.request_focus();
            }
            let wanted = search.trim().to_lowercase();
            let found: Vec<&String> = names
                .iter()
                .filter(|name| name.to_lowercase().contains(&wanted))
                .collect();
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if let (true, Some(first)) = (entered && !wanted.is_empty(), found.first()) {
                chosen = Some(Message::SetPartMaterial(Some((*first).clone())));
            }
            ui.separator();
            if wanted.is_empty() && ui.selectable_label(now.is_none(), "No material").clicked() {
                chosen = Some(Message::SetPartMaterial(None));
            }
            for name in &found {
                if ui
                    .selectable_label(now == Some(name.as_str()), *name)
                    .clicked()
                {
                    chosen = Some(Message::SetPartMaterial(Some((*name).clone())));
                }
            }
            if found.is_empty() {
                ui.weak("No material of that name");
            }
            if chosen.is_some() {
                search.clear();
                ui.close();
            }
            ui.data_mut(|d| d.insert_temp(search_id, search));
        });
    combo.response.on_hover_text(
        "What the part is made of: it shows as that and weighs by its density. \
         An Appearance step gives single bodies and faces another",
    );
    chosen
}

/// Which material the panel shows the numbers of.
#[derive(Default)]
pub struct MaterialsPanel {
    selected: usize,
}

impl MaterialsPanel {
    /// Draws the panel as a window over the view; what is changed is
    /// sent as it is changed.
    pub fn show(&mut self, ctx: &egui::Context, materials: &Materials, out: &mut Vec<Message>) {
        let mut open = true;
        egui::Window::new("Materials")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(egui::pos2(340.0, 150.0))
            .show(ctx, |ui| self.contents(ui, materials, out));
        if !open {
            out.push(Message::ShowPanel(None));
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, materials: &Materials, out: &mut Vec<Message>) {
        let names: Vec<String> = materials.library.iter().map(|m| m.name.clone()).collect();
        if materials.part {
            ui.horizontal(|ui| {
                ui.label("This part is made of");
                out.extend(of_part(ui, materials.of_part.as_deref(), &names));
            });
            ui.separator();
        }
        let mut library = materials.library.clone();
        ui.label(
            egui::RichText::new(format!(
                "Kept in {} beside the parts, for all of them",
                materials.file
            ))
            .size(11.0)
            .color(color::TEXT_WEAK),
        );
        ui.add_space(4.0);
        self.selected = self.selected.min(library.len().saturating_sub(1));
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("materials")
                    .max_height(260.0)
                    .min_scrolled_height(260.0)
                    .show(ui, |ui| {
                        ui.set_min_width(150.0);
                        for (k, material) in library.iter().enumerate() {
                            ui.horizontal(|ui| {
                                swatch(ui, material.colour);
                                if ui
                                    .selectable_label(self.selected == k, &material.name)
                                    .clicked()
                                {
                                    self.selected = k;
                                }
                            });
                        }
                    });
                ui.horizontal(|ui| {
                    if ui.button("+").on_hover_text("A new material").clicked() {
                        let fresh = shapecraft_cad_part::Library {
                            materials: library.clone(),
                        }
                        .fresh_name();
                        library.push(Material::new(fresh));
                        self.selected = library.len() - 1;
                    }
                    let any = !library.is_empty();
                    if ui
                        .add_enabled(any, egui::Button::new("Copy"))
                        .on_hover_text("Another like the one chosen")
                        .clicked()
                    {
                        let mut copy = library[self.selected].clone();
                        copy.name = (2..)
                            .map(|n| format!("{} {n}", copy.name))
                            .find(|name| library.iter().all(|m| m.name != *name))
                            .unwrap_or_default();
                        library.push(copy);
                        self.selected = library.len() - 1;
                    }
                    if ui
                        .add_enabled(any, egui::Button::new("Remove"))
                        .on_hover_text("Take the one chosen out of the library")
                        .clicked()
                    {
                        library.remove(self.selected);
                        self.selected = self.selected.min(library.len().saturating_sub(1));
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(230.0);
                let taken: Vec<String> = library.iter().map(|m| m.name.clone()).collect();
                if let Some(material) = library.get_mut(self.selected) {
                    numbers(ui, self.selected, material, &taken);
                }
            });
        });
        if library != materials.library {
            out.push(Message::SetLibrary { materials: library });
        }
    }
}

/// The numbers of one material, to change.
fn numbers(ui: &mut egui::Ui, k: usize, material: &mut Material, taken: &[String]) {
    egui::Grid::new("material-numbers")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Name");
            let id = egui::Id::new(("material-name", k));
            if let Some(name) = field(ui, id, &material.name, 140.0) {
                let name = name.trim();
                if !name.is_empty() && taken.iter().all(|t| t != name) {
                    material.name = name.to_owned();
                }
            }
            ui.end_row();
            ui.label("Density");
            ui.horizontal(|ui| {
                let id = egui::Id::new(("material-density", k));
                let density = field(ui, id, &format!("{}", material.density), 64.0);
                if let Some(density) = density.and_then(|d| d.trim().parse::<f64>().ok())
                    && density > 0.0
                {
                    material.density = density;
                }
                ui.weak("kg/m³");
            });
            ui.end_row();
            ui.label("Colour");
            ui.color_edit_button_srgb(&mut material.colour);
            ui.end_row();
            ui.label("Texture");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("material-texture", k))
                    .selected_text(material.texture.label())
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        for texture in Texture::ALL {
                            ui.selectable_value(&mut material.texture, texture, texture.label());
                        }
                    })
                    .response
                    .on_hover_text("What is drawn over its colour");
                if material.texture != Texture::Plain {
                    let id = egui::Id::new(("material-scale", k));
                    let scale = field(ui, id, &format!("{}", material.scale), 40.0);
                    if let Some(scale) = scale.and_then(|s| s.trim().parse::<f64>().ok())
                        && scale > 0.0
                    {
                        material.scale = scale;
                    }
                    ui.weak("a repeat");
                }
            });
            ui.end_row();
            let slider = |ui: &mut egui::Ui, label: &str, value: &mut f64, hint: &str| {
                ui.label(label);
                ui.add(egui::Slider::new(value, 0.0..=1.0).fixed_decimals(2))
                    .on_hover_text(hint);
                ui.end_row();
            };
            slider(
                ui,
                "Roughness",
                &mut material.roughness,
                "From a mirror (0) to matte (1): how wide what it reflects is spread, in a rendered picture",
            );
            slider(
                ui,
                "Metal",
                &mut material.metallic,
                "How much of a metal it is: a metal reflects in its own colour",
            );
            slider(
                ui,
                "Clear",
                &mut material.transparency,
                "How much light goes through it: 1 is clear as glass",
            );
        });
}
