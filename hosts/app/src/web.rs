//! The application in a browser: the same [`App`](crate::App), drawn
//! with WebGL into the page's canvas. What a browser does differently is
//! here, in one place:
//!
//! - files live in memory ([`shapecraft_core::files`]); "Open from this
//!   computer" reads files the user picks into it, and what is saved or
//!   exported is also handed over as a download. Nothing is uploaded.
//! - `?open=<url or name>` fetches a part file (and the parts it places,
//!   relative to it) from the site, a name being looked up in the
//!   examples' `index.json`; `?tab=drawing` then opens its drawing.
//! - with no part open, a start screen lists the examples.
//! - the settings are kept in the page's local storage.
//! - there are no threads: parts build on the page's one thread.
//!
//! Work that finishes later (a fetch, reading a picked file) leaves an
//! [`Event`] here that the next frame takes.

use std::cell::RefCell;
use std::collections::BTreeSet;

use eframe::egui;
use shapecraft_cad_part::Part;
use shapecraft_cad_ui::Message;
use shapecraft_core::files;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::theme::color;

/// Where the examples' index is, relative to the page (the app is served
/// at `/app/`, the examples at `/examples/`); `?examples=<url>` says
/// another.
const EXAMPLES: &str = "../examples/index.json";

/// The file types "Open from this computer" offers.
const ACCEPT: &str = ".shapecraft,.step,.stp,.igs,.iges,.stl,.3mf";

/// What the about text says of files.
const ABOUT: &str = "ShapeCraft runs in this browser: parts build on your computer. \
Files you open are read into the page's memory and nothing is uploaded anywhere; \
saving or exporting downloads the file.";

/// What happened on the page since the last frame.
enum Event {
    /// These files are in memory now: open the part at `path` (and,
    /// with `drawing`, its first drawing).
    Open { path: String, drawing: bool },
    /// The examples' index was read.
    Examples(Vec<Example>),
    /// The thumbnail of the example with this name: width, height, RGBA.
    Thumbnail(String, Option<(usize, usize, Vec<u8>)>),
    /// Something failed, as said.
    Failed(String),
}

/// An example as the examples' `index.json` lists it.
///
/// ```json
/// { "examples": [ { "name": "flange", "title": "Flange",
///   "description": "…", "file": "flange/flange.shapecraft",
///   "thumbnail": "flange/flange.png", "tags": ["revolve"] } ] }
/// ```
///
/// `file` and `thumbnail` are relative to the index; without a
/// thumbnail, the preview the part file holds is shown.
#[derive(Clone, Debug, serde::Deserialize)]
struct Example {
    name: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    file: String,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Index {
    examples: Vec<Example>,
}

/// The page's state between frames.
#[derive(Default)]
struct Page {
    events: Vec<Event>,
    /// To be asked to draw again when an event comes.
    ctx: Option<egui::Context>,
    /// The examples, once read, with where their index is.
    examples: Option<Vec<Example>>,
    /// Thumbnails asked for, and those there are.
    asked: BTreeSet<String>,
    thumbnails: std::collections::BTreeMap<String, Option<egui::TextureHandle>>,
    /// Whether the examples' window is open (over a part).
    window: bool,
    /// The part to open (with whether its drawing is to be), and whether
    /// a frame saying so was drawn: it builds as it opens, which keeps
    /// the page busy.
    opening: Option<(String, bool, bool)>,
    /// When the page started, and whether the first frame was drawn.
    started: f64,
    drawn: bool,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::default();
}

fn page<T>(f: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with_borrow_mut(f)
}

/// Leaves `event` for the next frame, and has it drawn.
fn send(event: Event) {
    let ctx = page(|p| {
        p.events.push(event);
        p.ctx.clone()
    });
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

fn window() -> web_sys::Window {
    web_sys::window().expect("a browser window")
}

/// Milliseconds since the page started loading.
fn now() -> f64 {
    window().performance().map_or(0.0, |p| p.now())
}

/// Writes `text` to the browser's console.
fn log(text: &str) {
    web_sys::console::log_1(&text.into());
}

/// A query parameter of the page's address.
fn query(name: &str) -> Option<String> {
    let search = window().location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search)
        .ok()?
        .get(name)
        .filter(|v| !v.is_empty())
}

/// Starts the application in the page's canvas `shapecraft`.
pub fn start() {
    page(|p| p.started = now());
    spawn_local(async {
        let runner = eframe::WebRunner::new();
        guard_panics();
        let Some(canvas) = window()
            .document()
            .and_then(|d| d.get_element_by_id("shapecraft"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        else {
            log("ShapeCraft: the page has no canvas with the id shapecraft");
            return;
        };
        let options = eframe::WebOptions {
            depth_buffer: 24,
            ..Default::default()
        };
        let create = Box::new(|cc: &eframe::CreationContext<'_>| {
            page(|p| p.ctx = Some(cc.egui_ctx.clone()));
            crate::create(cc)
        });
        match runner.start(canvas, options, create).await {
            Ok(()) => {
                if let Some(loading) = window()
                    .document()
                    .and_then(|d| d.get_element_by_id("loading"))
                {
                    loading.remove();
                }
            }
            Err(e) => log(&format!("ShapeCraft did not start: {e:?}")),
        }
        // Read the examples, and open what the address asks for.
        spawn_local(read_examples());
        if let Some(open) = query("open") {
            let drawing = query("tab").is_some_and(|t| t.eq_ignore_ascii_case("drawing"));
            spawn_local(open_named(open, drawing));
        }
    });
}

/// A panic ends the application in a browser (there is no unwinding
/// on `wasm32-unknown-unknown` with the standard library as shipped, so
/// a step that breaks cannot be caught as a failed step): the page says
/// so, with what broke, in place of a frozen canvas.
fn guard_panics() {
    let theirs = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        theirs(info);
        if let Some(element) = window()
            .document()
            .and_then(|d| d.get_element_by_id("crashed"))
        {
            let said = element.query_selector("pre").ok().flatten();
            if let Some(said) = said {
                said.set_text_content(Some(&info.to_string()));
            }
            let _ = element.set_attribute("style", "display: flex");
        }
    }));
}

/// What a link (or the start screen) asks for: a part file's URL, or the
/// name of an example.
async fn open_named(open: String, drawing: bool) {
    let url = if open.contains('/') || open.contains('.') {
        open.clone()
    } else {
        match examples().await {
            Ok(list) => match list.iter().find(|e| e.name == open) {
                Some(example) => example.file.clone(),
                None => {
                    return send(Event::Failed(format!("there is no example named {open}")));
                }
            },
            Err(e) => return send(Event::Failed(e)),
        }
    };
    let asked = now();
    match fetch_part(&url).await {
        Ok(path) => {
            log(&format!(
                "ShapeCraft: fetched {url} in {:.0} ms",
                now() - asked
            ));
            send(Event::Open { path, drawing });
        }
        Err(e) => send(Event::Failed(e)),
    }
}

/// The examples' index, the files it names as URLs.
async fn examples() -> Result<Vec<Example>, String> {
    let url = query("examples").unwrap_or_else(|| EXAMPLES.to_owned());
    let base = url
        .rsplit_once('/')
        .map_or(String::new(), |b| format!("{}/", b.0));
    let bytes = fetch(&url).await?;
    let index: Index = serde_json::from_slice(&bytes).map_err(|e| format!("reading {url}: {e}"))?;
    let mut list = index.examples;
    for example in &mut list {
        example.file = format!("{base}{}", example.file);
        if let Some(thumbnail) = &mut example.thumbnail {
            *thumbnail = format!("{base}{thumbnail}");
        }
    }
    Ok(list)
}

async fn read_examples() {
    match examples().await {
        Ok(list) => send(Event::Examples(list)),
        // (A site without examples just shows none.)
        Err(e) => log(&format!("ShapeCraft: no examples: {e}")),
    }
}

/// What the file at `url` holds.
async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let said = |e: JsValue| format!("fetching {url}: {}", js_text(&e));
    let response = JsFuture::from(window().fetch_with_str(url))
        .await
        .map_err(said)?;
    let response: web_sys::Response = response.dyn_into().map_err(said)?;
    if !response.ok() {
        return Err(format!(
            "fetching {url}: {} {}",
            response.status(),
            response.status_text()
        ));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(said)?)
        .await
        .map_err(said)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

fn js_text(value: &JsValue) -> String {
    value
        .as_string()
        .or_else(|| {
            value
                .dyn_ref::<js_sys::Error>()
                .map(|e| String::from(e.message()))
        })
        .unwrap_or_else(|| format!("{value:?}"))
}

/// Fetches the part file at `url` into memory, and the files it places
/// (relative to it), and theirs: what was in memory already is kept.
/// The part's path in memory is its name, the folder it is in being the
/// working directory.
async fn fetch_part(url: &str) -> Result<String, String> {
    let (base, name) = match url.rsplit_once('/') {
        Some((base, name)) => (format!("{base}/"), name.to_owned()),
        None => (String::new(), url.to_owned()),
    };
    let mut todo = vec![name.clone()];
    let mut seen = BTreeSet::new();
    while let Some(path) = todo.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let bytes = match files::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                let bytes = fetch(&format!("{base}{path}")).await?;
                files::write(&path, &bytes).map_err(|e| e.to_string())?;
                bytes
            }
        };
        if !shapecraft_cad_part::is_step(&path) {
            let text = String::from_utf8_lossy(&bytes);
            let part = Part::from_text(&text).map_err(|e| format!("reading {path}: {e:#}"))?;
            todo.extend(part.placed_parts());
        }
    }
    Ok(name)
}

/// Asks the user for files from their computer (with `folder`, a whole
/// folder: an assembly with its parts), reads them into memory and opens
/// the part among them that no other places.
pub fn pick(folder: bool) {
    let Some(input) = window()
        .document()
        .and_then(|d| d.create_element("input").ok())
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        return;
    };
    input.set_type("file");
    input.set_multiple(true);
    if folder {
        input.set_webkitdirectory(true);
    } else {
        input.set_accept(ACCEPT);
    }
    let picked = input.clone();
    let changed = Closure::once(move || {
        if let Some(list) = picked.files() {
            spawn_local(read_picked(list, folder));
        }
    });
    input.set_onchange(Some(changed.as_ref().unchecked_ref()));
    changed.forget();
    input.click();
}

async fn read_picked(list: web_sys::FileList, folder: bool) {
    let mut parts = Vec::new();
    let mut others = Vec::new();
    for i in 0..list.length() {
        let Some(file) = list.get(i) else { continue };
        // In a folder, paths are relative to it.
        let name = match folder {
            true => js_sys::Reflect::get(&file, &"webkitRelativePath".into())
                .ok()
                .and_then(|p| p.as_string())
                .and_then(|p| p.split_once('/').map(|p| p.1.to_owned()))
                .unwrap_or_else(|| file.name()),
            false => file.name(),
        };
        let buffer = match JsFuture::from(file.array_buffer()).await {
            Ok(buffer) => buffer,
            Err(e) => return send(Event::Failed(format!("reading {name}: {}", js_text(&e)))),
        };
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        if let Err(e) = files::write(&name, &bytes) {
            return send(Event::Failed(format!("reading {name}: {e}")));
        }
        if name.ends_with(&format!(".{}", shapecraft_ops_instances::PART_EXTENSION)) {
            let placed = Part::from_text(&String::from_utf8_lossy(&bytes))
                .map(|p| p.placed_parts())
                .unwrap_or_default();
            parts.push((name, placed));
        } else {
            others.push(name);
        }
    }
    let placed: BTreeSet<&String> = parts.iter().flat_map(|p| &p.1).collect();
    let top = parts
        .iter()
        .find(|p| !placed.contains(&p.0))
        .or(parts.first());
    match top {
        Some((path, _)) => send(Event::Open {
            path: path.clone(),
            drawing: false,
        }),
        None if !others.is_empty() => send(Event::Failed(format!(
            "{} {} in memory now: a STEP, IGES, STL or 3MF file is placed into a part \
             (Assembly) or imported (Import), from its name",
            others.join(", "),
            if others.len() == 1 { "is" } else { "are" }
        ))),
        None => {}
    }
}

/// Hands `bytes` to the user as a download named as `path`'s file.
pub fn download(path: &str, bytes: &[u8]) {
    let name = path.rsplit('/').next().unwrap_or(path);
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/octet-stream");
    let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options) else {
        return;
    };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else {
        return;
    };
    if let Some(a) = window()
        .document()
        .and_then(|d| d.create_element("a").ok())
        .and_then(|e| e.dyn_into::<web_sys::HtmlAnchorElement>().ok())
    {
        a.set_href(&url);
        a.set_download(name);
        a.click();
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}

/// What the page's local storage holds under `key`.
pub fn stored(key: &str) -> Option<String> {
    window().local_storage().ok()??.get_item(key).ok()?
}

/// Keeps `value` in the page's local storage under `key`.
pub fn store(key: &str, value: &str) {
    if let Ok(Some(storage)) = window().local_storage() {
        let _ = storage.set_item(key, value);
    }
}

/// The thumbnail of `example`: its picture, or the preview its part file
/// holds.
async fn thumbnail(example: Example) {
    let pixels = match &example.thumbnail {
        Some(url) => fetch(url).await.ok().and_then(|png| decode_png(&png)),
        None => fetch(&example.file).await.ok().and_then(|bytes| {
            let part = Part::from_text(&String::from_utf8_lossy(&bytes)).ok()?;
            shapecraft_cad_ui::preview::pixels(part.fresh_preview()?)
        }),
    };
    send(Event::Thumbnail(example.name, pixels));
}

fn decode_png(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer[..w * h * 4].to_vec(),
        png::ColorType::Rgb => buffer[..w * h * 3]
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buffer[..w * h * 2]
            .chunks(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        png::ColorType::Grayscale => buffer[..w * h]
            .iter()
            .flat_map(|&c| [c, c, c, 255])
            .collect(),
        png::ColorType::Indexed => return None,
    };
    Some((w, h, rgba))
}

/// Opens the first drawing of the active part.
fn open_drawing(model: &mut shapecraft_cad_ui::Model) -> anyhow::Result<()> {
    let Some(part) = model.active_part.clone() else {
        return Ok(());
    };
    let drawing = model.workspace.part(&part)?.program.iter().find_map(|op| {
        op.downcast_ref::<shapecraft_ops_drawing::Drawing>()
            .map(|_| op.id().to_owned())
    });
    match drawing {
        Some(id) => model.update(Message::Edit(vec![id])),
        None => anyhow::bail!("{part} has no drawing"),
    }
}

impl crate::App {
    /// Takes what happened on the page since the last frame.
    pub(crate) fn web_frame(&mut self, ctx: &egui::Context) {
        if !page(|p| std::mem::replace(&mut p.drawn, true)) {
            let started = page(|p| p.started);
            log(&format!(
                "ShapeCraft: first frame {:.0} ms after the start",
                now() - started
            ));
        }
        for event in page(|p| std::mem::take(&mut p.events)) {
            match event {
                Event::Open { path, drawing } => page(|p| p.opening = Some((path, drawing, false))),
                Event::Examples(list) => page(|p| p.examples = Some(list)),
                Event::Thumbnail(name, pixels) => {
                    let texture = pixels.map(|(w, h, rgba)| {
                        let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                        ctx.load_texture(
                            format!("example {name}"),
                            image,
                            egui::TextureOptions::LINEAR,
                        )
                    });
                    page(|p| p.thumbnails.insert(name, texture));
                }
                Event::Failed(e) => self.error = Some(e),
            }
        }
        match page(|p| p.opening.take()) {
            Some((path, drawing, true)) => {
                let asked = now();
                let opened = self.file(crate::FileAction::Open(path.clone()));
                log(&format!(
                    "ShapeCraft: opened and built {path} in {:.0} ms",
                    now() - asked
                ));
                let opened = opened.and_then(|()| match drawing {
                    true => open_drawing(&mut self.model),
                    false => Ok(()),
                });
                if let Err(e) = opened {
                    self.error = Some(format!("{e:#}"));
                }
                page(|p| p.window = false);
            }
            Some((path, drawing, false)) => {
                egui::Area::new(egui::Id::new("opening"))
                    .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(format!("Opening and building {path}…"));
                            });
                        });
                    });
                page(|p| p.opening = Some((path, drawing, true)));
                ctx.request_repaint();
            }
            None => {}
        }
        if page(|p| p.window) {
            let mut open = true;
            egui::Window::new("Examples")
                .open(&mut open)
                .collapsible(false)
                .default_size(egui::vec2(720.0, 480.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, examples_grid);
                });
            if !open {
                page(|p| p.window = false);
            }
        }
    }
}

/// The browser's own items of the File menu.
pub fn file_menu(ui: &mut egui::Ui) {
    if ui
        .button("Open from this computer…")
        .on_hover_text("Read a part file (or a STEP, IGES, STL or 3MF file) into the page")
        .clicked()
    {
        pick(false);
        ui.close();
    }
    if ui
        .button("Open a folder…")
        .on_hover_text("Read a folder of part files into the page: an assembly with its parts")
        .clicked()
    {
        pick(true);
        ui.close();
    }
    if ui.button("Examples…").clicked() {
        page(|p| p.window = true);
        ui.close();
    }
}

/// What shows while no part is open: the examples, to open with a click,
/// and what the page does with files.
pub fn start_screen(ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.heading("ShapeCraft");
            ui.label("A parametric CAD program, here running in your browser.");
            ui.add_space(4.0);
            ui.weak(ABOUT);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let width = 3.0 * 190.0;
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                if ui.button("Open from this computer…").clicked() {
                    pick(false);
                }
                if ui.button("Open a folder…").clicked() {
                    pick(true);
                }
                ui.weak("or File > New");
            });
        });
        ui.add_space(16.0);
        ui.separator();
        examples_grid(ui);
    });
}

/// The examples as cards: a thumbnail over the title; a click opens one.
fn examples_grid(ui: &mut egui::Ui) {
    const CARD: egui::Vec2 = egui::vec2(220.0, 210.0);
    const THUMB: egui::Vec2 = egui::vec2(208.0, 140.0);
    let Some(list) = page(|p| p.examples.clone()) else {
        ui.weak("Examples: loading…");
        return;
    };
    if list.is_empty() {
        ui.weak("This site has no examples.");
        return;
    }
    ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
    ui.horizontal_wrapped(|ui| {
        for example in &list {
            let (rect, response) = ui.allocate_exact_size(CARD, egui::Sense::click());
            let mut hover = example.description.clone();
            if !example.tags.is_empty() {
                hover = format!("{hover}\n\n{}", example.tags.join(", "));
            }
            let response = response.on_hover_text(hover.trim());
            if response.clicked() {
                spawn_local(open_named(example.file.clone(), false));
            }
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let fill = if response.hovered() {
                color::HOVER
            } else {
                color::PANEL
            };
            ui.painter().rect(
                rect,
                8.0,
                fill,
                egui::Stroke::new(1.0, color::BORDER),
                egui::StrokeKind::Inside,
            );
            let thumb = egui::Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), THUMB);
            let asked = page(|p| p.asked.insert(example.name.clone()));
            if asked {
                spawn_local(thumbnail(example.clone()));
            }
            match page(|p| p.thumbnails.get(&example.name).cloned()) {
                Some(Some(texture)) => {
                    let size = texture.size_vec2();
                    let scale = (THUMB.x / size.x).min(THUMB.y / size.y);
                    let at = egui::Rect::from_center_size(thumb.center(), size * scale);
                    egui::Image::new(&texture).corner_radius(5).paint_at(ui, at);
                }
                _ => {
                    ui.painter().rect_filled(thumb, 5.0, color::HOVER);
                }
            }
            let title = if example.title.is_empty() {
                &example.name
            } else {
                &example.title
            };
            let text = |text: &str, size: f32, ink: egui::Color32, width: f32| {
                let font = egui::FontId::proportional(size);
                let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, ink, width);
                job.wrap.max_rows = 2;
                job
            };
            let galley =
                ui.fonts_mut(|f| f.layout_job(text(title, 14.0, color::TEXT, CARD.x - 12.0)));
            let at = egui::pos2(rect.left() + 8.0, thumb.bottom() + 6.0);
            let below = at.y + galley.size().y + 2.0;
            ui.painter().galley(at, galley, color::TEXT);
            let galley = ui.fonts_mut(|f| {
                f.layout_job(text(
                    &example.description,
                    11.5,
                    color::TEXT_WEAK,
                    CARD.x - 16.0,
                ))
            });
            ui.painter().galley(
                egui::pos2(rect.left() + 8.0, below),
                galley,
                color::TEXT_WEAK,
            );
        }
    });
}
