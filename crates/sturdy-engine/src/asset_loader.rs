//! Asset loading, caching, and the [`AssetHandle`] type.
//!
//! # Quick start
//!
//! ```ignore
//! // In init:
//! let tex = engine.load_texture_2d("assets/diffuse.png");
//!
//! // In render (handle is clone-able; test before binding):
//! if tex.is_ready() {
//!     tex.with(|image| frame.bind_image("albedo", image));
//! } else {
//!     fallback.with(|image| frame.bind_image("albedo", image));
//! }
//! ```
//!
//! # Placeholder policy
//!
//! When a texture fails to load, the engine does **not** panic. Instead,
//! supply a checkerboard fallback via [`Engine::checkerboard_texture`] and use
//! it whenever a handle is not yet `Ready`.
//!
//! # Deduplication
//!
//! [`AssetCache`] prevents double-loading. Paths are normalised to absolute
//! form before lookup so relative-path variants of the same file share a handle.

use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};
use parking_lot::{Mutex, MutexGuard};

use crate::texture_compression::compress_texture;
use crate::{Engine, Format, FrameSyncReason, Image, Result, TextureUploadDesc};

// ── PendingUpload ─────────────────────────────────────────────────────────────

/// A texture that has been decoded and compressed on a background thread and is
/// waiting for its GPU upload to be performed on the render thread.
///
/// Stored in `Engine::pending_uploads`; drained by `Engine::drain_pending_uploads`.
pub(crate) struct PendingUpload {
    pub handle: AssetHandle<Image>,
    pub name: String,
    pub data: Vec<u8>,
    pub format: Format,
    pub width: u32,
    pub height: u32,
}

// ── CPU-only decode+compress ──────────────────────────────────────────────────

/// Decode and (optionally) compress a texture file — CPU work only, no GPU.
///
/// Returns a `PendingUpload` ready to be submitted to the GPU. Called from
/// rayon worker threads by `load_texture_2d_from_path_async`.
pub(crate) fn decode_and_compress(path: &Path, name: &str) -> Result<(Vec<u8>, Format, u32, u32)> {
    tracing::debug!(path = %path.display(), name, "decoding texture");
    let dyn_image = image::open(path)
        .map_err(|e| crate::Error::Unknown(format!("failed to open '{}': {e}", path.display())))?;
    let source_channels = dyn_image.color().channel_count() as u32;
    let rgba = dyn_image.into_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let pixels: Vec<u8> = rgba.into_raw();

    if let Some(compressed) = compress_texture(
        &pixels,
        width,
        height,
        source_channels,
        name,
        Some(path),
        true,
    ) {
        tracing::debug!(
            name,
            width = compressed.width,
            height = compressed.height,
            format = ?compressed.format,
            "texture block-compressed"
        );
        return Ok((
            compressed.data,
            compressed.format,
            compressed.width,
            compressed.height,
        ));
    }
    tracing::debug!(
        name,
        width,
        height,
        "texture not block-compressed (uploading as Rgba8Unorm)"
    );
    Ok((pixels, Format::Rgba8Unorm, width, height))
}

/// Async variant of `load_texture_2d_from_path`: returns a `Loading` handle
/// immediately and spawns a rayon task to decode+compress, then pushes to the
/// engine's `pending_uploads` queue for GPU upload at frame-start drain.
pub(crate) fn load_texture_2d_async(
    path: impl AsRef<Path>,
    pending: std::sync::Arc<parking_lot::Mutex<Vec<PendingUpload>>>,
) -> AssetHandle<Image> {
    let path = path.as_ref().to_owned();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("texture")
        .to_owned();

    tracing::debug!(path = %path.display(), name = %name, "queuing async texture load");
    let handle: AssetHandle<Image> = AssetHandle::new_loading();
    let handle_bg = handle.clone();

    rayon::spawn(move || match decode_and_compress(&path, &name) {
        Ok((data, format, width, height)) => {
            tracing::debug!(
                name = %name,
                format = ?format,
                width,
                height,
                "texture decoded, queuing GPU upload"
            );
            let upload = PendingUpload {
                handle: handle_bg,
                name,
                data,
                format,
                width,
                height,
            };
            //panic allowed, reason = "poisoned internal pending upload queue is unrecoverable"
            pending
                .lock()
                .push(upload);
        }
        Err(e) => {
            tracing::error!(path = %path.display(), "async texture load failed: {e}");
            handle_bg.set_failed(format!("load_texture_2d '{}': {e}", path.display()));
        }
    });

    handle
}

// ── LoadState ────────────────────────────────────────────────────────────────

/// Lifecycle state of a tracked asset.
#[derive(Debug)]
pub enum LoadState<T> {
    /// Load has been submitted but not yet completed.
    Loading,
    /// Asset is fully loaded and ready to use.
    Ready(T),
    /// Asset loaded but at reduced quality (e.g. lower mip, degraded codec).
    /// The inner value is usable; `reason` explains the degradation.
    Degraded { asset: T, reason: String },
    /// Load failed permanently. The `reason` string contains the error.
    Failed(String),
}

impl<T> LoadState<T> {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }

    pub fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }

    /// Return the inner asset if it is `Ready` or `Degraded`.
    pub fn asset(&self) -> Option<&T> {
        match self {
            Self::Ready(t) | Self::Degraded { asset: t, .. } => Some(t),
            _ => None,
        }
    }

    /// Return the failure reason if state is `Failed`.
    pub fn failed_reason(&self) -> Option<&str> {
        match self {
            Self::Failed(r) => Some(r),
            _ => None,
        }
    }

    /// Return the degradation reason if state is `Degraded`.
    pub fn degraded_reason(&self) -> Option<&str> {
        match self {
            Self::Degraded { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

// ── AssetHandle ───────────────────────────────────────────────────────────────

/// A lightweight, clone-able handle to an in-progress or completed asset load.
///
/// Cloning a handle produces a second reference to the **same** underlying
/// state; if a background loader updates the state to `Ready`, all clones
/// immediately see it.
///
/// # State queries
///
/// | method | returns `true` when |
/// |--------|---------------------|
/// | [`is_ready`](Self::is_ready) | asset is fully usable |
/// | [`is_loading`](Self::is_loading) | load is in progress |
/// | [`is_degraded`](Self::is_degraded) | usable at reduced quality |
/// | [`is_failed`](Self::is_failed) | load failed permanently |
pub struct AssetHandle<T> {
    inner: Arc<Mutex<LoadState<T>>>,
}

impl<T> AssetHandle<T> {
    /// Create a handle that immediately reports `Ready`.
    #[allow(dead_code)]
    pub(crate) fn new_ready(value: T) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LoadState::Ready(value))),
        }
    }

    /// Create a handle that immediately reports `Failed`.
    #[allow(dead_code)]
    pub(crate) fn new_failed(reason: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LoadState::Failed(reason.into()))),
        }
    }

    /// Create a handle in the `Loading` state for background loads.
    #[allow(dead_code)]
    pub(crate) fn new_loading() -> Self {
        Self {
            inner: Arc::new(Mutex::new(LoadState::Loading)),
        }
    }

    // ── State transitions (called by the loader, not by user code) ─────────

    #[allow(dead_code)]
    pub(crate) fn set_ready(&self, value: T) {
        *self.lock() = LoadState::Ready(value);
    }

    #[allow(dead_code)]
    pub(crate) fn set_failed(&self, reason: impl Into<String>) {
        *self.lock() = LoadState::Failed(reason.into());
    }

    #[allow(dead_code)]
    pub(crate) fn set_degraded(&self, asset: T, reason: impl Into<String>) {
        *self.lock() = LoadState::Degraded {
            asset,
            reason: reason.into(),
        };
    }

    // ── Public state queries ───────────────────────────────────────────────

    /// `true` when the asset is fully loaded and ready to use.
    pub fn is_ready(&self) -> bool {
        self.lock().is_ready()
    }

    /// `true` when the load has been submitted but not yet completed.
    pub fn is_loading(&self) -> bool {
        self.lock().is_loading()
    }

    /// `true` when the asset loaded at reduced quality.
    pub fn is_degraded(&self) -> bool {
        self.lock().is_degraded()
    }

    /// `true` when the load failed permanently.
    pub fn is_failed(&self) -> bool {
        self.lock().is_failed()
    }

    /// Return the failure reason, or `None` if not failed.
    pub fn failed_reason(&self) -> Option<String> {
        self.lock().failed_reason().map(str::to_owned)
    }

    /// Return the degradation reason, or `None` if not degraded.
    pub fn degraded_reason(&self) -> Option<String> {
        self.lock().degraded_reason().map(str::to_owned)
    }

    // ── Asset access ───────────────────────────────────────────────────────

    /// Call `f` with a reference to the inner asset if it is `Ready` or
    /// `Degraded`, returning the result wrapped in `Some`. Returns `None`
    /// while the asset is `Loading` or `Failed`.
    ///
    /// The lock is held for the duration of `f`. Do not call engine methods
    /// that acquire the same lock inside `f`.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let guard = self.lock();
        guard.asset().map(f)
    }

    // ── Internals ──────────────────────────────────────────────────────────

    fn lock(&self) -> MutexGuard<'_, LoadState<T>> {
        //panic allowed, reason = "poisoned asset handle mutex is unrecoverable"
        self.inner
            .lock()
    }
}

impl<T: Clone> AssetHandle<T> {
    /// Clone the inner asset value if it is `Ready` or `Degraded`.
    ///
    /// Prefer [`with`](Self::with) when `T: Clone` is expensive; use this
    /// for cheap clones like handles or IDs.
    pub fn try_clone_inner(&self) -> Option<T> {
        self.lock().asset().cloned()
    }
}

impl<T> Clone for AssetHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for AssetHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.lock() {
            LoadState::Loading => write!(f, "AssetHandle(Loading)"),
            LoadState::Ready(_) => write!(f, "AssetHandle(Ready)"),
            LoadState::Degraded { reason, .. } => {
                write!(f, "AssetHandle(Degraded: {reason})")
            }
            LoadState::Failed(r) => write!(f, "AssetHandle(Failed: {r})"),
        }
    }
}

// ── Texture loading ───────────────────────────────────────────────────────────

/// Load a texture from `path`, upload it to the GPU, and return a `Ready` handle.
///
/// Supports PNG, JPEG, WebP, and BMP (any format the `image` crate decodes).
/// The image is converted to RGBA8 before upload.
///
/// On success the handle is immediately `Ready`. On failure it is `Failed` with
/// the error message — no panic.
///
/// Called by [`Engine::load_texture_2d`].
/// Load an HDR or EXR image as a linear-float GPU texture.
///
/// Output format is `Rgba16Float` — sufficient for environment maps and most
/// HDR content. Use `load_hdr_texture_32f` for full f32 precision.
///
/// Supports `.hdr` (RGBE radiance format) and `.exr` (OpenEXR) via the
/// `image` crate's `hdr` and `exr` features.
pub(crate) fn load_hdr_texture_from_path(engine: &Engine, path: impl AsRef<Path>) -> Result<Image> {
    let path = path.as_ref();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("hdr_tex")
        .to_owned();
    load_and_upload_hdr(engine, path, &name, false)
}

/// Load an HDR or EXR image as a full 32-bit float RGBA GPU texture.
pub(crate) fn load_hdr_texture_32f_from_path(
    engine: &Engine,
    path: impl AsRef<Path>,
) -> Result<Image> {
    let path = path.as_ref();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("hdr_tex")
        .to_owned();
    load_and_upload_hdr(engine, path, &name, true)
}

fn load_and_upload_hdr(engine: &Engine, path: &Path, name: &str, full_32f: bool) -> Result<Image> {
    let dyn_image = image::open(path).map_err(|e| {
        crate::Error::Unknown(format!("failed to open hdr '{}': {e}", path.display()))
    })?;

    // Convert to f32 per channel regardless of source encoding.
    let rgba_f32 = dyn_image.into_rgba32f();
    let (width, height) = (rgba_f32.width(), rgba_f32.height());
    let raw: &[f32] = rgba_f32.as_raw();

    let mut frame = engine.begin_frame()?;
    let image = if full_32f {
        // Full f32 — 16 bytes/pixel.
        let bytes: &[u8] = bytemuck::cast_slice(raw);
        frame.upload_texture_2d(
            name,
            TextureUploadDesc::sampled_rgba32f(width, height),
            bytes,
        )?
    } else {
        // Convert f32 → f16 before upload — 8 bytes/pixel.
        let f16_pixels: Vec<half::f16> = raw.iter().map(|&v| half::f16::from_f32(v)).collect();
        let bytes: &[u8] = bytemuck::cast_slice(&f16_pixels);
        frame.upload_texture_2d(
            name,
            TextureUploadDesc::sampled_rgba16f(width, height),
            bytes,
        )?
    };
    let _ = image.set_debug_name(&format!("hdr-{name}"));
    frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
    frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
    Ok(image)
}

pub(crate) fn load_and_upload_blocking(engine: &Engine, path: &Path, name: &str) -> Result<Image> {
    load_and_upload(engine, path, name)
}

fn load_and_upload(engine: &Engine, path: &Path, name: &str) -> Result<Image> {
    // Decode via the `image` crate (handles PNG, JPEG, WebP, BMP, …)
    let dyn_image = image::open(path)
        .map_err(|e| crate::Error::Unknown(format!("failed to open '{}': {e}", path.display())))?;

    let source_channels = dyn_image.color().channel_count() as u32;
    let rgba = dyn_image.into_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let pixels: Vec<u8> = rgba.into_raw();

    // Attempt block compression. Falls back to Rgba8Unorm on failure or for
    // textures smaller than 4×4 pixels.
    if let Some(compressed) = compress_texture(
        &pixels,
        width,
        height,
        source_channels,
        name,
        Some(path),
        true, // prefer_compressed
    ) {
        let desc = TextureUploadDesc {
            width: compressed.width,
            height: compressed.height,
            format: compressed.format,
            usage: crate::ImageUsage::SAMPLED,
            prefer_compressed: true,
        generate_mips: true,
        };
        let mut frame = engine.begin_frame()?;
        let image = frame.upload_texture_2d(name, desc, &compressed.data)?;
        let _ = image.set_debug_name(&format!("tex2d-{name}"));
        frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
        frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
        return Ok(image);
    }

    // Fallback: uncompressed Rgba8Unorm.
    let mut frame = engine.begin_frame()?;
    let image = frame.upload_texture_2d(
        name,
        TextureUploadDesc::sampled_rgba8(width, height),
        &pixels,
    )?;
    let _ = image.set_debug_name(&format!("tex2d-{name}"));
    frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
    frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
    Ok(image)
}

// ── Checkerboard placeholder ──────────────────────────────────────────────────

/// Generate a checkerboard texture for use as a placeholder while a real texture
/// is loading or when a path cannot be found.
///
/// `size` is the total image side length (clamped to a power-of-two minimum of 4).
/// `tile_size` is the size of each check square in pixels.
///
/// The pattern alternates between a vivid magenta (`#FF00FF`) and a dark grey
/// (`#222222`) — distinctive enough to be immediately recognisable as a
/// missing-texture indicator in a rendered scene.
///
/// Called by [`Engine::checkerboard_texture`].
pub(crate) fn make_checkerboard(engine: &Engine, size: u32, tile_size: u32) -> Result<Image> {
    let size = size.next_power_of_two().max(4);
    let tile = tile_size.max(1);
    let mut frame = engine.begin_frame()?;
    let image = frame.upload_texture_2d(
        "checkerboard",
        TextureUploadDesc::sampled_rgba8(size, size),
        &checkerboard_pixels(size, tile),
    )?;
    let _ = image.set_debug_name("checkerboard-placeholder");
    frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
    frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
    Ok(image)
}

fn checkerboard_pixels(size: u32, tile: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let even = ((x / tile) + (y / tile)) % 2 == 0;
            let rgba: [u8; 4] = if even {
                [0xFF, 0x00, 0xFF, 0xFF] // magenta
            } else {
                [0x22, 0x22, 0x22, 0xFF] // dark grey
            };
            let i = ((y * size + x) * 4) as usize;
            pixels[i..i + 4].copy_from_slice(&rgba);
        }
    }
    pixels
}

// ── AssetCache ────────────────────────────────────────────────────────────────

/// A simple cache that prevents double-loading assets by path.
///
/// Keys are stored as canonical absolute paths when possible; relative paths are
/// accepted and stored as-is when canonicalisation fails (e.g. path not yet on disk).
///
/// # Usage
///
/// ```ignore
/// let mut cache: AssetCache<Image> = AssetCache::new();
///
/// // First call loads; subsequent calls with the same path return the cached handle.
/// let handle = cache.get_or_load("assets/rock.png", || engine.load_texture_2d("assets/rock.png"));
/// ```
pub struct AssetCache<T> {
    entries: HashMap<PathBuf, AssetHandle<T>>,
}

impl<T> AssetCache<T> {
    /// Create an empty cache.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Return an existing handle for `path`, or `None` if not cached.
    pub fn get(&self, path: impl AsRef<Path>) -> Option<AssetHandle<T>> {
        self.entries.get(&canonical(path)).cloned()
    }

    /// Return the cached handle for `path`, or call `load_fn` to produce one
    /// and insert it before returning.
    pub fn get_or_load(
        &mut self,
        path: impl AsRef<Path>,
        load_fn: impl FnOnce() -> AssetHandle<T>,
    ) -> AssetHandle<T> {
        let key = canonical(&path);
        if let Some(handle) = self.entries.get(&key) {
            return handle.clone();
        }
        let handle = load_fn();
        self.entries.insert(key, handle.clone());
        handle
    }

    /// Remove the cached entry for `path`. Returns `true` if it was present.
    ///
    /// Existing clones of the handle are unaffected; they still point to the
    /// same state. The next call to `get_or_load` with this path will trigger
    /// a fresh load.
    pub fn invalidate(&mut self, path: impl AsRef<Path>) -> bool {
        self.entries.remove(&canonical(path)).is_some()
    }

    /// Number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove all cached entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<T> Default for AssetCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for AssetCache<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AssetCache")
            .field("len", &self.entries.len())
            .finish()
    }
}

pub(crate) fn canonical(path: impl AsRef<Path>) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or_else(|_| path.as_ref().to_path_buf())
}

// ── Async HDR loading ─────────────────────────────────────────────────────────

/// Async variant of `load_hdr_texture`: returns an `AssetHandle<Image>` in the
/// `Loading` state immediately and spawns a rayon worker to decode the HDR/EXR
/// file and push it to the pending upload queue.
pub(crate) fn load_hdr_texture_async(
    path: impl AsRef<Path>,
    pending: std::sync::Arc<parking_lot::Mutex<Vec<PendingUpload>>>,
    full_32f: bool,
) -> AssetHandle<Image> {
    let path = path.as_ref().to_owned();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("hdr_tex")
        .to_owned();

    let handle: AssetHandle<Image> = AssetHandle::new_loading();
    let handle_bg = handle.clone();

    rayon::spawn(move || match decode_hdr(&path, &name, full_32f) {
        Ok((data, format, width, height)) => {
            let upload = PendingUpload {
                handle: handle_bg,
                name,
                data,
                format,
                width,
                height,
            };
            //panic allowed, reason = "poisoned internal pending upload queue is unrecoverable"
            pending
                .lock()
                .push(upload);
        }
        Err(e) => {
            handle_bg.set_failed(format!("load_hdr_texture '{}': {e}", path.display()));
        }
    });

    handle
}

fn decode_hdr(path: &Path, _name: &str, full_32f: bool) -> Result<(Vec<u8>, Format, u32, u32)> {
    let dyn_image = image::open(path).map_err(|e| {
        crate::Error::Unknown(format!("failed to open hdr '{}': {e}", path.display()))
    })?;
    let rgba_f32 = dyn_image.into_rgba32f();
    let (width, height) = (rgba_f32.width(), rgba_f32.height());
    let raw: &[f32] = rgba_f32.as_raw();

    if full_32f {
        let bytes: Vec<u8> = bytemuck::cast_slice(raw).to_vec();
        Ok((bytes, Format::Rgba32Float, width, height))
    } else {
        let f16_pixels: Vec<half::f16> = raw.iter().map(|&v| half::f16::from_f32(v)).collect();
        let bytes: Vec<u8> = bytemuck::cast_slice(&f16_pixels).to_vec();
        Ok((bytes, Format::Rgba16Float, width, height))
    }
}
