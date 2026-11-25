//! The workpiece desktop application.
//!
//! Each frame derives a [`View`] from the [`Model`] and draws it: the menu
//! and the ribbon at the top, the timeline and the part's contents on the
//! left, the viewport in the centre, and the edit popup over it while a
//! step is edited. What the user does becomes [`Message`]s, applied to the
//! model once the frame is drawn, so the next frame shows their effect.
//! Files are the host's job: it reads and writes part files in the working
//! directory and tells the model.
//!
//! Every button has a hotkey, shown when the pointer rests on it; the
//! view lists the keys that do something now
//! ([`View::hotkeys`]), and the host presses what they stand for. Besides
//! those: Tab cycles the candidates under the pointer; Ctrl+N makes a new
//! part, Ctrl+O opens, Ctrl+S saves, Ctrl+Shift+S saves under another
//! name, Ctrl+W closes; Ctrl+Y redoes, like Ctrl+Shift+Z; Escape first
//! closes the context menu, stops picking, or clears the selection.
//!
//! With `WORKPIECE_DEMO=1` the app opens a small assembly instead of a new
//! part; with `WORKPIECE_DEMO=edit` it also edits the pin of a placed part,
//! with `WORKPIECE_DEMO=frame` its frame, with `WORKPIECE_DEMO=plane` a new plane
//! on it; with `WORKPIECE_DEMO=sketch` it opens a plate with a sketch on it,
//! being edited; with `WORKPIECE_DEMO=blends` a bracket with rounded and chamfered edges;
//! with `WORKPIECE_DEMO=joints` an assembly with joints, its
//! parts ready to be dragged.

// A Windows release build is a window application: no console opens beside it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod demo;
mod files;
mod icons;
mod render;
mod theme;
mod ui;
#[cfg(target_arch = "wasm32")]
mod web;

use eframe::egui;
use workpiece_core::{Placement, Widget};
use workpiece_ui::{Message, Model, View};

use ui::file_dialog::{FileDialog, Purpose};
use ui::viewport::ViewportPanel;

/// What `--version` and `--help` print (on Windows a release build has no
/// console to print to, so the Python launcher answers those itself).
#[cfg(not(target_arch = "wasm32"))]
fn about() -> bool {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => println!("Workpiece {}", env!("CARGO_PKG_VERSION")),
        Some("--help" | "-h") => println!(
            "Workpiece {}\n\nusage: workpiece-app [PART.wpc] [--look X,Y,Z,DISTANCE] [--from X,Y,Z] [--perspective]\n\nStarts the desktop application; a part file named is opened.",
            env!("CARGO_PKG_VERSION")
        ),
        _ => return false,
    }
    true
}

/// The window's icon: the picture the Windows executable and the macOS bundle carry too.
#[cfg(not(target_arch = "wasm32"))]
fn icon() -> Option<egui::IconData> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../assets/workpiece.png"
    )))
    .read_info()
    .ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    if about() {
        return Ok(());
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Workpiece")
        // (As large as the screen; where nothing makes it so, small
        // enough to fit on most: what is at its right edge — the
        // dialogs — on the screen.)
        .with_inner_size([1280.0, 800.0])
        .with_maximized(true);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        depth_buffer: 24,
        multisampling: 4,
        viewport,
        ..Default::default()
    };
    eframe::run_native("Workpiece", options, Box::new(create)).map_err(|e| anyhow::anyhow!("{e}"))
}

/// In a browser, the application starts in the page (see [`web`]).
#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

/// The application, as eframe makes it: on desktop and in a browser.
fn create(
    cc: &eframe::CreationContext<'_>,
) -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
    theme::apply(&cc.egui_ctx);
    egui_extras::install_image_loaders(&cc.egui_ctx);
    Ok(Box::new(App::new(cc.gl.as_deref())?))
}

/// What the host does with files, after the frame's messages.
enum FileAction {
    /// Make a new part, named after the first free `untitled` file.
    New,
    /// Show the file dialog to open a part.
    Browse,
    /// Read the part at this path (and what it places) and open it.
    Open(String),
    /// Write the active part to its file, or ask for one.
    Save,
    /// Ask where to write the active part from now on.
    SaveAsDialog,
    /// Write the active part to this path from now on.
    SaveAs(String),
    /// Write the bodies shown to this path, for other programs: as STEP,
    /// or (named `.stl`) as STL.
    Export(String),
    /// Read the part at this path (and what it places) and put it in the
    /// edit dialog's file field at this widget path.
    Choose(Vec<usize>, String),
}

/// The application state handed to eframe.
struct App {
    model: Model,
    viewport: ViewportPanel,
    /// The drawing workbench.
    sheet: ui::sheet::SheetPanel,
    /// The picture workbench.
    picture: ui::picture::PicturePanel,
    /// The materials panel.
    materials: ui::materials::MaterialsPanel,
    /// The library of materials as its file has it.
    library: workpiece_part::Library,
    /// The properties panel.
    properties: ui::properties::PropertiesPanel,
    /// The sheet templates panel.
    templates: ui::templates::TemplatesPanel,
    /// The sheet templates and the properties parts have, as their
    /// files have them.
    kept: (workpiece_ops_drawing::Templates, workpiece_part::Schema),
    /// The settings as their file has them.
    settings: workpiece_ui::model::Settings,
    /// The view a camera view that is edited would be taken from, and
    /// since when the view rests there.
    rested: Option<(workpiece_ui::model::Camera, web_time::Instant)>,
    /// Whether the view is still to be put onto all of the part just
    /// opened, once that is built.
    fit: bool,
    /// `--from x,y,z` after the file: the direction the part is first
    /// looked at from (towards the eye), once it is fitted.
    from: Option<[f32; 3]>,
    /// `--perspective` after the file: the view starts in perspective.
    perspective: bool,
    file_dialog: Option<FileDialog>,
    /// The previews of part files, as the file dialog shows them.
    previews: ui::previews::Previews,
    /// The last file error, shown until dismissed.
    error: Option<String>,
    title: String,
}

/// Has `model` build its parts beside the interface, on all but one of
/// the machine's threads: the window stays alive while they do. (In a
/// browser there are no threads to build on: there, parts build as they
/// are asked for.)
fn threaded(model: &mut Model) {
    if workpiece_core::HAS_THREADS {
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get());
        model.engine = workpiece_part::Engine::threaded(threads.saturating_sub(1).max(1));
        model.background = true;
    }
}

impl App {
    /// A new part (or the demo).
    fn new(gl: Option<&eframe::glow::Context>) -> anyhow::Result<Self> {
        let model = match std::env::var("WORKPIECE_DEMO").ok().as_deref() {
            Some("edit") => demo::model(Some("pin"))?,
            Some("frame") => demo::model(Some("top"))?,
            Some("plane") => demo::model(Some("plane"))?,
            Some("sketch") => demo::sketch()?,
            Some("joints") => demo::joints()?,
            Some("blends") => demo::blends()?,
            Some(_) => demo::model(None)?,
            // A file named on the command line is opened.
            None => match std::env::args().nth(1) {
                Some(path) => {
                    let parts = files::load(&path, &workpiece_part::Workspace::default())?;
                    let mut model = Model::new(workpiece_part::Workspace::new(parts));
                    // (Built beside the interface from the start: a part
                    // that takes long to build opens a window at once.)
                    threaded(&mut model);
                    model.update(Message::Open(path))?;
                    model
                }
                // (In a browser, the start screen shows first.)
                None if cfg!(target_arch = "wasm32") => Model::default(),
                None => {
                    let mut model = Model::default();
                    model.update(Message::New(files::untitled(&model.workspace)))?;
                    model
                }
            },
        };
        // `--look x,y,z,distance` after the file: what the camera looks
        // at to begin with, and from how far.
        let mut model = model;
        let look: Vec<f32> = std::env::args()
            .skip_while(|a| a != "--look")
            .nth(1)
            .map(|l| l.split(',').filter_map(|v| v.trim().parse().ok()).collect())
            .unwrap_or_default();
        // A part named is looked at whole, unless told where to look.
        let flag = |name: &str| std::env::args().any(|a| a == name);
        let from = std::env::args()
            .skip_while(|a| a != "--from")
            .nth(1)
            .map(|l| {
                l.split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect::<Vec<f32>>()
            })
            .and_then(|v| <[f32; 3]>::try_from(v).ok());
        let perspective = flag("--perspective");
        let fit = look.len() != 4 && std::env::args().nth(1).is_some();
        if let &[x, y, z, distance] = &look[..] {
            model.viewport.camera.target = [x, y, z].into();
            model.viewport.camera.distance = distance;
        }
        let mut model = model;
        if !model.background {
            threaded(&mut model);
        }
        // The materials are those of the working directory's library.
        let library = files::library()?;
        model.workspace.library = library.clone();
        // And its sheet templates, and the properties its parts have.
        let kept = (files::templates()?, files::schema()?);
        model.workspace.sheets = kept.0.clone();
        model.workspace.schema = kept.1.clone();
        let settings = files::settings();
        model.settings = settings.clone();
        Ok(Self {
            model,
            settings,
            viewport: ViewportPanel::new(gl),
            sheet: ui::sheet::SheetPanel::default(),
            picture: ui::picture::PicturePanel::default(),
            materials: ui::materials::MaterialsPanel::default(),
            library,
            properties: ui::properties::PropertiesPanel::default(),
            templates: ui::templates::TemplatesPanel::default(),
            kept,
            rested: None,
            fit,
            from,
            perspective,
            file_dialog: None,
            previews: Default::default(),
            error: None,
            title: String::new(),
        })
    }

    /// Applies `message`, reporting what fails.
    fn apply(&mut self, message: Message) {
        if let Err(e) = self.model.update(message) {
            self.error = Some(format!("{e:#}"));
        }
    }

    fn file(&mut self, action: FileAction) -> anyhow::Result<()> {
        match action {
            FileAction::New => {
                let name = files::untitled(&self.model.workspace);
                self.model.update(Message::New(name))?;
            }
            FileAction::Browse => self.file_dialog = Some(FileDialog::new(Purpose::Open, "")),
            FileAction::Open(path) => {
                let parts = files::load(&path, &self.model.workspace)?;
                self.model.update(Message::Load {
                    parts,
                    open: Some(path),
                })?;
                self.fit = true;
            }
            FileAction::Save => {
                let Some(path) = self.model.active_part.clone() else {
                    return Ok(());
                };
                if self.model.saved.contains_key(&path) {
                    self.model.take_preview(&path)?;
                    files::save(&path, self.model.workspace.part(&path)?)?;
                    self.model.update(Message::Saved(path))?;
                } else {
                    self.file_dialog = Some(FileDialog::new(Purpose::SaveAs, &path));
                }
            }
            FileAction::SaveAsDialog => {
                let name = self.model.active_part.clone().unwrap_or_default();
                self.file_dialog = Some(FileDialog::new(Purpose::SaveAs, &name));
            }
            FileAction::SaveAs(path) => {
                self.model.update(Message::SaveAs(path.clone()))?;
                self.model.take_preview(&path)?;
                files::save(&path, self.model.workspace.part(&path)?)?;
                self.model.update(Message::Saved(path))?;
            }
            FileAction::Export(path) => {
                let world = self.model.viewport.shown.world.clone();
                let topo = &world.topo;
                let unit = self.model.unit();
                if path.to_lowercase().ends_with(".png") {
                    // The picture that is edited, traced in full beside
                    // the interface and written once it is.
                    anyhow::ensure!(
                        workpiece_ui::view::View::new(&self.model).picture.is_some(),
                        "a PNG file is a rendered picture: add a Render step (Properties) to save one"
                    );
                    self.picture.save(path);
                } else if path.to_lowercase().ends_with(".stl") {
                    files::export(&path, workpiece_rasterize::tessellate::stl(topo))?;
                } else if workpiece_ops_step::is_iges(&path) {
                    let mut bodies = Vec::new();
                    for (id, _) in topo.bodies().iter() {
                        if let Ok(solid) = workpiece_boolean::read(topo, id) {
                            bodies.push((topo.name_of(id).unwrap_or("body").to_owned(), solid));
                        }
                    }
                    let name = std::path::Path::new(&path)
                        .file_stem()
                        .map_or_else(|| "part".to_owned(), |n| n.to_string_lossy().into_owned());
                    files::export(
                        &path,
                        workpiece_ops_step::export_iges(&name, &bodies, unit)?,
                    )?;
                } else if path.to_lowercase().ends_with(".tap") {
                    // (With a tool 6 mm across in passes 2 mm deep, in
                    // the part's own unit: what to start from.)
                    let mm = 1.0 / unit.millimetres();
                    let code =
                        workpiece_ui::cam::mill_path(&world, unit, 600.0 * mm, 6.0 * mm, 2.0 * mm)?;
                    files::export(&path, code)?;
                } else if path.to_lowercase().ends_with(".nc") {
                    // (At a feed to start from: a machine's own is set there.)
                    files::export(&path, workpiece_ui::cam::cut_path(&world, unit, 1000.0)?)?;
                } else if let Some(shown) = workpiece_ui::view::View::new(&self.model).sheet
                    && [".dxf", ".svg"]
                        .iter()
                        .any(|e| path.to_lowercase().ends_with(e))
                {
                    // The drawing sheet that shows, line for line.
                    let file = match path.to_lowercase().ends_with(".dxf") {
                        true => shown.laid.dxf(),
                        false => shown.laid.svg(),
                    };
                    files::export(&path, file)?;
                } else if path.to_lowercase().ends_with(".dxf") {
                    let mut drawing = workpiece_ops_drawing::quick::ThreeViews::of(
                        &world,
                        &stem_of(&path),
                        None,
                    )?;
                    drawing.dimensions = sketch_dimensions(&self.model);
                    files::export(&path, drawing.dxf(unit))?;
                } else if path.to_lowercase().ends_with(".pdf")
                    && let Some(shown) = workpiece_ui::view::View::new(&self.model).sheet
                {
                    // The drawing sheet that shows, true to size.
                    files::export(&path, shown.laid.pdf())?;
                } else if path.to_lowercase().ends_with(".pdf") {
                    let mut drawing = workpiece_ops_drawing::quick::ThreeViews::of(
                        &world,
                        &stem_of(&path),
                        None,
                    )?;
                    drawing.dimensions = sketch_dimensions(&self.model);
                    files::export(&path, drawing.pdf(unit))?;
                } else if path.to_lowercase().ends_with(".3mf") {
                    files::export(&path, workpiece_rasterize::tessellate::three_mf(topo, unit))?;
                } else {
                    let mut bodies = Vec::new();
                    for (id, _) in topo.bodies().iter() {
                        if let Ok(solid) = workpiece_boolean::read(topo, id) {
                            let name = topo.name_of(id).unwrap_or("body").to_owned();
                            bodies.push((name, solid));
                        }
                    }
                    anyhow::ensure!(!bodies.is_empty(), "there is no solid body to export");
                    let name = std::path::Path::new(&path)
                        .file_stem()
                        .map_or_else(|| "part".to_owned(), |n| n.to_string_lossy().into_owned());
                    files::export(&path, workpiece_ops_step::export(&name, &bodies, unit)?)?;
                }
            }
            FileAction::Choose(field, path) => {
                let parts = files::load(&path, &self.model.workspace)?;
                self.model.update(Message::Load { parts, open: None })?;
                let view = View::new(&self.model);
                let input = view.edit_dialog.as_ref().and_then(|d| d.fields.at(&field));
                if let Some(Widget::FileInput(input)) = input {
                    self.model.update(input.set(path))?;
                }
            }
        }
        Ok(())
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        // What a build beside the interface made shows as soon as it is
        // there; until then, look again shortly.
        #[cfg(target_arch = "wasm32")]
        self.web_frame(ui.ctx());
        let building = self.model.poll();
        let mut stop = false;
        if building {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
            egui::Area::new(egui::Id::new("building"))
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-14.0, -12.0))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new("Building…").color(theme::color::TEXT_WEAK));
                        stop = ui
                            .small_button("Stop")
                            .on_hover_text("Stop this build; the part stays as it was shown")
                            .clicked();
                    });
                });
        }
        let view = View::new(&self.model);
        let mut actions = Vec::new();
        let mut out = shortcuts(ui, &view, &mut actions, self.sheet.holds_keys());
        if stop {
            out.push(Message::StopBuild);
        }
        if self.fit && !building {
            self.fit = false;
            out.push(Message::Camera(workpiece_ui::CameraMove::Home));
            if std::mem::take(&mut self.perspective) {
                out.push(Message::Camera(workpiece_ui::CameraMove::ToggleProjection));
            }
            if let Some(back) = self.from.take() {
                out.push(Message::Camera(workpiece_ui::CameraMove::LookFrom(back)));
            }
        }
        let mut choose = None;

        egui::Panel::top("ribbon")
            .frame(
                egui::Frame::new()
                    .fill(theme::color::PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        let item = |label: &str, keys: &str| {
                            egui::Button::new(label.to_owned()).shortcut_text(keys.to_owned())
                        };
                        if ui.add(item("New", "Ctrl+N")).clicked() {
                            actions.push(FileAction::New);
                            ui.close();
                        }
                        if ui.add(item("Open…", "Ctrl+O")).clicked() {
                            self.file_dialog = Some(FileDialog::new(Purpose::Open, ""));
                            ui.close();
                        }
                        #[cfg(target_arch = "wasm32")]
                        web::file_menu(ui);
                        let open = view.document.is_some();
                        if ui.add_enabled(open, item("Save", "Ctrl+S")).clicked() {
                            actions.push(FileAction::Save);
                            ui.close();
                        }
                        if ui
                            .add_enabled(open, item("Save as…", "Ctrl+Shift+S"))
                            .clicked()
                        {
                            let name = view.document.as_ref().map_or("", |d| d.0.as_str());
                            self.file_dialog = Some(FileDialog::new(Purpose::SaveAs, name));
                            ui.close();
                        }
                        if ui.add_enabled(open, item("Export…", "")).clicked() {
                            let name = view.document.as_ref().map_or("part", |d| d.0.as_str());
                            let stem = name.rsplit_once('.').map_or(name, |n| n.0);
                            let dialog = FileDialog::new(Purpose::Export, &format!("{stem}.step"));
                            self.file_dialog = Some(dialog);
                            ui.close();
                        }
                        ui.separator();
                        if ui.add_enabled(open, item("Close", "Ctrl+W")).clicked() {
                            out.push(Message::Close);
                            ui.close();
                        }
                    });
                    ui.separator();
                    ui::ribbon::history(ui, &view.ribbon, &mut out);
                    ui.separator();
                    if let Some((path, dirty)) = &view.document {
                        ui.weak(if *dirty {
                            format!("{path} •")
                        } else {
                            path.clone()
                        });
                    }
                });
                // A step that is a workbench of its own (a sketch) shows
                // its tools in place of the ribbon's.
                match (&view.sheet, &view.edit_dialog) {
                    // (A drawing sheet is a workbench too.)
                    (Some(shown), dialog) => {
                        self.sheet
                            .tools(ui, shown, dialog.as_ref(), &mut out, &mut choose);
                        if let Some(kind) = self.sheet.export.take() {
                            let name = view.document.as_ref().map_or("part", |d| d.0.as_str());
                            let stem = name.rsplit_once('.').map_or(name, |n| n.0);
                            let name = format!("{stem}.{kind}");
                            self.file_dialog = Some(FileDialog::new(Purpose::Export, &name));
                        }
                    }
                    (None, Some(dialog)) if dialog.placement == Placement::Ribbon => {
                        ui::edit_dialog::show_ribbon(ui, dialog, &mut out, &mut choose);
                    }
                    _ => ui::ribbon::show(ui, &view.ribbon, &mut out),
                }
            });
        egui::Panel::left("timeline")
            .resizable(true)
            .default_size(300.0)
            .max_size(460.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::color::PANEL)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ui, |ui| {
                let height = ui.available_height();
                // The timeline above, and the part's contents below it.
                let section =
                    |ui: &mut egui::Ui, share: f32, show: &mut dyn FnMut(&mut egui::Ui)| {
                        let size = egui::vec2(ui.available_width(), height * share);
                        ui.allocate_ui(size, |ui| {
                            ui.set_min_height(size.y);
                            show(ui);
                        });
                    };
                section(ui, 0.56, &mut |ui| {
                    ui::timeline::show(ui, &view.timeline, &mut out)
                });
                ui.separator();
                ui::overview::show(ui, &view.overview, &mut out);
            });
        let area = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::color::CANVAS))
            .show(ui, |ui| {
                if view.document.is_none() {
                    #[cfg(target_arch = "wasm32")]
                    web::start_screen(ui);
                    #[cfg(not(target_arch = "wasm32"))]
                    ui.centered_and_justified(|ui| {
                        ui.weak("Make or open a part from the File menu")
                    });
                    return ui.max_rect();
                }
                match (&view.sheet, &view.picture) {
                    (Some(shown), _) => self.sheet.show(ui, shown, &mut out),
                    (None, Some(picture)) => {
                        let navigate = &mut self.viewport;
                        self.picture
                            .show(ui, picture, &view.viewport, navigate, &mut out);
                    }
                    (None, None) => self.viewport.show(ui, &view.viewport, &mut out),
                }
                ui.max_rect()
            })
            .inner;
        // (What of the view is on the window: a ribbon wider than the
        // window makes the view wider too, past its right edge — the
        // dialogs stand inside the window all the same.)
        let area = area.intersect(ui.ctx().content_rect());
        if let Some(shown) = &view.sheet {
            // (A drawing's dialog says what is selected on its sheet.)
            self.sheet.inspector(ui.ctx(), area, shown, &mut out);
        } else if let Some(dialog) = &view.edit_dialog
            && dialog.placement == Placement::Popup
        {
            ui::edit_dialog::show(ui.ctx(), area, dialog, &mut out, &mut choose);
        }
        // What is selected in a step whose tools are the ribbon (a
        // sketch, also one drawn on a sheet): its panel at the side.
        if let Some(dialog) = &view.edit_dialog
            && dialog.placement == Placement::Ribbon
        {
            ui::edit_dialog::inspector(ui.ctx(), area, dialog, &mut out, &mut choose);
        }
        // The panels over the view: the materials, the parameters.
        if let Some(materials) = &view.materials {
            self.materials.show(ui.ctx(), materials, &mut out);
        }
        if let Some(properties) = &view.properties {
            self.properties.show(ui.ctx(), properties, &mut out);
        }
        if let Some(templates) = &view.templates {
            self.templates.show(ui.ctx(), templates, &mut out);
        }
        if let Some(asked) = &view.new_drawing {
            new_drawing(ui.ctx(), asked, &mut out);
        }
        // A camera view that is edited is as the view is, once it
        // rests.
        let camera = view.viewport.camera;
        if !view.viewport.takes_view
            || view.viewport.animating
            || ui.input(|i| i.pointer.any_down())
        {
            self.rested = None;
        } else {
            let now = web_time::Instant::now();
            let since = match self.rested {
                Some((was, since)) if was == camera => since,
                _ => now,
            };
            self.rested = Some((camera, since));
            let wait = std::time::Duration::from_millis(350);
            if now.duration_since(since) >= wait {
                out.push(Message::TakeView);
                self.rested = None;
            } else {
                ui.ctx().request_repaint_after(wait);
            }
        }
        if view.table_shows
            && let Some(table) = &view.table
        {
            let mut open = true;
            egui::Window::new("Parameters")
                .open(&mut open)
                .collapsible(false)
                .default_pos(egui::pos2(340.0, 150.0))
                .default_size(egui::vec2(420.0, 300.0))
                .show(ui.ctx(), |ui| ui::table::show(ui, table, &mut out));
            if !open {
                out.push(Message::ShowPanel(None));
            }
        }
        // The picture that is edited: saving it asks for a file.
        if view.picture.is_none() {
            self.picture.rest();
        }
        if std::mem::take(&mut self.picture.ask_file) {
            let name = view.document.as_ref().map_or("part", |d| d.0.as_str());
            let stem = name.rsplit_once('.').map_or(name, |n| n.0);
            let id = view.picture.as_ref().map_or("render", |p| p.id.as_str());
            let name = format!("{stem}-{id}.png");
            self.file_dialog = Some(FileDialog::new(Purpose::Export, &name));
        }
        if let Some(error) = self.picture.error.take() {
            self.error = Some(error);
        }
        // While the camera is on its way to a new view, time moves it.
        if view.viewport.animating {
            out.push(Message::Tick(ui.input(|i| i.stable_dt).min(0.1)));
            ui.ctx().request_repaint();
        }
        if let Some(menu) = &view.context_menu {
            context_menu(ui.ctx(), area, menu, &mut out);
        }
        if let Some(field) = choose {
            self.file_dialog = Some(FileDialog::new(Purpose::Field(field), ""));
        }
        if let Some(dialog) = &mut self.file_dialog
            && let Some(result) = dialog.show(ui.ctx(), &mut self.previews)
        {
            let purpose = dialog.purpose.clone();
            self.file_dialog = None;
            match (result, purpose) {
                (None, _) => {}
                (Some(path), Purpose::Open) => actions.push(FileAction::Open(path)),
                (Some(path), Purpose::SaveAs) => actions.push(FileAction::SaveAs(path)),
                (Some(path), Purpose::Export) => actions.push(FileAction::Export(path)),
                (Some(path), Purpose::Field(field)) => {
                    actions.push(FileAction::Choose(field, path));
                }
                (Some(path), Purpose::NewPart) => out.push(Message::NewPartHere(Some(path))),
            }
        }
        if let Some(error) = self.error.clone() {
            egui::Window::new("Error")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
                .show(ui.ctx(), |ui| {
                    ui.colored_label(theme::color::ERROR, error);
                    if ui.button("OK").clicked() {
                        self.error = None;
                    }
                });
        }

        for message in out {
            // A new part needs a name first: the dialog sends it on.
            if matches!(message, Message::NewPartHere(None)) {
                let name = files::untitled(&self.model.workspace);
                self.file_dialog = Some(FileDialog::new(Purpose::NewPart, &name));
                continue;
            }
            self.apply(message);
        }
        for action in actions {
            if let Err(e) = self.file(action) {
                self.error = Some(format!("{e:#}"));
            }
        }
        // The library of materials is kept in its file as it changes.
        if self.model.workspace.library != self.library {
            self.library = self.model.workspace.library.clone();
            if let Err(e) = files::save_library(&self.library) {
                self.error = Some(format!("{e:#}"));
            }
        }
        // So are the sheet templates and the properties parts have.
        if self.model.workspace.sheets != self.kept.0 {
            self.kept.0 = self.model.workspace.sheets.clone();
            if let Err(e) = files::save_templates(&self.kept.0) {
                self.error = Some(format!("{e:#}"));
            }
        }
        if self.model.workspace.schema != self.kept.1 {
            self.kept.1 = self.model.workspace.schema.clone();
            if let Err(e) = files::save_schema(&self.kept.1) {
                self.error = Some(format!("{e:#}"));
            }
        }
        // And the user's settings.
        if self.model.settings != self.settings {
            self.settings = self.model.settings.clone();
            if let Err(e) = files::save_settings(&self.settings) {
                self.error = Some(format!("{e:#}"));
            }
        }
        let title = match View::new(&self.model).document {
            Some((path, true)) => format!("{path} • — Workpiece"),
            Some((path, false)) => format!("{path} — Workpiece"),
            None => "Workpiece".to_owned(),
        };
        if title != self.title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

/// The menu at the pointer, open while the right (or middle) button that
/// dragged it open is held: groups of items, apart, the one under the
/// pointer lit. Letting go over an item chooses it; anywhere else, the
/// menu just closes.
fn context_menu(
    ctx: &egui::Context,
    area: egui::Rect,
    menu: &workpiece_ui::view::ContextMenu,
    out: &mut Vec<Message>,
) {
    const ROW: f32 = 28.0;
    const WIDTH: f32 = 210.0;
    const GAP: f32 = 7.0;
    let [x, y] = menu.position;
    let pointer = ctx.input(|i| i.pointer.latest_pos());
    let held = ctx.input(|i| i.pointer.secondary_down() || i.pointer.middle_down());
    let rows: usize = menu.groups.iter().map(Vec::len).sum();
    let gaps = menu.groups.len().saturating_sub(1) as f32 * GAP;
    let height = (rows.max(1) as f32) * ROW + gaps + 8.0;
    // Beside where the drag started, so that nothing is under the
    // pointer until it is moved there.
    let mut min = area.min + egui::vec2(x + 14.0, y + 10.0);
    min.x = min.x.min(area.right() - WIDTH - 4.0);
    min.y = min.y.min(area.bottom() - height - 4.0);
    let frame = egui::Rect::from_min_size(min, egui::vec2(WIDTH, height));
    let mut chosen = None;
    egui::Area::new(egui::Id::new("context-menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(min)
        .interactable(false)
        .show(ctx, |ui| {
            ui.allocate_rect(frame, egui::Sense::hover());
            let painter = ui.painter();
            let color = theme::color::PANEL;
            painter.rect_filled(frame.expand(1.0), 9.0, theme::color::BORDER);
            painter.rect_filled(frame, 8.0, color);
            let font = egui::FontId::proportional(13.0);
            if menu.groups.is_empty() {
                painter.text(
                    frame.left_center() + egui::vec2(12.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "Nothing fits the selection",
                    font.clone(),
                    theme::color::TEXT_WEAK,
                );
            }
            let mut top = frame.top() + 4.0;
            for (g, group) in menu.groups.iter().enumerate() {
                if g > 0 {
                    let y = top + GAP / 2.0;
                    painter.line_segment(
                        [
                            egui::pos2(frame.left() + 8.0, y),
                            egui::pos2(frame.right() - 8.0, y),
                        ],
                        egui::Stroke::new(1.0, theme::color::BORDER),
                    );
                    top += GAP;
                }
                for item in group {
                    let row = egui::Rect::from_min_size(
                        egui::pos2(frame.left() + 4.0, top),
                        egui::vec2(WIDTH - 8.0, ROW),
                    );
                    top += ROW;
                    let lit = item.enabled && pointer.is_some_and(|p| row.contains(p));
                    let (ink, weak) = if lit {
                        painter.rect_filled(row, 6.0, theme::color::ACCENT);
                        (egui::Color32::WHITE, egui::Color32::WHITE)
                    } else if item.enabled {
                        (theme::color::TEXT, theme::color::TEXT_WEAK)
                    } else {
                        let faint = theme::color::TEXT_WEAK.gamma_multiply(0.5);
                        (faint, faint)
                    };
                    if lit {
                        chosen = Some(item.button.click());
                    }
                    let icon = egui::Rect::from_center_size(
                        row.left_center() + egui::vec2(16.0, 0.0),
                        egui::vec2(16.0, 16.0),
                    );
                    icons::widget(icons::image(item.icon), 16.0, ink).paint_at(ui, icon);
                    painter.text(
                        row.left_center() + egui::vec2(32.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        &item.button.label,
                        font.clone(),
                        ink,
                    );
                    if let Some(hotkey) = item.button.hotkey {
                        painter.text(
                            row.right_center() - egui::vec2(8.0, 0.0),
                            egui::Align2::RIGHT_CENTER,
                            hotkey.to_string(),
                            egui::FontId::proportional(11.5),
                            weak,
                        );
                    }
                }
            }
        });
    if held {
        ctx.request_repaint();
    } else {
        // Let go: over an item, that is the choice.
        out.extend(chosen.into_iter().flat_map(|m| [m, Message::EndEdit]));
        out.push(Message::ContextMenu(None));
    }
}

/// The messages and file actions of key presses.
/// With `sheet_keys`, a tool of the drawing workbench is in hand: Escape
/// and Enter are its own (they end a cutting line, put the tool down).
fn shortcuts(
    ui: &mut egui::Ui,
    view: &View,
    actions: &mut Vec<FileAction>,
    sheet_keys: bool,
) -> Vec<Message> {
    use egui::{Key, Modifiers};
    let mut browse = false;
    let mut save_as = false;
    // While a field has the keyboard, plain keys are typed into it.
    let typing = ui.ctx().memory(|m| m.focused().is_some());
    let mut hotkeys = view.hotkeys();
    // Shift+R before R: a press with more modifiers also matches fewer.
    hotkeys.sort_by_key(|(key, _)| std::cmp::Reverse(u8::from(key.ctrl) + u8::from(key.shift)));
    let out = ui.input_mut(|i| {
        let mut out = Vec::new();
        if !typing && i.consume_key(Modifiers::NONE, Key::Tab) {
            out.push(Message::Pointer(workpiece_ui::Pointer::Cycle));
        }
        // Escape backs out of the innermost thing first.
        let picking = view
            .edit_dialog
            .as_ref()
            .is_some_and(|d| d.picking.is_some());
        // (On a sheet, Escape is the workbench's: it ends a cutting
        // line, puts the tool down.)
        let selected = view.edit_dialog.is_none()
            && view.sheet.is_none()
            && !view.viewport.selected.is_empty();
        if view.context_menu.is_some() && i.consume_key(Modifiers::NONE, Key::Escape) {
            out.push(Message::ContextMenu(None));
        } else if let Some(entry) = &view.viewport.entry
            && i.consume_key(Modifiers::NONE, Key::Escape)
        {
            // A value being typed in the scene is dropped.
            out.push(entry.cancel.clone());
        } else if picking && !sheet_keys && i.consume_key(Modifiers::NONE, Key::Escape) {
            out.push(Message::Pick(None));
        } else if selected && !typing && i.consume_key(Modifiers::NONE, Key::Escape) {
            out.push(Message::ClearSelection);
        }
        save_as = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
        if i.consume_key(Modifiers::COMMAND, Key::Y) {
            out.push(Message::Redo);
        }
        if i.consume_key(Modifiers::COMMAND, Key::N) {
            actions.push(FileAction::New);
        }
        if i.consume_key(Modifiers::COMMAND, Key::S) {
            actions.push(FileAction::Save);
        }
        if i.consume_key(Modifiers::COMMAND, Key::W) {
            out.push(Message::Close);
        }
        browse = i.consume_key(Modifiers::COMMAND, Key::O);
        if let Some(table) = &view.table {
            if ui::pressed(i, ui::table::ADD_VARIANT) {
                out.push(ui::table::with_variant(table));
            } else if ui::pressed(i, ui::table::ADD_PARAMETER) {
                out.push(ui::table::with_parameter(table));
            }
        }
        for (hotkey, press) in &hotkeys {
            let tools_key = matches!(
                hotkey.key,
                workpiece_core::Key::Escape | workpiece_core::Key::Enter
            );
            if (typing && !hotkey.ctrl) || (sheet_keys && tools_key) || !ui::pressed(i, *hotkey) {
                continue;
            }
            // Each press is its own undo step.
            out.extend(view.press(press));
            out.push(Message::EndEdit);
        }
        out
    });
    if browse {
        actions.push(FileAction::Browse);
    }
    if save_as && view.document.is_some() {
        actions.push(FileAction::SaveAsDialog);
    }
    out
}

/// The dimensions of the sketches of the part being worked on, as a
/// drawing writes them: of each sketch its name and its dimensions.
fn sketch_dimensions(model: &workpiece_ui::Model) -> Vec<(String, Vec<String>)> {
    let Some(part) = model
        .focused_part()
        .and_then(|p| model.workspace.part(&p).ok())
    else {
        return Vec::new();
    };
    let sketches = part
        .program
        .iter()
        .filter_map(|op| op.downcast_ref::<workpiece_ops_sketch::Sketch>());
    sketches.map(|s| (s.id.clone(), s.dimensions())).collect()
}

/// The name of the file at `path`, without what it ends in.
fn stem_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map_or_else(|| "part".to_owned(), |n| n.to_string_lossy().into_owned())
}

/// Asks what a new drawing starts from: an empty sheet, or the views
/// of a sheet template.
fn new_drawing(
    ctx: &egui::Context,
    asked: &workpiece_ui::view::NewDrawing,
    out: &mut Vec<Message>,
) {
    let modal = egui::Modal::new(egui::Id::new("new-drawing")).show(ctx, |ui| {
        ui.set_width(300.0);
        ui.heading("New drawing");
        ui.weak("What should the sheet start with?");
        ui.add_space(6.0);
        let wide = |ui: &mut egui::Ui, label: &str, hint: &str| {
            ui.add_sized([300.0, 26.0], egui::Button::new(label))
                .on_hover_text(hint)
                .clicked()
        };
        if wide(
            ui,
            "Empty sheet",
            "Only the frame and the title block: add the views yourself",
        ) {
            out.push(Message::NewDrawing(None));
        }
        ui.add_space(4.0);
        ui.weak("With the views of a template");
        for (k, name) in asked.templates.iter().enumerate() {
            let hint = match k {
                0 => "The usual start",
                _ => "The sheet, the views and the title block of this template",
            };
            if wide(ui, name, hint) {
                out.push(Message::NewDrawing(Some(name.clone())));
            }
        }
        ui.add_space(6.0);
        if ui.button("Cancel").clicked() {
            out.push(Message::ShowPanel(None));
        }
    });
    if modal.should_close() {
        out.push(Message::ShowPanel(None));
    }
}
