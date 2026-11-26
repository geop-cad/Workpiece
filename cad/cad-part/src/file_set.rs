//! Part files and the parts they place: the one rule for their paths.
//!
//! In a [`Workspace`](crate::Workspace) a part is known by its path as
//! the host names files: relative to its working directory (or whole).
//! In a part file, a part it places is named relative to the folder
//! that file is in. So a folder of parts opens the same wherever it is
//! and whatever the working directory: in the app and on the command
//! line. [`Part::from_file`] and [`Part::to_file`] turn the one into the
//! other; [`read`] reads a part file and, in turn, what it places.

use std::collections::BTreeMap;

use workpiece_ops_instances::AddPart;

use crate::Part;

/// Whether `path` is a whole one, not relative to anything.
fn absolute(path: &str) -> bool {
    path.starts_with('/') || path.starts_with('\\') || path.get(1..2) == Some(":")
}

/// `path` with `/` between its folders and nothing in it that goes
/// nowhere: `.`, a folder and the `..` after it, `//`.
pub fn normal(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let whole = absolute(&path);
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts
                .last()
                .is_some_and(|p| *p != ".." && !p.ends_with(':')) =>
            {
                parts.pop();
            }
            // (Above a whole path's root is the root.)
            ".." if whole && parts.last().is_none_or(|p| p.ends_with(':')) => {}
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    if path.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

/// The folder the file at `path` is in, as a path (empty for the
/// working directory).
pub fn folder_of(path: &str) -> String {
    let path = normal(path);
    match path.rsplit_once('/') {
        Some(("", _)) => "/".to_owned(),
        Some((folder, _)) => folder.to_owned(),
        None => String::new(),
    }
}

/// The path of what the part file at `placing` names `name`: `name` in
/// the folder that file is in.
pub fn beside(placing: &str, name: &str) -> String {
    if absolute(name.trim()) {
        return normal(name);
    }
    let folder = folder_of(placing);
    if folder.is_empty() {
        normal(name)
    } else {
        normal(&format!("{folder}/{name}"))
    }
}

/// `path` made whole, with the working directory, if it is not.
fn whole(path: &str) -> String {
    if absolute(path) {
        return normal(path);
    }
    let here = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    normal(&format!("{here}/{path}"))
}

/// What the part file at `placing` names the file at `path`: `path`
/// relative to the folder it is in (the reverse of [`beside`]).
pub fn relative_to(placing: &str, path: &str) -> String {
    let (mut from, mut to) = (folder_of(placing), normal(path));
    // (From a folder above the working directory, or between a whole
    // path and one that is not, only whole paths tell the way.)
    if absolute(&from) != absolute(&to) || from.split('/').any(|p| p == "..") {
        if std::env::current_dir().is_err() {
            return to;
        }
        (from, to) = (whole(&from), whole(&to));
    }
    let from: Vec<&str> = from.split('/').filter(|p| !p.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|p| !p.is_empty()).collect();
    let same = from
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    // (Whole paths on different drives have nothing in common.)
    if absolute(&to) && same == 0 && !to.starts_with('/') {
        return to;
    }
    let up = std::iter::repeat_n("..", from.len() - same);
    up.chain(to_parts[same..].iter().copied())
        .collect::<Vec<_>>()
        .join("/")
}

impl Part {
    /// The part with what it places named by `name` from what it was.
    fn renamed_placed(mut self, name: impl Fn(&str) -> String) -> Self {
        for op in &mut self.program {
            if let Some(add) = op.downcast_mut::<AddPart>() {
                add.args.part = name(&add.args.part);
            }
        }
        self
    }

    /// The part the file at `path` holds, as `text`: the parts it places
    /// named as the workspace knows them (see the [module](self)).
    pub fn from_file(path: &str, text: &str) -> anyhow::Result<Self> {
        Ok(Self::from_text(text)?.read_at(path))
    }

    /// The part, written by a file at `path` (what it places named
    /// relative to that file), as the workspace has it.
    #[must_use]
    pub fn read_at(self, path: &str) -> Self {
        self.renamed_placed(|placed| beside(path, placed))
    }

    /// What the file at `path` holds of the part: the parts it places
    /// named relative to the folder that file is in.
    pub fn to_file(&self, path: &str) -> anyhow::Result<String> {
        self.clone()
            .renamed_placed(|placed| relative_to(path, placed))
            .to_text()
    }

    /// The part with the part at `from` it places now at `to`.
    #[must_use]
    pub fn placing_moved(self, from: &str, to: &str) -> Self {
        self.renamed_placed(|placed| match placed == from {
            true => to.to_owned(),
            false => placed.to_owned(),
        })
    }
}

/// Reads the part file at `path` and, in turn, the part files it places
/// (each beside the file that places it), except those `have` has
/// already: the parts by their paths.
pub fn read(path: &str, have: &dyn Fn(&str) -> bool) -> anyhow::Result<BTreeMap<String, Part>> {
    read_placed(path, None, have)
}

/// [`read`], with the part at `path` as it is given (if it is): only
/// what it places is read.
pub fn read_placed(
    path: &str,
    part: Option<Part>,
    have: &dyn Fn(&str) -> bool,
) -> anyhow::Result<BTreeMap<String, Part>> {
    let mut parts = BTreeMap::new();
    let mut todo = Vec::new();
    match part {
        Some(part) => {
            todo.extend(
                part.placed_parts()
                    .into_iter()
                    .map(|p| (path.to_owned(), p)),
            );
            parts.insert(path.to_owned(), part);
        }
        None => todo.push((String::new(), path.to_owned())),
    }
    while let Some((by, path)) = todo.pop() {
        if parts.contains_key(&path) || have(&path) {
            continue;
        }
        let placing = |what: &str| match by.is_empty() {
            true => what.to_owned(),
            false => format!("{by} places {what}, which"),
        };
        // A STEP file is no part file: the workspace makes a part of
        // it, whose one step imports it.
        if crate::is_step(&path) {
            anyhow::ensure!(
                workpiece_core::files::exists(&path),
                "{} is no file",
                placing(&path)
            );
            continue;
        }
        let text = workpiece_core::files::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", placing(&path)))?;
        let part =
            Part::from_file(&path, &text).map_err(|e| e.context(format!("reading {path}")))?;
        todo.extend(part.placed_parts().into_iter().map(|p| (path.clone(), p)));
        parts.insert(path, part);
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placed_parts_are_named_beside_the_file_that_places_them() {
        assert_eq!(normal("./a//b/../c.wpc"), "a/c.wpc");
        assert_eq!(normal("../a.wpc"), "../a.wpc");
        assert_eq!(normal("/x/../../a.wpc"), "/a.wpc");
        assert_eq!(folder_of("a.wpc"), "");
        assert_eq!(folder_of("p/q/a.wpc"), "p/q");
        assert_eq!(beside("p/top.wpc", "gear.wpc"), "p/gear.wpc");
        assert_eq!(beside("p/top.wpc", "sub/gear.wpc"), "p/sub/gear.wpc");
        assert_eq!(beside("p/sub/top.wpc", "../gear.wpc"), "p/gear.wpc");
        assert_eq!(beside("top.wpc", "gear.wpc"), "gear.wpc");
        assert_eq!(beside("/w/top.wpc", "gear.wpc"), "/w/gear.wpc");
        assert_eq!(beside("p/top.wpc", "/w/gear.wpc"), "/w/gear.wpc");
        // And back.
        for (placing, path) in [
            ("p/top.wpc", "p/gear.wpc"),
            ("p/top.wpc", "p/sub/gear.wpc"),
            ("p/sub/top.wpc", "p/gear.wpc"),
            ("top.wpc", "q/gear.wpc"),
            ("a/b/top.wpc", "c/gear.wpc"),
            ("/w/x/top.wpc", "/w/y/gear.wpc"),
        ] {
            let named = relative_to(placing, path);
            assert!(!absolute(&named), "{named}");
            assert_eq!(
                beside(placing, &named),
                path,
                "{placing} names {path} {named}"
            );
        }
        assert_eq!(relative_to("p/top.wpc", "p/gear.wpc"), "gear.wpc");
        assert_eq!(relative_to("p/sub/top.wpc", "p/gear.wpc"), "../gear.wpc");
    }
}
