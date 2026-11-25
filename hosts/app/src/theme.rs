//! Visual design tokens and the global egui style.

use eframe::egui;
use egui::{Color32, CornerRadius, Stroke};

/// Rounding of panels, buttons and rows.
pub const RADIUS: CornerRadius = CornerRadius::same(6);

/// The light theme's colors.
pub mod color {
    use super::Color32;

    /// Behind the 3D viewport.
    pub const CANVAS: Color32 = Color32::from_rgb(238, 240, 243);
    /// Panels.
    pub const PANEL: Color32 = Color32::from_rgb(255, 255, 255);
    /// Borders and separators.
    pub const BORDER: Color32 = Color32::from_rgb(226, 228, 232);
    /// Hovered rows and buttons.
    pub const HOVER: Color32 = Color32::from_rgb(241, 243, 246);
    /// Pressed buttons.
    pub const ACTIVE: Color32 = Color32::from_rgb(229, 231, 235);
    /// Text.
    pub const TEXT: Color32 = Color32::from_rgb(31, 35, 40);
    /// Secondary text.
    pub const TEXT_WEAK: Color32 = Color32::from_rgb(107, 114, 128);
    /// The accent: the edited step, the scrubber, picking.
    pub const ACCENT: Color32 = Color32::from_rgb(47, 111, 228);
    /// Errors.
    pub const ERROR: Color32 = Color32::from_rgb(200, 60, 55);
}

/// Installs the light style. Call once at startup.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut v = egui::Visuals::light();
    v.panel_fill = color::PANEL;
    v.window_fill = color::PANEL;
    v.extreme_bg_color = color::CANVAS;
    v.window_stroke = Stroke::new(1.0, color::BORDER);
    v.window_corner_radius = CornerRadius::same(10);
    v.override_text_color = Some(color::TEXT);
    v.selection.bg_fill = color::ACCENT.gamma_multiply(0.18);
    v.selection.stroke = Stroke::new(1.0, color::ACCENT);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, color::BORDER);
    w.noninteractive.corner_radius = RADIUS;
    for (state, fill) in [
        (&mut w.inactive, color::PANEL),
        (&mut w.hovered, color::HOVER),
        (&mut w.active, color::ACTIVE),
        (&mut w.open, color::ACTIVE),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.corner_radius = RADIUS;
        state.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 5.0);
    });
}
