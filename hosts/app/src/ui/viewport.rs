//! The 3D viewport: draws the scene with the [`Renderer`], turns pointer
//! input into messages, and shows the orientation cube and the projection
//! toggle at the top right.
//!
//! * Moving the pointer hovers; a right or middle click (or Tab) cycles
//!   the candidates under it; a left click confirms (with Ctrl or Shift,
//!   adds to the selection); a left drag on a handle drags it.
//! * A right or middle drag orbits, with Shift pans; the wheel zooms.
//! * The right or middle button held still for a moment opens the menu
//!   at the pointer; letting go over one of its items chooses it.

use std::sync::{Arc, Mutex};

use eframe::{egui, egui_glow};
use glam::Vec3;
use shapecraft_cad_rasterize::{Drawing, Pick, Scene};
use shapecraft_cad_ui::view::Viewport;
use shapecraft_cad_ui::{CameraMove, Message, Pointer};

use super::tip;
use crate::icons;
use crate::render::{Frame, Highlight, Layer, Renderer, SceneData};
use crate::theme::color;

/// Size of the orientation cube's area, in points.
const CUBE_AREA: f32 = 96.0;

/// The vertex data of the scenes drawn, each made once: kept by the
/// scene itself, and let go when a frame no longer draws it.
#[derive(Default)]
struct Cached {
    kept: std::collections::HashMap<usize, (u64, Arc<Scene>, Arc<SceneData>)>,
    next_id: u64,
}

impl Cached {
    /// The layers of `drawing`; `highlight` is theirs, if they have one.
    fn layers(&mut self, drawings: &[(&Drawing, Highlight)]) -> Vec<Vec<Layer>> {
        let mut used = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (drawing, highlight) in drawings {
            let mut layers = Vec::with_capacity(drawing.items.len());
            for item in &drawing.items {
                if item.scene.is_empty() {
                    continue;
                }
                let key = Arc::as_ptr(&item.scene) as usize;
                used.insert(key);
                let next = &mut self.next_id;
                let (id, _, data) = self.kept.entry(key).or_insert_with(|| {
                    *next += 1;
                    (
                        *next,
                        item.scene.clone(),
                        Arc::new(SceneData::new(&item.scene)),
                    )
                });
                let (rotation, translation) = (item.pose.rotation(), item.pose.translation());
                layers.push(Layer {
                    id: *id,
                    data: data.clone(),
                    highlight: *highlight,
                    model: glam::Mat4::from_rotation_translation(
                        rotation.as_quat(),
                        translation.as_vec3(),
                    ),
                    pick_base: item.base as f32,
                });
            }
            out.push(layers);
        }
        self.kept.retain(|key, _| used.contains(key));
        out
    }
}

/// What finding the candidates under the pointer depends on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct HoverKey {
    position: [f32; 2],
    size: [f32; 2],
    camera: shapecraft_cad_ui::model::Camera,
    /// The scene shown, by identity: it changes when the model rebuilds.
    scene: usize,
}

/// The viewport's drawing state.
pub struct ViewportPanel {
    renderer: Option<Arc<Mutex<Renderer>>>,
    error: Option<String>,
    scenes: Cached,
    /// How many frames were drawn.
    frames: u64,
    /// What the candidates under the pointer were last found for.
    hovered: Option<HoverKey>,
    /// The size last reported to the model.
    size: [f32; 2],
    /// The text field open in the scene: what it is for, what is typed,
    /// and why that was not taken.
    entry: Option<(String, String, Option<String>)>,
    /// What the right or middle button, while it is down, is doing.
    held: Option<Hold>,
}

/// What a right or middle button held down over the viewport is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hold {
    /// Nothing yet: it went down at this time (in seconds) and the
    /// pointer has not moved.
    Since(f64),
    /// The pointer moved first: it turns the view.
    Turning,
    /// It was held still: the menu is open.
    Menu,
}

/// How long the right or middle button is held still to open the menu,
/// in seconds, and how far the pointer may stray meanwhile, in points.
const HOLD_SECONDS: f64 = 0.25;
const HOLD_SLACK: f32 = 4.0;

impl ViewportPanel {
    /// A viewport drawing with `gl`; without it, or if the shaders do not
    /// compile, it says why instead.
    pub fn new(gl: Option<&eframe::glow::Context>) -> Self {
        let (renderer, error) = match gl.map(Renderer::new) {
            Some(Ok(r)) => (Some(Arc::new(Mutex::new(r))), None),
            Some(Err(e)) => (None, Some(format!("{e:#}"))),
            None => (None, Some("no OpenGL context".to_owned())),
        };
        Self {
            renderer,
            error,
            scenes: Cached::default(),
            frames: 0,
            hovered: None,
            size: [0.0, 0.0],
            entry: None,
            held: None,
        }
    }

    /// Draws `view` filling `ui`, collecting messages into `out`.
    pub fn show(&mut self, ui: &mut egui::Ui, view: &Viewport, out: &mut Vec<Message>) {
        let rect = ui.available_rect_before_wrap();
        let size = [rect.width(), rect.height()];
        if self.size != size {
            self.size = size;
            out.push(Message::ViewportSize(size));
        }
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        // What a click elsewhere enters comes before the click.
        self.entry(ui, rect, view, out);
        self.input(ui, rect, &response, view, out);
        self.paint(ui, rect, view);
        labels(ui, rect, view);
        overlay(ui, rect, view, out);
    }

    /// Takes the pointer's input over what is left of `ui` as the view
    /// takes it — turning, moving and zooming — and draws nothing: for
    /// what shows in place of the model, seen from the same camera.
    pub fn navigate(&mut self, ui: &mut egui::Ui, view: &Viewport, out: &mut Vec<Message>) {
        let rect = ui.available_rect_before_wrap();
        let size = [rect.width(), rect.height()];
        if self.size != size {
            self.size = size;
            out.push(Message::ViewportSize(size));
        }
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        self.input(ui, rect, &response, view, out);
    }

    /// The text field the edited step has open in the scene: a
    /// dimension's value, typed where the dimension is. Enter, or a click
    /// elsewhere, enters the text; Escape (a shortcut) cancels.
    fn entry(&mut self, ui: &egui::Ui, rect: egui::Rect, view: &Viewport, out: &mut Vec<Message>) {
        let Some(entry) = &view.entry else {
            self.entry = None;
            return;
        };
        let fresh = self.entry.as_ref().map(|e| &e.0) != Some(&entry.key);
        if fresh {
            self.entry = Some((entry.key.clone(), entry.text.clone(), None));
        }
        let Some((_, text, error)) = &mut self.entry else {
            return;
        };
        let clip = view
            .camera
            .view_proj(rect.width() / rect.height().max(1.0))
            .project_point3(entry.position);
        let at = rect.min
            + egui::vec2(
                (clip.x + 1.0) / 2.0 * rect.width(),
                (1.0 - clip.y) / 2.0 * rect.height(),
            );
        if !at.x.is_finite() || !at.y.is_finite() {
            return;
        }
        const WIDTH: f32 = 96.0;
        let mut entered = false;
        egui::Area::new(egui::Id::new("viewport entry"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(at)
            .constrain_to(rect)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::same(3))
                    .show(ui, |ui| {
                        let edit = egui::TextEdit::singleline(text)
                            .desired_width(WIDTH)
                            .horizontal_align(egui::Align::Center);
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
                        if output.response.changed() {
                            *error = None;
                        }
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let away = ui.input(|i| i.pointer.any_pressed());
                        entered = output.response.lost_focus() && (enter || away);
                        if entered && enter {
                            // Stays for another try if the text is not taken.
                            output.response.request_focus();
                        }
                        if let Some(error) = error {
                            ui.set_max_width(WIDTH + 60.0);
                            ui.colored_label(color::ERROR, error.as_str());
                        }
                    });
            });
        if entered {
            match (entry.enter)(text) {
                Ok(message) => out.extend([message, Message::EndEdit]),
                Err(why) => *error = Some(why),
            }
        }
    }

    fn input(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        response: &egui::Response,
        view: &Viewport,
        out: &mut Vec<Message>,
    ) {
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let dragging = response.dragged_by(egui::PointerButton::Primary);
        let over = pointer.is_some_and(|p| rect.contains(p))
            && !cube_area(rect).contains(pointer.unwrap_or_default());
        // While the view is being turned nothing is looked for under the
        // pointer: that is found again when the view rests.
        let turning = matches!(self.held, Some(Hold::Turning));
        match pointer {
            Some(p) if (over || dragging) && !turning => {
                let position = p - rect.min;
                let key = HoverKey {
                    position: [position.x, position.y],
                    size: [rect.width(), rect.height()],
                    camera: view.camera,
                    scene: Arc::as_ptr(&view.scene) as usize,
                };
                // Only a real change finds the candidates again (which
                // starts at the first), so a resting pointer keeps the one
                // cycled to; with nothing under it, keep looking.
                if self.hovered != Some(key) || view.candidate.is_none() {
                    out.push(Message::Pointer(Pointer::Moved {
                        position: key.position,
                        size: key.size,
                    }));
                    self.hovered = Some(key);
                }
            }
            _ if self.hovered.is_some() => {
                out.push(Message::Pointer(Pointer::Left));
                self.hovered = None;
            }
            _ => {}
        }
        if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            out.push(Message::Pointer(Pointer::Press));
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            out.push(Message::Pointer(Pointer::DragStart));
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) {
            out.push(Message::Pointer(Pointer::DragEnd));
        }
        if response.double_clicked() {
            out.push(Message::Pointer(Pointer::DoubleClick));
        } else if response.clicked() {
            // Ctrl (or Shift) adds to, or removes from, the selection.
            let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            out.push(Message::Pointer(Pointer::Click { add }));
        }
        // The right and the middle button do the same. A click goes on
        // to the next candidate under the pointer; a drag turns the view
        // (with Shift, moves it). Held still for a moment, the button
        // opens the menu instead, and stays down while the pointer goes
        // to an item: the host chooses it when the button is let go.
        let others = [egui::PointerButton::Secondary, egui::PointerButton::Middle];
        let (down, pressed, origin, now, time) = ui.input(|i| {
            let p = &i.pointer;
            (
                p.secondary_down() || p.middle_down(),
                others.iter().any(|&b| p.button_pressed(b)),
                p.press_origin(),
                p.latest_pos(),
                i.time,
            )
        });
        if pressed && response.hovered() {
            self.held = Some(Hold::Since(time));
        }
        match self.held {
            _ if !down => {
                // A short press that went nowhere was a click.
                let clicked = others.iter().any(|&b| response.clicked_by(b));
                if clicked && matches!(self.held, Some(Hold::Since(_))) {
                    out.push(Message::Pointer(Pointer::Cycle));
                }
                self.held = None;
            }
            Some(Hold::Since(since)) => {
                let moved = origin
                    .zip(now)
                    .is_some_and(|(from, to)| from.distance(to) > HOLD_SLACK);
                if moved {
                    self.held = Some(Hold::Turning);
                } else if time - since >= HOLD_SECONDS {
                    self.held = Some(Hold::Menu);
                    if let Some(p) = origin {
                        let at = p - rect.min;
                        out.push(Message::ContextMenu(Some([at.x, at.y])));
                    }
                } else {
                    // Time passes without input: look again when it has.
                    ui.ctx().request_repaint();
                }
            }
            Some(Hold::Turning) => {
                let d = ui.input(|i| i.pointer.delta());
                if d != egui::Vec2::ZERO {
                    let shift = ui.input(|i| i.modifiers.shift);
                    out.push(Message::Camera(if shift {
                        CameraMove::Pan([d.x, d.y])
                    } else {
                        CameraMove::Orbit([d.x, d.y])
                    }));
                }
            }
            Some(Hold::Menu) | None => {}
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                out.push(Message::Camera(CameraMove::Zoom(scroll * 2.0)));
            }
        }
    }

    fn paint(&mut self, ui: &egui::Ui, rect: egui::Rect, view: &Viewport) {
        let Some(renderer) = self.renderer.clone() else {
            let text = self.error.as_deref().unwrap_or("no renderer");
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(14.0),
                color::ERROR,
            );
            return;
        };
        let selected = view.selected.iter().map(|p| drawn_id(&view.scene, Some(p)));
        let highlight = Highlight::new(drawn_id(&view.scene, view.hovered.as_ref()), selected);
        let mut layers = self.scenes.layers(&[
            (view.scene.as_ref(), highlight),
            (view.ghosts.as_ref(), Highlight::NONE),
        ]);
        let ghosts = layers.pop().unwrap_or_default();
        let scene = layers.pop().unwrap_or_default();
        let camera = &view.camera;
        let frame = Frame {
            view_proj: camera.view_proj(rect.width() / rect.height().max(1.0)),
            eye: camera.eye(),
            back: camera.back(),
            orthographic: camera.orthographic,
            analysis: {
                use shapecraft_cad_ui::model::display::Analysis;
                match view.analysis {
                    Analysis::None => 0.0,
                    Analysis::Zebra => 1.0,
                    Analysis::Curvature => 2.0,
                    Analysis::Draft => 3.0,
                }
            },
            reach: camera.distance,
            background: color::CANVAS,
            scene,
            ghosts,
            handles: Layer {
                // Made anew every frame, so uploaded anew every frame:
                // an id of its own each time, and never one of a scene
                // that is kept.
                id: {
                    self.frames = self.frames.wrapping_add(1);
                    u64::MAX - self.frames % (u64::MAX / 2)
                },
                data: Arc::new(SceneData::new(&view.handles)),
                highlight: Highlight::new(id_of(&view.handles, view.hovered.as_ref()), []),
                model: glam::Mat4::IDENTITY,
                pick_base: 0.0,
            },
        };
        let callback = egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                if let Ok(mut renderer) = renderer.lock() {
                    renderer.paint(painter.gl(), &info, &frame);
                }
            })),
        };
        ui.painter().add(callback);
    }
}

/// The id of `pick` in `drawing` as the shaders take it.
fn drawn_id(drawing: &Drawing, pick: Option<&Pick>) -> f32 {
    pick.and_then(|p| drawing.index_of(p))
        .map_or(-1.0, |id| id as f32)
}

/// The id of `pick` in `scene` as the shaders take it.
fn id_of(scene: &Scene, pick: Option<&Pick>) -> f32 {
    pick.and_then(|p| scene.index_of(p))
        .map_or(-1.0, |id| id as f32)
}

fn cube_area(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(rect.right() - CUBE_AREA - 8.0, rect.top() + 8.0),
        egui::vec2(CUBE_AREA, CUBE_AREA + 30.0),
    )
}

/// The candidate label at the pointer, the error, the orientation cube
/// and the projection toggle.
fn overlay(ui: &mut egui::Ui, rect: egui::Rect, view: &Viewport, out: &mut Vec<Message>) {
    let painter = ui.painter_at(rect);
    if let (Some((i, n, text)), Some(pointer)) =
        (&view.candidate, ui.input(|i| i.pointer.latest_pos()))
    {
        let text = if *n > 1 {
            format!("{text}   {}/{n}", i + 1)
        } else {
            text.clone()
        };
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), color::TEXT);
        let at = pointer + egui::vec2(14.0, 14.0);
        let bg = egui::Rect::from_min_size(at, galley.size()).expand2(egui::vec2(6.0, 3.0));
        painter.rect_filled(bg, 4.0, color::PANEL.gamma_multiply(0.92));
        painter.rect_stroke(
            bg,
            4.0,
            egui::Stroke::new(1.0, color::BORDER),
            egui::StrokeKind::Inside,
        );
        painter.galley(at, galley, color::TEXT);
    }
    if let Some(notice) = &view.notice {
        painter.text(
            rect.left_bottom() + egui::vec2(12.0, -34.0),
            egui::Align2::LEFT_BOTTOM,
            notice,
            egui::FontId::proportional(13.0),
            color::TEXT,
        );
    }
    if let Some(error) = &view.error {
        painter.text(
            rect.left_bottom() + egui::vec2(12.0, -12.0),
            egui::Align2::LEFT_BOTTOM,
            error,
            egui::FontId::proportional(13.0),
            color::ERROR,
        );
    }
    notes(ui, &painter, rect, view, out);
    display_bar(ui, rect, view, out);
    let area = cube_area(rect);
    cube(
        ui,
        egui::Rect::from_min_size(area.min, egui::vec2(CUBE_AREA, CUBE_AREA)),
        view,
        out,
    );
    // Home and the projection, as icons under the cube.
    let width = if view.normal.is_some() { 102.0 } else { 68.0 };
    let buttons = egui::Rect::from_min_size(
        egui::pos2(area.center().x - width / 2.0, area.top() + CUBE_AREA + 2.0),
        egui::vec2(width, 28.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(buttons), |ui| {
        ui.horizontal(|ui| {
            let button = |icon| {
                egui::Button::image(icons::widget(icon, 18.0, color::TEXT))
                    .fill(color::PANEL)
                    .stroke(egui::Stroke::new(1.0, color::BORDER))
            };
            if let Some(normal) = &view.normal {
                let hint = tip(
                    &normal.label,
                    normal.hotkey,
                    "Face the sketch plane square on",
                );
                if ui
                    .add(button(icons::NORMAL_VIEW))
                    .on_hover_text(hint)
                    .clicked()
                {
                    out.push(normal.click());
                }
            }
            let hint = tip(
                &view.home.label,
                view.home.hotkey,
                "Back to the starting view",
            );
            if ui.add(button(icons::HOME)).on_hover_text(hint).clicked() {
                out.push(view.home.click());
            }
            let (icon, now) = if view.projection.label == "Perspective" {
                (icons::ORTHOGRAPHIC, "The view is orthographic")
            } else {
                (icons::PERSPECTIVE, "The view is in perspective")
            };
            let hint = tip(&view.projection.label, view.projection.hotkey, now);
            if ui.add(button(icon)).on_hover_text(hint).clicked() {
                out.push(view.projection.click());
            }
        });
    });
}

/// What is said of the model on it, drawn as a drawing has it (ISO
/// 129-1, ISO 1101, ISO 5459) in the plane each note lies in:
/// extension and dimension lines with their arrowheads and the value
/// over the line, datum triangles and tolerance frames. A note's text
/// is dragged to stand elsewhere, as far from the eye as it was.
fn notes(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    rect: egui::Rect,
    view: &Viewport,
    out: &mut Vec<Message>,
) {
    if view.notes.is_empty() {
        return;
    }
    let view_proj = view.camera.view_proj(rect.width() / rect.height().max(1.0));
    // A point of the clip space on the screen.
    let on_screen = |c: glam::Vec2| {
        rect.min
            + egui::vec2(
                (c.x + 1.0) / 2.0 * rect.width(),
                (1.0 - c.y) / 2.0 * rect.height(),
            )
    };
    let screen = |p: Vec3| shapecraft_cad_ui::view::project(view_proj, p).map(on_screen);
    for note in &view.notes {
        // What is behind the eye, or has none of its place on the
        // screen, is not drawn: dividing by its depth would put it
        // anywhere.
        let Some(at) = screen(note.at) else {
            continue;
        };
        let corners: Vec<egui::Pos2> = note.grip.iter().filter_map(|p| screen(*p)).collect();
        let mut grip = egui::Rect::from_points(&corners).expand(3.0);
        if note.texts.is_empty() || corners.len() < 4 || !grip.is_finite() {
            grip = egui::Rect::from_center_size(at, egui::vec2(24.0, 14.0));
        }
        let response = ui
            .interact(
                grip,
                ui.id().with(("note", &note.name)),
                if note.movable {
                    egui::Sense::drag()
                } else {
                    egui::Sense::hover()
                },
            )
            .on_hover_text(format!("{}: {}", note.name, note.text))
            .on_hover_cursor(if note.movable {
                egui::CursorIcon::Grab
            } else {
                egui::CursorIcon::Default
            });
        if response.dragged() || response.drag_stopped() {
            // By as much as the pointer moved, at the depth it stood
            // at.
            let to = at + response.drag_delta();
            let depth = view_proj.project_point3(note.at).z;
            let clip = Vec3::new(
                (to.x - rect.min.x) / rect.width() * 2.0 - 1.0,
                1.0 - (to.y - rect.min.y) / rect.height() * 2.0,
                depth,
            );
            let to = view_proj.inverse().project_point3(clip);
            if to.is_finite() {
                out.push(Message::MoveNote {
                    name: note.name.clone(),
                    at: to.as_dvec3().to_array(),
                    settled: response.drag_stopped(),
                });
            }
        }
        let ink = match response.hovered() || response.dragged() {
            true => color::ACCENT,
            false => color::TEXT,
        };
        let thin = egui::Stroke::new(1.0, ink);
        for (a, b) in &note.strokes {
            // (Cut at the near plane: nothing is drawn from behind.)
            if let [a, b] = shapecraft_cad_ui::view::clip_polygon(view_proj, &[*a, *b])[..] {
                painter.line_segment([on_screen(a), on_screen(b)], thin);
            }
        }
        for fill in &note.fills {
            let corners: Vec<egui::Pos2> = shapecraft_cad_ui::view::clip_polygon(view_proj, fill)
                .into_iter()
                .map(on_screen)
                .collect();
            if corners.len() < 3 {
                continue;
            }
            // Filled as plain triangles, with a thin edge for the
            // smoothing: the painter's own smoothing of a filled shape
            // pushes the corners of one that is seen nearly edge-on out
            // by more than any size (and by all of it when it is flat),
            // which fills the screen.
            let mut mesh = egui::Mesh::default();
            for corner in &corners {
                mesh.colored_vertex(*corner, ink);
            }
            for k in 1..corners.len() as u32 - 1 {
                mesh.add_triangle(0, k, k + 1);
            }
            painter.add(egui::Shape::mesh(mesh));
            // (Its edges as single segments: a path round a flat shape
            // turns back on itself, and the joins of a path that does
            // are as long as the painter's smoothing likes.)
            for k in 0..corners.len() {
                let edge = [corners[k], corners[(k + 1) % corners.len()]];
                painter.line_segment(edge, egui::Stroke::new(0.8, ink));
            }
        }
        for text in &note.texts {
            let (Some(p), Some(q)) = (screen(text.at), screen(text.towards)) else {
                continue;
            };
            // Along its line as that is seen; read from below or from
            // the right, never upside down.
            let way = (q - p).normalized();
            let way = if way.x < -0.02 || (way.x.abs() <= 0.02 && way.y > 0.0) {
                -way
            } else {
                way
            };
            let angle = if way.length() > 0.5 {
                way.y.atan2(way.x)
            } else {
                0.0
            };
            let size = (text.height / 0.7).clamp(6.0, 40.0);
            let galley =
                painter.layout_no_wrap(text.text.clone(), egui::FontId::proportional(size), ink);
            let (sin, cos) = angle.sin_cos();
            let turned = |v: egui::Vec2| egui::vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
            let half = galley.size() / 2.0;
            let width = galley.size().x;
            painter
                .add(egui::epaint::TextShape::new(p - turned(half), galley, ink).with_angle(angle));
            // The deviations, small, one above the other after the
            // value (as the sketcher sets them).
            for (small, lift) in [(&text.upper, -4.5), (&text.lower, 4.5)] {
                if small.is_empty() {
                    continue;
                }
                let galley = painter.layout_no_wrap(
                    small.clone(),
                    egui::FontId::proportional(SMALL_TEXT_SIZE),
                    ink,
                );
                let at = p + turned(egui::vec2(
                    width / 2.0 + 3.0 + small.chars().count() as f32 * SMALL_TEXT_SIZE * 0.3,
                    lift,
                ));
                let corner = at - turned(galley.size() / 2.0);
                painter.add(egui::epaint::TextShape::new(corner, galley, ink).with_angle(angle));
            }
        }
    }
}

/// How the model is displayed, along the top of the view: cut open
/// square to an axis (and where), and what its faces are painted to
/// show.
fn display_bar(ui: &mut egui::Ui, rect: egui::Rect, view: &Viewport, out: &mut Vec<Message>) {
    use shapecraft_cad_ui::model::display::Analysis;
    let bar = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(10.0, 8.0),
        egui::vec2((rect.width() - CUBE_AREA - 30.0).max(120.0), 26.0),
    );
    let mut display = view.display.clone();
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
        // One row of the bar's height, its buttons centred in it: left
        // to themselves (`horizontal`) each is placed by the height the
        // row has when it comes, so those before the first taller one
        // sit higher.
        let row = egui::Layout::left_to_right(egui::Align::Center);
        ui.with_layout(row, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            // A whole number of pixels, so none is placed on a half one.
            ui.spacing_mut().interact_size.y = 24.0;
            // A drop-down of the bar: what it shows chosen from a list.
            let drop = |ui: &mut egui::Ui, id: &str, on: bool, text: &str, hint: &str, list: &mut dyn FnMut(&mut egui::Ui)| {
                let text = egui::RichText::new(text).size(11.5).color(if on {
                    color::ACCENT
                } else {
                    color::TEXT
                });
                egui::ComboBox::from_id_salt(id)
                    .selected_text(text)
                    .width(84.0)
                    .show_ui(ui, |ui| list(ui))
                    .response
                    .on_hover_text(hint);
            };
            // Cut open as a section of the part cuts it, or at the plane
            // a sketch is drawn on.
            {
                let now = display.cut.clone();
                let cut = display.plane.is_some();
                let text = match (&now, cut) {
                    (_, true) => "Cut: sketch plane".to_owned(),
                    (Some(n), _) => format!("Cut: {n}"),
                    _ => "Section".to_owned(),
                };
                let hint = "Draw the model cut open: at the plane a sketch is drawn on, or as a section of the part cuts it (a Section step: Annotate tab)";
                let mut sketching = view.sketch_section;
                drop(ui, "bar-section", now.is_some() || cut, &text, hint, &mut |ui| {
                    if ui
                        .selectable_label(now.is_none() && !cut, "Whole")
                        .clicked()
                    {
                        display.cut = None;
                        display.plane = None;
                    }
                    for name in &view.sections {
                        if ui.selectable_label(now.as_ref() == Some(name), name).clicked() {
                            display.cut = Some(name.clone());
                            display.plane = None;
                        }
                    }
                    ui.separator();
                    ui.checkbox(&mut sketching, "Section view while sketching")
                        .on_hover_text("Cut the model open at the plane a sketch is drawn on");
                });
                if sketching != view.sketch_section {
                    out.push(Message::SetSketchSection(sketching));
                }
            }
            if !view.views.is_empty() {
                let hint = "Look at the model as a camera view of the part does";
                drop(ui, "bar-view", false, "View", hint, &mut |ui| {
                    for name in &view.views {
                        if ui.selectable_label(false, name).clicked() {
                            out.push(Message::LookFrom(name.clone()));
                        }
                    }
                });
            }
            let ways = [
                (Analysis::None, "None", "The faces as they are"),
                (Analysis::Zebra, "Zebra", "Stripes mirrored in the faces: they break where faces do not run on smoothly"),
                (Analysis::Curvature, "Curvature", "How tightly the faces bend: blue where flat, to red"),
                (Analysis::Draft, "Draft", "The faces' draft from a pull along z: green with draft, yellow under 3°, red none or undercut"),
            ];
            let now = ways.iter().find(|(way, ..)| *way == display.analysis);
            let on = display.analysis != Analysis::None;
            let text = match now {
                Some((_, label, _)) if on => (*label).to_owned(),
                _ => "Analysis".to_owned(),
            };
            let hint = "Paint the faces to show how they run: zebra stripes, curvature, draft";
            drop(ui, "bar-analysis", on, &text, hint, &mut |ui| {
                for (way, label, hint) in ways {
                    ui.selectable_value(&mut display.analysis, way, label)
                        .on_hover_text(hint);
                }
            });
        });
    });
    if display != view.display {
        out.push(Message::SetDisplay(display));
    }
}

/// The orientation cube: a cube turned like the scene, its faces named;
/// clicking a face looks at the scene from that side.
fn cube(ui: &mut egui::Ui, rect: egui::Rect, view: &Viewport, out: &mut Vec<Message>) {
    let camera = &view.camera;
    let (right, up, back) = (camera.right(), camera.up(), camera.back());
    let center = rect.center();
    let scale = rect.width() * 0.28;
    let project = |p: Vec3| center + egui::vec2(p.dot(right), -p.dot(up)) * scale;
    let faces: [(Vec3, &str); 6] = [
        (Vec3::Z, "Top"),
        (Vec3::NEG_Z, "Bottom"),
        (Vec3::X, "Front"),
        (Vec3::NEG_X, "Back"),
        (Vec3::Y, "Right"),
        (Vec3::NEG_Y, "Left"),
    ];
    let response = ui.interact(rect, ui.id().with("orientation-cube"), egui::Sense::click());
    let pointer = response.hover_pos();
    let painter = ui.painter_at(rect);
    // Far faces first.
    let mut visible: Vec<_> = faces.iter().filter(|(n, _)| n.dot(back) > 1e-3).collect();
    visible.sort_by(|a, b| a.0.dot(back).total_cmp(&b.0.dot(back)));
    let mut hovered = None;
    let mut polygons = Vec::new();
    for &&(n, label) in &visible {
        // The face's corners: ±u ± v around n.
        let u = if n.z.abs() > 0.5 {
            Vec3::X
        } else {
            Vec3::Z.cross(n).normalize()
        };
        let v = n.cross(u);
        let corners: Vec<egui::Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|&(a, b)| project(n + u * a + v * b))
            .collect();
        if pointer.is_some_and(|p| contains(&corners, p)) {
            hovered = Some(n);
        }
        polygons.push((n, label, corners));
    }
    // The axes, from the cube's back corner, under the faces.
    let origin = Vec3::new(-1.0, -1.0, -1.0);
    for (axis, c) in [
        (Vec3::X, egui::Color32::from_rgb(200, 80, 75)),
        (Vec3::Y, egui::Color32::from_rgb(95, 160, 82)),
        (Vec3::Z, egui::Color32::from_rgb(74, 120, 200)),
    ] {
        painter.line_segment(
            [project(origin), project(origin + axis * 2.4)],
            egui::Stroke::new(2.0, c),
        );
    }
    for (n, label, corners) in polygons {
        let facing = n.dot(back);
        let shade = 0.82 + 0.18 * facing;
        let fill = if hovered == Some(n) {
            // Opaque, so the hovered face does not show the far ones.
            mix(egui::Color32::from_gray(245), color::ACCENT, 0.35)
        } else {
            egui::Color32::from_gray((245.0 * shade) as u8)
        };
        painter.add(egui::Shape::convex_polygon(
            corners.clone(),
            fill,
            egui::Stroke::new(1.0, color::TEXT_WEAK),
        ));
        let mid = corners
            .iter()
            .fold(egui::Vec2::ZERO, |s, p| s + p.to_vec2())
            / 4.0;
        if facing > 0.35 {
            painter.text(
                mid.to_pos2(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(10.5 * facing.sqrt()),
                color::TEXT,
            );
        }
    }
    if response.clicked()
        && let Some(n) = hovered
    {
        out.push(Message::Camera(CameraMove::LookFrom(n.to_array())));
    }
}

/// Whether the convex polygon `corners` contains `p`.
fn contains(corners: &[egui::Pos2], p: egui::Pos2) -> bool {
    let mut sign = 0.0;
    for (k, &a) in corners.iter().enumerate() {
        let b = corners[(k + 1) % corners.len()];
        let cross = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// `a` mixed towards `b` by `t`, opaque.
fn mix(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    egui::Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Size of dimension texts and badges, in points.
const TEXT_SIZE: f32 = 13.5;
const SMALL_TEXT_SIZE: f32 = 9.5;
const BADGE_SIZE: f32 = 11.0;

/// The texts of the edited step's handles: dimensions, written along
/// their lines and upright (with deviations set small after them), and
/// constraint badges in small boxes.
fn labels(ui: &egui::Ui, rect: egui::Rect, view: &Viewport) {
    if view.handles.labels.is_empty() {
        return;
    }
    let painter = ui.painter_at(rect);
    let view_proj = view.camera.view_proj(rect.width() / rect.height().max(1.0));
    let project = |p: Vec3| {
        let clip = view_proj.project_point3(p);
        rect.min
            + egui::vec2(
                (clip.x + 1.0) / 2.0 * rect.width(),
                (1.0 - clip.y) / 2.0 * rect.height(),
            )
    };
    for label in &view.handles.labels {
        let at = project(label.position);
        if !at.x.is_finite() || !at.y.is_finite() {
            continue;
        }
        let [r, g, b, a] = label.color;
        let tint: egui::Color32 = egui::Rgba::from_rgba_unmultiplied(r, g, b, a).into();
        if label.badge {
            let font = egui::FontId::monospace(BADGE_SIZE);
            let galley = painter.layout_no_wrap(label.text.clone(), font, tint);
            let frame = egui::Rect::from_center_size(at, galley.size() + egui::vec2(8.0, 4.0));
            painter.rect_filled(frame, 4.0, color::PANEL.gamma_multiply(0.95));
            painter.rect_stroke(
                frame,
                4.0,
                egui::Stroke::new(1.0, tint),
                egui::StrokeKind::Inside,
            );
            painter.galley(at - galley.size() / 2.0, galley, tint);
            continue;
        }
        // The baseline's direction on screen; written so it reads upright.
        let step = view.camera.distance * 0.01;
        let along = project(label.position + label.along * step) - at;
        let mut angle = along.y.atan2(along.x);
        if !angle.is_finite() {
            angle = 0.0;
        }
        if angle.cos() < -1e-3 {
            angle += std::f32::consts::PI;
        }
        let (sin, cos) = angle.sin_cos();
        let turned = |v: egui::Vec2| egui::vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
        let write = |text: &str, size: f32, offset: egui::Vec2| -> f32 {
            let font = egui::FontId::proportional(size);
            let galley = painter.layout_no_wrap(text.to_owned(), font, tint);
            let width = galley.size().x;
            let center = at + turned(offset);
            let shape = egui::epaint::TextShape::new(center - galley.size() / 2.0, galley, tint)
                .with_angle_and_anchor(angle, egui::Align2::CENTER_CENTER);
            painter.add(shape);
            width
        };
        let width = write(&label.text, TEXT_SIZE, egui::Vec2::ZERO);
        // The deviations, small, one above the other after the value.
        let small = |text: &str| text.chars().count() as f32 * SMALL_TEXT_SIZE * 0.3;
        if !label.upper.is_empty() {
            write(
                &label.upper,
                SMALL_TEXT_SIZE,
                egui::vec2(width / 2.0 + 3.0 + small(&label.upper), -4.5),
            );
        }
        if !label.lower.is_empty() {
            write(
                &label.lower,
                SMALL_TEXT_SIZE,
                egui::vec2(width / 2.0 + 3.0 + small(&label.lower), 4.5),
            );
        }
    }
}
