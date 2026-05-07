// Shader file watcher and hot-reload coordinator.
//
// Polls modification times of shader source files. When a change is detected,
// any registered program whose source file matches is reloaded via its
// existing `reload()` method. Failed reloads are reported as diagnostics and
// leave the previous program active (last-known-good semantics).
//
// Roadmap: Track 3 — shader hot reload.

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::Result;

// ── Reloadable trait ──────────────────────────────────────────────────────────

/// Implemented by shader program types that support hot-reload.
pub trait Reloadable {
    /// Path to the on-disk Slang source file, if this program was loaded from a file.
    fn source_path(&self) -> Option<&Path>;
    /// Recompile from the source file.
    ///
    /// Returns `Ok(true)` on success, `Ok(false)` if there is no source path,
    /// and `Err` on compile failure (the previous pipeline is kept).
    fn reload(&mut self) -> Result<bool>;
}

impl Reloadable for crate::ShaderProgram {
    fn source_path(&self) -> Option<&Path> {
        self.source_path()
    }
    fn reload(&mut self) -> Result<bool> {
        self.reload()
    }
}

impl Reloadable for crate::ComputeProgram {
    fn source_path(&self) -> Option<&Path> {
        self.source_path()
    }
    fn reload(&mut self) -> Result<bool> {
        self.reload()
    }
}

// ── Diagnostic ────────────────────────────────────────────────────────────────

/// Result of one hot-reload attempt.
#[derive(Debug)]
pub struct ShaderReloadDiagnostic {
    /// The file that changed and triggered a reload.
    pub path: PathBuf,
    /// Whether the recompilation succeeded.
    pub success: bool,
    /// Compiler error message when `success == false`.
    pub error: Option<String>,
}

impl ShaderReloadDiagnostic {
    /// A one-line human-readable summary for logging.
    pub fn summary(&self) -> String {
        if self.success {
            format!("[shader-reload] ✓ {}", self.path.display())
        } else {
            format!(
                "[shader-reload] ✗ {}: {}",
                self.path.display(),
                self.error.as_deref().unwrap_or("unknown error")
            )
        }
    }
}

// ── ShaderWatcher ─────────────────────────────────────────────────────────────

struct WatchedEntry {
    path: PathBuf,
    last_mtime: Option<SystemTime>,
}

/// Polls shader file modification times and drives hot-reload.
///
/// # Quick start
/// ```ignore
/// // At init:
/// let mut watcher = ShaderWatcher::new();
///
/// // Each frame:
/// for diag in watcher.tick(&mut [&mut my_shader, &mut my_compute]) {
///     eprintln!("{}", diag.summary());
/// }
/// ```
pub struct ShaderWatcher {
    entries: Vec<WatchedEntry>,
}

impl Default for ShaderWatcher {
    fn default() -> Self { Self::new() }
}

impl ShaderWatcher {
    pub fn new() -> Self { Self { entries: Vec::new() } }

    /// Register a shader file path for change detection.
    ///
    /// The current modification time is sampled immediately so the first `tick`
    /// call will not report a false positive for newly-watched files.
    pub fn watch(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        let last_mtime = mtime(&path);
        self.entries.push(WatchedEntry { path, last_mtime });
    }

    /// Register a path only if it exists on disk (ignores inline source, missing files).
    ///
    /// Returns `true` if registered.
    pub fn watch_if_file(&mut self, path: impl Into<PathBuf>) -> bool {
        let path = path.into();
        if !path.as_os_str().is_empty() && path.exists() {
            self.watch(path);
            true
        } else {
            false
        }
    }

    /// Auto-register a program's source path if it came from a file.
    pub fn watch_program(&mut self, program: &impl Reloadable) {
        if let Some(p) = program.source_path() {
            self.watch_if_file(p);
        }
    }

    /// Remove all watched entries for a given path.
    pub fn unwatch(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
    }

    /// Remove all watched entries.
    pub fn clear(&mut self) { self.entries.clear(); }

    /// Return paths whose modification time has changed since the last poll.
    /// Updates internal timestamps so subsequent calls only report new changes.
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

    /// Poll for file changes and reload any matching programs in `programs`.
    ///
    /// Each program in `programs` is checked against every changed path via
    /// its `source_path()`. Matching programs are reloaded; failed reloads
    /// emit a diagnostic and keep the previous pipeline active.
    ///
    /// Returns one diagnostic per changed path (even if no program matched).
    ///
    /// # Example
    /// ```ignore
    /// let diags = watcher.tick(&mut [&mut scene_shader, &mut bloom_shader]);
    /// for d in &diags {
    ///     if !d.success { log::warn!("{}", d.summary()); }
    /// }
    /// ```
    pub fn tick(&mut self, programs: &mut [&mut dyn Reloadable]) -> Vec<ShaderReloadDiagnostic> {
        let changed = self.poll_changed();
        let mut diags = Vec::new();

        for path in changed {
            let mut reloaded = false;
            let mut last_err: Option<String> = None;

            for program in programs.iter_mut() {
                if let Some(src) = program.source_path() {
                    if src == path {
                        match program.reload() {
                            Ok(true) => { reloaded = true; }
                            Ok(false) => {}
                            Err(e) => { last_err = Some(e.to_string()); }
                        }
                    }
                }
            }

            let success = reloaded && last_err.is_none();
            diags.push(ShaderReloadDiagnostic {
                path,
                success,
                error: last_err,
            });
        }
        diags
    }

    /// Return all currently watched paths.
    pub fn watched_paths(&self) -> impl Iterator<Item = &Path> {
        self.entries.iter().map(|e| e.path.as_path())
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
