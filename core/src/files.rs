//! The files the program reads and writes, by path: on disk, or — where
//! there is no disk (in a browser) — in memory, where the host puts what
//! the user opens or what it fetches. Everything that reads a file the
//! user names (a part file, a STEP file placed or imported) reads it
//! through here, so the same code works in both places.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

/// Whether the files are kept in memory rather than on disk: in a
/// browser, which has no disk to write to.
pub const IN_MEMORY: bool = cfg!(target_arch = "wasm32");

/// Files in memory, by path: each with how often it was written (what
/// tells one version from the next) and what it holds.
#[derive(Debug, Default)]
pub struct Store {
    files: BTreeMap<String, (u64, Arc<[u8]>)>,
    writes: u64,
}

impl Store {
    /// What the file at `path` holds.
    pub fn read(&self, path: &str) -> io::Result<Arc<[u8]>> {
        self.files
            .get(normal(path))
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no file {path}")))
    }

    /// Puts `bytes` in the file at `path`, in place of what it held.
    pub fn write(&mut self, path: &str, bytes: &[u8]) {
        self.writes += 1;
        self.files
            .insert(normal(path).to_owned(), (self.writes, bytes.into()));
    }

    /// The version of the file at `path`, if there is one.
    pub fn version(&self, path: &str) -> Option<u64> {
        self.files.get(normal(path)).map(|(version, _)| *version)
    }

    /// The paths of all files, sorted.
    pub fn paths(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
}

/// `path` without a leading `./`.
fn normal(path: &str) -> &str {
    let path = path.trim();
    path.strip_prefix("./").unwrap_or(path)
}

/// The files in memory.
fn memory() -> std::sync::MutexGuard<'static, Store> {
    static MEMORY: Mutex<Store> = Mutex::new(Store {
        files: BTreeMap::new(),
        writes: 0,
    });
    MEMORY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the file at `path` holds.
pub fn read(path: &str) -> io::Result<Vec<u8>> {
    if IN_MEMORY {
        memory().read(path).map(|bytes| bytes.to_vec())
    } else {
        std::fs::read(path.trim())
    }
}

/// What the file at `path` holds, as text.
pub fn read_to_string(path: &str) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Writes `bytes` to the file at `path` (making the folders it is in, on
/// disk).
pub fn write(path: &str, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    if IN_MEMORY {
        memory().write(path, bytes.as_ref());
        return Ok(());
    }
    let path = std::path::Path::new(path.trim());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)
}

/// Whether there is a file at `path`.
pub fn exists(path: &str) -> bool {
    if IN_MEMORY {
        memory().version(path).is_some()
    } else {
        std::path::Path::new(path.trim()).exists()
    }
}

/// What tells this version of the file at `path` from another, as far
/// as can be told without reading it (its size and when it was written);
/// `None` if there is no such file.
pub fn stamp(path: &str) -> Option<u64> {
    if IN_MEMORY {
        return memory().version(path);
    }
    let meta = std::fs::metadata(path.trim()).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (meta.len(), modified).hash(&mut hasher);
    Some(hasher.finish())
}

/// The paths of the files in memory, sorted (none where files are on
/// disk).
pub fn in_memory() -> Vec<String> {
    if IN_MEMORY {
        memory().paths()
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_keeps_files_and_tells_their_versions_apart() {
        let mut store = Store::default();
        assert!(store.read("a.step").is_err());
        store.write("./a.step", b"one");
        let first = store.version("a.step");
        store.write("a.step", b"two");
        assert_ne!(store.version("a.step"), first);
        assert_eq!(&*store.read("a.step").unwrap(), b"two");
        assert_eq!(store.paths(), ["a.step"]);
    }
}
