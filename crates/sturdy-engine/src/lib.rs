//! Ergonomic Rust API for Sturdy Engine.
//!
//! Use this crate from Rust applications. It wraps the core handle-oriented API
//! with RAII resource types and builder-style descriptors while keeping the
//! lower-level `sturdy-engine-core` crate available for engine internals.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub mod animation;
mod anti_aliasing_pass;
mod antialiasing;
mod ao_pass;
mod application;
mod asset_loader;
mod asset_watcher;
mod bind_group;
mod bindless;
mod bloom_pass;
mod compute_program;
mod debug_draw_2d;
mod debug_overlay;
mod debug_view_picker;
mod deferred_pass;
mod device_manager;
pub mod ecs;
mod environment_map;
mod frame_clock;
mod frontend_graph;
mod game_shell;
mod geometry;
mod gltf_animation;
mod gltf_loader;
mod gpu_procedural_texture;
mod graph_frame;
pub mod graph_report;
mod hdr_pipeline;
mod headless;
mod hiz_pass;
mod input;
mod light_bvh;
mod mesh;
mod mesh_loader;
mod mesh_program;
mod mip_pyramid;
mod motion_vector_debug;
mod obj_loader;
mod oit_pass;
mod pipeline_layout;
mod plot2d;
mod point_shadow_pass;
mod post_process;
mod procedural_texture;
mod quad_batch;
mod runtime;
mod sampler_catalog;
mod scene;
mod screenshot;
mod shader_playground;
pub mod shader_program;
mod shader_watcher;
mod shadow_pass;
mod spot_shadow_pass;
mod sprite_batch;
mod stl_loader;
#[cfg(test)]
mod tests;
mod text_draw;
mod text_engine;
mod text_overlay;
mod text_tiling;
mod texture;
mod texture_compression;
mod ui_renderer;
mod upload_arena;
mod window_registry;

pub use animation::{
    AnimationChannel, AnimationClip, AnimationPlayer, AnimationProperty, GltfSkin, Interpolation,
};
pub use anti_aliasing_pass::{AntiAliasingPass, taa_jitter_uv, taa_jittered_projection};
pub use antialiasing::{
    AntiAliasingConfig, AntiAliasingDial, AntiAliasingMode, FxaaSettings, MsaaSettings, TaaSettings,
};
pub use ao_pass::{AoConfig, AoMode, AoPass};
#[cfg(not(target_arch = "wasm32"))]
pub use application::{
    EngineApp, MotionVectorLayer, MotionVectorSpace, RuntimeMotionVectorDesc,
    RuntimePostProcessDesc, RuntimePostProcessOutput, ShellFrame, WindowConfig, WindowDesc, run,
    try_run,
};
pub use asset_loader::{AssetCache, AssetHandle, LoadState};
pub use asset_watcher::{AssetReloadDiagnostic, AssetWatcher};
pub use bloom_pass::{
    BloomCompositeConstants, BloomConfig, BloomPass, BrightPassConstants, DownsampleConstants,
    UpsampleConstants,
};
pub use clay_ui::{
    ClipSpace, Ndc, RenderTargetPx, SurfacePx, TexelPx, UiPx, Uv01, WindowLogicalPx,
    WindowPhysicalPx, WorldSpace, logical_to_physical, physical_to_logical, render_target_to_uv,
    surface_to_ndc, ui_to_surface, window_logical_to_surface, window_logical_to_ui,
};
pub use compute_program::ComputeProgram;
pub use debug_draw_2d::{DebugDraw2d, DebugDrawStyle};
pub use debug_overlay::{
    DebugHitRegion, DebugOverlay, DebugOverlayAntialiasing, DebugOverlayConfig,
    DebugOverlayRenderer, DebugOverlayTransform,
};
pub use debug_view_picker::DebugViewPicker;
pub use deferred_pass::{DeferredPass, RenderPath, SkyConfig};
pub use device_manager::{AdapterEntry, DeviceManager};
pub use ecs::{
    Acceleration,
    // Built-in components
    Active,
    // Parallel scheduling
    CompiledSchedule,
    Component,
    ComponentReadGuard,
    ComponentWriteGuard,
    // Core types
    Entity,
    EntityBuilder,
    Health,
    LocalTransform,
    Name,
    ParallelSystem,
    SceneLink,
    Schedule,
    System,
    SystemAccess,
    SystemFn,
    Transform,
    Velocity,
    World,
    WorldCommands,
    WorldView,
    // Built-in systems
    despawn_dead,
    integrate_transforms,
    propagate_local_transforms,
    run_once,
};
pub use environment_map::EnvironmentMap;
pub use frame_clock::{FrameClock, FrameTime};
#[cfg(not(target_arch = "wasm32"))]
pub use game_shell::{
    FixedUpdateContext, GameApp, GameConfig, GameContext, run_game, try_run_game,
};
pub use geometry::{
    BoundingSphere, DispatchIndirectCommand, DrawIndexedIndirectCommand, DrawIndirectCommand,
    DrawMeshTasksIndirectCommand, Frustum, GeometryBackend, GeometryRendererCaps, HizDesc,
    MAX_MESHLET_TRIANGLES, MAX_MESHLET_VERTICES, Meshlet, MeshletBounds, MeshletGroup, SubMesh,
    VirtualMesh, VirtualMeshBuilder, VirtualMeshProxy,
};
pub use gltf_animation::{
    load_animations, load_skinned_vertices, load_skins, load_skins_and_animations,
};
pub use gpu_procedural_texture::GpuProceduralTexture;
pub use graph_frame::{FullscreenPassBuilder, GraphFrame, ImageNode};
pub use hdr_pipeline::{HdrMode, HdrPipelineDesc, HdrPreference, ToneMappingOp};
pub use headless::{HeadlessApp, render_to_rgba8, render_to_rgba8_with_engine, run_headless};
pub use hiz_pass::{HizConfig, HizHistory, HizHistoryFrame, HizPass, HizPyramid};
pub use input::{
    ActionAxisDirection, ActionBinding, ActionBindingRegistry, ActionMap, BindingChange,
    GamepadAxis, GamepadAxisInput, GamepadButton, GamepadButtonInput, GamepadId, InputHub,
    KeyInput, KeyInputState, KeyModifier, KeyModifiers, KeyToken, Keybind, KeybindCapture,
};
pub use light_bvh::{BVH_EMPTY, GpuBvhNode, LEAF_FLAG, LightBvhBuilder};
pub use mesh::{Mesh, SkinnedVertex3d, Vertex2d, Vertex3d};
pub use mesh_loader::{MeshAlphaMode, MeshMaterialParams, MeshPrimitive, MeshTextures};
pub use mesh_program::{MeshProgram, MeshProgramDesc, MeshVertexKind};
pub use mip_pyramid::MipPyramid;
pub use motion_vector_debug::MotionVectorDebugPass;
pub use oit_pass::{OitConfig, OitPass};
pub use plot2d::{Plot2d, PlotBar, PlotInspection, PlotRange, PlotScale, PlotTheme, PlotView};
pub use point_shadow_pass::{
    GpuPointShadowData, MAX_POINT_SHADOWS, PointShadowConfig, PointShadowPass,
};
pub use procedural_texture::{
    CpuProceduralTexture2d, ProceduralTextureRecipe, ProceduralTextureUpdatePolicy,
};
pub use quad_batch::QuadBatch;
pub use runtime::{
    AppLayer, AppRuntime, AppRuntimeFrame, AssetDiagnostic, AssetState, DebugImageRegistry,
    DefaultSceneTargetConfig, FrameTimingReport, RuntimeApplyNotification, RuntimeApplyPath,
    RuntimeApplyReport, RuntimeChangeResult, RuntimeController, RuntimeDiagnostics,
    RuntimeGraphDiagnostics, RuntimePassTiming, RuntimeSettingChange, RuntimeSettingDescriptor,
    RuntimeSettingEntry, RuntimeSettingId, RuntimeSettingKey, RuntimeSettingOption,
    RuntimeSettingSource, RuntimeSettingSupport, RuntimeSettingValue, RuntimeSettingsSnapshot,
    RuntimeSettingsTransaction, RuntimeTimingSummary, RuntimeUserDiagnostic,
    RuntimeWindowDiagnostics, SceneRenderContext, ShaderCompileError, UiContext, WindowMode,
};
pub use sampler_catalog::SamplerPreset;
pub use scene::{
    CameraConstants, CameraId, CameraOutput, DirectionalLight, DiskLight, GpuInstanceData,
    InstanceData, MaterialDescriptor, MaterialDomain, MaterialExpr, MaterialInput, MeshId,
    ObjectId, ObjectKind, OrbitCamera, PointLight, RectLight, RenderState, RenderTarget, Scene,
    SceneCamera, SceneCommands, SceneView, ShadingModel, SphereLight, SpotLight, UnifiedMaterial,
    UnifiedMaterialBuilder, UvSource, gbuffer,
};
pub use screenshot::{ScreenshotCapture, ScreenshotExportReport};
pub use shader_playground::{PlaygroundParam, PlaygroundPreset, PlaygroundValue, ShaderPlayground};
pub use shader_watcher::{Reloadable, ShaderReloadDiagnostic, ShaderWatcher};
pub use shadow_pass::{
    CsmConfig, CsmOutput, CsmPass, GpuCsmData, MAX_CASCADES, ShadowConfig, ShadowOutput, ShadowPass,
};
pub use spot_shadow_pass::{GpuSpotShadowData, MAX_SPOT_SHADOWS, SpotShadowConfig, SpotShadowPass};
pub use sprite_batch::{Sprite, SpriteBatch, SpriteRenderer};
pub use sturdy_engine_core::{PcFieldKind, PushConstantField};
pub use text_draw::{
    TextAtlasContentMode, TextAtlasPage, TextDrawDesc, TextGlyphQuad, TextLayoutOutput,
    TextPlacement, TextRenderer, TextScene, TextSceneQuad, TextTypography,
};
pub use text_engine::{
    PreparedTextDraw, PreparedTextQuad, TextEngine, TextEngineFrame, TextUiRenderer,
};
pub use text_overlay::TextOverlay;
pub use text_tiling::{TiledTextAtlasPage, TiledTextEngineFrame};

pub use bind_group::BindGroupBuilder;
pub use bindless::BindlessHandle;
pub use frontend_graph::{
    GraphImage, GraphImageCacheKey, GraphImageHistory, GraphImageHistoryFrame, GraphImageView,
    RenderFrame, ShaderPassIntent,
};
pub use glam::{Vec2, Vec3};
pub use graph_report::{
    DiagnosticLevel, GraphDiagnostic, GraphImageInfo, GraphPassInfo, GraphReport, PassKind,
};
pub use pipeline_layout::PipelineLayoutBuilder;
pub use post_process::{
    AutoExposureConfig, CaConfig, CaPass, GrainConfig, GrainPass, LensConfig, PostProcessConfig,
    PostProcessPasses, VignetteConfig, VignettePass,
};
pub use shader_program::{ShaderName, ShaderProgram, ShaderProgramDesc, SlangEntryPoints};
#[cfg(not(target_arch = "wasm32"))]
pub use sturdy_engine_core::NativeSurfaceDesc;
pub use sturdy_engine_core::ShaderReflection;
pub use sturdy_engine_core::{
    Access, AccelerationStructureDesc, AccelerationStructureKind, AdapterInfo, AdapterKind,
    AdapterSelection, AddressMode, BackendKind, BackendRawCapabilities, BindGroupDesc,
    BindGroupEntry, BindingKind, BlendMode, BorderColor, BufferDesc, BufferUsage, BufferUse,
    CanonicalBinding, CanonicalGroupLayout, CanonicalPipelineLayout, Caps, ColorTargetDesc,
    CompareOp, CompiledShaderArtifact, ComputePipelineDesc, CopyBufferToImageDesc,
    CopyImageToBufferDesc, CullMode, D3d12RawCapabilities, DispatchDesc, DispatchIndirectDesc,
    DrawDesc, DrawIndirectCountDesc, DrawIndirectDesc, DrawMeshShaderDesc,
    DrawMeshShaderIndirectDesc, Error, ErrorCategory, Extent3d, ExternalBufferDesc,
    ExternalBufferHandle, ExternalImageDesc, ExternalImageHandle, FilterMode, Format,
    FormatCapabilities, FrontFace, GpuCaptureDesc, GpuCaptureTool, GpuMemoryBudget,
    GraphicsPipelineDesc, ImageBuilder, ImageDesc, ImageDimension, ImageRole, ImageUsage, ImageUse,
    IndexBufferBinding, IndexFormat, MetalRawCapabilities, MipmapMode, NativeHandleCapabilities,
    NativeHandleCapability, NativeHandleKind, NativeHandleOwnership, PassDesc, PassWork,
    PrimitiveTopology, PushConstants, QueueType, RasterState, ResolveImageDesc, ResourceBinding,
    Result, RgState, SamplerDesc, ShaderDesc, ShaderParameterKind, ShaderParameterReflection,
    ShaderResourceAccess, ShaderSource, ShaderStage, ShaderTarget, ShadingRate, SlangCompileDesc,
    StageMask, SubresourceRange, SurfaceCapabilities, SurfaceColorSpace, SurfaceEvent,
    SurfaceFormatInfo, SurfaceHdrCaps, SurfaceHdrPreference, SurfaceInfo, SurfacePresentMode,
    SurfaceRecreateDesc, UpdateRate, VertexAttributeDesc, VertexBufferBinding, VertexBufferLayout,
    VertexFormat, VertexInputRate, VertexInputReflection, VulkanExternalBuffer,
    VulkanExternalImage, VulkanRawCapabilities, compile_slang, compile_slang_to_file,
    compile_slang_to_spirv, native_handle_capabilities_for_backend, spirv_words_from_bytes,
};
pub use sturdy_engine_core::{
    AccelerationStructureHandle, DeviceDesc, DeviceFeature, ImageHandle, SamplerHandle,
    SubmissionHandle, SurfaceHandle, SurfaceSize,
};
pub use sturdy_engine_macros::push_constants;
pub use sturdy_engine_platform as platform;
pub use sturdy_engine_platform::{
    NativeWindowAppearanceApplyReport, NativeWindowAppearanceError, NativeWindowAppearanceStatus,
    PlatformCapabilityState, PlatformKind, SurfaceTransparency, WindowAppearance,
    WindowAppearanceCaps, WindowAppearancePreset, WindowBackdrop, WindowBlurDesc,
    WindowCornerStyle, WindowEffectQuality, WindowEffectRegion, WindowMaterialKind,
    WindowMaterialSupport, WindowShadowMode, WindowTransparencyDesc, appearance_wants_native_blur,
    apply_native_window_appearance, apply_native_window_appearance_for_window,
    apply_native_window_appearance_report_for_window, current_platform,
    current_window_appearance_caps, native_window_appearance_protocol, requested_backdrop_name,
};
pub use texture::{ImageCopyRegion, TextureUploadDesc};
pub use texture_compression::{CompressedTexture, TextureKind, compress_texture};
pub use ui_renderer::UiRenderer;
pub use window_registry::{WindowHandle, WindowId, WindowRegistry};

use sturdy_engine_core as core;
use upload_arena::UploadArena;

// ── Engine global ─────────────────────────────────────────────────────────────

// Process-wide engine singleton, set once at shell startup.
// All fields of Engine are Arc-backed, so clone is O(1) reference-count bumps.
static GLOBAL_ENGINE: std::sync::OnceLock<Engine> = std::sync::OnceLock::new();

// Latest frame-timing report, updated each frame by the runtime shell.
// Stored as a Mutex<Option<...>> so Engine::frame_timing() can read it from any thread.
static FRAME_TIMING: std::sync::OnceLock<std::sync::Mutex<Option<crate::FrameTimingReport>>> =
    std::sync::OnceLock::new();

fn frame_timing_cell() -> &'static std::sync::Mutex<Option<crate::FrameTimingReport>> {
    FRAME_TIMING.get_or_init(|| std::sync::Mutex::new(None))
}

/// Called by the runtime shell at the end of each frame to publish timing data.
pub(crate) fn set_global_frame_timing(report: crate::FrameTimingReport) {
    *frame_timing_cell()
        .lock()
        .expect("frame timing mutex poisoned") = Some(report);
}

// Compile-time proof that Engine is safe to share across threads.
// If this fails, a field was added that is not Send + Sync.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<Engine>();
};

// ── Engine ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Engine {
    device: core::Device,
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
            .prefer_feature(core::DeviceFeature::MeshShading);
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
    ///     eprintln!("{}", b.summary()); // "VRAM 423 / 512 MiB (82 %) [over budget]"
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
        let _ = GLOBAL_ENGINE.set(engine.clone());
    }

    /// Access the process-global engine from **any thread at any time**.
    ///
    /// Returns a `&'static Engine` with zero overhead — no locking, no
    /// allocation, no indirection beyond what `Engine` already pays.
    ///
    /// # Panics
    /// Panics if called before any shell entry point has run. In normal usage
    /// this never happens: the global is set before the first `init` or
    /// `render` callback.
    ///
    /// # Example
    /// ```ignore
    /// // From any system, any thread, at any time:
    /// let budget = Engine::global().memory_budget();
    /// let buf    = Engine::global().create_buffer(desc)?;
    /// ```
    pub fn global() -> &'static Engine {
        //panic allowed, reason = "explicit global-engine accessor requires shell initialization by contract"
        GLOBAL_ENGINE.get().expect(
            "Engine::global() called before the engine was initialised — \
             ensure you are inside a GameApp, HeadlessApp, or EngineApp callback.",
        )
    }

    /// Access the global engine, returning `None` if not yet initialised.
    ///
    /// Prefer [`Engine::global()`] in application code. Use this in library
    /// code or tests where the engine may not be set.
    pub fn try_global() -> Option<&'static Engine> {
        GLOBAL_ENGINE.get()
    }

    /// Returns `true` if the global engine has been initialised.
    pub fn global_is_set() -> bool {
        GLOBAL_ENGINE.get().is_some()
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
                    }),
                    fragment: ShaderDesc {
                        source,
                        entry_point: fragment,
                        stage: ShaderStage::Fragment,
            requires_ray_query: false,
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

    pub(crate) fn cached_graph_image(
        &self,
        key: GraphImageCacheKey,
        desc: ImageDesc,
    ) -> Result<(core::ImageHandle, ImageDesc)> {
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut cache = self
            .graph_image_cache
            .lock()
            .expect("graph image cache mutex poisoned");
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
                .expect("texture_cache mutex poisoned");
            if let Some(handle) = cache.get(&key) {
                return handle.clone();
            }
        }
        let handle = asset_loader::load_texture_2d_async(&path, self.pending_uploads.clone());
        //panic allowed, reason = "poisoned internal texture cache is unrecoverable"
        self.texture_cache
            .lock()
            .expect("texture_cache mutex poisoned")
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
                    eprintln!("[SturdyEngine] VRAM pressure: {}", budget.summary());
                }
            }
        }

        let uploads: Vec<asset_loader::PendingUpload> = {
            //panic allowed, reason = "poisoned internal pending upload queue is unrecoverable"
            let mut lock = self
                .pending_uploads
                .lock()
                .expect("pending_uploads mutex poisoned");
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
    /// if let Some(t) = Engine::global().frame_timing() {
    ///     if t.is_jittery() {
    ///         eprintln!("jitter! p99={:.1}ms mean={:.1}ms", t.p99_cpu_ms, t.mean_cpu_ms);
    ///     }
    /// }
    /// ```
    pub fn frame_timing(&self) -> Option<crate::FrameTimingReport> {
        frame_timing_cell()
            .lock()
            .expect("frame timing mutex poisoned")
            .clone()
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

pub struct Image {
    device: core::Device,
    handle: core::ImageHandle,
    desc: ImageDesc,
}

impl Image {
    pub fn handle(&self) -> core::ImageHandle {
        self.handle
    }

    pub fn desc(&self) -> ImageDesc {
        self.desc
    }

    pub fn set_debug_name(&self, name: &str) -> Result<()> {
        self.device.set_image_debug_name(self.handle, name)
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        let _ = self.device.destroy_image(self.handle);
    }
}

pub struct Buffer {
    device: core::Device,
    handle: core::BufferHandle,
    desc: BufferDesc,
}

impl Buffer {
    pub fn handle(&self) -> core::BufferHandle {
        self.handle
    }

    pub fn desc(&self) -> BufferDesc {
        self.desc
    }

    pub fn write(&self, offset: u64, data: &[u8]) -> Result<()> {
        self.device.write_buffer(self.handle, offset, data)
    }

    pub fn read(&self, offset: u64, out: &mut [u8]) -> Result<()> {
        self.device.read_buffer(self.handle, offset, out)
    }

    pub fn device_address(&self) -> Result<Option<u64>> {
        self.device.buffer_device_address(self.handle)
    }

    pub fn set_debug_name(&self, name: &str) -> Result<()> {
        self.device.set_buffer_debug_name(self.handle, name)
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        let _ = self.device.destroy_buffer(self.handle);
    }
}

pub struct Sampler {
    device: core::Device,
    handle: core::SamplerHandle,
    desc: SamplerDesc,
}

impl Sampler {
    pub fn handle(&self) -> core::SamplerHandle {
        self.handle
    }

    pub fn desc(&self) -> SamplerDesc {
        self.desc
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        let _ = self.device.destroy_sampler(self.handle);
    }
}

pub struct Shader {
    device: core::Device,
    handle: core::ShaderHandle,
    desc: ShaderDesc,
}

impl Shader {
    pub fn handle(&self) -> core::ShaderHandle {
        self.handle
    }

    pub fn desc(&self) -> &ShaderDesc {
        &self.desc
    }
}

impl Drop for Shader {
    fn drop(&mut self) {
        let _ = self.device.destroy_shader(self.handle);
    }
}

pub struct BindGroup {
    device: core::Device,
    handle: core::BindGroupHandle,
    desc: BindGroupDesc,
}

impl BindGroup {
    pub fn handle(&self) -> core::BindGroupHandle {
        self.handle
    }

    pub fn desc(&self) -> &BindGroupDesc {
        &self.desc
    }
}

impl Drop for BindGroup {
    fn drop(&mut self) {
        let _ = self.device.destroy_bind_group(self.handle);
    }
}

pub struct PipelineLayout {
    device: core::Device,
    handle: core::PipelineLayoutHandle,
    layout: CanonicalPipelineLayout,
}

impl PipelineLayout {
    pub fn handle(&self) -> core::PipelineLayoutHandle {
        self.handle
    }

    pub fn layout(&self) -> &CanonicalPipelineLayout {
        &self.layout
    }
}

impl Drop for PipelineLayout {
    fn drop(&mut self) {
        let _ = self.device.destroy_pipeline_layout(self.handle);
    }
}

pub struct Pipeline {
    device: core::Device,
    handle: core::PipelineHandle,
}

impl Pipeline {
    pub fn handle(&self) -> core::PipelineHandle {
        self.handle
    }

    pub fn set_debug_name(&self, name: &str) -> Result<()> {
        self.device.set_pipeline_debug_name(self.handle, name)
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        let _ = self.device.destroy_pipeline(self.handle);
    }
}

pub struct Surface {
    device: core::Device,
    handle: core::SurfaceHandle,
    info: SurfaceInfo,
}

impl Surface {
    pub fn handle(&self) -> core::SurfaceHandle {
        self.handle
    }

    pub fn size(&self) -> SurfaceSize {
        self.info.size
    }

    pub fn info(&self) -> SurfaceInfo {
        self.info
    }

    pub fn resize(&mut self, size: SurfaceSize) -> Result<()> {
        self.device.resize_surface(self.handle, size)?;
        self.info = self.device.surface_info(self.handle)?;
        Ok(())
    }

    pub fn recreate(&mut self, desc: SurfaceRecreateDesc) -> Result<()> {
        self.device.recreate_surface(self.handle, desc)?;
        self.info = self.device.surface_info(self.handle)?;
        Ok(())
    }

    pub fn drain_events(&mut self) -> Result<Vec<SurfaceEvent>> {
        let events = self.device.drain_surface_events(self.handle)?;
        self.info = self.device.surface_info(self.handle)?;
        Ok(events)
    }

    pub fn hdr_caps(&self) -> Result<SurfaceHdrCaps> {
        self.device.surface_hdr_caps(self.handle)
    }

    pub fn capabilities(&self) -> Result<SurfaceCapabilities> {
        self.device.query_surface_capabilities(self.handle)
    }

    pub fn acquire_image(&self) -> Result<SurfaceImage> {
        let (handle, slot) = self.device.acquire_surface_image(self.handle)?;
        let desc = self.device.image_desc(handle)?;
        Ok(SurfaceImage {
            device: self.device.clone(),
            handle,
            desc,
            slot,
        })
    }

    pub fn present(&self) -> Result<()> {
        self.device.present_surface(self.handle)
    }

    pub(crate) fn auto_present_info(&self) -> (core::Device, core::SurfaceHandle) {
        (self.device.clone(), self.handle)
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        let _ = self.device.destroy_surface(self.handle);
    }
}

pub struct SurfaceImage {
    device: core::Device,
    handle: core::ImageHandle,
    desc: ImageDesc,
    /// Stable swapchain image index (0..swapchain_image_count).
    slot: u64,
}

impl SurfaceImage {
    pub fn handle(&self) -> core::ImageHandle {
        self.handle
    }

    pub fn desc(&self) -> ImageDesc {
        self.desc
    }
}

impl Drop for SurfaceImage {
    fn drop(&mut self) {
        let _ = self.device.destroy_image(self.handle);
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FrameSyncReason {
    FrameBoundaryPresent,
    ReadbackCompletion,
    CompatibilityShim,
    ExplicitUserRequest,
    Shutdown,
    DeviceLossRecovery,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameSyncReport {
    pub reason: FrameSyncReason,
    pub submitted: bool,
    pub waited: bool,
    pub presented: bool,
    pub submission: Option<SubmissionHandle>,
    pub notes: Vec<String>,
}

impl FrameSyncReport {
    fn submitted(reason: FrameSyncReason, submission: SubmissionHandle) -> Self {
        Self {
            reason,
            submitted: true,
            waited: false,
            presented: false,
            submission: Some(submission),
            notes: vec![
                "flush may wait for the previous frame fence before submitting new work"
                    .to_string(),
            ],
        }
    }

    fn waited(reason: FrameSyncReason, waited: bool, submission: Option<SubmissionHandle>) -> Self {
        Self {
            reason,
            submitted: false,
            waited,
            presented: false,
            submission,
            notes: if waited {
                vec!["wait blocked until the submitted frame completed".to_string()]
            } else {
                vec!["wait skipped because no submission exists for this frame".to_string()]
            },
        }
    }

    fn frame_boundary_present(reason: FrameSyncReason, submission: SubmissionHandle) -> Self {
        Self {
            reason,
            submitted: true,
            waited: true,
            presented: true,
            submission: Some(submission),
            notes: vec![
                "frame-boundary present submitted queued work, waited for completion, then presented"
                    .to_string(),
            ],
        }
    }
}

pub trait ImageRef {
    fn image_handle(&self) -> core::ImageHandle;
    fn image_desc(&self) -> ImageDesc;
}

impl ImageRef for Image {
    fn image_handle(&self) -> core::ImageHandle {
        self.handle
    }
    fn image_desc(&self) -> ImageDesc {
        self.desc
    }
}

impl ImageRef for SurfaceImage {
    fn image_handle(&self) -> core::ImageHandle {
        self.handle
    }
    fn image_desc(&self) -> ImageDesc {
        self.desc
    }
}

pub struct DrawPassBuilder<'f> {
    frame: &'f mut Frame,
    name: String,
    pipeline: Option<core::PipelineHandle>,
    bind_groups: Vec<core::BindGroupHandle>,
    color_writes: Vec<(core::ImageHandle, ImageDesc)>,
    depth_write: Option<(core::ImageHandle, ImageDesc)>,
    image_reads: Vec<(core::ImageHandle, ImageDesc)>,
    extra_buffer_reads: Vec<(core::BufferHandle, BufferDesc)>,
    vertex_buf: Option<(core::BufferHandle, BufferDesc, u32, u64)>,
    index_buf: Option<(core::BufferHandle, BufferDesc, IndexFormat, u64)>,
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
    push_constants: Option<PushConstants>,
    pipeline_shading_rate: Option<ShadingRate>,
    /// Clear color per image handle (stored as f32 bit-patterns).
    clear_colors: Vec<(core::ImageHandle, [u32; 4])>,
    clear_depth: Option<(core::ImageHandle, u32, u8)>,
}

impl<'f> DrawPassBuilder<'f> {
    pub fn color(mut self, image: &impl ImageRef) -> Self {
        self.color_writes
            .push((image.image_handle(), image.image_desc()));
        self
    }

    /// Clear the last added color attachment to `rgba` before this pass executes.
    /// Must be called after the `.color()` call for the image to clear.
    pub fn clear_color(mut self, rgba: [f32; 4]) -> Self {
        if let Some((handle, _)) = self.color_writes.last() {
            let bits = rgba.map(f32::to_bits);
            self.clear_colors.push((*handle, bits));
        }
        self
    }

    /// Clear the depth attachment to `depth` (and `stencil`) before this pass executes.
    pub fn clear_depth(mut self, depth: f32, stencil: u8) -> Self {
        if let Some((handle, _)) = self.depth_write {
            self.clear_depth = Some((handle, depth.to_bits(), stencil));
        }
        self
    }

    pub fn depth(mut self, image: &impl ImageRef) -> Self {
        self.depth_write = Some((image.image_handle(), image.image_desc()));
        self
    }

    pub fn sample(mut self, image: &impl ImageRef) -> Self {
        self.image_reads
            .push((image.image_handle(), image.image_desc()));
        self
    }

    pub fn pipeline(mut self, pipeline: &Pipeline) -> Self {
        self.pipeline = Some(pipeline.handle());
        self
    }

    pub fn bind(mut self, bind_group: &BindGroup) -> Self {
        self.bind_groups.push(bind_group.handle());
        self
    }

    pub fn push_constants(mut self, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset: 0,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn push_constants_at(mut self, offset: u32, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn pipeline_shading_rate(mut self, rate: ShadingRate) -> Self {
        self.pipeline_shading_rate = Some(rate);
        self
    }

    pub fn vertex_buffer(mut self, buffer: &Buffer, binding: u32, offset: u64) -> Self {
        self.vertex_buf = Some((buffer.handle(), buffer.desc(), binding, offset));
        self
    }

    pub fn index_buffer(mut self, buffer: &Buffer, format: IndexFormat, offset: u64) -> Self {
        self.index_buf = Some((buffer.handle(), buffer.desc(), format, offset));
        self
    }

    pub fn draw(mut self, vertex_count: u32) -> Self {
        self.vertex_count = vertex_count;
        self
    }

    pub fn draw_instanced(mut self, vertex_count: u32, instance_count: u32) -> Self {
        self.vertex_count = vertex_count;
        self.instance_count = instance_count;
        self
    }

    pub fn submit(self) -> Result<()> {
        let Self {
            frame,
            name,
            pipeline,
            bind_groups,
            color_writes,
            depth_write,
            image_reads,
            extra_buffer_reads,
            vertex_buf,
            index_buf,
            vertex_count,
            instance_count,
            first_vertex,
            first_instance,
            push_constants,
            pipeline_shading_rate,
            clear_colors,
            clear_depth,
        } = self;

        for (handle, desc) in &color_writes {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        if let Some((handle, desc)) = &depth_write {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        for (handle, desc) in &image_reads {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        if let Some((handle, desc, _, _)) = &vertex_buf {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }
        if let Some((handle, desc, _, _)) = &index_buf {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }
        for (handle, desc) in &extra_buffer_reads {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }

        let subresource = SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };

        let writes: Vec<ImageUse> = color_writes
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource,
            })
            .chain(depth_write.iter().map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Write,
                state: RgState::DepthWrite,
                subresource,
            }))
            .collect();

        let reads: Vec<ImageUse> = image_reads
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            })
            .collect();

        let mut buffer_reads: Vec<BufferUse> = Vec::new();
        if let Some((handle, desc, _, _)) = &vertex_buf {
            buffer_reads.push(BufferUse {
                buffer: *handle,
                access: Access::Read,
                state: RgState::VertexRead,
                offset: 0,
                size: desc.size,
            });
        }
        if let Some((handle, desc, _, _)) = &index_buf {
            buffer_reads.push(BufferUse {
                buffer: *handle,
                access: Access::Read,
                state: RgState::IndexRead,
                offset: 0,
                size: desc.size,
            });
        }
        for (handle, desc) in &extra_buffer_reads {
            buffer_reads.push(BufferUse {
                buffer: *handle,
                access: Access::Read,
                state: RgState::ShaderRead,
                offset: 0,
                size: desc.size,
            });
        }

        let vertex_buffer = vertex_buf.map(|(handle, _, binding, offset)| VertexBufferBinding {
            buffer: handle,
            binding,
            offset,
        });
        let index_buffer = index_buf.map(|(handle, _, format, offset)| IndexBufferBinding {
            buffer: handle,
            offset,
            format,
        });

        frame.add_pass(PassDesc {
            name,
            queue: QueueType::Graphics,
            shader: None,
            pipeline,
            bind_groups,
            push_constants,
            pipeline_shading_rate,
            work: PassWork::Draw(DrawDesc {
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
                vertex_buffer,
                index_buffer,
                viewport: None,
            }),
            reads,
            writes,
            buffer_reads,
            buffer_writes: Vec::new(),
            clear_colors,
            clear_depth,
            push_descriptor_set: None,
            predicate: None,
        })
    }
}

pub struct ComputePassBuilder<'f> {
    frame: &'f mut Frame,
    name: String,
    pipeline: Option<core::PipelineHandle>,
    bind_groups: Vec<core::BindGroupHandle>,
    push_constants: Option<PushConstants>,
    image_reads: Vec<(core::ImageHandle, ImageDesc)>,
    image_writes: Vec<(core::ImageHandle, ImageDesc)>,
    buffer_reads: Vec<(core::BufferHandle, BufferDesc)>,
    buffer_writes: Vec<(core::BufferHandle, BufferDesc)>,
    dispatch: Option<DispatchDesc>,
}

impl<'f> ComputePassBuilder<'f> {
    pub fn read_image(mut self, image: &impl ImageRef) -> Self {
        self.image_reads
            .push((image.image_handle(), image.image_desc()));
        self
    }

    pub fn write_image(mut self, image: &impl ImageRef) -> Self {
        self.image_writes
            .push((image.image_handle(), image.image_desc()));
        self
    }

    pub fn read_buffer(mut self, buffer: &Buffer) -> Self {
        self.buffer_reads.push((buffer.handle(), buffer.desc()));
        self
    }

    pub fn write_buffer(mut self, buffer: &Buffer) -> Self {
        self.buffer_writes.push((buffer.handle(), buffer.desc()));
        self
    }

    pub fn pipeline(mut self, pipeline: &Pipeline) -> Self {
        self.pipeline = Some(pipeline.handle());
        self
    }

    pub fn bind(mut self, bind_group: &BindGroup) -> Self {
        self.bind_groups.push(bind_group.handle());
        self
    }

    pub fn push_constants(mut self, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset: 0,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn push_constants_at(mut self, offset: u32, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn dispatch(mut self, x: u32, y: u32, z: u32) -> Self {
        self.dispatch = Some(DispatchDesc { x, y, z });
        self
    }

    pub fn submit(self) -> Result<()> {
        let Self {
            frame,
            name,
            pipeline,
            bind_groups,
            push_constants,
            image_reads,
            image_writes,
            buffer_reads,
            buffer_writes,
            dispatch,
        } = self;

        let dispatch = dispatch.ok_or_else(|| {
            Error::InvalidInput("compute pass requires a dispatch call before submit".into())
        })?;

        for (handle, desc) in image_reads.iter().chain(image_writes.iter()) {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        for (handle, desc) in buffer_reads.iter().chain(buffer_writes.iter()) {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }

        let subresource = SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };

        let reads: Vec<ImageUse> = image_reads
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            })
            .collect();

        let writes: Vec<ImageUse> = image_writes
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Write,
                state: RgState::ShaderWrite,
                subresource,
            })
            .collect();

        let buf_reads: Vec<BufferUse> = buffer_reads
            .iter()
            .map(|(h, desc)| BufferUse {
                buffer: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                offset: 0,
                size: desc.size,
            })
            .collect();

        let buf_writes: Vec<BufferUse> = buffer_writes
            .iter()
            .map(|(h, desc)| BufferUse {
                buffer: *h,
                access: Access::Write,
                state: RgState::ShaderWrite,
                offset: 0,
                size: desc.size,
            })
            .collect();

        frame.add_pass(PassDesc {
            name,
            queue: QueueType::Compute,
            shader: None,
            pipeline,
            bind_groups,
            push_constants,
            pipeline_shading_rate: None,
            work: PassWork::Dispatch(dispatch),
            reads,
            writes,
            buffer_reads: buf_reads,
            buffer_writes: buf_writes,
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
        })
    }
}

pub struct Frame {
    pub(crate) engine: Engine,
    pub(crate) inner: core::Frame,
    pub(crate) upload_arena: UploadArena,
}

impl Frame {
    pub fn import_image(&mut self, image: &Image) -> Result<()> {
        self.inner
            .graph_mut(|graph| graph.import_image(image.handle(), image.desc()))
    }

    pub fn import_surface_image(&mut self, image: &SurfaceImage) -> Result<()> {
        self.inner
            .graph_mut(|graph| graph.import_image(image.handle(), image.desc()))
    }

    pub fn import_buffer(&mut self, buffer: &Buffer) -> Result<()> {
        self.inner
            .graph_mut(|graph| graph.import_buffer(buffer.handle(), buffer.desc()))
    }

    pub fn add_pass(&mut self, pass: PassDesc) -> Result<()> {
        self.inner.graph_mut(|graph| graph.add_pass(pass))
    }

    /// Generate a full mip chain for `image` using linear-filtered blits.
    ///
    /// The image must have been created with `mip_levels > 1` and
    /// `ImageUsage::COPY_SRC | ImageUsage::COPY_DST`. Call after any write to
    /// mip 0 that you want propagated to all deeper mips.
    ///
    /// The pass is recorded immediately into the current frame; the GPU executes
    /// it when `flush()` is called.
    ///
    /// # Example
    /// ```ignore
    /// let mut tex = engine.create_image(ImageDesc {
    ///     mip_levels: 8,
    ///     usage: ImageUsage::SAMPLED | ImageUsage::COPY_SRC | ImageUsage::COPY_DST | ImageUsage::RENDER_TARGET,
    ///     ..ImageDesc::d2(512, 512, Format::Rgba8Unorm)
    /// })?;
    /// // ... upload data into mip 0 ...
    /// frame.generate_mipmaps(&tex)?;
    /// ```
    pub fn generate_mipmaps(&mut self, image: &Image) -> Result<()> {
        self.import_image(image)?;
        self.add_pass(PassDesc {
            name: format!(
                "generate_mipmaps({})",
                image.desc().debug_name.unwrap_or("image")
            ),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::GenerateMipmaps {
                image: image.handle(),
                mip_count: image.desc().mip_levels as u32,
            },
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
        })
    }

    pub fn debug_marker(&mut self, name: impl Into<String>) -> Result<()> {
        self.add_pass(PassDesc {
            name: name.into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
        })
    }

    pub fn draw_pass(&mut self, name: impl Into<String>) -> DrawPassBuilder<'_> {
        DrawPassBuilder {
            frame: self,
            name: name.into(),
            pipeline: None,
            bind_groups: Vec::new(),
            color_writes: Vec::new(),
            depth_write: None,
            image_reads: Vec::new(),
            extra_buffer_reads: Vec::new(),
            vertex_buf: None,
            index_buf: None,
            vertex_count: 0,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
            push_constants: None,
            pipeline_shading_rate: None,
            clear_colors: Vec::new(),
            clear_depth: None,
        }
    }

    pub fn compute_pass(&mut self, name: impl Into<String>) -> ComputePassBuilder<'_> {
        ComputePassBuilder {
            frame: self,
            name: name.into(),
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            image_reads: Vec::new(),
            image_writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            dispatch: None,
        }
    }

    pub fn present_image(&mut self, image: &impl ImageRef) -> Result<()> {
        self.inner
            .graph_mut(|g| g.import_image(image.image_handle(), image.image_desc()))?;
        self.add_pass(PassDesc {
            name: "present".to_owned(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: vec![ImageUse {
                image: image.image_handle(),
                access: Access::Read,
                state: RgState::Present,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
        })
    }

    pub fn flush(&mut self) -> Result<SubmissionHandle> {
        self.inner.flush()
    }

    /// Explicitly submit queued frame work and report why synchronization is allowed here.
    pub fn flush_with_reason(&mut self, reason: FrameSyncReason) -> Result<FrameSyncReport> {
        let submission = self.flush()?;
        Ok(FrameSyncReport::submitted(reason, submission))
    }

    pub fn present(&mut self) -> Result<()> {
        self.inner.present()
    }

    pub fn wait(&self) -> Result<()> {
        self.inner.wait()
    }

    /// Explicitly wait for this frame's submission and report why blocking is allowed here.
    pub fn wait_with_reason(&self, reason: FrameSyncReason) -> Result<FrameSyncReport> {
        let submission = self.inner.last_submission();
        self.wait()?;
        Ok(FrameSyncReport::waited(
            reason,
            submission.is_some(),
            submission,
        ))
    }

    pub(crate) fn last_submission(&self) -> Option<SubmissionHandle> {
        self.inner.last_submission()
    }

    /// Finish rendering this frame and present to the given surface in a single call.
    ///
    /// This is a convenience method that calls `flush()`, `wait()`, and
    /// `surface.present()` in sequence, returning the first error if any step fails.
    ///
    /// It is the replacement for the common three-call pattern:
    /// ```ignore
    /// frame.flush()?;
    /// frame.wait()?;
    /// self.surface.present()?;
    /// ```
    ///
    /// **Note**: The caller must have already called [`present_image`](Self::present_image)
    /// with the surface image that will be presented.
    pub fn finish_and_present(&mut self, surface: &Surface) -> Result<()> {
        self.finish_and_present_with_reason(surface, FrameSyncReason::FrameBoundaryPresent)?;
        Ok(())
    }

    pub fn finish_and_present_with_reason(
        &mut self,
        surface: &Surface,
        reason: FrameSyncReason,
    ) -> Result<FrameSyncReport> {
        let submission = self.flush()?;
        self.wait()?;
        surface.present()?;
        Ok(FrameSyncReport::frame_boundary_present(reason, submission))
    }
}
