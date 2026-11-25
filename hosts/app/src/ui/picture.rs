//! The picture workbench: the step that is a rendered picture shows in
//! place of the model, traced beside the interface and shown as it
//! adds up. The view turns as ever; the picture is taken from where it
//! is.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;
use web_time::Instant;

use eframe::egui;
use shapecraft_cad_ui::Message;
use shapecraft_cad_ui::model::Camera;
use shapecraft_cad_ui::view::{Picture, Viewport};
use shapecraft_render::{Film, Lighting, Scene};

use crate::theme::color;

/// What is traced: a scene, from where, in what light, how large and
/// how often.
#[derive(Clone)]
struct Job {
    /// What tells it from another: the scene's stamp and all the rest.
    key: (u64, String),
    scene: Arc<Scene>,
    camera: shapecraft_render::Camera,
    lighting: Lighting,
    size: [usize; 2],
    samples: u32,
    /// The file the picture is written to once it is done; none for
    /// what only shows.
    file: Option<String>,
}

/// The picture as far as it is traced.
struct Frame {
    key: (u64, String),
    size: [usize; 2],
    rgba: Vec<u8>,
    passes: u32,
    samples: u32,
    /// Whether it is the picture for a file, and (once it is written,
    /// or could not be) how that went.
    file: Option<String>,
    written: Option<Result<(), String>>,
}

#[derive(Default)]
struct Shared {
    job: Option<Job>,
    frame: Option<Frame>,
    /// Counts the frames: another number is another frame.
    serial: u64,
    quit: bool,
}

type Lock = Arc<(Mutex<Shared>, Condvar)>;

/// Writes `rgba` (`size` pixels, row by row) as a PNG file at `path`.
fn write_png(path: &str, size: [usize; 2], rgba: &[u8]) -> anyhow::Result<()> {
    let mut file = Vec::new();
    let mut encoder = png::Encoder::new(&mut file, size[0] as u32, size[1] as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    crate::files::export(path, file)
}

/// What is to be done next, with `doing` in hand: `Some(Some(job))`
/// start on that job, `Some(None)` a pass more of this one, `None`
/// nothing until something else is asked for.
fn next(state: &Shared, doing: &Option<(Job, Film)>) -> Option<Option<Job>> {
    let done = doing
        .as_ref()
        .is_some_and(|(job, film)| film.passes() >= job.samples);
    let other = match (&state.job, doing) {
        (Some(asked), Some((job, _))) => asked.key != job.key,
        (Some(_), None) => true,
        (None, _) => false,
    };
    if other {
        return Some(state.job.clone());
    }
    (state.job.is_some() && !done).then_some(None)
}

/// Does what [`next`] said: starts on `job` (first a coarse picture, at
/// once: what shows while the view is turned), or adds a pass to what is
/// in hand; and shows how far it is.
fn work(lock: &Lock, ctx: &egui::Context, doing: &mut Option<(Job, Film)>, job: Option<Job>) {
    let publish = |job: &Job, film: &Film, passes: u32, written| {
        let mut state = lock.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.serial += 1;
        state.frame = Some(Frame {
            key: job.key.clone(),
            size: [film.width(), film.height()],
            rgba: film.rgba(),
            passes,
            samples: job.samples,
            file: job.file.clone(),
            written,
        });
        drop(state);
        ctx.request_repaint();
    };
    if let Some(job) = job {
        if job.file.is_none() {
            let mut coarse = Film::new((job.size[0] / 3).max(8), (job.size[1] / 3).max(8));
            shapecraft_render::pass(&job.scene, &job.camera, &job.lighting, &mut coarse);
            publish(&job, &coarse, 0, None);
        }
        let film = Film::new(job.size[0], job.size[1]);
        *doing = Some((job, film));
        return;
    }
    if let Some((job, film)) = doing {
        shapecraft_render::pass(&job.scene, &job.camera, &job.lighting, film);
        let done = film.passes() >= job.samples;
        let written = match &job.file {
            Some(path) if done => Some(
                write_png(path, [film.width(), film.height()], &film.rgba())
                    .map_err(|e| format!("writing {path}: {e:#}")),
            ),
            _ => None,
        };
        publish(job, film, film.passes(), written);
    }
}

/// Traces what is asked for, pass by pass, until something else is: on
/// a thread of its own.
fn trace(lock: &Lock, ctx: &egui::Context) {
    let (shared, wake) = &**lock;
    let mut doing: Option<(Job, Film)> = None;
    loop {
        let job = {
            let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if state.quit {
                    return;
                }
                if let Some(job) = next(&state, &doing) {
                    break job;
                }
                state = wake.wait(state).unwrap_or_else(PoisonError::into_inner);
            }
        };
        work(lock, ctx, &mut doing, job);
    }
}

/// The picture workbench's state.
#[derive(Default)]
pub struct PicturePanel {
    lock: Option<Lock>,
    /// Where there are no threads to trace on: what is traced, a pass
    /// a frame.
    doing: Option<(Job, Film)>,
    texture: Option<egui::TextureHandle>,
    /// The frame the texture holds, by its serial.
    shown: u64,
    /// The camera as it last was, and since when: a view that rests is
    /// where the picture is taken from.
    rested: Option<(Camera, Instant)>,
    /// The file a picture is being traced for.
    saving: Option<String>,
    /// Set when the user asks to save the picture: the application asks
    /// for a file.
    pub ask_file: bool,
    /// What went wrong writing a picture, to be told.
    pub error: Option<String>,
}

impl Drop for PicturePanel {
    fn drop(&mut self) {
        if let Some(lock) = &self.lock {
            lock.0.lock().unwrap_or_else(PoisonError::into_inner).quit = true;
            lock.1.notify_all();
        }
    }
}

/// The largest rectangle `wide` to `high` that fits in `area`, in its
/// middle.
fn fit(area: egui::Rect, wide: f32, high: f32) -> egui::Rect {
    let scale = (area.width() / wide).min(area.height() / high);
    egui::Rect::from_center_size(area.center(), egui::vec2(wide, high) * scale)
}

impl PicturePanel {
    fn lock(&mut self, ctx: &egui::Context) -> Lock {
        self.lock
            .get_or_insert_with(|| {
                let lock: Lock = Arc::default();
                if shapecraft_core::HAS_THREADS {
                    let (theirs, ctx) = (lock.clone(), ctx.clone());
                    std::thread::spawn(move || trace(&theirs, &ctx));
                }
                lock
            })
            .clone()
    }

    /// Nothing is traced any more: no picture is edited.
    pub fn rest(&mut self) {
        if let Some(lock) = &self.lock {
            let mut state = lock.0.lock().unwrap_or_else(PoisonError::into_inner);
            state.job = None;
            state.frame = None;
        }
        self.texture = None;
        self.rested = None;
        self.saving = None;
    }

    /// The picture is traced in full and written to the file at
    /// `path`.
    pub fn save(&mut self, path: String) {
        self.saving = Some(path);
    }

    fn job(&self, picture: &Picture, camera: &Camera, size: [usize; 2]) -> Job {
        let shot = &picture.shot;
        let camera = shapecraft_cad_ui::view::camera(camera, &picture.scene);
        let lighting = shapecraft_cad_ui::view::lighting(shot);
        let file = self.saving.clone();
        let samples = shot.samples.clamp(1, 1 << 16) as u32;
        let said = format!("{camera:?}{lighting:?}{size:?}{samples}{file:?}");
        Job {
            key: (picture.stamp, said),
            scene: picture.scene.clone(),
            camera,
            lighting,
            size,
            samples,
            file,
        }
    }

    /// Draws the picture filling `ui`, as far as it is traced, and has
    /// it traced on; the pointer turns the view as it does the
    /// model's.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        picture: &Picture,
        view: &Viewport,
        navigate: &mut super::viewport::ViewportPanel,
        out: &mut Vec<Message>,
    ) {
        let area = ui.available_rect_before_wrap();
        navigate.navigate(ui, view, out);
        let shot = &picture.shot;
        let (wide, high) = (shot.width.max(1) as f32, shot.height.max(1) as f32);
        let frame = fit(area.shrink(10.0), wide, high);
        ui.painter().rect_filled(area, 0.0, color::PANEL);
        // What shows is traced as large as it shows, the picture for a
        // file as large as it is.
        let points = ui.ctx().pixels_per_point();
        let size = match self.saving {
            Some(_) => [shot.width, shot.height],
            None => [
                ((frame.width() * points) as usize).clamp(16, shot.width.max(16)),
                ((frame.height() * points) as usize).clamp(16, shot.height.max(16)),
            ],
        };
        let camera = view.camera;
        let job = self.job(picture, &camera, size);
        let key = job.key.clone();
        let lock = self.lock(ui.ctx());
        let mut status = String::new();
        {
            let mut state = lock.0.lock().unwrap_or_else(PoisonError::into_inner);
            if state.job.as_ref().is_none_or(|j| j.key != key) {
                state.job = Some(job);
                lock.1.notify_all();
            }
            // Without threads, a piece of the tracing is done each
            // frame.
            if !shapecraft_core::HAS_THREADS
                && let Some(job) = next(&state, &self.doing)
            {
                drop(state);
                work(&lock, ui.ctx(), &mut self.doing, job);
                state = lock.0.lock().unwrap_or_else(PoisonError::into_inner);
            }
            if let Some(frame) = &mut state.frame {
                if let Some(written) = frame.written.take() {
                    self.saving = None;
                    if let Err(why) = written {
                        self.error = Some(why);
                    }
                }
                status = match (&frame.file, frame.passes) {
                    // (What shows is still of how it was before.)
                    _ if frame.key != key => "…".to_owned(),
                    (Some(path), n) => format!("Rendering {path}: {n} of {}", frame.samples),
                    (None, 0) => "…".to_owned(),
                    (None, n) => format!("{n} of {} samples", frame.samples),
                };
            }
            let serial = state.serial;
            if let Some(frame) = state.frame.as_ref().filter(|_| serial != self.shown) {
                let image = egui::ColorImage::from_rgba_unmultiplied(frame.size, &frame.rgba);
                let options = egui::TextureOptions::LINEAR;
                match &mut self.texture {
                    Some(texture) => texture.set(image, options),
                    None => self.texture = Some(ui.ctx().load_texture("picture", image, options)),
                }
                self.shown = serial;
            }
        }
        match &self.texture {
            Some(texture) => {
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter()
                    .image(texture.id(), frame, uv, egui::Color32::WHITE);
            }
            None => {
                let [r, g, b] = shot.backdrop();
                ui.painter()
                    .rect_filled(frame, 0.0, egui::Color32::from_rgb(r, g, b));
            }
        }
        // The picture is taken from where the view rests.
        if picture.taken || view.animating || ui.input(|i| i.pointer.any_down()) {
            self.rested = None;
        } else {
            let now = Instant::now();
            let since = match self.rested {
                Some((was, since)) if was == camera => since,
                _ => now,
            };
            self.rested = Some((camera, since));
            let wait = Duration::from_millis(350);
            if now.duration_since(since) >= wait {
                out.push(Message::TakeView);
                self.rested = None;
            } else {
                ui.ctx().request_repaint_after(wait);
            }
        }
        // What it is, how far it is, and saving it.
        let bar = egui::Rect::from_min_size(
            area.min + egui::vec2(12.0, 10.0),
            egui::vec2(area.width() - 24.0, 26.0),
        );
        let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar));
        bar_ui.horizontal(|ui| {
            egui::Frame::new()
                .fill(color::PANEL)
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(format!(
                        "{} · {} × {}",
                        picture.id, shot.width, shot.height
                    ));
                    ui.weak(&status);
                    if self.saving.is_some() {
                        ui.spinner();
                        if ui.button("Stop").clicked() {
                            self.saving = None;
                        }
                    } else if ui
                        .button("Save image…")
                        .on_hover_text(
                            "Trace the picture in full, as large as it is, and write it as a PNG file",
                        )
                        .clicked()
                    {
                        self.ask_file = true;
                    }
                });
        });
    }
}
