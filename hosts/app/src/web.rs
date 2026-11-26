//! The application in a browser: the same [`App`](crate::App), drawn
//! with WebGL into the page's canvas. What a browser does differently is
//! here, in one place:
//!
//! - files live in memory ([`workpiece_core::files`]); "Open from this
//!   computer" reads files the user picks into it, and what is saved or
//!   exported is also handed over as a download. Nothing is uploaded.
//! - `?open=<url or name>` fetches a part file (and the parts it places,
//!   relative to it) from the site, a name being looked up in the
//!   examples' `index.json`; `?tab=drawing` then opens its drawing, and
//!   `?tab=<a ribbon's tab>` (`annotate`, `assembly`…) shows that tab.
//! - with no part open, a start screen lists the examples.
//! - `?assistant=1` opens the assistant's panel ([`crate::assistant`]).
//! - the settings are kept in the page's local storage.
//! - the build engine's threads are Web Workers ([`threads`]): each runs
//!   this same program on the page's memory, so the engine, its pool and
//!   what a build hands back are the desktop's. That needs a page that is
//!   cross-origin isolated (`website/_headers`); where it is not, parts
//!   build on the page's one thread, as they are asked for.
//!
//! Work that finishes later (a fetch, reading a picked file) leaves an
//! [`Event`] here that the next frame takes.

use std::cell::RefCell;
use std::collections::BTreeSet;

use eframe::egui;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use workpiece_core::files;
use workpiece_part::Part;
use workpiece_ui::Message;

use crate::theme::color;

/// Where the examples' index is, relative to the page (the app is served
/// at `/app/`, the examples at `/examples/`); `?examples=<url>` says
/// another.
const EXAMPLES: &str = "../examples/index.json";

/// The file types "Open from this computer" offers.
const ACCEPT: &str = ".wpc,.step,.stp,.igs,.iges,.stl,.3mf";

/// What the about text says of files.
const ABOUT: &str = "Workpiece runs in this browser: parts build on your computer. \
Files you open are read into the page's memory and nothing is uploaded anywhere; \
saving or exporting downloads the file.";

/// What happened on the page since the last frame.
enum Event {
    /// These files are in memory now: open the part at `path` (and,
    /// with `tab`, its first drawing or that tab of the ribbon).
    Open { path: String, tab: Option<String> },
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
///   "description": "…", "file": "flange/flange.wpc",
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
    /// The part to open (with the tab, or its drawing, to show), and whether
    /// a frame saying so was drawn: where parts build on the page's
    /// thread, it builds as it opens, which keeps the page busy.
    opening: Option<(String, Option<String>, bool)>,
    /// When the page started, and whether the first frame was drawn.
    started: f64,
    drawn: bool,
    /// The emoji fonts fetched so far, in [`EMOJI_FONTS`]' order.
    emoji: Vec<(&'static str, f32, Vec<u8>)>,
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

/// Counts `event` with `properties` in the site's statistics: the page's
/// `window.workpieceTrack` (`../analytics.js`, which sends nothing
/// without the site's key, or under Do Not Track or an opt-out). Never a
/// file's name or anything typed: kinds and counts only.
pub fn track(event: &str, properties: serde_json::Value) {
    let window = window();
    let Ok(f) = js_sys::Reflect::get(&window, &"workpieceTrack".into()) else {
        return;
    };
    let Some(f) = f.dyn_ref::<js_sys::Function>() else {
        return;
    };
    let properties = js_sys::JSON::parse(&properties.to_string()).unwrap_or(JsValue::UNDEFINED);
    let _ = f.call2(&window, &event.into(), &properties);
}

/// What kind of file `path` is, for the statistics: its extension, if
/// it is one of those the app knows.
pub fn kind_of(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    let known = [
        "wpc", "step", "stp", "igs", "iges", "stl", "3mf", "dxf", "svg", "pdf", "png", "tap", "nc",
    ];
    known
        .into_iter()
        .find(|k| lower.ends_with(&format!(".{k}")))
        .unwrap_or("other")
}

/// A query parameter of the page's address.
fn query(name: &str) -> Option<String> {
    let search = window().location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search)
        .ok()?
        .get(name)
        .filter(|v| !v.is_empty())
}

/// Starts the application in the page's canvas `workpiece`.
pub fn start() {
    page(|p| p.started = now());
    spawn_local(async {
        let runner = eframe::WebRunner::new();
        guard_panics();
        let Some(canvas) = window()
            .document()
            .and_then(|d| d.get_element_by_id("workpiece"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        else {
            log("Workpiece: the page has no canvas with the id workpiece");
            return;
        };
        let options = eframe::WebOptions {
            depth_buffer: 24,
            ..Default::default()
        };
        let create = Box::new(|cc: &eframe::CreationContext<'_>| {
            page(|p| p.ctx = Some(cc.egui_ctx.clone()));
            cc.egui_ctx.set_fonts(fonts(Vec::new()));
            crate::create(cc)
        });
        match runner.start(canvas, options, create).await {
            Ok(()) => {
                for font in EMOJI_FONTS {
                    spawn_local(emoji_font(font));
                }
                track("app_loaded", serde_json::json!({}));
                if let Some(loading) = window()
                    .document()
                    .and_then(|d| d.get_element_by_id("loading"))
                {
                    loading.remove();
                }
            }
            Err(e) => log(&format!("Workpiece did not start: {e:?}")),
        }
        // Leaving the page with work not saved asks first.
        let leaving = Closure::<dyn FnMut(web_sys::Event)>::new(|event: web_sys::Event| {
            if UNSAVED.get() {
                event.prevent_default();
                let _ = js_sys::Reflect::set(&event, &"returnValue".into(), &"".into());
            }
        });
        let _ = window()
            .add_event_listener_with_callback("beforeunload", leaving.as_ref().unchecked_ref());
        leaving.forget();
        // Read the examples, and open what the address asks for
        // (`?assistant` opens the assistant's panel too).
        spawn_local(read_examples());
        if query("assistant").is_some() {
            crate::assistant::toggle();
        }
        if let Some(open) = query("open") {
            spawn_local(open_named(open, query("tab")));
        }
    });
}

thread_local! {
    /// Whether a part of the workspace has work that is not saved.
    static UNSAVED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Says whether a part of the workspace has work that is not saved:
/// leaving the page then asks first.
pub fn set_unsaved(unsaved: bool) {
    UNSAVED.set(unsaved);
}

/// The emoji fonts egui has on the desktop, fetched from the page's
/// `fonts/` (build.sh puts them there): its name there, and how much
/// smaller than the text they are drawn (as egui's defaults have it).
const EMOJI_FONTS: [(&str, f32); 2] = [("NotoEmoji-Regular", 0.81), ("emoji-icon-font", 0.90)];

/// egui's fonts, as on the desktop, of which the page compiles in only
/// the text fonts (Ubuntu Light, Hack): the emoji fonts, half of the
/// fonts' bytes, come with `emoji` (as [`EMOJI_FONTS`] names
/// them) once fetched.
fn fonts(emoji: Vec<(&'static str, f32, Vec<u8>)>) -> egui::FontDefinitions {
    use egui::{FontData, FontFamily, FontTweak};
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "Hack".to_owned(),
        Arc::new(FontData::from_static(epaint_default_fonts::HACK_REGULAR)),
    );
    fonts.font_data.insert(
        "Ubuntu-Light".to_owned(),
        Arc::new(FontData::from_static(epaint_default_fonts::UBUNTU_LIGHT)),
    );
    let mut proportional = vec!["Ubuntu-Light".to_owned()];
    // (Ubuntu Light is the fallback for √ and the like.)
    let mut monospace = vec!["Hack".to_owned(), "Ubuntu-Light".to_owned()];
    for (name, scale, bytes) in emoji {
        let data = FontData::from_owned(bytes).tweak(FontTweak {
            scale,
            ..Default::default()
        });
        fonts.font_data.insert(name.to_owned(), Arc::new(data));
        proportional.push(name.to_owned());
        monospace.push(name.to_owned());
    }
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Monospace, monospace);
    fonts
}

/// Fetches one of the emoji fonts, and draws with it (and those fetched
/// before it) from then on.
async fn emoji_font((name, scale): (&'static str, f32)) {
    let bytes = match fetch(&format!("fonts/{name}.ttf")).await {
        Ok(bytes) => bytes,
        Err(e) => return log(&format!("Workpiece: no emoji font: {e}")),
    };
    let (ctx, emoji) = page(|p| {
        p.emoji.push((name, scale, bytes));
        // (In the order egui falls back to them.)
        p.emoji
            .sort_by_key(|f| EMOJI_FONTS.iter().position(|e| e.0 == f.0));
        (p.ctx.clone(), p.emoji.clone())
    });
    if let Some(ctx) = ctx {
        ctx.set_fonts(fonts(emoji));
        ctx.request_repaint();
    }
}

/// A panic ends the application in a browser (there is no unwinding
/// on `wasm32-unknown-unknown` with the standard library as shipped, so
/// a step that breaks cannot be caught as a failed step): the page says
/// so, with what broke, in place of a frozen canvas. (A panic on one of
/// the build threads, which have no page, is kept for the page to say:
/// see [`spawn`].)
fn guard_panics() {
    let theirs = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        theirs(info);
        match web_sys::window() {
            Some(_) => crashed(&info.to_string()),
            None => *workpiece_part::pool::lock(&BROKE) = Some(info.to_string()),
        }
    }));
}

/// What broke on a build thread, for the page to say.
static BROKE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Covers the page with what broke.
fn crashed(said: &str) {
    if let Some(element) = window()
        .document()
        .and_then(|d| d.get_element_by_id("crashed"))
    {
        if let Some(pre) = element.query_selector("pre").ok().flatten() {
            pre.set_text_content(Some(said));
        }
        let _ = element.set_attribute("style", "display: flex");
    }
}

/// The script a build thread runs (`website/app/thread.js`, beside the
/// page): it starts this program on the memory it is given and runs
/// [`thread`].
const THREAD_SCRIPT: &str = "thread.js";

thread_local! {
    /// The build threads' workers, held while the page is.
    static WORKERS: RefCell<Vec<web_sys::Worker>> = const { RefCell::new(Vec::new()) };
}

/// How many threads the build engine has beside the page's: all but one
/// of the computer's (at least two, so that a build that is no longer
/// wanted, which runs on to its next step, holds up none that is), where
/// the page can share its memory with workers — it is cross-origin
/// isolated, and the program was built with atomics (`build.sh`).
/// `None` where it cannot: parts build on the page's thread.
pub fn threads() -> Option<usize> {
    let isolated = js_sys::Reflect::get(&js_sys::global(), &"crossOriginIsolated".into())
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !(cfg!(target_feature = "atomics") && isolated) {
        log("Workpiece: not cross-origin isolated: parts build on the page's thread");
        return None;
    }
    workpiece_part::pool::set_spawner(spawn);
    let cores = window().navigator().hardware_concurrency() as usize;
    let threads = cores.saturating_sub(1).clamp(2, 8);
    log(&format!(
        "Workpiece: parts build on {threads} threads beside the page"
    ));
    Some(threads)
}

/// Starts a build thread: a Web Worker that runs `work` on this
/// program's memory.
fn spawn(work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
    let options = web_sys::WorkerOptions::new();
    options.set_type(web_sys::WorkerType::Module);
    options.set_name("workpiece-build");
    let worker =
        web_sys::Worker::new_with_options(THREAD_SCRIPT, &options).map_err(|e| js_text(&e))?;
    // (It says something only when it broke.)
    let broke = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(|event: web_sys::MessageEvent| {
        let kept = workpiece_part::pool::lock(&BROKE).take();
        crashed(&kept.unwrap_or_else(|| js_text(&event.data())));
    });
    worker.set_onmessage(Some(broke.as_ref().unchecked_ref()));
    broke.forget();
    let work = Box::into_raw(Box::new(work)) as usize as u32;
    let message = js_sys::Array::of3(
        &wasm_bindgen::module(),
        &wasm_bindgen::memory(),
        &JsValue::from(work),
    );
    worker.post_message(&message).map_err(|e| js_text(&e))?;
    WORKERS.with_borrow_mut(|w| w.push(worker));
    Ok(())
}

/// What a build thread runs, once its worker has started this program on
/// the page's memory: the work [`spawn`] was given.
#[wasm_bindgen(js_name = workpieceThread)]
#[allow(unsafe_code)]
pub fn thread(work: u32) {
    // SAFETY: `work` is what `spawn` made with `Box::into_raw`, handed
    // to this one worker, which runs it once.
    let work = unsafe { Box::from_raw(work as usize as *mut Box<dyn FnOnce() + Send>) };
    work();
}

/// What a link (or the start screen) asks for: a part file's URL, or the
/// name of an example.
async fn open_named(open: String, tab: Option<String>) {
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
                "Workpiece: fetched {url} in {:.0} ms",
                now() - asked
            ));
            send(Event::Open { path, tab });
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
        Err(e) => log(&format!("Workpiece: no examples: {e}")),
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
        if !workpiece_part::is_step(&path) {
            let text = String::from_utf8_lossy(&bytes);
            let part =
                Part::from_file(&path, &text).map_err(|e| format!("reading {path}: {e:#}"))?;
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
        track("file_opened", serde_json::json!({ "kind": kind_of(&name) }));
        if let Err(e) = files::write(&name, &bytes) {
            return send(Event::Failed(format!("reading {name}: {e}")));
        }
        if name.ends_with(&format!(".{}", workpiece_ops_instances::PART_EXTENSION)) {
            let placed = Part::from_file(&name, &String::from_utf8_lossy(&bytes))
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
            tab: None,
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
    // (A part file saved, or the model exported for other programs.)
    match kind_of(path) {
        "wpc" => track("file_saved", serde_json::json!({ "kind": "wpc" })),
        format => track("export", serde_json::json!({ "format": format })),
    }
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
            workpiece_ui::preview::pixels(part.fresh_preview()?)
        }),
    };
    send(Event::Thumbnail(example.name, pixels));
}

pub(crate) fn decode_png(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
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
fn open_drawing(model: &mut workpiece_ui::Model) -> anyhow::Result<()> {
    let Some(part) = model.active_part.clone() else {
        return Ok(());
    };
    let drawing = model.workspace.part(&part)?.program.iter().find_map(|op| {
        op.downcast_ref::<workpiece_ops_drawing::Drawing>()
            .map(|_| op.id().to_owned())
    });
    match drawing {
        Some(id) => model.update(Message::Edit(vec![id])),
        None => anyhow::bail!("{part} has no drawing"),
    }
}

thread_local! {
    /// What the last frame showed, for tests in a browser ([`state`]):
    /// whether a build ran beside the page, and where each note's text
    /// that can be dragged was (on the model, or on the drawing sheet).
    static SEEN: RefCell<(bool, Vec<(String, egui::Pos2)>)> = RefCell::default();
}

/// The text of the note `name` showed at `at` (in CSS pixels).
pub fn seen(name: &str, at: egui::Pos2) {
    SEEN.with_borrow_mut(|s| s.1.push((name.to_owned(), at)));
}

/// What the last frame showed, as JSON, for tests that drive the page
/// (`website/app/drag.mjs`): `{"building": bool, "notes": [{"name",
/// "x", "y"}]}`.
#[wasm_bindgen(js_name = workpieceSeen)]
pub fn state() -> String {
    SEEN.with_borrow(|(building, notes)| {
        let notes: Vec<serde_json::Value> = notes
            .iter()
            .map(|(name, at)| serde_json::json!({ "name": name, "x": at.x, "y": at.y }))
            .collect();
        serde_json::json!({ "building": building, "notes": notes }).to_string()
    })
}

impl crate::App {
    /// Takes what happened on the page since the last frame.
    pub(crate) fn web_frame(&mut self, ctx: &egui::Context) {
        let building = self.model.is_building();
        SEEN.with_borrow_mut(|s| *s = (building, Vec::new()));
        if !page(|p| std::mem::replace(&mut p.drawn, true)) {
            let started = page(|p| p.started);
            log(&format!(
                "Workpiece: first frame {:.0} ms after the start",
                now() - started
            ));
        }
        for event in page(|p| std::mem::take(&mut p.events)) {
            match event {
                Event::Open { path, tab } => page(|p| p.opening = Some((path, tab, false))),
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
            Some((path, tab, true)) => {
                let asked = now();
                let opened = self.file(crate::FileAction::Open(path.clone()));
                log(&format!(
                    "Workpiece: opened {path} in {:.0} ms{}",
                    now() - asked,
                    match self.model.is_building() {
                        true => " (building beside the page)",
                        false => ", built",
                    }
                ));
                let opened = opened.and_then(|()| match tab {
                    Some(tab) if tab.eq_ignore_ascii_case("drawing") => {
                        open_drawing(&mut self.model)
                    }
                    Some(tab) => match workpiece_ui::Tab::ALL
                        .into_iter()
                        .find(|t| t.label().eq_ignore_ascii_case(&tab))
                    {
                        Some(tab) => self.model.update(Message::SetTab(tab)),
                        None => anyhow::bail!("there is no tab {tab:?}"),
                    },
                    None => Ok(()),
                });
                if let Err(e) = opened {
                    self.error = Some(format!("{e:#}"));
                }
                page(|p| p.window = false);
            }
            Some((path, tab, false)) => {
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
                page(|p| p.opening = Some((path, tab, true)));
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
            ui.heading("Workpiece");
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
                track(
                    "example_opened",
                    serde_json::json!({ "example": example.name }),
                );
                spawn_local(open_named(example.file.clone(), None));
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
