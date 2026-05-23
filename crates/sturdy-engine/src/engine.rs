use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use sturdy_engine_core as core;

use crate::upload_arena::UploadArena;
use crate::{
    AccelerationStructure, AccelerationStructureBuildSizes, AccelerationStructureDesc, AssetHandle,
    BackendKind, BackendRawCapabilities, BindGroup, BindGroupDesc, BindlessHandle, BlasBuildDesc,
    Buffer, BufferDesc, CanonicalPipelineLayout, Caps, ComputePipelineDesc, Error,
    ExternalBufferDesc, ExternalImageDesc, Format, FormatCapabilities, Frame, FrameSyncReason,
    GpuCaptureDesc, GpuCaptureTool, GpuMemoryBudget, GraphFrame, GraphImage, GraphImageCacheKey,
    GraphicsPipelineDesc, Image, ImageDesc, ImageUsage, MeshPrimitive, NativeHandleCapabilities,
    Pipeline, PipelineLayout, RayTracingPipelineDesc, RayTracingShaderBindingTable, RenderFrame,
    Result, Sampler, SamplerDesc, SamplerPreset, Shader, ShaderBindingTableDesc, ShaderDesc,
    ShaderName, ShaderProgram, ShaderProgramDesc, ShaderReflection, ShaderSource, ShaderStage,
    SlangEntryPoints, Surface, SurfaceHdrPreference, SurfaceImage, SurfaceSize, TextureUploadDesc,
    TlasBuildDesc, asset_loader, engine_global, mesh_loader, sampler_catalog,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::NativeSurfaceDesc;

// Compile-time proof that Engine is safe to share across threads.
// If this fails, a field was added that is not Send + Sync.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<Engine>();
};

// ── Engine ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Engine {
    pub(crate) device: core::Device,
    graph_image_cache: Arc<Mutex<HashMap<GraphImageCacheKey, Image>>>,
    sampler_catalog: Arc<sampler_catalog::SamplerCatalog>,
    /// Textures decoded and compressed on rayon workers, waiting for GPU upload.
    /// Drained once per frame by [`Engine::drain_pending_uploads`].
    pending_uploads: Arc<Mutex<Vec<asset_loader::PendingUpload>>>,
    /// Global texture cache: canonical path → handle. Prevents duplicate loads
    /// when the same path is passed to `load_texture_2d` multiple times.
    texture_cache: Arc<Mutex<HashMap<std::path::PathBuf, AssetHandle<Image>>>>,
    /// Unix-second timestamp of the last VRAM over-budget log line.
    /// Used to throttle the warning to at most once per 5 seconds.
    last_budget_warn_secs: Arc<std::sync::atomic::AtomicU64>,
}

impl Engine {
    pub fn new() -> Result<Self> {
        Self::with_backend(BackendKind::Auto)
    }

    pub fn with_backend(backend: BackendKind) -> Result<Self> {
        let mut desc = core::DeviceDesc {
            backend,
            validation: cfg!(debug_assertions),
            adapter: core::AdapterSelection::Auto,
            ..core::DeviceDesc::default()
        };
        desc = desc
            .prefer_feature(core::DeviceFeature::SamplerAnisotropy)
            .prefer_feature(core::DeviceFeature::BindlessResources)
            .prefer_feature(core::DeviceFeature::BufferDeviceAddress)
            .prefer_feature(core::DeviceFeature::MeshShading)
            .prefer_feature(core::DeviceFeature::RayTracing)
            .prefer_feature(core::DeviceFeature::RayQuery);
        Self::with_desc(desc)
    }

    pub fn with_desc(desc: core::DeviceDesc) -> Result<Self> {
        let device = core::Device::create(desc)?;
        let mut engine = Self {
            device,
            graph_image_cache: Arc::new(Mutex::new(HashMap::new())),
            sampler_catalog: Arc::new(sampler_catalog::SamplerCatalog::empty()),
            pending_uploads: Arc::new(Mutex::new(Vec::new())),
            texture_cache: Arc::new(Mutex::new(HashMap::new())),
            last_budget_warn_secs: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        let catalog = sampler_catalog::SamplerCatalog::build(&engine)?;
        engine.sampler_catalog = Arc::new(catalog);
        Ok(engine)
    }

    /// Return the handle for a sampler preset. Used internally to resolve shader bindings.
    pub fn sampler_handle(&self, preset: SamplerPreset) -> core::SamplerHandle {
        self.sampler_catalog.handle(preset)
    }

    pub(crate) fn default_sampler(&self) -> core::SamplerHandle {
        self.sampler_catalog.handle(SamplerPreset::Linear)
    }

    pub fn caps(&self) -> Caps {
        self.device.caps()
    }

    /// Current GPU memory usage and sub-allocator capacity.
    ///
    /// Returns `None` when the backend doesn't expose allocation statistics.
    /// Cheap to call every frame (one mutex read). Use `budget.over_budget()` or
    /// `budget.summary()` to log VRAM pressure.
    ///
    /// # Example
    /// ```ignore
    /// if let Some(b) = engine.memory_budget() {
    ///     tracing::warn!("{}", b.summary()); // "VRAM 423 / 512 MiB (82 %) [over budget]"
    /// }
    /// ```
    pub fn memory_budget(&self) -> Option<GpuMemoryBudget> {
        self.device.memory_budget()
    }

    // ── Global accessor ───────────────────────────────────────────────────────

    /// Set the process-global engine instance.
    ///
    /// Called automatically by all shell entry points (`run_game`,
    /// `run_headless`, `try_run`, etc.) before any application code runs.
    /// You should never need to call this manually.
    ///
    /// If the global is already set (e.g. in tests with multiple engines),
    /// the call is silently ignored — the first engine wins.
    pub(crate) fn set_global(engine: &Engine) {
        engine_global::set_engine(engine);
    }

    /// Access the process-global engine from **any thread at any time**.
    ///
    /// Returns a `&'static Engine` with zero overhead — no locking, no
    /// allocation, no indirection beyond what `Engine` already pays.
    ///
    /// # Example
    /// ```ignore
    /// // From any system, any thread, at any time:
    /// let budget = Engine::global()?.memory_budget();
    /// let buf    = Engine::global()?.create_buffer(desc)?;
    /// ```
    pub fn global() -> Result<&'static Engine> {
        engine_global::engine().ok_or_else(|| {
            Error::InvalidInput(
                "Engine::global() called before the engine was initialised — ensure you are inside a GameApp, HeadlessApp, or EngineApp callback.".into(),
            )
        })
    }

    /// Access the global engine, returning `None` if not yet initialised.
    ///
    /// Prefer [`Engine::global()`] in application code. Use this in library
    /// code or tests where the engine may not be set.
    pub fn try_global() -> Option<&'static Engine> {
        engine_global::engine()
    }

    /// Returns `true` if the global engine has been initialised.
    pub fn global_is_set() -> bool {
        engine_global::is_engine_set()
    }

    // ── Capabilities ──────────────────────────────────────────────────────────

    pub fn format_capabilities(&self, format: Format) -> FormatCapabilities {
        self.device.format_capabilities(format)
    }

    pub fn native_handle_capabilities(&self) -> NativeHandleCapabilities {
        self.device.native_handle_capabilities()
    }

    pub fn raw_capabilities(&self) -> BackendRawCapabilities {
        self.device.raw_capabilities()
    }

    pub fn backend_kind(&self) -> BackendKind {
        self.device.backend_kind()
    }

    pub fn adapter_name(&self) -> Option<String> {
        self.device.adapter_name()
    }

    pub fn create_image(&self, desc: ImageDesc) -> Result<Image> {
        let handle = self.device.create_image(desc)?;
        Ok(Image {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    /// Import a borrowed native image into the engine.
    ///
    /// # Safety
    ///
    /// The caller must uphold the backend-specific lifetime and compatibility
    /// requirements documented by `Device::import_external_image`.
    pub unsafe fn import_external_image(&self, desc: ExternalImageDesc) -> Result<Image> {
        let handle = unsafe { self.device.import_external_image(desc)? };
        Ok(Image {
            device: self.device.clone(),
            handle,
            desc: desc.desc,
        })
    }

    pub fn create_buffer(&self, desc: BufferDesc) -> Result<Buffer> {
        let handle = self.device.create_buffer(desc)?;
        Ok(Buffer {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    pub fn create_acceleration_structure(
        &self,
        desc: AccelerationStructureDesc,
    ) -> Result<AccelerationStructure> {
        let handle = self.device.create_acceleration_structure(desc)?;
        Ok(AccelerationStructure {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    pub fn blas_build_sizes(
        &self,
        desc: &BlasBuildDesc,
    ) -> Result<AccelerationStructureBuildSizes> {
        self.device.blas_build_sizes(desc)
    }

    pub fn tlas_build_sizes(
        &self,
        desc: &TlasBuildDesc,
    ) -> Result<AccelerationStructureBuildSizes> {
        self.device.tlas_build_sizes(desc)
    }

    pub fn acceleration_structure_device_address(
        &self,
        acceleration_structure: &AccelerationStructure,
    ) -> Result<u64> {
        acceleration_structure.device_address()
    }

    /// Import a borrowed native buffer into the engine.
    ///
    /// # Safety
    ///
    /// The caller must uphold the backend-specific lifetime and compatibility
    /// requirements documented by `Device::import_external_buffer`.
    pub unsafe fn import_external_buffer(&self, desc: ExternalBufferDesc) -> Result<Buffer> {
        let handle = unsafe { self.device.import_external_buffer(desc)? };
        Ok(Buffer {
            device: self.device.clone(),
            handle,
            desc: desc.desc,
        })
    }

    pub fn write_buffer(&self, buffer: &Buffer, offset: u64, data: &[u8]) -> Result<()> {
        self.device.write_buffer(buffer.handle, offset, data)
    }

    pub fn read_buffer(&self, buffer: &Buffer, offset: u64, out: &mut [u8]) -> Result<()> {
        self.device.read_buffer(buffer.handle, offset, out)
    }

    pub fn buffer_device_address(&self, buffer: &Buffer) -> Result<Option<u64>> {
        self.device.buffer_device_address(buffer.handle)
    }

    pub fn create_sampler(&self, desc: SamplerDesc) -> Result<Sampler> {
        let handle = self.device.create_sampler(desc)?;
        Ok(Sampler {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    pub fn create_shader(&self, desc: ShaderDesc) -> Result<Shader> {
        let handle = self.device.create_shader(desc.clone())?;
        Ok(Shader {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    pub fn load_shader(&self, path: impl Into<std::path::PathBuf>) -> Result<ShaderProgram> {
        ShaderProgram::load_fragment(self, path)
    }

    pub fn create_shader_program(&self, desc: ShaderProgramDesc) -> Result<ShaderProgram> {
        ShaderProgram::new(self, desc)
    }

    pub fn load_slang_source(
        &self,
        name: ShaderName,
        source: &'static str,
        entry_points: SlangEntryPoints,
    ) -> Result<ShaderProgram> {
        let source = ShaderSource::MemoryUtf8(source);
        match entry_points {
            SlangEntryPoints::Graphics { vertex, fragment } => {
                self.create_shader_program(ShaderProgramDesc {
                    vertex: Some(ShaderDesc {
                        source: source.clone(),
                        entry_point: vertex,
                        stage: ShaderStage::Vertex,
                        requires_ray_query: false,
                        requires_cooperative_matrix: false,
                        uses_ser: false,
                    }),
                    fragment: ShaderDesc {
                        source,
                        entry_point: fragment,
                        stage: ShaderStage::Fragment,
                        requires_ray_query: false,
                        requires_cooperative_matrix: false,
                        uses_ser: false,
                    },
                })
            }
            SlangEntryPoints::Fragment { fragment } => {
                self.create_shader_program(ShaderProgramDesc {
                    vertex: None,
                    fragment: ShaderDesc {
                        source,
                        entry_point: fragment,
                        stage: ShaderStage::Fragment,
                        requires_ray_query: false,
                        requires_cooperative_matrix: false,
                        uses_ser: false,
                    },
                })
            }
            SlangEntryPoints::Compute { compute } => {
                self.create_shader_program(ShaderProgramDesc {
                    vertex: None,
                    fragment: ShaderDesc {
                        source,
                        entry_point: compute,
                        stage: ShaderStage::Compute,
                        requires_ray_query: false,
                        requires_cooperative_matrix: false,
                        uses_ser: false,
                    },
                })
            }
        }
        .map_err(|error| {
            Error::Unknown(format!(
                "failed to load shader '{}': {error}",
                name.as_str()
            ))
        })
    }

    pub fn begin_render_frame(&self) -> Result<RenderFrame> {
        RenderFrame::new(self.clone(), 0)
    }

    /// Render into `image` using a closure, then block until the GPU finishes.
    ///
    /// This is a synchronous blocking convenience for offline rendering, screenshots,
    /// thumbnails, and test fixtures — not for the real-time render loop. The closure
    /// receives a `&RenderFrame` for recording passes; the frame is flushed and
    /// GPU-waited before `render_image` returns.
    pub fn render_image(
        &self,
        image: &Image,
        render: impl FnOnce(&RenderFrame) -> Result<()>,
    ) -> Result<()> {
        let frame = self.begin_render_frame()?;
        frame.import_image("render_target", image)?;
        render(&frame)?;
        frame.flush()?;
        frame.wait()?;
        Ok(())
    }

    /// Begin a render frame whose per-frame graph image cache is keyed by the
    /// given swapchain image. Use this instead of `begin_render_frame` when
    /// rendering to a swapchain so that intermediate images (e.g. `scene_color`)
    /// get separate GPU allocations for each swapchain slot, preventing races
    /// between frames in flight.
    pub fn begin_render_frame_for(&self, surface_image: &SurfaceImage) -> Result<RenderFrame> {
        RenderFrame::new(self.clone(), surface_image.slot)
    }

    /// Acquire the next swapchain image and begin a render frame tied to it.
    ///
    /// The returned frame is configured for zero-wait presentation: when the frame
    /// is dropped (or `finish_and_present()` is called), it flushes all queued GPU
    /// work and presents without a CPU fence wait. The fence is waited at the start
    /// of the *next* frame's submission, allowing CPU/GPU overlap across frames.
    ///
    /// Returns the frame and the registered swapchain [`GraphImage`] ready for use
    /// as a render target.
    pub fn begin_frame_for_surface(&self, surface: &Surface) -> Result<(RenderFrame, GraphImage)> {
        let surface_image = surface.acquire_image()?;
        let frame = self.begin_render_frame_for(&surface_image)?;
        let (device, handle) = surface.auto_present_info();
        frame.configure_auto_present(device, handle);
        let swapchain = frame.swapchain_image(&surface_image)?;
        frame.hold_surface_image(surface_image);
        Ok((frame, swapchain))
    }

    pub fn evict_cached_graph_images_with_prefix(&self, name_prefix: &str) {
        let mut cache = self
            .graph_image_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|key, _| !key.name().starts_with(name_prefix));
    }

    pub(crate) fn cached_graph_image(
        &self,
        key: GraphImageCacheKey,
        desc: ImageDesc,
    ) -> Result<(core::ImageHandle, ImageDesc)> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut cache = self
            .graph_image_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(image) = cache.get(&key) {
            return Ok((image.handle(), image.desc()));
        }

        // Evict any stale entry that has the same name+slot but a different
        // descriptor (e.g. after a swapchain resize changed the image dimensions).
        cache.retain(|k, _| !k.is_stale_for(&key));

        let image = self.create_image(desc)?;
        if let Some(name) = key.debug_name() {
            let _ = image.set_debug_name(&name);
        }
        let handle = image.handle();
        let desc = image.desc();
        cache.insert(key, image);
        Ok((handle, desc))
    }

    pub fn shader_reflection(&self, shader: &Shader) -> Result<ShaderReflection> {
        self.device.shader_reflection(shader.handle())
    }

    pub fn create_reflected_compute_pipeline_layout(
        &self,
        shader: &Shader,
    ) -> Result<PipelineLayout> {
        let layout = self
            .device
            .reflected_compute_pipeline_layout(shader.handle())?;
        self.create_pipeline_layout(layout)
    }

    pub fn create_reflected_graphics_pipeline_layout(
        &self,
        vertex_shader: &Shader,
        fragment_shader: Option<&Shader>,
    ) -> Result<PipelineLayout> {
        let layout = self.device.reflected_graphics_pipeline_layout(
            vertex_shader.handle(),
            fragment_shader.map(Shader::handle),
        )?;
        self.create_pipeline_layout(layout)
    }

    pub fn graphics_shader_reflection(
        &self,
        vertex_shader: &Shader,
        fragment_shader: Option<&Shader>,
    ) -> Result<ShaderReflection> {
        self.device.reflected_graphics_pipeline_reflection(
            vertex_shader.handle(),
            fragment_shader.map(Shader::handle),
        )
    }

    pub fn create_bind_group(&self, desc: BindGroupDesc) -> Result<BindGroup> {
        let handle = self.device.create_bind_group(desc.clone())?;
        Ok(BindGroup {
            device: self.device.clone(),
            handle,
            desc,
        })
    }

    pub fn create_pipeline_layout(
        &self,
        layout: CanonicalPipelineLayout,
    ) -> Result<PipelineLayout> {
        let handle = self.device.create_pipeline_layout(layout.clone())?;
        Ok(PipelineLayout {
            device: self.device.clone(),
            handle,
            layout,
        })
    }

    pub fn create_compute_pipeline(&self, desc: ComputePipelineDesc) -> Result<Pipeline> {
        let handle = self.device.create_compute_pipeline(desc)?;
        Ok(Pipeline {
            device: self.device.clone(),
            handle,
        })
    }

    pub fn create_graphics_pipeline(&self, desc: GraphicsPipelineDesc) -> Result<Pipeline> {
        let handle = self.device.create_graphics_pipeline(desc)?;
        Ok(Pipeline {
            device: self.device.clone(),
            handle,
        })
    }

    pub fn create_ray_tracing_pipeline(&self, desc: RayTracingPipelineDesc) -> Result<Pipeline> {
        let handle = self.device.create_ray_tracing_pipeline(desc)?;
        Ok(Pipeline {
            device: self.device.clone(),
            handle,
        })
    }

    pub fn create_shader_binding_table(
        &self,
        desc: ShaderBindingTableDesc,
    ) -> Result<RayTracingShaderBindingTable> {
        let table = self.device.create_shader_binding_table(desc)?;
        Ok(RayTracingShaderBindingTable {
            device: self.device.clone(),
            table,
        })
    }

    pub fn begin_frame(&self) -> Result<Frame> {
        Ok(Frame {
            engine: self.clone(),
            inner: self.device.begin_frame()?,
            upload_arena: UploadArena::default(),
        })
    }

    /// Begin a new image-centric graph frame.
    pub fn begin_graph_frame(&self) -> Result<GraphFrame> {
        let frame = self.begin_frame()?;
        Ok(GraphFrame::new(self.clone(), frame))
    }

    /// Generate a 2-D texture from a CPU pixel function, upload it, and return the image.
    ///
    /// `fill` receives `(x, y)` for every pixel and returns `[r, g, b, a]` as `u8`.
    /// The texture is created, uploaded, and the GPU work is submitted synchronously
    /// before this call returns.  Use this for one-time assets such as noise maps,
    /// gradient ramps, lookup tables, and debug patterns.
    ///
    /// The returned [`Image`] is sampled as `Rgba8Unorm` and ready to use as a
    /// shader input in subsequent frames.
    pub fn generate_texture_2d(
        &self,
        name: impl Into<String>,
        width: u32,
        height: u32,
        fill: impl Fn(u32, u32) -> [u8; 4],
    ) -> Result<Image> {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let rgba = fill(x, y);
                let i = ((y * width + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&rgba);
            }
        }
        let name = name.into();
        let mut frame = self.begin_frame()?;
        let image = frame.upload_texture_2d(
            &name,
            crate::TextureUploadDesc::sampled_rgba8(width, height),
            &pixels,
        )?;
        let _ = image.set_debug_name(&format!("procedural-{name}"));
        frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
        frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
        Ok(image)
    }

    /// Load a texture from `path` and upload it to the GPU.
    ///
    /// **Non-blocking.** Returns an [`AssetHandle`] immediately in the `Loading`
    /// state. Decoding and block compression happen on a rayon worker thread;
    /// the GPU upload is performed the next time [`drain_pending_uploads`] runs
    /// (automatically at frame-start in all shell entry points).
    ///
    /// The handle transitions to `Ready` after the first frame that drains
    /// uploads, or to `Failed` if the file is unreadable or corrupt — no panic.
    ///
    /// ```ignore
    /// let tex = engine.load_texture_2d("assets/rock.png");
    /// // In render — use a checkerboard fallback until Ready:
    /// let img = tex.with(|i| i.handle()).unwrap_or_else(|| fallback.handle());
    /// frame.bind_image_handle("albedo", img);
    /// ```
    ///
    /// [`drain_pending_uploads`]: Engine::drain_pending_uploads
    pub fn load_texture_2d(&self, path: impl AsRef<std::path::Path>) -> AssetHandle<Image> {
        let key = asset_loader::canonical(&path);
        // Return the existing handle if this path was already requested.
        // The handle may still be `Loading` if the worker hasn't finished;
        // the caller waits via `with()` or polls `is_ready()` each frame.
        {
            //panic allowed, reason = "poisoned internal texture cache is unrecoverable"
            let cache = self
                .texture_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(handle) = cache.get(&key) {
                return handle.clone();
            }
        }
        let handle = asset_loader::load_texture_2d_async(&path, self.pending_uploads.clone());
        //panic allowed, reason = "poisoned internal texture cache is unrecoverable"
        self.texture_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, handle.clone());
        handle
    }

    /// Load an HDR or EXR texture in the background.
    ///
    /// **Non-blocking.** Returns an `AssetHandle<Image>` in the `Loading` state
    /// immediately. The file is decoded on a rayon worker thread; the GPU upload
    /// happens at the next [`drain_pending_uploads`] call (frame-start in all shells).
    ///
    /// Output format is `Rgba16Float`. Use `load_hdr_texture_32f_async` for full
    /// 32-bit precision.
    ///
    /// [`drain_pending_uploads`]: Engine::drain_pending_uploads
    pub fn load_hdr_texture_async(&self, path: impl AsRef<std::path::Path>) -> AssetHandle<Image> {
        asset_loader::load_hdr_texture_async(path, self.pending_uploads.clone(), false)
    }

    /// Load an HDR or EXR texture in the background at full 32-bit float precision.
    pub fn load_hdr_texture_32f_async(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> AssetHandle<Image> {
        asset_loader::load_hdr_texture_async(path, self.pending_uploads.clone(), true)
    }

    /// Upload all textures that background workers have finished decoding.
    ///
    /// Called automatically at frame-start by every shell entry point
    /// (`run_game`, `run_headless`, `try_run`). Only call this manually when
    /// using a custom render loop that bypasses the built-in shells.
    ///
    /// Performs a single GPU submission for all pending textures; no-ops if
    /// the queue is empty.
    pub fn drain_pending_uploads(&self) -> Result<()> {
        // Warn once per 5 seconds when VRAM pressure exceeds 80 %.
        if let Some(budget) = self.memory_budget() {
            if budget.over_budget() {
                use std::sync::atomic::Ordering;
                use std::time::{SystemTime, UNIX_EPOCH};
                let now_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let last = self.last_budget_warn_secs.load(Ordering::Relaxed);
                if now_secs.saturating_sub(last) >= 5 {
                    self.last_budget_warn_secs
                        .store(now_secs, Ordering::Relaxed);
                    tracing::warn!("VRAM pressure: {}", budget.summary());
                }
            }
        }

        let uploads: Vec<asset_loader::PendingUpload> = {
            //panic allowed, reason = "poisoned internal pending upload queue is unrecoverable"
            let mut lock = self
                .pending_uploads
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *lock)
        };
        if uploads.is_empty() {
            return Ok(());
        }

        let mut frame = self.begin_frame()?;
        let mut results: Vec<Result<Image>> = Vec::with_capacity(uploads.len());
        for u in &uploads {
            let desc = TextureUploadDesc {
                width: u.width,
                height: u.height,
                format: u.format,
                usage: ImageUsage::SAMPLED,
                prefer_compressed: true,
            };
            results.push(frame.upload_texture_2d(&u.name, desc, &u.data));
        }
        frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
        frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;

        for (upload, result) in uploads.into_iter().zip(results) {
            match result {
                Ok(image) => upload.handle.set_ready(image),
                Err(e) => upload.handle.set_failed(e.to_string()),
            }
        }
        Ok(())
    }

    /// Synchronous variant of `load_texture_2d` that returns `Result<Image>` directly.
    ///
    /// Blocks until the image is loaded and uploaded to the GPU. Use this in
    /// hot-reload callbacks where you need the new image before the next frame.
    pub fn load_texture_2d_blocking(&self, path: impl AsRef<std::path::Path>) -> Result<Image> {
        let path = path.as_ref();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("texture");
        asset_loader::load_and_upload_blocking(self, path, name)
    }

    /// Load a 3D mesh file and upload all primitives to the GPU.
    ///
    /// Dispatches automatically by file extension:
    ///
    /// | Extension        | Format             |
    /// |-----------------|---------------------|
    /// | `.gltf` `.glb`  | GLTF 2.0 (primary) |
    /// | `.obj`          | Wavefront OBJ + MTL |
    /// | `.stl`          | STereoLithography   |
    ///
    /// Returns a flat `Vec<MeshPrimitive>` — one per draw-call-worth of geometry.
    ///
    /// ```ignore
    /// for prim in engine.load_mesh("assets/helmet.glb")? {
    ///     let id = scene.add_mesh(prim.mesh, MeshProgram::lit(&engine)?);
    ///     scene.set_material(id, prim.material_params.to_material_descriptor());
    /// }
    /// ```
    pub fn load_mesh(&self, path: impl AsRef<std::path::Path>) -> Result<Vec<MeshPrimitive>> {
        mesh_loader::load_mesh_from_path(self, path.as_ref())
    }

    /// Load an HDR or EXR image as a linear-float `Rgba16Float` GPU texture.
    ///
    /// Supports `.hdr` (RGBE radiance) and `.exr` (OpenEXR). Suitable for
    /// environment maps and IBL prefiltering. Use `load_hdr_texture_32f` for
    /// full 32-bit precision.
    pub fn load_hdr_texture(&self, path: impl AsRef<std::path::Path>) -> Result<Image> {
        asset_loader::load_hdr_texture_from_path(self, path)
    }

    /// Load an HDR or EXR image as a full 32-bit float `Rgba32Float` GPU texture.
    pub fn load_hdr_texture_32f(&self, path: impl AsRef<std::path::Path>) -> Result<Image> {
        asset_loader::load_hdr_texture_32f_from_path(self, path)
    }

    /// Generate a magenta/dark-grey checkerboard image for use as a
    /// missing-texture placeholder.
    ///
    /// `size` is the image side length in pixels (rounded up to the next
    /// power of two). `tile_size` controls the size of each coloured square.
    ///
    /// The vivid magenta pattern is immediately recognisable as a placeholder
    /// in a rendered scene, making missing asset bugs obvious at a glance.
    pub fn checkerboard_texture(&self, size: u32, tile_size: u32) -> Result<Image> {
        asset_loader::make_checkerboard(self, size, tile_size)
    }

    // ── Frame-time diagnostics (Track LL) ────────────────────────────────────

    /// Return the most recent frame timing snapshot, or `None` before the first frame.
    ///
    /// Updated once per frame by the runtime shell. Contains CPU frame time,
    /// GPU frame time, and rolling P95/P99 jitter metrics over 128 frames.
    ///
    /// **Callable from any thread at any time** — reads a Mutex without blocking
    /// (uncontended in normal use: the runtime only writes once per frame).
    ///
    /// ```ignore
    /// if let Ok(engine) = Engine::global() {
    /// if let Some(t) = engine.frame_timing() {
    ///     if t.is_jittery() {
    ///         tracing::warn!("jitter! p99={:.1}ms mean={:.1}ms", t.p99_cpu_ms, t.mean_cpu_ms);
    ///     }
    /// }
    /// ```
    pub fn frame_timing(&self) -> Option<crate::FrameTimingReport> {
        engine_global::frame_timing()
    }

    // ── Bindless descriptor heap (Track 8a) ───────────────────────────────────

    /// Returns `true` when the GPU supports the bindless descriptor heap.
    ///
    /// Requires `VK_EXT_descriptor_indexing` with runtime descriptor arrays.
    /// All discrete GPUs since ~2016 and most mobile GPUs since 2020 support this.
    pub fn bindless_supported(&self) -> bool {
        self.device.bindless_supported()
    }

    /// Register a 2-D texture (sampled image) in the global bindless heap.
    ///
    /// Returns a `BindlessHandle<Image>` whose `.index()` can be embedded in
    /// push constants or a per-draw data buffer for use in any shader that
    /// includes `bindless.slang`.
    ///
    /// The handle is valid for the lifetime of the `Image` (and the engine).
    /// The caller must ensure the image outlives any GPU work that uses the index.
    ///
    /// Returns `None` when bindless is not supported or the heap is full.
    pub fn register_bindless_image(&self, image: &Image) -> Option<BindlessHandle<Image>> {
        self.device
            .register_bindless_sampled_image(image.handle())
            .map(BindlessHandle::from_raw)
    }

    /// Register a sampler in the global bindless heap.
    pub fn register_bindless_sampler(&self, sampler: &Sampler) -> Option<BindlessHandle<Sampler>> {
        self.device
            .register_bindless_sampler(sampler.handle())
            .map(BindlessHandle::from_raw)
    }

    /// Register a storage (read-write) image in the global bindless heap.
    pub fn register_bindless_storage_image(&self, image: &Image) -> Option<BindlessHandle<Image>> {
        self.device
            .register_bindless_storage_image(image.handle())
            .map(BindlessHandle::from_raw)
    }

    /// Register a storage buffer in the global bindless heap.
    pub fn register_bindless_storage_buffer(
        &self,
        buffer: &Buffer,
    ) -> Option<BindlessHandle<Buffer>> {
        self.device
            .register_bindless_storage_buffer(buffer.handle())
            .map(BindlessHandle::from_raw)
    }

    pub fn wait_idle(&self) -> Result<()> {
        self.device.wait_idle()
    }

    pub fn supported_gpu_capture_tools(&self) -> Vec<GpuCaptureTool> {
        self.device.supported_gpu_capture_tools()
    }

    pub fn begin_gpu_capture(&self, desc: &GpuCaptureDesc) -> Result<()> {
        self.device.begin_gpu_capture(desc)
    }

    pub fn end_gpu_capture(&self, tool: GpuCaptureTool) -> Result<()> {
        self.device.end_gpu_capture(tool)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_surface(&self, desc: NativeSurfaceDesc) -> Result<Surface> {
        let handle = self.device.create_surface(desc)?;
        let info = self.device.surface_info(handle)?;
        Ok(Surface {
            device: self.device.clone(),
            handle,
            info,
        })
    }

    /// Create a surface from any window that provides raw handles.
    ///
    /// Handles extraction, `.as_raw()`, error mapping, and size clamping so
    /// callers never need to import `raw_window_handle` directly or write
    /// unsafe handle-lifetime casts.
    ///
    /// ```ignore
    /// let surface = engine.create_surface_for_window(&window, SurfaceSize { width: 1280, height: 720 })?;
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_surface_for_window(
        &self,
        window: &(impl raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle),
        size: SurfaceSize,
    ) -> Result<Surface> {
        self.create_surface_for_window_with_hdr(window, size, SurfaceHdrPreference::Sdr)
    }

    /// Create a surface from a window and request a specific HDR preference.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn create_surface_for_window_with_hdr(
        &self,
        window: &(impl raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle),
        size: SurfaceSize,
        hdr: SurfaceHdrPreference,
    ) -> Result<Surface> {
        let display = window
            .display_handle()
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        let window_handle = window
            .window_handle()
            .map_err(|e| Error::InvalidInput(e.to_string()))?;
        let raw_display = display.as_raw();
        let raw_window = window_handle.as_raw();
        let mut desc = NativeSurfaceDesc::new(
            raw_display,
            raw_window,
            SurfaceSize {
                width: size.width.max(1),
                height: size.height.max(1),
            },
        );
        desc.hdr = hdr;
        self.create_surface(desc)
    }
}
