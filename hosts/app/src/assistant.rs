//! The assistant's panel, in a browser only: a conversation with a model
//! that builds the open part by writing Python for the authoring layer.
//! The conversation, the session with the server, Turnstile and the
//! worker that runs the scripts are the page's (`website/app/assistant/`,
//! loaded the first time the panel opens); this draws what the page's
//! `window.workpieceAssistant` says, hands it what the user types, and
//! answers what it asks of the app: the open part (for the model to read
//! and look at), and a part a script built, which becomes the open part
//! as one change (undone as one).

use std::cell::RefCell;
use std::collections::BTreeMap;

use eframe::egui;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use workpiece_part::Part;
use workpiece_ui::{Message, Model};

use crate::theme::color;

/// The page's script, relative to the app's page.
const SCRIPT: &str = "assistant/assistant.js";

/// What the page says of the assistant (its `state()`).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    status: String,
    status_text: String,
    running: bool,
    verifying: bool,
    limits: Option<Limits>,
    entries: Vec<Entry>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct Limits {
    turns_left_today: u32,
    turns_per_day: u32,
}

/// One entry of the conversation.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct Entry {
    /// user, assistant, tool, notice or error.
    kind: String,
    text: String,
    /// A tool's answer (a script's report).
    detail: Option<String>,
    /// The script a tool ran.
    script: Option<String>,
    /// The pictures a tool answered with, by their id in the page.
    images: Vec<String>,
    running: bool,
    error: bool,
}

/// What the page asks of the app.
#[derive(Debug, serde::Deserialize)]
struct Request {
    id: f64,
    kind: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    part: String,
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    placed: BTreeMap<String, String>,
}

/// The panel's state between frames.
#[derive(Default)]
struct Panel {
    open: bool,
    /// Whether the page's script was asked for.
    loading: bool,
    /// Whether the page is told to have the app drawn when it changes.
    listening: bool,
    version: f64,
    state: State,
    prompt: String,
    pictures: BTreeMap<String, Option<egui::TextureHandle>>,
}

thread_local! {
    static PANEL: RefCell<Panel> = RefCell::default();
}

fn panel<T>(f: impl FnOnce(&mut Panel) -> T) -> T {
    PANEL.with_borrow_mut(f)
}

/// The page's `window.workpieceAssistant`, once its script ran.
fn bridge() -> Option<js_sys::Object> {
    let window = web_sys::window()?;
    js_sys::Reflect::get(&window, &"workpieceAssistant".into())
        .ok()
        .filter(|b| b.is_object())
        .map(JsCast::unchecked_into)
}

/// Calls `name` on the bridge with `args`.
fn call(bridge: &js_sys::Object, name: &str, args: &[JsValue]) -> JsValue {
    let Ok(f) = js_sys::Reflect::get(bridge, &name.into()) else {
        return JsValue::UNDEFINED;
    };
    let Some(f) = f.dyn_ref::<js_sys::Function>() else {
        return JsValue::UNDEFINED;
    };
    let args: js_sys::Array = args.iter().collect();
    f.apply(bridge, &args).unwrap_or(JsValue::UNDEFINED)
}

/// Whether the panel shows.
pub fn is_open() -> bool {
    panel(|p| p.open)
}

/// Opens or closes the panel; the first time, the page's script loads.
pub fn toggle() {
    let load = panel(|p| {
        p.open = !p.open;
        p.open && !std::mem::replace(&mut p.loading, true)
    });
    if load {
        let script = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.create_element("script").ok());
        if let Some(script) = script {
            let _ = script.set_attribute("type", "module");
            let _ = script.set_attribute("src", SCRIPT);
            let body = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.body());
            if let Some(body) = body {
                let _ = body.append_child(&script);
            }
        }
    }
}

/// The button that opens the panel, for the menu bar.
pub fn button(ui: &mut egui::Ui) {
    let open = is_open();
    let label =
        egui::RichText::new("Assistant").color(if open { color::ACCENT } else { color::TEXT });
    if ui
        .add(egui::Button::new(label).frame(false))
        .on_hover_text("A model that builds the part by writing a Python script for it (free, a few dozen turns a day)")
        .clicked()
    {
        toggle();
    }
}

/// Takes what the page changed and answers what it asks; draws the panel
/// if it is open. Call before the central panel.
pub fn show(ui: &mut egui::Ui, model: &mut Model, fit: &mut bool) {
    let Some(bridge) = bridge() else {
        if is_open() {
            side(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading the assistant…");
                });
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        return;
    };
    if !panel(|p| std::mem::replace(&mut p.listening, true)) {
        let ctx = ui.ctx().clone();
        let repaint = Closure::<dyn Fn()>::new(move || ctx.request_repaint());
        call(&bridge, "onChange", &[repaint.as_ref().clone()]);
        repaint.forget();
    }
    // What the page asks of the app.
    while let Some(text) = call(&bridge, "takeRequest", &[]).as_string() {
        let Ok(request) = serde_json::from_str::<Request>(&text) else {
            continue;
        };
        let answer = match request.kind.as_str() {
            "document" => document(model),
            "open" => match open(model, &request) {
                Ok(new) => {
                    *fit |= new;
                    serde_json::json!({ "ok": true })
                }
                Err(e) => serde_json::json!({ "error": format!("{e:#}") }),
            },
            _ => serde_json::json!({ "error": "unknown request" }),
        };
        call(
            &bridge,
            "answer",
            &[request.id.into(), answer.to_string().into()],
        );
    }
    let version = js_sys::Reflect::get(&bridge, &"version".into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or_default();
    if panel(|p| p.version != version) {
        if let Some(text) = call(&bridge, "state", &[]).as_string()
            && let Ok(state) = serde_json::from_str::<State>(&text)
        {
            panel(|p| p.state = state);
        }
        panel(|p| p.version = version);
    }
    if is_open() {
        side(ui, |ui| conversation(ui, &bridge));
    }
}

/// The panel at the right.
fn side(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Panel::right("assistant")
        .resizable(true)
        .default_size(380.0)
        .min_size(280.0)
        .max_size(640.0)
        .frame(
            egui::Frame::new()
                .fill(color::PANEL)
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ui, add);
}

/// The open part, as the page reads it: its path, how many steps it
/// has, the script it was made of, and every part file of the workspace
/// as text (it and what it places).
fn document(model: &Model) -> serde_json::Value {
    let Some(path) = model.active_part.clone() else {
        return serde_json::Value::Null;
    };
    let Ok(part) = model.workspace.part(&path) else {
        return serde_json::Value::Null;
    };
    let files: BTreeMap<&String, String> = model
        .workspace
        .parts
        .iter()
        .filter_map(|(at, part)| {
            // (Without the picture lists show: nothing builds from it.)
            let mut part = part.clone();
            part.preview = Default::default();
            Some((at, part.to_text().ok()?))
        })
        .collect();
    serde_json::json!({
        "path": path,
        "steps": part.program.len(),
        "script": part.script,
        "files": files,
    })
}

/// Makes the part a script built the open part (a new one if none is
/// open), with the script kept in it; whether a part was opened for it.
fn open(model: &mut Model, request: &Request) -> anyhow::Result<bool> {
    let mut part = Part::from_text(&request.part)?;
    part.script = request.script.clone();
    let mut placed = BTreeMap::new();
    for (path, text) in &request.placed {
        placed.insert(path.clone(), Part::from_text(text)?);
    }
    // (Only what the part places, and what that places.)
    let mut wanted = part.placed_parts();
    let mut kept = BTreeMap::new();
    while let Some(path) = wanted.pop() {
        if let Some(p) = placed.remove(&path) {
            wanted.extend(p.placed_parts());
            kept.insert(path, p);
        }
    }
    let new = model.active_part.is_none();
    if new {
        let mut path = request.path.clone();
        let mut n = 1;
        while model.workspace.parts.contains_key(&path) {
            n += 1;
            let stem = request.path.trim_end_matches(".wpc");
            path = format!("{stem}-{n}.wpc");
        }
        model.update(Message::New(path))?;
    }
    model.update(Message::ReplacePart {
        part: Box::new(part),
        placed: kept,
    })?;
    Ok(new)
}

/// The conversation, and the box to type in.
fn conversation(ui: &mut egui::Ui, bridge: &js_sys::Object) {
    let (status, status_text, running, verifying, turns) = panel(|p| {
        let s = &p.state;
        (
            s.status.clone(),
            s.status_text.clone(),
            s.running,
            s.verifying,
            s.limits
                .as_ref()
                .map(|l| (l.turns_left_today, l.turns_per_day)),
        )
    });
    ui.horizontal(|ui| {
        ui.strong("Assistant");
        if let Some((left, of)) = turns {
            ui.weak(format!("{left} of {of} turns left today"))
                .on_hover_text("Turns renew at 00:00 UTC");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Close")
                .on_hover_text("Close the assistant")
                .clicked()
            {
                toggle();
            }
            let empty = panel(|p| p.state.entries.is_empty());
            if ui
                .add_enabled(!empty, egui::Button::new("New chat").small())
                .on_hover_text("Start the conversation over (the part stays as it is)")
                .clicked()
            {
                call(bridge, "clear", &[]);
            }
        });
    });
    match status.as_str() {
        "starting" => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(&status_text);
            });
        }
        "paused" | "failed" => {
            ui.colored_label(color::ERROR, &status_text);
        }
        _ => {}
    }
    ui.separator();
    let bottom = 118.0;
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .max_height((ui.available_height() - bottom).max(80.0))
        .show(ui, |ui| {
            let count = panel(|p| p.state.entries.len());
            if count == 0 {
                ui.add_space(8.0);
                ui.weak(
                    "Describe a part — \"a 60 × 40 mm plate, 8 mm thick, with four M5 holes 6 mm \
                     from the corners\" — or ask for a change to the part that is open. The \
                     assistant writes a Python script for it, runs it here in your browser and \
                     opens the part it built; Undo takes it back.",
                );
            }
            for i in 0..count {
                entry(ui, bridge, i);
            }
            if verifying {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Checking that you are human…");
                });
            }
        });
    ui.separator();
    let usable = status != "paused" && status != "failed";
    let mut prompt = panel(|p| std::mem::take(&mut p.prompt));
    let edit = ui.add_enabled(
        usable,
        egui::TextEdit::multiline(&mut prompt)
            .hint_text("What should the part be?")
            .desired_rows(3)
            .char_limit(4000)
            .desired_width(f32::INFINITY),
    );
    let enter =
        edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
    let mut send = false;
    ui.horizontal(|ui| {
        if running {
            if ui
                .button("Stop")
                .on_hover_text("Stop this answer")
                .clicked()
            {
                call(bridge, "stop", &[]);
            }
            ui.spinner();
        } else {
            send = ui
                .add_enabled(
                    usable && !prompt.trim().is_empty(),
                    egui::Button::new("Send"),
                )
                .on_hover_text("Enter; Shift+Enter for a new line")
                .clicked();
        }
        ui.weak("Runs a free model; what you type is sent to Anthropic.");
    });
    if (send || enter) && !running && usable && !prompt.trim().is_empty() {
        call(bridge, "send", &[prompt.trim().into()]);
        prompt.clear();
    } else if enter {
        // (Enter while an answer runs keeps the text as it was.)
        prompt = prompt.trim_end_matches('\n').to_owned();
    }
    panel(|p| p.prompt = prompt);
}

/// The `i`th entry of the conversation.
fn entry(ui: &mut egui::Ui, bridge: &js_sys::Object, i: usize) {
    let Some((kind, text, detail, script, images, running, error)) = panel(|p| {
        p.state.entries.get(i).map(|e| {
            (
                e.kind.clone(),
                e.text.clone(),
                e.detail.clone(),
                e.script.clone(),
                e.images.clone(),
                e.running,
                e.error,
            )
        })
    }) else {
        return;
    };
    match kind.as_str() {
        "user" => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                egui::Frame::new()
                    .fill(color::ACCENT.gamma_multiply(0.12))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width() * 0.85);
                        ui.label(&text);
                    });
            });
        }
        "assistant" => {
            ui.label(&text);
        }
        "tool" => {
            let ink = if error {
                color::ERROR
            } else {
                color::TEXT_WEAK
            };
            let title = egui::RichText::new(&text).color(ink);
            egui::CollapsingHeader::new(title)
                .id_salt(("assistant tool", i))
                .default_open(false)
                .show(ui, |ui| {
                    if running {
                        ui.spinner();
                    }
                    if let Some(script) = &script {
                        ui.horizontal(|ui| {
                            ui.weak("The script");
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(script.clone());
                            }
                        });
                        code(ui, script, ("script", i));
                    }
                    if let Some(detail) = detail.as_ref().filter(|_| !running) {
                        if script.is_some() {
                            ui.weak("What it printed");
                        }
                        code(ui, detail, ("detail", i));
                    }
                });
            for id in &images {
                picture(ui, bridge, id);
            }
        }
        "error" => {
            ui.colored_label(color::ERROR, &text);
        }
        _ => {
            ui.weak(&text);
        }
    }
    ui.add_space(4.0);
}

/// Text in a fixed-width font, in a box of its own that scrolls.
fn code(ui: &mut egui::Ui, text: &str, salt: (&str, usize)) {
    egui::Frame::new()
        .fill(color::CANVAS)
        .corner_radius(4)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt(salt)
                .max_height(220.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).monospace().size(11.0)).extend(),
                    );
                });
        });
}

/// A picture the model looked at.
fn picture(ui: &mut egui::Ui, bridge: &js_sys::Object, id: &str) {
    let texture = match panel(|p| p.pictures.get(id).cloned()) {
        Some(texture) => texture,
        None => {
            let bytes = call(bridge, "image", &[id.into()]);
            if bytes.is_null() || bytes.is_undefined() {
                return;
            }
            let bytes = js_sys::Uint8Array::new(&bytes).to_vec();
            let texture = crate::web::decode_png(&bytes).map(|(w, h, rgba)| {
                let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                ui.ctx().load_texture(
                    format!("assistant {id}"),
                    image,
                    egui::TextureOptions::LINEAR,
                )
            });
            panel(|p| p.pictures.insert(id.to_owned(), texture.clone()));
            texture
        }
    };
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        let width = ui.available_width().min(size.x);
        ui.add(
            egui::Image::new(&texture)
                .fit_to_exact_size(egui::vec2(width, width * size.y / size.x))
                .corner_radius(4),
        );
    }
}
