//! The icons, as SVG images tinted to the text color: the program's own,
//! and those the crates of operations tell of.

use eframe::egui;
use shapecraft_cad_ui::view::Icon;

/// The image of `icon`.
pub fn image(icon: Icon) -> egui::ImageSource<'static> {
    match icon {
        Icon::Undo => egui::include_image!("../assets/icons/undo.svg"),
        Icon::Redo => egui::include_image!("../assets/icons/redo.svg"),
        Icon::Done => egui::include_image!("../assets/icons/done.svg"),
        Icon::Cancel => egui::include_image!("../assets/icons/cancel.svg"),
        Icon::Focus => egui::include_image!("../assets/icons/focus.svg"),
        Icon::NewPart => egui::include_image!("../assets/icons/new-part.svg"),
        Icon::Named(name) => named(name),
    }
}

/// The image of the icon an operation calls `name` (a point for a name
/// there is no icon for).
pub fn named(name: &str) -> egui::ImageSource<'static> {
    if let Some(svg) = shapecraft_cad_part::registry().svg(name) {
        return egui::ImageSource::Bytes {
            uri: format!("bytes://icons/{name}.svg").into(),
            bytes: egui::load::Bytes::Static(svg.as_bytes()),
        };
    }
    match name {
        "project" => egui::include_image!("../assets/icons/project.svg"),
        _ => egui::include_image!("../assets/icons/point.svg"),
    }
}
/// Back to the starting view.
pub const NORMAL_VIEW: egui::ImageSource<'static> =
    egui::include_image!("../assets/icons/normal-view.svg");
pub const HOME: egui::ImageSource<'static> = egui::include_image!("../assets/icons/home.svg");
/// Orthographic projection.
pub const ORTHOGRAPHIC: egui::ImageSource<'static> =
    egui::include_image!("../assets/icons/orthographic.svg");
/// Perspective projection.
pub const PERSPECTIVE: egui::ImageSource<'static> =
    egui::include_image!("../assets/icons/perspective.svg");
/// Shown.
pub const EYE: egui::ImageSource<'static> = egui::include_image!("../assets/icons/eye.svg");
/// Hidden.
pub const EYE_OFF: egui::ImageSource<'static> = egui::include_image!("../assets/icons/eye-off.svg");
/// A file.
pub const FOLDER: egui::ImageSource<'static> = egui::include_image!("../assets/icons/folder.svg");

/// The disclosure arrow of tree nodes (pointing right; turned for open).
pub const CHEVRON: egui::ImageSource<'static> = egui::include_image!("../assets/icons/chevron.svg");

/// `icon` at `size` points, tinted `color`.
pub fn widget(
    icon: egui::ImageSource<'static>,
    size: f32,
    color: egui::Color32,
) -> egui::Image<'static> {
    egui::Image::new(icon)
        .fit_to_exact_size(egui::vec2(size, size))
        .tint(color)
}
