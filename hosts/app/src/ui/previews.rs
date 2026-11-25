//! The previews of the part files in the working directory, as lists of
//! parts show them: read from the files that have one, taken beside the
//! interface of those that have none.

use std::collections::HashMap;
use std::sync::mpsc;

use eframe::egui;
use workpiece_part::{Cancel, Engine, Part, Workspace};
use workpiece_ui::preview;

use crate::files;

/// A picture as it comes from a file or from a build: its width, its
/// height, and red, green, blue and alpha of each pixel.
type Pixels = (usize, usize, Vec<u8>);

/// A file's path with the picture there is of it, if there is one.
type Taken = (String, Option<Pixels>);

/// What is known of a file's preview.
enum Known {
    /// It is being read or taken.
    Coming,
    /// The file has nothing to show: it does not build, or to no body.
    Nothing,
    /// The picture.
    Picture(egui::TextureHandle),
}

/// What a list shows for a file.
pub enum Shown<'a> {
    /// The picture is on its way.
    Coming,
    /// There is none.
    Nothing,
    /// The picture.
    Picture(&'a egui::TextureHandle),
}

/// The previews of part files, by path: each as the file was when it
/// was last looked at.
pub struct Previews {
    known: HashMap<String, (Option<u64>, Known)>,
    /// To the thread that reads and takes them (none where there are no
    /// threads: there, one is done per frame).
    jobs: Option<mpsc::Sender<String>>,
    waiting: Vec<String>,
    results: (mpsc::Sender<Taken>, mpsc::Receiver<Taken>),
}

/// What tells this version of the file at `path` from another.
fn written(path: &str) -> Option<u64> {
    workpiece_core::files::stamp(path)
}

/// The preview of the part file at `path`: the one it holds, if that is
/// of the part as it is, or one taken of it as it builds now (with the
/// parts it places, and the materials of the working directory).
fn of_file(path: &str) -> Option<Pixels> {
    if workpiece_part::is_step(path) {
        return None;
    }
    let part = Part::from_text(&workpiece_core::files::read_to_string(path).ok()?).ok()?;
    if let Some(kept) = part.fresh_preview() {
        return preview::pixels(kept);
    }
    let parts = files::load(path, &Workspace::default()).ok()?;
    let mut workspace = Workspace::new(parts);
    workspace.library = files::library().unwrap_or_default();
    let workspace = std::sync::Arc::new(workspace);
    // (A part that breaks while it builds has no preview, like one that
    // fails.)
    let cancel = Cancel::new();
    let taken = cancel.run(|| preview::take(&workspace, &Engine::inline(), path, &cancel));
    preview::pixels(&taken.ok()?.ok()??)
}

impl Default for Previews {
    fn default() -> Self {
        let results = mpsc::channel();
        let jobs = workpiece_core::HAS_THREADS.then(|| {
            let (jobs, todo) = mpsc::channel::<String>();
            let done = results.0.clone();
            std::thread::spawn(move || {
                for path in todo {
                    let pixels = of_file(&path);
                    if done.send((path, pixels)).is_err() {
                        break;
                    }
                }
            });
            jobs
        });
        Self {
            known: HashMap::new(),
            jobs,
            waiting: Vec::new(),
            results,
        }
    }
}

impl Previews {
    /// Forgets the previews of the files among `paths` that were
    /// written since they were looked at: they are looked at again.
    pub fn look_again(&mut self, paths: &[String]) {
        for path in paths {
            if let Some((then, known)) = self.known.get(path)
                && !matches!(known, Known::Coming)
                && *then != written(path)
            {
                self.known.remove(path);
            }
        }
    }

    /// What there is to show for the file at `path`; asks for its
    /// preview, if that was not asked for yet.
    pub fn of(&mut self, ctx: &egui::Context, path: &str) -> Shown<'_> {
        // What came in since.
        while let Ok((done, pixels)) = self.results.1.try_recv() {
            let known = match pixels {
                Some((width, height, rgba)) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                    let name = format!("preview {done}");
                    Known::Picture(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
                }
                None => Known::Nothing,
            };
            if let Some(entry) = self.known.get_mut(&done) {
                entry.1 = known;
            }
        }
        if !self.known.contains_key(path) {
            self.known
                .insert(path.to_owned(), (written(path), Known::Coming));
            match &self.jobs {
                Some(jobs) => {
                    let _ = jobs.send(path.to_owned());
                }
                None => self.waiting.push(path.to_owned()),
            }
        }
        match &self.known[path].1 {
            Known::Coming => {
                // (It shows as soon as it is there.)
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                Shown::Coming
            }
            Known::Nothing => Shown::Nothing,
            Known::Picture(texture) => Shown::Picture(texture),
        }
    }

    /// Where nothing works beside the interface: reads or takes one of
    /// the previews asked for. Once a frame.
    pub fn work(&mut self) {
        if self.jobs.is_none()
            && let Some(path) = self.waiting.pop()
        {
            let pixels = of_file(&path);
            let _ = self.results.0.send((path, pixels));
        }
    }
}
