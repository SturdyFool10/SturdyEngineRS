use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::runtime::RuntimeController;

struct WatchedEntry {
    path: PathBuf,
    last_mtime: Option<SystemTime>,
}

/// Polls shader file modification times and reports which paths have changed.
///
/// Call `watch` to register files and `poll_changed` each frame (or on a timer)
/// to get the list of paths whose on-disk modification time has changed since
/// the last poll.
pub struct ShaderWatcher {
    entries: Vec<WatchedEntry>,
}

impl Default for ShaderWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ShaderWatcher {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a shader file path for change detection.
    ///
    /// The current modification time is sampled immediately so the first call
    /// to `poll_changed` will not report a false positive for newly-watched files.
    pub fn watch(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        let last_mtime = mtime(&path);
        self.entries.push(WatchedEntry { path, last_mtime });
    }

    /// Register a path only if it is an actual file (not inline source or missing).
    ///
    /// Returns `true` if the path was registered. Silently no-ops for paths that
    /// do not exist yet — the watcher will begin tracking them on the next call
    /// to `watch` after the file appears.
    pub fn watch_if_file(&mut self, path: impl Into<PathBuf>) -> bool {
        let path = path.into();
        if !path.as_os_str().is_empty() && path.exists() {
            self.watch(path);
            true
        } else {
            false
        }
    }

    /// Remove all watched entries for a given path.
    pub fn unwatch(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
    }

    /// Remove all watched entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Return every path whose modification time has changed since the last poll.
    ///
    /// For each path reported, the stored timestamp is updated so subsequent
    /// polls only report new changes. Paths that no longer exist (mtime = None)
    /// are also included if their previous state was Some.
    pub fn poll_changed(&mut self) -> Vec<PathBuf> {
        let mut changed = Vec::new();
        for entry in &mut self.entries {
            let current = mtime(&entry.path);
            if current != entry.last_mtime {
                entry.last_mtime = current;
                changed.push(entry.path.clone());
            }
        }
        changed
    }

    /// Return all currently watched paths.
    pub fn watched_paths(&self) -> impl Iterator<Item = &Path> {
        self.entries.iter().map(|e| e.path.as_path())
    }

    /// Check every watched path against the runtime controller.
    ///
    /// Paths that exist on disk are reported as `AssetState::Ok`; missing paths
    /// are reported as `AssetState::Missing`. Call this once at startup after
    /// registering all shader files to surface missing-file errors immediately.
    pub fn check_all_with_controller(&self, controller: &RuntimeController) {
        for entry in &self.entries {
            controller.check_asset_path(entry.path.clone());
        }
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
