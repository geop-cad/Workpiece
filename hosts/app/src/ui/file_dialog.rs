//! A file dialog for part files in the working directory: choose one from
//! the cards, each with a preview of its part, or (to save) type a new
//! name.

use eframe::egui;

use super::previews::{Previews, Shown};
use crate::files;
use crate::icons;
use crate::theme::color;

/// How large a file's card is, and the preview on it.
const CARD: egui::Vec2 = egui::vec2(148.0, 136.0);
const THUMB: egui::Vec2 = egui::vec2(136.0, 102.0);

/// What the dialog is for.
#[derive(Debug, Clone, PartialEq)]
pub enum Purpose {
    /// Open a part.
    Open,
    /// Save the active part under a new name.
    SaveAs,
    /// Fill the edit dialog's file field at this widget path.
    Field(Vec<usize>),
    /// Name a new part, to add to the one worked on.
    NewPart,
    /// Name the file the part's bodies are written to for other
    /// programs: STEP, or (named `.stl`) STL.
    Export,
}

/// An open file dialog.
pub struct FileDialog {
    /// What it is for.
    pub purpose: Purpose,
    name: String,
    files: Vec<String>,
    /// Whether it has not been drawn yet: its name field then takes the
    /// keyboard, the name selected.
    fresh: bool,
}

impl FileDialog {
    /// A dialog for `purpose`, starting at `name`.
    pub fn new(purpose: Purpose, name: &str) -> Self {
        Self {
            name: name.to_owned(),
            files: files::list(matches!(purpose, Purpose::Field(_))),
            fresh: true,
            purpose,
        }
    }

    /// The file the name stands for: a part file — or, exporting, a
    /// STEP file unless it is named as an STL one.
    fn chosen(&self) -> String {
        if self.purpose != Purpose::Export {
            return files::with_extension(&self.name);
        }
        let name = self.name.trim();
        let lower = name.to_lowercase();
        if [
            ".step", ".stp", ".igs", ".iges", ".stl", ".3mf", ".dxf", ".pdf", ".nc", ".tap", ".png",
        ]
        .iter()
        .any(|e| lower.ends_with(e))
        {
            name.to_owned()
        } else {
            format!("{name}.step")
        }
    }

    /// Draws the dialog. `Some(Some(path))` when a file is chosen,
    /// `Some(None)` when cancelled.
    pub fn show(&mut self, ctx: &egui::Context, previews: &mut Previews) -> Option<Option<String>> {
        if self.fresh {
            previews.look_again(&self.files);
        }
        previews.work();
        let saving = matches!(
            self.purpose,
            Purpose::SaveAs | Purpose::NewPart | Purpose::Export
        );
        let title = match self.purpose {
            Purpose::Open => "Open part",
            Purpose::SaveAs => "Save part as",
            Purpose::Field(_) => "Choose a part or a STEP file",
            Purpose::NewPart => "Name the new part",
            Purpose::Export => {
                "Export as STEP (.step), IGES (.igs), STL (.stl), 3MF (.3mf), a drawing (.dxf, .pdf), a cut path (.nc), a milling path (.tap) or, of a Render step, a picture (.png)"
            }
        };
        let mut result = None;
        egui::Modal::new(egui::Id::new("file-dialog")).show(ctx, |ui| {
            ui.set_width(4.0 * (CARD.x + 6.0) + 10.0);
            ui.heading(title);
            ui.weak(if shapecraft_core::files::IN_MEMORY {
                "The files in this page's memory: those opened, and examples. Saving downloads the file."
                    .to_owned()
            } else {
                std::env::current_dir()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default()
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(3.0 * (CARD.y + 6.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if self.files.is_empty() {
                        ui.weak("No part files here yet.");
                    }
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    ui.horizontal_wrapped(|ui| {
                        for file in &self.files {
                            let selected = self.chosen() == *file;
                            let response = card(ui, previews, file, selected);
                            if response.clicked() {
                                self.name = file.clone();
                            }
                            if response.double_clicked() {
                                result = Some(Some(file.clone()));
                            }
                        }
                    });
                });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Name");
                let mut output = egui::TextEdit::singleline(&mut self.name)
                    .desired_width(240.0)
                    .show(ui);
                if std::mem::take(&mut self.fresh) {
                    // Typing right away names the file; the extension
                    // stays.
                    output.response.request_focus();
                    let stem = self.name.rsplit_once('.').map_or(&*self.name, |n| n.0);
                    let all = egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(stem.chars().count()),
                    );
                    output.state.cursor.set_char_range(Some(all));
                    output.state.store(ui.ctx(), output.response.id);
                }
                let field = output.response;
                if field.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.name.trim().is_empty()
                {
                    result = Some(Some(self.chosen()));
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let chosen = self.chosen();
                let exists = if self.purpose == Purpose::Export {
                    shapecraft_core::files::exists(&chosen)
                } else {
                    self.files.contains(&chosen)
                };
                let ok = !self.name.trim().is_empty() && (saving || exists);
                let label = match self.purpose {
                    Purpose::NewPart => "Create",
                    Purpose::Export => "Export",
                    _ if saving => "Save",
                    _ => "Open",
                };
                let button =
                    egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE))
                        .fill(color::ACCENT);
                if ui.add_enabled(ok, button).clicked() {
                    result = Some(Some(chosen));
                }
                if ui.button("Cancel").clicked() {
                    result = Some(None);
                }
                if saving && exists {
                    ui.colored_label(color::ERROR, "replaces the file");
                }
            });
        });
        result
    }
}

/// A file's card: the preview of its part over its name.
fn card(ui: &mut egui::Ui, previews: &mut Previews, file: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(CARD, egui::Sense::click());
    let response = response.on_hover_text(file);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let (fill, border) = if selected {
        (color::ACCENT.gamma_multiply(0.10), color::ACCENT)
    } else if response.hovered() {
        (color::HOVER, color::BORDER)
    } else {
        (color::PANEL, color::BORDER)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        8.0,
        fill,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    let thumb = egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), THUMB);
    match previews.of(ui.ctx(), file) {
        Shown::Picture(texture) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            egui::Image::new(texture)
                .uv(uv)
                .corner_radius(5)
                .paint_at(ui, thumb);
        }
        shown => {
            // No picture (yet): a quiet tile with the file's icon.
            painter.rect_filled(thumb, 5.0, color::HOVER);
            let icon = egui::Rect::from_center_size(thumb.center(), egui::vec2(22.0, 22.0));
            icons::widget(icons::FOLDER, 22.0, color::TEXT_WEAK).paint_at(ui, icon);
            if matches!(shown, Shown::Coming) {
                painter.text(
                    thumb.center_bottom() - egui::vec2(0.0, 8.0),
                    egui::Align2::CENTER_BOTTOM,
                    "…",
                    egui::FontId::proportional(12.0),
                    color::TEXT_WEAK,
                );
            }
        }
    }
    // Its name, without the folders it is in and what kind of file it
    // is, cut short to fit.
    let name = file.rsplit('/').next().unwrap_or(file);
    let name = name.strip_suffix(".shapecraft").unwrap_or(name);
    let font = egui::FontId::proportional(12.5);
    let ink = if selected { color::ACCENT } else { color::TEXT };
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), font, ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width(CARD.x - 12.0);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let at = egui::pos2(
        rect.center().x - galley.size().x / 2.0,
        thumb.bottom() + 6.0,
    );
    ui.painter().galley(at, galley, ink);
    response
}
