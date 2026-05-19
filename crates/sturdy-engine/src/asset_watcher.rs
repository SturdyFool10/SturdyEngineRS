// Asset hot-reload watcher for textures and meshes.
//
// Polls modification times of on-disk asset files. When a change is detected,
// registered callbacks are invoked so the application can reload the asset into
// its existing GPU resource.  The pattern mirrors `ShaderWatcher` — last-known-
// good semantics, one-liner diagnostics, zero background threads.
//
// # Quick start
// ```ignore
// // At init:
// let mut watcher = AssetWatcher::new();
//
// // Register a texture file to watch.
// watcher.watch_texture("assets/rock_albedo.png", texture_arc.clone(), &engine);
//
// // Each frame:
// for diag in watcher.tick(&engine, &frame) {
//     eprintln!("{}", diag.summary());
// }
// ```
//
// Roadmap: Track 3 — asset hot reload.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use crate::{Engine, Image, Result};

// ── Diagnostic ────────────────────────────────────────────────────────────────

/// Result of one asset hot-reload attempt.
#[derive(Debug)]
pub struct AssetReloadDiagnostic {
    /// The file that changed and triggered a reload.
    pub path: PathBuf,
    /// Whether the reload succeeded.
    pub success: bool,
    /// Error message when `success == false`.
    pub error: Option<String>,
}

impl AssetReloadDiagnostic {
    pub fn summary(&self) -> String {
        if self.success {
            format!("[asset-reload] ✓ {}", self.path.display())
        } else {
            format!(
                "[asset-reload] ✗ {}: {}",
                self.path.display(),
                self.error.as_deref().unwrap_or("unknown error")
            )
        }
    }
}

// ── Internal watch entry ──────────────────────────────────────────────────────

enum AssetKind {
    /// Texture slot: `Arc<Mutex<Option<Arc<Image>>>>` — atomically replaced on reload.
    Texture(Arc<Mutex<Option<Arc<Image>>>>),
    /// Arbitrary reload callback.
    Callback(Box<dyn FnMut(&Engine, &Path) -> Result<()> + Send + Sync>),
}

struct WatchEntry {
    path: PathBuf,
    last_mtime: Option<SystemTime>,
    kind: AssetKind,
}

// ── AssetWatcher ──────────────────────────────────────────────────────────────

/// Watches asset files on disk and drives hot-reload via callbacks or slot swapping.
///
/// # Usage patterns
///
/// ## Callback-based (most flexible)
/// ```ignore
/// watcher.watch_with_callback("assets/rock.png", move |engine, path| {
///     let new_tex = engine.load_texture_2d(path)?;
///     *my_texture.lock()? = Some(new_tex);
///     Ok(())
/// });
/// ```
///
/// ## Texture slot (convenience)
/// ```ignore
/// let slot = Arc::new(Mutex::new(Some(engine.load_texture_2d("assets/rock.png")?)));
/// watcher.watch_texture("assets/rock.png", slot.clone(), &engine);
/// // Each frame: render with `if let Some(texture) = slot.lock()?.as_ref() { ... }`
/// ```
pub struct AssetWatcher {
    entries: Vec<WatchEntry>,
}

impl Default for AssetWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetWatcher {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Watch an asset file and invoke `callback(engine, path)` each time it changes.
    ///
    /// The callback is responsible for loading the asset and updating any GPU resources.
    /// On error, `tick` reports the failure and leaves the previous asset active.
    pub fn watch_with_callback(
        &mut self,
        path: impl Into<PathBuf>,
        callback: impl FnMut(&Engine, &Path) -> Result<()> + Send + Sync + 'static,
    ) {
        let path = path.into();
        let last_mtime = mtime(&path);
        self.entries.push(WatchEntry {
            path,
            last_mtime,
            kind: AssetKind::Callback(Box::new(callback)),
        });
    }

    /// Watch a texture file and keep a shared `Arc<Mutex<Option<Arc<Image>>>>` slot updated.
    ///
    /// When the file changes, a new `Image` is loaded and the slot is replaced atomically.
    /// Callers read the current texture via `slot.lock().as_ref().map(|a| a.as_ref())`.
    ///
    /// # Example
    /// ```ignore
    /// let slot = Arc::new(Mutex::new(None::<Arc<Image>>));
    /// watcher.watch_texture("rock_albedo.png", Arc::clone(&slot), &engine);
    ///
    /// // Each frame:
    /// if let Some(tex) = slot.lock()?.as_ref() {
    ///     frame.bind_image("albedo", tex);
    /// }
    /// ```
    pub fn watch_texture(
        &mut self,
        path: impl Into<PathBuf>,
        slot: Arc<Mutex<Option<Arc<Image>>>>,
        engine: &Engine,
    ) {
        let path = path.into();
        let last_mtime = mtime(&path);
        // Populate the slot immediately so it's ready before the first tick.
        let slot_is_empty = slot.lock().map(|slot| slot.is_none()).unwrap_or(false);
        if slot_is_empty {
            if let Ok(img) = engine.load_texture_2d_blocking(&path) {
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(Arc::new(img));
                }
            }
        }
        self.entries.push(WatchEntry {
            path,
            last_mtime,
            kind: AssetKind::Texture(slot),
        });
    }

    /// Remove all watched entries for a given path.
    pub fn unwatch(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
    }

    /// Remove all watched entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Poll for file changes and reload any modified assets.
    ///
    /// Returns one `AssetReloadDiagnostic` per changed file.
    /// Call once per frame; the engine is used only for reloading (no frame ownership needed).
    pub fn tick(&mut self, engine: &Engine) -> Vec<AssetReloadDiagnostic> {
        let mut diagnostics = Vec::new();

        for entry in &mut self.entries {
            let current = mtime(&entry.path);
            if current == entry.last_mtime {
                continue;
            }
            entry.last_mtime = current;

            let result = match &mut entry.kind {
                AssetKind::Texture(slot) => match engine.load_texture_2d_blocking(&entry.path) {
                    Ok(img) => slot
                        .lock()
                        .map(|mut slot| {
                            *slot = Some(Arc::new(img));
                        })
                        .map_err(|_| {
                            crate::Error::Unknown(
                                "asset watcher texture slot mutex poisoned".into(),
                            )
                        }),
                    Err(e) => Err(e),
                },
                AssetKind::Callback(cb) => cb(engine, &entry.path),
            };

            diagnostics.push(AssetReloadDiagnostic {
                path: entry.path.clone(),
                success: result.is_ok(),
                error: result.err().map(|e| e.to_string()),
            });
        }

        diagnostics
    }

    /// Number of watched entries.
    pub fn watch_count(&self) -> usize {
        self.entries.len()
    }
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
