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

/// A name for a new part: `untitled.wpc`, or with the smallest number
/// from 2 that is neither in `workspace` nor on disk.
pub fn untitled(workspace: &Workspace) -> String {
    let free = |name: &String| !workspace.parts.contains_key(name) && !files::exists(name);
    std::iter::once(format!("untitled.{PART_EXTENSION}"))
        .chain((2..).map(|n| format!("untitled{n}.{PART_EXTENSION}")))
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

/// Reads the part at `path` and, recursively, the parts it places, except
/// those `workspace` has already.
pub fn load(path: &str, workspace: &Workspace) -> anyhow::Result<BTreeMap<String, Part>> {
    let mut parts = BTreeMap::new();
    let mut todo = vec![path.to_owned()];
    while let Some(path) = todo.pop() {
        if parts.contains_key(&path) || workspace.parts.contains_key(&path) {
            continue;
        }
        // A STEP file is no part file: the workspace makes a part of
        // it, whose one step imports it.
        if workpiece_part::is_step(&path) {
            anyhow::ensure!(files::exists(&path), "no file {path}");
            continue;
        }
        let text =
            files::read_to_string(&path).map_err(|e| anyhow::anyhow!("reading {path}: {e}"))?;
        let part = Part::from_text(&text).map_err(|e| e.context(format!("reading {path}")))?;
        todo.extend(part.placed_parts());
        parts.insert(path, part);
    }
    Ok(parts)
}

/// Writes `part` to the file at `path` (in a browser, also handed to the
/// user as a download).
pub fn save(path: &str, part: &Part) -> anyhow::Result<()> {
    export(path, part.to_text()?)
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
