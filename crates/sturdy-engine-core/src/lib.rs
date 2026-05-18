//! Core engine crate.
//!
//! This crate owns the renderer's backend-neutral model: devices, capabilities,
//! opaque handles, images, shaders, frame graphs, and submission semantics. It
//! intentionally has no C ABI concerns and no high-level ergonomic wrappers.
#![allow(dead_code)]

pub mod acceleration_structure;
pub mod adapter_info;
pub mod adapter_kind;
pub mod adapter_selection;
pub mod backend;
pub mod backend_features;
pub mod binding;
pub mod buffer;
pub mod caps;
pub mod device;
pub mod error;
pub mod external_resource;
pub mod gpu_capture;
pub mod handles;
pub mod image;
pub mod indirect_commands;
pub mod latency;
pub mod limits;
pub mod memory_budget;
pub mod native_handles;
pub mod optical_flow;
pub mod pipeline;
pub mod push_constants;
pub mod raw_capabilities;
pub mod ray_tracing_pipeline;
pub mod render_graph;
pub mod sampler;
pub mod shader;
pub mod shader_object;
pub mod slang;
pub mod surface;
pub mod video;
pub mod vulkan_version;

pub use acceleration_structure::{
    AccelerationStructureBuildSizes, AccelerationStructureDesc, AccelerationStructureKind,
};
pub use adapter_info::AdapterInfo;
pub use adapter_kind::AdapterKind;
pub use adapter_selection::AdapterSelection;
pub use backend::{Backend, BackendKind, auto_backend_preference_order, available_backend_kinds};
pub use backend_features::BackendFeatures;
pub use binding::{
    BINDLESS_COUNT, BindGroupDesc, BindGroupEntry, BindingKind, CanonicalBinding,
    CanonicalGroupLayout, CanonicalPipelineLayout, ResourceBinding, StageMask, UpdateRate,
};
pub use buffer::{BufferDesc, BufferUsage};
pub use caps::{AmdShaderStageStats, Caps, CoopMatrixElementType, CoopMatrixProperty, CoopMatrixScope, ExecutableStat, PerfCounter, PerfCounterCategory};
pub use device::{Device, DeviceDesc, DeviceFeature, Frame, enumerate_adapters};
pub use error::{Error, ErrorCategory, Result};
pub use external_resource::{
    ExternalBufferDesc, ExternalBufferHandle, ExternalImageDesc, ExternalImageHandle,
    VulkanExternalBuffer, VulkanExternalImage,
};
pub use gpu_capture::{GpuCaptureDesc, GpuCaptureTool};
pub use handles::{
    AccelerationStructureHandle, BindGroupHandle, BufferHandle, DeviceHandle, FrameHandle,
    ImageHandle, IndirectCommandLayoutHandle, OpticalFlowSessionHandle, PassHandle, PipelineHandle,
    PipelineLayoutHandle, SamplerHandle, ShaderHandle, SubmissionHandle, SurfaceHandle,
    VideoSessionHandle, SemaphoreHandle,
};
pub use image::{
    Extent3d, Format, FormatCapabilities, ImageBuilder, ImageClearValue, ImageCompression,
    ImageDesc, ImageDimension, ImageRole, ImageUsage,
};
pub use indirect_commands::{
    DgcExecuteDesc, DgcPreprocessDesc, IndirectCommandLayoutDesc, IndirectCommandToken,
};
pub use latency::{AntiLagMode, LatencyMode, ReflexMode};
pub use limits::Limits;
pub use memory_budget::{GpuMemoryBudget, MemoryBudgetReport, MemoryHeapBudget};
pub use native_handles::{
    NativeHandleCapabilities, NativeHandleCapability, NativeHandleKind, NativeHandleOwnership,
    native_handle_capabilities_for_backend,
};
pub use optical_flow::{OpticalFlowEstimateDesc, OpticalFlowSessionDesc};
pub use pipeline::{
    BlendMode, ColorTargetDesc, ComputePipelineDesc, ConservativeRasterMode, CullMode, FrontFace,
    GraphicsPipelineDesc, PolygonMode, PrimitiveTopology, RasterState, VertexAttributeDesc,
    VertexBufferLayout, VertexFormat, VertexInputRate,
};
pub use push_constants::PushConstants;
pub use raw_capabilities::{
    BackendRawCapabilities, D3d12RawCapabilities, MetalRawCapabilities, VulkanRawCapabilities,
};
pub use ray_tracing_pipeline::{
    RayTracingPipelineDesc, RayTracingStageDesc, RtShaderGroupDesc, RtShaderGroupKind,
    ShaderBindingTableDesc, ShaderBindingTableProperties,
};
pub use render_graph::{
    AccelerationStructureBuildMode, Access, AliasPlan, Barrier, BlasBuildDesc, BlasGeometryDesc,
    BufferBarrier, BufferStateKey, BufferUse, CompiledGraph, CopyBufferToImageDesc,
    CopyImageToBufferDesc, DispatchDesc, DispatchIndirectDesc, DrawDesc, DrawIndirectCountDesc,
    DrawIndirectDesc, DrawMeshShaderDesc, DrawMeshShaderIndirectDesc, ImageBarrier, ImageStateKey,
    ImageUse, IndexBufferBinding, IndexFormat, PassDesc, PassWork, PushDescriptorBinding,
    PushDescriptorSetDesc, QueueType, RecordBatch, RenderGraph, ResolveImageDesc, ResourceLifetime,
    ResourceUse, RgState, ShaderBinding, ShaderBindingTable, ShaderBindingTableRegion, ShadingRate,
    SubresourceRange, TlasBuildDesc, TraceRaysDesc, VertexBufferBinding,
};
pub use sampler::{
    AddressMode, BorderColor, CompareOp, FilterMode, MipmapMode, SamplerDesc, SamplerReductionMode,
};
pub use shader::{
    CompiledShaderArtifact, ShaderDesc, ShaderModule, ShaderParameterKind,
    ShaderParameterReflection, ShaderReflection, ShaderResourceAccess, ShaderSource, ShaderStage,
    ShaderTarget, VertexInputReflection,
};
pub use slang::spirv_push_constants::{PcFieldKind, PushConstantField};
pub use slang::{
    SlangCompileDesc, compile_and_reflect, compile_slang, compile_slang_to_file,
    compile_slang_to_spirv, reflect_pipeline_layout, reflect_pipeline_layout_with_caps,
    spirv_words_from_bytes,
};
#[cfg(not(target_arch = "wasm32"))]
pub use surface::NativeSurfaceDesc;
pub use surface::{
    HdrMetadata, SurfaceCapabilities, SurfaceColorSpace, SurfaceEvent, SurfaceFormatInfo,
    SurfaceHdrCaps, SurfaceHdrPreference, SurfaceInfo, SurfacePresentMode, SurfaceRecreateDesc,
    SurfaceSize,
};
pub use video::{
    BitRateControl, DecodeFrameDesc, EncodeFrameDesc, QualityPreset, VideoCodec, VideoEncodeConfig,
    VideoSessionDesc, VideoSessionKind,
};
pub use vulkan_version::VulkanApiVersion;
