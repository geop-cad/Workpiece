//! Part files in the working directory: listing, reading (with the parts
//! they place) and writing. Paths are relative to the working directory;
//! only opened files and what they place are read.
//!
//! Files go through [`workpiece_core::files`]: on disk, or in a browser
//! in memory (what the user opened, what was fetched). There, what is
//! saved or exported is also handed to the user as a download.

use std::collections::BTreeMap;
use std::path::Path;

use workpiece_core::files;
use workpiece_ops_instances::PART_EXTENSION;
use workpiece_part::{Part, Workspace};

/// How deep the file list looks into subdirectories.
const DEPTH: usize = 4;

/// The part files under the working directory, relative to it, sorted;
/// with `placing`, the STEP files too, which can be placed as parts.
pub fn list(placing: bool) -> Vec<String> {
    let mut out = Vec::new();
    if files::IN_MEMORY {
        let fits = |name: &String| {
            name.ends_with(&format!(".{PART_EXTENSION}"))
                || (placing && workpiece_part::is_step(name))
        };
        out = files::in_memory().into_iter().filter(fits).collect();
    } else {
        walk(Path::new("."), 0, placing, &mut out);
    }
    out.sort();
    out
}

fn walk(dir: &Path, depth: usize, placing: bool, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            if depth < DEPTH {
                walk(&path, depth + 1, placing, out);
            }
        } else if path.extension().is_some_and(|e| e == PART_EXTENSION)
            || (placing && workpiece_part::is_step(&name))
        {
            let relative = path.strip_prefix(".").unwrap_or(&path);
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// The materials the working directory has in its library file, or
/// those a library starts with, if it has none.
pub fn library() -> anyhow::Result<workpiece_part::Library> {
    let path = workpiece_part::LIBRARY_FILE;
    if !files::exists(path) {
        return Ok(workpiece_part::Library::default());
    }
    let text = files::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {path}: {e}"))?;
    workpiece_part::Library::from_text(&text).map_err(|e| e.context(format!("reading {path}")))
}

/// Writes `library` to the working directory's library file.
pub fn save_library(library: &workpiece_part::Library) -> anyhow::Result<()> {
    let path = workpiece_part::LIBRARY_FILE;
    files::write(path, library.to_text()?).map_err(|e| anyhow::anyhow!("writing {path}: {e}"))
}

/// The sheet templates the working directory has in their file, or
/// those a workspace starts with, if it has none.
pub fn templates() -> anyhow::Result<workpiece_ops_drawing::Templates> {
    let path = workpiece_ops_drawing::template::TEMPLATES_FILE;
    if !files::exists(path) {
        return Ok(workpiece_ops_drawing::Templates::default());
    }
    let text = files::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {path}: {e}"))?;
    workpiece_ops_drawing::Templates::from_text(&text)
        .map_err(|e| e.context(format!("reading {path}")))
}

/// Writes `templates` to the working directory's file of them.
pub fn save_templates(templates: &workpiece_ops_drawing::Templates) -> anyhow::Result<()> {
    let path = workpiece_ops_drawing::template::TEMPLATES_FILE;
    files::write(path, templates.to_text()?).map_err(|e| anyhow::anyhow!("writing {path}: {e}"))
}

/// The properties parts have, as the working directory's file says, or
/// those a workspace starts with, if it has none.
pub fn schema() -> anyhow::Result<workpiece_part::Schema> {
    let path = workpiece_part::SCHEMA_FILE;
    if !files::exists(path) {
        return Ok(workpiece_part::Schema::default());
    }
    let text = files::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {path}: {e}"))?;
    workpiece_part::Schema::from_text(&text).map_err(|e| e.context(format!("reading {path}")))
}

/// Writes `schema` to the working directory's file of it.
pub fn save_schema(schema: &workpiece_part::Schema) -> anyhow::Result<()> {
    let path = workpiece_part::SCHEMA_FILE;
    files::write(path, schema.to_text()?).map_err(|e| anyhow::anyhow!("writing {path}: {e}"))
}

/// The file of the user's settings: in the user's configuration directory
/// (`$WORKPIECE_CONFIG_DIR` if that is set, which tests and trials use).
#[cfg(not(target_arch = "wasm32"))]
fn settings_file() -> Option<std::path::PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let dir = if let Some(dir) = var("WORKPIECE_CONFIG_DIR") {
        std::path::PathBuf::from(dir)
    } else if cfg!(target_os = "windows") {
        std::path::PathBuf::from(var("APPDATA")?).join("workpiece")
    } else if cfg!(target_os = "macos") {
        let home = std::path::PathBuf::from(var("HOME")?);
        home.join("Library/Application Support/workpiece")
    } else if let Some(xdg) = var("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(xdg).join("workpiece")
    } else {
        std::path::PathBuf::from(var("HOME")?).join(".config/workpiece")
    };
    Some(dir.join("settings.json"))
}

/// What the user's settings file holds, if there is one.
#[cfg(not(target_arch = "wasm32"))]
fn settings_text() -> Option<String> {
    std::fs::read_to_string(settings_file()?).ok()
}

/// Writes `text` to the user's settings file.
#[cfg(not(target_arch = "wasm32"))]
fn write_settings_text(text: &str) -> anyhow::Result<()> {
    let Some(path) = settings_file() else {
        return Ok(());
    };
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, text)
    };
    write().map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))
}

/// In a browser, the settings are kept in the page's local storage.
#[cfg(target_arch = "wasm32")]
fn settings_text() -> Option<String> {
    crate::web::stored(SETTINGS_KEY)
}

#[cfg(target_arch = "wasm32")]
fn write_settings_text(text: &str) -> anyhow::Result<()> {
    crate::web::store(SETTINGS_KEY, text);
    Ok(())
}

/// Where in a browser's local storage the settings are.
#[cfg(target_arch = "wasm32")]
const SETTINGS_KEY: &str = "workpiece.settings";

/// The settings the user's file has; those not in it are as they are by
/// default. The recent files are those of the working directory.
pub fn settings() -> workpiece_ui::model::Settings {
    settings_from(settings_text().as_deref(), &workspace_key())
}

/// The settings `text` (a settings file) has, the recent files those of
/// the workspace `key`.
fn settings_from(text: Option<&str>, key: &str) -> workpiece_ui::model::Settings {
    let mut settings = workpiece_ui::model::Settings::default();
    let json: Option<serde_json::Value> = text.and_then(|t| serde_json::from_str(t).ok());
    if let Some(on) = json
        .as_ref()
        .and_then(|j| j.get("sketch_section"))
        .and_then(serde_json::Value::as_bool)
    {
        settings.sketch_section = on;
    }
    if let Some(snaps) = json.as_ref().and_then(|j| j.get("snaps")) {
        use workpiece_core::handles::SnapKind;
        let on = |v: Option<&serde_json::Value>, or: bool| {
            v.and_then(serde_json::Value::as_bool).unwrap_or(or)
        };
        let mut kinds = settings.snaps;
        kinds.on = on(snaps.get("on"), kinds.on);
        for kind in SnapKind::ALL {
            kinds = kinds.with(kind, on(snaps.get(kind.label()), kinds.has(kind)));
        }
        settings.snaps = kinds;
    }
    if let Some(recent) = json
        .as_ref()
        .and_then(|j| j.get("recent"))
        .and_then(|r| r.get(key))
        .and_then(serde_json::Value::as_array)
    {
        settings.recent = recent
            .iter()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .take(workpiece_ui::model::RECENT)
            .collect();
    }
    settings
}

/// Writes `settings` to the user's file of them (the recent files under
/// the working directory, beside those of other workspaces).
pub fn save_settings(settings: &workpiece_ui::model::Settings) -> anyhow::Result<()> {
    let text = settings_into(settings_text().as_deref(), &workspace_key(), settings)?;
    write_settings_text(&text)
}

/// The settings file `text` with `settings` in it, for the workspace
/// `key`.
fn settings_into(
    text: Option<&str>,
    key: &str,
    settings: &workpiece_ui::model::Settings,
) -> anyhow::Result<String> {
    let mut json: serde_json::Value = text
        .and_then(|t| serde_json::from_str(t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    json["sketch_section"] = settings.sketch_section.into();
    let mut snaps = serde_json::json!({ "on": settings.snaps.on });
    for kind in workpiece_core::handles::SnapKind::ALL {
        snaps[kind.label()] = settings.snaps.has(kind).into();
    }
    json["snaps"] = snaps;
    if !json.get("recent").is_some_and(serde_json::Value::is_object) {
        json["recent"] = serde_json::json!({});
    }
    json["recent"][key] = settings.recent.clone().into();
    Ok(serde_json::to_string_pretty(&json)?)
}

/// What the workspace is known by among the recent files: the working
/// directory, in full (in a browser, which has none, the empty name).
fn workspace_key() -> String {
    std::env::current_dir()
        .ok()
        .map(|d| d.canonicalize().unwrap_or(d))
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A name for a new part in `folder` (a path; empty for the working
/// directory): `untitled.wpc`, or with the smallest number from 2 that is
/// neither in `workspace` nor on disk.
pub fn untitled(workspace: &Workspace, folder: &str) -> String {
    let free = |name: &String| !workspace.parts.contains_key(name) && !files::exists(name);
    std::iter::once(format!("untitled.{PART_EXTENSION}"))
        .chain((2..).map(|n| format!("untitled{n}.{PART_EXTENSION}")))
        .map(|name| match folder {
            "" => name,
            folder => format!("{folder}/{name}"),
        })
        .find(free)
        .unwrap_or_default()
}

/// `name` with the part extension, if it has none.
pub fn with_extension(name: &str) -> String {
    let name = name.trim();
    if Path::new(name)
        .extension()
        .is_some_and(|e| e == PART_EXTENSION)
        || workpiece_part::is_step(name)
    {
        name.to_owned()
    } else {
        format!("{name}.{PART_EXTENSION}")
    }
}

/// What tells apart the versions of the part files read or written here,
/// by path, as they were then: a file that is not so any more was
/// changed by someone else, and saving does not write over it.
fn stamps() -> std::sync::MutexGuard<'static, BTreeMap<String, Option<u64>>> {
    static STAMPS: std::sync::Mutex<BTreeMap<String, Option<u64>>> =
        std::sync::Mutex::new(BTreeMap::new());
    STAMPS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reads the part at `path` and, recursively, the parts it places (each
/// beside the file that places it), except those `workspace` has
/// already.
pub fn load(path: &str, workspace: &Workspace) -> anyhow::Result<BTreeMap<String, Part>> {
    let parts = workpiece_part::file_set::read(path, &|p| workspace.parts.contains_key(p))?;
    let mut stamps = stamps();
    for path in parts.keys() {
        stamps.insert(path.clone(), files::stamp(path));
    }
    Ok(parts)
}

/// Writes `part` to the file at `path`, what it places named relative to
/// it (in a browser, also handed to the user as a download).
pub fn save(path: &str, part: &Part) -> anyhow::Result<()> {
    export(path, part.to_file(path)?)?;
    stamps().insert(path.to_owned(), files::stamp(path));
    Ok(())
}

/// Saves the part at `path` with everything it needs to be opened again:
/// of the parts it places (in turn), each that differs from its file or
/// has none, then it — a part after those it places, so that a file
/// never names one that is not written yet. A part with no file yet goes
/// where the workspace has it (beside the part that places it, unless
/// it was named otherwise). Nothing is written if a part would go over a
/// file that is not its own (one it was not read from), or over its file
/// changed by someone else since it was read — but for the part at
/// `path` itself with `replace` (the user chose to write over that file).
/// The paths written; a
/// failure on the way says what was written and what not (those
/// written are written whole).
pub fn save_all(
    model: &workpiece_ui::Model,
    path: &str,
    replace: bool,
) -> (Vec<String>, anyhow::Result<()>) {
    let mut todo = model.unsaved(path);
    if !todo.iter().any(|p| p == path) {
        todo.push(path.to_owned());
    }
    // First what would go wrong before anything is written.
    for p in &todo {
        if !files::exists(p) || (replace && p == path) {
            continue;
        }
        let clash = if model.saved.contains_key(p) {
            stamps()
                .get(p)
                .is_some_and(|was| *was != files::stamp(p))
                .then(|| format!("{p} was changed on disk since it was read"))
        } else {
            Some(format!(
                "there is a file {p} already, which is not the part of that name here \
                 (that was never read from it)"
            ))
        };
        if let Some(clash) = clash {
            let by = placing(model, path, p);
            return (
                Vec::new(),
                Err(anyhow::anyhow!(
                    "{clash}{by}: nothing was saved. Save the part under another name \
                     (Open it on its own from the timeline, then Save as…), or move \
                     that file away."
                )),
            );
        }
    }
    let mut written = Vec::new();
    for (k, p) in todo.iter().enumerate() {
        let done = model
            .workspace
            .part(p)
            .map_err(anyhow::Error::from)
            .and_then(|part| save(p, part));
        if let Err(e) = done {
            let said = |paths: &[String]| match paths {
                [] => "nothing".to_owned(),
                paths => paths.join(", "),
            };
            let error = anyhow::anyhow!(
                "saving {p}: {e:#}. Written: {}; not written: {}",
                said(&written),
                said(&todo[k..])
            );
            return (written, Err(error));
        }
        written.push(p.clone());
    }
    (written, Ok(()))
}

/// Which part, of those the part at `top` needs, places `path`, said as
/// the end of a sentence (nothing for `top` itself).
fn placing(model: &workpiece_ui::Model, top: &str, path: &str) -> String {
    model
        .needed(top)
        .into_iter()
        .find(|p| {
            model
                .workspace
                .parts
                .get(p)
                .is_some_and(|part| part.placed_parts().iter().any(|q| q == path))
        })
        .map_or_else(String::new, |by| format!(" ({by} places it)"))
}

/// Writes `bytes` to the file at `path`, for the user: in a browser, it is
/// handed to them as a download too.
pub fn export(path: &str, bytes: impl AsRef<[u8]>) -> anyhow::Result<()> {
    files::write(path, bytes.as_ref()).map_err(|e| anyhow::anyhow!("writing {path}: {e}"))?;
    #[cfg(target_arch = "wasm32")]
    crate::web::download(path, bytes.as_ref());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use workpiece_ops_instances::{AddPart, AddPartArgs};
    use workpiece_part::AnyOperation;

    #[test]
    fn parts_load_with_what_they_place() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("workpiece-files-{}", std::process::id()));
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let place = |part: String| -> AnyOperation {
            AddPart {
                id: "p".into(),
                args: AddPartArgs {
                    part,
                    variant: None,
                    hold: Default::default(),
                    bodies: Vec::new(),
                },
            }
            .into()
        };
        save(&path("nut.wpc"), &Part::default())?;
        save(
            &path("bolt.wpc"),
            &Part {
                program: vec![place(path("nut.wpc"))],
                ..Default::default()
            },
        )?;
        let loaded = load(&path("bolt.wpc"), &Workspace::default())?;
        assert_eq!(loaded.len(), 2);
        // What the workspace has already is not read again.
        let mut ws = Workspace::default();
        ws.parts.insert(path("nut.wpc"), Part::default());
        assert_eq!(load(&path("bolt.wpc"), &ws)?.len(), 1);
        assert!(load(&path("missing.wpc"), &ws).is_err());
        std::fs::remove_dir_all(&dir)?;
        assert_eq!(with_extension("a"), "a.wpc");
        assert_eq!(with_extension("a.wpc"), "a.wpc");
        Ok(())
    }

    #[test]
    fn saving_an_assembly_writes_every_part_it_needs() -> anyhow::Result<()> {
        use workpiece_ui::{Message, Model};
        let dir = std::env::temp_dir().join(format!("workpiece-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        let top = path("arm/top.wpc");
        // An assembly that has a file; in it a new part, as the app makes
        // one (its file first), and in that another, in a folder of its
        // own, with a box in it.
        save(&top, &Part::default())?;
        let mut m = Model::new(Workspace::new(load(&top, &Workspace::default())?));
        m.update(Message::Load {
            parts: BTreeMap::new(),
            open: Some(top.clone()),
        })?;
        m.saved.insert(top.clone(), Part::default());
        for new in [path("arm/gearbox.wpc"), path("arm/screws/screw.wpc")] {
            save(&new, &Part::default())?;
            m.update(Message::NewPartHere(Some(new.clone())))?;
            m.update(Message::Saved(new))?;
        }
        let screw = path("arm/screws/screw.wpc");
        let id = m.workspace.part(&screw)?.fresh_id("box");
        let op = workpiece_ops_basic_shapes::AddBox {
            id,
            args: workpiece_ops_basic_shapes::AddBoxArgs {
                frame: Some(workpiece_core::Reference::datum(
                    workpiece_core::world::ORIGIN,
                )),
                ..Default::default()
            },
        };
        m.update(Message::AddOperation {
            part: screw.clone(),
            op: op.into(),
        })?;
        m.update(Message::FinishEdit)?;
        assert_eq!(
            m.unsaved(&top),
            [screw.clone(), path("arm/gearbox.wpc"), top.clone()]
        );
        // One save writes all three, each beside what places it.
        let (written, done) = save_all(&m, &top, false);
        done?;
        assert_eq!(written.len(), 3);
        for p in written {
            m.update(Message::Saved(p))?;
        }
        assert!(m.unsaved_all().is_empty());
        let gearbox = std::fs::read_to_string(path("arm/gearbox.wpc"))?;
        assert!(gearbox.contains("\"screws/screw.wpc\""), "{gearbox}");
        // The files alone open as they were: in a fresh workspace, and
        // on the command line.
        let read = Workspace::new(load(&top, &Workspace::default())?);
        assert_eq!(read.parts.len(), 3);
        let volume = |w: &workpiece_core::World| -> f64 {
            fn all(w: &workpiece_core::World) -> f64 {
                let own: f64 = w
                    .topo
                    .bodies()
                    .iter()
                    .filter_map(|(id, _)| workpiece_boolean::read(&w.topo, id).ok())
                    .map(|s| workpiece_boolean::measure(&s).volume)
                    .sum();
                own + w.instances.values().map(|i| all(&i.world)).sum::<f64>()
            }
            all(w)
        };
        let here = volume(&*m.workspace.build(&top)?);
        assert!(here > 0.0);
        assert!((volume(&*read.build(&top)?) - here).abs() < 1e-9);
        let report = path("report.json");
        let args = ["report", top.as_str(), "--report", report.as_str()];
        workpiece_make::command(&args.map(str::to_owned), &|_| {})?;
        let said: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report)?)?;
        let placed = said["placed"].as_array().cloned().unwrap_or_default();
        let files: Vec<&str> = placed.iter().filter_map(|p| p["file"].as_str()).collect();
        assert_eq!(files, ["gearbox.wpc", "screws/screw.wpc"]);
        let cli: f64 = placed.iter().filter_map(|p| p["volume"].as_f64()).sum();
        assert!((cli - here).abs() < 1e-9, "{cli} {here}");

        // Saved as in another folder, it names the parts where they are.
        let moved = path("elsewhere/top.wpc");
        m.update(Message::SaveAs(moved.clone()))?;
        let (_, done) = save_all(&m, &moved, true);
        done?;
        let text = std::fs::read_to_string(&moved)?;
        assert!(text.contains("\"../arm/gearbox.wpc\""), "{text}");
        let read = Workspace::new(load(&moved, &Workspace::default())?);
        assert!((volume(&*read.build(&moved)?) - here).abs() < 1e-9);

        // A part never read that would go over a file there is refused,
        // and nothing is written.
        let other = path("arm/other.wpc");
        std::fs::write(&other, "{\"program\": []}")?;
        m.workspace.parts.insert(other.clone(), Part::default());
        m.workspace
            .part_mut(&moved)?
            .program
            .push(place_at("o", &other));
        let (written, done) = save_all(&m, &moved, false);
        assert!(written.is_empty());
        let said = format!("{:#}", done.expect_err("refused"));
        assert!(said.contains("nothing was saved"), "{said}");
        assert_eq!(std::fs::read_to_string(&other)?, "{\"program\": []}");
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    fn place_at(id: &str, part: &str) -> AnyOperation {
        AddPart {
            id: id.into(),
            args: AddPartArgs {
                part: part.into(),
                variant: None,
                hold: Default::default(),
                bodies: Vec::new(),
            },
        }
        .into()
    }

    #[test]
    fn recent_files_are_kept_for_each_workspace() -> anyhow::Result<()> {
        let mut here = settings_from(None, "/a");
        assert!(here.recent.is_empty());
        here.remember("x.wpc");
        here.remember("y.wpc");
        here.remember("x.wpc");
        assert_eq!(here.recent, ["x.wpc", "y.wpc"]);
        // (What snaps is kept too, kind by kind.)
        here.snaps = here
            .snaps
            .with(workpiece_core::handles::SnapKind::Grid, false);
        let text = settings_into(None, "/a", &here)?;
        assert_eq!(settings_from(Some(&text), "/a").snaps, here.snaps);
        // Another workspace has its own; the first keeps its.
        let mut there = settings_from(Some(&text), "/b");
        assert!(there.recent.is_empty());
        there.remember("z.wpc");
        let text = settings_into(Some(&text), "/b", &there)?;
        assert_eq!(settings_from(Some(&text), "/a").recent, ["x.wpc", "y.wpc"]);
        assert_eq!(settings_from(Some(&text), "/b").recent, ["z.wpc"]);
        Ok(())
    }
}
