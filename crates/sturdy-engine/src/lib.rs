//! Ergonomic Rust API for Sturdy Engine.
//!
//! Use this crate from Rust applications. It wraps the core handle-oriented API
//! with RAII resource types and builder-style descriptors while keeping the
//! lower-level `sturdy-engine-core` crate available for engine internals.
#![allow(dead_code)]

pub mod animation;
mod anti_aliasing_pass;
mod antialiasing;
mod camera_motion_vector_pass;
mod ao_pass;
mod application;
mod asset_loader;
mod asset_watcher;
mod auto_exposure;
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
mod engine;
mod engine_global;
mod environment_map;
mod frame;
mod frame_clock;
mod frame_sync;
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
mod material_table;
mod mesh;
mod material_registry;
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
mod realtime_gi;
mod realtime_raytracing;
pub mod render_world;
mod renderer_metrics;
mod resource;
mod resource_table;
mod runtime;
mod sampler_catalog;
mod scene;
mod screenshot;
mod shader_data;
mod shader_playground;
pub mod shader_program;
mod shader_watcher;
mod shadow_pass;
pub(crate) mod shadow_pipeline;
mod spot_shadow_pass;
mod sprite_batch;
mod srd_denoiser;
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
mod virtualized_geometry;
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
    RuntimePostProcessDesc, RuntimePostProcessOutput, ShellFrame, WindowConfig, WindowDesc,
    init_tracing_with_default_filter, run, run_with_runtime, set_log_level, try_run,
    try_run_with_runtime,
};
pub use asset_loader::{AssetCache, AssetHandle, LoadState};
pub use asset_watcher::{AssetReloadDiagnostic, AssetWatcher};
pub use auto_exposure::{AutoExposurePass, AutoExposureReadback};
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
pub use camera_motion_vector_pass::CameraMotionVectorPass;
pub use deferred_pass::{DeferredOutput, DeferredPass, RenderPath, SkyConfig};
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
pub use engine::Engine;
pub use environment_map::EnvironmentMap;
pub use frame::{ComputePassBuilder, DrawPassBuilder, Frame, ImageRef};
pub use frame_clock::{FrameClock, FrameTime};
pub use frame_sync::{FrameSyncReason, FrameSyncReport};
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
    LateSample,
};
pub use light_bvh::{BVH_EMPTY, GpuBvhNode, LEAF_FLAG, LightBvhBuilder};
pub use material_registry::{
    GpuMaterialEntry, MATERIAL_NO_TEXTURE, MATERIAL_REGISTRY_CAPACITY, MaterialEntry,
    MaterialRegistry,
};
pub use material_table::{
    MaterialTableCaps, MaterialTableDirtyRange, MaterialTablePlan, MaterialTableSettings,
    material_table_dirty_ranges,
};
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
pub use realtime_gi::{
    RealtimeGiCaps, RealtimeGiPath, RealtimeGiPlan, RealtimeGiRequest, RealtimeGiSettings,
    RealtimeGiSurfaceCachePlan, RealtimeGiSurfaceCacheSettings,
};
pub use realtime_raytracing::{
    RealtimeBlas, RealtimeRayTracingPipeline, RealtimeRayTracingShaderDesc,
    RealtimeRayTracingSupport, RealtimeTlas, RealtimeTlasInstance,
};
pub use render_world::{
    Aabb, GpuObjectAllocator, GpuObjectId, GpuTransformDirtyRange, GpuTransformSourceData,
    LayerMask, LocalToWorld, LodGroupId, MaterialId, MaterialShaderClass, PipelineClass,
    PreviousTransform, RenderBounds, RenderDirtyFlags, RenderExtractionStats, RenderMaterial,
    RenderMesh, RenderObjectState, RenderStateClass, RenderVisibility, RenderWorld,
    RenderWorldBatchRange, RenderWorldBinKey, RenderWorldCommand, RenderWorldCommands,
    RenderWorldGpuBinData, RenderWorldGpuCullCaps, RenderWorldGpuCullDispatchStats,
    RenderWorldGpuCullOutputStats, RenderWorldGpuCullPass, RenderWorldGpuCullPlan,
    RenderWorldGpuCullSettings, RenderWorldGpuDrawGenerationDispatchStats,
    RenderWorldGpuDrawGenerationPass, RenderWorldGpuDrawGenerationPlan,
    RenderWorldGpuDrawGenerationStats, RenderWorldGpuDrawOutput, RenderWorldGpuMatrixCaps,
    RenderWorldGpuMatrixPlan, RenderWorldGpuMatrixSettings, RenderWorldGpuMatrixStats,
    RenderWorldGpuMeshDrawInfo, RenderWorldGpuSceneData, RenderWorldGpuSceneStats,
    RenderWorldGpuTransformBuildPass, RenderWorldGpuTransformBuildStats,
    RenderWorldGpuTransformSourceData, RenderWorldPersistentBin, RenderWorldPersistentBinPlan,
    RenderWorldPersistentBins, VertexLayoutClass, VisibilityFlags, gpu_transform_dirty_ranges,
};
pub use renderer_metrics::{
    RendererWorkloadEvaluation, RendererWorkloadMetrics, RendererWorkloadMetricsBuilder,
    RendererWorkloadTargets,
};
pub use resource::{
    AccelerationStructure, BindGroup, Buffer, Image, Pipeline, PipelineLayout,
    RayTracingShaderBindingTable, Sampler, Shader, ShaderObject, Surface, SurfaceImage,
};
pub use resource_table::{
    SceneResourceDirtyRange, SceneResourceId, SceneResourceTableCaps, SceneResourceTableKind,
    SceneResourceTablePlan, SceneResourceTableSettings, scene_resource_dirty_ranges,
};
pub use runtime::{
    AppLayer, AppRuntime, AppRuntimeFrame, AssetDiagnostic, AssetState, AutoExposureDiagnostics,
    BackendFeatureChange, BackendRestartOutcome, BenchmarkFrameSample, BenchmarkPassSample,
    BenchmarkReport, DebugImageRegistry, DefaultSceneTargetConfig, FrameStats, FrameTimingReport,
    RuntimeApp, RuntimeApplyNotification,
    RuntimeApplyPath, RuntimeApplyReport, RuntimeChangeResult, RuntimeController,
    RuntimeDiagnostics, RuntimeFixedUpdateContext, RuntimeGraphDiagnostics, RuntimePassTiming,
    RuntimeSettingChange, RuntimeSettingDescriptor, RuntimeSettingEntry, RuntimeSettingId,
    RuntimeSettingKey, RuntimeSettingOption, RuntimeSettingSource, RuntimeSettingSupport,
    RuntimeSettingValue, RuntimeSettingsSnapshot, RuntimeSettingsTransaction, RuntimeTimingSummary,
    RuntimeUserDiagnostic, RuntimeWindowDiagnostics, RuntimeWorkloadDiagnostics, SceneRenderContext,
    ShaderCompileError, UiContext, WindowMode,
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
pub use shadow_pipeline::ShadowPipeline;
pub use spot_shadow_pass::{GpuSpotShadowData, MAX_SPOT_SHADOWS, SpotShadowConfig, SpotShadowPass};
pub use sprite_batch::{Sprite, SpriteBatch, SpriteRenderer};
#[allow(deprecated)]
pub use srd_denoiser::{
    RealtimeRayTracingDenoiser, SRD_CLEAR_HISTORY_WORKGROUP_SIZE,
    SRD_RADIANCE_SURFACE_MASK_TILE_SIZE, SRD_TEMPORAL_CONSTANTS_SIZE, SrdAtrousSettings,
    SrdCapabilities, SrdClearConstants, SrdCommonSettings, SrdConstantArena, SrdConstantRange,
    SrdDenoiser, SrdDenoiserDesc, SrdDenoiserId, SrdDenoiserMode, SrdDenoiserSettings,
    SrdDepthConvention, SrdDescriptorType, SrdDispatchDesc, SrdFamilySettings,
    SrdHistoryClampSettings, SrdHistoryMode, SrdHistoryRejectionSettings, SrdHistoryRing,
    SrdHitDistanceSettings, SrdInstance, SrdInstanceDesc, SrdMotionVectorConvention,
    SrdNormalPacking, SrdOcclusionPlan, SrdOcclusionSettings, SrdOcclusionStabilizerExecutor,
    SrdOcclusionStabilizerInputs, SrdOcclusionStabilizerPrograms, SrdOutlierClampSettings,
    SrdPassBuilder, SrdPipelineDesc, SrdPoolClass, SrdPostBlurSettings,
    SrdRadianceAccumulateConstants, SrdRadianceAccumulateResources, SrdRadianceAtrousConstants,
    SrdRadianceClampConstants, SrdRadianceCombinedPlan, SrdRadianceDiffuseSpecularPlan,
    SrdRadianceOutlierSuppressConstants, SrdRadianceOutlierSuppressResources,
    SrdRadianceOutputResource, SrdRadiancePostBlurConstants, SrdRadianceReconstructConstants,
    SrdRadianceReconstructResources, SrdRadianceReprojectConstants, SrdRadianceReprojectResources,
    SrdRadianceSettings, SrdRadianceSpatialFilterConstants, SrdRadianceStabilizerExecutor,
    SrdRadianceStabilizerInputs, SrdRadianceStabilizerPlan, SrdRadianceStabilizerPrograms,
    SrdRadianceStabilizerResources, SrdRadianceSurfaceMaskConstants, SrdRadianceSurfaceMaskResources,
    SrdReferenceSettings, SrdReferenceTemporalComputeExecutor, SrdReferenceTemporalComputePrograms,
    SrdReferenceTemporalExecutor, SrdReferenceTemporalPipelines, SrdReferenceTemporalPrograms,
    SrdResourceDesc, SrdResourceFormatDesc, SrdResourceSlot, SrdShaderContract,
    SrdShadowPlan, SrdShadowSettings, SrdShadowStabilizerExecutor, SrdShadowStabilizerInputs,
    SrdShadowStabilizerPrograms, SrdSignalMomentsConstants, SrdSpatialFilterSettings,
    SrdSpectralLayout, SrdSpectralRadiancePlan, SrdSpectralRadianceSettings,
    SrdSpectralRadianceStabilizerExecutor, SrdSpectralRadianceStabilizerInputs,
    SrdSpectralRadianceStabilizerPrograms, SrdTemporalBindings, SrdTemporalConstants,
    SrdTextureDesc, SrdTranslucentShadowPlan, SrdTranslucentShadowSettings,
    SrdTranslucentShadowStabilizerExecutor, SrdTranslucentShadowStabilizerInputs,
    SrdTranslucentShadowStabilizerPrograms, SrdVarianceSettings,
};
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
    MultiMeshDrawBinItem, RenderFrame, ShaderPassIntent, UniformBinding,
};
pub use glam::{Vec2, Vec3};
pub use graph_report::{
    DiagnosticLevel, GraphDiagnostic, GraphImageInfo, GraphPassInfo, GraphReport, PassKind,
};
pub use pipeline_layout::PipelineLayoutBuilder;
pub use post_process::{
    AutoExposureConfig, CaConfig, CaPass, GrainConfig, GrainPass, LensConfig, LensPass,
    PostProcessConfig, PostProcessPasses, VignetteConfig, VignettePass,
};
pub use shader_data::ShaderData;
pub use shader_program::{ShaderName, ShaderProgram, ShaderProgramDesc, SlangEntryPoints};
#[cfg(not(target_arch = "wasm32"))]
pub use sturdy_engine_core::NativeSurfaceDesc;
pub use sturdy_engine_core::ShaderReflection;
pub use sturdy_engine_core::{
    AccelerationStructureBuildMode, AccelerationStructureBuildSizes, AccelerationStructureDesc,
    AccelerationStructureKind, Access, AdapterInfo, AdapterKind, AdapterSelection, AddressMode,
    BackendKind, BackendRawCapabilities, BindGroupDesc, BindGroupEntry, BindingKind, BlasBuildDesc,
    BlasGeometryDesc, BlendMode, BorderColor, BufferDesc, BufferUsage, BufferUse, CanonicalBinding,
    CanonicalGroupLayout, CanonicalPipelineLayout, Caps, ColorTargetDesc, CompareOp,
    CompiledShaderArtifact, ComputePipelineDesc, CopyBufferToImageDesc, CopyImageToBufferDesc,
    CullMode, D3d12RawCapabilities, DispatchDesc, DispatchIndirectDesc, DrawDesc,
    DrawIndirectCountDesc, DrawIndirectDesc, DrawMeshShaderDesc, DrawMeshShaderIndirectDesc, Error,
    MultiMeshIndirectDrawDesc, MultiMeshIndirectDrawItem,
    ErrorCategory, Extent3d, ExternalBufferDesc, ExternalBufferHandle, ExternalImageDesc,
    ExternalImageHandle, FilterMode, Format, FormatCapabilities, FrontFace, GpuCaptureDesc,
    GpuCaptureTool, GpuMemoryBudget, GpuTimeline, GraphicsPipelineDesc, HdrMetadata, ImageBuilder,
    ImageCompression, ImageDesc, ImageDimension, ImageRole, ImageUsage, ImageUse,
    IndexBufferBinding, IndexFormat, MemoryBudgetReport, MemoryHeapBudget, MetalRawCapabilities,
    MipmapMode, NativeHandleCapabilities, NativeHandleCapability, NativeHandleKind,
    NativeHandleOwnership, PassDesc, PassTimingReport, PassWork, PerfCounterHandle, PolygonMode,
    PrimitiveTopology, PushConstants, PushDescriptorBinding, PushDescriptorSetDesc,
    QueueType, RasterState, RayTracingPipelineDesc, RayTracingStageDesc, ResolveImageDesc,
    ResourceBinding, Result, RgState, RtShaderGroupDesc, RtShaderGroupKind, SamplerDesc,
    ShaderBindingTableDesc, ShaderDesc, ShaderParameterKind, ShaderParameterReflection,
    ShaderResourceAccess, ShaderSource, ShaderStage, ShaderTarget, ShadingRate, SlangCompileDesc,
    StageMask, SubresourceRange, SurfaceCapabilities, SurfaceColorSpace, SurfaceEvent,
    SurfaceFormatInfo, SurfaceHdrCaps, SurfaceHdrPreference, SurfaceInfo, SurfacePresentMode,
    SurfaceRecreateDesc, TlasBuildDesc, TraceRaysDesc, UpdateRate, VertexAttributeDesc,
    VertexBufferBinding, VertexBufferLayout, VertexFormat, VertexInputRate, VertexInputReflection,
    VulkanExternalBuffer, VulkanExternalImage, VulkanRawCapabilities, compile_slang,
    compile_slang_to_file, compile_slang_to_spirv, native_handle_capabilities_for_backend,
    spirv_words_from_bytes,
};
pub use sturdy_engine_core::{
    AccelerationStructureHandle, BackendFeature, DeviceDesc, DeviceFeature, ImageHandle,
    SamplerHandle, SubmissionHandle, SurfaceHandle, SurfaceSize,
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
pub use virtualized_geometry::{
    DenseGeometryPlan, DenseGeometryResidencyPlan, DenseGeometryResidencySettings,
    DenseGeometrySettings, VirtualGeometryLodParams,
};
pub use window_registry::{WindowHandle, WindowId, WindowRegistry};
